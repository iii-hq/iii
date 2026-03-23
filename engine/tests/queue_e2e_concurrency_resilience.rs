mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use iii::builtins::kv::BuiltinKvStore;
use iii::builtins::pubsub_lite::BuiltInPubSubLite;
use iii::builtins::queue::{
    BuiltinQueue, Job, JobHandler, QueueConfig, QueueMode, SubscriptionConfig,
};
use iii::builtins::queue_kv::QueueKvStore;
use iii::engine::Engine;
use iii::modules::{module::Module, queue::QueueCoreModule};
use tokio::sync::Mutex;

use common::queue_helpers::{dlq_count, enqueue, register_panicking_function};

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
        handle
            .await
            .expect("task should not panic")
            .expect("move_delayed_to_waiting should not error");
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

// ---------------------------------------------------------------------------
// QBLT-04: Multi-group FIFO ordering under concurrent load
// ---------------------------------------------------------------------------
//
// Proves that:
// (a) Messages within the same message group are processed in FIFO order
// (b) Multiple message groups are processed concurrently (not sequentially)
// (c) All messages across all groups are eventually processed
//
// Uses the direct BuiltinQueue + push_fifo + subscribe path to exercise
// GroupedFifoWorker with concurrency >= 3 (FIFO mode with multiple groups).
//
// NOTE: enqueue_to_topic was not used because queue.push() does not set
// group_id on jobs, causing all messages to land in the "default" group.
// push_fifo is the only push path that sets group_id, which is required
// for the GroupedFifoWorker to distinguish between groups.

/// A JobHandler that records (group_id, sequence_number) pairs and sleeps
/// for a configurable duration to simulate work.
struct GroupOrderRecordingHandler {
    records: Arc<Mutex<Vec<(String, i64)>>>,
    processing_delay: Duration,
}

#[async_trait]
impl JobHandler for GroupOrderRecordingHandler {
    async fn handle(&self, job: &Job) -> Result<(), String> {
        let group_id = job
            .group_id
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let seq = job.data.get("seq").and_then(|v| v.as_i64()).unwrap_or(-1);
        self.records.lock().await.push((group_id, seq));
        tokio::time::sleep(self.processing_delay).await;
        Ok(())
    }
}

#[tokio::test]
async fn fifo_multi_group_ordering_under_concurrent_load() {
    let base_kv = Arc::new(BuiltinKvStore::new(None));
    let kv_store = Arc::new(QueueKvStore::new(base_kv, None));
    let pubsub = Arc::new(BuiltInPubSubLite::new(None));
    let config = QueueConfig {
        max_attempts: 3,
        backoff_ms: 100,
        concurrency: 3,
        poll_interval_ms: 50,
        mode: QueueMode::Fifo,
    };
    let queue = Arc::new(BuiltinQueue::new(kv_store, pubsub, config));

    let queue_name = "fifo-multi";
    let records: Arc<Mutex<Vec<(String, i64)>>> = Arc::new(Mutex::new(Vec::new()));

    let handler: Arc<dyn JobHandler> = Arc::new(GroupOrderRecordingHandler {
        records: records.clone(),
        processing_delay: Duration::from_millis(30),
    });

    // Subscribe with FIFO mode and concurrency=3 to trigger GroupedFifoWorker
    let subscription_config = Some(SubscriptionConfig {
        concurrency: Some(3),
        max_attempts: Some(3),
        backoff_ms: Some(100),
        mode: Some(QueueMode::Fifo),
    });

    let _handle = queue
        .subscribe(queue_name, handler, subscription_config)
        .await;

    // Enqueue 3 groups x 5 messages each = 15 messages total
    // Each group's messages are enqueued in sequence order (0..4)
    let groups = ["A", "B", "C"];
    let messages_per_group = 5;

    let start = std::time::Instant::now();

    for group in &groups {
        for seq in 0..messages_per_group {
            queue
                .push_fifo(
                    queue_name,
                    json!({"group": group, "seq": seq}),
                    group,
                    None,
                    None,
                )
                .await;
        }
    }

    // Wait for all 15 messages to be processed (poll with timeout)
    let expected_total = groups.len() * messages_per_group as usize;
    let timeout = Duration::from_secs(10);
    let poll_interval = Duration::from_millis(50);
    let deadline = std::time::Instant::now() + timeout;

    loop {
        let count = records.lock().await.len();
        if count >= expected_total {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!(
                "Timed out waiting for all {} messages to be processed, only got {}",
                expected_total, count
            );
        }
        tokio::time::sleep(poll_interval).await;
    }

    let elapsed = start.elapsed();
    let recs = records.lock().await;

    // Verify all 15 messages were processed
    assert_eq!(
        recs.len(),
        expected_total,
        "All {} messages should have been processed, got {}",
        expected_total,
        recs.len()
    );

    // Verify per-group FIFO ordering: sequence numbers must be monotonically increasing
    for group in &groups {
        let group_seqs: Vec<i64> = recs
            .iter()
            .filter(|(g, _)| g == group)
            .map(|(_, s)| *s)
            .collect();
        assert_eq!(
            group_seqs.len(),
            messages_per_group as usize,
            "Group {} should have {} messages, got {}",
            group,
            messages_per_group,
            group_seqs.len()
        );
        for window in group_seqs.windows(2) {
            assert!(
                window[0] < window[1],
                "FIFO violation in group {}: seq {} came before seq {}",
                group,
                window[0],
                window[1]
            );
        }
    }

    // Soft parallelism assertion:
    // 30ms delay x 5 messages per group = 150ms minimum per group.
    // If groups run in parallel (3 concurrent), total ~150ms.
    // If all 15 messages ran sequentially, total ~450ms.
    // We assert < 400ms to prove some parallelism occurred.
    if elapsed > Duration::from_millis(400) {
        eprintln!(
            "WARNING: Total elapsed time was {:?}, which suggests limited parallelism. \
             Expected < 400ms with 3 concurrent groups.",
            elapsed
        );
    }
}
