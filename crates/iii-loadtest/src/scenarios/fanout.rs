// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `fanout`: the `rpc` load at a fixed concurrency while every called
//! function gains auto observers (no filters), 0 → 64 each. Every successful
//! call then activates that many targets, fire-and-forget: what it costs the
//! engine per activation, whether the observed call itself slows down, and
//! when activations start being dropped instead of delivered.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use iii_sdk::Source;
use iii_sdk::protocol::TriggerRequest;
use serde_json::json;

use super::{
    Env, Measured, closed_loop, drain, echo, observer_totals, payload, shutdown, sink,
    wait_callable, workers,
};

const FUNCTIONS: usize = 2;
const OBSERVER_WORKERS: usize = 4;
const CONCURRENCY: usize = 64;

pub async fn run(env: &Env) -> Result<()> {
    let hosts = workers(&env.url, "echo", FUNCTIONS).await?;
    let ids: Arc<Vec<String>> =
        Arc::new((0..FUNCTIONS).map(|i| format!("bench::echo-{i}")).collect());
    for (host, id) in hosts.iter().zip(ids.iter()) {
        host.register_function(id.as_str(), echo());
    }
    let received = Arc::new(AtomicU64::new(0));
    let observers = workers(&env.url, "observer", OBSERVER_WORKERS).await?;
    for (j, w) in observers.iter().enumerate() {
        w.register_function(format!("bench::sink-{j}"), sink(received.clone()));
    }
    let callers = workers(&env.url, "caller", 4).await?;
    for id in ids.iter() {
        wait_callable(&callers[0], id).await?;
    }
    for j in 0..OBSERVER_WORKERS {
        wait_callable(&callers[0], &format!("bench::sink-{j}")).await?;
    }
    let ctl = &callers[0];

    let calls = {
        let ids = ids.clone();
        let pad = "x".repeat(64);
        move |task: usize, seq: u64| {
            TriggerRequest::new(ids[task % ids.len()].as_str(), payload(seq, &pad))
        }
    };
    let calls = Arc::new(calls);

    closed_loop(&callers, CONCURRENCY, Instant::now() + env.opts.warmup, {
        let calls = calls.clone();
        move |t, s| calls(t, s)
    })
    .await;

    let mut handles = Vec::new();
    let mut per_function = 0;
    for k in env.levels(&[0, 1, 4, 16, 64], &[0, 4]) {
        // Grow every function to k observers, spread over the observing workers.
        let mut adding = Vec::new();
        for id in ids.iter() {
            for m in per_function..k {
                let w = m % OBSERVER_WORKERS;
                adding.push(
                    observers[w].observe(Source::function(id.as_str()), format!("bench::sink-{w}")),
                );
            }
        }
        for h in &adding {
            env.guard(h.ready()).await??;
        }
        handles.extend(adding);
        per_function = k;
        tokio::time::sleep(Duration::from_millis(500)).await;
        drain(&received, Duration::from_secs(2)).await;

        let before = observer_totals(ctl).await?;
        let received0 = received.load(Ordering::Relaxed);
        let calls = calls.clone();
        let received = received.clone();
        env.step(
            format!("{k} observers/function"),
            json!({ "observers_per_function": k, "functions": FUNCTIONS, "concurrency": CONCURRENCY }),
            |deadline| {
                let callers = callers.clone();
                async move {
                    let load = closed_loop(&callers, CONCURRENCY, deadline, move |t, s| calls(t, s)).await;
                    let ended = Instant::now();
                    let secs = (ended - (deadline - env.opts.step)).as_secs_f64();
                    let in_window = received.load(Ordering::Relaxed) - received0;
                    let delivered = drain(&received, Duration::from_secs(10)).await - received0;
                    let after = observer_totals(ctl).await?;
                    let expected = load.ops * k as u64;
                    Ok(Measured::timed(load, "calls")
                        .ended(ended)
                        .extra(json!({
                            "activations_per_sec": in_window as f64 / secs,
                            "activations_expected": expected,
                            "activations_delivered": delivered,
                            "engine": after.since(&before),
                        })))
                }
            },
        )
        .await?;
    }

    // What the engine keeps once the load is over (peak vs. retained).
    drain(&received, Duration::from_secs(10)).await;
    env.idle("after the load (at rest)", json!({}), json!({}))
        .await?;

    for h in &handles {
        h.dispose().await.ok();
    }
    shutdown(callers.into_iter().chain(observers).chain(hosts)).await;
    Ok(())
}
