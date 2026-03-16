mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::json;

use iii::builtins::kv::BuiltinKvStore;
use iii::builtins::pubsub_lite::BuiltInPubSubLite;
use iii::builtins::queue::{BuiltinQueue, QueueConfig};
use iii::builtins::queue_kv::QueueKvStore;
use iii::engine::Engine;
use iii::modules::{module::Module, queue::QueueCoreModule};

use common::queue_helpers::{
    dlq_count, enqueue, register_panicking_function,
};

// ---------------------------------------------------------------------------
// QBLT-06: Handler panic does not crash the consumer loop
// ---------------------------------------------------------------------------
//
// Proves that:
// (a) The consumer loop continues after a handler panic
// (b) The panicked message is nacked and follows the retry/DLQ path
// (c) Subsequent non-panicking messages are processed normally
//
// Uses real wall-clock time because the nack path uses SystemTime::now().

#[tokio::test]
async fn handler_panic_does_not_crash_worker() {
    let config = json!({
        "queue_configs": {
            "default": {
                "type": "standard",
                "concurrency": 3,
                "max_retries": 2,
                "backoff_ms": 100,
                "poll_interval_ms": 50
            }
        }
    });

    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let success_count = Arc::new(AtomicU64::new(0));
    register_panicking_function(&engine, "test::panic_handler", success_count.clone());

    let module = QueueCoreModule::create(engine.clone(), Some(config))
        .await
        .expect("QueueCoreModule::create should succeed");
    module.initialize().await.expect("init should succeed");

    // Enqueue a message that will cause a panic
    enqueue(
        &engine,
        "default",
        "test::panic_handler",
        json!({"panic": true}),
    )
    .await
    .expect("enqueue panicking message should succeed");

    // Wait for the panicking message to be processed and nacked
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Enqueue a normal message to prove the consumer loop survived the panic
    enqueue(
        &engine,
        "default",
        "test::panic_handler",
        json!({"panic": false, "id": "normal-msg"}),
    )
    .await
    .expect("enqueue normal message should succeed");

    // Wait for the normal message to be processed
    tokio::time::sleep(Duration::from_millis(500)).await;

    // The consumer loop must have survived: the normal message was processed
    assert!(
        success_count.load(Ordering::SeqCst) >= 1,
        "At least one normal message should have been processed after the panic, got {}",
        success_count.load(Ordering::SeqCst)
    );

    // Wait for the panicking message to exhaust retries and land in DLQ.
    // max_retries=2 means max_attempts=2, with backoff_ms=100 and exponential backoff:
    // attempt 1: panic -> nack -> delayed (100ms) -> re-poll -> attempt 2: panic -> DLQ
    // 3 seconds gives ample headroom.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let dlq = dlq_count(&engine, "default").await;
    assert!(
        dlq >= 1,
        "Panicked message should eventually land in DLQ after exhausting retries, got {} DLQ entries",
        dlq
    );
}

// ---------------------------------------------------------------------------
// FIX-02: Concurrent move_delayed_to_waiting produces no duplicates
// ---------------------------------------------------------------------------
//
// Verifies the atomic transition fix by calling move_delayed_to_waiting
// concurrently from multiple tasks. Each job should appear in the waiting
// queue exactly once.

#[tokio::test]
async fn move_delayed_to_waiting_no_duplicates() {
    let base_kv = Arc::new(BuiltinKvStore::new(None));
    let kv_store = Arc::new(QueueKvStore::new(base_kv, None));
    let pubsub = Arc::new(BuiltInPubSubLite::new(None));
    let config = QueueConfig::default();
    let queue = Arc::new(BuiltinQueue::new(kv_store.clone(), pubsub, config));

    let queue_name = "test-queue";
    let delayed_key = format!("queue:{}:delayed", queue_name);
    let waiting_key = format!("queue:{}:waiting", queue_name);

    // Add 10 jobs to the delayed sorted set with score=0 (immediately ready)
    for i in 0..10 {
        let job_id = format!("job-{}", i);
        kv_store.zadd(&delayed_key, 0, job_id).await;
    }

    // Verify all 10 jobs are in the delayed set
    let delayed_before = kv_store.zrangebyscore(&delayed_key, 0, i64::MAX).await;
    assert_eq!(
        delayed_before.len(),
        10,
        "Should have 10 jobs in delayed set before move"
    );

    // Spawn 10 concurrent tasks, each calling move_delayed_to_waiting
    let mut handles = Vec::new();
    for _ in 0..10 {
        let q = queue.clone();
        let name = queue_name.to_string();
        handles.push(tokio::spawn(async move {
            q.move_delayed_to_waiting(&name).await
        }));
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.expect("task should not panic").expect("move_delayed_to_waiting should not error");
    }

    // Check: exactly 10 entries in waiting queue (not 20, 30, etc.)
    let waiting_count = kv_store.llen(&waiting_key).await;
    assert_eq!(
        waiting_count, 10,
        "Waiting queue should have exactly 10 entries (no duplicates), got {}",
        waiting_count
    );

    // Check: 0 entries remain in the delayed sorted set
    let delayed_after = kv_store.zrangebyscore(&delayed_key, 0, i64::MAX).await;
    assert_eq!(
        delayed_after.len(),
        0,
        "All jobs should have been moved from delayed set, {} remain",
        delayed_after.len()
    );
}
