use std::collections::HashMap;
use std::sync::Arc;

use redis::{Client, Commands};
use serde_json::Value;
use tokio::sync::{RwLock, oneshot};
use tokio::task::JoinHandle;

use crate::engine::{Engine, EngineTrait};
use crate::modules::event::EventAdapter;

pub struct RedisAdapter {
    publisher: Arc<Client>,
    subscriber: Arc<Client>,
    subscriptions: Arc<RwLock<HashMap<String, SubscriptionInfo>>>,
    engine: Arc<Engine>,
}

struct SubscriptionInfo {
    id: String,
    task_handle: JoinHandle<()>,
}

impl RedisAdapter {
    pub fn new(redis_url: String, engine: Arc<Engine>) -> anyhow::Result<Self> {
        let publisher = Arc::new(Client::open(redis_url.as_str())?);
        let subscriber = Arc::new(Client::open(redis_url.as_str())?);

        Ok(Self {
            publisher,
            subscriber,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            engine,
        })
    }
}

impl EventAdapter for RedisAdapter {
    fn emit(&self, topic: &str, event_data: Value) {
        let topic = topic.to_string();
        let event_data = event_data.clone();
        let publisher = Arc::clone(&self.publisher);

        tokio::spawn(async move {
            tracing::info!(topic = %topic, event_data = %event_data, "Emitting event to Redis");

            let event_json = match serde_json::to_string(&event_data) {
                Ok(json) => json,
                Err(e) => {
                    tracing::error!(error = %e, topic = %topic, "Failed to serialize event data");
                    return;
                }
            };

            let mut conn = match publisher.get_connection() {
                Ok(conn) => conn,
                Err(e) => {
                    tracing::error!(error = %e, topic = %topic, "Failed to get Redis connection for publishing");
                    return;
                }
            };

            if let Err(e) = conn.publish::<_, _, ()>(&topic, &event_json) {
                tracing::error!(error = %e, topic = %topic, "Failed to publish event to Redis");
            } else {
                tracing::debug!(topic = %topic, "Event published to Redis");
            }
        });
    }

    fn subscribe(&self, topic: &str, id: &str, function_path: &str) {
        let topic = topic.to_string();
        let id = id.to_string();
        let function_path = function_path.to_string();
        let subscriber = Arc::clone(&self.subscriber);
        let engine = Arc::clone(&self.engine);
        let subscriptions = Arc::clone(&self.subscriptions);

        // Use a oneshot channel to get the result of the subscription check
        let (tx, rx) = oneshot::channel();
        let subscriptions_check = Arc::clone(&subscriptions);
        let topic_check = topic.clone();

        tokio::spawn(async move {
            let subs = subscriptions_check.read().await;
            let already_subscribed = subs.contains_key(&topic_check);
            let _ = tx.send(already_subscribed);
        });

        // Spawn a task to handle the async work
        tokio::spawn(async move {
            // Wait for the check result
            let already_subscribed = match rx.await {
                Ok(result) => result,
                Err(_) => {
                    tracing::error!(topic = %topic, "Failed to check subscription status");
                    return;
                }
            };

            if already_subscribed {
                tracing::warn!(topic = %topic, id = %id, "Already subscribed to topic");
                return;
            }

            let topic_for_task = topic.clone();
            let id_for_task = id.clone();
            let function_path_for_task = function_path.clone();

            let task_handle = tokio::spawn(async move {
                let mut conn = match subscriber.get_connection() {
                    Ok(conn) => conn,
                    Err(e) => {
                        tracing::error!(error = %e, topic = %topic_for_task, "Failed to get Redis connection for subscription");
                        return;
                    }
                };

                let mut pubsub = conn.as_pubsub();

                if let Err(e) = pubsub.subscribe(&topic_for_task) {
                    tracing::error!(error = %e, topic = %topic_for_task, "Failed to subscribe to Redis channel");
                    return;
                }

                tracing::info!(topic = %topic_for_task, id = %id_for_task, function_path = %function_path_for_task, "Subscribed to Redis channel");

                loop {
                    let msg = match pubsub.get_message() {
                        Ok(msg) => msg,
                        Err(e) => {
                            tracing::error!(error = %e, topic = %topic_for_task, "Error getting message from Redis");
                            break;
                        }
                    };

                    let payload: String = match msg.get_payload() {
                        Ok(payload) => payload,
                        Err(e) => {
                            tracing::error!(error = %e, topic = %topic_for_task, "Failed to get message payload");
                            continue;
                        }
                    };

                    tracing::info!(payload = %payload, "Received message from Redis");

                    let event_data: Value = match serde_json::from_str(&payload) {
                        Ok(data) => data,
                        Err(e) => {
                            tracing::error!(error = %e, topic = %topic_for_task, payload = %payload, "Failed to parse message as JSON");
                            continue;
                        }
                    };

                    tracing::info!(topic = %topic_for_task, function_path = %function_path_for_task, "Received event from Redis, invoking function");
                    engine.invoke_function(&function_path_for_task, event_data);
                }

                tracing::info!(topic = %topic_for_task, id = %id_for_task, "Subscription task ended");
            });

            // Store the subscription
            let mut subs = subscriptions.write().await;
            subs.insert(topic, SubscriptionInfo { id, task_handle });
        });
    }

    fn unsubscribe(&self, topic: &str, id: &str) {
        let topic = topic.to_string();
        let subscriptions = Arc::clone(&self.subscriptions);
        let id = id.to_string();

        tokio::spawn(async move {
            let mut subs = subscriptions.write().await;

            if let Some(sub_info) = subs.remove(&topic) {
                if sub_info.id == id {
                    tracing::info!(topic = %topic, id = %id, "Unsubscribing from Redis channel");
                    sub_info.task_handle.abort();
                } else {
                    tracing::warn!(topic = %topic, id = %id, "Subscription ID mismatch, not unsubscribing");
                    subs.insert(topic, sub_info);
                }
            } else {
                tracing::warn!(topic = %topic, id = %id, "No active subscription found for topic");
            }
        });
    }
}
