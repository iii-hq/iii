// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `match-keys`: an event source that matches observers by keys, as the
//! database source does with its tables. One worker hosts `bench::keyed`, a
//! private function whose handler returns its input, with an
//! `onObserverMatch` by keys: an observer of `{ "key": "k42" }` is filed under
//! `k42`, and a call of `{ "key": "k42" }` matches it. N observers (up to
//! 10,000) each take their own key, and the worker calls the function
//! in-process: first with keys that match exactly one observer, then with a
//! key nobody observes. What grows with N is the index, not the work per call:
//! one hash lookup per call key, one filter evaluation and one report item.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use iii_sdk::{CallRequest, Error, IIIClient, Match, RegisterFunction, Source};
use serde_json::{Value, json};

use super::{
    Env, Measured, closed_loop, drain, observer_totals, shutdown, sink, wait_callable, worker,
    workers,
};

const KEYED: &str = "bench::keyed";
const SINKS: usize = 4;
const CONCURRENCY: usize = 64;
/// Observers one connection may own is 1024 by default.
const PER_CONNECTION: usize = 1000;
/// The most observers of one function the scenario creates. The engine
/// allows 4096 by default: [`super::engine_observers`] raises it for this
/// scenario.
pub const MOST: usize = 10_000;

pub async fn run(env: &Env) -> Result<()> {
    let host = worker(&env.url, "keyed").await?;
    // Set by id before registering, so it sees every observer.
    host.on_observer_match_for(
        KEYED,
        Match::keys(
            |obs| obs.args_value()["key"].as_str().into(),
            |call| call.input["key"].as_str().into(),
        ),
    );
    let keyed = host.register_function(
        KEYED,
        RegisterFunction::new(|event: Value| Ok::<_, Error>(event))
            .args_format_value(json!({
                "type": "object",
                "properties": { "key": { "type": "string" } },
                "required": ["key"],
            }))
            .private(),
    );
    let received = Arc::new(AtomicU64::new(0));
    let sinks = workers(&env.url, "sink", SINKS).await?;
    for (j, w) in sinks.iter().enumerate() {
        w.register_function(format!("bench::sink-{j}"), sink(received.clone()));
    }
    let ctl = worker(&env.url, "ctl").await?;
    for j in 0..SINKS {
        wait_callable(&ctl, &format!("bench::sink-{j}")).await?;
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while !keyed.local_dispatch_active() {
        env.alive()?;
        if Instant::now() > deadline {
            bail!("{KEYED} never ran in-process (local dispatch off?)");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let mut observing = Vec::new();
    let mut handles = Vec::new();
    for n in env.levels(&[100, 1_000, MOST], &[100, 1_000]) {
        let created = Instant::now();
        let mut adding = Vec::new();
        for i in handles.len()..n {
            if i / PER_CONNECTION >= observing.len() {
                observing.push(worker(&env.url, &format!("observer-{}", observing.len())).await?);
            }
            let source = Source::function(KEYED).args(json!({ "key": key(i as u64) }));
            adding.push(
                observing[i / PER_CONNECTION].observe(source, format!("bench::sink-{}", i % SINKS)),
            );
        }
        let added = adding.len();
        for h in &adding {
            env.guard(h.ready()).await??;
        }
        handles.extend(adding);
        // The host matches the observers it has acked: wait until it holds all n.
        let deadline = Instant::now() + Duration::from_secs(60);
        while keyed.observers().len() < n && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let create_secs = created.elapsed().as_secs_f64();
        tokio::time::sleep(Duration::from_millis(500)).await;

        step(
            env,
            &host,
            &ctl,
            &received,
            n,
            Some(added as f64 / create_secs),
        )
        .await?;
        step(env, &host, &ctl, &received, n, None).await?;
    }

    for h in &handles {
        h.dispose().await.ok();
    }
    shutdown([host, ctl].into_iter().chain(observing).chain(sinks)).await;
    Ok(())
}

/// The key observer `i` is filed under.
fn key(i: u64) -> String {
    format!("k{i}")
}

/// One step over `n` keyed observers: calls whose key matches exactly one of
/// them (`created_per_sec` is how fast they were created), or with `None`, calls
/// whose key matches none.
async fn step(
    env: &Env,
    host: &IIIClient,
    ctl: &IIIClient,
    received: &Arc<AtomicU64>,
    n: usize,
    created_per_sec: Option<f64>,
) -> Result<()> {
    let matching = created_per_sec.is_some();
    drain(received, Duration::from_secs(5)).await;
    let before = observer_totals(ctl).await?;
    let received0 = received.load(Ordering::Relaxed);
    let label = if matching {
        format!("{n} keyed observers, 1 matching")
    } else {
        format!("{n} keyed observers, none matching")
    };
    env.step(
        label,
        json!({ "observers": n, "concurrency": CONCURRENCY, "matching": u8::from(matching) }),
        |deadline| {
            let host = host.clone();
            let received = received.clone();
            async move {
                let n = n as u64;
                let load = closed_loop(
                    std::slice::from_ref(&host),
                    CONCURRENCY,
                    deadline,
                    move |task, seq| {
                        let key = if matching {
                            // Spread the keys over [0, n) without a shared RNG.
                            key((seq.wrapping_mul(2_654_435_761) + task as u64) % n)
                        } else {
                            "nobody".to_string()
                        };
                        CallRequest::new(KEYED, json!({ "key": key, "n": seq }))
                    },
                )
                .await;
                let ended = Instant::now();
                let secs = (ended - (deadline - env.opts.step)).as_secs_f64();
                let in_window = received.load(Ordering::Relaxed) - received0;
                let delivered = drain(&received, Duration::from_secs(10)).await - received0;
                let after = observer_totals(ctl).await?;
                let expected = if matching { load.ops } else { 0 };
                let mut extra = json!({
                    "activations_per_sec": in_window as f64 / secs,
                    "activations_delivered": delivered,
                    "activations_expected": expected,
                    "engine": after.since(&before),
                });
                if let Some(rate) = created_per_sec {
                    extra["observers_created_per_sec"] = json!(rate);
                    extra["delivery_ratio"] = json!(delivered as f64 / expected.max(1) as f64);
                }
                Ok(Measured::timed(load, "calls").ended(ended).extra(extra))
            }
        },
    )
    .await?;
    Ok(())
}
