// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! End-to-end tests for DLQ redrive and CLI trigger features.
//!
//! These tests exercise the full lifecycle:
//!   enqueue → failure → retry exhaustion → DLQ → redrive → re-processing
//!
//! They also cover the `iii trigger` CLI subcommand (argument parsing + connection handling).

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use serde_json::{Value, json};

use iii::{
    engine::Engine,
    function::{Function, FunctionResult},
    modules::{module::Module, queue::QueueCoreModule},
    protocol::ErrorBody,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn queue_config_with_fast_retries() -> Value {
    json!({
        "queue_configs": {
            "orders": {
                "type": "standard",
                "concurrency": 1,
                "max_retries": 2,
                "backoff_ms": 50,
                "poll_interval_ms": 30
            }
        }
    })
}

/// Creates an engine with the QueueCoreModule initialized AND the module's
/// built-in functions (including `iii::queue::redrive`) registered on the engine.
async fn create_engine_with_redrive(config: Value) -> Arc<Engine> {
    iii::modules::observability::metrics::ensure_default_meter();
    let engine = Arc::new(Engine::new());

    let module = QueueCoreModule::create(engine.clone(), Some(config))
        .await
        .expect("QueueCoreModule::create should succeed");

    // Register macro-generated functions (iii::queue::redrive, enqueue, etc.)
    module.register_functions(engine.clone());

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    engine
}

async fn enqueue(
    engine: &Engine,
    queue_name: &str,
    function_id: &str,
    data: Value,
) -> anyhow::Result<()> {
    let guard = engine.queue_module.read().await;
    let qm = guard
        .as_ref()
        .expect("queue_module should be set after initialize");
    let message_id = uuid::Uuid::new_v4().to_string();
    qm.enqueue_to_function_queue(queue_name, function_id, data, message_id, None, None)
        .await
}

async fn dlq_count(engine: &Engine, queue_name: &str) -> u64 {
    let guard = engine.queue_module.read().await;
    let qm = guard
        .as_ref()
        .expect("queue_module should be set after initialize");
    qm.function_queue_dlq_count(queue_name)
        .await
        .expect("dlq_count should not fail")
}

/// Invoke the `iii::queue::redrive` function through the engine's function registry.
async fn invoke_redrive(
    engine: &Engine,
    queue_name: &str,
) -> FunctionResult<Option<Value>, ErrorBody> {
    let function = engine
        .functions
        .get("iii::queue::redrive")
        .expect("iii::queue::redrive should be registered");
    function
        .call_handler(None, json!({ "queue": queue_name }), None)
        .await
}

fn register_failing_function(engine: &Arc<Engine>, function_id: &str, call_count: Arc<AtomicU64>) {
    let function = Function {
        handler: Arc::new(move |_invocation_id, _input, _session| {
            let count = call_count.clone();
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                FunctionResult::Failure(ErrorBody {
                    code: "PROCESSING_ERROR".to_string(),
                    message: "simulated processing failure".to_string(),
                    stacktrace: None,
                })
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("always-failing handler for DLQ tests".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
}

fn register_counting_function(engine: &Arc<Engine>, function_id: &str, counter: Arc<AtomicU64>) {
    let function = Function {
        handler: Arc::new(move |_invocation_id, _input, _session| {
            let count = counter.clone();
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                FunctionResult::Success(Some(json!({ "ok": true })))
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("counting handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
}

/// Registers a function that fails the first N times, then succeeds.
fn register_flaky_function(
    engine: &Arc<Engine>,
    function_id: &str,
    fail_count: Arc<AtomicU64>,
    success_count: Arc<AtomicU64>,
    failures_before_success: u64,
) {
    let function = Function {
        handler: Arc::new(move |_invocation_id, _input, _session| {
            let fc = fail_count.clone();
            let sc = success_count.clone();
            let threshold = failures_before_success;
            Box::pin(async move {
                let attempt = fc.fetch_add(1, Ordering::SeqCst);
                if attempt < threshold {
                    FunctionResult::Failure(ErrorBody {
                        code: "TRANSIENT".to_string(),
                        message: format!("transient failure #{}", attempt + 1),
                        stacktrace: None,
                    })
                } else {
                    sc.fetch_add(1, Ordering::SeqCst);
                    FunctionResult::Success(Some(json!({ "recovered": true })))
                }
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("flaky handler that eventually succeeds".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
}

// ---------------------------------------------------------------------------
// E2E: full DLQ lifecycle (enqueue → fail → DLQ → redrive → reprocess)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_dlq_redrive_full_lifecycle() {
    // Phase 1: set up engine, register a failing function, enqueue a message
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let fail_count = Arc::new(AtomicU64::new(0));
    let success_count = Arc::new(AtomicU64::new(0));

    // Register a failing function so it exhausts retries
    register_failing_function(&engine, "e2e::order_processor", fail_count.clone());

    let module = QueueCoreModule::create(engine.clone(), Some(queue_config_with_fast_retries()))
        .await
        .expect("create should succeed");

    // Register iii::queue::redrive and other built-in functions
    module.register_functions(engine.clone());

    module
        .initialize()
        .await
        .expect("initialize should succeed");

    // Verify DLQ starts empty
    assert_eq!(
        dlq_count(&engine, "orders").await,
        0,
        "DLQ should start empty"
    );

    // Enqueue a message that will fail
    enqueue(
        &engine,
        "orders",
        "e2e::order_processor",
        json!({
            "order_id": "ORD-12345",
            "amount": 99.99,
            "customer": "sergio"
        }),
    )
    .await
    .expect("enqueue should succeed");

    // Wait for retries to exhaust (max_retries=2, backoff_ms=50)
    tokio::time::sleep(Duration::from_millis(2000)).await;

    let total_failures = fail_count.load(Ordering::SeqCst);
    assert!(
        total_failures >= 1 && total_failures <= 3,
        "Expected 1-3 attempts (1 initial + up to 2 retries), got {total_failures}"
    );

    let in_dlq = dlq_count(&engine, "orders").await;
    assert_eq!(
        in_dlq, 1,
        "Message should be in DLQ after retry exhaustion, got {in_dlq}"
    );

    // Phase 2: swap handler for one that succeeds, then redrive
    register_counting_function(&engine, "e2e::order_processor", success_count.clone());

    let result = invoke_redrive(&engine, "orders").await;
    match &result {
        FunctionResult::Success(_) => {} // expected
        FunctionResult::Failure(e) => panic!("Redrive should succeed, got failure: {:?}", e),
        _ => panic!("Redrive returned unexpected variant"),
    }

    // Wait for the redriven message to be reprocessed
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let successes = success_count.load(Ordering::SeqCst);
    assert_eq!(
        successes, 1,
        "Redriven message should have been processed successfully, got {successes}"
    );

    // DLQ should be empty after redrive + successful processing
    let remaining = dlq_count(&engine, "orders").await;
    assert_eq!(
        remaining, 0,
        "DLQ should be empty after successful redrive, got {remaining}"
    );
}

// ---------------------------------------------------------------------------
// E2E: redrive with multiple DLQ messages
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_redrive_multiple_dlq_messages() {
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let fail_count = Arc::new(AtomicU64::new(0));
    register_failing_function(&engine, "e2e::batch_handler", fail_count.clone());

    let module = QueueCoreModule::create(engine.clone(), Some(queue_config_with_fast_retries()))
        .await
        .expect("create should succeed");

    module.register_functions(engine.clone());
    module
        .initialize()
        .await
        .expect("initialize should succeed");

    // Enqueue 3 messages that will all fail
    for i in 0..3 {
        enqueue(
            &engine,
            "orders",
            "e2e::batch_handler",
            json!({ "batch_id": i, "type": "batch_item" }),
        )
        .await
        .expect("enqueue should succeed");
    }

    // Wait for all retries to exhaust
    tokio::time::sleep(Duration::from_millis(4000)).await;

    let in_dlq = dlq_count(&engine, "orders").await;
    assert_eq!(in_dlq, 3, "All 3 messages should be in DLQ, got {in_dlq}");

    // Replace with a succeeding handler
    let success_count = Arc::new(AtomicU64::new(0));
    register_counting_function(&engine, "e2e::batch_handler", success_count.clone());

    // Redrive all messages
    let result = invoke_redrive(&engine, "orders").await;
    match &result {
        FunctionResult::Success(_) => {}
        FunctionResult::Failure(e) => panic!("Redrive should succeed, got: {:?}", e),
        _ => panic!("Unexpected result variant"),
    }

    // Wait for all redriven messages to be processed
    tokio::time::sleep(Duration::from_millis(2000)).await;

    let successes = success_count.load(Ordering::SeqCst);
    assert_eq!(
        successes, 3,
        "All 3 redriven messages should be processed, got {successes}"
    );

    assert_eq!(
        dlq_count(&engine, "orders").await,
        0,
        "DLQ should be empty after full redrive"
    );
}

// ---------------------------------------------------------------------------
// E2E: redrive on empty DLQ is a no-op
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_redrive_empty_dlq_is_noop() {
    let engine = create_engine_with_redrive(queue_config_with_fast_retries()).await;

    assert_eq!(dlq_count(&engine, "orders").await, 0);

    let result = invoke_redrive(&engine, "orders").await;
    match &result {
        FunctionResult::Success(_) => {}
        FunctionResult::Failure(e) => panic!("Redrive on empty DLQ should succeed, got: {:?}", e),
        _ => panic!("Unexpected result variant"),
    }

    assert_eq!(dlq_count(&engine, "orders").await, 0);
}

// ---------------------------------------------------------------------------
// E2E: redrive with empty queue name returns error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_redrive_empty_queue_name_returns_error() {
    let engine = create_engine_with_redrive(queue_config_with_fast_retries()).await;

    let result = invoke_redrive(&engine, "").await;
    match result {
        FunctionResult::Failure(e) => {
            assert_eq!(
                e.code, "queue_not_set",
                "Expected queue_not_set, got: {}",
                e.code
            );
            assert_eq!(e.message, "Queue name is required");
        }
        FunctionResult::Success(_) => panic!("Expected failure for empty queue name"),
        _ => panic!("Unexpected result variant"),
    }
}

// ---------------------------------------------------------------------------
// E2E: flaky function recovers after redrive
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_flaky_function_recovers_after_redrive() {
    // A function that fails 5 times then succeeds.
    // With max_retries=2, the first run (3 attempts) exhausts retries → DLQ.
    // After redrive, it gets 3 more attempts (calls 4,5,6). Call 6 succeeds.
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let fail_count = Arc::new(AtomicU64::new(0));
    let success_count = Arc::new(AtomicU64::new(0));
    // Fail first 5 times, succeed on 6th
    register_flaky_function(
        &engine,
        "e2e::flaky_processor",
        fail_count.clone(),
        success_count.clone(),
        5,
    );

    let module = QueueCoreModule::create(engine.clone(), Some(queue_config_with_fast_retries()))
        .await
        .expect("create should succeed");

    module.register_functions(engine.clone());
    module
        .initialize()
        .await
        .expect("initialize should succeed");

    enqueue(
        &engine,
        "orders",
        "e2e::flaky_processor",
        json!({ "task": "flaky_operation" }),
    )
    .await
    .expect("enqueue should succeed");

    // Wait for first round of retries to exhaust (3 attempts)
    tokio::time::sleep(Duration::from_millis(2000)).await;

    assert_eq!(
        dlq_count(&engine, "orders").await,
        1,
        "Message should land in DLQ after first round"
    );

    // Redrive attempt 1 — calls 4, 5, 6: fail, fail, succeed on 6th
    invoke_redrive(&engine, "orders").await;
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // If still not succeeded, redrive once more
    if success_count.load(Ordering::SeqCst) == 0 {
        invoke_redrive(&engine, "orders").await;
        tokio::time::sleep(Duration::from_millis(2000)).await;
    }

    let total_fails = fail_count.load(Ordering::SeqCst);
    let total_success = success_count.load(Ordering::SeqCst);

    assert!(
        total_success >= 1,
        "Flaky function should eventually succeed after redrives, \
         fails={total_fails}, successes={total_success}"
    );
}

// ---------------------------------------------------------------------------
// E2E: redrive result contains queue name and count
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_redrive_result_contains_metadata() {
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let fail_count = Arc::new(AtomicU64::new(0));
    register_failing_function(&engine, "e2e::meta_handler", fail_count.clone());

    let module = QueueCoreModule::create(engine.clone(), Some(queue_config_with_fast_retries()))
        .await
        .expect("create should succeed");

    module.register_functions(engine.clone());
    module
        .initialize()
        .await
        .expect("initialize should succeed");

    // Send 2 messages that will fail
    for i in 0..2 {
        enqueue(&engine, "orders", "e2e::meta_handler", json!({ "item": i }))
            .await
            .expect("enqueue should succeed");
    }

    tokio::time::sleep(Duration::from_millis(3000)).await;
    assert_eq!(dlq_count(&engine, "orders").await, 2);

    // Swap to success handler before redrive
    let success_count = Arc::new(AtomicU64::new(0));
    register_counting_function(&engine, "e2e::meta_handler", success_count.clone());

    let result = invoke_redrive(&engine, "orders").await;
    match result {
        FunctionResult::Success(Some(value)) => {
            // The RedriveResult is serialized as JSON with queue and redriven fields
            assert_eq!(
                value.get("queue").and_then(|v| v.as_str()),
                Some("orders"),
                "Result should contain the queue name"
            );
            assert_eq!(
                value.get("redriven").and_then(|v| v.as_u64()),
                Some(2),
                "Result should report 2 redriven messages"
            );
        }
        FunctionResult::Failure(e) => panic!("Expected success, got failure: {:?}", e),
        _ => panic!("Unexpected result variant"),
    }
}

// ---------------------------------------------------------------------------
// CLI trigger: argument parsing and connection tests
// ---------------------------------------------------------------------------

#[test]
fn cli_trigger_subcommand_parses_correctly() {
    use std::process::Command;

    let output = Command::new(env!("CARGO_BIN_EXE_iii"))
        .args(["trigger", "--help"])
        .output()
        .expect("failed to execute");

    assert!(output.status.success(), "trigger --help should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("function-id"),
        "Help should mention --function-id, got: {}",
        stdout
    );
    assert!(
        stdout.contains("payload"),
        "Help should mention --payload, got: {}",
        stdout
    );
}

#[test]
fn cli_trigger_missing_required_args_fails() {
    use std::process::Command;

    let output = Command::new(env!("CARGO_BIN_EXE_iii"))
        .args(["trigger"])
        .output()
        .expect("failed to execute");

    assert!(
        !output.status.success(),
        "trigger without required args should fail"
    );
}

#[test]
fn cli_trigger_invalid_json_payload_fails() {
    use std::process::Command;

    let output = Command::new(env!("CARGO_BIN_EXE_iii"))
        .args([
            "trigger",
            "--function-id",
            "test::fn",
            "--payload",
            "not-valid-json",
            "--port",
            "19876",
        ])
        .output()
        .expect("failed to execute");

    assert!(
        !output.status.success(),
        "trigger with invalid JSON should fail"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid JSON") || stderr.contains("JSON"),
        "Should mention JSON error, got: {}",
        stderr
    );
}

#[test]
fn cli_trigger_connection_refused_fails_gracefully() {
    use std::process::Command;

    let output = Command::new(env!("CARGO_BIN_EXE_iii"))
        .args([
            "trigger",
            "--function-id",
            "iii::queue::redrive",
            "--payload",
            r#"{"queue":"orders"}"#,
            "--port",
            "19877",
            "--timeout-ms",
            "2000",
        ])
        .output()
        .expect("failed to execute");

    assert!(
        !output.status.success(),
        "trigger to unreachable port should fail"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Failed to connect")
            || stderr.contains("connect")
            || stderr.contains("Timed out")
            || stderr.contains("timeout"),
        "Should mention connection failure or timeout, got: {}",
        stderr
    );
}

// ---------------------------------------------------------------------------
// CLI: backward compatibility — no subcommand still works
// ---------------------------------------------------------------------------

#[test]
fn cli_no_subcommand_with_version_still_works() {
    use std::process::Command;

    let output = Command::new(env!("CARGO_BIN_EXE_iii"))
        .args(["--version"])
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "iii --version should still work without subcommand"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "Version output should not be empty"
    );
}
