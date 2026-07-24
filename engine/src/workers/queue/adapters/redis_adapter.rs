// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
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
    condition::check_condition,
    engine::{Engine, EngineTrait},
    telemetry::SpanExt,
    workers::{
        queue::{
            QueueAdapter, SubscriberQueueConfig,
            registry::{QueueAdapterFuture, QueueAdapterRegistration},
        },
        redis::DEFAULT_REDIS_CONNECTION_TIMEOUT,
    },
};

pub struct RedisAdapter {
    publisher: Arc<Mutex<ConnectionManager>>,
    subscriber: Arc<Client>,
    subscriptions: Arc<RwLock<HashMap<String, SubscriptionInfo>>>,
    engine: Arc<Engine>,
}

struct SubscriptionInfo {
    topic: String,
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

crate::register_adapter!(<QueueAdapterRegistration> name: "redis", make_adapter);

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
        // Redis pub/sub is fanout: subscriptions are keyed by (topic, trigger
        // id) and each dispatches in its own namespace, resolved live by id at
        // delivery. There is no durable per-namespace queue to name here.
        _namespace: &str,
        condition_function_id: Option<String>,
        _queue_config: Option<SubscriberQueueConfig>,
    ) {
        let topic = topic.to_string();
        let id = id.to_string();
        let function_id = function_id.to_string();
        let subscriber = Arc::clone(&self.subscriber);
        let engine = Arc::clone(&self.engine);
        let subscriptions = Arc::clone(&self.subscriptions);

        // Key by (topic, trigger id) rather than topic alone: Redis pub/sub
        // broadcasts, so several subscribers can listen on one channel and each
        // dispatches in its own namespace (resolved live from the trigger id).
        // Keying by topic alone would reject a second namespace's subscriber and
        // silently starve it of events.
        let sub_key = format!("{}:{}", topic, id);
        let already_subscribed = {
            let subs = subscriptions.read().await;
            subs.contains_key(&sub_key)
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

                // Extract trace context from envelope, with backward compatibility.
                // Only treat as envelope when "__trace" is an object AND "data" is present,
                // so payloads that merely contain a "__trace" key are not silently clobbered.
                let (data, traceparent, baggage) =
                    if parsed.get("__trace").and_then(|v| v.as_object()).is_some()
                        && parsed.get("data").is_some()
                    {
                        let trace = parsed.get("__trace").unwrap();
                        let data = parsed.get("data").cloned().unwrap();
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
                        // Legacy format or non-envelope payload: use entire payload as data
                        (parsed, None, None)
                    };

                tracing::debug!(topic = %topic_for_task, function_id = %function_id_for_task, traceparent = ?traceparent, "Received message from Redis queue, invoking function");

                let engine = Arc::clone(&engine);
                let function_id = function_id_for_task.clone();
                let topic_for_span = topic_for_task.clone();
                // Resolve the subscribing trigger's namespace LIVE by id, used
                // for both the condition and the target.
                let namespace = engine.trigger_registry.namespace_of(&id_for_task);

                if let Some(ref condition_id) = condition_function_id_for_task {
                    tracing::debug!(
                        condition_function_id = %condition_id,
                        "Checking trigger conditions"
                    );
                    match check_condition(engine.as_ref(), &namespace, condition_id, data.clone())
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            tracing::debug!(
                                function_id = %function_id,
                                "Condition check failed, skipping handler"
                            );
                            continue;
                        }
                        Err(err) => {
                            tracing::error!(
                                condition_function_id = %condition_id,
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
                .with_parent_headers(
                    traceparent.as_deref(),
                    None,
                    baggage.as_deref(),
                );

                tokio::spawn(
                    async move {
                        match engine
                            .call_with_metadata_ns(&namespace, &function_id, data, None)
                            .await
                        {
                            Ok(_) => {
                                tracing::Span::current().record("otel.status_code", "OK");
                            }
                            Err(e) => {
                                tracing::error!(
                                    error = ?e,
                                    function_id = %function_id,
                                    topic = %topic_for_span,
                                    "Failed to invoke function for queue job"
                                );
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
            sub_key,
            SubscriptionInfo {
                topic,
                condition_function_id,
                task_handle,
            },
        );
    }

    async fn unsubscribe(&self, topic: &str, id: &str) {
        tracing::debug!(topic = %topic, id = %id, "Unsubscribing from Redis channel");

        let subscriptions = Arc::clone(&self.subscriptions);
        let sub_key = format!("{}:{}", topic, id);

        let mut subs = subscriptions.write().await;

        if let Some(sub_info) = subs.remove(&sub_key) {
            tracing::debug!(topic = %topic, id = %id, "Unsubscribing from Redis channel");
            sub_info.task_handle.abort();
        } else {
            tracing::warn!(topic = %topic, id = %id, "No active subscription found for topic");
        }
    }

    async fn redrive_dlq(&self, _topic: &str) -> anyhow::Result<u64> {
        Err(anyhow::anyhow!(
            "RedisAdapter does not support DLQ operations (pub/sub only)"
        ))
    }

    async fn redrive_dlq_message(&self, _topic: &str, _message_id: &str) -> anyhow::Result<bool> {
        Err(anyhow::anyhow!(
            "RedisAdapter does not support DLQ operations (pub/sub only)"
        ))
    }

    async fn discard_dlq_message(&self, _topic: &str, _message_id: &str) -> anyhow::Result<bool> {
        Err(anyhow::anyhow!(
            "RedisAdapter does not support DLQ operations (pub/sub only)"
        ))
    }

    async fn dlq_count(&self, _topic: &str) -> anyhow::Result<u64> {
        Err(anyhow::anyhow!(
            "RedisAdapter does not support DLQ operations (pub/sub only)"
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn publish_to_function_queue(
        &self,
        queue_name: &str,
        function_id: &str,
        data: Value,
        message_id: &str,
        _max_retries: u32,
        _backoff_ms: u64,
        traceparent: Option<String>,
        baggage: Option<String>,
        namespace: Option<String>,
        // RabbitMQ-only feature; the redis pub/sub adapter ignores it.
        _priority: Option<u8>,
    ) {
        let channel = format!("__queue::{}", queue_name);
        let publisher = Arc::clone(&self.publisher);

        // `namespace` rides the envelope so a consumer (once implemented) can
        // resolve the target in the enqueuer's namespace. Omitted when absent to
        // keep the envelope stable for pre-namespace consumers.
        let envelope = serde_json::json!({
            "__trace": {
                "traceparent": traceparent,
                "baggage": baggage,
            },
            "function_id": function_id,
            "message_id": message_id,
            "namespace": namespace,
            "data": data,
        });

        let json = match serde_json::to_string(&envelope) {
            Ok(json) => json,
            Err(e) => {
                tracing::error!(error = %e, queue = %queue_name, "Failed to serialize function queue data");
                return;
            }
        };

        tracing::debug!(queue = %queue_name, function_id = %function_id, "Publishing to Redis function queue channel");

        let mut conn = publisher.lock().await;

        if let Err(e) = conn.publish::<_, _, ()>(&channel, &json).await {
            tracing::error!(error = %e, queue = %queue_name, "Failed to publish to Redis function queue channel");
        }
    }

    async fn consume_function_queue(
        &self,
        _queue_name: &str,
        _prefetch: u32,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<crate::workers::queue::QueueMessage>> {
        anyhow::bail!("Redis function queue consumer not yet implemented")
    }

    async fn list_topics(&self) -> anyhow::Result<Vec<crate::workers::queue::TopicInfo>> {
        // Subscriptions are keyed by "{topic}:{id}"; count by the stored topic
        // so multiple subscribers (across namespaces) roll up per topic.
        let subs = self.subscriptions.read().await;
        let mut topic_counts: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        for info in subs.values() {
            *topic_counts.entry(info.topic.clone()).or_insert(0) += 1;
        }
        Ok(topic_counts
            .into_iter()
            .map(|(name, count)| crate::workers::queue::TopicInfo {
                name,
                broker_type: "redis".to_string(),
                subscriber_count: count,
            })
            .collect())
    }
}
