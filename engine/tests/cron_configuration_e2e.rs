// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end test for the `iii-cron` ↔ `configuration` worker integration:
//! seed-on-first-boot, no-clobber across worker restarts, a full lock-backend
//! hot-swap that rebinds every live cron job onto the new transport, the strict
//! gate that keeps the previous scheduler when a stored adapter cannot be
//! resolved, `${VAR:default}` expansion on read, and that a scheduled job
//! actually fires through the configured adapter.
//!
//! Modeled on `engine/tests/state_configuration_e2e.rs` — composes the two
//! workers against a real `FsAdapter` on a `tempfile::tempdir()`. No engine
//! boot, no WebSocket, no subprocess.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use iii::engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest};
use iii::function::FunctionResult;
use iii::trigger::{Trigger, TriggerRegistrator};
use iii::workers::configuration::ConfigurationWorker;
use iii::workers::configuration::adapters::ConfigurationAdapter;
use iii::workers::configuration::adapters::fs::FsAdapter;
use iii::workers::configuration::structs::ConfigurationSetInput;
use iii::workers::cron::CronWorker;
use iii::workers::traits::Worker;

const CONFIG_ID: &str = "iii-cron";

struct Harness {
    engine: Arc<Engine>,
    configuration: ConfigurationWorker,
    // Keep the shutdown channel alive for the worker lifecycle.
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

/// Create, initialize, and start an `iii-cron` worker with the given seed.
async fn start_cron_worker(harness: &Harness, seed: Value) -> CronWorker {
    let worker = CronWorker::for_test(harness.engine.clone(), Some(seed))
        .await
        .expect("cron worker");
    worker.initialize().await.expect("cron initialize");
    Worker::register_functions(&worker, harness.engine.clone());
    worker
        .start_background_tasks(harness.shutdown_rx.clone(), harness.shutdown_tx.clone())
        .await
        .expect("cron start_background_tasks");
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

/// Invoke the config-change handler synchronously so assertions can't pass
/// vacuously before the (also async) trigger fan-out applies the change.
async fn drive_apply(harness: &Harness) {
    harness
        .engine
        .call("iii-cron::on-config-change", json!({}))
        .await
        .expect("config-change handler is invocable");
}

async fn stored_value(harness: &Harness, raw: bool) -> Value {
    harness
        .engine
        .call("configuration::get", json!({ "id": CONFIG_ID, "raw": raw }))
        .await
        .expect("configuration::get")
        .expect("get returns a body")
}

/// Register an engine function that increments `counter` each time it is called.
fn register_counter(engine: &Arc<Engine>, function_id: &str, counter: Arc<AtomicU64>) {
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: function_id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(move |_input: Value| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                FunctionResult::Success(Some(json!({})))
            }
        }),
    );
}

fn cron_trigger(id: &str, function_id: &str, expression: &str) -> Trigger {
    Trigger {
        id: id.to_string(),
        trigger_type: "cron".to_string(),
        function_id: function_id.to_string(),
        config: json!({ "expression": expression }),
        worker_id: None,
        metadata: None,
    }
}

#[tokio::test]
async fn first_boot_seeds_configuration_entry() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_cron_worker(
        &harness,
        json!({ "adapter": { "name": "kv", "config": { "lock_index": "seeded" } } }),
    )
    .await;

    let stored = stored_value(&harness, false).await;
    assert_eq!(stored["id"], CONFIG_ID);
    assert_eq!(stored["value"]["adapter"]["name"], "kv");
    assert_eq!(stored["value"]["adapter"]["config"]["lock_index"], "seeded");
}

#[tokio::test]
async fn runtime_edit_survives_worker_restart() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_cron_worker(&harness, json!({})).await;

    // Operator edits the adapter at runtime.
    set_value(
        &harness,
        json!({ "adapter": { "name": "kv", "config": { "lock_index": "edited" } } }),
    )
    .await;

    // "Restart": a fresh worker with a different seed must NOT clobber the
    // stored value, and must adopt it as the runtime source of truth (its boot
    // catch-up hot-swaps onto the persisted adapter).
    let restarted = start_cron_worker(
        &harness,
        json!({ "adapter": { "name": "kv", "config": { "lock_index": "seed-default" } } }),
    )
    .await;

    let stored = stored_value(&harness, false).await;
    assert_eq!(
        stored["value"]["adapter"]["config"]["lock_index"], "edited",
        "seed must not clobber the runtime-edited value"
    );
    let adapter = restarted
        .config_snapshot()
        .adapter
        .clone()
        .expect("adapter recorded in the snapshot");
    assert_eq!(
        adapter
            .config
            .as_ref()
            .and_then(|c| c["lock_index"].as_str()),
        Some("edited"),
        "restarted worker must adopt the persisted value, not its seed"
    );
}

#[tokio::test]
async fn adapter_hot_swap_rebinds_live_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_cron_worker(&harness, json!({})).await;

    // A live (non-firing) job must survive the swap onto the new transport.
    worker
        .register_trigger(cron_trigger("hot-swap-job", "test::noop", "0 0 * * * *"))
        .await
        .expect("register job");

    let before = worker.adapter_snapshot();

    // A distinguishing adapter config flips the effective adapter, forcing the
    // full lock-backend hot-swap path.
    set_value(
        &harness,
        json!({ "adapter": { "name": "kv", "config": { "lock_index": "hot-swap" } } }),
    )
    .await;
    drive_apply(&harness).await;

    let after = worker.adapter_snapshot();
    assert!(
        !Arc::ptr_eq(&before, &after),
        "a lock-backend change must rebuild the scheduler instance"
    );
    assert_eq!(
        after.job_count().await,
        1,
        "the live cron job must be re-registered onto the new adapter"
    );
    assert_eq!(
        worker
            .config_snapshot()
            .adapter
            .as_ref()
            .and_then(|a| a.config.as_ref())
            .and_then(|c| c["lock_index"].as_str()),
        Some("hot-swap"),
        "the live config must reflect the applied adapter"
    );
}

#[tokio::test]
async fn unresolvable_adapter_keeps_previous_scheduler() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_cron_worker(&harness, json!({})).await;
    let before = worker.adapter_snapshot();

    // An adapter name that isn't registered deserializes fine and passes the
    // JSON schema, so it reaches the apply gate — where resolving the transport
    // fails and the previous scheduler/config must stand.
    set_value(&harness, json!({ "adapter": { "name": "does-not-exist" } })).await;
    drive_apply(&harness).await;

    assert!(
        Arc::ptr_eq(&before, &worker.adapter_snapshot()),
        "a failed resolve must keep the previous scheduler"
    );
    assert!(
        worker.config_snapshot().adapter.is_none(),
        "a failed resolve must keep the previous config"
    );
}

#[tokio::test]
async fn env_placeholder_expands_on_read() {
    // Scrub the var so the `${VAR:default}` placeholder resolves to its default.
    unsafe {
        std::env::remove_var("CRON_E2E_LOCK_INDEX");
    }

    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_cron_worker(
        &harness,
        json!({
            "adapter": {
                "name": "kv",
                "config": { "lock_index": "${CRON_E2E_LOCK_INDEX:fallback-index}" }
            }
        }),
    )
    .await;

    // The live snapshot sees the expanded value.
    assert_eq!(
        worker
            .config_snapshot()
            .adapter
            .as_ref()
            .and_then(|a| a.config.as_ref())
            .and_then(|c| c["lock_index"].as_str()),
        Some("fallback-index"),
        "the live config must see the expanded placeholder"
    );

    // The stored value keeps the placeholder verbatim (raw read).
    let raw = stored_value(&harness, true).await;
    assert_eq!(
        raw["value"]["adapter"]["config"]["lock_index"], "${CRON_E2E_LOCK_INDEX:fallback-index}",
        "the persisted value must retain the placeholder for re-expansion"
    );
}

#[tokio::test]
async fn cron_job_fires_through_configured_adapter() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_cron_worker(&harness, json!({})).await;

    let counter = Arc::new(AtomicU64::new(0));
    register_counter(&harness.engine, "test::tick", counter.clone());

    // Fire every second so the test resolves quickly.
    worker
        .register_trigger(cron_trigger("fire-job", "test::tick", "* * * * * *"))
        .await
        .expect("register firing job");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if counter.load(Ordering::SeqCst) >= 1 {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("cron job did not fire through the configured adapter within 10s");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
