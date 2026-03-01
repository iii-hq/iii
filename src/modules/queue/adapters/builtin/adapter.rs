// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use tracing::Instrument;

use crate::{
    builtins::{
        kv::BuiltinKvStore,
        pubsub_lite::BuiltInPubSubLite,
        queue::{
            BuiltinQueue, JobHandler, QueueConfig, QueueMode, SubscriptionConfig,
            SubscriptionHandle,
        },
        queue_kv::QueueKvStore,
    },
    engine::{Engine, EngineTrait},
    modules::queue::{
        QueueAdapter, SubscriberQueueConfig,
        registry::{QueueAdapterFuture, QueueAdapterRegistration},
    },
    telemetry::SpanExt,
};

pub struct BuiltinQueueAdapter {
    queue: Arc<BuiltinQueue>,
    engine: Arc<Engine>,
    subscriptions: Arc<RwLock<HashMap<String, SubscriptionHandle>>>,
}

struct FunctionHandler {
    engine: Arc<Engine>,
    function_id: String,
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

        async {
            let result = self.engine.call(&self.function_id, job.data.clone()).await;
            match &result {
                Ok(_) => {
                    tracing::Span::current().record("otel.status_code", "OK");
                }
                Err(_) => {
                    tracing::Span::current().record("otel.status_code", "ERROR");
                }
            }
            result
        }
        .instrument(span)
        .await
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
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
        }
    }
}

impl Clone for BuiltinQueueAdapter {
    fn clone(&self) -> Self {
        Self {
            queue: Arc::clone(&self.queue),
            engine: Arc::clone(&self.engine),
            subscriptions: Arc::clone(&self.subscriptions),
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
        _condition_function_id: Option<String>,
        queue_config: Option<SubscriberQueueConfig>,
    ) {
        let handler = Arc::new(FunctionHandler {
            engine: Arc::clone(&self.engine),
            function_id: function_id.to_string(),
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
    use crate::modules::queue::SubscriberQueueConfig;
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
        };
        success
            .handle(&job)
            .await
            .expect("success handler should succeed");

        let failure = FunctionHandler {
            engine,
            function_id: "queue.failure".to_string(),
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
}
