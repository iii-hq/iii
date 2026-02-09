// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

#![cfg(feature = "rabbitmq")]

use std::{collections::HashMap, str::FromStr, sync::Arc};

use async_trait::async_trait;
use lapin::{Channel, Connection, ConnectionProperties, options::*};
use serde_json::Value;
use tokio::{sync::RwLock, task::JoinHandle};
use uuid::Uuid;

use crate::{
    engine::Engine,
    modules::queue::{
        QueueAdapter, SubscriberQueueConfig,
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
}

struct SubscriptionInfo {
    id: String,
    consumer_tag: String,
    task_handle: JoinHandle<()>,
}

impl RabbitMQAdapter {
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
        })
    }
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
    async fn enqueue(&self, topic: &str, data: Value) {
        let job = Job::new(topic, data, self.config.max_attempts);

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
        use super::naming::RabbitNames;
        use lapin::options::*;
        use serde_json::Value;

        let names = RabbitNames::new(topic);
        let dlq_name = names.dlq();

        let mut count: u64 = 0;

        loop {
            let get_result = self
                .channel
                .basic_get(&dlq_name, BasicGetOptions { no_ack: false })
                .await
                .map_err(|e| anyhow::anyhow!("Failed to get message from DLQ: {}", e))?;

            match get_result {
                Some(delivery) => {
                    let dlq_payload: Value = serde_json::from_slice(&delivery.delivery.data)
                        .map_err(|e| anyhow::anyhow!("Failed to parse DLQ message: {}", e))?;

                    let job: super::types::Job = serde_json::from_value(
                        dlq_payload
                            .get("job")
                            .ok_or_else(|| anyhow::anyhow!("DLQ message missing 'job' field"))?
                            .clone(),
                    )
                    .map_err(|e| anyhow::anyhow!("Failed to parse job from DLQ payload: {}", e))?;

                    let mut job_with_reset_attempts = job.clone();
                    job_with_reset_attempts.attempts_made = 0;

                    if let Err(e) = self
                        .publisher
                        .publish(topic, &job_with_reset_attempts)
                        .await
                    {
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

    async fn dlq_count(&self, topic: &str) -> anyhow::Result<u64> {
        use super::naming::RabbitNames;

        let names = RabbitNames::new(topic);
        let dlq_name = names.dlq();

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
}

crate::register_adapter!(<QueueAdapterRegistration> "modules::queue::RabbitMQAdapter", make_adapter);
