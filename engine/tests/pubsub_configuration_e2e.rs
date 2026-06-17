// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end test for the `iii-pubsub` ↔ `configuration` worker integration:
//! seed-on-first-boot, no-clobber across worker restarts, the full adapter
//! hot-swap (rebuild backend + re-subscribe live `subscribe` triggers onto it),
//! the gated keep-previous behavior on an unresolvable adapter, schema
//! rejection of unknown keys, and `${VAR:default}` expansion on read.
//!
//! Modeled on `engine/tests/state_configuration_e2e.rs` — composes the two
//! workers against a real `FsAdapter` on a `tempfile::tempdir()`. No engine
//! boot, no subprocess. The pub/sub adapters used here (`local`) need no
//! external server, so no Redis and no port serialization.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use iii::engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest};
use iii::function::FunctionResult;
use iii::trigger::Trigger;
use iii::workers::configuration::ConfigurationWorker;
use iii::workers::configuration::adapters::ConfigurationAdapter;
use iii::workers::configuration::adapters::fs::FsAdapter;
use iii::workers::configuration::structs::{ConfigurationGetInput, ConfigurationSetInput};
use iii::workers::pubsub::{PubSubInput, PubSubWorker};
use iii::workers::traits::Worker;

const CONFIG_ID: &str = "iii-pubsub";

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

/// Create, initialize, and start an `iii-pubsub` worker with the given seed.
async fn start_pubsub_worker(harness: &Harness, seed: Value) -> PubSubWorker {
    let worker = PubSubWorker::for_test(harness.engine.clone(), Some(seed))
        .await
        .expect("pubsub worker");
    worker.initialize().await.expect("pubsub initialize");
    Worker::register_functions(&worker, harness.engine.clone());
    worker
        .start_background_tasks(harness.shutdown_rx.clone(), harness.shutdown_tx.clone())
        .await
        .expect("pubsub start_background_tasks");
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
        .call("iii-pubsub::on-config-change", json!({}))
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

/// Read the stored value verbatim (placeholders unexpanded).
async fn stored_value_raw(harness: &Harness) -> Value {
    let raw = serde_json::to_value(ConfigurationGetInput {
        id: CONFIG_ID.to_string(),
        raw: true,
    })
    .unwrap();
    harness
        .engine
        .call("configuration::get", raw)
        .await
        .expect("configuration::get raw")
        .expect("get returns a body")
}

/// Register a function that bumps `counter` each time it is invoked, returning
/// the shared counter so a test can observe pub/sub delivery.
fn register_counting_listener(engine: &Arc<Engine>, function_id: &str) -> Arc<AtomicU64> {
    let counter = Arc::new(AtomicU64::new(0));
    let counter_for_handler = counter.clone();
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: function_id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(move |_input: Value| {
            let counter = counter_for_handler.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                FunctionResult::Success(None)
            }
        }),
    );
    counter
}

/// Register a `subscribe` trigger (topic → function), routed through the
/// worker's registrator so it subscribes on the live backend.
async fn subscribe(harness: &Harness, id: &str, topic: &str, function_id: &str) {
    harness
        .engine
        .trigger_registry
        .register_trigger(Trigger {
            id: id.to_string(),
            trigger_type: "subscribe".to_string(),
            function_id: function_id.to_string(),
            config: json!({ "topic": topic }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("register subscribe trigger");
}

/// Publish through the worker (reads the live backend via `adapter_snapshot`).
async fn publish(worker: &PubSubWorker, topic: &str, data: Value) {
    let _ = worker
        .publish(PubSubInput {
            topic: topic.to_string(),
            data,
        })
        .await;
}

/// Poll until `counter` reaches `expected`, or panic after 5s. Delivery is
/// async (the adapter spawns the invocation), so assertions must wait.
async fn wait_for_count(counter: &Arc<AtomicU64>, expected: u64) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if counter.load(Ordering::SeqCst) >= expected {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!(
                "counter never reached {expected} (stuck at {})",
                counter.load(Ordering::SeqCst)
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn first_boot_seeds_configuration_entry() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_pubsub_worker(
        &harness,
        json!({ "adapter": { "name": "local", "config": { "label": "seeded" } } }),
    )
    .await;

    let stored = stored_value(&harness).await;
    assert_eq!(stored["id"], CONFIG_ID);
    assert_eq!(stored["value"]["adapter"]["name"], "local");
    assert_eq!(stored["value"]["adapter"]["config"]["label"], "seeded");
}

#[tokio::test]
async fn runtime_edit_survives_worker_restart() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_pubsub_worker(&harness, json!({ "adapter": { "name": "local" } })).await;

    // Operator repoints the adapter at runtime (config differs so it's a real
    // change, but stays on the in-process `local` backend — no Redis needed).
    set_value(
        &harness,
        json!({ "adapter": { "name": "local", "config": { "label": "edited" } } }),
    )
    .await;

    // "Restart": a fresh worker with a different seed must NOT clobber the
    // stored value, and must adopt it as the runtime source of truth.
    let restarted = start_pubsub_worker(&harness, json!({ "adapter": { "name": "local" } })).await;

    let stored = stored_value(&harness).await;
    assert_eq!(
        stored["value"]["adapter"]["config"]["label"], "edited",
        "seed must not clobber the runtime-edited value"
    );
    let adapter = restarted
        .current_config()
        .adapter
        .expect("adapter in restarted snapshot");
    assert_eq!(
        adapter.config.expect("adapter config")["label"],
        "edited",
        "restarted worker must adopt the persisted value, not its seed"
    );
}

#[tokio::test]
async fn adapter_hot_swap_rebuilds_backend_and_rebinds_subscriptions() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_pubsub_worker(&harness, json!({ "adapter": { "name": "local" } })).await;

    // A live subscription on the seed backend.
    let counter = register_counting_listener(&harness.engine, "test::on_orders");
    subscribe(&harness, "sub-orders", "orders", "test::on_orders").await;

    // Delivery works on the seed backend.
    publish(&worker, "orders", json!({ "id": 1 })).await;
    wait_for_count(&counter, 1).await;

    let backend_before = worker.adapter_snapshot();

    // Repoint the adapter at runtime: a config difference forces a real backend
    // rebuild while staying on the in-process `local` adapter (no Redis needed).
    set_value(
        &harness,
        json!({ "adapter": { "name": "local", "config": { "generation": 2 } } }),
    )
    .await;
    drive_apply(&harness).await;

    // The backend instance was rebuilt...
    let backend_after = worker.adapter_snapshot();
    assert!(
        !Arc::ptr_eq(&backend_before, &backend_after),
        "a changed adapter config must rebuild the backend instance"
    );

    // ...and the pre-existing subscription was rebound onto it: a publish after
    // the swap still reaches the handler. If re-subscription were missing, the
    // fresh backend would have no `orders` subscription and the count would
    // stay at 1.
    publish(&worker, "orders", json!({ "id": 2 })).await;
    wait_for_count(&counter, 2).await;
}

#[tokio::test]
async fn unresolvable_adapter_keeps_previous_backend() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_pubsub_worker(&harness, json!({ "adapter": { "name": "local" } })).await;

    let counter = register_counting_listener(&harness.engine, "test::on_events");
    subscribe(&harness, "sub-events", "events", "test::on_events").await;
    publish(&worker, "events", json!({ "n": 1 })).await;
    wait_for_count(&counter, 1).await;

    let backend_before = worker.adapter_snapshot();

    // A syntactically valid value naming an adapter the registry doesn't know.
    // It passes the schema (free-form AdapterEntry) but the build is gated.
    set_value(&harness, json!({ "adapter": { "name": "does-not-exist" } })).await;
    drive_apply(&harness).await;

    // The build failure kept the previous backend and config.
    assert!(
        Arc::ptr_eq(&backend_before, &worker.adapter_snapshot()),
        "an unresolvable adapter must keep the previous backend instance"
    );
    assert_eq!(
        worker
            .current_config()
            .adapter
            .expect("adapter unchanged")
            .name,
        "local",
        "an unresolvable adapter must not mutate the live config"
    );

    // Delivery still works on the retained backend.
    publish(&worker, "events", json!({ "n": 2 })).await;
    wait_for_count(&counter, 2).await;
}

#[tokio::test]
async fn noop_edit_does_not_rebuild_backend() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_pubsub_worker(&harness, json!({ "adapter": { "name": "local" } })).await;

    let backend_before = worker.adapter_snapshot();

    // Re-set the identical effective adapter: the change-detection must treat
    // this as a no-op and NOT rebuild the backend or churn subscriptions.
    set_value(&harness, json!({ "adapter": { "name": "local" } })).await;
    drive_apply(&harness).await;

    assert!(
        Arc::ptr_eq(&backend_before, &worker.adapter_snapshot()),
        "an unchanged adapter must not rebuild the backend instance"
    );
}

#[tokio::test]
async fn subscription_registered_after_swap_uses_new_backend() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_pubsub_worker(&harness, json!({ "adapter": { "name": "local" } })).await;

    // Swap the backend first (a config difference forces a rebuild).
    set_value(
        &harness,
        json!({ "adapter": { "name": "local", "config": { "generation": 2 } } }),
    )
    .await;
    drive_apply(&harness).await;

    // A subscription registered AFTER the swap must land on the new backend
    // (the registrator reads the live adapter under the same lock as the swap).
    let counter = register_counting_listener(&harness.engine, "test::after_swap");
    subscribe(&harness, "sub-after", "after", "test::after_swap").await;
    publish(&worker, "after", json!({ "n": 1 })).await;
    wait_for_count(&counter, 1).await;
}

#[tokio::test]
async fn schema_rejects_unknown_top_level_key() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_pubsub_worker(&harness, json!({ "adapter": { "name": "local" } })).await;

    // deny_unknown_fields flows into the schema: an unknown top-level key and an
    // unknown adapter-entry key are both rejected at `configuration::set` time.
    set_value_expect_rejection(&harness, json!({ "unknown_top_level": 1 })).await;
    set_value_expect_rejection(
        &harness,
        json!({ "adapter": { "name": "local", "bogus": 1 } }),
    )
    .await;
}

#[tokio::test]
async fn env_placeholder_expands_on_read() {
    // The default in the placeholder applies regardless of ambient env; scrub
    // the var so the assertion is deterministic.
    unsafe {
        std::env::remove_var("PUBSUB_E2E_LABEL");
    }

    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_pubsub_worker(
        &harness,
        json!({
            "adapter": {
                "name": "local",
                "config": { "label": "${PUBSUB_E2E_LABEL:default-label}" }
            }
        }),
    )
    .await;

    // The live snapshot resolved the placeholder to its default on read.
    let adapter = worker.current_config().adapter.expect("adapter present");
    assert_eq!(
        adapter.config.expect("adapter config")["label"],
        "default-label",
        "placeholder must expand to its default on read"
    );

    // The stored value keeps the placeholder verbatim (raw get).
    let raw = stored_value_raw(&harness).await;
    assert_eq!(
        raw["value"]["adapter"]["config"]["label"], "${PUBSUB_E2E_LABEL:default-label}",
        "raw get must preserve the placeholder"
    );
}
