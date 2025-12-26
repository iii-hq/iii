use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::StreamExt;
use redis::{AsyncCommands, Client, aio::ConnectionManager};
use serde_json::Value;
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::timeout,
};

use crate::{
    engine::{Engine, EngineTrait},
    modules::event::{
        EventAdapter,
        registry::{EventAdapterFuture, EventAdapterRegistration},
    },
};

/// Default timeout for Redis connection attempts
const REDIS_CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

pub struct RedisAdapter {
    publisher: Arc<Mutex<ConnectionManager>>,
    subscriber: Arc<Client>,
    subscriptions: Arc<RwLock<HashMap<String, SubscriptionInfo>>>,
    engine: Arc<Engine>,
}

struct SubscriptionInfo {
    id: String,
    task_handle: JoinHandle<()>,
}

impl RedisAdapter {
    pub async fn new(redis_url: String, engine: Arc<Engine>) -> anyhow::Result<Self> {
        let client = Client::open(redis_url.as_str())?;

        let manager = timeout(REDIS_CONNECTION_TIMEOUT, client.get_connection_manager())
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Redis connection timed out after {:?}. Please ensure Redis is running at: {}",
                    REDIS_CONNECTION_TIMEOUT,
                    redis_url
                )
            })?
            .map_err(|e| anyhow::anyhow!("Failed to connect to Redis at {}: {}", redis_url, e))?;

        let publisher = Arc::new(Mutex::new(manager));
        let subscriber = Arc::new(client);

        Ok(Self {
            publisher,
            subscriber,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            engine,
        })
    }
}

fn make_adapter(engine: Arc<Engine>, config: Option<Value>) -> EventAdapterFuture {
    Box::pin(async move {
        let redis_url = config
            .as_ref()
            .and_then(|v| v.get("redis_url"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "redis://localhost:6379".to_string());
        Ok(Arc::new(RedisAdapter::new(redis_url, engine).await?) as Arc<dyn EventAdapter>)
    })
}

crate::register_adapter!(<EventAdapterRegistration> "modules::event::RedisAdapter", make_adapter);

#[async_trait]
impl EventAdapter for RedisAdapter {
    async fn emit(&self, topic: &str, event_data: Value) {
        let topic = topic.to_string();
        let event_data = event_data.clone();
        let publisher = Arc::clone(&self.publisher);

        tracing::debug!(topic = %topic, event_data = %event_data, "Emitting event to Redis");

        let event_json = match serde_json::to_string(&event_data) {
            Ok(json) => json,
            Err(e) => {
                tracing::error!(error = %e, topic = %topic, "Failed to serialize event data");
                return;
            }
        };

        let mut conn = publisher.lock().await;

        if let Err(e) = conn.publish::<_, _, ()>(&topic, &event_json).await {
            tracing::error!(error = %e, topic = %topic, "Failed to publish event to Redis");
            return;
        } else {
            tracing::debug!(topic = %topic, "Event published to Redis");
        }
    }

    async fn subscribe(&self, topic: &str, id: &str, function_path: &str) {
        let topic = topic.to_string();
        let id = id.to_string();
        let function_path = function_path.to_string();
        let subscriber = Arc::clone(&self.subscriber);
        let engine = Arc::clone(&self.engine);
        let subscriptions = Arc::clone(&self.subscriptions);

        // Check if already subscribed
        let already_subscribed = {
            let subs = subscriptions.read().await;
            subs.contains_key(&topic)
        };

        if already_subscribed {
            tracing::warn!(topic = %topic, id = %id, "Already subscribed to topic");
            return;
        }

        let topic_for_task = topic.clone();
        let id_for_task = id.clone();
        let function_path_for_task = function_path.clone();

        tracing::debug!(topic = %topic_for_task, id = %id_for_task, function_path = %function_path_for_task, "Subscribing to Redis channel");

        let task_handle = tokio::spawn(async move {
            // let mut conn = subscriber.get_connection();
            let mut pubsub = match subscriber.get_async_pubsub().await {
                Ok(pubsub) => pubsub,
                Err(e) => {
                    tracing::error!(error = %e, topic = %topic_for_task, "Failed to get async pubsub connection");
                    return;
                }
            };

            if let Err(e) = pubsub.subscribe(&topic_for_task).await {
                tracing::error!(error = %e, topic = %topic_for_task, "Failed to subscribe to Redis channel");
                return;
            }

            tracing::debug!(topic = %topic_for_task, id = %id_for_task, function_path = %function_path_for_task, "Subscribed to Redis channel");

            let mut msg = pubsub.into_on_message();

            while let Some(msg) = msg.next().await {
                let payload: String = match msg.get_payload() {
                    Ok(payload) => payload,
                    Err(e) => {
                        tracing::error!(error = %e, topic = %topic_for_task, "Failed to get message payload");
                        continue;
                    }
                };

                tracing::debug!(payload = %payload, "Received message from Redis");

                let event_data: Value = match serde_json::from_str(&payload) {
                    Ok(data) => data,
                    Err(e) => {
                        tracing::error!(error = %e, topic = %topic_for_task, "Failed to parse message as JSON");
                        continue;
                    }
                };

                tracing::debug!(topic = %topic_for_task, function_path = %function_path, "Received event from Redis, invoking function");

                let engine = Arc::clone(&engine);
                let function_path = function_path_for_task.clone();

                // We may want to limit concurrency at some point
                tokio::spawn(async move {
                    let _ = engine.invoke_function(&function_path, event_data).await;
                });
            }

            tracing::debug!(topic = %topic_for_task, id = %id_for_task, "Subscription task ended");
        });

        tracing::debug!("Subscription task spawned");

        // Store the subscription
        let mut subs = subscriptions.write().await;
        subs.insert(topic, SubscriptionInfo { id, task_handle });
    }

    async fn unsubscribe(&self, topic: &str, id: &str) {
        tracing::debug!(topic = %topic, id = %id, "Unsubscribing from Redis channel");

        let topic = topic.to_string();
        let subscriptions = Arc::clone(&self.subscriptions);
        let id = id.to_string();

        let mut subs = subscriptions.write().await;

        if let Some(sub_info) = subs.remove(&topic) {
            if sub_info.id == id {
                tracing::debug!(topic = %topic, id = %id, "Unsubscribing from Redis channel");
                sub_info.task_handle.abort();
            } else {
                tracing::warn!(topic = %topic, id = %id, "Subscription ID mismatch, not unsubscribing");
                subs.insert(topic, sub_info);
            }
        } else {
            tracing::warn!(topic = %topic, id = %id, "No active subscription found for topic");
        }
    }
}
