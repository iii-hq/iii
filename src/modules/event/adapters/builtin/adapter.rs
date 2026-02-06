// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

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
    modules::event::{
        EventAdapter, SubscriberQueueConfig,
        registry::{EventAdapterFuture, EventAdapterRegistration},
    },
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
        match self
            .engine
            .invoke_function(&self.function_id, job.data.clone())
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("{:?}", e)),
        }
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

pub fn make_adapter(engine: Arc<Engine>, config: Option<Value>) -> EventAdapterFuture {
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

        Ok(adapter as Arc<dyn EventAdapter>)
    })
}

#[async_trait]
impl EventAdapter for BuiltinQueueAdapter {
    async fn emit(&self, topic: &str, event_data: Value) {
        self.queue.push(topic, event_data).await;
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
    <EventAdapterRegistration>
    "modules::event::BuiltinQueueAdapter",
    make_adapter
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::queue::QueueMode;
    use crate::modules::event::SubscriberQueueConfig;

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
}
