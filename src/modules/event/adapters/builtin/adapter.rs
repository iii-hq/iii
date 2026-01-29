use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::{
    builtins::{
        kv::BuiltinKvStore, pubsub_lite::BuiltInPubSubLite, queue::{BuiltinQueue, JobHandler, QueueConfig, SubscriptionConfig, SubscriptionHandle}, queue_kv::QueueKvStore
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
    function_path: String,
}

#[async_trait]
impl JobHandler for FunctionHandler {
    async fn handle(&self, job: &crate::builtins::queue::Job) -> Result<(), String> {
        match self
            .engine
            .invoke_function(&self.function_path, job.data.clone())
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
        function_path: &str,
        _condition_function_path: Option<String>,
        queue_config: Option<SubscriberQueueConfig>,
    ) {
        let handler = Arc::new(FunctionHandler {
            engine: Arc::clone(&self.engine),
            function_path: function_path.to_string(),
        });

        let subscription_config = queue_config.map(|c| SubscriptionConfig {
            concurrency: c.concurrency,
            max_attempts: c.max_retries,
            backoff_ms: c.backoff_delay_ms,
        });

        let handle = self
            .queue
            .subscribe(topic, handler, subscription_config)
            .await;

        let mut subs = self.subscriptions.write().await;
        subs.insert(format!("{}:{}", topic, id), handle);

        tracing::debug!(topic = %topic, id = %id, function_path = %function_path, "Subscribed to queue via BuiltinQueueAdapter");
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
