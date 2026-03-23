mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::json;
use tokio::sync::Mutex;

use iii::{
    engine::Engine,
    modules::{module::Module, queue::QueueCoreModule},
};

use common::queue_helpers::{
    dlq_count, dlq_messages, enqueue, register_failing_function,
    register_failing_function_with_timestamps,
};

// ---------------------------------------------------------------------------
// QBLT-02: Retry backoff timing is exponential
// ---------------------------------------------------------------------------
//
// The function queue consumer path goes through nack -> delayed sorted set ->
// move_delayed_to_waiting (uses SystemTime::now()) -> re-pop. This requires
// real wall-clock time (not start_paused) for the delayed-to-waiting transition.
//
// Using standard queue with small backoff_ms for fast execution.
// With max_retries=4 (-> max_attempts=4): 4 handler calls, 3 retry intervals.
// Backoff formula: backoff_ms * 2^(attempts_made - 1)
// Intervals: 50ms, 100ms, 200ms

#[tokio::test]
async fn retry_backoff_timing_is_exponential() {
    let config = json!({
        "queue_configs": {
            "backoff-test": {
                "type": "standard",
                "concurrency": 1,
                "max_retries": 4,
                "backoff_ms": 50,
                "poll_interval_ms": 25
            }
        }
    });

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    let timestamps: Arc<Mutex<Vec<std::time::Instant>>> = Arc::new(Mutex::new(Vec::new()));
    register_failing_function_with_timestamps(
        &engine,
        "test::backoff_handler",
        call_count.clone(),
        timestamps.clone(),
    );

    let module = QueueCoreModule::create(engine.clone(), Some(config))
        .await
        .expect("QueueCoreModule::create should succeed");
    module.initialize().await.expect("init should succeed");

    enqueue(
        &engine,
        "backoff-test",
        "test::backoff_handler",
        json!({"task": "test_backoff"}),
    )
    .await
    .expect("enqueue should succeed");

    // Wait for all retries to exhaust.
    // Total backoff: 50 + 100 + 200 = 350ms plus poll intervals (~75ms) + overhead.
    // 3 seconds gives ample headroom.
    tokio::time::sleep(Duration::from_secs(3)).await;

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        4,
        "Expected 4 invocations (1 initial + 3 retries)"
    );

    let ts = timestamps.lock().await;
    assert_eq!(ts.len(), 4, "Should have 4 timestamps recorded");

    // Interval between 1st and 2nd call: ~50ms (backoff_ms * 2^0) + poll overhead
    let interval_1 = ts[1].duration_since(ts[0]);
    // Interval between 2nd and 3rd call: ~100ms (backoff_ms * 2^1) + poll overhead
    let interval_2 = ts[2].duration_since(ts[1]);
    // Interval between 3rd and 4th call: ~200ms (backoff_ms * 2^2) + poll overhead
    let interval_3 = ts[3].duration_since(ts[2]);

    // Threshold assertions with generous tolerance for poll interval + scheduling overhead.
    // The backoff delay is the dominant factor; poll_interval_ms (25ms) adds some jitter.
    // Lower bound: at least the backoff delay
    // Upper bound: backoff delay + poll interval + 100ms scheduling tolerance
    assert!(
        interval_1 >= Duration::from_millis(50) && interval_1 <= Duration::from_millis(250),
        "1st retry interval should be ~50ms + overhead, got {:?}",
        interval_1
    );
    assert!(
        interval_2 >= Duration::from_millis(100) && interval_2 <= Duration::from_millis(350),
        "2nd retry interval should be ~100ms + overhead, got {:?}",
        interval_2
    );
    assert!(
        interval_3 >= Duration::from_millis(200) && interval_3 <= Duration::from_millis(500),
        "3rd retry interval should be ~200ms + overhead, got {:?}",
        interval_3
    );

    // Verify exponential ratio pattern: each interval should be ~2x the previous.
    // With real-time jitter, use a wider tolerance band.
    let ratio_1_2 = interval_2.as_millis() as f64 / interval_1.as_millis() as f64;
    let ratio_2_3 = interval_3.as_millis() as f64 / interval_2.as_millis() as f64;
    assert!(
        ratio_1_2 >= 1.2 && ratio_1_2 <= 3.5,
        "Interval ratio 2/1 should be ~2.0, got {:.2}",
        ratio_1_2
    );
    assert!(
        ratio_2_3 >= 1.2 && ratio_2_3 <= 3.5,
        "Interval ratio 3/2 should be ~2.0, got {:.2}",
        ratio_2_3
    );
}

// ---------------------------------------------------------------------------
// QBLT-03: DLQ exhaustion preserves payload and metadata
// ---------------------------------------------------------------------------
//
// After retry exhaustion via the function queue consumer nack path, reads DLQ
// content and verifies the original payload, error string, and attempt count.
// Uses real wall-clock time since the nack path uses SystemTime for delayed jobs.

#[tokio::test]
async fn dlq_exhaustion_preserves_payload_and_metadata() {
    let config = json!({
        "queue_configs": {
            "dlq-test": {
                "type": "standard",
                "concurrency": 1,
                "max_retries": 2,
                "backoff_ms": 50,
                "poll_interval_ms": 25
            }
        }
    });

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    register_failing_function(&engine, "test::dlq_handler", call_count.clone());

    let module = QueueCoreModule::create(engine.clone(), Some(config))
        .await
        .expect("QueueCoreModule::create should succeed");
    module.initialize().await.expect("init should succeed");

    // Distinctive payload for verification
    let sent_payload = json!({
        "order_id": 42,
        "items": [{"sku": "ABC-123", "qty": 3}],
        "metadata": {"source": "test", "timestamp": 1234567890}
    });

    enqueue(
        &engine,
        "dlq-test",
        "test::dlq_handler",
        sent_payload.clone(),
    )
    .await
    .expect("enqueue should succeed");

    // Wait for exhaustion: 50ms backoff after 1st fail, then DLQ after 2nd fail.
    // With poll_interval=25ms and overhead, 2 seconds is generous.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify handler was called exactly 2 times (1 initial + 1 retry for max_attempts=2)
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        2,
        "Handler should be called exactly 2 times (1 initial + 1 retry for max_retries=2/max_attempts=2)"
    );

    // Verify DLQ count
    assert_eq!(
        dlq_count(&engine, "dlq-test").await,
        1,
        "Exactly one message should be in DLQ"
    );

    // Read DLQ content and verify structure
    let messages = dlq_messages(&engine, "dlq-test", 10).await;
    assert_eq!(messages.len(), 1, "Exactly one message should be in DLQ");

    let dlq_entry = &messages[0];

    // Verify payload preservation: the "data" field inside "job" should match sent_payload
    let job = &dlq_entry["job"];
    assert_eq!(
        job["data"], sent_payload,
        "DLQ job data should match original payload"
    );

    // Verify attempt count
    assert_eq!(
        job["attempts_made"], 2,
        "attempts_made should equal max_retries (max_attempts)"
    );
    assert_eq!(
        job["max_attempts"], 2,
        "max_attempts should match config max_retries"
    );

    // Verify error string is present and non-empty.
    // In the function queue nack path, the error string is "function call failed"
    // (hardcoded in BuiltinQueueAdapter::nack_function_queue).
    let error = dlq_entry["error"]
        .as_str()
        .expect("error should be a string");
    assert!(!error.is_empty(), "error reason should not be empty");

    // Verify failed_at timestamp exists and is a positive number
    assert!(
        dlq_entry["failed_at"].is_u64(),
        "failed_at should be a u64 timestamp"
    );
    assert!(
        dlq_entry["failed_at"].as_u64().unwrap() > 0,
        "failed_at should be positive"
    );
}

// ---------------------------------------------------------------------------
// QBLT-05: max_retries=0 sends directly to DLQ after single attempt
// ---------------------------------------------------------------------------
//
// With max_retries=0 (-> max_attempts=0), the handler is called once. On
// failure, nack increments attempts_made to 1, is_exhausted(1>=0) = true,
// and the message goes directly to DLQ. No retries occur.
//
// start_paused works here because there are no delayed retries -- the job goes
// straight to DLQ after the first nack.

#[tokio::test(start_paused = true)]
async fn max_retries_zero_sends_directly_to_dlq() {
    let config = json!({
        "queue_configs": {
            "zero-retry": {
                "type": "standard",
                "concurrency": 1,
                "max_retries": 0,
                "backoff_ms": 100,
                "poll_interval_ms": 50
            }
        }
    });

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let call_count = Arc::new(AtomicU64::new(0));
    register_failing_function(&engine, "test::zero_retry_handler", call_count.clone());

    let module = QueueCoreModule::create(engine.clone(), Some(config))
        .await
        .expect("QueueCoreModule::create should succeed");
    module.initialize().await.expect("init should succeed");

    // Verify DLQ starts empty
    assert_eq!(
        dlq_count(&engine, "zero-retry").await,
        0,
        "DLQ should start empty"
    );

    enqueue(
        &engine,
        "zero-retry",
        "test::zero_retry_handler",
        json!({"task": "should_dlq_immediately"}),
    )
    .await
    .expect("enqueue should succeed");

    // Wait for processing
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify exactly 1 handler invocation (initial attempt, no retries)
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "Handler should be called exactly once (no retries with max_retries=0)"
    );

    // Verify message in DLQ
    assert_eq!(
        dlq_count(&engine, "zero-retry").await,
        1,
        "Message should be in DLQ after single failed attempt"
    );

    // Wait a bit more to confirm no additional invocations
    let calls_before = call_count.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        calls_before,
        "No further invocations should occur after DLQ"
    );

    // Verify DLQ content
    let messages = dlq_messages(&engine, "zero-retry", 10).await;
    assert_eq!(messages.len(), 1, "Exactly one message should be in DLQ");
    let job = &messages[0]["job"];
    assert_eq!(
        job["max_attempts"], 0,
        "max_attempts should be 0 for max_retries=0"
    );
    assert_eq!(
        job["attempts_made"], 1,
        "attempts_made should be 1 (one failed attempt)"
    );
    assert_eq!(
        job["data"]["task"], "should_dlq_immediately",
        "original payload preserved"
    );
}
