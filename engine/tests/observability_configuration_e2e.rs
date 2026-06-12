// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end test for the `iii-observability` ↔ `configuration` worker
//! integration: seed-on-first-boot, no-clobber across worker restarts, hot
//! apply of live fields and alert rules, schema rejection of invalid values,
//! and `${VAR:default}` expansion.
//!
//! Modeled on `engine/tests/http_configuration_e2e.rs` — composes the two
//! workers against a real `FsAdapter` on a `tempfile::tempdir()`. No engine
//! boot, no WebSocket, no subprocess.
//!
//! Every test is `#[serial]`: the observability worker manages process-global
//! state (the global config snapshot, log/metric storages, the alert
//! manager), so concurrent tests in this binary would race each other.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use serial_test::serial;

use iii::engine::{Engine, EngineTrait};
use iii::function::FunctionResult;
use iii::workers::configuration::ConfigurationWorker;
use iii::workers::configuration::adapters::ConfigurationAdapter;
use iii::workers::configuration::adapters::fs::FsAdapter;
use iii::workers::configuration::structs::ConfigurationSetInput;
use iii::workers::observability::ObservabilityWorker;
use iii::workers::traits::Worker;

struct Harness {
    engine: Arc<Engine>,
    configuration: ConfigurationWorker,
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

/// Create, initialize, and start an `iii-observability` worker with the
/// given seed.
async fn start_observability_worker(harness: &Harness, seed: Value) -> ObservabilityWorker {
    let worker = ObservabilityWorker::for_test(harness.engine.clone(), Some(seed)).expect("worker");
    worker.initialize().await.expect("initialize");
    Worker::register_functions(&worker, harness.engine.clone());
    worker
        .start_background_tasks(harness.shutdown_rx.clone(), harness.shutdown_tx.clone())
        .await
        .expect("start_background_tasks");
    worker
}

async fn set_value(harness: &Harness, value: Value) {
    let result = harness
        .configuration
        .set_fn(ConfigurationSetInput {
            id: "iii-observability".to_string(),
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
            id: "iii-observability".to_string(),
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
#[serial]
async fn first_boot_seeds_configuration_entry() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_observability_worker(
        &harness,
        json!({ "service_name": "e2e-observability", "logs_max_count": 4242 }),
    )
    .await;

    let stored = harness
        .engine
        .call("configuration::get", json!({ "id": "iii-observability" }))
        .await
        .expect("configuration::get")
        .expect("get returns a body");
    assert_eq!(stored["value"]["service_name"], "e2e-observability");
    assert_eq!(stored["value"]["logs_max_count"], 4242);
    // Unset fields are not seeded as nulls.
    assert!(
        stored["value"].get("endpoint").is_none(),
        "unset fields must not seed: {stored}"
    );
}

#[tokio::test]
#[serial]
async fn runtime_edit_survives_worker_restart() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let seed = json!({ "logs_max_count": 1000 });

    let worker = start_observability_worker(&harness, seed.clone()).await;

    set_value(&harness, json!({ "logs_max_count": 4321 })).await;
    wait_for(
        || worker.current_config().logs_max_count == Some(4321),
        "runtime edit to apply",
    )
    .await;

    // Restart the worker with the same seed (ReloadManager semantics).
    worker.destroy().await.expect("destroy");
    let restarted = start_observability_worker(&harness, seed).await;

    // The runtime edit wins; the config.yaml seed must not clobber it.
    assert_eq!(restarted.current_config().logs_max_count, Some(4321));
    let stored = harness
        .engine
        .call("configuration::get", json!({ "id": "iii-observability" }))
        .await
        .expect("configuration::get")
        .expect("get returns a body");
    assert_eq!(stored["value"]["logs_max_count"], 4321);
}

#[tokio::test]
#[serial]
async fn live_field_hot_applies_through_the_trigger() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_observability_worker(&harness, json!({ "logs_sampling_ratio": 1.0 })).await;

    set_value(&harness, json!({ "logs_sampling_ratio": 0.25 })).await;

    // Applied via the real configuration:updated fan-out, no manual driving.
    wait_for(
        || (worker.current_config().logs_sampling_ratio - 0.25).abs() < f64::EPSILON,
        "logs_sampling_ratio to hot-apply",
    )
    .await;
}

#[tokio::test]
#[serial]
async fn alert_rules_hot_swap_and_prune() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_observability_worker(&harness, json!({})).await;

    set_value(
        &harness,
        json!({
            "alerts": [{
                "name": "e2e-alert-hotswap",
                "metric": "iii.invocations.error",
                "threshold": 1.0,
            }],
        }),
    )
    .await;

    wait_for(
        || {
            iii::workers::observability::metrics::get_alert_manager()
                .map(|m| m.get_rules().iter().any(|r| r.name == "e2e-alert-hotswap"))
                .unwrap_or(false)
        },
        "alert rule to hot-add",
    )
    .await;

    // Removing every rule prunes them (and their states) again.
    set_value(&harness, json!({ "alerts": [] })).await;
    wait_for(
        || {
            iii::workers::observability::metrics::get_alert_manager()
                .map(|m| m.get_rules().iter().all(|r| r.name != "e2e-alert-hotswap"))
                .unwrap_or(false)
        },
        "alert rule to hot-remove",
    )
    .await;
}

#[tokio::test]
#[serial]
async fn invalid_values_rejected_by_schema() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_observability_worker(&harness, json!({ "logs_max_count": 555 })).await;

    // Unknown fields, out-of-range ratios, and zero counts are all rejected
    // engine-side at configuration::set time.
    set_value_expect_rejection(&harness, json!({ "fake_key": true })).await;
    set_value_expect_rejection(&harness, json!({ "sampling_ratio": 7.5 })).await;
    set_value_expect_rejection(&harness, json!({ "logs_batch_size": 0 })).await;
    set_value_expect_rejection(
        &harness,
        json!({ "alerts": [{ "name": "x", "metric": "m", "threshold": 1.0, "typo": 1 }] }),
    )
    .await;

    // The live configuration was never touched by the rejected sets.
    assert_eq!(worker.current_config().logs_max_count, Some(555));
}

#[tokio::test]
#[serial]
async fn env_placeholders_expand_on_read() {
    // Scrub ambient state so the `${VAR:default}` default branch is what we
    // actually exercise. SAFETY: runs before the harness spawns any task;
    // remove_var is unsafe in edition 2024 because concurrent env access
    // is UB.
    unsafe { std::env::remove_var("OBS_CFG_E2E_NAME") };

    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker = start_observability_worker(
        &harness,
        json!({ "service_name": "${OBS_CFG_E2E_NAME:expanded-name}" }),
    )
    .await;

    let expanded = harness
        .engine
        .call("configuration::get", json!({ "id": "iii-observability" }))
        .await
        .expect("configuration::get")
        .expect("get returns a body");
    assert_eq!(expanded["value"]["service_name"], "expanded-name");

    // The stored value keeps the placeholder verbatim.
    let raw = harness
        .engine
        .call(
            "configuration::get",
            json!({ "id": "iii-observability", "raw": true }),
        )
        .await
        .expect("configuration::get raw")
        .expect("get returns a body");
    assert_eq!(
        raw["value"]["service_name"],
        "${OBS_CFG_E2E_NAME:expanded-name}"
    );
}

#[tokio::test]
#[serial]
async fn restart_tier_change_applies_live_fields_and_reports_rest() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let worker = start_observability_worker(&harness, json!({})).await;
    assert!(worker.current_config().logs_console_output);

    // A mixed edit: `exporter` is restart-tier (warned, applied at next
    // boot via the persisted entry), `logs_console_output` is live.
    set_value(
        &harness,
        json!({ "exporter": "otlp", "logs_console_output": false }),
    )
    .await;

    // Drive the handler synchronously so the assertion cannot pass vacuously
    // before the trigger fan-out ran.
    harness
        .engine
        .call("iii-observability::on-config-change", json!({}))
        .await
        .expect("config-change handler is invocable");

    assert!(
        !worker.current_config().logs_console_output,
        "live fields in a mixed edit must still apply"
    );
    assert_eq!(
        worker.current_config().exporter,
        Some(iii::workers::observability::config::OtelExporterType::Otlp),
        "the stored snapshot carries the restart-tier value for the next boot"
    );
}
