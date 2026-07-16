// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! E2E tests for the opt-in `ping_interval_secs` keepalive on the
//! worker-manager listener: unresponsive connections are closed after two
//! silent intervals, responsive ones stay, and the default (unset) keeps
//! the previous no-ping behavior.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use iii::engine::Engine;
use iii::workers::traits::Worker;
use iii::workers::worker::WorkerManager;
use serde_json::{Value, json};
use tokio::net::TcpListener;

async fn spawn_engine(mut config: Value) -> (u16, Arc<Engine>) {
    iii::workers::observability::metrics::ensure_default_meter();

    let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
    let port = probe.local_addr().expect("local_addr").port();
    drop(probe);

    config["port"] = json!(port);
    config["host"] = json!("127.0.0.1");

    let engine = Arc::new(Engine::new());
    let worker = WorkerManager::create(engine.clone(), Some(config))
        .await
        .expect("create WorkerManager");

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    worker
        .start_background_tasks(shutdown_rx, shutdown_tx)
        .await
        .expect("start WorkerManager");

    tokio::time::sleep(Duration::from_millis(150)).await;

    (port, engine)
}

async fn wait_for_workers(engine: &Engine, expected: usize, timeout: Duration) -> bool {
    tokio::time::timeout(timeout, async {
        loop {
            if engine.worker_registry.list_workers().len() == expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .is_ok()
}

#[tokio::test]
async fn unresponsive_connection_is_closed_when_ping_enabled() {
    let (port, engine) = spawn_engine(json!({ "ping_interval_secs": 1 })).await;

    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/", port))
        .await
        .expect("connect");

    assert!(
        wait_for_workers(&engine, 1, Duration::from_millis(500)).await,
        "connection should register a worker"
    );

    // Never read from the socket: tungstenite only replies to pings while the
    // stream is being polled, so this client stays silent like a dead peer.
    assert!(
        wait_for_workers(&engine, 0, Duration::from_secs(4)).await,
        "silent connection should be reaped after two ping intervals"
    );

    drop(ws);
}

#[tokio::test]
async fn responsive_connection_stays_open_when_ping_enabled() {
    let (port, engine) = spawn_engine(json!({ "ping_interval_secs": 1 })).await;

    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/", port))
        .await
        .expect("connect");

    assert!(
        wait_for_workers(&engine, 1, Duration::from_millis(500)).await,
        "connection should register a worker"
    );

    // Poll the stream continuously: tungstenite answers server pings with
    // pongs automatically, which is what browsers do per RFC 6455.
    let reader = tokio::spawn(async move {
        let mut ws = ws;
        while let Some(msg) = ws.next().await {
            if msg.is_err() {
                break;
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(3500)).await;
    assert_eq!(
        engine.worker_registry.list_workers().len(),
        1,
        "responsive connection must survive multiple ping intervals"
    );

    reader.abort();
}

#[tokio::test]
async fn no_pings_and_no_reaping_when_disabled() {
    let (port, engine) = spawn_engine(json!({})).await;

    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/", port))
        .await
        .expect("connect");

    assert!(
        wait_for_workers(&engine, 1, Duration::from_millis(500)).await,
        "connection should register a worker"
    );

    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        engine.worker_registry.list_workers().len(),
        1,
        "without ping_interval_secs a silent connection must stay registered"
    );

    drop(ws);
}
