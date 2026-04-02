// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use iii_sdk::{
    III, IIIError, InitOptions, RegisterFunctionMessage, RegisterTriggerInput, Trigger,
    TriggerAction, TriggerRequest, register_worker,
};
use serde_json::Value;
use tokio::sync::RwLock;
use uuid::Uuid;

use tracing::Instrument;

use crate::{
    condition::check_condition,
    engine::{Engine, EngineTrait},
    modules::queue::{
        QueueAdapter, SubscriberQueueConfig,
        registry::{QueueAdapterFuture, QueueAdapterRegistration},
    },
    telemetry::SpanExt,
};

struct SubscriptionInfo {
    trigger: Trigger,
}

/// Bridge-based queue adapter for cross-engine queue communication.
///
/// This adapter allows queue messages to be enqueued and subscribed across
/// different engine instances via the Bridge WebSocket protocol.
///
/// # Usage
///
/// Configure in `config.yaml`:
/// ```yaml
/// modules:
///   - class: modules::queue::QueueModule
///     config:
///       adapter:
///         class: modules::queue::adapters::Bridge
///         config:
///           bridge_url: "ws://localhost:49134"
/// ```
///
/// # Limitations
///
/// - DLQ operations are not supported (returns error)
/// - Queue config is not supported (ignored)
/// - Functions registered via bridge persist for bridge lifetime
pub struct BridgeAdapter {
    engine: Arc<Engine>,
    bridge: Arc<III>,
    subscriptions: Arc<RwLock<HashMap<String, SubscriptionInfo>>>,
}

impl BridgeAdapter {
    /// Creates a new bridge adapter and connects to the bridge.
    ///
    /// # Arguments
    ///
    /// * `engine` - The engine instance for function invocation
    /// * `bridge_url` - WebSocket URL for bridge connection (e.g., "ws://localhost:49134")
    ///
    /// # Errors
    ///
    /// Returns error if bridge connection fails.
    pub async fn new(engine: Arc<Engine>, bridge_url: String) -> anyhow::Result<Self> {
        tracing::info!(bridge_url = %bridge_url, "Connecting to bridge");

        let bridge = Arc::new(register_worker(&bridge_url, InitOptions::default()));

        Ok(Self {
            engine,
            bridge,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    const ENQUEUE_FUNCTION_ID: &'static str = "enqueue";

    /// Builds the JSON payload for enqueuing a message via the bridge.
    ///
    /// # Reserved fields
    ///
    /// This function embeds trace context using the following reserved field names:
    /// - `__traceparent` — W3C Trace Context `traceparent` header for distributed tracing
    /// - `__baggage` — W3C Baggage header for cross-service context propagation
    ///
    /// The double-underscore (`__`) prefix is a convention indicating internal bridge
    /// metadata. **User payloads must not use `__traceparent` or `__baggage` as field
    /// names**, as they will be overwritten during enqueue and stripped on the receiving
    /// side during trace context extraction.
    fn build_enqueue_payload(
        topic: &str,
        data: Value,
        traceparent: Option<&str>,
        baggage: Option<&str>,
    ) -> Value {
        let mut payload = serde_json::json!({ "topic": topic, "data": data });
        if let Some(tp) = traceparent {
            payload["__traceparent"] = Value::String(tp.to_string());
        }
        if let Some(bg) = baggage {
            payload["__baggage"] = Value::String(bg.to_string());
        }
        payload
    }
}

#[async_trait]
impl QueueAdapter for BridgeAdapter {
    /// Enqueues a message to the bridge for distribution to other engines.
    /// Failures are logged but do not block the caller.
    ///
    /// Trace context is embedded in the payload as `__traceparent` and `__baggage`
    /// fields so it can be extracted on the receiving side.
    async fn enqueue(
        &self,
        topic: &str,
        data: Value,
        traceparent: Option<String>,
        baggage: Option<String>,
    ) {
        tracing::debug!(
            topic = %topic,
            has_traceparent = traceparent.is_some(),
            has_baggage = baggage.is_some(),
            "enqueue via bridge with trace context"
        );
        let input =
            Self::build_enqueue_payload(topic, data, traceparent.as_deref(), baggage.as_deref());
        if let Err(e) = self
            .bridge
            .trigger(TriggerRequest {
                function_id: Self::ENQUEUE_FUNCTION_ID.to_string(),
                payload: input,
                action: Some(TriggerAction::Void),
                timeout_ms: None,
            })
            .await
        {
            tracing::error!(error = %e, topic = %topic, "Failed to enqueue message via bridge");
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
        let key = format!("{}:{}", topic, id);

        // Acquire write lock upfront to make check+register+insert atomic,
        // preventing a TOCTOU race where two concurrent calls could both
        // pass the contains_key check and register duplicate triggers.
        let mut subs = self.subscriptions.write().await;

        if subs.contains_key(&key) {
            tracing::warn!(
                topic = %topic,
                id = %id,
                "Already subscribed to topic/id, skipping duplicate subscription"
            );
            return;
        }

        let handler_path = format!("queue::bridge::on_message::{}", Uuid::new_v4());
        let engine = Arc::clone(&self.engine);
        let function_id_owned = function_id.to_string();
        let condition_function_id_owned = condition_function_id.clone();
        let topic_owned = topic.to_string();
        self.bridge.register_function((
            RegisterFunctionMessage {
                id: handler_path.clone(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
                invocation: None,
            },
            move |data: Value| {
                let engine = Arc::clone(&engine);
                let function_id = function_id_owned.clone();
                let condition_function_id = condition_function_id_owned.clone();
                let topic_name = topic_owned.clone();
                async move {
                    // Extract trace context embedded by the sender
                    let traceparent = data["__traceparent"].as_str().map(|s| s.to_string());
                    let baggage = data["__baggage"].as_str().map(|s| s.to_string());

                    let span = tracing::info_span!(
                        "queue_job",
                        otel.name = %format!("queue {}", topic_name),
                        queue = %topic_name,
                        "messaging.system" = "bridge-queue",
                        "messaging.destination.name" = %topic_name,
                        "messaging.operation.type" = "process",
                        otel.status_code = tracing::field::Empty,
                    )
                    .with_parent_headers(traceparent.as_deref(), baggage.as_deref());

                    async move {
                        if let Some(condition_path) = condition_function_id {
                            tracing::debug!(
                                condition_function_id = %condition_path,
                                "Checking trigger conditions"
                            );
                            match check_condition(engine.as_ref(), &condition_path, data.clone())
                                .await
                            {
                                Ok(true) => {}
                                Ok(false) => {
                                    tracing::debug!(
                                        condition_path = %condition_path,
                                        "Condition check failed, skipping handler"
                                    );
                                    tracing::Span::current().record("otel.status_code", "OK");
                                    return Ok(Value::Null);
                                }
                                Err(err) => {
                                    tracing::error!(
                                        condition_function_id = %condition_path,
                                        error = ?err,
                                        "Error invoking condition function"
                                    );
                                    tracing::Span::current().record("otel.status_code", "ERROR");
                                    return Err(IIIError::Remote {
                                        code: err.code,
                                        message: err.message,
                                        stacktrace: err.stacktrace,
                                    });
                                }
                            }
                        }

                        // Invoke the actual handler
                        match engine.call(&function_id, data).await {
                            Ok(result) => {
                                tracing::Span::current().record("otel.status_code", "OK");
                                Ok::<Value, IIIError>(result.unwrap_or(Value::Null))
                            }
                            Err(err) => {
                                tracing::Span::current().record("otel.status_code", "ERROR");
                                Err(IIIError::Remote {
                                    code: err.code,
                                    message: err.message,
                                    stacktrace: err.stacktrace,
                                })
                            }
                        }
                    }
                    .instrument(span)
                    .await
                }
            },
        ));

        let trigger = match self.bridge.register_trigger(RegisterTriggerInput {
            trigger_type: "queue".to_string(),
            function_id: handler_path.clone(),
            config: serde_json::json!({ "topic": topic }),
        }) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    topic = %topic,
                    id = %id,
                    function_id = %function_id,
                    handler_path = %handler_path,
                    "Failed to register queue trigger via bridge, subscription not created"
                );
                // Note: If register_trigger fails after register_function succeeds,
                // we have a registered function that will never be called.
                // Bridge doesn't provide unregister_function, so this function persists.
                // This is acceptable since bridge connections are long-lived and the
                // function won't cause issues, but it's worth documenting.
                return;
            }
        };

        subs.insert(key, SubscriptionInfo { trigger });
    }

    async fn unsubscribe(&self, topic: &str, id: &str) {
        let key = format!("{}:{}", topic, id);
        let mut subs = self.subscriptions.write().await;
        if let Some(subscription) = subs.remove(&key) {
            subscription.trigger.unregister();
            // Note: Bridge doesn't have unregister_function, functions persist
            // for bridge lifetime, which is acceptable since bridge lives with adapter
        }
    }

    async fn redrive_dlq(&self, _topic: &str) -> anyhow::Result<u64> {
        Err(anyhow::anyhow!(
            "Bridge queue adapter does not support DLQ operations"
        ))
    }

    async fn redrive_dlq_message(&self, _topic: &str, _message_id: &str) -> anyhow::Result<bool> {
        Err(anyhow::anyhow!(
            "Bridge queue adapter does not support DLQ operations"
        ))
    }

    async fn discard_dlq_message(&self, _topic: &str, _message_id: &str) -> anyhow::Result<bool> {
        Err(anyhow::anyhow!(
            "Bridge queue adapter does not support DLQ operations"
        ))
    }

    async fn dlq_count(&self, _topic: &str) -> anyhow::Result<u64> {
        Err(anyhow::anyhow!(
            "Bridge queue adapter does not support DLQ operations"
        ))
    }

    async fn publish_to_function_queue(
        &self,
        queue_name: &str,
        function_id: &str,
        data: Value,
        _message_id: &str,
        _max_retries: u32,
        _backoff_ms: u64,
        _traceparent: Option<String>,
        _baggage: Option<String>,
    ) {
        if let Err(e) = self
            .bridge
            .trigger(TriggerRequest {
                function_id: function_id.to_string(),
                payload: data,
                action: Some(TriggerAction::Enqueue {
                    queue: queue_name.to_string(),
                }),
                timeout_ms: None,
            })
            .await
        {
            tracing::error!(error = %e, queue = %queue_name, function_id = %function_id, "Failed to enqueue via bridge");
        }
    }

    async fn consume_function_queue(
        &self,
        _queue_name: &str,
        _prefetch: u32,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<crate::modules::queue::QueueMessage>> {
        // Bridge adapter: consuming happens on the remote engine.
        // Return a channel that will never receive (consumer loop stays idle).
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(rx)
    }

    async fn list_topics(&self) -> anyhow::Result<Vec<crate::modules::queue::TopicInfo>> {
        Ok(vec![]) // Bridge doesn't track local queue state
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::engine::Engine;

    #[test]
    fn test_enqueue_uses_enqueue_function_id() {
        assert_eq!(BridgeAdapter::ENQUEUE_FUNCTION_ID, "enqueue");
    }

    #[test]
    fn test_enqueue_builds_enqueue_payload() {
        let payload = BridgeAdapter::build_enqueue_payload(
            "topic.orders.created",
            serde_json::json!({ "order_id": "o-1" }),
            None,
            None,
        );

        assert_eq!(payload["topic"], "topic.orders.created");
        assert_eq!(payload["data"]["order_id"], "o-1");
        assert!(payload.get("__traceparent").is_none());
        assert!(payload.get("__baggage").is_none());
    }

    #[test]
    fn test_enqueue_builds_enqueue_payload_with_trace_context() {
        let payload = BridgeAdapter::build_enqueue_payload(
            "topic.orders.created",
            serde_json::json!({ "order_id": "o-1" }),
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
            Some("key=value"),
        );

        assert_eq!(payload["topic"], "topic.orders.created");
        assert_eq!(payload["data"]["order_id"], "o-1");
        assert_eq!(
            payload["__traceparent"],
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        );
        assert_eq!(payload["__baggage"], "key=value");
    }

    #[tokio::test]
    async fn test_subscribe_handles_bridge_error_gracefully() {
        let engine = Arc::new(Engine::new());
        let result = BridgeAdapter::new(engine.clone(), "ws://invalid-host:9999".to_string()).await;

        // Connection failure is the expected path; if it somehow succeeds,
        // verify subscribe doesn't panic either.
        let Ok(adapter) = result else { return };
        adapter
            .subscribe("test_topic", "test_id", "functions.test", None, None)
            .await;
    }

    #[tokio::test]
    async fn test_redrive_dlq_message_returns_error() {
        use crate::modules::queue::QueueAdapter;

        let engine = Arc::new(Engine::new());
        let result = BridgeAdapter::new(engine.clone(), "ws://invalid-host:9999".to_string()).await;
        let Ok(adapter) = result else { return };

        let res = adapter.redrive_dlq_message("test_topic", "msg-1").await;
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            "Bridge queue adapter does not support DLQ operations"
        );
    }

    #[tokio::test]
    async fn test_discard_dlq_message_returns_error() {
        use crate::modules::queue::QueueAdapter;

        let engine = Arc::new(Engine::new());
        let result = BridgeAdapter::new(engine.clone(), "ws://invalid-host:9999".to_string()).await;
        let Ok(adapter) = result else { return };

        let res = adapter.discard_dlq_message("test_topic", "msg-1").await;
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            "Bridge queue adapter does not support DLQ operations"
        );
    }

    #[tokio::test]
    async fn test_list_topics_returns_empty() {
        use crate::modules::queue::QueueAdapter;

        let engine = Arc::new(Engine::new());
        let result = BridgeAdapter::new(engine.clone(), "ws://invalid-host:9999".to_string()).await;
        let Ok(adapter) = result else { return };

        let topics = adapter.list_topics().await.unwrap();
        assert!(topics.is_empty());
    }
}

fn make_adapter(engine: Arc<Engine>, config: Option<Value>) -> QueueAdapterFuture {
    Box::pin(async move {
        let bridge_url = config
            .as_ref()
            .and_then(|c| c.get("bridge_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("ws://localhost:49134")
            .to_string();
        Ok(Arc::new(BridgeAdapter::new(engine, bridge_url).await?) as Arc<dyn QueueAdapter>)
    })
}

crate::register_adapter!(
    <QueueAdapterRegistration> "modules::queue::adapters::Bridge",
    make_adapter
);
