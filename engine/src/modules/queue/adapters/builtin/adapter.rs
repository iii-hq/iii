// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{RwLock, mpsc};

use tracing::Instrument;

use crate::{
    builtins::{
        kv::BuiltinKvStore,
        pubsub_lite::BuiltInPubSubLite,
        queue::{
            BuiltinQueue, Job, JobHandler, QueueConfig, QueueMode, SubscriptionConfig,
            SubscriptionHandle,
        },
        queue_kv::QueueKvStore,
    },
    condition::check_condition,
    engine::{Engine, EngineTrait},
    modules::queue::{
        QueueAdapter, SubscriberQueueConfig,
        registry::{QueueAdapterFuture, QueueAdapterRegistration},
    },
    telemetry::SpanExt,
};

struct DeliveryInfo {
    queue_name: String,
    job_id: String,
}

pub struct BuiltinQueueAdapter {
    queue: Arc<BuiltinQueue>,
    engine: Arc<Engine>,
    subscriptions: Arc<RwLock<HashMap<String, SubscriptionHandle>>>,
    delivery_map: Arc<RwLock<HashMap<u64, DeliveryInfo>>>,
    delivery_counter: Arc<AtomicU64>,
    poll_intervals: Arc<RwLock<HashMap<String, u64>>>,
}

struct FunctionHandler {
    engine: Arc<Engine>,
    function_id: String,
    condition_function_id: Option<String>,
}

#[async_trait]
impl JobHandler for FunctionHandler {
    async fn handle(&self, job: &crate::builtins::queue::Job) -> Result<(), String> {
        tracing::debug!(
            job_id = %job.id,
            queue = %job.queue,
            traceparent = ?job.traceparent,
            baggage = ?job.baggage,
            "Queue worker handling job with trace context"
        );

        let span = tracing::info_span!(
            "queue_job",
            otel.name = %format!("queue {}", job.queue),
            job_id = %job.id,
            queue = %job.queue,
            "messaging.system" = "iii-queue",
            "messaging.destination.name" = %job.queue,
            "messaging.operation.type" = "process",
            "baggage.queue" = %job.queue,
            otel.status_code = tracing::field::Empty,
        )
        .with_parent_headers(job.traceparent.as_deref(), job.baggage.as_deref());

        let engine = Arc::clone(&self.engine);
        let function_id = self.function_id.clone();
        let condition_function_id = self.condition_function_id.clone();
        let data = job.data.clone();

        async move {
            if let Some(ref condition_id) = condition_function_id {
                tracing::debug!(
                    condition_function_id = %condition_id,
                    "Checking trigger conditions"
                );
                match check_condition(engine.as_ref(), condition_id, data.clone()).await {
                    Ok(true) => {}
                    Ok(false) => {
                        tracing::debug!(
                            function_id = %function_id,
                            "Condition check failed, skipping handler"
                        );
                        tracing::Span::current().record("otel.status_code", "OK");
                        return Ok(());
                    }
                    Err(err) => {
                        tracing::error!(
                            condition_function_id = %condition_id,
                            error = ?err,
                            "Error invoking condition function"
                        );
                        tracing::Span::current().record("otel.status_code", "ERROR");
                        return Err(format!("Condition function error: {:?}", err));
                    }
                }
            }

            let result = engine.call(&function_id, data).await;
            match &result {
                Ok(_) => {
                    tracing::Span::current().record("otel.status_code", "OK");
                }
                Err(_) => {
                    tracing::Span::current().record("otel.status_code", "ERROR");
                }
            }
            result.map(|_| ()).map_err(|e| format!("{:?}", e))
        }
        .instrument(span)
        .await
    }
}

impl BuiltinQueueAdapter {
    pub fn new(
        kv_store: Arc<QueueKvStore>,
        pubsub: Arc<BuiltInPubSubLite>,
        engine: Arc<Engine>,
        config: QueueConfig,
    ) -> Self {
        let queue = Arc::new(BuiltinQueue::new(kv_store, pubsub, config));
        Self {
            queue,
            engine,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            delivery_map: Arc::new(RwLock::new(HashMap::new())),
            delivery_counter: Arc::new(AtomicU64::new(0)),
            poll_intervals: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Clone for BuiltinQueueAdapter {
    fn clone(&self) -> Self {
        Self {
            queue: Arc::clone(&self.queue),
            engine: Arc::clone(&self.engine),
            subscriptions: Arc::clone(&self.subscriptions),
            delivery_map: Arc::clone(&self.delivery_map),
            delivery_counter: Arc::clone(&self.delivery_counter),
            poll_intervals: Arc::clone(&self.poll_intervals),
        }
    }
}

pub fn make_adapter(engine: Arc<Engine>, config: Option<Value>) -> QueueAdapterFuture {
    Box::pin(async move {
        let queue_config = QueueConfig::from_value(config.as_ref());

        let base_kv = Arc::new(BuiltinKvStore::new(config.clone()));
        let kv_store = Arc::new(QueueKvStore::new(base_kv, config.clone()));
        let pubsub = Arc::new(BuiltInPubSubLite::new(config.clone()));

        let adapter = Arc::new(BuiltinQueueAdapter::new(
            kv_store,
            pubsub,
            engine,
            queue_config,
        ));

        if let Err(e) = adapter.queue.rebuild_from_storage().await {
            tracing::error!(error = ?e, "Failed to rebuild queue state from storage");
        }

        Ok(adapter as Arc<dyn QueueAdapter>)
    })
}

#[async_trait]
impl QueueAdapter for BuiltinQueueAdapter {
    async fn enqueue(
        &self,
        topic: &str,
        data: Value,
        traceparent: Option<String>,
        baggage: Option<String>,
    ) {
        self.queue.push(topic, data, traceparent, baggage).await;
    }

    async fn subscribe(
        &self,
        topic: &str,
        id: &str,
        function_id: &str,
        condition_function_id: Option<String>,
        queue_config: Option<SubscriberQueueConfig>,
    ) {
        let handler: Arc<dyn JobHandler> = Arc::new(FunctionHandler {
            engine: Arc::clone(&self.engine),
            function_id: function_id.to_string(),
            condition_function_id,
        });

        let subscription_config = queue_config.map(|c| SubscriptionConfig {
            concurrency: c.concurrency,
            max_attempts: c.max_retries,
            backoff_ms: c.backoff_delay_ms,
            mode: c.queue_mode.as_ref().map(|m| match m.as_str() {
                "fifo" => QueueMode::Fifo,
                _ => QueueMode::Concurrent,
            }),
        });

        let handle = self
            .queue
            .subscribe(topic, handler, subscription_config)
            .await;

        let mut subs = self.subscriptions.write().await;
        subs.insert(format!("{}:{}", topic, id), handle);

        tracing::debug!(topic = %topic, id = %id, function_id = %function_id, "Subscribed to queue via BuiltinQueueAdapter");
    }

    async fn unsubscribe(&self, topic: &str, id: &str) {
        let key = format!("{}:{}", topic, id);
        let mut subs = self.subscriptions.write().await;

        if let Some(handle) = subs.remove(&key) {
            self.queue.unsubscribe(handle).await;
            tracing::debug!(topic = %topic, id = %id, "Unsubscribed from queue");
        } else {
            tracing::warn!(topic = %topic, id = %id, "No subscription found to unsubscribe");
        }
    }

    async fn redrive_dlq(&self, topic: &str) -> anyhow::Result<u64> {
        Ok(self.queue.dlq_redrive(topic).await)
    }

    async fn dlq_count(&self, topic: &str) -> anyhow::Result<u64> {
        Ok(self.queue.dlq_count(topic).await)
    }

    async fn dlq_messages(
        &self,
        topic: &str,
        count: usize,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        Ok(self.queue.dlq_messages(topic, count).await)
    }

    async fn setup_function_queue(
        &self,
        queue_name: &str,
        config: &crate::modules::queue::FunctionQueueConfig,
    ) -> anyhow::Result<()> {
        self.poll_intervals
            .write()
            .await
            .insert(queue_name.to_string(), config.poll_interval_ms);
        Ok(())
    }

    async fn publish_to_function_queue(
        &self,
        queue_name: &str,
        function_id: &str,
        data: Value,
        message_id: &str,
        max_retries: u32,
        backoff_ms: u64,
        traceparent: Option<String>,
        baggage: Option<String>,
    ) {
        let namespaced_queue = format!("__fn_queue::{}", queue_name);
        let job = Job {
            function_id: Some(function_id.to_string()),
            message_id: Some(message_id.to_string()),
            ..Job::new(
                &namespaced_queue,
                data,
                max_retries,
                backoff_ms,
                traceparent,
                baggage,
            )
        };
        self.queue.push_job(job).await;
    }

    async fn consume_function_queue(
        &self,
        queue_name: &str,
        prefetch: u32,
    ) -> anyhow::Result<mpsc::Receiver<crate::modules::queue::QueueMessage>> {
        use crate::modules::queue::QueueMessage;

        let poll_interval_ms = self
            .poll_intervals
            .read()
            .await
            .get(queue_name)
            .copied()
            .unwrap_or(100);

        let (tx, rx) = mpsc::channel(prefetch as usize);
        let queue = Arc::clone(&self.queue);
        let delivery_map = Arc::clone(&self.delivery_map);
        let delivery_counter = Arc::clone(&self.delivery_counter);
        let namespaced_queue = format!("__fn_queue::{}", queue_name);

        tokio::spawn(async move {
            let poll_interval = std::time::Duration::from_millis(poll_interval_ms);
            loop {
                if tx.is_closed() {
                    break;
                }

                // Move delayed jobs to waiting
                if let Err(e) = queue.move_delayed_to_waiting(&namespaced_queue).await {
                    tracing::error!(error = %e, queue = %namespaced_queue, "Failed to move delayed jobs");
                }

                if let Some(job) = queue.pop(&namespaced_queue).await {
                    let delivery_id = delivery_counter.fetch_add(1, Ordering::SeqCst);

                    delivery_map.write().await.insert(
                        delivery_id,
                        DeliveryInfo {
                            queue_name: namespaced_queue.clone(),
                            job_id: job.id.clone(),
                        },
                    );

                    let msg = QueueMessage {
                        delivery_id,
                        function_id: job.function_id.unwrap_or_default(),
                        data: job.data,
                        attempt: job.attempts_made,
                        message_id: job.message_id,
                        traceparent: job.traceparent,
                        baggage: job.baggage,
                    };

                    if tx.send(msg).await.is_err() {
                        // Channel closed — nack the job so it returns to the queue
                        if let Some(info) = delivery_map.write().await.remove(&delivery_id)
                            && let Err(e) = queue
                                .nack(&info.queue_name, &info.job_id, "consumer channel closed")
                                .await
                        {
                            tracing::error!(error = %e, "Failed to nack stranded job");
                        }
                        break;
                    }
                } else {
                    tokio::time::sleep(poll_interval).await;
                }
            }
        });

        Ok(rx)
    }

    async fn ack_function_queue(&self, _queue_name: &str, delivery_id: u64) -> anyhow::Result<()> {
        let info = self.delivery_map.write().await.remove(&delivery_id);
        if let Some(info) = info {
            self.queue.ack(&info.queue_name, &info.job_id).await?;
        }
        Ok(())
    }

    async fn nack_function_queue(
        &self,
        _queue_name: &str,
        delivery_id: u64,
        _attempt: u32,
        _max_retries: u32,
    ) -> anyhow::Result<()> {
        let info = self.delivery_map.write().await.remove(&delivery_id);
        if let Some(info) = info {
            // BuiltinQueue::nack() handles retry vs DLQ internally
            // using the job's own attempts_made and max_attempts fields.
            self.queue
                .nack(&info.queue_name, &info.job_id, "function call failed")
                .await?;
        }
        Ok(())
    }
}

crate::register_adapter!(
    <QueueAdapterRegistration>
    "modules::queue::BuiltinQueueAdapter",
    make_adapter
);

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use std::collections::HashSet;
    use std::time::Duration;

    use crate::modules::queue::SubscriberQueueConfig;
    use crate::modules::queue::config::FunctionQueueConfig;
    use crate::{
        builtins::queue::{Job, QueueMode},
        function::{Function, FunctionResult},
        protocol::ErrorBody,
    };

    fn make_adapter(engine: Arc<Engine>) -> BuiltinQueueAdapter {
        let base_kv = Arc::new(BuiltinKvStore::new(None));
        let kv_store = Arc::new(QueueKvStore::new(base_kv, None));
        let pubsub = Arc::new(BuiltInPubSubLite::new(None));
        BuiltinQueueAdapter::new(kv_store, pubsub, engine, QueueConfig::default())
    }

    fn register_test_function(engine: &Arc<Engine>, function_id: &str, success: bool) {
        let function = Function {
            handler: Arc::new(move |_invocation_id, _input| {
                Box::pin(async move {
                    if success {
                        FunctionResult::Success(Some(json!({ "ok": true })))
                    } else {
                        FunctionResult::Failure(ErrorBody {
                            code: "QUEUE_FAIL".to_string(),
                            message: "job failed".to_string(),
                            stacktrace: None,
                        })
                    }
                })
            }),
            _function_id: function_id.to_string(),
            _description: Some("test queue handler".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        };
        engine
            .functions
            .register_function(function_id.to_string(), function);
    }

    #[test]
    fn test_subscriber_queue_config_mode_mapping_fifo() {
        let config = SubscriberQueueConfig {
            queue_mode: Some("fifo".to_string()),
            max_retries: Some(3),
            concurrency: Some(5),
            visibility_timeout: None,
            delay_seconds: None,
            backoff_type: None,
            backoff_delay_ms: Some(1000),
        };

        let subscription_config = Some(config).map(|c| SubscriptionConfig {
            concurrency: c.concurrency,
            max_attempts: c.max_retries,
            backoff_ms: c.backoff_delay_ms,
            mode: c.queue_mode.as_ref().map(|m| match m.as_str() {
                "fifo" => QueueMode::Fifo,
                _ => QueueMode::Concurrent,
            }),
        });

        let sub_config = subscription_config.unwrap();
        assert_eq!(sub_config.mode, Some(QueueMode::Fifo));
        assert_eq!(sub_config.concurrency, Some(5));
        assert_eq!(sub_config.max_attempts, Some(3));
        assert_eq!(sub_config.backoff_ms, Some(1000));
    }

    #[test]
    fn test_subscriber_queue_config_mode_mapping_concurrent() {
        let config = SubscriberQueueConfig {
            queue_mode: Some("concurrent".to_string()),
            max_retries: None,
            concurrency: None,
            visibility_timeout: None,
            delay_seconds: None,
            backoff_type: None,
            backoff_delay_ms: None,
        };

        let subscription_config = Some(config).map(|c| SubscriptionConfig {
            concurrency: c.concurrency,
            max_attempts: c.max_retries,
            backoff_ms: c.backoff_delay_ms,
            mode: c.queue_mode.as_ref().map(|m| match m.as_str() {
                "fifo" => QueueMode::Fifo,
                _ => QueueMode::Concurrent,
            }),
        });

        let sub_config = subscription_config.unwrap();
        assert_eq!(sub_config.mode, Some(QueueMode::Concurrent));
    }

    #[test]
    fn test_subscriber_queue_config_mode_mapping_standard() {
        // "standard" and any other value should map to Concurrent
        let config = SubscriberQueueConfig {
            queue_mode: Some("standard".to_string()),
            max_retries: None,
            concurrency: None,
            visibility_timeout: None,
            delay_seconds: None,
            backoff_type: None,
            backoff_delay_ms: None,
        };

        let subscription_config = Some(config).map(|c| SubscriptionConfig {
            concurrency: c.concurrency,
            max_attempts: c.max_retries,
            backoff_ms: c.backoff_delay_ms,
            mode: c.queue_mode.as_ref().map(|m| match m.as_str() {
                "fifo" => QueueMode::Fifo,
                _ => QueueMode::Concurrent,
            }),
        });

        let sub_config = subscription_config.unwrap();
        assert_eq!(sub_config.mode, Some(QueueMode::Concurrent));
    }

    #[test]
    fn test_subscriber_queue_config_mode_mapping_none() {
        let config = SubscriberQueueConfig {
            queue_mode: None,
            max_retries: Some(5),
            concurrency: Some(10),
            visibility_timeout: None,
            delay_seconds: None,
            backoff_type: None,
            backoff_delay_ms: None,
        };

        let subscription_config = Some(config).map(|c| SubscriptionConfig {
            concurrency: c.concurrency,
            max_attempts: c.max_retries,
            backoff_ms: c.backoff_delay_ms,
            mode: c.queue_mode.as_ref().map(|m| match m.as_str() {
                "fifo" => QueueMode::Fifo,
                _ => QueueMode::Concurrent,
            }),
        });

        let sub_config = subscription_config.unwrap();
        // When queue_mode is None, mode should also be None (inherits global default)
        assert_eq!(sub_config.mode, None);
        assert_eq!(sub_config.concurrency, Some(10));
        assert_eq!(sub_config.max_attempts, Some(5));
    }

    #[tokio::test]
    async fn function_handler_maps_engine_results_to_queue_worker_results() {
        let engine = Arc::new(Engine::new());
        register_test_function(&engine, "queue.success", true);
        register_test_function(&engine, "queue.failure", false);

        let job = Job::new(
            "jobs",
            json!({ "hello": "world" }),
            3,
            100,
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_string()),
            Some("queue=jobs".to_string()),
        );

        let success = FunctionHandler {
            engine: Arc::clone(&engine),
            function_id: "queue.success".to_string(),
            condition_function_id: None,
        };
        success
            .handle(&job)
            .await
            .expect("success handler should succeed");

        let failure = FunctionHandler {
            engine,
            function_id: "queue.failure".to_string(),
            condition_function_id: None,
        };
        let err = failure
            .handle(&job)
            .await
            .expect_err("failure handler should bubble up error");
        assert!(err.contains("QUEUE_FAIL"));
        assert!(err.contains("job failed"));
    }

    #[tokio::test]
    async fn builtin_queue_adapter_clone_subscribe_unsubscribe_and_make_adapter_work() {
        let engine = Arc::new(Engine::new());
        let adapter = make_adapter(Arc::clone(&engine));
        let cloned = adapter.clone();
        assert!(Arc::ptr_eq(&adapter.queue, &cloned.queue));
        assert!(Arc::ptr_eq(&adapter.engine, &cloned.engine));

        let fifo_config = SubscriberQueueConfig {
            queue_mode: Some("fifo".to_string()),
            max_retries: Some(3),
            concurrency: Some(2),
            visibility_timeout: None,
            delay_seconds: None,
            backoff_type: None,
            backoff_delay_ms: Some(25),
        };
        adapter
            .subscribe("jobs", "sub-fifo", "queue.success", None, Some(fifo_config))
            .await;

        let standard_config = SubscriberQueueConfig {
            queue_mode: Some("standard".to_string()),
            max_retries: Some(2),
            concurrency: Some(1),
            visibility_timeout: None,
            delay_seconds: None,
            backoff_type: None,
            backoff_delay_ms: Some(10),
        };
        adapter
            .subscribe(
                "jobs",
                "sub-standard",
                "queue.success",
                None,
                Some(standard_config),
            )
            .await;

        {
            let subs = adapter.subscriptions.read().await;
            assert!(subs.contains_key("jobs:sub-fifo"));
            assert!(subs.contains_key("jobs:sub-standard"));
        }

        adapter.unsubscribe("jobs", "sub-fifo").await;
        adapter.unsubscribe("jobs", "sub-standard").await;
        adapter.unsubscribe("jobs", "missing").await;
        assert!(adapter.subscriptions.read().await.is_empty());

        assert_eq!(adapter.redrive_dlq("jobs").await.unwrap(), 0);
        assert_eq!(adapter.dlq_count("jobs").await.unwrap(), 0);

        let adapter = super::make_adapter(Arc::clone(&engine), Some(json!({ "max_attempts": 5 })))
            .await
            .expect("trait object adapter should build");
        adapter
            .enqueue("jobs", json!({ "task": "queued" }), None, None)
            .await;
        assert_eq!(adapter.dlq_count("jobs").await.unwrap(), 0);
    }

    // =========================================================================
    // Function queue transport integration tests
    // =========================================================================

    #[tokio::test]
    async fn publish_and_consume_delivers_message() {
        let engine = Arc::new(Engine::new());
        let adapter = make_adapter(Arc::clone(&engine));
        let config = FunctionQueueConfig::default();

        adapter
            .setup_function_queue("test-q", &config)
            .await
            .expect("setup should succeed");

        let mut rx = adapter
            .consume_function_queue("test-q", 1)
            .await
            .expect("consume should return receiver");

        adapter
            .publish_to_function_queue(
                "test-q",
                "fn::handler",
                json!({"key": "value"}),
                "test-msg-id",
                3,
                1000,
                None,
                None,
            )
            .await;

        let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("channel should not be closed");

        assert_eq!(msg.function_id, "fn::handler");
        assert_eq!(msg.data, json!({"key": "value"}));
        assert_eq!(msg.attempt, 0);
        // delivery_id is assigned, just verify it exists (u64)
        let _ = msg.delivery_id;
    }

    #[tokio::test]
    async fn ack_removes_delivery_tracking() {
        let engine = Arc::new(Engine::new());
        let adapter = make_adapter(Arc::clone(&engine));
        let config = FunctionQueueConfig::default();

        adapter
            .setup_function_queue("test-q", &config)
            .await
            .expect("setup should succeed");

        let mut rx = adapter
            .consume_function_queue("test-q", 1)
            .await
            .expect("consume should return receiver");

        adapter
            .publish_to_function_queue(
                "test-q",
                "fn::handler",
                json!({"ack": true}),
                "test-msg-id",
                3,
                1000,
                None,
                None,
            )
            .await;

        let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("channel should not be closed");

        let result = adapter.ack_function_queue("test-q", msg.delivery_id).await;
        assert!(result.is_ok(), "ack should succeed");

        assert!(
            adapter.delivery_map.read().await.is_empty(),
            "delivery_map should be empty after ack"
        );
    }

    #[tokio::test]
    async fn nack_removes_delivery_tracking() {
        let engine = Arc::new(Engine::new());
        let adapter = make_adapter(Arc::clone(&engine));
        let config = FunctionQueueConfig::default();

        adapter
            .setup_function_queue("test-q", &config)
            .await
            .expect("setup should succeed");

        let mut rx = adapter
            .consume_function_queue("test-q", 1)
            .await
            .expect("consume should return receiver");

        adapter
            .publish_to_function_queue(
                "test-q",
                "fn::handler",
                json!({"nack": true}),
                "test-msg-id",
                3,
                1000,
                None,
                None,
            )
            .await;

        let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("channel should not be closed");

        let result = adapter
            .nack_function_queue("test-q", msg.delivery_id, 0, 3)
            .await;
        assert!(result.is_ok(), "nack should succeed");

        assert!(
            adapter.delivery_map.read().await.is_empty(),
            "delivery_map should be empty after nack"
        );
    }

    #[tokio::test]
    async fn consume_multiple_messages_in_order() {
        let engine = Arc::new(Engine::new());
        let adapter = make_adapter(Arc::clone(&engine));
        let config = FunctionQueueConfig::default();

        adapter
            .setup_function_queue("test-q", &config)
            .await
            .expect("setup should succeed");

        let mut rx = adapter
            .consume_function_queue("test-q", 10)
            .await
            .expect("consume should return receiver");

        for i in 0..3 {
            adapter
                .publish_to_function_queue(
                    "test-q",
                    "fn::handler",
                    json!({"order": i}),
                    "test-msg-id",
                    3,
                    1000,
                    None,
                    None,
                )
                .await;
        }

        let mut delivery_ids = Vec::new();
        for i in 0..3 {
            let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap_or_else(|_| panic!("should receive message {} within timeout", i))
                .unwrap_or_else(|| panic!("channel should not be closed for message {}", i));

            assert_eq!(
                msg.data,
                json!({"order": i}),
                "messages should arrive in order"
            );
            delivery_ids.push(msg.delivery_id);
        }

        // All delivery IDs must be unique
        let unique: HashSet<u64> = delivery_ids.iter().copied().collect();
        assert_eq!(
            unique.len(),
            3,
            "each message should have a unique delivery_id"
        );
    }

    #[tokio::test]
    async fn delivery_ids_are_unique() {
        let engine = Arc::new(Engine::new());
        let adapter = make_adapter(Arc::clone(&engine));
        let config = FunctionQueueConfig::default();

        adapter
            .setup_function_queue("test-q", &config)
            .await
            .expect("setup should succeed");

        let mut rx = adapter
            .consume_function_queue("test-q", 10)
            .await
            .expect("consume should return receiver");

        for i in 0..5 {
            adapter
                .publish_to_function_queue(
                    "test-q",
                    "fn::handler",
                    json!({"idx": i}),
                    "test-msg-id",
                    3,
                    1000,
                    None,
                    None,
                )
                .await;
        }

        let mut ids = HashSet::new();
        for i in 0..5 {
            let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap_or_else(|_| panic!("should receive message {} within timeout", i))
                .unwrap_or_else(|| panic!("channel should not be closed for message {}", i));
            ids.insert(msg.delivery_id);
        }

        assert_eq!(ids.len(), 5, "all 5 delivery_ids should be unique");
    }

    #[tokio::test]
    async fn publish_with_trace_context_propagates() {
        let engine = Arc::new(Engine::new());
        let adapter = make_adapter(Arc::clone(&engine));
        let config = FunctionQueueConfig::default();

        adapter
            .setup_function_queue("test-q", &config)
            .await
            .expect("setup should succeed");

        let mut rx = adapter
            .consume_function_queue("test-q", 1)
            .await
            .expect("consume should return receiver");

        adapter
            .publish_to_function_queue(
                "test-q",
                "fn::handler",
                json!({"traced": true}),
                "test-msg-id",
                3,
                1000,
                Some("00-abc-def-01".to_string()),
                Some("key=value".to_string()),
            )
            .await;

        let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("should receive within timeout")
            .expect("channel should not be closed");

        assert_eq!(
            msg.traceparent.as_deref(),
            Some("00-abc-def-01"),
            "traceparent should propagate"
        );
        assert_eq!(
            msg.baggage.as_deref(),
            Some("key=value"),
            "baggage should propagate"
        );
    }
}
