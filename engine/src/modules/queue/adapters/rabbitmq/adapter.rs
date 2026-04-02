// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

#![cfg(feature = "rabbitmq")]

use std::{
    collections::HashMap,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use lapin::{Channel, Connection, ConnectionProperties, message::Delivery, options::*};
use serde_json::Value;
use tokio::{sync::RwLock, task::JoinHandle};
use uuid::Uuid;

use crate::{
    engine::Engine,
    modules::queue::{
        FunctionQueueConfig, QueueAdapter, SubscriberQueueConfig,
        registry::{QueueAdapterFuture, QueueAdapterRegistration},
    },
};

use super::{
    publisher::Publisher,
    retry::RetryHandler,
    topology::TopologyManager,
    types::{Job, QueueMode, RabbitMQConfig},
    worker::Worker,
};

pub struct RabbitMQAdapter {
    publisher: Arc<Publisher>,
    retry_handler: Arc<RetryHandler>,
    topology: Arc<TopologyManager>,
    channel: Arc<Channel>,
    subscriptions: Arc<RwLock<HashMap<String, SubscriptionInfo>>>,
    engine: Arc<Engine>,
    config: RabbitMQConfig,
    delivery_map: Arc<RwLock<HashMap<u64, Delivery>>>,
    delivery_counter: Arc<AtomicU64>,
}

struct SubscriptionInfo {
    id: String,
    consumer_tag: String,
    task_handle: JoinHandle<()>,
}

impl RabbitMQAdapter {
    /// Resolve the DLQ queue name for a given topic.
    /// Handles both function queues (__fn_queue:: prefix) and topic-based queues.
    fn resolve_dlq_name(topic: &str) -> (String, bool) {
        use super::naming::{FnQueueNames, RabbitNames};
        if let Some(queue_name) = topic.strip_prefix("__fn_queue::") {
            (FnQueueNames::new(queue_name).dlq(), true)
        } else {
            (RabbitNames::new(topic).dlq(), false)
        }
    }

    /// Scan a DLQ for a specific message by ID, applying `on_found` when the target is located.
    /// Non-target messages are nacked back to the queue.
    ///
    /// CONCURRENCY NOTE: This uses basic_get + nack(requeue) which is not atomic.
    /// Under concurrent DLQ operations, messages may be reordered. The iteration
    /// is bounded by the queue depth snapshot to prevent infinite loops from
    /// requeued messages, but concurrent producers may cause messages to be missed.
    async fn find_dlq_message<F, Fut>(
        &self,
        topic: &str,
        message_id: &str,
        on_found: F,
    ) -> anyhow::Result<bool>
    where
        F: FnOnce(&Delivery, bool, &str) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<()>>,
    {
        let (dlq_name, is_fn_queue) = Self::resolve_dlq_name(topic);

        let queue_info = self
            .channel
            .queue_declare(
                &dlq_name,
                lapin::options::QueueDeclareOptions {
                    passive: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get DLQ info: {}", e))?;

        let count = queue_info.message_count();

        for _ in 0..count {
            let get_result = self
                .channel
                .basic_get(&dlq_name, BasicGetOptions { no_ack: false })
                .await
                .map_err(|e| anyhow::anyhow!("Failed to get message from DLQ: {}", e))?;

            let Some(delivery) = get_result else { break };

            // Determine if this is the target message.
            // Function queue DLQ: stable ID from AMQP message_id or body hash
            // Topic DLQ: ID is in the JSON payload at job.id
            let is_target = if is_fn_queue {
                delivery_stable_id(&delivery.delivery) == message_id
            } else {
                let dlq_payload: Value =
                    serde_json::from_slice(&delivery.delivery.data).unwrap_or(Value::Null);
                dlq_payload
                    .get("job")
                    .and_then(|j| j.get("id"))
                    .and_then(|id| id.as_str())
                    == Some(message_id)
            };

            if is_target {
                let queue_name = topic.strip_prefix("__fn_queue::").unwrap_or(topic);
                on_found(&delivery.delivery, is_fn_queue, queue_name).await?;

                delivery
                    .delivery
                    .ack(BasicAckOptions { multiple: false })
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to ack target message: {}", e))?;

                return Ok(true);
            }

            // Not the target -- put it back
            delivery
                .delivery
                .nack(BasicNackOptions {
                    requeue: true,
                    multiple: false,
                })
                .await
                .map_err(|e| anyhow::anyhow!("Failed to nack non-target message: {}", e))?;
        }

        Ok(false)
    }

    pub async fn new(config: RabbitMQConfig, engine: Arc<Engine>) -> anyhow::Result<Self> {
        let connection = Connection::connect(&config.amqp_url, ConnectionProperties::default())
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to connect to RabbitMQ at {}: {}",
                    config.amqp_url,
                    e
                )
            })?;

        let channel = connection
            .create_channel()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create RabbitMQ channel: {}", e))?;

        let effective_prefetch = match config.queue_mode {
            super::types::QueueMode::Fifo => 1,
            super::types::QueueMode::Standard => config.prefetch_count,
        };

        channel
            .basic_qos(effective_prefetch, BasicQosOptions::default())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to set QoS: {}", e))?;

        let channel = Arc::new(channel);
        let publisher = Arc::new(Publisher::new(Arc::clone(&channel)));
        let topology = Arc::new(TopologyManager::new(Arc::clone(&channel)));
        let retry_handler = Arc::new(RetryHandler::new(Arc::clone(&publisher)));

        Ok(Self {
            publisher,
            retry_handler,
            topology,
            channel,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            engine,
            config,
            delivery_map: Arc::new(RwLock::new(HashMap::new())),
            delivery_counter: Arc::new(AtomicU64::new(0)),
        })
    }
}

/// Extracts a stable message ID from a DLQ delivery.
/// Tries AMQP `message_id` property first, then falls back to a hash of the
/// raw body so that `dlq_peek` and `redrive_dlq_message` always agree on the ID.
fn delivery_stable_id(delivery: &Delivery) -> String {
    // 1. Try AMQP message_id property
    if let Some(mid) = delivery.properties.message_id() {
        let s = mid.as_str();
        if !s.is_empty() {
            return s.to_string();
        }
    }
    // 2. Fallback: deterministic hash of the raw body
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    delivery.data.hash(&mut hasher);
    format!("dlq-{:016x}", hasher.finish())
}

impl Clone for RabbitMQAdapter {
    fn clone(&self) -> Self {
        Self {
            publisher: Arc::clone(&self.publisher),
            retry_handler: Arc::clone(&self.retry_handler),
            topology: Arc::clone(&self.topology),
            channel: Arc::clone(&self.channel),
            subscriptions: Arc::clone(&self.subscriptions),
            engine: Arc::clone(&self.engine),
            config: self.config.clone(),
            delivery_map: Arc::clone(&self.delivery_map),
            delivery_counter: Arc::clone(&self.delivery_counter),
        }
    }
}

pub fn make_adapter(engine: Arc<Engine>, config: Option<Value>) -> QueueAdapterFuture {
    Box::pin(async move {
        let config = RabbitMQConfig::from_value(config.as_ref());
        Ok(Arc::new(RabbitMQAdapter::new(config, engine).await?) as Arc<dyn QueueAdapter>)
    })
}

#[async_trait]
impl QueueAdapter for RabbitMQAdapter {
    async fn enqueue(
        &self,
        topic: &str,
        data: Value,
        traceparent: Option<String>,
        baggage: Option<String>,
    ) {
        let job = Job::new(topic, data, self.config.max_attempts, traceparent, baggage);

        if let Err(e) = self.topology.setup_topic(topic).await {
            tracing::error!(
                error = ?e,
                topic = %topic,
                "Failed to setup RabbitMQ topology"
            );
            return;
        }

        if let Err(e) = self.publisher.publish(topic, &job).await {
            tracing::error!(
                error = ?e,
                topic = %topic,
                "Failed to publish to RabbitMQ"
            );
        } else {
            tracing::debug!(
                topic = %topic,
                job_id = %job.id,
                "Published to RabbitMQ queue"
            );
        }
    }

    async fn subscribe(
        &self,
        topic: &str,
        id: &str,
        function_id: &str,
        condition_function_id: Option<String>,
        queue_config: Option<SubscriberQueueConfig>,
    ) {
        let topic = topic.to_string();
        let id = id.to_string();
        let function_id = function_id.to_string();
        let subscriptions = Arc::clone(&self.subscriptions);

        let already_subscribed = {
            let subs = subscriptions.read().await;
            subs.contains_key(&format!("{}:{}", topic, id))
        };

        if already_subscribed {
            tracing::warn!(topic = %topic, id = %id, "Already subscribed to topic");
            return;
        }

        if let Err(e) = self.topology.setup_topic(&topic).await {
            tracing::error!(
                error = ?e,
                topic = %topic,
                "Failed to setup RabbitMQ topology for subscription"
            );
            return;
        }

        let consumer_tag = format!("consumer-{}", Uuid::new_v4());

        let effective_queue_mode = queue_config
            .as_ref()
            .and_then(|c| c.queue_mode.as_ref())
            .map(|mode| QueueMode::from_str(mode).unwrap_or_default())
            .unwrap_or_else(|| self.config.queue_mode.clone());

        let effective_prefetch_count = queue_config
            .as_ref()
            .and_then(|c| c.concurrency)
            .map(|c| c as u16)
            .unwrap_or(self.config.prefetch_count);

        let worker = Arc::new(Worker::new(
            Arc::clone(&self.channel),
            Arc::clone(&self.retry_handler),
            Arc::clone(&self.engine),
            effective_queue_mode,
            effective_prefetch_count,
        ));

        let topic_clone = topic.clone();
        let function_id_clone = function_id.clone();
        let consumer_tag_clone = consumer_tag.clone();

        let task_handle = tokio::spawn(async move {
            worker
                .run(
                    topic_clone,
                    function_id_clone,
                    condition_function_id,
                    consumer_tag_clone,
                )
                .await;
        });

        let mut subs = subscriptions.write().await;
        subs.insert(
            format!("{}:{}", topic, id),
            SubscriptionInfo {
                id,
                consumer_tag,
                task_handle,
            },
        );

        tracing::debug!(
            topic = %topic,
            function_id = %function_id,
            "Subscribed to RabbitMQ queue"
        );
    }

    async fn unsubscribe(&self, topic: &str, id: &str) {
        let subscriptions = Arc::clone(&self.subscriptions);
        let key = format!("{}:{}", topic, id);

        let mut subs = subscriptions.write().await;

        if let Some(sub_info) = subs.remove(&key) {
            if sub_info.id == id {
                tracing::debug!(
                    topic = %topic,
                    id = %id,
                    "Unsubscribing from RabbitMQ queue"
                );

                if let Err(e) = self
                    .channel
                    .basic_cancel(&sub_info.consumer_tag, BasicCancelOptions::default())
                    .await
                {
                    tracing::error!(
                        error = ?e,
                        topic = %topic,
                        consumer_tag = %sub_info.consumer_tag,
                        "Failed to cancel consumer"
                    );
                }

                sub_info.task_handle.abort();
            } else {
                tracing::warn!(
                    topic = %topic,
                    id = %id,
                    "Subscription ID mismatch, not unsubscribing"
                );
                subs.insert(key, sub_info);
            }
        } else {
            tracing::warn!(
                topic = %topic,
                id = %id,
                "No active subscription found for topic"
            );
        }
    }

    async fn redrive_dlq(&self, topic: &str) -> anyhow::Result<u64> {
        use super::naming::FnQueueNames;
        use lapin::options::*;
        use serde_json::Value;

        let (dlq_name, is_fn_queue) = Self::resolve_dlq_name(topic);
        let mut count: u64 = 0;

        loop {
            let get_result = self
                .channel
                .basic_get(&dlq_name, BasicGetOptions { no_ack: false })
                .await
                .map_err(|e| anyhow::anyhow!("Failed to get message from DLQ: {}", e))?;

            match get_result {
                Some(delivery) => {
                    let republish_result: anyhow::Result<()> = if is_fn_queue {
                        // Function queue DLQ: raw data payload, republish directly
                        let queue_name = topic.strip_prefix("__fn_queue::").unwrap();
                        let names = FnQueueNames::new(queue_name);

                        let mut headers = delivery
                            .delivery
                            .properties
                            .headers()
                            .clone()
                            .unwrap_or_default();
                        headers.insert("x-attempt".into(), lapin::types::AMQPValue::LongUInt(0));

                        let properties = lapin::BasicProperties::default()
                            .with_content_type("application/json".into())
                            .with_delivery_mode(2)
                            .with_headers(headers);

                        let properties =
                            if let Some(mid) = delivery.delivery.properties.message_id() {
                                properties.with_message_id(mid.clone())
                            } else {
                                properties
                            };

                        self.channel
                            .basic_publish(
                                &names.exchange(),
                                queue_name,
                                BasicPublishOptions::default(),
                                &delivery.delivery.data,
                                properties,
                            )
                            .await
                            .map_err(|e| anyhow::anyhow!("Failed to republish: {}", e))?
                            .await
                            .map_err(|e| anyhow::anyhow!("Failed to confirm: {}", e))?;
                        Ok(())
                    } else {
                        // Topic DLQ: wrapped payload with job/error/exhausted_at
                        let dlq_payload: Value = serde_json::from_slice(&delivery.delivery.data)
                            .map_err(|e| anyhow::anyhow!("Failed to parse DLQ message: {}", e))?;

                        let job: super::types::Job = serde_json::from_value(
                            dlq_payload
                                .get("job")
                                .ok_or_else(|| anyhow::anyhow!("DLQ message missing 'job' field"))?
                                .clone(),
                        )
                        .map_err(|e| anyhow::anyhow!("Failed to parse job: {}", e))?;

                        let mut redriven = job;
                        redriven.attempts_made = 0;

                        self.publisher
                            .publish(topic, &redriven)
                            .await
                            .map_err(|e| anyhow::anyhow!("Failed to republish: {}", e))?;
                        Ok(())
                    };

                    if let Err(e) = republish_result {
                        tracing::error!(error = ?e, "Failed to republish DLQ message");
                        delivery
                            .delivery
                            .nack(BasicNackOptions {
                                requeue: true,
                                multiple: false,
                            })
                            .await
                            .map_err(|e| anyhow::anyhow!("Failed to nack message: {}", e))?;
                        break;
                    }

                    delivery
                        .delivery
                        .ack(BasicAckOptions { multiple: false })
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to ack message: {}", e))?;

                    count += 1;
                }
                None => break,
            }
        }

        Ok(count)
    }

    async fn redrive_dlq_message(&self, topic: &str, message_id: &str) -> anyhow::Result<bool> {
        use super::naming::FnQueueNames;

        let publisher = self.publisher.clone();
        let channel = Arc::clone(&self.channel);
        let topic_owned = topic.to_string();

        self.find_dlq_message(topic, message_id, |delivery, is_fn_queue, queue_name| {
            let delivery_data = delivery.data.clone();
            let delivery_props = delivery.properties.clone();
            let queue_name = queue_name.to_string();

            async move {
                if is_fn_queue {
                    // Function queue: republish raw data to the function queue exchange
                    let names = FnQueueNames::new(&queue_name);

                    // Preserve original headers, reset attempt counter
                    let mut headers = delivery_props.headers().clone().unwrap_or_default();
                    headers.insert("x-attempt".into(), lapin::types::AMQPValue::LongUInt(0));

                    let properties = lapin::BasicProperties::default()
                        .with_content_type("application/json".into())
                        .with_delivery_mode(2)
                        .with_headers(headers);

                    // Copy message_id if present
                    let properties = if let Some(mid) = delivery_props.message_id() {
                        properties.with_message_id(mid.clone())
                    } else {
                        properties
                    };

                    channel
                        .basic_publish(
                            &names.exchange(),
                            &queue_name,
                            BasicPublishOptions::default(),
                            &delivery_data,
                            properties,
                        )
                        .await
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to republish to function queue: {}", e)
                        })?
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to confirm republish: {}", e))?;
                } else {
                    // Topic queue: parse job, reset attempts, republish via Publisher
                    let dlq_payload: Value = serde_json::from_slice(&delivery_data)
                        .map_err(|e| anyhow::anyhow!("Failed to parse DLQ message: {}", e))?;

                    let job_value = dlq_payload
                        .get("job")
                        .ok_or_else(|| anyhow::anyhow!("DLQ message missing 'job' field"))?;

                    let mut job: super::types::Job = serde_json::from_value(job_value.clone())
                        .map_err(|e| anyhow::anyhow!("Failed to parse job: {}", e))?;

                    job.attempts_made = 0;

                    publisher
                        .publish(&topic_owned, &job)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to republish message: {}", e))?;
                }

                Ok(())
            }
        })
        .await
    }

    async fn discard_dlq_message(&self, topic: &str, message_id: &str) -> anyhow::Result<bool> {
        // Discard = just ack (handled by find_dlq_message after on_found returns)
        self.find_dlq_message(
            topic,
            message_id,
            |_delivery, _is_fn_queue, _queue_name| async { Ok(()) },
        )
        .await
    }

    async fn dlq_count(&self, topic: &str) -> anyhow::Result<u64> {
        // Function queues use FnQueueNames (e.g., __fn_queue::orders -> ::dlq.queue),
        // while topic-based queues use RabbitNames (e.g., user.created -> .dlq).
        let (dlq_name, _is_fn_queue) = Self::resolve_dlq_name(topic);

        let queue = self
            .channel
            .queue_declare(
                &dlq_name,
                lapin::options::QueueDeclareOptions {
                    passive: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get DLQ info: {}", e))?;

        Ok(queue.message_count() as u64)
    }

    async fn dlq_messages(&self, topic: &str, count: usize) -> anyhow::Result<Vec<Value>> {
        let (dlq_name, _is_fn_queue) = Self::resolve_dlq_name(topic);

        // Cap iteration at actual queue depth to avoid unnecessary basic_get calls
        let queue_depth = match self
            .channel
            .queue_declare(
                &dlq_name,
                lapin::options::QueueDeclareOptions {
                    passive: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
        {
            Ok(info) => info.message_count() as usize,
            Err(_) => return Ok(vec![]),
        };

        let fetch_count = count.min(queue_depth);
        let mut messages = Vec::new();
        let mut deliveries = Vec::new();

        for _ in 0..fetch_count {
            let get_result = self
                .channel
                .basic_get(&dlq_name, BasicGetOptions { no_ack: false })
                .await
                .map_err(|e| anyhow::anyhow!("Failed to get message from DLQ: {}", e))?;

            let Some(delivery) = get_result else { break };

            let payload: Value = serde_json::from_slice(&delivery.delivery.data)
                .map_err(|e| anyhow::anyhow!("Failed to parse DLQ message: {}", e))?;

            messages.push(payload);
            deliveries.push(delivery);
        }

        // Nack all back to DLQ (peek, not consume)
        for delivery in deliveries {
            let _ = delivery
                .delivery
                .nack(BasicNackOptions {
                    requeue: true,
                    multiple: false,
                })
                .await;
        }

        Ok(messages)
    }

    async fn dlq_peek(
        &self,
        topic: &str,
        offset: u64,
        limit: u64,
    ) -> anyhow::Result<Vec<crate::modules::queue::DlqMessage>> {
        let (dlq_name, is_fn_queue) = Self::resolve_dlq_name(topic);

        // Cap at actual queue depth
        let queue_depth = match self
            .channel
            .queue_declare(
                &dlq_name,
                lapin::options::QueueDeclareOptions {
                    passive: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
        {
            Ok(info) => info.message_count() as u64,
            Err(_) => return Ok(vec![]),
        };

        let fetch_count = (offset + limit).min(queue_depth) as usize;
        let mut results = Vec::new();
        let mut deliveries_to_nack = Vec::new();

        for i in 0..fetch_count {
            let get_result = self
                .channel
                .basic_get(&dlq_name, BasicGetOptions { no_ack: false })
                .await
                .map_err(|e| anyhow::anyhow!("Failed to get message from DLQ: {}", e))?;

            let Some(delivery) = get_result else { break };

            // Only process messages past the offset
            if (i as u64) >= offset {
                let raw_data = &delivery.delivery.data;
                let payload: Value = serde_json::from_slice(raw_data).unwrap_or(Value::Null);

                let dlq_msg = if is_fn_queue {
                    // Function queue DLQ: stable ID from AMQP properties or body hash
                    let id = delivery_stable_id(&delivery.delivery);

                    let function_id = delivery
                        .delivery
                        .properties
                        .headers()
                        .as_ref()
                        .and_then(|h| h.inner().get("function_id"))
                        .and_then(|v| match v {
                            lapin::types::AMQPValue::LongString(s) => Some(s.to_string()),
                            _ => None,
                        })
                        .unwrap_or_default();

                    let error = format!("Function {} exhausted retries", function_id);

                    crate::modules::queue::DlqMessage {
                        id,
                        payload,
                        error,
                        failed_at: 0,
                        retries: delivery
                            .delivery
                            .properties
                            .headers()
                            .as_ref()
                            .and_then(|h| h.inner().get("x-attempt"))
                            .and_then(|v| match v {
                                lapin::types::AMQPValue::LongUInt(n) => Some(*n),
                                _ => None,
                            })
                            .unwrap_or(0),
                        size_bytes: raw_data.len() as u64,
                    }
                } else {
                    // Topic-based DLQ: wrapped payload with job/error/exhausted_at
                    let id = payload
                        .get("job")
                        .and_then(|j| j.get("id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    let job_payload = payload
                        .get("job")
                        .and_then(|j| j.get("data"))
                        .cloned()
                        .unwrap_or(Value::Null);

                    let error = payload
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let failed_at = payload
                        .get("exhausted_at")
                        .and_then(|v| v.as_u64())
                        .map(|ms| ms / 1000)
                        .unwrap_or(0);

                    let retries = payload
                        .get("job")
                        .and_then(|j| j.get("attempts_made"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;

                    crate::modules::queue::DlqMessage {
                        id,
                        payload: job_payload,
                        error,
                        failed_at,
                        retries,
                        size_bytes: raw_data.len() as u64,
                    }
                };

                results.push(dlq_msg);
            }

            deliveries_to_nack.push(delivery);
        }

        // Nack all back to DLQ (peek, not consume)
        for delivery in deliveries_to_nack {
            let _ = delivery
                .delivery
                .nack(BasicNackOptions {
                    requeue: true,
                    multiple: false,
                })
                .await;
        }

        Ok(results)
    }

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
    ) {
        use super::naming::FnQueueNames;

        let names = FnQueueNames::new(queue_name);

        let payload = match serde_json::to_vec(&data) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize data");
                return;
            }
        };

        let mut headers = lapin::types::FieldTable::default();
        headers.insert(
            "function_id".into(),
            lapin::types::AMQPValue::LongString(function_id.into()),
        );
        if let Some(tp) = &traceparent {
            headers.insert(
                "traceparent".into(),
                lapin::types::AMQPValue::LongString(tp.as_str().into()),
            );
        }
        if let Some(bg) = &baggage {
            headers.insert(
                "baggage".into(),
                lapin::types::AMQPValue::LongString(bg.as_str().into()),
            );
        }

        let properties = lapin::BasicProperties::default()
            .with_content_type("application/json".into())
            .with_delivery_mode(2)
            .with_message_id(message_id.into())
            .with_headers(headers);

        match self
            .channel
            .basic_publish(
                &names.exchange(),
                queue_name,
                BasicPublishOptions::default(),
                &payload,
                properties,
            )
            .await
        {
            Ok(confirm) => {
                if let Err(e) = confirm.await {
                    tracing::error!(error = %e, queue = %queue_name, "Failed to confirm publish to function queue");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, queue = %queue_name, "Failed to publish to function queue");
            }
        }
    }

    async fn setup_function_queue(
        &self,
        queue_name: &str,
        config: &FunctionQueueConfig,
    ) -> anyhow::Result<()> {
        self.topology
            .setup_function_queue(queue_name, config.backoff_ms)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to setup function queue topology: {}", e))
    }

    async fn consume_function_queue(
        &self,
        queue_name: &str,
        prefetch: u32,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<crate::modules::queue::QueueMessage>> {
        use super::naming::FnQueueNames;
        use crate::modules::queue::QueueMessage;
        use futures::StreamExt;

        let names = FnQueueNames::new(queue_name);

        self.channel
            .basic_qos(prefetch as u16, BasicQosOptions::default())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to set QoS: {}", e))?;

        let consumer_tag = format!("fn-queue-{}-{}", queue_name, Uuid::new_v4());
        let mut consumer = self
            .channel
            .basic_consume(
                &names.queue(),
                &consumer_tag,
                BasicConsumeOptions::default(),
                lapin::types::FieldTable::default(),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create consumer: {}", e))?;

        let (tx, rx) = tokio::sync::mpsc::channel(prefetch as usize);
        let delivery_map = Arc::clone(&self.delivery_map);
        let delivery_counter = Arc::clone(&self.delivery_counter);

        tokio::spawn(async move {
            while let Some(delivery_result) = consumer.next().await {
                match delivery_result {
                    Ok(delivery) => {
                        let delivery_id = delivery_counter.fetch_add(1, Ordering::SeqCst);

                        let data: serde_json::Value = match serde_json::from_slice(&delivery.data) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to parse function queue message");
                                let _ = delivery
                                    .nack(BasicNackOptions {
                                        requeue: false,
                                        ..Default::default()
                                    })
                                    .await;
                                continue;
                            }
                        };

                        let headers = delivery.properties.headers().as_ref();

                        let function_id = headers
                            .and_then(|h| h.inner().get("function_id"))
                            .and_then(|v| match v {
                                lapin::types::AMQPValue::LongString(s) => Some(s.to_string()),
                                _ => None,
                            })
                            .unwrap_or_default();

                        let traceparent = headers
                            .and_then(|h| h.inner().get("traceparent"))
                            .and_then(|v| match v {
                                lapin::types::AMQPValue::LongString(s) => Some(s.to_string()),
                                _ => None,
                            });

                        let baggage =
                            headers
                                .and_then(|h| h.inner().get("baggage"))
                                .and_then(|v| match v {
                                    lapin::types::AMQPValue::LongString(s) => Some(s.to_string()),
                                    _ => None,
                                });

                        let attempt = headers
                            .and_then(|h| h.inner().get("x-attempt"))
                            .and_then(|v| match v {
                                lapin::types::AMQPValue::LongUInt(n) => Some(*n),
                                _ => None,
                            })
                            .unwrap_or(0);

                        let message_id = delivery
                            .properties
                            .message_id()
                            .as_ref()
                            .map(|s| s.to_string());

                        delivery_map.write().await.insert(delivery_id, delivery);

                        let msg = QueueMessage {
                            delivery_id,
                            function_id,
                            data,
                            attempt,
                            message_id,
                            traceparent,
                            baggage,
                        };

                        if tx.send(msg).await.is_err() {
                            // Receiver dropped — nack the delivery we just stored so
                            // RabbitMQ requeues it rather than leaving it stranded.
                            if let Some(d) = delivery_map.write().await.remove(&delivery_id) {
                                let _ = d
                                    .nack(BasicNackOptions {
                                        requeue: true,
                                        ..Default::default()
                                    })
                                    .await;
                            }
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Error receiving delivery from function queue");
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn ack_function_queue(&self, _queue_name: &str, delivery_id: u64) -> anyhow::Result<()> {
        let delivery = self.delivery_map.write().await.remove(&delivery_id);
        if let Some(delivery) = delivery {
            delivery
                .ack(BasicAckOptions::default())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to ack: {}", e))?;
        }
        Ok(())
    }

    async fn nack_function_queue(
        &self,
        queue_name: &str,
        delivery_id: u64,
        attempt: u32,
        max_retries: u32,
    ) -> anyhow::Result<()> {
        use super::naming::FnQueueNames;

        let delivery = self.delivery_map.write().await.remove(&delivery_id);
        let delivery = match delivery {
            Some(d) => d,
            None => return Ok(()),
        };

        if attempt < max_retries {
            // Retry: ack the original delivery, republish to the retry exchange.
            // The retry queue has a TTL; after expiry RabbitMQ dead-letters the
            // message back to the main exchange (configured via x-dead-letter-exchange
            // on the retry queue).
            delivery
                .ack(BasicAckOptions::default())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to ack for retry: {}", e))?;

            let names = FnQueueNames::new(queue_name);

            let mut headers = delivery.properties.headers().clone().unwrap_or_default();

            // Increment our own attempt counter so classic queues (which do not
            // populate x-delivery-count) can still track retry depth.
            let current_attempt = headers
                .inner()
                .get("x-attempt")
                .and_then(|v| match v {
                    lapin::types::AMQPValue::LongUInt(n) => Some(*n),
                    _ => None,
                })
                .unwrap_or(0);
            headers.insert(
                "x-attempt".into(),
                lapin::types::AMQPValue::LongUInt(current_attempt + 1),
            );

            let mut properties = lapin::BasicProperties::default()
                .with_content_type("application/json".into())
                .with_delivery_mode(2)
                .with_headers(headers);

            // Preserve message_id through retries so DLQ messages remain identifiable
            if let Some(mid) = delivery.properties.message_id() {
                properties = properties.with_message_id(mid.clone());
            }

            self.channel
                .basic_publish(
                    &names.retry_exchange(),
                    queue_name,
                    BasicPublishOptions::default(),
                    &delivery.data,
                    properties,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Failed to publish to retry exchange: {}", e))?
                .await
                .map_err(|e| anyhow::anyhow!("Failed to confirm retry publish: {}", e))?;

            tracing::debug!(
                queue = %queue_name,
                attempt = attempt,
                max_retries = max_retries,
                "Message sent to retry queue"
            );
        } else {
            // Exhausted: nack without requeue. The main queue's DLX points to the
            // DLQ exchange, so RabbitMQ routes the message there automatically.
            delivery
                .nack(BasicNackOptions {
                    requeue: false,
                    ..Default::default()
                })
                .await
                .map_err(|e| anyhow::anyhow!("Failed to nack to DLQ: {}", e))?;

            tracing::warn!(
                queue = %queue_name,
                attempt = attempt,
                max_retries = max_retries,
                "Message exhausted retries, routed to DLQ"
            );
        }

        Ok(())
    }

    async fn topic_stats(&self, topic: &str) -> anyhow::Result<crate::modules::queue::TopicStats> {
        use super::naming::{FnQueueNames, RabbitNames};

        let (queue_name, dlq_name) = if topic.starts_with("__fn_queue::") {
            let name = topic.strip_prefix("__fn_queue::").unwrap();
            let names = FnQueueNames::new(name);
            (names.queue(), names.dlq())
        } else {
            let names = RabbitNames::new(topic);
            (names.queue(), names.dlq())
        };

        // Get main queue depth (passive declare to inspect without creating)
        let depth = match self
            .channel
            .queue_declare(
                &queue_name,
                lapin::options::QueueDeclareOptions {
                    passive: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
        {
            Ok(info) => info.message_count() as u64,
            Err(_) => 0,
        };

        // Get DLQ depth
        let dlq_depth = match self
            .channel
            .queue_declare(
                &dlq_name,
                lapin::options::QueueDeclareOptions {
                    passive: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
        {
            Ok(info) => info.message_count() as u64,
            Err(_) => 0,
        };

        // Count consumers for this topic from subscriptions map
        let consumer_count = {
            let subs = self.subscriptions.read().await;
            subs.keys()
                .filter(|key| key.split(':').next().map(|t| t == topic).unwrap_or(false))
                .count() as u64
        };

        Ok(crate::modules::queue::TopicStats {
            depth,
            consumer_count,
            dlq_depth,
            config: None,
        })
    }

    async fn list_topics(&self) -> anyhow::Result<Vec<crate::modules::queue::TopicInfo>> {
        let subs = self.subscriptions.read().await;
        let mut topics: HashMap<String, u64> = HashMap::new();
        for key in subs.keys() {
            // subscription key format is "topic:id"
            if let Some(topic) = key.split(':').next() {
                *topics.entry(topic.to_string()).or_insert(0u64) += 1;
            }
        }
        Ok(topics
            .into_iter()
            .map(|(name, count)| crate::modules::queue::TopicInfo {
                name,
                broker_type: "rabbitmq".to_string(),
                subscriber_count: count,
            })
            .collect())
    }
}

crate::register_adapter!(<QueueAdapterRegistration> "modules::queue::RabbitMQAdapter", make_adapter);
