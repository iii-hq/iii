/// Pattern: Custom Triggers
/// Comparable to: Custom event adapters, webhook connectors, subscription bridges
///
/// Demonstrates how to define entirely new trigger types beyond the built-in
/// http, durable:subscriber, cron, state, and subscribe triggers. A custom trigger type
/// registers handler callbacks that the engine invokes when triggers of that
/// type are created or removed, letting you bridge any external event source
/// (webhooks, message subscriptions) into the iii function graph.
///
/// How-to references:
///   - Custom trigger types: https://iii.dev/docs/how-to/create-custom-trigger-type
use std::collections::HashMap;
use std::sync::Arc;

use iii_sdk::{
    register_worker, IIIError, InitOptions, RegisterFunction, RegisterTriggerInput,
    RegisterTriggerType, TriggerConfig, TriggerHandler, TriggerRequest,
};
use serde_json::json;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Custom trigger type — Webhook receiver
// Registers an HTTP endpoint per trigger and fires the bound function when
// an external service POSTs to it.
// ---------------------------------------------------------------------------

struct WebhookEndpoint {
    path: String,
    function_id: String,
}

struct WebhookTriggerHandler {
    iii: iii_sdk::III,
    endpoints: Arc<Mutex<HashMap<String, WebhookEndpoint>>>,
}

impl TriggerHandler for WebhookTriggerHandler {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), IIIError> {
        let path = config
            .config
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(&format!("/webhooks/{}", config.id))
            .to_string();

        let endpoint = WebhookEndpoint {
            path: path.clone(),
            function_id: config.function_id.clone(),
        };

        self.endpoints
            .lock()
            .await
            .insert(config.id.clone(), endpoint);

        Ok(())
    }

    // NOTE: In production, an HTTP listener would match incoming requests
    // to endpoints and call iii.trigger(endpoint.function_id, payload)
    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), IIIError> {
        self.endpoints.lock().await.remove(&config.id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Custom trigger type — External subscription
// Bridges an event source that already pushes topic messages.
// ---------------------------------------------------------------------------

struct TopicBinding {
    topic: String,
    function_id: String,
}

struct TopicSubscriptionHandler {
    iii: iii_sdk::III,
    bindings: Arc<Mutex<HashMap<String, TopicBinding>>>,
}

impl TopicSubscriptionHandler {
    async fn emit(&self, topic: &str, message: serde_json::Value) {
        let bindings = self.bindings.lock().await;
        for (trigger_id, binding) in bindings.iter() {
            if binding.topic == topic {
                let _ = self
                    .iii
                    .trigger(TriggerRequest {
                        function_id: binding.function_id.clone(),
                        payload: json!({
                            "source": "topic-subscription",
                            "trigger_id": trigger_id,
                            "topic": topic,
                            "data": message.clone(),
                        }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await;
            }
        }
    }
}

impl TriggerHandler for TopicSubscriptionHandler {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), IIIError> {
        let topic = config
            .config
            .get("topic")
            .and_then(|v| v.as_str())
            .ok_or_else(|| IIIError::Handler("missing topic in config".into()))?
            .to_string();

        self.bindings.lock().await.insert(
            config.id.clone(),
            TopicBinding {
                topic,
                function_id: config.function_id,
            },
        );
        Ok(())
    }

    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), IIIError> {
        self.bindings.lock().await.remove(&config.id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Handler function — processes events from any custom trigger above
// ---------------------------------------------------------------------------

fn on_event(input: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(json!({
        "received": true,
        "source": input.get("source").and_then(|v| v.as_str()).unwrap_or("unknown"),
    }))
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // Register handler function
    iii.register_function(
        RegisterFunction::new("custom-triggers::on-event", on_event)
            .description("Processes events from custom trigger types"),
    );

    // Register webhook trigger type
    let webhook_handler = WebhookTriggerHandler {
        iii: iii.clone(),
        endpoints: Arc::new(Mutex::new(HashMap::new())),
    };
    iii.register_trigger_type(RegisterTriggerType::new(
        "webhook",
        "Fires when an external service sends an HTTP POST to the registered endpoint",
        webhook_handler,
    ));

    // Register topic subscription trigger type
    let topic_handler = TopicSubscriptionHandler {
        iii: iii.clone(),
        bindings: Arc::new(Mutex::new(HashMap::new())),
    };
    iii.register_trigger_type(RegisterTriggerType::new(
        "topic-subscription",
        "Fires when an external event bus publishes to a topic",
        topic_handler,
    ));

    // Bind triggers using the custom types
    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "webhook".into(),
        function_id: "custom-triggers::on-event".into(),
        config: json!({ "path": "/hooks/github" }),
        metadata: None,
    })?;

    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "topic-subscription".into(),
        function_id: "custom-triggers::on-event".into(),
        config: json!({ "topic": "orders.created" }),
        metadata: None,
    })?;

    tokio::signal::ctrl_c().await.ok();
    iii.shutdown();
    Ok(())
}
