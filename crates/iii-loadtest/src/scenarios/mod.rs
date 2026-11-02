// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The load scenarios. Each one connects real SDK workers (and, for
//! `connections`, raw sockets) to one engine and reports a list of steps:
//! what it offered, what came back, and what the engine cost meanwhile.

mod connections;
mod fanout;
mod filters;
mod local;
mod match_keys;
mod observers;
mod overload;
mod rpc;
mod source;

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use iii_helpers::observability::OtelConfig;
use iii_sdk::protocol::CallRequest;
use iii_sdk::runtime::WorkerMetadata;
use iii_sdk::{
    Error, IIIClient, InitOptions, RegisterFunction, WorkerIdentityMode, register_worker,
};

use crate::monitor::{Monitor, Resources};
use crate::stats::{Latency, Load};

pub const ALL: &[&str] = &[
    "rpc",
    "fanout",
    "filters",
    "observers",
    "source",
    "match-keys",
    "local",
    "overload",
    "connections",
];

/// What each scenario says it measures, for `--help` and the report.
pub fn describe(name: &str) -> &'static str {
    match name {
        "rpc" => "worker-to-worker calls through the engine, no observers (the routing baseline)",
        "fanout" => "the same calls with N observers on the called function",
        "filters" => "N observers with selective filters on one function: each call matches one",
        "observers" => "how many observers the engine holds, and what each one costs in memory",
        "source" => {
            "an event source (cron/database-style) calling itself with N observers: floods, then the sustained rate"
        }
        "match-keys" => {
            "an event source matching observers by keys: 1 of up to 10,000 observers per call, then none"
        }
        "local" => "a worker calling its own function in-process, unobserved and observed",
        "overload" => {
            "slow observers behind fast calls: bounded queues, drops, memory per queued activation"
        }
        "connections" => {
            "idle worker connections: memory per worker, connect rate and where it goes"
        }
        _ => "",
    }
}

/// Engine `observers:` settings a scenario needs, as `key=value`, on top of
/// the defaults. A run's own `--observers` setting of the same key wins.
pub fn engine_observers(name: &str) -> &'static [&'static str] {
    match name {
        // 10,000 observers of one function, past the default 4096.
        "match-keys" => &["max_per_function=10240"],
        _ => &[],
    }
}

/// The engine `observers:` settings of a run of `scenario`: what the scenario
/// needs, then the run's own, which replace a scenario setting of the same key.
pub fn observer_settings(scenario: &str, run: &[String]) -> Vec<String> {
    let key = |kv: &str| kv.split_once('=').map_or(kv, |(k, _)| k).trim().to_string();
    let mut out: Vec<String> = engine_observers(scenario)
        .iter()
        .filter(|kv| !run.iter().any(|own| key(own) == key(kv)))
        .map(|kv| kv.to_string())
        .collect();
    out.extend(run.iter().cloned());
    out
}

#[derive(Debug, Clone)]
pub struct Opts {
    /// Measured seconds of a load step.
    pub step: Duration,
    /// Unmeasured load before each step.
    pub warmup: Duration,
    /// Fewer, smaller steps: a smoke run.
    pub quick: bool,
}

/// One measured step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub label: String,
    pub params: Value,
    pub secs: f64,
    /// Operations that succeeded (calls, activations, connections, …: the
    /// scenario says which in `unit`).
    pub ops: u64,
    pub unit: String,
    pub errors: u64,
    pub ops_per_sec: f64,
    pub latency: Option<Latency>,
    pub first_error: Option<String>,
    pub extra: Value,
    pub resources: Resources,
}

/// What a scenario measured inside a step, before the resources are added.
pub struct Measured {
    pub load: Load,
    pub unit: &'static str,
    /// Whether `load.hist` timed the operations (a latency to report).
    pub timed: bool,
    pub extra: Value,
    /// When the load stopped, if the scenario kept going after it (draining
    /// activations, reading counters): rates and resources stop there.
    pub ended: Option<Instant>,
}

impl Measured {
    pub fn timed(load: Load, unit: &'static str) -> Self {
        Self {
            load,
            unit,
            timed: true,
            extra: json!({}),
            ended: None,
        }
    }

    pub fn counted(ops: u64, unit: &'static str) -> Self {
        Self {
            load: Load {
                ops,
                ..Load::default()
            },
            unit,
            timed: false,
            extra: json!({}),
            ended: None,
        }
    }

    pub fn extra(mut self, extra: Value) -> Self {
        self.extra = extra;
        self
    }

    pub fn ended(mut self, at: Instant) -> Self {
        self.ended = Some(at);
        self
    }
}

pub struct Env {
    pub url: String,
    pub opts: Opts,
    pub monitor: Arc<Monitor>,
    /// CPUs the engine may run on.
    pub cpus: usize,
    pub steps: std::sync::Mutex<Vec<Step>>,
}

impl Env {
    /// Runs `f` as one step. It gets the instant the step ends, and should
    /// stop offering load then.
    pub async fn step<F, Fut>(&self, label: impl Into<String>, params: Value, f: F) -> Result<Step>
    where
        F: FnOnce(Instant) -> Fut,
        Fut: Future<Output = Result<Measured>>,
    {
        let (step, ()) = self
            .step_with(label, params, |deadline| async move {
                Ok((f(deadline).await?, ()))
            })
            .await?;
        Ok(step)
    }

    /// [`Env::step`] for a step that also hands something back (the observers
    /// it created, say).
    pub async fn step_with<F, Fut, T>(
        &self,
        label: impl Into<String>,
        params: Value,
        f: F,
    ) -> Result<(Step, T)>
    where
        F: FnOnce(Instant) -> Fut,
        Fut: Future<Output = Result<(Measured, T)>>,
    {
        let label = label.into();
        self.alive()?;
        let started = Instant::now();
        // A step waits on the engine (an observer's `ready()`, a call): when
        // the engine dies under it (OOM-killed at its cap, say), nothing
        // would ever answer, so the step ends with it.
        let (measured, out) = tokio::select! {
            result = f(started + self.opts.step) => result?,
            () = self.monitor.wait_exit() => {
                // Recorded, so a reader knows which step the engine died in.
                let failed = Step {
                    label: label.clone(),
                    params: params.clone(),
                    secs: started.elapsed().as_secs_f64(),
                    ops: 0,
                    unit: "failed".to_string(),
                    errors: 0,
                    ops_per_sec: 0.0,
                    latency: None,
                    first_error: None,
                    extra: json!({}),
                    resources: self.monitor.window(started, Instant::now(), self.cpus),
                };
                self.steps.lock().unwrap().push(failed);
                bail!("the engine exited during the step")
            }
        };
        let ended = measured.ended.unwrap_or_else(Instant::now);
        let secs = ended.duration_since(started).as_secs_f64();
        self.monitor
            .wait_sample_after(ended, Duration::from_secs(1))
            .await;
        let resources = self.monitor.window(started, ended, self.cpus);
        let Measured {
            load,
            unit,
            timed,
            extra,
            ..
        } = measured;
        let step = Step {
            label: label.clone(),
            params,
            secs,
            ops: load.ops,
            unit: unit.to_string(),
            errors: load.errors,
            ops_per_sec: load.ops as f64 / secs,
            latency: if timed { Latency::of(&load.hist) } else { None },
            first_error: load.first_error,
            extra,
            resources,
        };
        eprintln!("    {}", summary(&step));
        self.steps.lock().unwrap().push(step.clone());
        self.alive()?;
        Ok((step, out))
    }

    /// Adds `extra`'s fields to the last step's.
    pub fn annotate_last(&self, extra: Value) {
        let mut steps = self.steps.lock().unwrap();
        if let (Some(step), Value::Object(fields)) = (steps.last_mut(), extra) {
            if !step.extra.is_object() {
                step.extra = json!({});
            }
            for (k, v) in fields {
                step.extra[k] = v;
            }
        }
    }

    /// A step with no load: what the engine holds at rest (`secs` of samples).
    pub async fn idle(
        &self,
        label: impl Into<String>,
        params: Value,
        extra: Value,
    ) -> Result<Step> {
        let secs = if self.opts.quick { 1.0 } else { 3.0 };
        self.step(label, params, |_| async move {
            tokio::time::sleep(Duration::from_secs_f64(secs)).await;
            Ok(Measured::counted(0, "idle").extra(extra))
        })
        .await
    }

    /// Waits for `fut`, unless the engine dies first (between steps, nothing
    /// else would notice).
    pub async fn guard<T>(&self, fut: impl Future<Output = T>) -> Result<T> {
        tokio::select! {
            value = fut => Ok(value),
            () = self.monitor.wait_exit() => bail!("the engine exited"),
        }
    }

    pub fn alive(&self) -> Result<()> {
        if self.monitor.exited() {
            bail!("the engine exited: see engine.log");
        }
        Ok(())
    }

    pub fn levels<T: Copy>(&self, full: &[T], quick: &[T]) -> Vec<T> {
        if self.opts.quick {
            quick.to_vec()
        } else {
            full.to_vec()
        }
    }
}

fn summary(step: &Step) -> String {
    let r = &step.resources;
    let lat = step
        .latency
        .as_ref()
        .map(|l| format!(" p50 {} p99 {}", fmt_us(l.p50_us), fmt_us(l.p99_us)))
        .unwrap_or_default();
    let errors = if step.errors > 0 {
        format!(" errors {}", step.errors)
    } else {
        String::new()
    };
    format!(
        "{:<28} {:>10.0} {}/s{lat}{errors} | engine {:.2} cpu, rss {:.0} MiB",
        step.label, step.ops_per_sec, step.unit, r.engine_cpu, r.rss_max_mb
    )
}

pub fn fmt_us(us: u64) -> String {
    if us >= 10_000 {
        format!("{:.0}ms", us as f64 / 1000.0)
    } else if us >= 1000 {
        format!("{:.1}ms", us as f64 / 1000.0)
    } else {
        format!("{us}µs")
    }
}

pub async fn run(name: &str, env: &Env) -> Result<()> {
    match name {
        "rpc" => rpc::run(env).await,
        "fanout" => fanout::run(env).await,
        "filters" => filters::run(env).await,
        "observers" => observers::run(env).await,
        "source" => source::run(env).await,
        "match-keys" => match_keys::run(env).await,
        "local" => local::run(env).await,
        "overload" => overload::run(env).await,
        "connections" => connections::run(env).await,
        other => bail!("unknown scenario {other:?} (known: {})", ALL.join(", ")),
    }
}

// ─────────────────────────── shared pieces ───────────────────────────

/// An SDK worker connected to the engine, telemetry off.
pub async fn worker(url: &str, name: &str) -> Result<IIIClient> {
    let iii = register_worker(
        url,
        InitOptions {
            metadata: Some(WorkerMetadata {
                name: name.to_string(),
                ..WorkerMetadata::default()
            }),
            otel: Some(OtelConfig {
                enabled: Some(false),
                ..OtelConfig::default()
            }),
            identity: WorkerIdentityMode::Explicit,
            ..InitOptions::default()
        },
    );
    iii.wait_until_registered(Duration::from_secs(30))
        .await
        .with_context(|| format!("worker {name} did not register"))?;
    Ok(iii)
}

pub async fn workers(url: &str, prefix: &str, n: usize) -> Result<Vec<IIIClient>> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(worker(url, &format!("{prefix}-{i}")).await?);
    }
    Ok(out)
}

pub async fn shutdown(clients: impl IntoIterator<Item = IIIClient>) {
    for c in clients {
        c.shutdown_async().await;
    }
}

/// Returns its input.
pub fn echo() -> RegisterFunction {
    RegisterFunction::new(|input: Value| Ok::<_, Error>(input))
}

/// Counts what it receives.
pub fn sink(counter: Arc<AtomicU64>) -> RegisterFunction {
    RegisterFunction::new(move |_: Value| {
        counter.fetch_add(1, Ordering::Relaxed);
        Ok::<_, Error>(Value::Null)
    })
}

/// Waits until `caller` can call `function_id` (the registration reached the
/// engine).
pub async fn wait_callable(caller: &IIIClient, function_id: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let req = CallRequest {
            timeout_ms: Some(2_000),
            ..CallRequest::new(function_id, json!({ "probe": true }))
        };
        match caller.call(req).await {
            Ok(_) => return Ok(()),
            Err(e) if Instant::now() > deadline => {
                return Err(e).with_context(|| format!("{function_id} never became callable"));
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
}

pub fn payload(seq: u64, pad: &str) -> Value {
    json!({ "n": seq, "pad": pad })
}

/// `concurrency` tasks, spread over `callers`, each calling and waiting
/// until `deadline`. `request(task, seq)` says what to call.
pub async fn closed_loop<F>(
    callers: &[IIIClient],
    concurrency: usize,
    deadline: Instant,
    request: F,
) -> Load
where
    F: Fn(usize, u64) -> CallRequest + Send + Sync + 'static,
{
    let request = Arc::new(request);
    let mut tasks = Vec::with_capacity(concurrency);
    for task in 0..concurrency {
        let caller = callers[task % callers.len()].clone();
        let request = request.clone();
        tasks.push(tokio::spawn(async move {
            let mut load = Load::default();
            let mut seq = 0u64;
            while Instant::now() < deadline {
                let mut req = request(task, seq);
                req.timeout_ms.get_or_insert(10_000);
                seq += 1;
                let t = Instant::now();
                match caller.call(req).await {
                    Ok(_) => load.ok(t.elapsed()),
                    Err(e) => {
                        load.err(e);
                        // A refused or failing engine answers at once: do not spin.
                        if t.elapsed() < Duration::from_millis(1) {
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        }
                    }
                }
            }
            load
        }));
    }
    let mut total = Load::default();
    for task in tasks {
        if let Ok(load) = task.await {
            total.merge(load);
        }
    }
    total
}

/// Totals of `engine::observers::list` over every observer the engine has.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ObserverTotals {
    pub observers: u64,
    pub fires: u64,
    pub dropped: BTreeMap<String, u64>,
    pub activation_failures: BTreeMap<String, u64>,
}

impl ObserverTotals {
    /// What happened between `before` and `self`.
    pub fn since(&self, before: &ObserverTotals) -> Value {
        let diff = |now: &BTreeMap<String, u64>, then: &BTreeMap<String, u64>| {
            now.iter()
                .map(|(k, v)| (k.clone(), v - then.get(k).copied().unwrap_or(0).min(*v)))
                .filter(|(_, v)| *v > 0)
                .collect::<BTreeMap<_, _>>()
        };
        json!({
            "fires": self.fires.saturating_sub(before.fires),
            "dropped": diff(&self.dropped, &before.dropped),
            "activation_failures": diff(&self.activation_failures, &before.activation_failures),
        })
    }
}

pub async fn observer_totals(ctl: &IIIClient) -> Result<ObserverTotals> {
    let out = ctl
        .call(CallRequest {
            timeout_ms: Some(60_000),
            ..CallRequest::new("engine::observers::list", json!({}))
        })
        .await
        .context("engine::observers::list")?;
    let mut totals = ObserverTotals::default();
    for o in out["observers"].as_array().into_iter().flatten() {
        totals.observers += 1;
        totals.fires += o["fires"].as_u64().unwrap_or(0);
        for (field, map) in [
            ("dropped", &mut totals.dropped),
            ("activation_failures", &mut totals.activation_failures),
        ] {
            for (k, v) in o[field].as_object().into_iter().flatten() {
                *map.entry(k.clone()).or_default() += v.as_u64().unwrap_or(0);
            }
        }
    }
    Ok(totals)
}

/// Waits until `counter` stops moving (the activations still on their way
/// after a step), at most `max`.
pub async fn drain(counter: &AtomicU64, max: Duration) -> u64 {
    let deadline = Instant::now() + max;
    let mut last = counter.load(Ordering::Relaxed);
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let now = counter.load(Ordering::Relaxed);
        if now == last || Instant::now() > deadline {
            return now;
        }
        last = now;
    }
}
