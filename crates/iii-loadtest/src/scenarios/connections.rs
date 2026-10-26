// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `connections`: idle workers. Each is a raw WebSocket that announces itself
//! (`engine::workers::register`), registers two functions and then only
//! answers pings, so thousands fit in this process (an SDK client is a thread
//! each). Connections are opened in levels and the engine is sampled at rest
//! after each: memory per connected worker, and the CPU an idle fleet costs.
//! Then every connection closes and the engine is sampled again.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{Semaphore, watch};
use tokio_tungstenite::tungstenite::Message as Ws;

use super::{Env, Measured};
use crate::stats::{Load, histogram, record};

const FUNCTIONS_PER_WORKER: usize = 2;
/// Connections opened at once.
const OPENING: usize = 64;

pub async fn run(env: &Env) -> Result<()> {
    let (close_tx, close_rx) = watch::channel(false);
    let mut open = 0usize;
    let mut tasks = Vec::new();

    let baseline = env
        .idle("0 workers (at rest)", json!({ "workers": 0 }), json!({}))
        .await?;
    let base_rss = baseline.resources.rss_avg_mb;

    for n in env.levels(&[100, 1_000, 5_000, 10_000], &[100, 1_000]) {
        let from = open;
        let url = env.url.clone();
        let close_rx = close_rx.clone();
        let (_, opened) = env
            .step_with(
                format!("connect → {n}"),
                json!({ "workers": n, "connected": n - from }),
                |_| async move {
                    let gate = Arc::new(Semaphore::new(OPENING));
                    let mut connecting = Vec::new();
                    for i in from..n {
                        let permit = gate.clone().acquire_owned().await?;
                        let url = url.clone();
                        let close_rx = close_rx.clone();
                        connecting.push(tokio::spawn(async move {
                            let started = Instant::now();
                            let result = connect(&url, i).await;
                            drop(permit);
                            result.map(|(ws, phases)| {
                                (started.elapsed(), phases, tokio::spawn(hold(ws, close_rx)))
                            })
                        }));
                    }
                    let mut load = Load::default();
                    let mut held = Vec::new();
                    let mut parts = [histogram(), histogram(), histogram()];
                    for c in connecting {
                        match c.await? {
                            Ok((elapsed, phases, task)) => {
                                load.ok(elapsed);
                                record(&mut parts[0], phases.tcp);
                                record(&mut parts[1], phases.handshake);
                                record(&mut parts[2], phases.registered);
                                held.push(task);
                            }
                            Err(e) => load.err(format!("{e:#}")),
                        }
                    }
                    let p50 = |h: &hdrhistogram::Histogram<u64>| h.value_at_quantile(0.5);
                    // Where a connection's time goes (median, µs): the TCP
                    // connect, the WebSocket upgrade, then `workerregistered`.
                    let extra = json!({
                        "phases_p50_us": {
                            "tcp": p50(&parts[0]),
                            "handshake": p50(&parts[1]),
                            "registered": p50(&parts[2]),
                        }
                    });
                    Ok((Measured::timed(load, "connections").extra(extra), held))
                },
            )
            .await?;
        open += opened.len();
        tasks.extend(opened);
        // Registrations are processed after the handshake: give them a moment.
        tokio::time::sleep(Duration::from_secs(1)).await;
        let rest = env
            .idle(
                format!("{n} workers (at rest)"),
                json!({ "workers": n }),
                json!({ "open": open }),
            )
            .await?;
        if open > 0 {
            let per = (rest.resources.rss_avg_mb - base_rss) * 1024.0 * 1024.0 / open as f64;
            env.annotate_last(json!({ "rss_bytes_per_worker": per.round() }));
        }
    }

    close_tx.send(true).ok();
    for t in tasks {
        let _ = t.await;
    }
    // Cleaning up after the workers that left is work too: watch it end.
    env.idle("just closed", json!({ "workers": 0 }), json!({}))
        .await?;
    tokio::time::sleep(Duration::from_secs(if env.opts.quick { 2 } else { 10 })).await;
    env.idle("after close (at rest)", json!({ "workers": 0 }), json!({}))
        .await?;
    Ok(())
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// How long each part of a connection took.
#[derive(Clone, Copy, Default)]
struct Phases {
    tcp: Duration,
    handshake: Duration,
    registered: Duration,
}

/// Connects worker `i`, waits for `workerregistered`, announces it and
/// registers its functions.
async fn connect(url: &str, i: usize) -> Result<(Socket, Phases)> {
    let mut phases = Phases::default();
    let started = Instant::now();
    let addr = url
        .trim_start_matches("ws://")
        .trim_end_matches('/')
        .to_string();
    let deadline = started + Duration::from_secs(30);
    let tcp = tokio::time::timeout_at(deadline.into(), tokio::net::TcpStream::connect(&addr))
        .await
        .context("TCP connect timed out")?
        .context("TCP connect")?;
    // TCP_NODELAY, as the SDKs set once the engine advertises it.
    tcp.set_nodelay(true)?;
    phases.tcp = started.elapsed();
    let (mut ws, _) = tokio::time::timeout_at(
        deadline.into(),
        tokio_tungstenite::client_async(url, tokio_tungstenite::MaybeTlsStream::Plain(tcp)),
    )
    .await
    .context("WebSocket handshake timed out")?
    .context("WebSocket handshake")?;
    phases.handshake = started.elapsed() - phases.tcp;
    loop {
        let frame = tokio::time::timeout_at(deadline.into(), ws.next())
            .await
            .context("no workerregistered")?;
        match frame {
            Some(Ok(Ws::Text(text))) => {
                let v: Value = serde_json::from_str(&text).unwrap_or_default();
                if v["type"] == "workerregistered" {
                    break;
                }
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(e).context("read"),
            None => bail!("closed before workerregistered"),
        }
    }
    phases.registered = started.elapsed() - phases.tcp - phases.handshake;
    let mut frames = vec![json!({
        "type": "invokefunction",
        "function_id": "engine::workers::register",
        "data": { "runtime": "rust", "version": "loadtest", "name": format!("idle-{i}"), "os": "linux" },
        "action": { "type": "void" },
    })];
    for f in 0..FUNCTIONS_PER_WORKER {
        frames.push(json!({
            "type": "registerfunction",
            "id": format!("bench::idle-{i}-{f}"),
            "description": "idle worker function",
        }));
    }
    for frame in frames {
        ws.send(Ws::Text(frame.to_string().into()))
            .await
            .context("send")?;
    }
    Ok((ws, phases))
}

/// Answers pings until told to close.
async fn hold(mut ws: Socket, mut close: watch::Receiver<bool>) {
    loop {
        tokio::select! {
            _ = close.changed() => break,
            frame = ws.next() => match frame {
                Some(Ok(Ws::Text(text))) => {
                    if text.contains("\"ping\"") {
                        let _ = ws.send(Ws::Text(json!({ "type": "pong" }).to_string().into())).await;
                    }
                }
                Some(Ok(Ws::Ping(p))) => {
                    let _ = ws.send(Ws::Pong(p)).await;
                }
                Some(Ok(_)) => {}
                _ => return,
            },
        }
    }
    let _ = ws.close(None).await;
}
