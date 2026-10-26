// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `observers`: how many observers the engine holds and what each costs. No
//! calls: observers are added in levels (each with a small filter, spread over
//! 16 functions to stay under the per-function cap, 1000 per observing
//! connection to stay under the per-connection one), the engine is sampled at
//! rest after each level, then every observer is disposed and the engine is
//! sampled again to see the memory come back.

use std::time::Instant;

use anyhow::Result;
use iii_sdk::Source;
use serde_json::json;

use super::{Env, Measured, echo, shutdown, sink, wait_callable, worker};
use crate::stats::Load;

const FUNCTIONS: usize = 16;
const PER_CONNECTION: usize = 1000;

pub async fn run(env: &Env) -> Result<()> {
    let host = worker(&env.url, "observed").await?;
    for f in 0..FUNCTIONS {
        host.register_function(format!("bench::observed-{f}"), echo());
    }
    let target = worker(&env.url, "target").await?;
    target.register_function("bench::target", sink(Default::default()));
    wait_callable(&host, "bench::target").await?;

    let baseline = env
        .idle(
            "0 observers (at rest)",
            json!({ "observers": 0 }),
            json!({}),
        )
        .await?;
    let base_rss = baseline.resources.rss_avg_mb;

    let mut observing = Vec::new();
    let mut handles = Vec::new();
    for n in env.levels(&[1_000, 4_000, 16_000, 64_000], &[1_000, 4_000]) {
        let from = handles.len();
        let mut needed = Vec::new();
        for i in from..n {
            if i / PER_CONNECTION >= observing.len() + needed.len() {
                needed.push(observing.len() + needed.len());
            }
        }
        for c in needed {
            observing.push(worker(&env.url, &format!("observer-{c}")).await?);
        }
        let observing_now = observing.clone();
        let step = env
            .step_with(
                format!("create → {n}"),
                json!({ "observers": n, "created": n - from }),
                |_| async move {
                    let mut load = Load::default();
                    let mut adding = Vec::with_capacity(n - from);
                    let started = Instant::now();
                    for i in from..n {
                        let source = Source::function(format!("bench::observed-{}", i % FUNCTIONS))
                            .filters(json!({ "input.k": { "$eq": i } }));
                        adding.push(
                            observing_now[i / PER_CONNECTION].observe(source, "bench::target"),
                        );
                    }
                    for h in &adding {
                        match h.ready().await {
                            // Time to ready, measured from when the batch started:
                            // the queueing the engine imposes on a burst.
                            Ok(()) => load.ok(started.elapsed()),
                            Err(e) => load.err(e),
                        }
                    }
                    Ok((Measured::timed(load, "observers"), adding))
                },
            )
            .await;
        let (step, added) = step?;
        handles.extend(added);
        let rest = env
            .idle(
                format!("{n} observers (at rest)"),
                json!({ "observers": n }),
                json!({ "create_errors": step.errors }),
            )
            .await?;
        let per = (rest.resources.rss_avg_mb - base_rss) * 1024.0 * 1024.0 / n as f64;
        env.annotate_last(json!({ "rss_bytes_per_observer": per.round() }));
    }

    let total = handles.len();
    env.step(
        format!("dispose {total}"),
        json!({ "observers": total }),
        |_| async {
            let mut load = Load::default();
            let started = Instant::now();
            for h in &handles {
                match h.dispose().await {
                    Ok(()) => load.ok(started.elapsed()),
                    Err(e) => load.err(e),
                }
            }
            Ok(Measured::timed(load, "disposals"))
        },
    )
    .await?;
    env.idle(
        "after dispose (at rest)",
        json!({ "observers": 0 }),
        json!({}),
    )
    .await?;

    shutdown(observing.into_iter().chain([host, target])).await;
    Ok(())
}
