use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iii::{
    engine::Engine,
    function::{Function, FunctionResult},
    modules::{module::Module, queue::QueueCoreModule},
};

/// Creates an Engine with a QueueCoreModule initialized from the given config.
pub async fn create_engine_with_queue(config: Value) -> Arc<Engine> {
    iii::modules::observability::metrics::ensure_default_meter();
    let engine = Arc::new(Engine::new());
    let module = QueueCoreModule::create(engine.clone(), Some(config))
        .await
        .expect("QueueCoreModule::create should succeed");
    module
        .initialize()
        .await
        .expect("Module initialization should succeed");
    engine
}

/// Enqueue a message to a function queue via the engine's queue_module.
pub async fn enqueue(
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

/// Returns the number of messages in the DLQ for a function queue.
pub async fn dlq_count(engine: &Engine, queue_name: &str) -> u64 {
    let guard = engine.queue_module.read().await;
    let qm = guard
        .as_ref()
        .expect("queue_module should be set after initialize");
    qm.function_queue_dlq_count(queue_name)
        .await
        .expect("dlq_count should not fail")
}

/// Returns up to `count` DLQ messages for a function queue as parsed JSON Values.
/// Each message has structure: { "job": {...}, "error": "...", "failed_at": u64 }
pub async fn dlq_messages(engine: &Engine, queue_name: &str, count: usize) -> Vec<Value> {
    let guard = engine.queue_module.read().await;
    let qm = guard
        .as_ref()
        .expect("queue_module should be set after initialize");
    qm.function_queue_dlq_messages(queue_name, count)
        .await
        .expect("dlq_messages should not fail")
}

/// Registers a test function whose handler increments a shared counter on
/// every invocation.
pub fn register_counting_function(
    engine: &Arc<Engine>,
    function_id: &str,
    counter: Arc<AtomicU64>,
) {
    let function = Function {
        handler: Arc::new(move |_invocation_id, _input, _session| {
            let count = counter.clone();
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                FunctionResult::Success(Some(json!({ "ok": true })))
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("counting test handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
}

/// Registers a test function that records the value of a named field from
/// each invocation payload into a shared ordered list.
pub fn register_order_recording_function(
    engine: &Arc<Engine>,
    function_id: &str,
    field_name: &'static str,
    record: Arc<Mutex<Vec<Value>>>,
) {
    let function = Function {
        handler: Arc::new(move |_invocation_id, input, _session| {
            let rec = record.clone();
            Box::pin(async move {
                let value = input.get(field_name).cloned().unwrap_or(Value::Null);
                rec.lock().await.push(value);
                FunctionResult::Success(Some(json!({ "ok": true })))
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("order recording test handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
}

/// Registers a test function that sleeps for `delay` before succeeding.
/// Records invocation timestamps (start time) in `timestamps`.
pub fn register_slow_function(
    engine: &Arc<Engine>,
    function_id: &str,
    delay: Duration,
    timestamps: Arc<Mutex<Vec<std::time::Instant>>>,
) {
    let function = Function {
        handler: Arc::new(move |_invocation_id, _input, _session| {
            let ts = timestamps.clone();
            let d = delay;
            Box::pin(async move {
                ts.lock().await.push(std::time::Instant::now());
                tokio::time::sleep(d).await;
                FunctionResult::Success(Some(json!({ "ok": true })))
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("slow test handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
}

/// Registers a test function that records `(group_id, sequence_number)` tuples
/// from each invocation, then sleeps for `processing_delay` to simulate work.
/// This makes concurrent group processing observable in timing assertions.
pub fn register_group_order_recording_function(
    engine: &Arc<Engine>,
    function_id: &str,
    group_field: &'static str,
    seq_field: &'static str,
    records: Arc<Mutex<Vec<(String, i64)>>>,
    processing_delay: Duration,
) {
    let function = Function {
        handler: Arc::new(move |_invocation_id, input, _session| {
            let rec = records.clone();
            let delay = processing_delay;
            Box::pin(async move {
                let group_id = input
                    .get(group_field)
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let seq = input.get(seq_field).and_then(|v| v.as_i64()).unwrap_or(-1);
                rec.lock().await.push((group_id, seq));
                tokio::time::sleep(delay).await;
                FunctionResult::Success(Some(json!({ "ok": true })))
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("group order recording test handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
}

/// Registers a test function that always fails (returns FunctionResult::Failure).
/// Records the number of invocations in `call_count`.
pub fn register_failing_function(
    engine: &Arc<Engine>,
    function_id: &str,
    call_count: Arc<AtomicU64>,
) {
    let function = Function {
        handler: Arc::new(move |_invocation_id, _input, _session| {
            let count = call_count.clone();
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                FunctionResult::Failure(iii::protocol::ErrorBody {
                    code: "FAIL".to_string(),
                    message: "intentional failure".to_string(),
                    stacktrace: None,
                })
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("always-failing test handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
}

/// Registers a test function that always fails AND records the std::time::Instant
/// of each invocation. Use for measuring real wall-clock backoff timing in tests
/// that exercise the function queue consumer path (nack -> delayed -> re-poll).
pub fn register_failing_function_with_timestamps(
    engine: &Arc<Engine>,
    function_id: &str,
    call_count: Arc<AtomicU64>,
    timestamps: Arc<Mutex<Vec<std::time::Instant>>>,
) {
    let function = Function {
        handler: Arc::new(move |_invocation_id, _input, _session| {
            let count = call_count.clone();
            let ts = timestamps.clone();
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                ts.lock().await.push(std::time::Instant::now());
                FunctionResult::Failure(iii::protocol::ErrorBody {
                    code: "FAIL".to_string(),
                    message: "intentional failure for timing".to_string(),
                    stacktrace: None,
                })
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("timing-recording failing handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
}

/// Registers a test function that stores every received `Value` payload into a shared
/// `Arc<Mutex<Vec<Value>>>`. Captures the entire input, not a single field.
pub fn register_payload_capturing_function(
    engine: &Arc<Engine>,
    function_id: &str,
    captured: Arc<Mutex<Vec<Value>>>,
) {
    let function = Function {
        handler: Arc::new(move |_invocation_id, input, _session| {
            let store = captured.clone();
            Box::pin(async move {
                store.lock().await.push(input);
                FunctionResult::Success(Some(json!({ "ok": true })))
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("payload capturing test handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
}

/// Registers a condition function that returns `json!(true)` when
/// `input[field] == expected_value`, and `json!(false)` otherwise.
pub fn register_condition_function(
    engine: &Arc<Engine>,
    function_id: &str,
    field: &'static str,
    expected_value: Value,
) {
    let expected = expected_value.clone();
    let function = Function {
        handler: Arc::new(move |_invocation_id, input, _session| {
            let exp = expected.clone();
            Box::pin(async move {
                let matches = input.get(field).map(|v| *v == exp).unwrap_or(false);
                FunctionResult::Success(Some(json!(matches)))
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("condition function".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
}

/// Registers a test function that panics when input contains `{"panic": true}`,
/// otherwise increments the `success_count` counter and succeeds.
pub fn register_panicking_function(
    engine: &Arc<Engine>,
    function_id: &str,
    success_count: Arc<AtomicU64>,
) {
    let function = Function {
        handler: Arc::new(move |_invocation_id, input, _session| {
            let count = success_count.clone();
            Box::pin(async move {
                let should_panic = input
                    .get("panic")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if should_panic {
                    panic!("intentional test panic");
                }
                count.fetch_add(1, Ordering::SeqCst);
                FunctionResult::Success(Some(json!({ "ok": true })))
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("panicking test handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
}

/// Calls `engine.call("enqueue", ...)` with a topic and data payload,
/// mapping the result to `anyhow::Result<()>`.
pub async fn enqueue_to_topic(engine: &Engine, topic: &str, data: Value) -> anyhow::Result<()> {
    use iii::engine::EngineTrait;
    let result = engine
        .call("enqueue", json!({"topic": topic, "data": data}))
        .await;
    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow::anyhow!(
            "enqueue to topic failed: {} - {}",
            e.code,
            e.message
        )),
    }
}

/// Default builtin queue config with "default" (standard) and "payment" (fifo) queues.
pub fn builtin_queue_config() -> Value {
    json!({
        "queue_configs": {
            "default": {
                "type": "standard",
                "concurrency": 3,
                "max_retries": 2,
                "backoff_ms": 100,
                "poll_interval_ms": 50
            },
            "payment": {
                "type": "fifo",
                "message_group_field": "transaction_id",
                "concurrency": 1,
                "max_retries": 2,
                "backoff_ms": 100,
                "poll_interval_ms": 50
            }
        }
    })
}
