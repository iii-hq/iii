use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::{
    builtins::{
        kv::BuiltinKvStore,
        pubsub::BuiltInPubSubAdapter,
        queue::{BuiltinQueue, JobHandler, QueueConfig, SubscriptionHandle},
    },
    engine::{Engine, EngineTrait},
    modules::event::{
        EventAdapter,
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
        kv_store: Arc<BuiltinKvStore>,
        pubsub: Arc<BuiltInPubSubAdapter>,
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

        let kv_store = Arc::new(BuiltinKvStore::new(config.clone()));
        let pubsub = Arc::new(BuiltInPubSubAdapter::new(config.clone()));

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
    ) {
        let handler = Arc::new(FunctionHandler {
            engine: Arc::clone(&self.engine),
            function_path: function_path.to_string(),
        });

        let handle = self.queue.subscribe(topic, handler, None).await;

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
}

crate::register_adapter!(
    <EventAdapterRegistration>
    "modules::event::BuiltinQueueAdapter",
    make_adapter
);
