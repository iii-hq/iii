// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `overload`: observers whose target is slower than the calls it observes.
//! The observers of one function target a function that takes 20 ms, so at
//! the default `max_in_flight` of 64 each drains about 3 200 activations a
//! second; the calls go much faster. The engine must keep each observer's
//! queue at `max_queue_per_observer` and drop the rest (`OVERFLOW`), and the
//! observed calls must not slow down. Four observers at rising concurrency
//! first, then 64 and 256 of them: every one holds a full queue, so the
//! engine's memory shows what a queued activation costs and how far the
//! per-observer bound lets the total grow.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use iii_sdk::protocol::CallRequest;
use iii_sdk::{Error, RegisterFunction, Source};
use serde_json::{Value, json};

use super::{
    Env, Measured, closed_loop, drain, echo, observer_totals, payload, shutdown, wait_callable,
    worker, workers,
};

const TARGET_MS: u64 = 20;

pub async fn run(env: &Env) -> Result<()> {
    let host = worker(&env.url, "echo-0").await?;
    host.register_function("bench::echo-0", echo());
    let received = Arc::new(AtomicU64::new(0));
    let slow = worker(&env.url, "slow").await?;
    slow.register_function("bench::slow", {
        let received = received.clone();
        RegisterFunction::new_async(move |_: Value| {
            let received = received.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(TARGET_MS)).await;
                received.fetch_add(1, Ordering::Relaxed);
                Ok::<_, Error>(Value::Null)
            }
        })
    });
    let callers = workers(&env.url, "caller", 4).await?;
    wait_callable(&callers[0], "bench::echo-0").await?;
    wait_callable(&callers[0], "bench::slow").await?;
    let ctl = &callers[0];

    let mut handles = Vec::new();
    let pad = "x".repeat(64);
    // (slow observers, concurrency): the call rate first, then the number of
    // full queues the engine holds at once.
    let levels = env.levels(
        &[(4, 4), (4, 64), (4, 256), (64, 64), (256, 64)],
        &[(4, 64), (64, 64)],
    );
    for (observers, concurrency) in levels {
        while handles.len() < observers {
            handles.push(slow.observe(Source::function("bench::echo-0"), "bench::slow"));
        }
        for h in &handles {
            env.guard(h.ready()).await??;
        }
        drain(&received, Duration::from_secs(30)).await;
        let before = observer_totals(ctl).await?;
        let received0 = received.load(Ordering::Relaxed);
        let received = received.clone();
        let pad = pad.clone();
        env.step(
            format!("{observers} slow observers, concurrency {concurrency}"),
            json!({ "concurrency": concurrency, "observers": observers, "target_ms": TARGET_MS }),
            |deadline| {
                let callers = callers.clone();
                async move {
                    let load = closed_loop(&callers, concurrency, deadline, move |_, s| {
                        CallRequest::new("bench::echo-0", payload(s, &pad))
                    })
                    .await;
                    let ended = Instant::now();
                    let secs = (ended - (deadline - env.opts.step)).as_secs_f64();
                    let in_window = received.load(Ordering::Relaxed) - received0;
                    let after = observer_totals(ctl).await?;
                    Ok(Measured::timed(load, "calls").ended(ended).extra(json!({
                        "activations_per_sec": in_window as f64 / secs,
                        "activations_expected": load_expected(&after, &before),
                        "engine": after.since(&before),
                    })))
                }
            },
        )
        .await?;
    }
    // The backlog drains; the queues' memory should come back.
    drain(&received, Duration::from_secs(60)).await;
    env.idle("after the backlog (at rest)", json!({}), json!({}))
        .await?;

    for h in &handles {
        h.dispose().await.ok();
    }
    shutdown(callers.into_iter().chain([host, slow])).await;
    Ok(())
}

/// Activations the engine set out to make (fires) in the window.
fn load_expected(after: &super::ObserverTotals, before: &super::ObserverTotals) -> u64 {
    after.fires.saturating_sub(before.fires)
}
