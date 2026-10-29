// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `source`: an event source, the way cron or a database source works. One
//! worker hosts `bench::source`, a private function whose handler returns the
//! event it is called with, and calls it in-process with every event. The
//! worker delivers each call itself: the matches go to the engine in batched
//! reports that carry the event once (`output_is_input`), and the engine
//! activates every observer's target. What comes back: calls per second,
//! activations delivered, and what the SDK or the engine dropped on the way.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use iii_sdk::runtime::FunctionRef;
use iii_sdk::{CallRequest, Error, IIIClient, RegisterFunction, Source};
use serde_json::{Value, json};

use super::{
    Env, Measured, drain, observer_totals, shutdown, sink, wait_callable, worker, workers,
};
use crate::stats::Load;

const SOURCE: &str = "bench::source";
const SINKS: usize = 4;
const CALLERS: usize = 4;
/// Observers the paced ladder delivers to: every call activates each of them.
const PACED_OBSERVERS: usize = 16;
/// Offered activations per second, all observers together.
const PACED_RATES: &[u64] = &[1_000, 5_000, 20_000, 50_000, 100_000, 200_000, 500_000];
/// A rate is sustained when at least this share of it is delivered.
const SUSTAINED: f64 = 0.95;

pub async fn run(env: &Env) -> Result<()> {
    let host = worker(&env.url, "host").await?;
    let source = host.register_function(
        SOURCE,
        RegisterFunction::new(|event: Value| Ok::<_, Error>(event)).private(),
    );
    let received = Arc::new(AtomicU64::new(0));
    let sinks = workers(&env.url, "sink", SINKS).await?;
    for (j, w) in sinks.iter().enumerate() {
        w.register_function(format!("bench::sink-{j}"), sink(received.clone()));
    }
    let observing = worker(&env.url, "observer").await?;
    for j in 0..SINKS {
        wait_callable(&observing, &format!("bench::sink-{j}")).await?;
    }
    wait_local(env, &source).await?;
    let ctl = &observing;

    let mut handles = Vec::new();
    for k in env.levels(&[1, 16, 256], &[1, 16]) {
        for i in handles.len()..k {
            handles.push(observing.observe(
                Source::function(SOURCE),
                format!("bench::sink-{}", i % SINKS),
            ));
        }
        for h in &handles {
            env.guard(h.ready()).await??;
        }
        // The host delivers to the observers it has acked: wait until it holds all k.
        let deadline = Instant::now() + Duration::from_secs(30);
        while source.observers().len() < k && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        drain(&received, Duration::from_secs(2)).await;

        let before = observer_totals(ctl).await?;
        let received0 = received.load(Ordering::Relaxed);
        let flood_host = host.clone();
        let flood_received = received.clone();
        env.step(
            format!("{k} observers"),
            json!({ "observers": k, "callers": CALLERS }),
            |deadline| async move {
                let mut tasks = Vec::new();
                for c in 0..CALLERS {
                    let host = flood_host.clone();
                    tasks.push(tokio::spawn(async move {
                        let mut load = Load::default();
                        let mut seq = 0u64;
                        while Instant::now() < deadline {
                            let t = Instant::now();
                            match host.call(event(seq, c)).await {
                                Ok(_) => load.ok(t.elapsed()),
                                Err(err) => load.err(err),
                            }
                            seq += 1;
                            // A local call never waits on the engine: give the
                            // connection its turn.
                            tokio::task::yield_now().await;
                        }
                        load
                    }));
                }
                let mut load = Load::default();
                for t in tasks {
                    load.merge(t.await?);
                }
                let ended = Instant::now();
                let secs = (ended - (deadline - env.opts.step)).as_secs_f64();
                let in_window = flood_received.load(Ordering::Relaxed) - received0;
                let delivered = drain(&flood_received, Duration::from_secs(10)).await - received0;
                let after = observer_totals(ctl).await?;
                let calls = load.ops;
                Ok(Measured::timed(load, "calls").ended(ended).extra(json!({
                    "activations_per_sec": in_window as f64 / secs,
                    "activations_delivered": delivered,
                    "activations_expected": calls * k as u64,
                    "engine": after.since(&before),
                })))
            },
        )
        .await?;

        // The flood above shows what overload does; this finds what the
        // engine sustains: a paced ladder of offered activation rates over
        // these 16 observers, up to the first that is not delivered in full.
        if k == PACED_OBSERVERS {
            for rate in env.levels(PACED_RATES, &[1_000, 20_000]) {
                let step = paced(env, &host, k, &received, ctl, rate).await?;
                let ratio = step.extra["delivery_ratio"].as_f64().unwrap_or(0.0);
                if ratio < SUSTAINED {
                    break;
                }
            }
        }
    }

    // What the engine keeps once the load is over (peak vs. retained).
    drain(&received, Duration::from_secs(10)).await;
    env.idle("after the load (at rest)", json!({}), json!({}))
        .await?;

    for h in &handles {
        h.dispose().await.ok();
    }
    shutdown(sinks.into_iter().chain([host, observing])).await;
    Ok(())
}

/// One event from caller `c`.
fn event(seq: u64, c: usize) -> CallRequest {
    CallRequest::new(SOURCE, json!({ "n": seq, "caller": c }))
}

/// Waits until the host's calls of the source run in-process: before the
/// engine confirms the registration they go through it.
async fn wait_local(env: &Env, source: &FunctionRef) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !source.local_dispatch_active() {
        env.alive()?;
        if Instant::now() > deadline {
            bail!("{SOURCE} never ran in-process (local dispatch off?)");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

/// One paced step: `rate` activations a second over `observers` observers,
/// that is `rate / observers` calls a second, for the step's length; then
/// what arrived.
async fn paced(
    env: &Env,
    host: &IIIClient,
    observers: usize,
    received: &Arc<AtomicU64>,
    ctl: &IIIClient,
    rate: u64,
) -> Result<super::Step> {
    drain(received, Duration::from_secs(5)).await;
    let before = observer_totals(ctl).await?;
    let received0 = received.load(Ordering::Relaxed);
    env.step(
        format!(
            "{observers} observers, {} activations/s offered",
            crate::report::fmt_rate(rate as f64)
        ),
        json!({ "observers": observers, "offered_per_sec": rate }),
        |deadline| async move {
            let mut tick = tokio::time::interval(Duration::from_millis(1));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
            let per_tick = rate as f64 / observers as f64 / 1000.0;
            let (mut owed, mut calls, mut failed) = (0.0, 0u64, 0u64);
            while Instant::now() < deadline {
                tick.tick().await;
                owed += per_tick;
                while owed >= 1.0 {
                    owed -= 1.0;
                    if host.call(event(calls, 0)).await.is_err() {
                        failed += 1;
                    }
                    calls += 1;
                }
            }
            let ended = Instant::now();
            let secs = (ended - (deadline - env.opts.step)).as_secs_f64();
            let in_window = received.load(Ordering::Relaxed) - received0;
            let delivered = drain(received, Duration::from_secs(10)).await - received0;
            let after = observer_totals(ctl).await?;
            let expected = calls * observers as u64;
            Ok(Measured::counted(calls, "calls").ended(ended).extra(json!({
                "offered_per_sec": rate,
                "calls_failed": failed,
                "activations_per_sec": in_window as f64 / secs,
                "activations_delivered": delivered,
                "activations_expected": expected,
                "delivery_ratio": delivered as f64 / expected.max(1) as f64,
                "engine": after.since(&before),
            })))
        },
    )
    .await
}
