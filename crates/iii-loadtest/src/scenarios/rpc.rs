// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `rpc`: callers invoke echo functions hosted by other workers, through the
//! engine, with nobody observing. The baseline every observe number is read
//! against, and the engine's routing capacity: concurrency rises until
//! throughput stops growing, then the payload grows at a fixed concurrency.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use iii_sdk::protocol::CallRequest;
use serde_json::json;

use super::{Env, Measured, closed_loop, echo, payload, shutdown, wait_callable, workers};

const ECHO_WORKERS: usize = 4;
const CALLERS: usize = 4;

pub async fn run(env: &Env) -> Result<()> {
    let hosts = workers(&env.url, "echo", ECHO_WORKERS).await?;
    let ids: Arc<Vec<String>> = Arc::new(
        (0..ECHO_WORKERS)
            .map(|i| format!("bench::echo-{i}"))
            .collect(),
    );
    for (host, id) in hosts.iter().zip(ids.iter()) {
        host.register_function(id.as_str(), echo());
    }
    let callers = workers(&env.url, "caller", CALLERS).await?;
    for id in ids.iter() {
        wait_callable(&callers[0], id).await?;
    }

    let calls = |pad: String| {
        let ids = ids.clone();
        move |task: usize, seq: u64| {
            CallRequest::new(ids[task % ids.len()].as_str(), payload(seq, &pad))
        }
    };

    closed_loop(
        &callers,
        32,
        Instant::now() + env.opts.warmup,
        calls("x".repeat(64)),
    )
    .await;
    for concurrency in env.levels(&[1, 8, 32, 128, 512, 2048], &[1, 32]) {
        let request = calls("x".repeat(64));
        env.step(
            format!("concurrency {concurrency}"),
            json!({ "concurrency": concurrency, "payload_bytes": 64 }),
            |deadline| {
                let callers = callers.clone();
                async move {
                    let load = closed_loop(&callers, concurrency, deadline, request).await;
                    Ok(Measured::timed(load, "calls"))
                }
            },
        )
        .await?;
    }

    for kib in env.levels(&[1, 16, 256], &[16]) {
        let request = calls("x".repeat(kib * 1024));
        env.step(
            format!("payload {kib} KiB"),
            json!({ "concurrency": 32, "payload_bytes": kib * 1024 }),
            |deadline| {
                let callers = callers.clone();
                async move {
                    let load = closed_loop(&callers, 32, deadline, request).await;
                    Ok(Measured::timed(load, "calls"))
                }
            },
        )
        .await?;
    }

    shutdown(callers.into_iter().chain(hosts)).await;
    Ok(())
}
