// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end test for the `iii-queue` ↔ `configuration` worker integration:
//! seed-on-first-boot, no-clobber across worker restarts, hot-add of a queue
//! (live consumer), hot-change of per-queue settings, full adapter/transport
//! hot-swap, all-or-nothing gate on an invalid value, and `${VAR:default}`
//! expansion.
//!
//! Modeled on `engine/tests/http_configuration_e2e.rs` — composes the two
//! workers against a real `FsAdapter` on a `tempfile::tempdir()`. The queue
//! uses the in-process `builtin` adapter, so a hot-added or rebound consumer is
//! observed by enqueuing a message and watching the target function fire.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use iii::engine::{Engine, EngineTrait};
use iii::function::FunctionResult;
use iii::workers::configuration::ConfigurationWorker;
use iii::workers::configuration::adapters::ConfigurationAdapter;
use iii::workers::configuration::adapters::fs::FsAdapter;
use iii::workers::configuration::structs::ConfigurationSetInput;
use iii::workers::queue::QueueWorker;
use iii::workers::traits::Worker;

use common::queue_helpers::{enqueue, register_counting_function};

/// Serializes the suite: tests share the process-global adapter registry
/// (`add_adapter`) and environment (`${VAR}` expansion test), so running one at
/// a time keeps those mutations from racing. `tokio::sync::Mutex` never
/// poisons, so a panicking test still releases it cleanly.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Harness {
    engine: Arc<Engine>,
    configuration: ConfigurationWorker,
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

    Harness {
        engine,
        configuration,
    }
}

/// Create, initialize, and start an `iii-queue` worker with the given seed.
async fn start_queue_worker(harness: &Harness, seed: Value) -> QueueWorker {
    let worker = QueueWorker::for_test(harness.engine.clone(), Some(seed))
        .await
        .expect("queue worker");
    worker.initialize().await.expect("queue initialize");
    Worker::register_functions(&worker, harness.engine.clone());
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    worker
        .start_background_tasks(shutdown_rx, shutdown_tx)
        .await
        .expect("queue start_background_tasks");
    worker
}

async fn set_value(harness: &Harness, value: Value) {
    let result = harness
        .configuration
        .set_fn(ConfigurationSetInput {
            id: "iii-queue".to_string(),
            value,
        })
        .await;
    match result {
        FunctionResult::Success(_) => {}
        FunctionResult::Failure(err) => panic!("configuration::set failed: {err:?}"),
        _ => panic!("unexpected configuration::set result"),
    }
}

/// Set a value expected to be rejected by the registered JSON schema at
/// `configuration::set` (the closed adapter union is the gate).
async fn set_value_expect_rejection(harness: &Harness, value: Value) {
    let result = harness
        .configuration
        .set_fn(ConfigurationSetInput {
            id: "iii-queue".to_string(),
            value,
        })
        .await;
    match result {
        FunctionResult::Failure(_) => {}
        FunctionResult::Success(_) => {
            panic!("expected configuration::set to be rejected, but it succeeded")
        }
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

#[tokio::test]
async fn first_boot_seeds_configuration_entry() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_queue_worker(
        &harness,
        json!({ "queue_configs": { "reports": { "concurrency": 4 } } }),
    )
    .await;

    let stored = harness
        .engine
        .call("configuration::get", json!({ "id": "iii-queue" }))
        .await
        .expect("configuration::get")
        .expect("get returns a body");
    assert_eq!(
        stored["value"]["queue_configs"]["reports"]["concurrency"],
        4
    );
    // The always-present built-in `default` queue is seeded too.
    assert!(
        stored["value"]["queue_configs"]["default"].is_object(),
        "default queue must be seeded: {stored}"
    );
}

#[tokio::test]
async fn runtime_edit_survives_worker_restart() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let seed = json!({ "queue_configs": { "default": { "concurrency": 1 } } });

    let worker = start_queue_worker(&harness, seed.clone()).await;

    set_value(
        &harness,
        json!({ "queue_configs": { "default": { "concurrency": 5 } } }),
    )
    .await;
    wait_for(
        || {
            worker
                .config_snapshot()
                .queue_configs
                .get("default")
                .map(|c| c.concurrency)
                == Some(5)
        },
        "runtime edit to apply",
    )
    .await;

    // Restart the queue worker with the same seed (ReloadManager semantics).
    worker.destroy().await.expect("destroy");
    let restarted = start_queue_worker(&harness, seed).await;

    // The runtime edit wins; the config.yaml seed must not clobber it.
    assert_eq!(
        restarted.config_snapshot().queue_configs["default"].concurrency,
        5
    );
}

#[tokio::test]
async fn hot_add_queue_starts_a_live_consumer() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let counter = Arc::new(AtomicU64::new(0));
    register_counting_function(&harness.engine, "e2e::jobs", counter.clone());

    // Boot with only the built-in default queue.
    let worker = start_queue_worker(&harness, json!({})).await;

    // Hot-add a `jobs` queue.
    set_value(
        &harness,
        json!({ "queue_configs": { "jobs": { "concurrency": 2, "max_retries": 1 } } }),
    )
    .await;
    wait_for(
        || worker.config_snapshot().queue_configs.contains_key("jobs"),
        "jobs queue to appear in the live config",
    )
    .await;

    // A consumer for the new queue must be live: enqueue a job and watch the
    // target function fire.
    enqueue(&worker, "jobs", "e2e::jobs", json!({ "x": 1 }))
        .await
        .expect("enqueue to hot-added queue");
    wait_for(
        || counter.load(Ordering::SeqCst) >= 1,
        "handler invoked for the hot-added queue",
    )
    .await;
}

#[tokio::test]
async fn hot_change_concurrency_keeps_queue_live() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let counter = Arc::new(AtomicU64::new(0));
    register_counting_function(&harness.engine, "e2e::smoke", counter.clone());

    let worker = start_queue_worker(
        &harness,
        json!({ "queue_configs": { "default": { "concurrency": 1 } } }),
    )
    .await;

    set_value(
        &harness,
        json!({ "queue_configs": { "default": { "concurrency": 4 } } }),
    )
    .await;
    wait_for(
        || worker.config_snapshot().queue_configs["default"].concurrency == 4,
        "concurrency change to apply",
    )
    .await;

    // The default queue's consumer was restarted with the new settings and is
    // still live.
    enqueue(&worker, "default", "e2e::smoke", json!({}))
        .await
        .expect("enqueue after concurrency change");
    wait_for(
        || counter.load(Ordering::SeqCst) >= 1,
        "handler invoked after concurrency change",
    )
    .await;
}

#[tokio::test]
async fn adapter_hot_swap_rebinds_consumers() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    // Swap to the `memory` transport: a `test-adapters`-gated in-process backend
    // that aliases `builtin`, so the swap is a real transport rebuild without an
    // external broker. It is advertised in the schema (under the same feature),
    // so `configuration::set` accepts it.
    let counter = Arc::new(AtomicU64::new(0));
    register_counting_function(&harness.engine, "e2e::swap", counter.clone());

    let worker = start_queue_worker(
        &harness,
        json!({ "queue_configs": { "jobs": { "concurrency": 1 } } }),
    )
    .await;
    // Capture the live transport instance before the swap.
    let old_adapter = worker.adapter_snapshot();

    // Works on the original (builtin) transport.
    enqueue(&worker, "jobs", "e2e::swap", json!({}))
        .await
        .expect("enqueue on builtin");
    wait_for(
        || counter.load(Ordering::SeqCst) >= 1,
        "handler on the builtin transport",
    )
    .await;

    // Swap the transport.
    set_value(
        &harness,
        json!({
            "adapter": { "name": "memory" },
            "queue_configs": { "jobs": { "concurrency": 1 } }
        }),
    )
    .await;
    wait_for(
        || {
            worker
                .config_snapshot()
                .adapter
                .as_ref()
                .map(|a| a.name.as_str() == "memory")
                .unwrap_or(false)
        },
        "transport to hot-swap",
    )
    .await;

    // The transport instance was actually rebuilt — not a half-swap where only
    // the config name changed. A pointer-identity check rules out the no-op.
    assert!(
        !Arc::ptr_eq(&old_adapter, &worker.adapter_snapshot()),
        "adapter instance must be rebuilt on a transport hot-swap"
    );

    // Consumers were rebound on the new transport: a job enqueued after the
    // swap is delivered.
    let before = counter.load(Ordering::SeqCst);
    enqueue(&worker, "jobs", "e2e::swap", json!({}))
        .await
        .expect("enqueue on the new transport");
    wait_for(
        || counter.load(Ordering::SeqCst) > before,
        "handler on the new transport",
    )
    .await;
}

#[tokio::test]
async fn unbuildable_adapter_keeps_previous_transport() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let counter = Arc::new(AtomicU64::new(0));
    register_counting_function(&harness.engine, "e2e::swap_fail", counter.clone());

    let worker = start_queue_worker(
        &harness,
        json!({ "queue_configs": { "jobs": { "concurrency": 1 } } }),
    )
    .await;
    // Capture the live transport instance before the failed swap.
    let old_adapter = worker.adapter_snapshot();

    // Point the transport at `redis` with a dead URL: schema-valid (an advertised
    // adapter) and past `validate()`, so it reaches the transport-swap branch
    // where `resolve_adapter` fails to *build* (connection refused). That failure
    // must keep the previous config, adapter, and consumers — the all-or-nothing
    // guarantee for a transport swap (`resolve_adapter().await?` returns before
    // `set_live`).
    set_value(
        &harness,
        json!({
            "adapter": { "name": "redis", "config": { "redis_url": "redis://127.0.0.1:6390" } },
            "queue_configs": { "jobs": { "concurrency": 1 } }
        }),
    )
    .await;

    // Drive the handler synchronously so the assertion observes a completed apply
    // attempt. The build fails deterministically, so neither this call nor the
    // trigger-fired apply can mutate state — they don't race.
    let _ = harness
        .engine
        .call("iii-queue::on-config-change", json!({}))
        .await
        .expect("on-config-change call");

    // The transport instance was NOT rebuilt — pointer identity is preserved.
    assert!(
        Arc::ptr_eq(&old_adapter, &worker.adapter_snapshot()),
        "an unbuildable adapter must retain the previous transport"
    );
    // The unbuildable adapter was not adopted into the live config.
    assert!(
        worker.config_snapshot().adapter.is_none(),
        "an unbuildable adapter must not be adopted into the live config"
    );

    // Consumers on the original transport are intact: a job is still delivered.
    enqueue(&worker, "jobs", "e2e::swap_fail", json!({}))
        .await
        .expect("enqueue on the retained transport");
    wait_for(
        || counter.load(Ordering::SeqCst) >= 1,
        "handler on the retained transport",
    )
    .await;
}

#[tokio::test]
async fn unknown_adapter_name_is_rejected_at_set() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_queue_worker(
        &harness,
        json!({ "queue_configs": { "jobs": { "concurrency": 1 } } }),
    )
    .await;

    // The closed adapter union rejects an unknown transport name at the schema
    // gate, before it can ever reach the worker — so a typo can't strand the
    // queue on an unbuildable transport.
    set_value_expect_rejection(&harness, json!({ "adapter": { "name": "does_not_exist" } })).await;
}

#[tokio::test]
async fn hot_remove_queue_drops_it_from_live_config() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_queue_worker(
        &harness,
        json!({ "queue_configs": { "jobs": { "concurrency": 1 } } }),
    )
    .await;
    assert!(worker.config_snapshot().queue_configs.contains_key("jobs"));

    // Remove `jobs` — only the always-present `default` should remain. This
    // exercises the apply_config removal branch (abort + drop from the map).
    set_value(&harness, json!({ "queue_configs": {} })).await;
    wait_for(
        || !worker.config_snapshot().queue_configs.contains_key("jobs"),
        "jobs queue to be removed from the live config",
    )
    .await;

    // The removal took effect: enqueueing to the gone queue now fails.
    let result = enqueue(&worker, "jobs", "e2e::gone", json!({})).await;
    assert!(
        result.is_err(),
        "enqueue to a removed queue must fail (consumer stopped, queue absent)"
    );
    // `default` survives every edit (ensure_default_queue in fetch_config).
    assert!(
        worker
            .config_snapshot()
            .queue_configs
            .contains_key("default")
    );
}

#[tokio::test]
async fn invalid_value_keeps_previous_config() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_queue_worker(
        &harness,
        json!({ "queue_configs": { "default": { "concurrency": 3 } } }),
    )
    .await;

    // A FIFO queue without `message_group_field` is schema-valid (the schema
    // can't express the cross-field rule) but fails the worker's `validate()`
    // gate. The configuration worker accepts the value; the worker must reject
    // applying it and keep the previous config.
    set_value(
        &harness,
        json!({ "queue_configs": { "orders": { "type": "fifo" } } }),
    )
    .await;

    // Drive the handler synchronously so the assertion observes a completed
    // apply attempt. The value fails validate() deterministically, so neither
    // this call nor the trigger-fired apply can mutate state — they don't race.
    let _ = harness
        .engine
        .call("iii-queue::on-config-change", json!({}))
        .await
        .expect("on-config-change call");

    let config = worker.config_snapshot();
    assert!(
        config.queue_configs.contains_key("default"),
        "previous config must be kept after an invalid value"
    );
    assert!(
        !config.queue_configs.contains_key("orders"),
        "an invalid value must not be applied"
    );
}

#[tokio::test]
async fn boot_with_invalid_stored_value_falls_back_to_seed() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let seed = json!({ "queue_configs": { "default": { "concurrency": 2 } } });

    // First boot registers the entry. Then store a value the schema accepts but
    // validate() rejects (fifo without message_group_field).
    let worker = start_queue_worker(&harness, seed.clone()).await;
    set_value(
        &harness,
        json!({ "queue_configs": { "orders": { "type": "fifo" } } }),
    )
    .await;
    worker.destroy().await.expect("destroy");

    // Restart: the boot fetch reads the invalid stored value and must fall back
    // to the validated seed instead of booting the broken config.
    let restarted = start_queue_worker(&harness, seed).await;
    let cfg = restarted.config_snapshot();
    assert!(
        !cfg.queue_configs.contains_key("orders"),
        "an invalid stored value must not be adopted at boot"
    );
    assert_eq!(
        cfg.queue_configs["default"].concurrency, 2,
        "boot must fall back to the validated seed on an invalid stored value"
    );
}

#[tokio::test]
async fn apply_after_destroy_does_not_respawn() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_queue_worker(
        &harness,
        json!({ "queue_configs": { "default": { "concurrency": 1 } } }),
    )
    .await;

    worker.destroy().await.expect("destroy");

    // A configuration event delivered after teardown (here driven synchronously,
    // standing in for the detached retry / in-flight trigger) must hit the
    // `destroyed` gate and bail — never re-spawn consumers on a dead worker.
    set_value(
        &harness,
        json!({ "queue_configs": { "jobs": { "concurrency": 1 } } }),
    )
    .await;
    let _ = harness
        .engine
        .call("iii-queue::on-config-change", json!({}))
        .await
        .expect("on-config-change call");

    assert!(
        !worker.config_snapshot().queue_configs.contains_key("jobs"),
        "apply_config must bail after destroy() instead of applying the edit"
    );
}

#[tokio::test]
async fn env_placeholder_expands_on_read() {
    let _serial = SERIAL.lock().await;
    // Scrub ambient state so the `${VAR:default}` default branch is what we
    // observe. remove_var is unsafe in edition 2024 because concurrent env
    // access is UB; SERIAL guarantees we hold the only handle.
    unsafe { std::env::remove_var("QUEUE_CFG_E2E_FIELD") };

    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_queue_worker(
        &harness,
        json!({
            "queue_configs": {
                "orders": {
                    "type": "fifo",
                    "message_group_field": "${QUEUE_CFG_E2E_FIELD:session_id}"
                }
            }
        }),
    )
    .await;

    // The live config sees the expanded default.
    wait_for(
        || {
            worker
                .config_snapshot()
                .queue_configs
                .get("orders")
                .and_then(|c| c.message_group_field.clone())
                == Some("session_id".to_string())
        },
        "message_group_field placeholder to expand",
    )
    .await;

    // The stored value keeps the placeholder verbatim.
    let raw = harness
        .engine
        .call(
            "configuration::get",
            json!({ "id": "iii-queue", "raw": true }),
        )
        .await
        .expect("configuration::get raw")
        .expect("get returns a body");
    assert_eq!(
        raw["value"]["queue_configs"]["orders"]["message_group_field"],
        "${QUEUE_CFG_E2E_FIELD:session_id}"
    );
}
