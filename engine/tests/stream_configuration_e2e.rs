// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end test for the `iii-stream` ↔ `configuration` worker integration:
//! seed-on-first-boot, no-clobber across worker restarts, an `auth_function`
//! hot-apply with no rebind, a full pub/sub adapter hot-swap, a host/port
//! rebind of the live WebSocket listener, the strict gates that keep the
//! previous adapter/server when a stored value cannot be resolved or bound, and
//! `${VAR:default}` expansion on read.
//!
//! Modeled on `engine/tests/http_configuration_e2e.rs` — composes the two
//! workers against a real `FsAdapter` on a `tempfile::tempdir()`. No engine
//! boot, no client WebSocket, no subprocess.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use iii::engine::{Engine, EngineTrait};
use iii::function::FunctionResult;
use iii::workers::configuration::ConfigurationWorker;
use iii::workers::configuration::adapters::ConfigurationAdapter;
use iii::workers::configuration::adapters::fs::FsAdapter;
use iii::workers::configuration::structs::ConfigurationSetInput;
use iii::workers::stream::StreamWorker;
use iii::workers::traits::Worker;

const CONFIG_ID: &str = "iii-stream";

struct Harness {
    engine: Arc<Engine>,
    configuration: ConfigurationWorker,
    // Keep the shutdown channel alive for the worker lifecycle: dropping the
    // sender would gracefully stop the stream server task.
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

/// Create, initialize, and start an `iii-stream` worker with the given seed.
async fn start_stream_worker(harness: &Harness, seed: Value) -> StreamWorker {
    let worker = StreamWorker::for_test(harness.engine.clone(), Some(seed))
        .await
        .expect("stream worker");
    worker.initialize().await.expect("stream initialize");
    Worker::register_functions(&worker, harness.engine.clone());
    worker
        .start_background_tasks(harness.shutdown_rx.clone(), harness.shutdown_tx.clone())
        .await
        .expect("stream start_background_tasks");
    worker
}

async fn set_value(harness: &Harness, value: Value) {
    let result = harness
        .configuration
        .set_fn(ConfigurationSetInput {
            id: CONFIG_ID.to_string(),
            value,
        })
        .await;
    match result {
        FunctionResult::Success(_) => {}
        FunctionResult::Failure(err) => panic!("configuration::set failed: {err:?}"),
        _ => panic!("unexpected configuration::set result"),
    }
}

/// Assert `configuration::set` rejects a value against the closed adapter schema
/// with `SCHEMA_INVALID` (the bus guard that keeps an unknown adapter name or a
/// stray config key from ever reaching the worker).
async fn set_value_expect_rejection(harness: &Harness, value: Value) {
    let result = harness
        .configuration
        .set_fn(ConfigurationSetInput {
            id: CONFIG_ID.to_string(),
            value: value.clone(),
        })
        .await;
    match result {
        FunctionResult::Failure(err) => assert_eq!(
            err.code, "SCHEMA_INVALID",
            "expected schema rejection for {value}: {err:?}"
        ),
        FunctionResult::Success(_) => panic!("configuration::set must reject {value}"),
        _ => panic!("unexpected configuration::set result for {value}"),
    }
}

/// Invoke the config-change handler synchronously so assertions can't pass
/// vacuously before the (also async) trigger fan-out applies the change.
async fn drive_apply(harness: &Harness) {
    harness
        .engine
        .call("iii-stream::on-config-change", json!({}))
        .await
        .expect("config-change handler is invocable");
}

async fn stored_value(harness: &Harness, raw: bool) -> Value {
    harness
        .engine
        .call(
            "configuration::get",
            json!({ "id": CONFIG_ID, "raw": raw }),
            None,
        )
        .await
        .expect("configuration::get")
        .expect("get returns a body")
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

async fn wait_for_async<F, Fut>(mut predicate: F, what: &str)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if predicate().await {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("timed out waiting for {what}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Serializes the whole suite so no two tests contend for TCP ports. The stream
/// worker binds a real listener in `start_background_tasks`; cargo runs these
/// `#[tokio::test]`s in parallel, so without this lock two tests could draw the
/// same just-freed ephemeral port and collide with `EADDRINUSE`. Each test
/// holds the lock for its whole lifetime. `tokio::sync::Mutex` never poisons, so
/// a panicking test still releases it cleanly.
static PORT_SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Reserve a free TCP port by binding to port 0 and dropping the listener.
/// Safe against cross-test reuse only while [`PORT_SERIAL`] is held.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral")
        .local_addr()
        .expect("local addr")
        .port()
}

#[tokio::test]
async fn first_boot_seeds_configuration_entry() {
    let _serial = PORT_SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_stream_worker(
        &harness,
        json!({ "host": "127.0.0.1", "port": 0, "auth_function": "auth::seeded" }),
    )
    .await;

    let stored = stored_value(&harness, false).await;
    assert_eq!(stored["id"], CONFIG_ID);
    assert_eq!(stored["value"]["host"], "127.0.0.1");
    assert_eq!(stored["value"]["port"], 0);
    assert_eq!(stored["value"]["auth_function"], "auth::seeded");
}

#[tokio::test]
async fn auth_function_hot_applies_without_rebind() {
    let _serial = PORT_SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_stream_worker(&harness, json!({ "host": "127.0.0.1", "port": 0 })).await;
    assert!(worker.config_snapshot().auth_function.is_none());
    let before = worker.adapter_snapshot();

    // Same address + same adapter: only `auth_function` changes, so the running
    // server picks it up per connection with no rebind and no adapter swap.
    set_value(
        &harness,
        json!({ "host": "127.0.0.1", "port": 0, "auth_function": "auth::live" }),
    )
    .await;

    wait_for(
        || worker.config_snapshot().auth_function.as_deref() == Some("auth::live"),
        "auth_function to hot-apply",
    )
    .await;
    assert!(
        Arc::ptr_eq(&before, &worker.adapter_snapshot()),
        "an auth_function-only change must not rebuild the adapter"
    );
}

#[tokio::test]
async fn runtime_edits_survive_worker_restart() {
    let _serial = PORT_SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let seed = json!({ "host": "127.0.0.1", "port": 0 });

    let worker = start_stream_worker(&harness, seed.clone()).await;

    // Operator repoints the adapter at runtime.
    set_value(
        &harness,
        json!({ "host": "127.0.0.1", "port": 0, "adapter": { "name": "kv", "config": { "channel_size": 128 } } }),
    )
    .await;
    drive_apply(&harness).await;
    wait_for(
        || {
            worker
                .config_snapshot()
                .adapter
                .as_ref()
                .and_then(|a| a.config.as_ref())
                .and_then(|c| c["channel_size"].as_u64())
                == Some(128)
        },
        "adapter edit to apply",
    )
    .await;

    // "Restart": a fresh worker with a different seed must NOT clobber the
    // stored value, and must adopt it as the live config (the boot fetch makes
    // the persisted value the source of truth). The adapter instance rebuild is
    // covered separately by `boot_rebuilds_adapter_from_persisted_value`.
    worker.destroy().await.expect("destroy");
    let restarted = start_stream_worker(
        &harness,
        json!({ "host": "127.0.0.1", "port": 0, "adapter": { "name": "kv" } }),
    )
    .await;

    assert_eq!(
        restarted
            .config_snapshot()
            .adapter
            .as_ref()
            .and_then(|a| a.config.as_ref())
            .and_then(|c| c["channel_size"].as_u64()),
        Some(128),
        "restarted worker must adopt the persisted adapter, not its seed"
    );
}

#[tokio::test]
async fn boot_rebuilds_adapter_from_persisted_value() {
    // Observes the boot adapter-adoption path directly (not via config_snapshot,
    // which the boot fetch sets regardless): the seed-built adapter instance
    // must be REPLACED at boot when the persisted value selects a different
    // effective adapter. Asserting `Arc` identity makes this non-vacuous — a
    // regression that drops boot adoption would leave the seed adapter in place
    // and fail here.
    let _serial = PORT_SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    // First boot seeds + registers, then persist an adapter differing from the
    // restart worker's seed.
    let first = start_stream_worker(&harness, json!({ "host": "127.0.0.1", "port": 0 })).await;
    set_value(
        &harness,
        json!({ "host": "127.0.0.1", "port": 0, "adapter": { "name": "kv", "config": { "channel_size": 200 } } }),
    )
    .await;
    first.destroy().await.expect("destroy");

    // Restart with a seed whose effective adapter (kv, no config) differs from
    // the persisted one, so boot adoption must rebuild the backend.
    let restarted = StreamWorker::for_test(
        harness.engine.clone(),
        Some(json!({ "host": "127.0.0.1", "port": 0 })),
    )
    .await
    .expect("stream worker");
    let seed_adapter = restarted.adapter_snapshot();
    restarted.initialize().await.expect("initialize");
    Worker::register_functions(&restarted, harness.engine.clone());
    restarted
        .start_background_tasks(harness.shutdown_rx.clone(), harness.shutdown_tx.clone())
        .await
        .expect("start_background_tasks");

    assert!(
        !Arc::ptr_eq(&seed_adapter, &restarted.adapter_snapshot()),
        "boot adoption must rebuild the adapter from the persisted value, not keep the seed instance"
    );
}

#[tokio::test]
async fn boot_resolve_failure_reconciles_config_to_served_adapter() {
    // When the persisted config selects an unresolvable adapter, boot keeps the
    // seed-built backend AND reverts the live config's adapter field to match,
    // so config_snapshot() never advertises a backend that is not running (and a
    // future no-op apply comparison can't treat the bad adapter as applied).
    let _serial = PORT_SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    // First boot seeds + persists a valid `iii-stream` entry to disk.
    let first = start_stream_worker(&harness, json!({ "host": "127.0.0.1", "port": 0 })).await;
    first.destroy().await.expect("destroy");

    // Hand-edit the persisted yaml so its stored adapter is an unresolvable name.
    // This bypasses `configuration::set`'s closed-schema guard exactly as a
    // manual edit of the persisted file would; deserialization stays lenient, so
    // the bad value loads on the next prime.
    let entry_path = dir.path().join("iii-stream.yaml");
    let mut entry: serde_yaml::Value =
        serde_yaml::from_str(&std::fs::read_to_string(&entry_path).expect("read persisted entry"))
            .expect("parse persisted entry");
    entry
        .get_mut("value")
        .and_then(|v| v.as_mapping_mut())
        .expect("persisted value mapping")
        .insert(
            serde_yaml::Value::String("adapter".to_string()),
            serde_yaml::from_str("name: does-not-exist").unwrap(),
        );
    std::fs::write(&entry_path, serde_yaml::to_string(&entry).unwrap())
        .expect("write hand-edited entry");

    // A fresh harness re-primes the configuration store from disk, loading the
    // unresolvable adapter that the bus would never have accepted.
    let harness = build_harness(dir.path()).await;

    let restarted = StreamWorker::for_test(
        harness.engine.clone(),
        Some(json!({ "host": "127.0.0.1", "port": 0 })),
    )
    .await
    .expect("stream worker");
    let seed_adapter = restarted.adapter_snapshot();
    restarted.initialize().await.expect("initialize");
    Worker::register_functions(&restarted, harness.engine.clone());
    restarted
        .start_background_tasks(harness.shutdown_rx.clone(), harness.shutdown_tx.clone())
        .await
        .expect("start_background_tasks");

    assert!(
        Arc::ptr_eq(&seed_adapter, &restarted.adapter_snapshot()),
        "an unresolvable boot adapter must keep the seed-built backend"
    );
    assert!(
        restarted.config_snapshot().adapter.is_none(),
        "the live config adapter must be reconciled to the served (seed) backend, not the unresolved stored value"
    );
}

#[tokio::test]
async fn adapter_hot_swap_rebuilds_backend() {
    let _serial = PORT_SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_stream_worker(&harness, json!({ "host": "127.0.0.1", "port": 0 })).await;
    let before = worker.adapter_snapshot();

    // A distinguishing adapter config flips the effective adapter, forcing the
    // full backend hot-swap path. Address is unchanged, so no rebind.
    set_value(
        &harness,
        json!({ "host": "127.0.0.1", "port": 0, "adapter": { "name": "kv", "config": { "channel_size": 64 } } }),
    )
    .await;
    drive_apply(&harness).await;

    let after = worker.adapter_snapshot();
    assert!(
        !Arc::ptr_eq(&before, &after),
        "an adapter change must rebuild the backend instance"
    );
    assert_eq!(
        worker
            .config_snapshot()
            .adapter
            .as_ref()
            .and_then(|a| a.config.as_ref())
            .and_then(|c| c["channel_size"].as_u64()),
        Some(64),
        "the live config must reflect the applied adapter"
    );
}

#[tokio::test]
async fn set_rejects_unknown_adapter_and_stray_config() {
    // The closed per-adapter schema guards `configuration::set`: an unknown
    // adapter name or a config key outside the chosen adapter's schema is
    // rejected at the bus, so the worker never has to resolve a backend that
    // can't exist. (The worker's defensive keep-previous behavior for a
    // hand-edited persisted file that bypasses this guard is covered by the
    // `configuration.rs` unit tests.)
    let _serial = PORT_SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_stream_worker(&harness, json!({ "host": "127.0.0.1", "port": 0 })).await;

    // Unknown adapter name.
    set_value_expect_rejection(
        &harness,
        json!({ "host": "127.0.0.1", "port": 0, "adapter": { "name": "does-not-exist" } }),
    )
    .await;
    // Stray config key for a known adapter (kv reads store_method/file_path/
    // save_interval_ms/channel_size).
    set_value_expect_rejection(
        &harness,
        json!({ "host": "127.0.0.1", "port": 0, "adapter": { "name": "kv", "config": { "bogus": 1 } } }),
    )
    .await;
    // A valid known adapter is still accepted.
    set_value(
        &harness,
        json!({ "host": "127.0.0.1", "port": 0, "adapter": { "name": "redis", "config": { "redis_url": "redis://localhost:6379" } } }),
    )
    .await;
}

#[tokio::test]
async fn port_change_rebinds_the_listener() {
    let _serial = PORT_SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let port_a = free_port();
    let port_b = free_port();
    assert_ne!(port_a, port_b);

    let _worker =
        start_stream_worker(&harness, json!({ "host": "127.0.0.1", "port": port_a })).await;
    tokio::net::TcpStream::connect(("127.0.0.1", port_a))
        .await
        .expect("initial port accepts connections");

    set_value(&harness, json!({ "host": "127.0.0.1", "port": port_b })).await;

    wait_for_async(
        || async {
            tokio::net::TcpStream::connect(("127.0.0.1", port_b))
                .await
                .is_ok()
        },
        "rebind to the new port",
    )
    .await;

    // The old listener is torn down once the new one is live.
    wait_for_async(
        || async {
            tokio::net::TcpStream::connect(("127.0.0.1", port_a))
                .await
                .is_err()
        },
        "old listener to release the previous port",
    )
    .await;
}

#[tokio::test]
async fn failed_rebind_keeps_previous_server() {
    let _serial = PORT_SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let port_a = free_port();
    let port_b = free_port();
    assert_ne!(port_a, port_b);

    let worker =
        start_stream_worker(&harness, json!({ "host": "127.0.0.1", "port": port_a })).await;
    tokio::net::TcpStream::connect(("127.0.0.1", port_a))
        .await
        .expect("initial port accepts connections");

    // Occupy port_b so the rebind's bind() fails. Hold it for the rest of the
    // test.
    let _blocker = std::net::TcpListener::bind(("127.0.0.1", port_b)).expect("occupy port_b");

    set_value(&harness, json!({ "host": "127.0.0.1", "port": port_b })).await;
    drive_apply(&harness).await;

    // Old server still serving on port_a; the live config was not mutated.
    tokio::net::TcpStream::connect(("127.0.0.1", port_a))
        .await
        .expect("old port still accepts after failed rebind");
    assert_eq!(worker.config_snapshot().port, port_a);
}

#[tokio::test]
async fn restart_falls_back_to_seed_when_stored_address_unbindable() {
    let _serial = PORT_SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let port_a = free_port();
    let port_b = free_port();
    assert_ne!(port_a, port_b);
    let seed = json!({ "host": "127.0.0.1", "port": port_a });

    let worker = start_stream_worker(&harness, seed.clone()).await;

    // Occupy port_b for the whole test, then persist an edit pointing at it.
    // The hot rebind fails (all-or-nothing), but the bad value is now stored.
    let _blocker = std::net::TcpListener::bind(("127.0.0.1", port_b)).expect("occupy port_b");
    set_value(&harness, json!({ "host": "127.0.0.1", "port": port_b })).await;
    drive_apply(&harness).await;

    // Restart (ReloadManager semantics). The boot fetch returns the stored
    // unbindable address; the worker must fall back to the seed instead of
    // failing to start — a bad runtime edit must not become a stream outage.
    worker.destroy().await.expect("destroy");
    wait_for_async(
        || async {
            tokio::net::TcpStream::connect(("127.0.0.1", port_a))
                .await
                .is_err()
        },
        "old listener to release port_a",
    )
    .await;
    let restarted = start_stream_worker(&harness, seed).await;

    tokio::net::TcpStream::connect(("127.0.0.1", port_a))
        .await
        .expect("restarted server falls back to the seed address");
    assert_eq!(restarted.config_snapshot().port, port_a);
}

#[tokio::test]
async fn restart_refuses_seed_fallback_that_widens_loopback() {
    let _serial = PORT_SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let port_a = free_port();
    let port_b = free_port();
    assert_ne!(port_a, port_b);
    // Non-loopback seed; the runtime edit restricts the listener to loopback.
    let seed = json!({ "host": "0.0.0.0", "port": port_a });

    let worker = start_stream_worker(&harness, seed.clone()).await;

    let _blocker = std::net::TcpListener::bind(("127.0.0.1", port_b)).expect("occupy port_b");
    set_value(&harness, json!({ "host": "127.0.0.1", "port": port_b })).await;
    drive_apply(&harness).await;

    worker.destroy().await.expect("destroy");
    wait_for_async(
        || async {
            tokio::net::TcpStream::connect(("127.0.0.1", port_a))
                .await
                .is_err()
        },
        "old listener to release port_a",
    )
    .await;

    // Restart: the stored loopback address cannot bind, and the 0.0.0.0 seed
    // would WIDEN the listen surface — the worker must refuse and fail to start
    // rather than silently exposing every interface.
    let restarted = StreamWorker::for_test(harness.engine.clone(), Some(seed))
        .await
        .expect("stream worker");
    restarted.initialize().await.expect("initialize");
    Worker::register_functions(&restarted, harness.engine.clone());
    let result = restarted
        .start_background_tasks(harness.shutdown_rx.clone(), harness.shutdown_tx.clone())
        .await;
    assert!(
        result.is_err(),
        "loopback-widening fallback must be refused, got: {result:?}"
    );
}

#[tokio::test]
async fn env_placeholders_expand_on_read() {
    let _serial = PORT_SERIAL.lock().await;
    // Scrub the var so the `${VAR:default}` default branch is what we exercise.
    // SAFETY: runs before the harness spawns any task; remove_var is unsafe in
    // edition 2024 because concurrent env access is UB.
    unsafe { std::env::remove_var("STREAM_CFG_E2E_HOST") };

    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_stream_worker(
        &harness,
        json!({ "host": "${STREAM_CFG_E2E_HOST:127.0.0.1}", "port": 0 }),
    )
    .await;

    // The live snapshot sees the expanded value.
    assert_eq!(worker.config_snapshot().host, "127.0.0.1");

    // The stored value keeps the placeholder verbatim (raw read).
    let raw = stored_value(&harness, true).await;
    assert_eq!(
        raw["value"]["host"], "${STREAM_CFG_E2E_HOST:127.0.0.1}",
        "the persisted value must retain the placeholder for re-expansion"
    );
}
