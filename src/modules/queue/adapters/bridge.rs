// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use iii_sdk::{III, IIIError, Trigger};
use serde_json::Value;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    engine::{Engine, EngineTrait},
    modules::queue::{
        QueueAdapter, SubscriberQueueConfig,
        registry::{QueueAdapterFuture, QueueAdapterRegistration},
    },
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

        let bridge = Arc::new(III::new(&bridge_url));
        bridge
            .connect()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(Self {
            engine,
            bridge,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    fn queue_enqueue_function_id() -> &'static str {
        "enqueue"
    }

    fn build_enqueue_payload(topic: &str, data: Value) -> Value {
        serde_json::json!({ "topic": topic, "data": data })
    }
}

#[async_trait]
impl QueueAdapter for BridgeAdapter {
    /// Enqueues a message to the bridge for distribution to other engines.
    /// Failures are logged but do not block the caller.
    async fn enqueue(&self, topic: &str, data: Value) {
        let input = Self::build_enqueue_payload(topic, data);
        if let Err(e) = self
            .bridge
            .call_void(Self::queue_enqueue_function_id(), input)
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
        let handler_path_for_trigger = handler_path.clone();
        let engine = Arc::clone(&self.engine);
        let function_id_for_subscription = function_id.to_string();
        let function_id_for_handler = function_id_for_subscription.clone();
        let condition_function_id_for_handler = condition_function_id.clone();
        self.bridge
            .register_function(handler_path.clone(), move |data| {
                let engine = Arc::clone(&engine);
                let function_id = function_id_for_handler.clone();
                let condition_function_id = condition_function_id_for_handler.clone();
                async move {
                    // Check condition if provided
                    if let Some(condition_path) = condition_function_id {
                        match engine.call(&condition_path, data.clone()).await {
                            Ok(Some(result)) => {
                                if let Some(passed) = result.as_bool()
                                    && !passed
                                {
                                    tracing::debug!(
                                        condition_path = %condition_path,
                                        "Condition check failed, skipping handler"
                                    );
                                    return Ok(Value::Null);
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    condition_path = %condition_path,
                                    "Condition function returned no result"
                                );
                            }
                            Err(err) => {
                                tracing::error!(
                                    condition_path = %condition_path,
                                    error = ?err,
                                    "Error invoking condition function"
                                );
                                return Err(IIIError::Remote {
                                    code: err.code,
                                    message: err.message,
                                });
                            }
                        }
                    }

                    // Invoke the actual handler
                    match engine.call(&function_id, data).await {
                        Ok(result) => Ok(result.unwrap_or(Value::Null)),
                        Err(err) => Err(IIIError::Remote {
                            code: err.code,
                            message: err.message,
                        }),
                    }
                }
            });

        let trigger = match self.bridge.register_trigger(
            "queue",
            handler_path_for_trigger.clone(),
            serde_json::json!({ "topic": topic }),
        ) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    topic = %topic,
                    id = %id,
                    function_id = %function_id_for_subscription,
                    handler_path = %handler_path_for_trigger,
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

    async fn dlq_count(&self, _topic: &str) -> anyhow::Result<u64> {
        Err(anyhow::anyhow!(
            "Bridge queue adapter does not support DLQ operations"
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::engine::Engine;

    #[test]
    fn test_enqueue_uses_enqueue_function_id() {
        assert_eq!(BridgeAdapter::queue_enqueue_function_id(), "enqueue");
    }

    #[test]
    fn test_enqueue_builds_enqueue_payload() {
        let payload = BridgeAdapter::build_enqueue_payload(
            "topic.orders.created",
            serde_json::json!({ "order_id": "o-1" }),
        );

        assert_eq!(payload["topic"], "topic.orders.created");
        assert_eq!(payload["data"]["order_id"], "o-1");
    }

    #[tokio::test]
    async fn test_subscribe_handles_bridge_error_gracefully() {
        // This test verifies subscribe doesn't panic on bridge errors
        // We can't easily test actual bridge failure without mocking,
        // so we test the error path exists
        let engine = Arc::new(Engine::new());

        // Use a URL that will fail to connect
        // Note: This test may be flaky - consider mocking bridge in future
        let result = BridgeAdapter::new(engine.clone(), "ws://invalid-host:9999".to_string()).await;

        // Connection should fail, but that's expected
        if result.is_err() {
            // This is fine - we're testing error handling
            return;
        }

        // If connection succeeds (unlikely), test subscribe doesn't panic
        let adapter = result.unwrap();
        adapter
            .subscribe("test_topic", "test_id", "functions.test", None, None)
            .await;
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
