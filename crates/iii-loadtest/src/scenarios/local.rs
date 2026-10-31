// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `local`: a worker calling a function it hosts. With `local-dispatch.v1`
//! the call runs in-process and touches the wire only when an observer's
//! filter matches, in batched reports. Three steps: nobody observing (the
//! engine should see nothing), an observer matching 1% of the calls, and
//! one matching all of them.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use iii_sdk::Source;
use iii_sdk::protocol::CallRequest;
use serde_json::{Value, json};

use super::{
    Env, Measured, closed_loop, drain, echo, observer_totals, shutdown, sink, wait_callable, worker,
};

const CONCURRENCY: usize = 16;

pub async fn run(env: &Env) -> Result<()> {
    let local = worker(&env.url, "local").await?;
    local.register_function("bench::local", echo());
    let received = Arc::new(AtomicU64::new(0));
    let observing = worker(&env.url, "observer").await?;
    observing.register_function("bench::sink", sink(received.clone()));
    wait_callable(&local, "bench::local").await?;
    wait_callable(&local, "bench::sink").await?;
    let ctl = &observing;
    let callers = vec![local.clone()];

    let calls = |seq: u64| CallRequest::new("bench::local", json!({ "n": seq, "pct": seq % 100 }));
    closed_loop(
        &callers,
        CONCURRENCY,
        Instant::now() + env.opts.warmup,
        move |_, s| calls(s),
    )
    .await;

    let cases: [(&str, Option<Value>); 3] = [
        ("unobserved", None),
        (
            "observed, 1% match",
            Some(json!({ "input.pct": { "$eq": 0 } })),
        ),
        // No filters: every call matches.
        ("observed, 100% match", Some(Value::Null)),
    ];
    for (label, filters) in cases {
        let handle = match &filters {
            Some(f) => {
                let mut source = Source::function("bench::local");
                if !f.is_null() {
                    source = source.filters(f.clone());
                }
                let h = observing.observe(source, "bench::sink");
                env.guard(h.ready()).await??;
                // The engine pushes the observer to the host's SDK.
                tokio::time::sleep(Duration::from_millis(500)).await;
                Some(h)
            }
            None => None,
        };
        let before = observer_totals(ctl).await?;
        let received0 = received.load(Ordering::Relaxed);
        let received = received.clone();
        env.step(
            label,
            json!({ "concurrency": CONCURRENCY, "filters": filters }),
            |deadline| {
                let callers = callers.clone();
                async move {
                    let load =
                        closed_loop(&callers, CONCURRENCY, deadline, move |_, s| calls(s)).await;
                    let ended = Instant::now();
                    let secs = (ended - (deadline - env.opts.step)).as_secs_f64();
                    let in_window = received.load(Ordering::Relaxed) - received0;
                    let delivered = drain(&received, Duration::from_secs(10)).await - received0;
                    let after = observer_totals(ctl).await?;
                    Ok(Measured::timed(load, "local calls")
                        .ended(ended)
                        .extra(json!({
                            "activations_per_sec": in_window as f64 / secs,
                            "activations_delivered": delivered,
                            "engine": after.since(&before),
                        })))
                }
            },
        )
        .await?;
        if let Some(h) = handle {
            h.dispose().await.ok();
        }
    }

    // What the engine keeps once the load is over (peak vs. retained).
    drain(&received, Duration::from_secs(10)).await;
    env.idle("after the load (at rest)", json!({}), json!({}))
        .await?;

    shutdown([local, observing]).await;
    Ok(())
}
