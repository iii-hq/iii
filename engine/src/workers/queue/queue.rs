// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    pin::Pin,
    sync::{
        Arc, Mutex as StdMutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
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
use tokio::task::AbortHandle;

use super::{
    QueueAdapter, SubscriberQueueConfig, TopicInfo,
    config::{FunctionQueueConfig, QueueModuleConfig},
};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    protocol::ErrorBody,
    telemetry::{SpanExt, inject_baggage_from_context, inject_traceparent_from_context},
    trigger::{Trigger, TriggerRegistrator, TriggerType},
    workers::traits::{AdapterFactory, ConfigurableWorker, Worker},
};

/// The live config + transport, swapped together under one lock so every
/// reader observes a coherent generation. Combining them is what prevents a
/// runtime adapter hot-swap from exposing a new-config + old-adapter window to
/// a concurrent enqueue (which would publish through the dying transport with
/// the new per-queue settings). The inner `Arc`s make a snapshot a cheap clone.
#[derive(Clone)]
struct LiveState {
    config: Arc<QueueModuleConfig>,
    adapter: Arc<dyn QueueAdapter>,
}

#[derive(Clone)]
pub struct QueueWorker {
    engine: Arc<Engine>,
    /// Live config + transport behind a single lock. The outer lock is held
    /// only for pointer swaps; readers clone the inner `Arc`(s) once per call
    /// via `config_snapshot()` / `adapter_snapshot()` / `live_snapshot()`.
    /// Both are hot-swappable from the configuration worker, and `apply_config`
    /// swaps the pair atomically so no reader sees a half-applied transport
    /// change.
    live: Arc<RwLock<LiveState>>,
    /// The config.yaml block passed to `build()` (post-`ensure_default_queue`).
    /// Used only as the seed for the first `configuration::register`; the
    /// configuration worker entry is the runtime source of truth afterwards.
    seed: Option<QueueModuleConfig>,
    /// One live consumer task per queue name, so an individual consumer can be
    /// restarted on a config change without disturbing the others.
    consumers: Arc<StdMutex<HashMap<String, AbortHandle>>>,
    /// Serializes concurrent `apply_config` runs (rapid configuration edits).
    apply_lock: Arc<tokio::sync::Mutex<()>>,
    /// Set by `destroy()` (under `apply_lock`). A late `apply_config` — from an
    /// in-flight trigger event or the detached one-shot retry in
    /// `on_config_change` — checks this after acquiring the lock and bails, so a
    /// torn-down worker can never re-spawn consumers. The queue analogue of
    /// `HttpWorker`'s `shutdown_rx` None-guard.
    destroyed: Arc<AtomicBool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct QueueInput {
    /// Topic to publish to. Subscribers registered for this topic receive the message.
    topic: String,
    /// JSON payload delivered to each subscriber.
    data: Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct RedriveInput {
    /// Queue or topic whose dead-letter messages should be moved back to the main
    /// queue. For a durable topic this is the topic name.
    #[serde(alias = "topic")]
    queue: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RedriveResult {
    pub queue: String,
    pub redriven: u64,
}

#[derive(Deserialize, JsonSchema)]
pub struct RedriveSingleInput {
    /// Queue or topic owning the dead-letter message. For a durable topic this is
    /// the topic name.
    #[serde(alias = "topic")]
    queue: String,
    /// Identifier of the dead-letter message to redrive (from
    /// `engine::queue::dlq_messages`).
    message_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RedriveSingleResult {
    pub queue: String,
    pub message_id: String,
    pub redriven: u64,
}

#[derive(Deserialize, JsonSchema)]
pub struct TopicStatsInput {
    /// Topic or named queue to inspect. For a durable topic this is the topic name.
    #[serde(alias = "queue")]
    topic: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct DlqMessagesInput {
    /// Topic or named queue whose dead-letter messages to browse. For a durable
    /// topic this is the topic name.
    #[serde(alias = "queue")]
    topic: String,
    /// Number of messages to skip (default 0).
    #[serde(default)]
    offset: u64,
    /// Maximum number of messages to return (default 50).
    #[serde(default = "default_dlq_limit")]
    limit: u64,
}

fn default_dlq_limit() -> u64 {
    50
}

#[service(name = "queue")]
impl QueueWorker {
    /// Resolves a display-friendly queue name to the internal adapter key.
    /// Function queues (listed in config) are prefixed with `__fn_queue::`;
    /// topic-based queues pass through unchanged.
    fn resolve_queue_key(&self, name: &str) -> String {
        if self.config_snapshot().queue_configs.contains_key(name) {
            format!("__fn_queue::{}", name)
        } else {
            name.to_string()
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn enqueue_to_function_queue(
        &self,
        queue_name: &str,
        function_id: &str,
        data: Value,
        message_id: &str,
        namespace: Option<String>,
        traceparent: Option<String>,
        baggage: Option<String>,
    ) -> anyhow::Result<()> {
        // One coherent snapshot of config + transport: the per-queue settings
        // and the adapter we publish through must come from the same generation
        // so a concurrent adapter hot-swap can't make us publish through the old
        // transport with the new config (or vice versa).
        let live = self.live_snapshot();
        let queue_config = live.config.queue_configs.get(queue_name).ok_or_else(|| {
            tracing::warn!(
                queue_name = %queue_name,
                available = ?live.config.queue_configs.keys().collect::<Vec<_>>(),
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

        // Resolve the per-message priority from the configured `priority_field`
        // (mirrors the FIFO `message_group_field` extraction above). Clamp to the
        // queue's `max_priority`; the value is honored only by priority queues
        // (RabbitMQ declared with `x-max-priority`) and ignored by other adapters.
        let priority = queue_config.priority_field.as_ref().and_then(|field| {
            let raw = data.get(field)?.as_u64()?;
            let max = queue_config.max_priority.unwrap_or(u8::MAX) as u64;
            Some(raw.min(max) as u8)
        });

        // Name the function this message will run in the propagated baggage, so
        // the `fn_queue` consumer span (and the worker context it delivers) carry
        // `iii.function.id = <target>` rather than the enqueuer's id that
        // `BaggageSpanProcessor` would otherwise stamp.
        let baggage = crate::telemetry::baggage_with_function_id(baggage.as_deref(), function_id);

        live.adapter
            .publish_to_function_queue(
                queue_name,
                function_id,
                data,
                message_id,
                queue_config.max_retries,
                queue_config.backoff_ms,
                traceparent,
                baggage,
                namespace,
                priority,
            )
            .await;
        crate::workers::telemetry::collector::track_queue_emit();
        Ok(())
    }

    /// Returns the number of messages in the DLQ for a function queue.
    pub async fn function_queue_dlq_count(&self, queue_name: &str) -> anyhow::Result<u64> {
        let namespaced = format!("__fn_queue::{}", queue_name);
        self.adapter_snapshot().dlq_count(&namespaced).await
    }

    /// Returns up to `count` DLQ messages for a function queue as parsed JSON Values.
    pub async fn function_queue_dlq_messages(
        &self,
        queue_name: &str,
        count: usize,
    ) -> anyhow::Result<Vec<Value>> {
        let namespaced = format!("__fn_queue::{}", queue_name);
        self.adapter_snapshot()
            .dlq_messages(&namespaced, count)
            .await
    }

    #[function(id = "iii::durable::publish", description = "Enqueue a message")]
    pub async fn enqueue(&self, input: QueueInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let adapter = self.adapter_snapshot();
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
        crate::workers::telemetry::collector::track_queue_emit();

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
        match self.adapter_snapshot().redrive_dlq(&resolved).await {
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
            .adapter_snapshot()
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
            .adapter_snapshot()
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
        match self.adapter_snapshot().list_topics().await {
            Ok(topics) => {
                // Merge function queue topics from config.
                // Adapters report these with subscriber_count=0 because internal
                // consumers aren't tracked in the subscription map. Override with
                // the configured concurrency so the console shows the real count.
                let mut all_topics = topics;
                for (name, config) in &self.config_snapshot().queue_configs {
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
        input: TopicStatsInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let topic = input.topic;
        if topic.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "topic_required".into(),
                message: "topic is required".into(),
                stacktrace: None,
            });
        }

        let resolved = self.resolve_queue_key(&topic);
        match self.adapter_snapshot().topic_stats(&resolved).await {
            Ok(mut stats) => {
                // Adapters report consumer_count=0 for function queues because
                // internal consumers aren't tracked in the subscription map.
                // Override with the configured concurrency.
                if let Some(config) = self.config_snapshot().queue_configs.get(&topic) {
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
        let adapter = self.adapter_snapshot();
        match adapter.list_topics().await {
            Ok(topics) => {
                let mut dlq_topics = Vec::new();
                for topic in &topics {
                    let dlq_count = adapter.dlq_count(&topic.name).await.unwrap_or(0);
                    dlq_topics.push(json!({
                        "topic": topic.name,
                        "broker_type": topic.broker_type,
                        "message_count": dlq_count,
                    }));
                }
                // Also include function queue DLQs
                for name in self.config_snapshot().queue_configs.keys() {
                    let namespaced = format!("__fn_queue::{}", name);
                    let dlq_count = adapter.dlq_count(&namespaced).await.unwrap_or(0);
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
        input: DlqMessagesInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let topic = input.topic;
        if topic.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "topic_required".into(),
                message: "topic is required".into(),
                stacktrace: None,
            });
        }
        let offset = input.offset;
        let limit = input.limit;

        let resolved = self.resolve_queue_key(&topic);
        match self
            .adapter_snapshot()
            .dlq_peek(&resolved, offset, limit)
            .await
        {
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

impl TriggerRegistrator for QueueWorker {
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
        let adapter = self.adapter_snapshot();

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
        let adapter = self.adapter_snapshot();

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

impl QueueWorker {
    /// Coherent snapshot of the live config + transport in a single lock
    /// acquisition. Use this when one operation needs both (e.g. enqueue reads
    /// the per-queue config AND publishes through the adapter) so it can never
    /// observe a half-applied hot-swap.
    fn live_snapshot(&self) -> LiveState {
        self.live.read().expect("live lock poisoned").clone()
    }

    /// Cheap clone of the live config. Take one snapshot per call so all reads
    /// within it are consistent.
    pub fn config_snapshot(&self) -> Arc<QueueModuleConfig> {
        self.live.read().expect("live lock poisoned").config.clone()
    }

    fn set_config(&self, config: QueueModuleConfig) {
        self.live.write().expect("live lock poisoned").config = Arc::new(config);
    }

    /// Cheap clone of the live transport. Take one snapshot per call.
    /// `pub` (mirroring `config_snapshot`) so integration tests can confirm a
    /// hot-swap actually rebuilt the adapter instance.
    pub fn adapter_snapshot(&self) -> Arc<dyn QueueAdapter> {
        self.live
            .read()
            .expect("live lock poisoned")
            .adapter
            .clone()
    }

    /// Atomically swap both the config and the transport. Used by the adapter
    /// hot-swap path so a concurrent reader sees either the old (config,
    /// adapter) pair or the new one — never a mix.
    fn set_live(&self, config: QueueModuleConfig, adapter: Arc<dyn QueueAdapter>) {
        let mut guard = self.live.write().expect("live lock poisoned");
        guard.config = Arc::new(config);
        guard.adapter = adapter;
    }

    /// Resolve and instantiate the adapter named by `config` (or the default).
    /// Mirrors steps 3–4 of `ConfigurableWorker::create_with_adapters`, reused
    /// for test construction and for runtime adapter hot-swaps.
    async fn resolve_adapter(
        engine: &Arc<Engine>,
        config: &QueueModuleConfig,
    ) -> anyhow::Result<Arc<dyn QueueAdapter>> {
        let adapter_name = Self::adapter_name_from_config(config)
            .unwrap_or_else(|| Self::DEFAULT_ADAPTER_NAME.to_string());
        let factory = Self::get_adapter(&adapter_name)
            .await
            .ok_or_else(|| anyhow::anyhow!("queue adapter '{adapter_name}' is not registered"))?;
        let adapter_config = Self::adapter_config_from_config(config);
        factory(engine.clone(), adapter_config).await
    }

    /// Effective `(name, config)` of the adapter selected by `config`, with the
    /// default name filled in, so `None` vs `Some(builtin)` is not a false
    /// change when comparing two configs.
    fn effective_adapter(config: &QueueModuleConfig) -> (String, Option<Value>) {
        (
            Self::adapter_name_from_config(config)
                .unwrap_or_else(|| Self::DEFAULT_ADAPTER_NAME.to_string()),
            Self::adapter_config_from_config(config),
        )
    }

    /// Construct a worker from a raw config value — mirrors `HttpWorker::for_test`
    /// so integration tests in `engine/tests/` can drive the concrete worker
    /// without booting the full engine. Async because the queue resolves its
    /// adapter from the registry.
    #[doc(hidden)]
    pub async fn for_test(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Self> {
        let parsed: QueueModuleConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let adapter = Self::resolve_adapter(&engine, &parsed).await?;
        Ok(Self::build(engine, parsed, adapter))
    }

    /// Register the `iii-queue::on-config-change` handler. Idempotent
    /// (replace-by-id), so it is safe to call from both `register_functions`
    /// (which runs inside the worker scope for destroy/reload cleanup) and
    /// `start_background_tasks` (which registers the trigger and runs first).
    fn register_config_handler(&self, engine: &Arc<Engine>) {
        let worker = self.clone();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: super::configuration::CONFIG_FN_ID.to_string(),
                description: Some(
                    "Internal: re-apply the iii-queue configuration when the \
                     authoritative configuration entry changes."
                        .to_string(),
                ),
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |_payload: Value| {
                let worker = worker.clone();
                async move {
                    super::configuration::on_config_change(&worker).await;
                    FunctionResult::Success(Some(json!({ "ok": true })))
                }
            }),
        );
    }

    /// Set up and start (or restart) the consumer for one queue. Aborts any
    /// prior consumer for `name` only after the new one is live, so a restart
    /// never strands the queue. Every fallible adapter call happens BEFORE the
    /// `consumers` map is mutated, so a failed (re)start leaves the previous
    /// consumer running — the property that makes the runtime apply best-effort
    /// safe.
    async fn spawn_consumer(&self, name: &str, config: &FunctionQueueConfig) -> anyhow::Result<()> {
        let adapter = self.adapter_snapshot();
        adapter.setup_function_queue(name, config).await?;

        let prefetch = if config.r#type == "fifo" {
            1
        } else {
            config.concurrency
        };
        let max_retries = config.max_retries;
        let mut receiver = adapter.consume_function_queue(name, prefetch).await?;

        let engine = self.engine.clone();
        let queue_name = name.to_string();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(prefetch as usize));

        let handle = tokio::spawn(async move {
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
                // Publisher-scope relevance tags must not leak into the
                // delivery's scope — see `scrub_relevance_tags`.
                let baggage = msg.baggage.as_deref().and_then(scrub_relevance_tags);

                tokio::spawn(async move {
                    let delivery_id = msg.delivery_id;
                    let function_id = msg.function_id.clone();
                    let attempt = msg.attempt;
                    // Resolve the target in the namespace captured at enqueue.
                    // Absent (pre-namespace messages) falls back to `default`.
                    let namespace = msg
                        .namespace
                        .clone()
                        .unwrap_or_else(|| crate::protocol::DEFAULT_NAMESPACE.to_string());

                    // `iii.tag.*` is the console timeline's relevant-span
                    // convention (workers/console/docs/timeline-span-tags.md):
                    // `queue.process` marks this span as THE function-trigger
                    // span of a queue dispatch, and the display name renders
                    // it as `<function> (<queue>)` instead of the raw
                    // `fn_queue <queue>` span name. Stamped directly as span
                    // attributes — no baggage involved, so nothing here leaks
                    // onto child spans.
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
                        "iii.tag.kind" = "queue.process",
                        "iii.tag.display_name" = %format!("queue({}) {}", queue_name, function_id),
                        otel.status_code = tracing::field::Empty,
                    )
                    .with_parent_headers(
                        traceparent.as_deref(),
                        None,
                        baggage.as_deref(),
                    );

                    let result = AssertUnwindSafe(async {
                        engine
                            .call_with_metadata_ns(&namespace, &function_id, msg.data, None)
                            .await
                    })
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
                                .nack_function_queue(&queue_name, delivery_id, attempt, max_retries)
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
                                .nack_function_queue(&queue_name, delivery_id, attempt, max_retries)
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

        // Insert the new handle, then abort any prior one for this name. The
        // fallible work above already succeeded, so the swap cannot strand the
        // queue without a consumer.
        let previous = self
            .consumers
            .lock()
            .expect("consumers mutex poisoned")
            .insert(name.to_string(), handle.abort_handle());
        if let Some(previous) = previous {
            previous.abort();
        }

        tracing::info!(
            queue = %name,
            r#type = %config.r#type,
            concurrency = %config.concurrency,
            "Started function queue consumer"
        );
        Ok(())
    }

    /// Re-fetch the authoritative configuration and hot-apply it. The
    /// authoritative read happens under `apply_lock` so overlapping
    /// configuration events can't apply a stale value last (lost update).
    ///
    /// The fetch+validate gate is strict all-or-nothing: any failure keeps the
    /// previous config, adapter, and every consumer. Past the gate, an adapter
    /// change re-instantiates the transport and restarts every consumer on it;
    /// otherwise the `queue_configs` delta is applied per queue, best-effort —
    /// a single queue's restart failure is logged while the others apply and
    /// that queue keeps its previous consumer.
    pub(super) async fn apply_config(&self) -> anyhow::Result<()> {
        let _guard = self.apply_lock.lock().await;

        // Bail if the worker was torn down. `destroy()` sets this under the same
        // lock, so a late apply (in-flight trigger event, or the detached retry
        // in `on_config_change`) can never re-spawn consumers post-destroy.
        if self.destroyed.load(Ordering::SeqCst) {
            return Ok(());
        }

        // GATE: fetch under the lock; keep the `Elapsed` error downcastable so
        // `on_config_change` schedules its one-shot retry for timeouts only.
        let new_config = match tokio::time::timeout(
            super::configuration::CONFIG_BUS_TIMEOUT,
            super::configuration::fetch_config(self.engine.as_ref(), &self.config_snapshot()),
        )
        .await
        {
            Ok(result) => result?,
            Err(elapsed) => {
                return Err(anyhow::Error::new(elapsed)
                    .context("configuration::get timed out; keeping previous config"));
            }
        };
        new_config.validate()?;

        let old = self.config_snapshot();

        if Self::effective_adapter(&old) != Self::effective_adapter(&new_config) {
            // FULL TRANSPORT HOT-SWAP. Build the new adapter first (fallible); a
            // failure keeps the previous config, adapter, and consumers. Then
            // restart every consumer on the new transport.
            let new_adapter = Self::resolve_adapter(&self.engine, &new_config).await?;
            // Swap config + adapter atomically so a concurrent enqueue never
            // observes the new config paired with the old (dying) transport.
            self.set_live(new_config.clone(), new_adapter);
            let previous: Vec<AbortHandle> = self
                .consumers
                .lock()
                .expect("consumers mutex poisoned")
                .drain()
                .map(|(_, handle)| handle)
                .collect();
            for handle in previous {
                handle.abort();
            }
            // Data-loss boundary (accepted tradeoff for a transport hot-swap):
            // the new adapter starts empty, so messages still queued in the old
            // adapter do NOT migrate — plus in-flight per-message tasks finish
            // acking against the OLD adapter Arc they captured. The old adapter
            // is dropped once its last ref is gone. Re-spawn is best-effort per
            // queue: a queue whose `spawn_consumer` fails here is left with NO
            // consumer (unlike the same-adapter path, which retains the prior
            // one) because the transport it was bound to no longer exists.
            for (name, queue_config) in &new_config.queue_configs {
                if let Err(err) = self.spawn_consumer(name, queue_config).await {
                    tracing::error!(
                        queue = %name,
                        error = %err,
                        "iii-queue: failed to start consumer on the new adapter; queue left unconsumed"
                    );
                }
            }
            tracing::info!("iii-queue transport hot-swapped after configuration change");
            return Ok(());
        }

        // Same adapter: diff `queue_configs` and restart only what changed.
        // Publish the new snapshot first so restarted consumers and the enqueue
        // path read the new settings.
        self.set_config(new_config.clone());

        // Removed: present in old, absent from new. `default` is always present
        // in `new_config` (ensure_default_queue runs in fetch_config), so it can
        // never land in this branch.
        for name in old.queue_configs.keys() {
            if !new_config.queue_configs.contains_key(name) {
                if let Some(handle) = self
                    .consumers
                    .lock()
                    .expect("consumers mutex poisoned")
                    .remove(name)
                {
                    handle.abort();
                }
                tracing::info!(queue = %name, "Stopped consumer for removed queue");
            }
        }

        // Added or changed: (re)start the consumer (best-effort per queue).
        for (name, queue_config) in &new_config.queue_configs {
            let unchanged = old
                .queue_configs
                .get(name)
                .is_some_and(|prev| prev == queue_config);
            if unchanged {
                continue;
            }
            if let Err(err) = self.spawn_consumer(name, queue_config).await {
                tracing::error!(
                    queue = %name,
                    error = %err,
                    "iii-queue: failed to (re)start consumer; keeping previous consumer for this queue"
                );
            }
        }

        Ok(())
    }
}

/// Strip the timeline relevance tags (`iii.tag.kind`, `iii.tag.display_name`,
/// `iii.tag.hidden`) from an incoming W3C baggage header before attaching it
/// to a queue job's span context. The publisher's baggage necessarily carries
/// its OWN scope's tags (a turn enqueuing its next step is inside
/// `iii.tag.kind=harness.turn`, and the enqueue call site itself may ride an
/// `iii.tag.hidden` scope — harness `turn enqueue`), and a delivery starts a
/// NEW logical scope: the `fn_queue` span stamps its own `queue.process`
/// identity directly, and the consumed function re-stamps its own tags.
/// Without the scrub, a baggage-materializing span processor copies the stale
/// publisher tags onto this span (duplicate keys next to the direct ones) and
/// onto every span under it — for `iii.tag.hidden` that would mark the entire
/// next delivery subtree as internal and trace UIs would hide the whole step.
/// Identity/lineage keys (`iii.session.id`, `iii.message.id`,
/// `iii.tag.message`, …) pass through untouched. Returns `None` when nothing
/// is left.
fn scrub_relevance_tags(header: &str) -> Option<String> {
    const SCRUBBED: [&str; 3] = ["iii.tag.kind", "iii.tag.display_name", "iii.tag.hidden"];
    let kept: Vec<&str> = header
        .split(',')
        .filter(|entry| {
            let key = entry.split(['=', ';']).next().unwrap_or("").trim();
            !SCRUBBED.contains(&key)
        })
        .collect();
    if kept.is_empty() {
        None
    } else {
        Some(kept.join(","))
    }
}

#[async_trait]
impl Worker for QueueWorker {
    fn name(&self) -> &'static str {
        "QueueModule"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        // The `#[service]`-generated inherent method registers every queue
        // function; the config handler is registered alongside it (inside the
        // worker scope) so destroy/reload track and remove it. The hook order
        // differs by pipeline: initial boot runs `register_functions` BEFORE
        // `start_background_tasks`, reload runs it AFTER — so
        // `start_background_tasks` also registers the handler (if absent) before
        // subscribing to configuration events.
        self.register_functions(engine.clone());
        self.register_config_handler(&engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing QueueModule");
        self.config_snapshot().validate()?;

        let trigger_type = TriggerType::new(
            "durable:subscriber",
            "Queue core module",
            Box::new(self.clone()),
            None,
        );

        let _ = self.engine.register_trigger_type(trigger_type).await;

        Ok(())
    }

    async fn start_background_tasks(
        &self,
        _shutdown_rx: tokio::sync::watch::Receiver<bool>,
        _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        // Adopt the configuration worker as the runtime source of truth.
        // `configuration::*` is callable on both pipelines (initial boot
        // registers all worker functions before serving; reload starts the
        // mandatory configuration worker before optional ones). Failures
        // degrade to the static config.yaml block so the queue stays up. Both
        // bus calls are time-bounded: worker startup is awaited serially by the
        // boot and reload pipelines, so a hung `configuration::*` provider must
        // not wedge every other worker behind this one.
        let register = tokio::time::timeout(
            super::configuration::CONFIG_BUS_TIMEOUT,
            super::configuration::register_config(self.engine.as_ref(), self.seed.as_ref()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("configuration::register timed out"))
        .and_then(|result| result);
        if let Err(err) = register {
            tracing::warn!(
                error = %err,
                "iii-queue: configuration::register failed; continuing with static config"
            );
        }
        let fetched = tokio::time::timeout(
            super::configuration::CONFIG_BUS_TIMEOUT,
            super::configuration::fetch_config(self.engine.as_ref(), &self.config_snapshot()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("configuration::get timed out"))
        .and_then(|result| result);
        match fetched {
            // Validate before adopting. The configuration worker's JSON schema
            // can't express cross-field rules (a `fifo` queue requires
            // `message_group_field`), so a value that `apply_config` rejects at
            // runtime can still be stored and read back here. Without this gate
            // a bad stored value would boot a misconfigured consumer on the next
            // restart. On failure keep the seed (already validated in
            // `initialize`), matching the runtime apply gate.
            Ok(config) => match config.validate() {
                Ok(()) => self.set_config(config),
                Err(err) => tracing::warn!(
                    error = %err,
                    "iii-queue: stored configuration is invalid; continuing with static config"
                ),
            },
            Err(err) => tracing::warn!(
                error = %err,
                "iii-queue: failed to read configuration; continuing with static config"
            ),
        }

        // Start a consumer per queue from the (possibly updated) snapshot. The
        // initial spawn is fatal: a queue that cannot be set up at boot fails
        // the worker, preserving the pre-migration behavior. Runtime restarts
        // in `apply_config` are best-effort by contrast.
        let config = self.config_snapshot();
        for (name, queue_config) in &config.queue_configs {
            self.spawn_consumer(name, queue_config).await?;
        }

        // Register the handler before the trigger so a configuration event can
        // never fan out to a missing function. On reload, `register_functions`
        // runs after this hook and re-registers the handler inside the worker
        // scope; the `get` check keeps the initial-boot path (where it already
        // ran) from logging a spurious overwrite.
        if self
            .engine
            .functions
            .get(
                crate::protocol::DEFAULT_NAMESPACE,
                super::configuration::CONFIG_FN_ID,
            )
            .is_none()
        {
            self.register_config_handler(&self.engine);
        }
        if let Err(err) = super::configuration::register_config_trigger(&self.engine).await {
            tracing::warn!(
                error = %err,
                "iii-queue: failed to watch configuration changes; hot-reload disabled"
            );
        } else {
            // Catch-up pass: replay any `configuration::set` that landed between
            // the boot fetch above and the trigger subscription — without it
            // that window's updates would be dropped until the next edit. Routed
            // through `on_config_change` (not `apply_config` directly) so a
            // timed-out catch-up gets the same one-shot delayed retry as a
            // trigger-driven apply.
            super::configuration::on_config_change(self).await;
        }

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying QueueModule");
        // The trigger is registered outside the worker scope, so remove it
        // explicitly to keep ReloadManager restarts duplicate-free.
        let _ = self
            .engine
            .trigger_registry
            .unregister_trigger(
                super::configuration::CONFIG_TRIGGER_ID.to_string(),
                Some(super::configuration::CONFIG_TRIGGER_TYPE.to_string()),
            )
            .await;

        // Serialize with any in-flight `apply_config` so a restart can't spawn a
        // replacement consumer after we abort below. Setting `destroyed` under
        // the lock makes every later apply bail before re-spawning.
        let _guard = self.apply_lock.lock().await;
        self.destroyed.store(true, Ordering::SeqCst);
        let aborts: Vec<AbortHandle> = self
            .consumers
            .lock()
            .expect("consumers mutex poisoned")
            .drain()
            .map(|(_, handle)| handle)
            .collect();
        for abort in aborts {
            abort.abort();
        }
        Ok(())
    }
}

#[async_trait]
impl ConfigurableWorker for QueueWorker {
    type Config = QueueModuleConfig;
    type Adapter = dyn QueueAdapter;
    type AdapterRegistration = super::registry::QueueAdapterRegistration;
    const DEFAULT_ADAPTER_NAME: &'static str = "builtin";

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn QueueAdapter>>>> =
            Lazy::new(|| RwLock::new(QueueWorker::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, mut config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        // Provision the built-in `default` queue so enqueues to it work with no
        // user config. Done here (the single construction site) so both the
        // consumer spawn in `start_background_tasks` and the lookup in
        // `enqueue_to_function_queue` see it.
        config.ensure_default_queue();
        // The config.yaml block (with the default queue provisioned) seeds the
        // first `configuration::register`; the configuration entry is the
        // runtime source of truth afterwards.
        let seed = Some(config.clone());
        Self {
            engine,
            live: Arc::new(RwLock::new(LiveState {
                config: Arc::new(config),
                adapter,
            })),
            seed,
            consumers: Arc::new(StdMutex::new(HashMap::new())),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
            destroyed: Arc::new(AtomicBool::new(false)),
        }
    }

    fn adapter_name_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.name.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

// Opt-in only: the standalone `queue` worker owns queue runtime behavior by
// default (see MOT-3944). This registration exists so an explicit `iii-queue`
// entry in config.yaml (written by `iii worker add iii-queue`) still boots the
// in-engine implementation instead of being silently ignored.
crate::register_worker!(
    "iii-queue",
    QueueWorker,
    description = "Async job processing with named queues, retries, and dead-letter support.",
    enabled_by_default = false
);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // =========================================================================
    // Baggage relevance-tag scrub
    // =========================================================================

    #[test]
    fn scrub_relevance_tags_drops_only_the_relevance_keys() {
        let header = "iii.session.id=S-1,iii.tag.kind=harness.turn,\
                      iii.tag.message=fix%20login,iii.tag.display_name=Sub-agent%20x,\
                      iii.message.id=M-1";
        assert_eq!(
            scrub_relevance_tags(header).as_deref(),
            Some("iii.session.id=S-1,iii.tag.message=fix%20login,iii.message.id=M-1"),
        );
        // Properties after the value don't confuse key extraction.
        assert_eq!(
            scrub_relevance_tags("iii.tag.kind=x;prop=1, iii.session.id=S-1").as_deref(),
            Some(" iii.session.id=S-1"),
        );
        // All entries scrubbed → no header at all.
        assert_eq!(scrub_relevance_tags("iii.tag.kind=queue.process"), None);
    }

    #[test]
    fn scrub_relevance_tags_drops_the_hidden_tag() {
        // The harness wraps its next-step enqueue in an `iii.tag.hidden`
        // baggage scope ("turn enqueue"); replayed verbatim it would mark the
        // ENTIRE next delivery subtree internal and trace UIs would hide the
        // whole step.
        let header = "iii.tag.hidden=turn%20enqueue,iii.session.id=S-1";
        assert_eq!(
            scrub_relevance_tags(header).as_deref(),
            Some("iii.session.id=S-1"),
        );
        assert_eq!(scrub_relevance_tags("iii.tag.hidden=turn%20enqueue"), None);
    }

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
    // QueueWorker::name
    // =========================================================================
    // Note: The module itself requires an adapter and engine, so we test what
    // we can without those dependencies.

    // =========================================================================
    // ConfigurableWorker trait constants
    // =========================================================================

    #[test]
    fn default_adapter_name() {
        assert_eq!(QueueWorker::DEFAULT_ADAPTER_NAME, "builtin");
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
        /// Queue names whose `setup_function_queue` returns an error, for
        /// exercising per-queue spawn-failure isolation in `apply_config`.
        setup_fail_queues: Mutex<std::collections::HashSet<String>>,
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
                setup_fail_queues: Mutex::new(std::collections::HashSet::new()),
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

        #[allow(clippy::too_many_arguments)]
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
            _namespace: Option<String>,
            _priority: Option<u8>,
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
            queue_name: &str,
            _config: &super::super::config::FunctionQueueConfig,
        ) -> anyhow::Result<()> {
            if self.setup_fail_queues.lock().await.contains(queue_name) {
                return Err(anyhow::anyhow!("mock setup failure for '{queue_name}'"));
            }
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

    fn setup_queue_module() -> (Arc<Engine>, QueueWorker, Arc<MockQueueAdapter>) {
        crate::workers::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter = Arc::new(MockQueueAdapter::new());
        let adapter_dyn: Arc<dyn QueueAdapter> = adapter.clone();
        let module = QueueWorker {
            engine: engine.clone(),
            live: Arc::new(RwLock::new(LiveState {
                config: Arc::new(super::super::config::QueueModuleConfig::default()),
                adapter: adapter_dyn,
            })),
            seed: None,
            consumers: Arc::new(StdMutex::new(HashMap::new())),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
            destroyed: Arc::new(AtomicBool::new(false)),
        };
        (engine, module, adapter)
    }

    /// Stub `configuration::get` on the engine to return a fixed value.
    fn stub_configuration_get(engine: &Arc<Engine>, value: Value) {
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "configuration::get".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |_input: Value| {
                let value = value.clone();
                async move {
                    FunctionResult::Success(Some(json!({ "id": "iii-queue", "value": value })))
                }
            }),
        );
    }

    #[tokio::test]
    async fn apply_config_isolates_per_queue_spawn_failure() {
        let (engine, module, adapter) = setup_queue_module();
        // The mock fails `setup_function_queue` for "bad" only.
        adapter
            .setup_fail_queues
            .lock()
            .await
            .insert("bad".to_string());

        stub_configuration_get(
            &engine,
            json!({ "queue_configs": {
                "good": { "concurrency": 1 },
                "bad": { "concurrency": 1 }
            } }),
        );

        // A single queue's spawn failure must not abort the whole apply.
        module
            .apply_config()
            .await
            .expect("apply_config must not error on a per-queue spawn failure");

        // The gate adopted the new config (set_config ran before the per-queue loop).
        let cfg = module.config_snapshot();
        assert!(cfg.queue_configs.contains_key("good"));
        assert!(cfg.queue_configs.contains_key("bad"));
        assert!(cfg.queue_configs.contains_key("default"));

        // Best-effort isolation: healthy queues got consumers; the failing one
        // did not, but it did not block the others.
        let consumers = module.consumers.lock().expect("consumers mutex");
        assert!(
            consumers.contains_key("good"),
            "a healthy queue must get a consumer even when a sibling fails"
        );
        assert!(
            consumers.contains_key("default"),
            "the default queue must get a consumer"
        );
        assert!(
            !consumers.contains_key("bad"),
            "the failing queue must be left out of the consumers map"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn apply_config_timeout_surfaces_downcastable_elapsed() {
        let (engine, module, _adapter) = setup_queue_module();
        // `configuration::get` never returns, so the bounded fetch times out.
        // The paused clock auto-advances to the timeout (no real 10s wait).
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "configuration::get".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async {
                std::future::pending::<FunctionResult<Option<Value>, crate::protocol::ErrorBody>>()
                    .await
            }),
        );

        let err = module
            .apply_config()
            .await
            .expect_err("a timed-out fetch must surface as an error");

        // `on_config_change` branches on this downcast to schedule its one-shot
        // retry; if the timeout stopped being downcastable the retry path would
        // silently break.
        assert!(
            err.downcast_ref::<tokio::time::error::Elapsed>().is_some(),
            "apply_config timeout must stay downcastable to Elapsed: {err:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn on_config_change_retries_once_after_timeout() {
        let (engine, module, _adapter) = setup_queue_module();
        // First `configuration::get` hangs (forcing a timeout); every later call
        // returns a valid config. The one-shot retry must pick it up.
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "configuration::get".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |_input: Value| {
                let calls = calls.clone();
                async move {
                    let n = calls.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        std::future::pending::<
                            FunctionResult<Option<Value>, crate::protocol::ErrorBody>,
                        >()
                        .await
                    } else {
                        FunctionResult::Success(Some(json!({
                            "id": "iii-queue",
                            "value": { "queue_configs": { "jobs": { "concurrency": 5 } } }
                        })))
                    }
                }
            }),
        );

        // Initial apply times out and schedules the detached one-shot retry.
        super::super::configuration::on_config_change(&module).await;
        assert!(
            !module.config_snapshot().queue_configs.contains_key("jobs"),
            "the timed-out first apply must not have applied anything"
        );

        // Let virtual time pass the retry delay so the detached retry runs and
        // applies the now-readable config.
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        assert!(
            module.config_snapshot().queue_configs.contains_key("jobs"),
            "the one-shot retry after a timeout must apply the now-readable config"
        );
    }

    // =========================================================================
    // QueueWorker::name
    // =========================================================================

    #[test]
    fn queue_module_name() {
        let (_engine, module, _adapter) = setup_queue_module();
        assert_eq!(Worker::name(&module), "QueueModule");
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
            trigger_type: "durable:subscriber".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"topic": "my-topic"}),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
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
            trigger_type: "durable:subscriber".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"topic": ""}),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
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
            trigger_type: "durable:subscriber".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({}),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
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
            trigger_type: "durable:subscriber".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "topic": "conditioned-topic",
                "condition_function_id": "test::condition_fn"
            }),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
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
            trigger_type: "durable:subscriber".to_string(),
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
            metadata: None,
            namespace: "default".to_string(),
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
            trigger_type: "durable:subscriber".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"topic": "unsub-topic"}),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
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
            trigger_type: "durable:subscriber".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({}),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
        };
        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());
        // unsubscribe is called regardless (with empty topic)
        assert_eq!(adapter.unsubscribe_count.load(Ordering::SeqCst), 1);
    }

    // =========================================================================
    // ConfigurableWorker trait tests
    // =========================================================================

    #[test]
    fn adapter_name_from_config_none() {
        let config = super::super::config::QueueModuleConfig::default();
        assert!(QueueWorker::adapter_name_from_config(&config).is_none());
    }

    #[test]
    fn adapter_name_from_config_some() {
        let config = super::super::config::QueueModuleConfig {
            adapter: Some(crate::workers::traits::AdapterEntry {
                name: "my::CustomAdapter".to_string(),
                config: None,
            }),
            ..Default::default()
        };
        assert_eq!(
            QueueWorker::adapter_name_from_config(&config),
            Some("my::CustomAdapter".to_string())
        );
    }

    #[test]
    fn adapter_config_from_config_none() {
        let config = super::super::config::QueueModuleConfig::default();
        assert!(QueueWorker::adapter_config_from_config(&config).is_none());
    }

    #[test]
    fn adapter_config_from_config_some() {
        let config = super::super::config::QueueModuleConfig {
            adapter: Some(crate::workers::traits::AdapterEntry {
                name: "my::Adapter".to_string(),
                config: Some(json!({"url": "redis://localhost"})),
            }),
            ..Default::default()
        };
        assert_eq!(
            QueueWorker::adapter_config_from_config(&config),
            Some(json!({"url": "redis://localhost"}))
        );
    }

    #[test]
    fn adapter_config_from_config_adapter_without_config() {
        let config = super::super::config::QueueModuleConfig {
            adapter: Some(crate::workers::traits::AdapterEntry {
                name: "my::Adapter".to_string(),
                config: None,
            }),
            ..Default::default()
        };
        assert!(QueueWorker::adapter_config_from_config(&config).is_none());
    }

    // =========================================================================
    // Worker::initialize test
    // =========================================================================

    #[tokio::test]
    async fn initialize_registers_trigger_type() {
        let (engine, module, _adapter) = setup_queue_module();
        let result = module.initialize().await;
        assert!(result.is_ok());
        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key("durable:subscriber")
        );
    }

    // =========================================================================
    // build helper test
    // =========================================================================

    #[test]
    fn build_creates_module() {
        crate::workers::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter: Arc<dyn QueueAdapter> = Arc::new(MockQueueAdapter::new());
        let config = super::super::config::QueueModuleConfig::default();
        let module = QueueWorker::build(engine.clone(), config, adapter);
        assert_eq!(Worker::name(&module), "QueueModule");
    }

    #[test]
    fn build_provisions_builtin_default_queue() {
        use super::super::config::{DEFAULT_QUEUE_NAME, QueueModuleConfig};

        crate::workers::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter: Arc<dyn QueueAdapter> = Arc::new(MockQueueAdapter::new());
        // Empty config — the operator declared no queues.
        let module = QueueWorker::build(engine, QueueModuleConfig::default(), adapter);
        assert!(
            module
                .config_snapshot()
                .queue_configs
                .contains_key(DEFAULT_QUEUE_NAME),
            "build() must provision the built-in default queue"
        );
    }

    #[tokio::test]
    async fn enqueue_to_default_queue_succeeds_without_config() {
        use super::super::config::{DEFAULT_QUEUE_NAME, QueueModuleConfig};

        crate::workers::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter = Arc::new(MockQueueAdapter::new());
        // A worker built from empty config — exactly what a user gets when they
        // never touch `queue_configs` in config.yaml.
        let module = QueueWorker::build(engine, QueueModuleConfig::default(), adapter.clone());

        let result = module
            .enqueue_to_function_queue(
                DEFAULT_QUEUE_NAME,
                "fn-1",
                json!({"key": "value"}),
                "test-msg-id",
                None,
                None,
                None,
            )
            .await;

        assert!(
            result.is_ok(),
            "enqueue to the built-in default queue should succeed, got: {:?}",
            result.err()
        );
        assert_eq!(adapter.enqueue_to_queue_count.load(Ordering::SeqCst), 1);
    }

    // =========================================================================
    // enqueue_to_function_queue tests
    // =========================================================================

    fn setup_queue_module_with_configs() -> (Arc<Engine>, QueueWorker, Arc<MockQueueAdapter>) {
        use super::super::config::{FunctionQueueConfig, QueueModuleConfig};

        crate::workers::observability::metrics::ensure_default_meter();
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

        let adapter_dyn: Arc<dyn QueueAdapter> = adapter.clone();
        let module = QueueWorker {
            engine: engine.clone(),
            live: Arc::new(RwLock::new(LiveState {
                config: Arc::new(config),
                adapter: adapter_dyn,
            })),
            seed: None,
            consumers: Arc::new(StdMutex::new(HashMap::new())),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
            destroyed: Arc::new(AtomicBool::new(false)),
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
                None,
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(adapter.enqueue_to_queue_count.load(Ordering::SeqCst), 1);
    }

    // =========================================================================
    // Integration tests with real BuiltinQueueAdapter
    // =========================================================================

    async fn setup_integration_module() -> (
        Arc<Engine>,
        QueueWorker,
        Arc<dyn super::super::QueueAdapter>,
    ) {
        use super::super::config::{FunctionQueueConfig, QueueModuleConfig};

        crate::workers::observability::metrics::ensure_default_meter();

        let engine = Arc::new(Engine::new());

        let factory = QueueWorker::get_adapter("builtin")
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

        let adapter_dyn: Arc<dyn QueueAdapter> = adapter.clone();
        let module = QueueWorker {
            engine: engine.clone(),
            live: Arc::new(RwLock::new(LiveState {
                config: Arc::new(config),
                adapter: adapter_dyn,
            })),
            seed: None,
            consumers: Arc::new(StdMutex::new(HashMap::new())),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
            destroyed: Arc::new(AtomicBool::new(false)),
        };

        (engine, module, adapter)
    }

    #[tokio::test]
    async fn integration_enqueue_consume_invoke_ack() {
        let (engine, module, adapter) = setup_integration_module().await;

        let call_count = Arc::new(AtomicU64::new(0));
        let counter = call_count.clone();

        let function = crate::function::Function {
            handler: Arc::new(move |_invocation_id, _input, _session, _metadata| {
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
                None,
                None,
            )
            .await;

        // Initialize starts the consumer loop
        module
            .initialize()
            .await
            .expect("initialize should succeed");
        let (_shutdown_tx_keep, shutdown_rx) = tokio::sync::watch::channel(false);
        module
            .start_background_tasks(shutdown_rx, _shutdown_tx_keep.clone())
            .await
            .expect("start_background_tasks should succeed");

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
            handler: Arc::new(move |_invocation_id, _input, _session, _metadata| {
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
                None,
                None,
            )
            .await;

        module
            .initialize()
            .await
            .expect("initialize should succeed");
        let (_shutdown_tx_keep, shutdown_rx) = tokio::sync::watch::channel(false);
        module
            .start_background_tasks(shutdown_rx, _shutdown_tx_keep.clone())
            .await
            .expect("start_background_tasks should succeed");

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
            handler: Arc::new(move |_invocation_id, input, _session, _metadata| {
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
                    None,
                    None,
                )
                .await;
        }

        module
            .initialize()
            .await
            .expect("initialize should succeed");
        let (_shutdown_tx_keep, shutdown_rx) = tokio::sync::watch::channel(false);
        module
            .start_background_tasks(shutdown_rx, _shutdown_tx_keep.clone())
            .await
            .expect("start_background_tasks should succeed");

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
            handler: Arc::new(move |_invocation_id, input, _session, _metadata| {
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
                    None,
                    None,
                )
                .await;
        }

        module
            .initialize()
            .await
            .expect("initialize should succeed");
        let (_shutdown_tx_keep, shutdown_rx) = tokio::sync::watch::channel(false);
        module
            .start_background_tasks(shutdown_rx, _shutdown_tx_keep.clone())
            .await
            .expect("start_background_tasks should succeed");

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

    fn topic_stats_input(v: Value) -> TopicStatsInput {
        serde_json::from_value(v).expect("valid TopicStatsInput")
    }

    fn dlq_messages_input(v: Value) -> DlqMessagesInput {
        serde_json::from_value(v).expect("valid DlqMessagesInput")
    }

    // =========================================================================
    // console_topic_stats tests
    // =========================================================================

    #[tokio::test]
    async fn console_topic_stats_empty_topic_returns_failure() {
        let (_engine, module, _adapter) = setup_queue_module();
        let result = module
            .console_topic_stats(topic_stats_input(json!({"topic": ""})))
            .await;
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
            .console_topic_stats(topic_stats_input(json!({"topic": "default"})))
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
            .console_topic_stats(topic_stats_input(json!({"topic": "some-topic"})))
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
        let result = module
            .console_dlq_messages(dlq_messages_input(json!({"topic": ""})))
            .await;
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
            .console_dlq_messages(dlq_messages_input(
                json!({"topic": "my-queue", "offset": 0, "limit": 50}),
            ))
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
            .console_dlq_messages(dlq_messages_input(json!({"topic": "default"})))
            .await;
        // Should succeed (even with empty results)
        assert!(matches!(result, FunctionResult::Success(_)));
    }

    #[tokio::test]
    async fn console_dlq_messages_adapter_error() {
        let (_engine, module, adapter) = setup_queue_module();
        adapter.dlq_peek_should_fail.store(true, Ordering::SeqCst);

        let result = module
            .console_dlq_messages(dlq_messages_input(json!({"topic": "my-queue"})))
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
            .console_dlq_messages(dlq_messages_input(json!({"topic": "my-queue"})))
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
