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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use serial_test::serial;

use iii::engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest};
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

/// Poll for `duration` and panic if `predicate` becomes true. Use this for
/// negative assertions where an event must *not* happen after a config change
/// has already been driven through `drive_apply`.
async fn assert_does_not_happen_for(
    mut predicate: impl FnMut() -> bool,
    duration: Duration,
    what: &str,
) {
    let deadline = tokio::time::Instant::now() + duration;
    while tokio::time::Instant::now() < deadline {
        if predicate() {
            panic!("{what}");
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
async fn disabled_boot_still_registers_the_configuration_entry() {
    // A worker that boots disabled must still adopt the configuration worker
    // (register the entry + watch it), so a remote `enabled: true` can be
    // persisted and applied at the next start. Before the adoption block was
    // moved ahead of the enabled gate, a disabled boot registered nothing and
    // the entry was unreachable via configuration::set / the console.
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;

    let _worker =
        start_observability_worker(&harness, json!({ "enabled": false, "logs_max_count": 222 }))
            .await;

    let stored = harness
        .engine
        .call("configuration::get", json!({ "id": "iii-observability" }))
        .await
        .expect("configuration::get must succeed even when observability booted disabled")
        .expect("get returns a body");
    assert_eq!(stored["value"]["enabled"], false);
    assert_eq!(stored["value"]["logs_max_count"], 222);

    // The change handler is registered too, so a later edit is observable.
    assert!(
        harness
            .engine
            .functions
            .get("iii-observability::on-config-change")
            .is_some(),
        "the on-config-change handler must be registered on a disabled boot"
    );
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

// ── Edge-case coverage: log-trigger reactivation, ingest gates, live store ──
// limits, schema set-rejection, and concurrent-edit convergence. ────────────

/// Register a `log` trigger that bumps a counter on each fan-out, filtered to
/// `level`. Returns the hit counter. Call AFTER `start_observability_worker`
/// (the worker registers the `log` trigger type in `initialize`).
async fn register_log_recorder(harness: &Harness, fn_id: &str, level: &str) -> Arc<AtomicUsize> {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    harness.engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: fn_id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(move |_payload: Value| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                FunctionResult::Success(Some(json!({ "ok": true })))
            }
        }),
    );
    harness
        .engine
        .trigger_registry
        .register_trigger(iii::trigger::Trigger {
            id: format!("{fn_id}-trigger"),
            trigger_type: iii::workers::observability::LOG_TRIGGER_TYPE.to_string(),
            function_id: fn_id.to_string(),
            config: json!({ "level": level }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("register log trigger");
    hits
}

fn log_storage_len() -> usize {
    iii::workers::observability::otel::get_log_storage()
        .map(|s| s.len())
        .unwrap_or(0)
}

fn clear_log_storage() {
    if let Some(s) = iii::workers::observability::otel::get_log_storage() {
        s.clear();
    }
}

async fn ingest_log(harness: &Harness, level_fn: &str, message: &str) {
    harness
        .engine
        .call(
            level_fn,
            json!({ "message": message, "service_name": "e2e" }),
        )
        .await
        .expect("log ingest call");
}

/// Drive `apply_config` synchronously through the real handler, so an
/// assertion cannot pass vacuously before the trigger fan-out ran.
async fn drive_apply(harness: &Harness) {
    harness
        .engine
        .call("iii-observability::on-config-change", json!({}))
        .await
        .expect("config-change handler is invocable");
}

#[tokio::test]
#[serial]
async fn logs_reenable_reactivates_the_log_trigger_pipeline() {
    // Boot with logs disabled (observability itself stays enabled, so the
    // background task set — including the parked log-trigger subscriber — runs).
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let worker = start_observability_worker(&harness, json!({ "logs_enabled": false })).await;

    let hits = register_log_recorder(&harness, "test::reactivate-recorder", "all").await;

    // While disabled, the ingest gate drops the log before any broadcast, so
    // the trigger never fires.
    ingest_log(&harness, "engine::log::info", "while-disabled").await;
    assert_does_not_happen_for(
        || hits.load(Ordering::SeqCst) > 0,
        Duration::from_millis(150),
        "log trigger fired while logs were disabled",
    )
    .await;

    // Enable logs at runtime; the F3 reactivation path revives the store in the
    // LIMITS tier and respawns the log-trigger subscriber.
    set_value(&harness, json!({ "logs_enabled": true })).await;
    drive_apply(&harness).await;
    wait_for(
        || worker.current_config().logs_enabled == Some(true),
        "logs_enabled:true to apply",
    )
    .await;

    // The reactivated subscriber now delivers ingested logs to the trigger —
    // no engine restart required.
    ingest_log(&harness, "engine::log::info", "after-enable").await;
    wait_for(
        || hits.load(Ordering::SeqCst) >= 1,
        "log trigger to fire after reactivation",
    )
    .await;
}

#[tokio::test]
#[serial]
async fn logs_disable_after_enable_stops_trigger_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let worker = start_observability_worker(&harness, json!({ "logs_enabled": true })).await;

    let hits = register_log_recorder(&harness, "test::disable-recorder", "all").await;

    // Enabled: the trigger fires.
    ingest_log(&harness, "engine::log::info", "enabled-1").await;
    wait_for(
        || hits.load(Ordering::SeqCst) >= 1,
        "log trigger to fire while enabled",
    )
    .await;
    let after_enabled = hits.load(Ordering::SeqCst);

    // Disable at runtime: the per-call ingest gate drops subsequent logs.
    set_value(&harness, json!({ "logs_enabled": false })).await;
    drive_apply(&harness).await;
    wait_for(
        || worker.current_config().logs_enabled == Some(false),
        "logs_enabled:false to apply",
    )
    .await;

    ingest_log(&harness, "engine::log::info", "disabled-2").await;
    assert_does_not_happen_for(
        || hits.load(Ordering::SeqCst) > after_enabled,
        Duration::from_millis(150),
        "log trigger fired after logs were disabled at runtime",
    )
    .await;
}

#[tokio::test]
#[serial]
async fn logs_max_count_retune_bounds_the_live_store() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let worker = start_observability_worker(&harness, json!({ "logs_max_count": 1000 })).await;
    clear_log_storage();

    set_value(&harness, json!({ "logs_max_count": 3 })).await;
    drive_apply(&harness).await;
    wait_for(
        || worker.current_config().logs_max_count == Some(3),
        "logs_max_count to apply",
    )
    .await;

    // Ingest well past the new cap; the LIMITS tier retuned the live store, so
    // it bounds itself.
    for i in 0..8 {
        ingest_log(&harness, "engine::log::info", &format!("m{i}")).await;
    }
    assert!(
        log_storage_len() <= 3,
        "retuned cap must bound the live store, got {}",
        log_storage_len()
    );
}

#[tokio::test]
#[serial]
async fn logs_sampling_ratio_zero_drops_then_restores() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let worker = start_observability_worker(&harness, json!({ "logs_sampling_ratio": 1.0 })).await;
    clear_log_storage();

    set_value(&harness, json!({ "logs_sampling_ratio": 0.0 })).await;
    drive_apply(&harness).await;
    wait_for(
        || worker.current_config().logs_sampling_ratio == 0.0,
        "sampling_ratio 0 to apply",
    )
    .await;

    clear_log_storage();
    for i in 0..5 {
        ingest_log(&harness, "engine::log::info", &format!("drop{i}")).await;
    }
    assert_eq!(
        log_storage_len(),
        0,
        "logs_sampling_ratio 0 drops every ingested log at the gate"
    );

    // Restore full sampling -> logs flow again.
    set_value(&harness, json!({ "logs_sampling_ratio": 1.0 })).await;
    drive_apply(&harness).await;
    wait_for(
        || (worker.current_config().logs_sampling_ratio - 1.0).abs() < f64::EPSILON,
        "sampling_ratio 1 to apply",
    )
    .await;
    clear_log_storage();
    ingest_log(&harness, "engine::log::info", "kept").await;
    assert_eq!(log_storage_len(), 1, "full sampling keeps the ingested log");
}

#[tokio::test]
#[serial]
async fn schema_rejects_out_of_range_log_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let worker = start_observability_worker(&harness, json!({ "logs_batch_size": 100 })).await;

    // The tightened schema bounds reject values the OTLP exporter would
    // otherwise silently clamp to its defaults: logs_batch_size max 10_000,
    // logs_flush_interval_ms in [100, 3_600_000].
    set_value_expect_rejection(&harness, json!({ "logs_batch_size": 20_000 })).await;
    set_value_expect_rejection(&harness, json!({ "logs_flush_interval_ms": 50 })).await;
    set_value_expect_rejection(&harness, json!({ "logs_flush_interval_ms": 4_000_000 })).await;

    // The rejected sets never touched the live config.
    assert_eq!(worker.current_config().logs_batch_size, Some(100));

    // Boundary values are accepted (inclusive bounds) and apply.
    set_value(
        &harness,
        json!({ "logs_batch_size": 10_000, "logs_flush_interval_ms": 100 }),
    )
    .await;
    drive_apply(&harness).await;
    wait_for(
        || worker.current_config().logs_batch_size == Some(10_000),
        "accepted boundary value to apply",
    )
    .await;
}

#[tokio::test]
#[serial]
async fn concurrent_applies_converge_under_apply_lock() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let worker = start_observability_worker(&harness, json!({ "logs_max_count": 1 })).await;

    // Persist a new value, then fire several applies concurrently. The
    // apply_lock serializes them; they all read the same stored value and
    // converge — no torn or lost update.
    set_value(&harness, json!({ "logs_max_count": 77 })).await;
    let mut handles = Vec::new();
    for _ in 0..5 {
        let engine = harness.engine.clone();
        handles.push(tokio::spawn(async move {
            let _ = engine
                .call("iii-observability::on-config-change", json!({}))
                .await;
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    wait_for(
        || worker.current_config().logs_max_count == Some(77),
        "concurrent applies to converge",
    )
    .await;
    assert_eq!(worker.current_config().logs_max_count, Some(77));
}

#[tokio::test]
#[serial]
async fn failed_log_level_reload_is_not_advertised_and_keeps_retrying() {
    // C4 regression: the global snapshot must never report a log level we
    // failed to install, and old.level != new.level must persist so the next
    // apply retries instead of masking the drift.
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let worker = start_observability_worker(&harness, json!({})).await;
    assert_eq!(
        worker.current_config().level,
        None,
        "no level seeded at boot"
    );

    // `engine=notalevel` passes schema (level is a free string) but fails the
    // EnvFilter parse in reload_log_level, so the install always fails —
    // deterministically, independent of whether a reload handle exists.
    set_value(&harness, json!({ "level": "engine=notalevel" })).await;
    drive_apply(&harness).await;
    assert_eq!(
        worker.current_config().level,
        None,
        "a failed log-level reload must not advertise the un-applied level"
    );

    // Re-driving applies again: still reverted, never converged to the bad value.
    drive_apply(&harness).await;
    assert_eq!(
        worker.current_config().level,
        None,
        "the failed level must keep retrying, not mask the drift"
    );
}

fn make_span(i: u64) -> iii::workers::observability::otel::StoredSpan {
    iii::workers::observability::otel::StoredSpan {
        trace_id: format!("trace-{i}"),
        span_id: format!("span-{i}"),
        parent_span_id: None,
        name: format!("op-{i}"),
        start_time_unix_nano: 1_000_000_000 + i,
        end_time_unix_nano: 2_000_000_000 + i,
        status: "OK".to_string(),
        status_description: None,
        attributes: Vec::new(),
        service_name: "e2e".to_string(),
        events: Vec::new(),
        links: Vec::new(),
        instrumentation_scope_name: None,
        instrumentation_scope_version: None,
        flags: None,
    }
}

#[tokio::test]
#[serial]
async fn memory_max_spans_retune_bounds_the_live_span_store() {
    // T3: prove the LIMITS tier wires `memory_max_spans` through apply_config to
    // the live span store (the log path was covered; spans were not).
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let worker = start_observability_worker(&harness, json!({ "memory_max_spans": 1000 })).await;
    // The lean e2e harness skips full OTEL init, so the in-memory span store
    // isn't created at boot. Bootstrap it (as the engine's memory exporter
    // would) so the LIMITS tier has a live store to retune.
    let _ = iii::workers::observability::otel::InMemorySpanExporter::new(1000, "e2e".to_string());
    let storage =
        iii::workers::observability::otel::get_span_storage().expect("span storage exists");
    storage.clear();

    set_value(&harness, json!({ "memory_max_spans": 2 })).await;
    drive_apply(&harness).await;
    wait_for(
        || worker.current_config().memory_max_spans == Some(2),
        "memory_max_spans to apply",
    )
    .await;

    storage.add_spans((0..6).map(make_span).collect());
    assert!(
        storage.len() <= 2,
        "retuned span cap must bound the live store, got {}",
        storage.len()
    );
}

#[tokio::test]
#[serial]
async fn collapse_rules_hot_apply_through_config() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let worker = start_observability_worker(&harness, json!({})).await;
    assert!(worker.current_config().collapse_spans.is_empty());

    // SWAP tier: a collapse rule recompiles the cache through apply_config.
    set_value(
        &harness,
        json!({ "collapse_spans": [{ "name": "wrapper *" }] }),
    )
    .await;
    drive_apply(&harness).await;
    wait_for(
        || worker.current_config().collapse_spans.len() == 1,
        "collapse rule to hot-apply",
    )
    .await;

    // Clearing the rules recompiles the cache back to empty.
    set_value(&harness, json!({ "collapse_spans": [] })).await;
    drive_apply(&harness).await;
    wait_for(
        || worker.current_config().collapse_spans.is_empty(),
        "collapse rules to clear",
    )
    .await;
}

#[tokio::test]
#[serial]
async fn sampler_rebuilds_on_trace_sampling_ratio_change() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    let worker = start_observability_worker(&harness, json!({ "sampling_ratio": 1.0 })).await;

    // Changing the TRACE sampling_ratio (not logs_sampling_ratio) drives the
    // SWAP-tier otel::refresh_sampler() path; assert it applies and the worker
    // stays healthy.
    set_value(&harness, json!({ "sampling_ratio": 0.1 })).await;
    drive_apply(&harness).await;
    wait_for(
        || worker.current_config().sampling_ratio == Some(0.1),
        "sampler rebuild to apply",
    )
    .await;
}

#[tokio::test]
#[serial]
async fn logs_exporter_respawns_on_batch_size_change() {
    let dir = tempfile::tempdir().unwrap();
    let harness = build_harness(dir.path()).await;
    // `both` runs the OTLP logs exporter alongside the memory store. The
    // default endpoint is unreachable in tests, but the OTLP exporter connects
    // lazily in the background, so the apply path exercises the respawn_exporter
    // branch (a TASK-REBUILD tier) without a live collector.
    let worker = start_observability_worker(
        &harness,
        json!({ "logs_exporter": "both", "logs_batch_size": 100 }),
    )
    .await;

    set_value(&harness, json!({ "logs_batch_size": 200 })).await;
    drive_apply(&harness).await;
    wait_for(
        || worker.current_config().logs_batch_size == Some(200),
        "logs exporter respawn to apply the new batch size",
    )
    .await;
}
