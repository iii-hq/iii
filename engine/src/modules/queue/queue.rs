// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use colored::Colorize;
use function_macros::{function, service};
use futures::{Future, FutureExt};
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::panic::AssertUnwindSafe;

use super::{QueueAdapter, SubscriberQueueConfig, TopicInfo, config::QueueModuleConfig};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::{
    engine::{Engine, EngineTrait, Handler, QueueEnqueuer, RegisterFunctionRequest},
    function::FunctionResult,
    modules::module::{AdapterFactory, ConfigurableModule, Module},
    protocol::ErrorBody,
    telemetry::{SpanExt, inject_baggage_from_context, inject_traceparent_from_context},
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};

#[derive(Clone)]
pub struct QueueCoreModule {
    adapter: Arc<dyn QueueAdapter>,
    engine: Arc<Engine>,
    _config: QueueModuleConfig,
}

#[derive(Deserialize, JsonSchema)]
pub struct QueueInput {
    topic: String,
    data: Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct RedriveInput {
    queue: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RedriveResult {
    pub queue: String,
    pub redriven: u64,
}

#[derive(Deserialize, JsonSchema)]
pub struct RedriveSingleInput {
    queue: String,
    message_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RedriveSingleResult {
    pub queue: String,
    pub message_id: String,
    pub redriven: u64,
}

#[service(name = "queue")]
impl QueueCoreModule {
    /// Resolves a display-friendly queue name to the internal adapter key.
    /// Function queues (listed in config) are prefixed with `__fn_queue::`;
    /// topic-based queues pass through unchanged.
    fn resolve_queue_key(&self, name: &str) -> String {
        if self._config.queue_configs.contains_key(name) {
            format!("__fn_queue::{}", name)
        } else {
            name.to_string()
        }
    }

    pub async fn enqueue_to_function_queue(
        &self,
        queue_name: &str,
        function_id: &str,
        data: Value,
        message_id: &str,
        traceparent: Option<String>,
        baggage: Option<String>,
    ) -> anyhow::Result<()> {
        let queue_config = self._config.queue_configs.get(queue_name).ok_or_else(|| {
            tracing::warn!(
                queue_name = %queue_name,
                available = ?self._config.queue_configs.keys().collect::<Vec<_>>(),
                "Enqueue attempted for unknown queue"
            );
            anyhow::anyhow!("Queue '{}' not found", queue_name)
        })?;

        // FIFO validation: ensure message_group_field is configured and present in data
        if queue_config.r#type == "fifo" {
            let group_field = queue_config.message_group_field.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "FIFO queue '{}' requires 'message_group_field' to be configured",
                    queue_name
                )
            })?;
            let group_value = data.get(group_field).ok_or_else(|| {
                anyhow::anyhow!(
                    "FIFO queue '{}' requires field '{}' in data, but it was not found",
                    queue_name,
                    group_field
                )
            })?;
            if group_value.is_null() {
                return Err(anyhow::anyhow!(
                    "FIFO queue '{}': field '{}' must not be null",
                    queue_name,
                    group_field
                ));
            }
        }

        self.adapter
            .publish_to_function_queue(
                queue_name,
                function_id,
                data,
                message_id,
                queue_config.max_retries,
                queue_config.backoff_ms,
                traceparent,
                baggage,
            )
            .await;
        crate::modules::telemetry::collector::track_queue_emit();
        Ok(())
    }

    /// Returns the number of messages in the DLQ for a function queue.
    pub async fn function_queue_dlq_count(&self, queue_name: &str) -> anyhow::Result<u64> {
        let namespaced = format!("__fn_queue::{}", queue_name);
        self.adapter.dlq_count(&namespaced).await
    }

    /// Returns up to `count` DLQ messages for a function queue as parsed JSON Values.
    pub async fn function_queue_dlq_messages(
        &self,
        queue_name: &str,
        count: usize,
    ) -> anyhow::Result<Vec<Value>> {
        let namespaced = format!("__fn_queue::{}", queue_name);
        self.adapter.dlq_messages(&namespaced, count).await
    }

    #[function(id = "enqueue", description = "Enqueue a message")]
    pub async fn enqueue(&self, input: QueueInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let adapter = self.adapter.clone();
        let event_data = input.data;
        let topic = input.topic;

        if topic.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "topic_not_set".into(),
                message: "Topic is not set".into(),
                stacktrace: None,
            });
        }

        // Record the queue topic on the current span for trace visibility
        let current_span = tracing::Span::current();
        current_span.set_attribute("messaging.destination.name", topic.clone());
        current_span.set_attribute("messaging.operation.type", "publish".to_string());
        current_span.set_attribute("baggage.topic", topic.clone());

        let ctx = current_span.context();
        let traceparent = inject_traceparent_from_context(&ctx);
        let baggage = inject_baggage_from_context(&ctx);

        tracing::debug!(topic = %topic, traceparent = ?traceparent, baggage = ?baggage, "Enqueuing message with trace context");
        let _ = adapter
            .enqueue(&topic, event_data.clone(), traceparent, baggage)
            .await;
        crate::modules::telemetry::collector::track_queue_emit();

        FunctionResult::Success(None)
    }

    #[function(
        id = "iii::queue::redrive",
        description = "Redrive all DLQ messages back to the main queue"
    )]
    pub async fn redrive(&self, input: RedriveInput) -> FunctionResult<RedriveResult, ErrorBody> {
        if input.queue.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "queue_not_set".into(),
                message: "Queue name is required".into(),
                stacktrace: None,
            });
        }

        let resolved = self.resolve_queue_key(&input.queue);
        match self.adapter.redrive_dlq(&resolved).await {
            Ok(count) => {
                tracing::info!(
                    queue = %input.queue,
                    redriven = %count,
                    "Redrove DLQ messages back to main queue"
                );
                FunctionResult::Success(RedriveResult {
                    queue: input.queue,
                    redriven: count,
                })
            }
            Err(e) => {
                tracing::error!(
                    queue = %input.queue,
                    error = %e,
                    "Failed to redrive DLQ"
                );
                FunctionResult::Failure(ErrorBody {
                    code: "redrive_failed".into(),
                    message: format!("Failed to redrive DLQ for queue '{}': {}", input.queue, e),
                    stacktrace: None,
                })
            }
        }
    }

    #[function(
        id = "iii::queue::redrive_message",
        description = "Redrive a single DLQ message by ID back to the main queue"
    )]
    pub async fn redrive_message(
        &self,
        input: RedriveSingleInput,
    ) -> FunctionResult<RedriveSingleResult, ErrorBody> {
        if input.queue.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "queue_not_set".into(),
                message: "Queue name is required".into(),
                stacktrace: None,
            });
        }

        if input.message_id.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "message_id_not_set".into(),
                message: "Message ID is required".into(),
                stacktrace: None,
            });
        }

        let resolved = self.resolve_queue_key(&input.queue);
        match self
            .adapter
            .redrive_dlq_message(&resolved, &input.message_id)
            .await
        {
            Ok(found) => {
                let redriven = if found { 1 } else { 0 };
                tracing::info!(
                    queue = %input.queue,
                    message_id = %input.message_id,
                    redriven = %redriven,
                    "Redrove single DLQ message back to main queue"
                );
                FunctionResult::Success(RedriveSingleResult {
                    queue: input.queue,
                    message_id: input.message_id,
                    redriven,
                })
            }
            Err(e) => {
                tracing::error!(
                    queue = %input.queue,
                    message_id = %input.message_id,
                    error = %e,
                    "Failed to redrive single DLQ message"
                );
                FunctionResult::Failure(ErrorBody {
                    code: "redrive_message_failed".into(),
                    message: format!(
                        "Failed to redrive DLQ message '{}' for queue '{}': {}",
                        input.message_id, input.queue, e
                    ),
                    stacktrace: None,
                })
            }
        }
    }

    #[function(
        id = "iii::queue::discard_message",
        description = "Discard (purge) a single DLQ message by ID"
    )]
    pub async fn discard_message(
        &self,
        input: RedriveSingleInput,
    ) -> FunctionResult<RedriveSingleResult, ErrorBody> {
        if input.queue.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "queue_not_set".into(),
                message: "Queue name is required".into(),
                stacktrace: None,
            });
        }
        if input.message_id.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "message_id_not_set".into(),
                message: "Message ID is required".into(),
                stacktrace: None,
            });
        }

        let resolved = self.resolve_queue_key(&input.queue);
        match self
            .adapter
            .discard_dlq_message(&resolved, &input.message_id)
            .await
        {
            Ok(found) => {
                let discarded = if found { 1 } else { 0 };
                FunctionResult::Success(RedriveSingleResult {
                    queue: input.queue,
                    message_id: input.message_id,
                    redriven: discarded,
                })
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                code: "discard_message_failed".into(),
                message: format!("Failed to discard DLQ message: {}", e),
                stacktrace: None,
            }),
        }
    }

    #[function(
        id = "engine::queue::list_topics",
        description = "List all queue topics"
    )]
    pub async fn console_list_topics(
        &self,
        _input: Value,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.adapter.list_topics().await {
            Ok(topics) => {
                // Merge function queue topics from config.
                // Adapters report these with subscriber_count=0 because internal
                // consumers aren't tracked in the subscription map. Override with
                // the configured concurrency so the console shows the real count.
                let mut all_topics = topics;
                for (name, config) in &self._config.queue_configs {
                    let namespaced = format!("__fn_queue::{}", name);
                    if let Some(existing) = all_topics
                        .iter_mut()
                        .find(|t| t.name == namespaced || t.name == *name)
                    {
                        existing.subscriber_count = config.concurrency as u64;
                    } else {
                        all_topics.push(TopicInfo {
                            name: name.clone(),
                            broker_type: "function_queue".to_string(),
                            subscriber_count: config.concurrency as u64,
                        });
                    }
                }
                // Normalize: strip __fn_queue:: prefix for display
                for topic in &mut all_topics {
                    if let Some(stripped) = topic.name.strip_prefix("__fn_queue::") {
                        topic.name = stripped.to_string();
                    }
                }
                FunctionResult::Success(Some(
                    serde_json::to_value(&all_topics).unwrap_or(json!([])),
                ))
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                code: "list_topics_failed".into(),
                message: format!("Failed to list topics: {}", e),
                stacktrace: None,
            }),
        }
    }

    #[function(
        id = "engine::queue::topic_stats",
        description = "Get stats for a queue topic"
    )]
    pub async fn console_topic_stats(
        &self,
        input: Value,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let topic = input.get("topic").and_then(|v| v.as_str()).unwrap_or("");
        if topic.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "topic_required".into(),
                message: "topic is required".into(),
                stacktrace: None,
            });
        }

        let resolved = self.resolve_queue_key(topic);
        match self.adapter.topic_stats(&resolved).await {
            Ok(mut stats) => {
                // Adapters report consumer_count=0 for function queues because
                // internal consumers aren't tracked in the subscription map.
                // Override with the configured concurrency.
                if let Some(config) = self._config.queue_configs.get(topic) {
                    stats.consumer_count = config.concurrency as u64;
                }
                FunctionResult::Success(Some(serde_json::to_value(&stats).unwrap_or(json!({}))))
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                code: "topic_stats_failed".into(),
                message: format!("Failed to get topic stats: {}", e),
                stacktrace: None,
            }),
        }
    }

    #[function(
        id = "engine::queue::dlq_topics",
        description = "List DLQ topics with counts"
    )]
    pub async fn console_dlq_topics(
        &self,
        _input: Value,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.adapter.list_topics().await {
            Ok(topics) => {
                let mut dlq_topics = Vec::new();
                for topic in &topics {
                    let dlq_count = self.adapter.dlq_count(&topic.name).await.unwrap_or(0);
                    dlq_topics.push(json!({
                        "topic": topic.name,
                        "broker_type": topic.broker_type,
                        "message_count": dlq_count,
                    }));
                }
                // Also include function queue DLQs
                for name in self._config.queue_configs.keys() {
                    let namespaced = format!("__fn_queue::{}", name);
                    let dlq_count = self.adapter.dlq_count(&namespaced).await.unwrap_or(0);
                    dlq_topics.push(json!({
                        "topic": name,
                        "broker_type": "function_queue",
                        "message_count": dlq_count,
                    }));
                }
                FunctionResult::Success(Some(json!(dlq_topics)))
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                code: "dlq_topics_failed".into(),
                message: format!("Failed to list DLQ topics: {}", e),
                stacktrace: None,
            }),
        }
    }

    #[function(
        id = "engine::queue::dlq_messages",
        description = "Browse DLQ messages"
    )]
    pub async fn console_dlq_messages(
        &self,
        input: Value,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let topic = input.get("topic").and_then(|v| v.as_str()).unwrap_or("");
        if topic.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "topic_required".into(),
                message: "topic is required".into(),
                stacktrace: None,
            });
        }
        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0);
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(50);

        let resolved = self.resolve_queue_key(topic);
        match self.adapter.dlq_peek(&resolved, offset, limit).await {
            Ok(messages) => {
                FunctionResult::Success(Some(serde_json::to_value(&messages).unwrap_or(json!([]))))
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                code: "dlq_messages_failed".into(),
                message: format!("Failed to browse DLQ messages: {}", e),
                stacktrace: None,
            }),
        }
    }
}

#[async_trait]
impl QueueEnqueuer for QueueCoreModule {
    async fn enqueue_to_function_queue(
        &self,
        queue_name: &str,
        function_id: &str,
        data: Value,
        message_id: String,
        traceparent: Option<String>,
        baggage: Option<String>,
    ) -> anyhow::Result<()> {
        self.enqueue_to_function_queue(
            queue_name,
            function_id,
            data,
            &message_id,
            traceparent,
            baggage,
        )
        .await
    }

    async fn function_queue_dlq_count(&self, queue_name: &str) -> anyhow::Result<u64> {
        self.function_queue_dlq_count(queue_name).await
    }

    async fn function_queue_dlq_messages(
        &self,
        queue_name: &str,
        count: usize,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        self.function_queue_dlq_messages(queue_name, count).await
    }
}

impl TriggerRegistrator for QueueCoreModule {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let topic = trigger
            .clone()
            .config
            .get("topic")
            .unwrap_or_default()
            .as_str()
            .unwrap_or("")
            .to_string();

        tracing::info!(
            "{} Subscription {} → {}",
            "[REGISTERED]".green(),
            topic.purple(),
            trigger.function_id.cyan()
        );

        // Get adapter reference before async block
        let adapter = self.adapter.clone();

        Box::pin(async move {
            if !topic.is_empty() {
                let condition_function_id = trigger
                    .config
                    .get("condition_function_id")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());

                let queue_config = trigger
                    .config
                    .get("queue_config")
                    .and_then(|q| SubscriberQueueConfig::from_value(Some(q)));

                adapter
                    .subscribe(
                        &topic,
                        &trigger.id,
                        &trigger.function_id,
                        condition_function_id,
                        queue_config,
                    )
                    .await;
            } else {
                tracing::warn!(
                    function_id = %trigger.function_id.purple(),
                    "Topic is not set for trigger"
                );
            }

            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        // Get adapter reference before async block
        let adapter = self.adapter.clone();

        Box::pin(async move {
            tracing::debug!(trigger = %trigger.id, "Unregistering trigger");
            adapter
                .unsubscribe(
                    trigger
                        .config
                        .get("topic")
                        .unwrap_or_default()
                        .as_str()
                        .unwrap_or(""),
                    &trigger.id,
                )
                .await;
            Ok(())
        })
    }
}

#[async_trait]
impl Module for QueueCoreModule {
    fn name(&self) -> &'static str {
        "QueueModule"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing QueueModule");
        self._config.validate()?;
        self.engine.set_queue_module(Arc::new(self.clone())).await;

        for (name, config) in &self._config.queue_configs {
            self.adapter.setup_function_queue(name, config).await?;

            let prefetch = if config.r#type == "fifo" {
                1
            } else {
                config.concurrency
            };
            let max_retries = config.max_retries;
            let mut receiver = self.adapter.consume_function_queue(name, prefetch).await?;

            let adapter = self.adapter.clone();
            let engine = self.engine.clone();
            let queue_name = name.clone();

            let semaphore = Arc::new(tokio::sync::Semaphore::new(prefetch as usize));

            tokio::spawn(async move {
                while let Some(msg) = receiver.recv().await {
                    let adapter = adapter.clone();
                    let engine = engine.clone();
                    let queue_name = queue_name.clone();
                    let semaphore = semaphore.clone();

                    let permit = semaphore.acquire_owned().await;
                    if permit.is_err() {
                        break;
                    }
                    let permit = permit.unwrap();

                    let traceparent = msg.traceparent.clone();
                    let baggage = msg.baggage.clone();

                    tokio::spawn(async move {
                        let delivery_id = msg.delivery_id;
                        let function_id = msg.function_id.clone();
                        let attempt = msg.attempt;

                        let span = tracing::info_span!(
                            "fn_queue_job",
                            otel.name = %format!("fn_queue {}", queue_name),
                            function_id = %function_id,
                            queue = %queue_name,
                            attempt = %attempt,
                            delivery_id = %delivery_id,
                            "messaging.system" = "iii-queue",
                            "messaging.destination.name" = %queue_name,
                            "messaging.operation.type" = "process",
                            otel.status_code = tracing::field::Empty,
                        )
                        .with_parent_headers(traceparent.as_deref(), baggage.as_deref());

                        let result =
                            AssertUnwindSafe(async { engine.call(&function_id, msg.data).await })
                                .catch_unwind()
                                .instrument(span)
                                .await;

                        match result {
                            Ok(Ok(_)) => {
                                tracing::Span::current().record("otel.status_code", "OK");
                                if let Err(e) =
                                    adapter.ack_function_queue(&queue_name, delivery_id).await
                                {
                                    tracing::error!(error = %e, "Failed to ack message");
                                }
                            }
                            Ok(Err(ref err)) => {
                                tracing::Span::current().record("otel.status_code", "ERROR");
                                tracing::warn!(
                                    function_id = %function_id,
                                    queue = %queue_name,
                                    attempt = %attempt,
                                    max_retries = %max_retries,
                                    error = ?err,
                                    "Function queue job failed"
                                );
                                if let Err(e) = adapter
                                    .nack_function_queue(
                                        &queue_name,
                                        delivery_id,
                                        attempt,
                                        max_retries,
                                    )
                                    .await
                                {
                                    tracing::error!(error = %e, "Failed to nack message");
                                }
                            }
                            Err(_panic) => {
                                tracing::Span::current().record("otel.status_code", "ERROR");
                                tracing::error!(
                                    function_id = %function_id,
                                    queue = %queue_name,
                                    attempt = %attempt,
                                    max_retries = %max_retries,
                                    "Function queue job panicked"
                                );
                                if let Err(e) = adapter
                                    .nack_function_queue(
                                        &queue_name,
                                        delivery_id,
                                        attempt,
                                        max_retries,
                                    )
                                    .await
                                {
                                    tracing::error!(error = %e, "Failed to nack panicked message");
                                }
                            }
                        }

                        drop(permit);
                    });
                }

                tracing::warn!(queue = %queue_name, "Consumer loop ended");
            });

            tracing::info!(
                queue = %name,
                r#type = %config.r#type,
                concurrency = %config.concurrency,
                "Started function queue consumer"
            );
        }

        let trigger_type = TriggerType {
            id: "queue".to_string(),
            _description: "Queue core module".to_string(),
            registrator: Box::new(self.clone()),
            worker_id: None,
        };

        let _ = self.engine.register_trigger_type(trigger_type).await;

        Ok(())
    }
}

#[async_trait]
impl ConfigurableModule for QueueCoreModule {
    type Config = QueueModuleConfig;
    type Adapter = dyn QueueAdapter;
    type AdapterRegistration = super::registry::QueueAdapterRegistration;
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::queue::BuiltinQueueAdapter";

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn QueueAdapter>>>> =
            Lazy::new(|| RwLock::new(QueueCoreModule::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self {
            engine,
            _config: config,
            adapter,
        }
    }

    fn adapter_class_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.class.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

crate::register_module!(
    "modules::queue::QueueModule",
    QueueCoreModule,
    enabled_by_default = true
);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // =========================================================================
    // QueueInput deserialization
    // =========================================================================

    #[test]
    fn queue_input_deserialize() {
        let json = json!({
            "topic": "my-topic",
            "data": {"key": "value"}
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "my-topic");
        assert_eq!(input.data["key"], "value");
    }

    #[test]
    fn queue_input_deserialize_empty_topic() {
        let json = json!({
            "topic": "",
            "data": null
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "");
        assert_eq!(input.data, Value::Null);
    }

    #[test]
    fn queue_input_deserialize_missing_topic_fails() {
        let json = json!({"data": "hello"});
        let result: Result<QueueInput, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn queue_input_deserialize_missing_data_fails() {
        let json = json!({"topic": "test"});
        let result: Result<QueueInput, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn queue_input_deserialize_complex_data() {
        let json = json!({
            "topic": "events",
            "data": {
                "event_type": "user.created",
                "payload": {
                    "user_id": 123,
                    "email": "test@example.com"
                }
            }
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "events");
        assert_eq!(input.data["event_type"], "user.created");
        assert_eq!(input.data["payload"]["user_id"], 123);
    }

    // =========================================================================
    // QueueCoreModule::name
    // =========================================================================
    // Note: The module itself requires an adapter and engine, so we test what
    // we can without those dependencies.

    // =========================================================================
    // ConfigurableModule trait constants
    // =========================================================================

    #[test]
    fn default_adapter_class() {
        assert_eq!(
            QueueCoreModule::DEFAULT_ADAPTER_CLASS,
            "modules::queue::BuiltinQueueAdapter"
        );
    }

    // =========================================================================
    // QueueInput additional deserialization tests
    // =========================================================================

    #[test]
    fn queue_input_deserialize_array_data() {
        let json = json!({
            "topic": "batch",
            "data": [1, 2, 3]
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "batch");
        assert!(input.data.is_array());
        assert_eq!(input.data.as_array().unwrap().len(), 3);
    }

    #[test]
    fn queue_input_deserialize_string_data() {
        let json = json!({
            "topic": "simple",
            "data": "just a string"
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "simple");
        assert_eq!(input.data, "just a string");
    }

    #[test]
    fn queue_input_deserialize_number_data() {
        let json = json!({
            "topic": "numeric",
            "data": 42
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "numeric");
        assert_eq!(input.data, 42);
    }

    #[test]
    fn queue_input_deserialize_bool_data() {
        let json = json!({
            "topic": "flags",
            "data": true
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "flags");
        assert_eq!(input.data, true);
    }

    #[test]
    fn queue_input_deserialize_extra_fields_ignored() {
        // serde by default ignores extra fields (no deny_unknown_fields)
        let json = json!({
            "topic": "t",
            "data": null,
            "extra": "ignored"
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "t");
    }

    #[test]
    fn queue_input_deserialize_nested_deeply() {
        let json = json!({
            "topic": "deep",
            "data": { "a": { "b": { "c": { "d": 99 } } } }
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.data["a"]["b"]["c"]["d"], 99);
    }

    // =========================================================================
    // Mock adapter for integration tests
    // =========================================================================

    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use tokio::sync::Mutex;

    struct MockQueueAdapter {
        enqueue_count: AtomicU64,
        enqueue_to_queue_count: AtomicU64,
        subscribe_count: AtomicU64,
        unsubscribe_count: AtomicU64,
        last_topic: Mutex<String>,
        last_data: Mutex<Option<Value>>,
        last_traceparent: Mutex<Option<String>>,
        last_baggage: Mutex<Option<String>>,
        redrive_dlq_result: AtomicU64,
        redrive_dlq_should_fail: AtomicBool,
        // Fields for console/DLQ management tests
        list_topics_result: Mutex<Vec<TopicInfo>>,
        list_topics_should_fail: AtomicBool,
        topic_stats_result: Mutex<super::super::TopicStats>,
        topic_stats_should_fail: AtomicBool,
        dlq_peek_result: Mutex<Vec<super::super::DlqMessage>>,
        dlq_peek_should_fail: AtomicBool,
        dlq_count_value: AtomicU64,
        dlq_count_should_fail: AtomicBool,
        discard_should_fail: AtomicBool,
    }

    impl MockQueueAdapter {
        fn new() -> Self {
            Self {
                enqueue_count: AtomicU64::new(0),
                enqueue_to_queue_count: AtomicU64::new(0),
                subscribe_count: AtomicU64::new(0),
                unsubscribe_count: AtomicU64::new(0),
                last_topic: Mutex::new(String::new()),
                last_data: Mutex::new(None),
                last_traceparent: Mutex::new(None),
                last_baggage: Mutex::new(None),
                redrive_dlq_result: AtomicU64::new(0),
                redrive_dlq_should_fail: AtomicBool::new(false),
                list_topics_result: Mutex::new(vec![]),
                list_topics_should_fail: AtomicBool::new(false),
                topic_stats_result: Mutex::new(super::super::TopicStats {
                    depth: 0,
                    consumer_count: 0,
                    dlq_depth: 0,
                    config: None,
                }),
                topic_stats_should_fail: AtomicBool::new(false),
                dlq_peek_result: Mutex::new(vec![]),
                dlq_peek_should_fail: AtomicBool::new(false),
                dlq_count_value: AtomicU64::new(0),
                dlq_count_should_fail: AtomicBool::new(false),
                discard_should_fail: AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl QueueAdapter for MockQueueAdapter {
        async fn enqueue(
            &self,
            topic: &str,
            data: Value,
            traceparent: Option<String>,
            baggage: Option<String>,
        ) {
            self.enqueue_count.fetch_add(1, Ordering::SeqCst);
            *self.last_topic.lock().await = topic.to_string();
            *self.last_data.lock().await = Some(data);
            *self.last_traceparent.lock().await = traceparent;
            *self.last_baggage.lock().await = baggage;
        }

        async fn publish_to_function_queue(
            &self,
            _queue_name: &str,
            _function_id: &str,
            _data: Value,
            _message_id: &str,
            _max_retries: u32,
            _backoff_ms: u64,
            _traceparent: Option<String>,
            _baggage: Option<String>,
        ) {
            self.enqueue_to_queue_count.fetch_add(1, Ordering::SeqCst);
        }

        async fn subscribe(
            &self,
            _topic: &str,
            _id: &str,
            _function_id: &str,
            _condition_function_id: Option<String>,
            _queue_config: Option<SubscriberQueueConfig>,
        ) {
            self.subscribe_count.fetch_add(1, Ordering::SeqCst);
        }

        async fn unsubscribe(&self, _topic: &str, _id: &str) {
            self.unsubscribe_count.fetch_add(1, Ordering::SeqCst);
        }

        async fn redrive_dlq(&self, _topic: &str) -> anyhow::Result<u64> {
            if self.redrive_dlq_should_fail.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("mock redrive error"));
            }
            Ok(self.redrive_dlq_result.load(Ordering::SeqCst))
        }

        async fn redrive_dlq_message(
            &self,
            _topic: &str,
            _message_id: &str,
        ) -> anyhow::Result<bool> {
            if self.redrive_dlq_should_fail.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("mock redrive message error"));
            }
            Ok(self.redrive_dlq_result.load(Ordering::SeqCst) > 0)
        }

        async fn discard_dlq_message(
            &self,
            _topic: &str,
            _message_id: &str,
        ) -> anyhow::Result<bool> {
            if self.discard_should_fail.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("mock discard error"));
            }
            Ok(true)
        }

        async fn dlq_count(&self, _topic: &str) -> anyhow::Result<u64> {
            if self.dlq_count_should_fail.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("mock dlq_count error"));
            }
            Ok(self.dlq_count_value.load(Ordering::SeqCst))
        }

        async fn list_topics(&self) -> anyhow::Result<Vec<TopicInfo>> {
            if self.list_topics_should_fail.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("mock list_topics error"));
            }
            Ok(self.list_topics_result.lock().await.clone())
        }

        async fn topic_stats(&self, _topic: &str) -> anyhow::Result<super::super::TopicStats> {
            if self.topic_stats_should_fail.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("mock topic_stats error"));
            }
            Ok(self.topic_stats_result.lock().await.clone())
        }

        async fn dlq_peek(
            &self,
            _topic: &str,
            _offset: u64,
            _limit: u64,
        ) -> anyhow::Result<Vec<super::super::DlqMessage>> {
            if self.dlq_peek_should_fail.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("mock dlq_peek error"));
            }
            Ok(self.dlq_peek_result.lock().await.clone())
        }

        async fn setup_function_queue(
            &self,
            _queue_name: &str,
            _config: &super::super::config::FunctionQueueConfig,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn consume_function_queue(
            &self,
            _queue_name: &str,
            _prefetch: u32,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<super::super::QueueMessage>> {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }
    }

    fn setup_queue_module() -> (Arc<Engine>, QueueCoreModule, Arc<MockQueueAdapter>) {
        crate::modules::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter = Arc::new(MockQueueAdapter::new());
        let module = QueueCoreModule {
            adapter: adapter.clone(),
            engine: engine.clone(),
            _config: super::super::config::QueueModuleConfig::default(),
        };
        (engine, module, adapter)
    }

    // =========================================================================
    // QueueCoreModule::name
    // =========================================================================

    #[test]
    fn queue_module_name() {
        let (_engine, module, _adapter) = setup_queue_module();
        assert_eq!(Module::name(&module), "QueueModule");
    }

    // =========================================================================
    // enqueue service function tests
    // =========================================================================

    #[tokio::test]
    async fn enqueue_with_empty_topic_returns_failure() {
        let (_engine, module, _adapter) = setup_queue_module();
        let input = QueueInput {
            topic: "".to_string(),
            data: json!({"msg": "hello"}),
        };
        let result = module.enqueue(input).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "topic_not_set");
                assert_eq!(e.message, "Topic is not set");
            }
            _ => panic!("Expected Failure for empty topic"),
        }
    }

    #[tokio::test]
    async fn enqueue_success_calls_adapter() {
        let (_engine, module, adapter) = setup_queue_module();
        let input = QueueInput {
            topic: "my-topic".to_string(),
            data: json!({"msg": "hello"}),
        };
        let result = module.enqueue(input).await;
        assert!(matches!(result, FunctionResult::Success(None)));
        assert_eq!(adapter.enqueue_count.load(Ordering::SeqCst), 1);
        assert_eq!(*adapter.last_topic.lock().await, "my-topic");
        assert_eq!(
            *adapter.last_data.lock().await,
            Some(json!({"msg": "hello"}))
        );
    }

    #[tokio::test]
    async fn enqueue_multiple_calls() {
        let (_engine, module, adapter) = setup_queue_module();
        for i in 0..5 {
            let input = QueueInput {
                topic: format!("topic-{}", i),
                data: json!(i),
            };
            let result = module.enqueue(input).await;
            assert!(matches!(result, FunctionResult::Success(None)));
        }
        assert_eq!(adapter.enqueue_count.load(Ordering::SeqCst), 5);
    }

    // =========================================================================
    // redrive service function tests
    // =========================================================================

    #[tokio::test]
    async fn redrive_with_empty_queue_returns_failure() {
        let (_engine, module, _adapter) = setup_queue_module();
        let input = RedriveInput {
            queue: "".to_string(),
        };
        let result = module.redrive(input).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "queue_not_set");
                assert_eq!(e.message, "Queue name is required");
            }
            _ => panic!("Expected Failure for empty queue name"),
        }
    }

    #[tokio::test]
    async fn redrive_success_returns_result() {
        let (_engine, module, adapter) = setup_queue_module();
        adapter.redrive_dlq_result.store(5, Ordering::SeqCst);
        let input = RedriveInput {
            queue: "payment".to_string(),
        };
        let result = module.redrive(input).await;
        match result {
            FunctionResult::Success(r) => {
                assert_eq!(r.queue, "payment");
                assert_eq!(r.redriven, 5);
            }
            _ => panic!("Expected Success with RedriveResult"),
        }
    }

    #[tokio::test]
    async fn redrive_adapter_error_returns_failure() {
        let (_engine, module, adapter) = setup_queue_module();
        adapter
            .redrive_dlq_should_fail
            .store(true, Ordering::SeqCst);
        let input = RedriveInput {
            queue: "payment".to_string(),
        };
        let result = module.redrive(input).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "redrive_failed");
                assert!(e.message.contains("mock redrive error"));
            }
            _ => panic!("Expected Failure when adapter returns error"),
        }
    }

    // =========================================================================
    // TriggerRegistrator tests
    // =========================================================================

    #[tokio::test]
    async fn register_trigger_with_valid_topic_subscribes() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-1".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"topic": "my-topic"}),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());
        assert_eq!(adapter.subscribe_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn register_trigger_with_empty_topic_does_not_subscribe() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-2".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"topic": ""}),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());
        assert_eq!(adapter.subscribe_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn register_trigger_with_missing_topic_does_not_subscribe() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-3".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({}),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());
        // topic defaults to "" when missing, so subscribe should not be called
        assert_eq!(adapter.subscribe_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn register_trigger_with_condition_function_id_subscribes() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-cond".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "topic": "conditioned-topic",
                "condition_function_id": "test::condition_fn"
            }),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());
        assert_eq!(adapter.subscribe_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn register_trigger_with_queue_infrastructure_metadata() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-infra".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "topic": "infra-topic",
                "metadata": {
                    "infrastructure": {
                        "queue": {
                            "type": "fifo",
                            "maxRetries": 3
                        }
                    }
                }
            }),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());
        assert_eq!(adapter.subscribe_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unregister_trigger_calls_unsubscribe() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-unsub".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"topic": "unsub-topic"}),
            worker_id: None,
        };
        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());
        assert_eq!(adapter.unsubscribe_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unregister_trigger_with_no_topic() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-no-topic".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({}),
            worker_id: None,
        };
        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());
        // unsubscribe is called regardless (with empty topic)
        assert_eq!(adapter.unsubscribe_count.load(Ordering::SeqCst), 1);
    }

    // =========================================================================
    // ConfigurableModule trait tests
    // =========================================================================

    #[test]
    fn adapter_class_from_config_none() {
        let config = super::super::config::QueueModuleConfig::default();
        assert!(QueueCoreModule::adapter_class_from_config(&config).is_none());
    }

    #[test]
    fn adapter_class_from_config_some() {
        let config = super::super::config::QueueModuleConfig {
            adapter: Some(crate::modules::module::AdapterEntry {
                class: "my::CustomAdapter".to_string(),
                config: None,
            }),
            ..Default::default()
        };
        assert_eq!(
            QueueCoreModule::adapter_class_from_config(&config),
            Some("my::CustomAdapter".to_string())
        );
    }

    #[test]
    fn adapter_config_from_config_none() {
        let config = super::super::config::QueueModuleConfig::default();
        assert!(QueueCoreModule::adapter_config_from_config(&config).is_none());
    }

    #[test]
    fn adapter_config_from_config_some() {
        let config = super::super::config::QueueModuleConfig {
            adapter: Some(crate::modules::module::AdapterEntry {
                class: "my::Adapter".to_string(),
                config: Some(json!({"url": "redis://localhost"})),
            }),
            ..Default::default()
        };
        assert_eq!(
            QueueCoreModule::adapter_config_from_config(&config),
            Some(json!({"url": "redis://localhost"}))
        );
    }

    #[test]
    fn adapter_config_from_config_adapter_without_config() {
        let config = super::super::config::QueueModuleConfig {
            adapter: Some(crate::modules::module::AdapterEntry {
                class: "my::Adapter".to_string(),
                config: None,
            }),
            ..Default::default()
        };
        assert!(QueueCoreModule::adapter_config_from_config(&config).is_none());
    }

    // =========================================================================
    // Module::initialize test
    // =========================================================================

    #[tokio::test]
    async fn initialize_registers_trigger_type() {
        let (engine, module, _adapter) = setup_queue_module();
        let result = module.initialize().await;
        assert!(result.is_ok());
        assert!(engine.trigger_registry.trigger_types.contains_key("queue"));
    }

    // =========================================================================
    // build helper test
    // =========================================================================

    #[test]
    fn build_creates_module() {
        crate::modules::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter: Arc<dyn QueueAdapter> = Arc::new(MockQueueAdapter::new());
        let config = super::super::config::QueueModuleConfig::default();
        let module = QueueCoreModule::build(engine.clone(), config, adapter);
        assert_eq!(Module::name(&module), "QueueModule");
    }

    // =========================================================================
    // enqueue_to_function_queue tests
    // =========================================================================

    fn setup_queue_module_with_configs() -> (Arc<Engine>, QueueCoreModule, Arc<MockQueueAdapter>) {
        use super::super::config::{FunctionQueueConfig, QueueModuleConfig};

        crate::modules::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter = Arc::new(MockQueueAdapter::new());

        let mut queue_configs = HashMap::new();
        queue_configs.insert(
            "default".to_string(),
            FunctionQueueConfig {
                r#type: "standard".to_string(),
                ..Default::default()
            },
        );
        queue_configs.insert(
            "payment".to_string(),
            FunctionQueueConfig {
                r#type: "fifo".to_string(),
                message_group_field: Some("transaction_id".to_string()),
                ..Default::default()
            },
        );

        let config = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };

        let module = QueueCoreModule {
            adapter: adapter.clone(),
            engine: engine.clone(),
            _config: config,
        };

        (engine, module, adapter)
    }

    #[tokio::test]
    async fn enqueue_to_function_queue_unknown_queue_fails() {
        let (_engine, module, _adapter) = setup_queue_module_with_configs();
        let result = module
            .enqueue_to_function_queue(
                "nonexistent",
                "fn-1",
                json!({"key": "value"}),
                "test-msg-id",
                None,
                None,
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found"),
            "Error should contain 'not found', got: {}",
            err
        );
    }

    #[tokio::test]
    async fn enqueue_to_function_queue_fifo_missing_group_field_fails() {
        let (_engine, module, _adapter) = setup_queue_module_with_configs();
        let result = module
            .enqueue_to_function_queue(
                "payment",
                "fn-1",
                json!({"amount": 100}), // missing "transaction_id"
                "test-msg-id",
                None,
                None,
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("transaction_id"),
            "Error should mention the missing field name, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn enqueue_to_function_queue_fifo_null_group_field_fails() {
        let (_engine, module, _adapter) = setup_queue_module_with_configs();
        let result = module
            .enqueue_to_function_queue(
                "payment",
                "fn-1",
                json!({"transaction_id": null, "amount": 100}),
                "test-msg-id",
                None,
                None,
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("null"),
            "Error should mention null, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn enqueue_to_function_queue_fifo_with_group_field_ok() {
        let (_engine, module, adapter) = setup_queue_module_with_configs();
        let result = module
            .enqueue_to_function_queue(
                "payment",
                "fn-1",
                json!({"transaction_id": "txn-123", "amount": 100}),
                "test-msg-id",
                None,
                None,
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(adapter.enqueue_to_queue_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn enqueue_to_function_queue_standard_ok() {
        let (_engine, module, adapter) = setup_queue_module_with_configs();
        let result = module
            .enqueue_to_function_queue(
                "default",
                "fn-1",
                json!({"key": "value"}),
                "test-msg-id",
                None,
                None,
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(adapter.enqueue_to_queue_count.load(Ordering::SeqCst), 1);
    }

    // =========================================================================
    // Integration tests with real BuiltinQueueAdapter
    // =========================================================================

    use crate::modules::module::ConfigurableModule;

    async fn setup_integration_module() -> (
        Arc<Engine>,
        QueueCoreModule,
        Arc<dyn super::super::QueueAdapter>,
    ) {
        use super::super::config::{FunctionQueueConfig, QueueModuleConfig};

        crate::modules::observability::metrics::ensure_default_meter();

        let engine = Arc::new(Engine::new());

        let factory = QueueCoreModule::get_adapter("modules::queue::BuiltinQueueAdapter")
            .await
            .expect("BuiltinQueueAdapter factory must be registered");
        let adapter = factory(engine.clone(), None)
            .await
            .expect("BuiltinQueueAdapter creation should succeed");

        let mut queue_configs = HashMap::new();
        queue_configs.insert(
            "default".to_string(),
            FunctionQueueConfig {
                r#type: "standard".to_string(),
                concurrency: 3,
                ..Default::default()
            },
        );
        queue_configs.insert(
            "payment".to_string(),
            FunctionQueueConfig {
                r#type: "fifo".to_string(),
                message_group_field: Some("transaction_id".to_string()),
                concurrency: 1,
                ..Default::default()
            },
        );

        let config = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };

        let module = QueueCoreModule {
            adapter: adapter.clone(),
            engine: engine.clone(),
            _config: config,
        };

        (engine, module, adapter)
    }

    #[tokio::test]
    async fn integration_enqueue_consume_invoke_ack() {
        let (engine, module, adapter) = setup_integration_module().await;

        let call_count = Arc::new(AtomicU64::new(0));
        let counter = call_count.clone();

        let function = crate::function::Function {
            handler: Arc::new(move |_invocation_id, _input| {
                let counter = counter.clone();
                Box::pin(async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    FunctionResult::Success(Some(json!({ "ok": true })))
                })
            }),
            _function_id: "integration::ack_fn".to_string(),
            _description: Some("test".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        };
        engine
            .functions
            .register_function("integration::ack_fn".to_string(), function);

        // Publish a message before initializing the consumer loop
        adapter
            .publish_to_function_queue(
                "default",
                "integration::ack_fn",
                json!({"task": "do_work"}),
                "test-msg-id",
                3,
                1000,
                None,
                None,
            )
            .await;

        // Initialize starts the consumer loop
        module
            .initialize()
            .await
            .expect("initialize should succeed");

        // Wait for the consumer loop to pick up and process the message
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "Function should have been called exactly once"
        );
    }

    #[tokio::test]
    async fn integration_function_failure_nacks() {
        let (engine, module, adapter) = setup_integration_module().await;

        let call_count = Arc::new(AtomicU64::new(0));
        let counter = call_count.clone();

        let function = crate::function::Function {
            handler: Arc::new(move |_invocation_id, _input| {
                let counter = counter.clone();
                Box::pin(async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    FunctionResult::Failure(ErrorBody {
                        code: "QUEUE_FAIL".to_string(),
                        message: "job failed".to_string(),
                        stacktrace: None,
                    })
                })
            }),
            _function_id: "integration::fail_fn".to_string(),
            _description: Some("test".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        };
        engine
            .functions
            .register_function("integration::fail_fn".to_string(), function);

        adapter
            .publish_to_function_queue(
                "default",
                "integration::fail_fn",
                json!({"task": "will_fail"}),
                "test-msg-id",
                3,
                100,
                None,
                None,
            )
            .await;

        module
            .initialize()
            .await
            .expect("initialize should succeed");

        // Wait for the consumer to process the message (and potentially retries)
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let count = call_count.load(Ordering::SeqCst);
        assert!(
            count >= 1,
            "Failing function should have been called at least once, got {}",
            count
        );
    }

    #[tokio::test]
    async fn integration_fifo_ordering() {
        let (engine, module, adapter) = setup_integration_module().await;

        let invocation_order = Arc::new(Mutex::new(Vec::<String>::new()));
        let order_ref = invocation_order.clone();

        let function = crate::function::Function {
            handler: Arc::new(move |_invocation_id, input| {
                let order_ref = order_ref.clone();
                Box::pin(async move {
                    let txn_id = input
                        .get("transaction_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    order_ref.lock().await.push(txn_id);
                    FunctionResult::Success(Some(json!({ "ok": true })))
                })
            }),
            _function_id: "integration::fifo_fn".to_string(),
            _description: Some("test".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        };
        engine
            .functions
            .register_function("integration::fifo_fn".to_string(), function);

        // Enqueue 3 messages in order to the FIFO "payment" queue
        for txn_id in &["txn-001", "txn-002", "txn-003"] {
            adapter
                .publish_to_function_queue(
                    "payment",
                    "integration::fifo_fn",
                    json!({"transaction_id": txn_id, "amount": 100}),
                    "test-msg-id",
                    3,
                    1000,
                    None,
                    None,
                )
                .await;
        }

        module
            .initialize()
            .await
            .expect("initialize should succeed");

        // Wait for consumer to process all 3 messages
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

        let order = invocation_order.lock().await;
        assert_eq!(
            order.len(),
            3,
            "All 3 messages should have been processed, got {}",
            order.len()
        );
        assert_eq!(
            *order,
            vec!["txn-001", "txn-002", "txn-003"],
            "FIFO queue should process messages in order"
        );
    }

    #[tokio::test]
    async fn integration_standard_queue_concurrency() {
        let (engine, module, adapter) = setup_integration_module().await;

        let timestamps = Arc::new(Mutex::new(Vec::<(String, std::time::Instant)>::new()));
        let ts_ref = timestamps.clone();

        let function = crate::function::Function {
            handler: Arc::new(move |_invocation_id, input| {
                let ts_ref = ts_ref.clone();
                Box::pin(async move {
                    let task_id = input
                        .get("task_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let start = std::time::Instant::now();
                    // Simulate work that takes time so concurrent processing is observable
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    ts_ref.lock().await.push((task_id, start));
                    FunctionResult::Success(Some(json!({ "ok": true })))
                })
            }),
            _function_id: "integration::concurrent_fn".to_string(),
            _description: Some("test".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        };
        engine
            .functions
            .register_function("integration::concurrent_fn".to_string(), function);

        // Enqueue 3 messages to the "default" queue (concurrency=3)
        for i in 0..3 {
            adapter
                .publish_to_function_queue(
                    "default",
                    "integration::concurrent_fn",
                    json!({"task_id": format!("task-{}", i)}),
                    "test-msg-id",
                    3,
                    1000,
                    None,
                    None,
                )
                .await;
        }

        module
            .initialize()
            .await
            .expect("initialize should succeed");

        // Wait enough for all tasks to be picked up and processed concurrently
        // With concurrency=3 and 200ms sleep each, concurrent execution should
        // finish around 200-400ms; sequential would take 600ms+.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let ts = timestamps.lock().await;
        assert_eq!(
            ts.len(),
            3,
            "All 3 messages should have been processed, got {}",
            ts.len()
        );

        // Check that tasks started concurrently: the time between the earliest
        // and latest start times should be less than 200ms (the handler sleep
        // duration). If processed sequentially, the gap would be >= 200ms.
        let earliest_start = ts.iter().map(|(_, t)| *t).min().unwrap();
        let latest_start = ts.iter().map(|(_, t)| *t).max().unwrap();
        let start_gap = latest_start.duration_since(earliest_start);
        assert!(
            start_gap < std::time::Duration::from_millis(200),
            "Tasks should start concurrently. Start gap was {:?} (expected < 200ms)",
            start_gap
        );
    }

    // =========================================================================
    // resolve_queue_key tests
    // =========================================================================

    #[test]
    fn resolve_queue_key_function_queue_gets_prefixed() {
        let (_engine, module, _adapter) = setup_queue_module_with_configs();
        assert_eq!(module.resolve_queue_key("default"), "__fn_queue::default");
        assert_eq!(module.resolve_queue_key("payment"), "__fn_queue::payment");
    }

    #[test]
    fn resolve_queue_key_topic_passes_through() {
        let (_engine, module, _adapter) = setup_queue_module_with_configs();
        assert_eq!(module.resolve_queue_key("some-topic"), "some-topic");
        assert_eq!(
            module.resolve_queue_key("events.user.created"),
            "events.user.created"
        );
    }

    #[test]
    fn resolve_queue_key_empty_config_passes_through() {
        let (_engine, module, _adapter) = setup_queue_module();
        assert_eq!(module.resolve_queue_key("anything"), "anything");
    }

    // =========================================================================
    // redrive_message tests
    // =========================================================================

    #[tokio::test]
    async fn redrive_message_empty_queue_returns_failure() {
        let (_engine, module, _adapter) = setup_queue_module();
        let input = RedriveSingleInput {
            queue: "".to_string(),
            message_id: "msg-1".to_string(),
        };
        let result = module.redrive_message(input).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "queue_not_set");
            }
            _ => panic!("Expected Failure for empty queue"),
        }
    }

    #[tokio::test]
    async fn redrive_message_empty_message_id_returns_failure() {
        let (_engine, module, _adapter) = setup_queue_module();
        let input = RedriveSingleInput {
            queue: "payment".to_string(),
            message_id: "".to_string(),
        };
        let result = module.redrive_message(input).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "message_id_not_set");
            }
            _ => panic!("Expected Failure for empty message_id"),
        }
    }

    #[tokio::test]
    async fn redrive_message_success_found() {
        let (_engine, module, adapter) = setup_queue_module();
        adapter.redrive_dlq_result.store(1, Ordering::SeqCst);
        let input = RedriveSingleInput {
            queue: "payment".to_string(),
            message_id: "msg-1".to_string(),
        };
        let result = module.redrive_message(input).await;
        match result {
            FunctionResult::Success(r) => {
                assert_eq!(r.queue, "payment");
                assert_eq!(r.message_id, "msg-1");
                assert_eq!(r.redriven, 1);
            }
            _ => panic!("Expected Success"),
        }
    }

    #[tokio::test]
    async fn redrive_message_success_not_found() {
        let (_engine, module, adapter) = setup_queue_module();
        adapter.redrive_dlq_result.store(0, Ordering::SeqCst);
        let input = RedriveSingleInput {
            queue: "payment".to_string(),
            message_id: "msg-nonexistent".to_string(),
        };
        let result = module.redrive_message(input).await;
        match result {
            FunctionResult::Success(r) => {
                assert_eq!(r.redriven, 0);
            }
            _ => panic!("Expected Success with redriven=0"),
        }
    }

    #[tokio::test]
    async fn redrive_message_adapter_error() {
        let (_engine, module, adapter) = setup_queue_module();
        adapter
            .redrive_dlq_should_fail
            .store(true, Ordering::SeqCst);
        let input = RedriveSingleInput {
            queue: "payment".to_string(),
            message_id: "msg-1".to_string(),
        };
        let result = module.redrive_message(input).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "redrive_message_failed");
                assert!(e.message.contains("mock redrive message error"));
            }
            _ => panic!("Expected Failure on adapter error"),
        }
    }

    // =========================================================================
    // discard_message tests
    // =========================================================================

    #[tokio::test]
    async fn discard_message_empty_queue_returns_failure() {
        let (_engine, module, _adapter) = setup_queue_module();
        let input = RedriveSingleInput {
            queue: "".to_string(),
            message_id: "msg-1".to_string(),
        };
        let result = module.discard_message(input).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "queue_not_set");
            }
            _ => panic!("Expected Failure for empty queue"),
        }
    }

    #[tokio::test]
    async fn discard_message_empty_message_id_returns_failure() {
        let (_engine, module, _adapter) = setup_queue_module();
        let input = RedriveSingleInput {
            queue: "payment".to_string(),
            message_id: "".to_string(),
        };
        let result = module.discard_message(input).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "message_id_not_set");
            }
            _ => panic!("Expected Failure for empty message_id"),
        }
    }

    #[tokio::test]
    async fn discard_message_success() {
        let (_engine, module, _adapter) = setup_queue_module();
        let input = RedriveSingleInput {
            queue: "payment".to_string(),
            message_id: "msg-1".to_string(),
        };
        let result = module.discard_message(input).await;
        match result {
            FunctionResult::Success(r) => {
                assert_eq!(r.queue, "payment");
                assert_eq!(r.message_id, "msg-1");
                assert_eq!(r.redriven, 1); // found=true => 1
            }
            _ => panic!("Expected Success"),
        }
    }

    #[tokio::test]
    async fn discard_message_adapter_error() {
        let (_engine, module, adapter) = setup_queue_module();
        adapter.discard_should_fail.store(true, Ordering::SeqCst);
        let input = RedriveSingleInput {
            queue: "payment".to_string(),
            message_id: "msg-1".to_string(),
        };
        let result = module.discard_message(input).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "discard_message_failed");
                assert!(e.message.contains("mock discard error"));
            }
            _ => panic!("Expected Failure on adapter error"),
        }
    }

    // =========================================================================
    // console_list_topics tests
    // =========================================================================

    #[tokio::test]
    async fn console_list_topics_returns_topics() {
        let (_engine, module, adapter) = setup_queue_module_with_configs();
        *adapter.list_topics_result.lock().await = vec![TopicInfo {
            name: "events.user".to_string(),
            broker_type: "builtin".to_string(),
            subscriber_count: 2,
        }];

        let result = module.console_list_topics(json!({})).await;
        match result {
            FunctionResult::Success(Some(val)) => {
                let topics: Vec<Value> = serde_json::from_value(val).unwrap();
                // Should have: events.user + default + payment (from config)
                assert!(topics.len() >= 3);
            }
            _ => panic!("Expected Success with topics"),
        }
    }

    #[tokio::test]
    async fn console_list_topics_merges_function_queue_concurrency() {
        let (_engine, module, adapter) = setup_queue_module_with_configs();
        // Adapter reports the function queue with subscriber_count=0
        *adapter.list_topics_result.lock().await = vec![TopicInfo {
            name: "__fn_queue::default".to_string(),
            broker_type: "builtin".to_string(),
            subscriber_count: 0,
        }];

        let result = module.console_list_topics(json!({})).await;
        match result {
            FunctionResult::Success(Some(val)) => {
                let topics: Vec<Value> = serde_json::from_value(val).unwrap();
                // "default" should have concurrency from config (10 is default)
                let default_topic = topics.iter().find(|t| t["name"] == "default");
                assert!(default_topic.is_some(), "default topic should exist");
                let sub_count = default_topic.unwrap()["subscriber_count"].as_u64().unwrap();
                assert!(
                    sub_count > 0,
                    "subscriber_count should be overridden from config"
                );
            }
            _ => panic!("Expected Success"),
        }
    }

    #[tokio::test]
    async fn console_list_topics_strips_fn_queue_prefix() {
        let (_engine, module, adapter) = setup_queue_module_with_configs();
        *adapter.list_topics_result.lock().await = vec![TopicInfo {
            name: "__fn_queue::default".to_string(),
            broker_type: "builtin".to_string(),
            subscriber_count: 0,
        }];

        let result = module.console_list_topics(json!({})).await;
        match result {
            FunctionResult::Success(Some(val)) => {
                let topics: Vec<Value> = serde_json::from_value(val).unwrap();
                // No topic should have __fn_queue:: prefix
                for topic in &topics {
                    let name = topic["name"].as_str().unwrap();
                    assert!(
                        !name.starts_with("__fn_queue::"),
                        "Topic name should not have __fn_queue:: prefix: {}",
                        name
                    );
                }
            }
            _ => panic!("Expected Success"),
        }
    }

    #[tokio::test]
    async fn console_list_topics_adapter_error() {
        let (_engine, module, adapter) = setup_queue_module();
        adapter
            .list_topics_should_fail
            .store(true, Ordering::SeqCst);

        let result = module.console_list_topics(json!({})).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "list_topics_failed");
            }
            _ => panic!("Expected Failure on adapter error"),
        }
    }

    // =========================================================================
    // console_topic_stats tests
    // =========================================================================

    #[tokio::test]
    async fn console_topic_stats_empty_topic_returns_failure() {
        let (_engine, module, _adapter) = setup_queue_module();
        let result = module.console_topic_stats(json!({})).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "topic_required");
            }
            _ => panic!("Expected Failure for missing topic"),
        }
    }

    #[tokio::test]
    async fn console_topic_stats_returns_stats() {
        let (_engine, module, adapter) = setup_queue_module_with_configs();
        *adapter.topic_stats_result.lock().await = super::super::TopicStats {
            depth: 5,
            consumer_count: 0,
            dlq_depth: 2,
            config: None,
        };

        let result = module
            .console_topic_stats(json!({"topic": "default"}))
            .await;
        match result {
            FunctionResult::Success(Some(val)) => {
                assert_eq!(val["depth"], 5);
                assert_eq!(val["dlq_depth"], 2);
                // consumer_count should be overridden with config concurrency
                let consumer_count = val["consumer_count"].as_u64().unwrap();
                assert!(
                    consumer_count > 0,
                    "consumer_count should be overridden from config"
                );
            }
            _ => panic!("Expected Success with stats"),
        }
    }

    #[tokio::test]
    async fn console_topic_stats_adapter_error() {
        let (_engine, module, adapter) = setup_queue_module();
        adapter
            .topic_stats_should_fail
            .store(true, Ordering::SeqCst);

        let result = module
            .console_topic_stats(json!({"topic": "some-topic"}))
            .await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "topic_stats_failed");
            }
            _ => panic!("Expected Failure on adapter error"),
        }
    }

    // =========================================================================
    // console_dlq_topics tests
    // =========================================================================

    #[tokio::test]
    async fn console_dlq_topics_returns_topics_with_counts() {
        let (_engine, module, adapter) = setup_queue_module_with_configs();
        *adapter.list_topics_result.lock().await = vec![TopicInfo {
            name: "events.user".to_string(),
            broker_type: "builtin".to_string(),
            subscriber_count: 1,
        }];
        adapter.dlq_count_value.store(3, Ordering::SeqCst);

        let result = module.console_dlq_topics(json!({})).await;
        match result {
            FunctionResult::Success(Some(val)) => {
                let topics: Vec<Value> = serde_json::from_value(val).unwrap();
                assert!(!topics.is_empty());
                // Should include event topics + function queue topics
                let has_events = topics.iter().any(|t| t["topic"] == "events.user");
                assert!(has_events, "Should include events.user topic");
            }
            _ => panic!("Expected Success with DLQ topics"),
        }
    }

    #[tokio::test]
    async fn console_dlq_topics_adapter_error() {
        let (_engine, module, adapter) = setup_queue_module();
        adapter
            .list_topics_should_fail
            .store(true, Ordering::SeqCst);

        let result = module.console_dlq_topics(json!({})).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "dlq_topics_failed");
            }
            _ => panic!("Expected Failure on adapter error"),
        }
    }

    // =========================================================================
    // console_dlq_messages tests
    // =========================================================================

    #[tokio::test]
    async fn console_dlq_messages_empty_topic_returns_failure() {
        let (_engine, module, _adapter) = setup_queue_module();
        let result = module.console_dlq_messages(json!({})).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "topic_required");
            }
            _ => panic!("Expected Failure for missing topic"),
        }
    }

    #[tokio::test]
    async fn console_dlq_messages_returns_messages() {
        let (_engine, module, adapter) = setup_queue_module();
        *adapter.dlq_peek_result.lock().await = vec![super::super::DlqMessage {
            id: "msg-1".to_string(),
            payload: json!({"key": "value"}),
            error: "test error".to_string(),
            failed_at: 1234567890,
            retries: 3,
            size_bytes: 128,
        }];

        let result = module
            .console_dlq_messages(json!({"topic": "my-queue", "offset": 0, "limit": 50}))
            .await;
        match result {
            FunctionResult::Success(Some(val)) => {
                let messages: Vec<Value> = serde_json::from_value(val).unwrap();
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0]["id"], "msg-1");
                assert_eq!(messages[0]["error"], "test error");
            }
            _ => panic!("Expected Success with messages"),
        }
    }

    #[tokio::test]
    async fn console_dlq_messages_uses_resolve_queue_key() {
        // When topic is a function queue name, it should be resolved
        let (_engine, module, adapter) = setup_queue_module_with_configs();
        *adapter.dlq_peek_result.lock().await = vec![];

        let result = module
            .console_dlq_messages(json!({"topic": "default"}))
            .await;
        // Should succeed (even with empty results)
        assert!(matches!(result, FunctionResult::Success(_)));
    }

    #[tokio::test]
    async fn console_dlq_messages_adapter_error() {
        let (_engine, module, adapter) = setup_queue_module();
        adapter.dlq_peek_should_fail.store(true, Ordering::SeqCst);

        let result = module
            .console_dlq_messages(json!({"topic": "my-queue"}))
            .await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "dlq_messages_failed");
            }
            _ => panic!("Expected Failure on adapter error"),
        }
    }

    #[tokio::test]
    async fn console_dlq_messages_default_offset_and_limit() {
        let (_engine, module, adapter) = setup_queue_module();
        *adapter.dlq_peek_result.lock().await = vec![];

        // Only provide topic, offset and limit should default
        let result = module
            .console_dlq_messages(json!({"topic": "my-queue"}))
            .await;
        assert!(matches!(result, FunctionResult::Success(_)));
    }

    // =========================================================================
    // redrive uses resolve_queue_key
    // =========================================================================

    #[tokio::test]
    async fn redrive_uses_resolve_queue_key_for_function_queues() {
        let (_engine, module, adapter) = setup_queue_module_with_configs();
        adapter.redrive_dlq_result.store(2, Ordering::SeqCst);

        let input = RedriveInput {
            queue: "default".to_string(),
        };
        let result = module.redrive(input).await;
        match result {
            FunctionResult::Success(r) => {
                assert_eq!(r.queue, "default");
                assert_eq!(r.redriven, 2);
            }
            _ => panic!("Expected Success"),
        }
    }
}
