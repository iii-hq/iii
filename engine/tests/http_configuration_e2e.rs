// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end test for the `iii-http` ↔ `configuration` worker integration:
//! seed-on-first-boot, no-clobber across worker restarts, hot apply of
//! router-level fields, host/port rebind, and `${VAR:default}` expansion.
//!
//! Modeled on `engine/tests/configuration_e2e.rs` — composes the two workers
//! against a real `FsAdapter` on a `tempfile::tempdir()`. No engine boot, no
//! WebSocket, no subprocess.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use iii::engine::{Engine, EngineTrait};
use iii::function::FunctionResult;
use iii::workers::configuration::ConfigurationWorker;
use iii::workers::configuration::adapters::ConfigurationAdapter;
use iii::workers::configuration::adapters::fs::FsAdapter;
use iii::workers::configuration::structs::ConfigurationSetInput;
use iii::workers::rest_api::HttpWorker;
use iii::workers::traits::Worker;

struct Harness {
    engine: Arc<Engine>,
    configuration: ConfigurationWorker,
    // Keep the shutdown sender alive: dropping it would gracefully stop the
    // HTTP server task.
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

async fn build_harness(dir: &std::path::Path) -> Harness {
    iii::workers::observability::metrics::ensure_default_meter();
    let adapter = Arc::new(
        FsAdapter::new(Some(json!({ "directory": dir.to_str().unwrap() })))
            .await
            .expect("fs adapter"),
    ) as Arc<dyn ConfigurationAdapter>;
    let engine = Arc::new(Engine::new());

    let configuration = ConfigurationWorker::for_test(engine.clone(), adapter, 0);
    configuration
        .initialize()
        .await
        .expect("configuration initialize");
    Worker::register_functions(&configuration, engine.clone());

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    Harness {
        engine,
        configuration,
        shutdown_tx,
        shutdown_rx,
    }
}

/// Create, initialize, and start an `iii-http` worker with the given seed.
async fn start_http_worker(harness: &Harness, seed: Value) -> HttpWorker {
    let worker = HttpWorker::for_test(harness.engine.clone(), Some(seed)).expect("http worker");
    worker.initialize().await.expect("http initialize");
    Worker::register_functions(&worker, harness.engine.clone());
    worker
        .start_background_tasks(harness.shutdown_rx.clone(), harness.shutdown_tx.clone())
        .await
        .expect("http start_background_tasks");
    worker
}

async fn set_value(harness: &Harness, value: Value) {
    let result = harness
        .configuration
        .set_fn(ConfigurationSetInput {
            id: "iii-http".to_string(),
            value,
        })
        .await;
    match result {
        FunctionResult::Success(_) => {}
        FunctionResult::Failure(err) => panic!("configuration::set failed: {err:?}"),
        _ => panic!("unexpected configuration::set result"),
    }
}

/// Poll until `predicate` returns true or the deadline elapses. Trigger
/// fan-out is spawned, so observable effects are eventually consistent.
async fn wait_for(mut predicate: impl FnMut() -> bool, what: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if predicate() {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("timed out waiting for {what}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Reserve a free TCP port by binding to port 0 and dropping the listener.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral")
        .local_addr()
        .expect("local addr")
        .port()
}

#[tokio::test]
async fn first_boot_seeds_configuration_entry() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_http_worker(
        &harness,
        json!({ "host": "127.0.0.1", "port": 0, "default_timeout": 7000 }),
    )
    .await;

    let stored = harness
        .engine
        .call("configuration::get", json!({ "id": "iii-http" }))
        .await
        .expect("configuration::get")
        .expect("get returns a body");
    assert_eq!(stored["value"]["port"], 0);
    assert_eq!(stored["value"]["host"], "127.0.0.1");
    assert_eq!(stored["value"]["default_timeout"], 7000);
}

#[tokio::test]
async fn updated_value_hot_applies_without_rebind() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_http_worker(&harness, json!({ "host": "127.0.0.1", "port": 0 })).await;
    assert_eq!(worker.config_snapshot().default_timeout, 30000);

    set_value(
        &harness,
        json!({ "host": "127.0.0.1", "port": 0, "default_timeout": 1234 }),
    )
    .await;

    wait_for(
        || worker.config_snapshot().default_timeout == 1234,
        "default_timeout to hot-apply",
    )
    .await;
}

#[tokio::test]
async fn runtime_edits_survive_worker_restart() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let seed = json!({ "host": "127.0.0.1", "port": 0, "default_timeout": 7000 });

    let worker = start_http_worker(&harness, seed.clone()).await;

    set_value(
        &harness,
        json!({ "host": "127.0.0.1", "port": 0, "default_timeout": 4321 }),
    )
    .await;
    wait_for(
        || worker.config_snapshot().default_timeout == 4321,
        "runtime edit to apply",
    )
    .await;

    // Restart the HTTP worker with the same seed (ReloadManager semantics).
    worker.destroy().await.expect("destroy");
    let restarted = start_http_worker(&harness, seed).await;

    // The runtime edit wins; the config.yaml seed must not clobber it.
    assert_eq!(restarted.config_snapshot().default_timeout, 4321);
}

#[tokio::test]
async fn port_change_rebinds_the_listener() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let port_a = free_port();
    let port_b = free_port();
    assert_ne!(port_a, port_b);

    let _worker =
        start_http_worker(&harness, json!({ "host": "127.0.0.1", "port": port_a })).await;
    tokio::net::TcpStream::connect(("127.0.0.1", port_a))
        .await
        .expect("initial port accepts connections");

    set_value(&harness, json!({ "host": "127.0.0.1", "port": port_b })).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tokio::net::TcpStream::connect(("127.0.0.1", port_b))
            .await
            .is_ok()
        {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("timed out waiting for rebind to port {port_b}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    // The old listener is torn down once the new one is live.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tokio::net::TcpStream::connect(("127.0.0.1", port_a))
            .await
            .is_err()
        {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("old port {port_a} still accepting after rebind");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn env_placeholders_expand_on_read() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    // HTTP_CFG_E2E_HOST is unset, so the default expands.
    let worker = start_http_worker(
        &harness,
        json!({ "host": "${HTTP_CFG_E2E_HOST:127.0.0.1}", "port": 0 }),
    )
    .await;

    assert_eq!(worker.config_snapshot().host, "127.0.0.1");

    // The stored value keeps the placeholder verbatim.
    let raw = harness
        .engine
        .call("configuration::get", json!({ "id": "iii-http", "raw": true }))
        .await
        .expect("configuration::get raw")
        .expect("get returns a body");
    assert_eq!(raw["value"]["host"], "${HTTP_CFG_E2E_HOST:127.0.0.1}");
}
