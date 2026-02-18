// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use futures::StreamExt;
use redis::{AsyncCommands, Client, aio::ConnectionManager};
use serde_json::Value;
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::timeout,
};
use tracing::Instrument;

use crate::{
    engine::{Engine, EngineTrait},
    modules::{
        queue::{
            QueueAdapter, SubscriberQueueConfig,
            registry::{QueueAdapterFuture, QueueAdapterRegistration},
        },
        redis::DEFAULT_REDIS_CONNECTION_TIMEOUT,
    },
    telemetry::SpanExt,
};

pub struct RedisAdapter {
    publisher: Arc<Mutex<ConnectionManager>>,
    subscriber: Arc<Client>,
    subscriptions: Arc<RwLock<HashMap<String, SubscriptionInfo>>>,
    engine: Arc<Engine>,
}

struct SubscriptionInfo {
    id: String,
    #[allow(dead_code)]
    condition_function_id: Option<String>,
    task_handle: JoinHandle<()>,
}

impl RedisAdapter {
    pub async fn new(redis_url: String, engine: Arc<Engine>) -> anyhow::Result<Self> {
        let client = Client::open(redis_url.as_str())?;

        let manager = timeout(
            DEFAULT_REDIS_CONNECTION_TIMEOUT,
            client.get_connection_manager(),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Redis connection timed out after {:?}. Please ensure Redis is running at: {}",
                DEFAULT_REDIS_CONNECTION_TIMEOUT,
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

fn make_adapter(engine: Arc<Engine>, config: Option<Value>) -> QueueAdapterFuture {
    Box::pin(async move {
        let redis_url = config
            .as_ref()
            .and_then(|v| v.get("redis_url"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "redis://localhost:6379".to_string());
        Ok(Arc::new(RedisAdapter::new(redis_url, engine).await?) as Arc<dyn QueueAdapter>)
    })
}

crate::register_adapter!(<QueueAdapterRegistration> "modules::queue::RedisAdapter", make_adapter);

#[async_trait]
impl QueueAdapter for RedisAdapter {
    async fn enqueue(
        &self,
        topic: &str,
        data: Value,
        traceparent: Option<String>,
        baggage: Option<String>,
    ) {
        let topic = topic.to_string();
        let publisher = Arc::clone(&self.publisher);

        tracing::debug!(topic = %topic, data = %data, "Publishing to Redis queue");

        // Wrap data in an envelope that includes trace context
        let envelope = serde_json::json!({
            "__trace": {
                "traceparent": traceparent,
                "baggage": baggage,
            },
            "data": data,
        });

        let json = match serde_json::to_string(&envelope) {
            Ok(json) => json,
            Err(e) => {
                tracing::error!(error = %e, topic = %topic, "Failed to serialize queue data");
                return;
            }
        };

        let mut conn = publisher.lock().await;

        if let Err(e) = conn.publish::<_, _, ()>(&topic, &json).await {
            tracing::error!(error = %e, topic = %topic, "Failed to publish to Redis queue");
            return;
        } else {
            tracing::debug!(topic = %topic, "Published to Redis queue");
        }
    }

    async fn subscribe(
        &self,
        topic: &str,
        id: &str,
        function_id: &str,
        condition_function_id: Option<String>,
        _queue_config: Option<SubscriberQueueConfig>,
    ) {
        let topic = topic.to_string();
        let id = id.to_string();
        let function_id = function_id.to_string();
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
        let function_id_for_task = function_id.clone();
        let condition_function_id_for_task = condition_function_id.clone();

        tracing::debug!(topic = %topic_for_task, id = %id_for_task, function_id = %function_id_for_task, "Subscribing to Redis channel");

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

            tracing::debug!(
                topic = %topic_for_task,
                id = %id_for_task,
                function_id = %function_id_for_task,
                condition_function_id = ?condition_function_id_for_task,
                "Subscribed to Redis channel"
            );

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

                let parsed: Value = match serde_json::from_str(&payload) {
                    Ok(data) => data,
                    Err(e) => {
                        tracing::error!(error = %e, topic = %topic_for_task, "Failed to parse message as JSON");
                        continue;
                    }
                };

                // Extract trace context from envelope, with backward compatibility
                let (data, traceparent, baggage) = if let Some(trace) = parsed.get("__trace") {
                    let data = parsed.get("data").cloned().unwrap_or(Value::Null);
                    let traceparent = trace
                        .get("traceparent")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let baggage = trace
                        .get("baggage")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    (data, traceparent, baggage)
                } else {
                    // Legacy format: entire payload is the data
                    (parsed, None, None)
                };

                tracing::debug!(topic = %topic_for_task, function_id = %function_id_for_task, traceparent = ?traceparent, "Received message from Redis queue, invoking function");

                let engine = Arc::clone(&engine);
                let function_id = function_id_for_task.clone();
                let topic_for_span = topic_for_task.clone();

                if let Some(condition_function_id) = condition_function_id_for_task.as_ref() {
                    tracing::debug!(
                        condition_function_id = %condition_function_id,
                        "Checking trigger conditions"
                    );

                    match engine.call(condition_function_id, data.clone()).await {
                        Ok(Some(result)) => {
                            if let Some(passed) = result.as_bool()
                                && !passed
                            {
                                tracing::debug!(
                                    function_id = %function_id,
                                    "Condition check failed, skipping handler"
                                );
                                continue;
                            }
                        }
                        Ok(None) => {
                            tracing::warn!(
                                condition_function_id = %condition_function_id,
                                "Condition function returned no result"
                            );
                        }
                        Err(err) => {
                            tracing::error!(
                                condition_function_id = %condition_function_id,
                                error = ?err,
                                "Error invoking condition function"
                            );
                            continue;
                        }
                    }
                }

                // Create span with parent from trace context propagated through Redis
                let span = tracing::info_span!(
                    "queue_job",
                    otel.name = %format!("queue {}", topic_for_span),
                    queue = %topic_for_span,
                    otel.status_code = tracing::field::Empty,
                )
                .with_parent_headers(traceparent.as_deref(), baggage.as_deref());

                tokio::spawn(
                    async move {
                        match engine.call(&function_id, data).await {
                            Ok(_) => {
                                tracing::Span::current().record("otel.status_code", "OK");
                            }
                            Err(_) => {
                                tracing::Span::current().record("otel.status_code", "ERROR");
                            }
                        }
                    }
                    .instrument(span),
                );
            }

            tracing::debug!(topic = %topic_for_task, id = %id_for_task, "Subscription task ended");
        });

        tracing::debug!("Subscription task spawned");

        // Store the subscription
        let mut subs = subscriptions.write().await;
        subs.insert(
            topic,
            SubscriptionInfo {
                id,
                condition_function_id,
                task_handle,
            },
        );
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

    async fn redrive_dlq(&self, _topic: &str) -> anyhow::Result<u64> {
        Err(anyhow::anyhow!(
            "RedisAdapter does not support DLQ operations (pub/sub only)"
        ))
    }

    async fn dlq_count(&self, _topic: &str) -> anyhow::Result<u64> {
        Err(anyhow::anyhow!(
            "RedisAdapter does not support DLQ operations (pub/sub only)"
        ))
    }
}
