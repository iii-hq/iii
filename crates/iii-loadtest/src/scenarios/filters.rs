// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `filters`: one function with N observers, each filtering on a different
//! value of `input.k` (`{"input.k": {"$eq": i}}`), and calls that carry a
//! `k` below N: every call matches exactly one observer. What grows with N
//! is the filter work per call, not the activations, so the cost per call
//! shows how filter evaluation scales up to the per-function cap (4096).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use iii_sdk::Source;
use iii_sdk::protocol::CallRequest;
use serde_json::json;

use super::{
    Env, Measured, closed_loop, drain, echo, observer_totals, shutdown, sink, wait_callable,
    worker, workers,
};

const SINKS: usize = 4;
const CONCURRENCY: usize = 64;
/// Observers one connection may own is 1024 by default.
const PER_CONNECTION: usize = 1000;

pub async fn run(env: &Env) -> Result<()> {
    let host = worker(&env.url, "echo-0").await?;
    host.register_function("bench::echo-0", echo());
    let received = Arc::new(AtomicU64::new(0));
    let sinks = workers(&env.url, "sink", SINKS).await?;
    for (j, w) in sinks.iter().enumerate() {
        w.register_function(format!("bench::sink-{j}"), sink(received.clone()));
    }
    let callers = workers(&env.url, "caller", 4).await?;
    wait_callable(&callers[0], "bench::echo-0").await?;
    for j in 0..SINKS {
        wait_callable(&callers[0], &format!("bench::sink-{j}")).await?;
    }
    let ctl = &callers[0];

    let mut observing = Vec::new();
    let mut handles = Vec::new();
    for n in env.levels(&[16, 256, 1024, 4000], &[16, 256]) {
        let created = Instant::now();
        let mut adding = Vec::new();
        for i in handles.len()..n {
            if i / PER_CONNECTION >= observing.len() {
                observing.push(worker(&env.url, &format!("observer-{}", observing.len())).await?);
            }
            let source =
                Source::function("bench::echo-0").filters(json!({ "input.k": { "$eq": i } }));
            adding.push(
                observing[i / PER_CONNECTION].observe(source, format!("bench::sink-{}", i % SINKS)),
            );
        }
        let added = adding.len();
        for h in &adding {
            env.guard(h.ready()).await??;
        }
        let create_secs = created.elapsed().as_secs_f64();
        handles.extend(adding);
        tokio::time::sleep(Duration::from_millis(500)).await;

        let before = observer_totals(ctl).await?;
        let received0 = received.load(Ordering::Relaxed);
        let received = received.clone();
        env.step(
            format!("{n} filtered observers"),
            json!({ "observers": n, "concurrency": CONCURRENCY }),
            |deadline| {
                let callers = callers.clone();
                async move {
                    let n = n as u64;
                    let load = closed_loop(&callers, CONCURRENCY, deadline, move |task, seq| {
                        // Spread k over [0, n) without a shared RNG.
                        let k = (seq.wrapping_mul(2_654_435_761) + task as u64) % n;
                        CallRequest::new("bench::echo-0", json!({ "k": k }))
                    })
                    .await;
                    let ended = Instant::now();
                    let secs = (ended - (deadline - env.opts.step)).as_secs_f64();
                    let in_window = received.load(Ordering::Relaxed) - received0;
                    let delivered = drain(&received, Duration::from_secs(10)).await - received0;
                    let after = observer_totals(ctl).await?;
                    Ok(Measured::timed(load, "calls").ended(ended).extra(json!({
                        "observers_created": added,
                        "observers_created_per_sec": added as f64 / create_secs,
                        "activations_per_sec": in_window as f64 / secs,
                        "activations_delivered": delivered,
                        "engine": after.since(&before),
                    })))
                }
            },
        )
        .await?;
    }

    for h in &handles {
        h.dispose().await.ok();
    }
    shutdown(
        callers
            .into_iter()
            .chain(observing)
            .chain(sinks)
            .chain([host]),
    )
    .await;
    Ok(())
}
