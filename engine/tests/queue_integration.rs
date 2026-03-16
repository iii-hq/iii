mod common;

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

use common::queue_helpers::{
    builtin_queue_config, create_engine_with_queue, dlq_count, enqueue, register_counting_function,
    register_failing_function, register_order_recording_function, register_slow_function,
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enqueue_to_standard_queue_succeeds() {
    let engine = create_engine_with_queue(builtin_queue_config()).await;

    let result = enqueue(&engine, "default", "test::handler", json!({"key": "value"})).await;

    assert!(result.is_ok(), "Enqueue to 'default' should succeed");
}

#[tokio::test]
async fn enqueue_to_unknown_queue_fails() {
    let engine = create_engine_with_queue(builtin_queue_config()).await;

    let result = enqueue(
        &engine,
        "nonexistent",
        "test::handler",
        json!({"key": "value"}),
    )
    .await;

    assert!(result.is_err(), "Enqueue to unknown queue should fail");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found"),
        "Error should mention 'not found', got: {err}"
    );
}

#[tokio::test]
async fn enqueue_to_fifo_missing_group_field_fails() {
    let engine = create_engine_with_queue(builtin_queue_config()).await;

    // The "payment" queue is FIFO with message_group_field = "transaction_id".
    // Sending a payload without that field should be rejected.
    let result = enqueue(&engine, "payment", "test::handler", json!({"amount": 100})).await;

    assert!(
        result.is_err(),
        "Enqueue to FIFO queue without group field should fail"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("transaction_id"),
        "Error should reference the missing field, got: {err}"
    );
}

#[tokio::test]
async fn enqueue_to_fifo_null_group_field_fails() {
    let engine = create_engine_with_queue(builtin_queue_config()).await;

    let result = enqueue(
        &engine,
        "payment",
        "test::handler",
        json!({"transaction_id": null, "amount": 100}),
    )
    .await;

    assert!(
        result.is_err(),
        "Enqueue to FIFO queue with null group field should fail"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("null"),
        "Error should mention null, got: {err}"
    );
}

#[tokio::test]
async fn full_roundtrip_enqueue_consume_invoke() {
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    register_counting_function(&engine, "test::handler", call_count.clone());

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");

    // Initialize starts the consumer loops in the background.
    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    // Enqueue a single message to the standard queue.
    enqueue(
        &engine,
        "default",
        "test::handler",
        json!({"task": "process_order", "order_id": 42}),
    )
    .await
    .expect("Enqueue should succeed");

    // Allow the consumer loop time to poll, dequeue, and invoke the function.
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "The registered function should have been invoked exactly once"
    );
}

#[tokio::test]
async fn full_roundtrip_fifo_preserves_order() {
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let invocation_order: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    register_order_recording_function(
        &engine,
        "test::fifo_handler",
        "seq",
        invocation_order.clone(),
    );

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    // Enqueue 5 messages to the FIFO queue with the same transaction_id
    // (same message group) so they are processed sequentially.
    let message_count: usize = 5;
    for i in 0..message_count {
        enqueue(
            &engine,
            "payment",
            "test::fifo_handler",
            json!({
                "transaction_id": "txn-abc",
                "seq": i,
            }),
        )
        .await
        .expect("Enqueue should succeed");
    }

    // Wait for all messages to be consumed and processed.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let recorded = invocation_order.lock().await;
    assert_eq!(
        recorded.len(),
        message_count,
        "All {message_count} messages should have been processed, but got {}",
        recorded.len()
    );

    // Verify FIFO ordering: seq values should arrive in 0, 1, 2, 3, 4 order.
    let expected: Vec<Value> = (0..message_count as i64).map(|i| json!(i)).collect();
    assert_eq!(
        *recorded, expected,
        "FIFO queue should preserve insertion order"
    );
}

#[tokio::test]
async fn retry_exhaustion_stops_redelivery() {
    // The "default" queue has max_retries=2, so a permanently failing function
    // should be invoked at most 1 (initial) + 2 (retries) = 3 times.
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    register_failing_function(&engine, "test::always_fails", call_count.clone());

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    enqueue(
        &engine,
        "default",
        "test::always_fails",
        json!({"key": "should_exhaust"}),
    )
    .await
    .expect("Enqueue should succeed");

    // Wait long enough for initial attempt + retries + backoff intervals.
    // max_retries=2, backoff_ms=100, poll_interval_ms=50
    // Worst case: 3 attempts * (100ms backoff + 50ms poll) + margin
    tokio::time::sleep(Duration::from_millis(2000)).await;

    let total_calls = call_count.load(Ordering::SeqCst);
    assert!(
        total_calls >= 1 && total_calls <= 3,
        "Expected 1-3 invocations (1 initial + up to 2 retries), got {total_calls}"
    );

    // Wait a bit more to confirm no further redeliveries after exhaustion.
    let calls_before = total_calls;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let calls_after = call_count.load(Ordering::SeqCst);

    assert_eq!(
        calls_before, calls_after,
        "No further invocations should occur after retry exhaustion, \
         but got {calls_after} (was {calls_before})"
    );
}

#[tokio::test]
async fn exhausted_message_lands_in_dlq() {
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    register_failing_function(&engine, "test::dlq_target", call_count.clone());

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    // DLQ should start empty
    assert_eq!(dlq_count(&engine, "default").await, 0);

    enqueue(
        &engine,
        "default",
        "test::dlq_target",
        json!({"should_land_in": "dlq"}),
    )
    .await
    .expect("Enqueue should succeed");

    // Wait for retries to exhaust (max_retries=2, backoff_ms=100)
    tokio::time::sleep(Duration::from_millis(3000)).await;

    let count = dlq_count(&engine, "default").await;
    assert_eq!(
        count, 1,
        "Exactly one message should be in the DLQ after retry exhaustion, got {count}"
    );
}

#[tokio::test]
async fn standard_queue_processes_concurrently() {
    // "default" queue has concurrency=3. If we enqueue 3 messages with a
    // 200ms handler, sequential processing would take >= 600ms while
    // concurrent processing takes ~200ms.
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let timestamps: Arc<Mutex<Vec<std::time::Instant>>> = Arc::new(Mutex::new(Vec::new()));
    register_slow_function(
        &engine,
        "test::slow_handler",
        Duration::from_millis(200),
        timestamps.clone(),
    );

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    let start = std::time::Instant::now();

    for i in 0..3 {
        enqueue(&engine, "default", "test::slow_handler", json!({"idx": i}))
            .await
            .expect("Enqueue should succeed");
    }

    // Wait for all 3 to complete
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let ts = timestamps.lock().await;
    assert_eq!(ts.len(), 3, "All 3 messages should have been processed");

    // All 3 handlers should have started within ~200ms of each other
    // (concurrent), not 200ms apart (sequential).
    let first_start = *ts.iter().min().unwrap();
    let last_start = *ts.iter().max().unwrap();
    let spread = last_start.duration_since(first_start);

    assert!(
        spread < Duration::from_millis(400),
        "Concurrent handlers should start close together, but spread was {:?} \
         (start timestamps relative to test start: {:?})",
        spread,
        ts.iter()
            .map(|t| t.duration_since(start))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn nonexistent_function_nacks_without_blocking_queue() {
    // Enqueue a message targeting a function that doesn't exist.
    // The consumer should nack it (function_not_found error) and continue
    // processing subsequent messages for other functions.
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    register_counting_function(&engine, "test::real_handler", call_count.clone());
    // Note: "test::ghost" is NOT registered

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    // Enqueue to a nonexistent function first
    enqueue(&engine, "default", "test::ghost", json!({"should": "fail"}))
        .await
        .expect("Enqueue should succeed (validation is at consume time)");

    // Then enqueue to a real function
    enqueue(
        &engine,
        "default",
        "test::real_handler",
        json!({"should": "succeed"}),
    )
    .await
    .expect("Enqueue should succeed");

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(2000)).await;

    let count = call_count.load(Ordering::SeqCst);
    assert_eq!(
        count, 1,
        "The real handler should have been invoked despite the ghost function failing, got {count}"
    );
}

#[tokio::test]
async fn multiple_queues_operate_independently() {
    // Enqueue to both "default" (standard) and "payment" (fifo) queues
    // simultaneously. Each queue should process its own messages without
    // interference.
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let default_count = Arc::new(AtomicU64::new(0));
    let payment_count = Arc::new(AtomicU64::new(0));
    register_counting_function(&engine, "test::default_handler", default_count.clone());
    register_counting_function(&engine, "test::payment_handler", payment_count.clone());

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");

    module
        .initialize()
        .await
        .expect("Module initialization should succeed");

    // Enqueue 3 messages to each queue
    for i in 0..3 {
        enqueue(
            &engine,
            "default",
            "test::default_handler",
            json!({"idx": i}),
        )
        .await
        .expect("Enqueue to default should succeed");

        enqueue(
            &engine,
            "payment",
            "test::payment_handler",
            json!({"transaction_id": format!("txn-{i}"), "idx": i}),
        )
        .await
        .expect("Enqueue to payment should succeed");
    }

    tokio::time::sleep(Duration::from_millis(2000)).await;

    let dc = default_count.load(Ordering::SeqCst);
    let pc = payment_count.load(Ordering::SeqCst);

    assert_eq!(
        dc, 3,
        "Default queue should have processed 3 messages, got {dc}"
    );
    assert_eq!(
        pc, 3,
        "Payment queue should have processed 3 messages, got {pc}"
    );
}

#[tokio::test(start_paused = true)]
async fn start_paused_smoke_test() {
    // Verify that tokio's test-util feature is working: time auto-advances
    // past sleeps when there is no other work to do.
    let before = tokio::time::Instant::now();
    tokio::time::sleep(Duration::from_secs(60)).await;
    let elapsed = before.elapsed();

    // With start_paused, the 60-second sleep should resolve near-instantly
    // in wall-clock time, but tokio's internal clock should show 60s elapsed.
    assert!(
        elapsed >= Duration::from_secs(60),
        "tokio time should have auto-advanced by 60s, but elapsed was {:?}",
        elapsed
    );
}
