// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end test for the `iii-state` ↔ `configuration` worker integration:
//! seed-on-first-boot, no-clobber across worker restarts, hot apply of the
//! live knobs (`triggers_enabled`, `max_value_bytes`) and the `save_interval_ms`
//! task-rebuild tier, the restart-tier `adapter` change, and schema rejection
//! of out-of-range values.
//!
//! Modeled on `engine/tests/http_configuration_e2e.rs` — composes the two
//! workers against a real `FsAdapter` on a `tempfile::tempdir()`. No engine
//! boot, no WebSocket, no subprocess.

use std::sync::Arc;

use serde_json::{Value, json};

use iii::engine::{Engine, EngineTrait};
use iii::function::FunctionResult;
use iii::workers::configuration::ConfigurationWorker;
use iii::workers::configuration::adapters::ConfigurationAdapter;
use iii::workers::configuration::adapters::fs::FsAdapter;
use iii::workers::configuration::structs::ConfigurationSetInput;
use iii::workers::state::StateWorker;
use iii::workers::traits::Worker;

const CONFIG_ID: &str = "iii-state";

struct Harness {
    engine: Arc<Engine>,
    configuration: ConfigurationWorker,
    // Keep the shutdown sender alive for the worker lifecycle.
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

/// Create, initialize, and start an `iii-state` worker with the given seed.
async fn start_state_worker(harness: &Harness, seed: Value) -> StateWorker {
    let worker = StateWorker::for_test(harness.engine.clone(), Some(seed)).expect("state worker");
    worker.initialize().await.expect("state initialize");
    Worker::register_functions(&worker, harness.engine.clone());
    worker
        .start_background_tasks(harness.shutdown_rx.clone(), harness.shutdown_tx.clone())
        .await
        .expect("state start_background_tasks");
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
        .call("iii-state::on-config-change", json!({}))
        .await
        .expect("config-change handler is invocable");
}

async fn stored_value(harness: &Harness) -> Value {
    harness
        .engine
        .call("configuration::get", json!({ "id": CONFIG_ID }))
        .await
        .expect("configuration::get")
        .expect("get returns a body")
}

#[tokio::test]
async fn first_boot_seeds_configuration_entry() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_state_worker(&harness, json!({ "max_value_bytes": 256 })).await;

    let stored = stored_value(&harness).await;
    assert_eq!(stored["id"], CONFIG_ID);
    assert_eq!(stored["value"]["max_value_bytes"], 256);
}

#[tokio::test]
async fn runtime_edit_survives_worker_restart() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_state_worker(&harness, json!({})).await;

    // Operator edits the value at runtime.
    set_value(&harness, json!({ "max_value_bytes": 4096 })).await;

    // "Restart": a fresh worker with a different seed must NOT clobber the
    // stored value, and must adopt it as the runtime source of truth.
    let restarted = start_state_worker(&harness, json!({ "max_value_bytes": 256 })).await;

    let stored = stored_value(&harness).await;
    assert_eq!(
        stored["value"]["max_value_bytes"], 4096,
        "seed must not clobber the runtime-edited value"
    );
    assert_eq!(
        restarted.current_config().max_value_bytes,
        Some(4096),
        "restarted worker must adopt the persisted value, not its seed"
    );
}

#[tokio::test]
async fn max_value_bytes_hot_applies_and_rejects_oversized_writes() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_state_worker(&harness, json!({})).await;

    set_value(&harness, json!({ "max_value_bytes": 8 })).await;
    drive_apply(&harness).await;

    // A value over the live limit is rejected before reaching the adapter.
    let oversized = harness
        .engine
        .call(
            "state::set",
            json!({ "scope": "s", "key": "big", "value": "way more than eight bytes" }),
        )
        .await;
    let err = oversized.expect_err("oversized write must be rejected");
    assert_eq!(err.code, "VALUE_TOO_LARGE");

    // A value within the limit still writes.
    harness
        .engine
        .call(
            "state::set",
            json!({ "scope": "s", "key": "ok", "value": 1 }),
        )
        .await
        .expect("small write succeeds");
}

#[tokio::test]
async fn save_interval_ms_retune_updates_live_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_state_worker(&harness, json!({})).await;

    set_value(&harness, json!({ "save_interval_ms": 500 })).await;
    drive_apply(&harness).await;

    assert_eq!(worker.current_config().save_interval_ms, Some(500));
}

/// Poll until `dir` contains at least one entry, or panic after 5s.
async fn wait_for_persisted_file(dir: &std::path::Path) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let has_entry = std::fs::read_dir(dir)
            .map(|mut rd| rd.next().is_some())
            .unwrap_or(false);
        if has_entry {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("state was not persisted to disk after the save-loop retune");
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn save_interval_ms_retunes_a_file_backed_save_loop() {
    let cfg_dir = tempfile::tempdir().unwrap();
    let harness = build_harness(cfg_dir.path()).await;
    let store_dir = tempfile::tempdir().unwrap();

    let worker = start_state_worker(
        &harness,
        json!({
            "adapter": {
                "name": "kv",
                "config": {
                    "store_method": "file_based",
                    "file_path": store_dir.path().to_str().unwrap()
                }
            }
        }),
    )
    .await;

    // Retune the cadence at runtime; this must reach the real file-backed loop,
    // not no-op against an in-memory store.
    set_value(&harness, json!({ "save_interval_ms": 200 })).await;
    drive_apply(&harness).await;
    assert_eq!(worker.current_config().save_interval_ms, Some(200));

    // A write now flushes at the retuned cadence — proof the reconfigure drove a
    // live file-backed save loop end-to-end.
    harness
        .engine
        .call(
            "state::set",
            json!({ "scope": "s", "key": "k", "value": { "v": 1 } }),
        )
        .await
        .expect("state::set");
    wait_for_persisted_file(store_dir.path()).await;
}

#[tokio::test]
async fn triggers_enabled_hot_applies_to_live_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_state_worker(&harness, json!({})).await;

    set_value(&harness, json!({ "triggers_enabled": false })).await;
    drive_apply(&harness).await;

    assert_eq!(worker.current_config().triggers_enabled, Some(false));
}

#[tokio::test]
async fn restart_tier_adapter_change_updates_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_state_worker(&harness, json!({})).await;

    set_value(
        &harness,
        json!({ "adapter": { "name": "kv", "config": { "store_method": "in_memory" } } }),
    )
    .await;
    drive_apply(&harness).await;

    // Restart-tier: the live snapshot records it (warn logged) and the
    // persisted entry drives adapter construction at the next engine start.
    let adapter = worker
        .current_config()
        .adapter
        .expect("adapter recorded in the snapshot");
    assert_eq!(adapter.name, "kv");
}

#[tokio::test]
async fn schema_rejects_out_of_range_knobs() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_state_worker(&harness, json!({})).await;

    // max_value_bytes minimum is 1; 0 would reject every write.
    set_value_expect_rejection(&harness, json!({ "max_value_bytes": 0 })).await;
    // save_interval_ms minimum is 100; 10 would hammer the disk.
    set_value_expect_rejection(&harness, json!({ "save_interval_ms": 10 })).await;
}
