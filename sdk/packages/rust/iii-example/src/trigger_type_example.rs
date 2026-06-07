use iii_sdk::trigger::{TriggerConfig, TriggerHandler};
use iii_sdk::{III, IIIError, RegisterTriggerType, TriggerRequest};
use serde::Deserialize;

/// Minimal deserialization target for `engine::triggers::list` rows used
/// only by this example. The SDK no longer carries a hand-written type for
/// this — the engine surface will be auto-generated later.
#[derive(Debug, Deserialize)]
struct TriggerTypeRow {
    id: String,
    worker_name: String,
    description: String,
}

// ── Custom trigger type config & call request as typed structs ──────────

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct WebhookTriggerConfig {
    /// URL to listen for incoming webhooks
    pub url: String,
    /// Optional secret for HMAC signature verification
    pub secret: Option<String>,
    /// HTTP methods to accept (defaults to POST)
    pub methods: Option<Vec<String>>,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct WebhookCallRequest {
    /// HTTP method of the incoming webhook
    pub method: String,
    /// Request headers
    pub headers: std::collections::HashMap<String, String>,
    /// Request body
    pub body: serde_json::Value,
    /// Whether the HMAC signature was verified
    pub signature_verified: bool,
}

// ── Handler implementation ──────────────────────────────────────────────

struct WebhookHandler;

#[async_trait::async_trait]
impl TriggerHandler for WebhookHandler {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), IIIError> {
        println!(
            "[webhook] Registered trigger {} for function {} with config: {}",
            config.id, config.function_id, config.config
        );
        Ok(())
    }

    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), IIIError> {
        println!("[webhook] Unregistered trigger {}", config.id);
        Ok(())
    }
}

// ── Setup ───────────────────────────────────────────────────────────────

pub fn setup(iii: &III) {
    // Register trigger type — returns a typed handle
    let webhook = iii.register_trigger_type(
        RegisterTriggerType::new("webhook", "Incoming webhook trigger", WebhookHandler)
            .trigger_request_format::<WebhookTriggerConfig>()
            .call_request_format::<WebhookCallRequest>(),
    );

    // register_function on the handle: enforces Fn(WebhookCallRequest) -> ...
    webhook.register_function("example::webhook_handler", handle_webhook);

    // register_trigger on the handle: enforces WebhookTriggerConfig
    webhook
        .register_trigger(
            "example::webhook_handler",
            WebhookTriggerConfig {
                url: "/hooks/my-service".into(),
                secret: Some("my-secret-key".into()),
                methods: Some(vec!["POST".into(), "PUT".into()]),
            },
        )
        .expect("failed to register webhook trigger");
}

fn handle_webhook(input: WebhookCallRequest) -> Result<serde_json::Value, IIIError> {
    Ok(serde_json::json!({
        "processed": true,
        "method": input.method,
        "body": input.body,
    }))
}

// ── List trigger types example ──────────────────────────────────────────

pub async fn print_trigger_type_catalog(iii: &III) {
    println!("\n--- Listing all trigger types ---");

    // `engine::trigger-types::list` was retired in favor of
    // `engine::triggers::list` (which now returns trigger TYPES). The list
    // shape is lean — call `engine::triggers::info` per id for schemas.
    let result = iii
        .trigger(TriggerRequest {
            function_id: "engine::triggers::list".to_string(),
            payload: serde_json::json!({ "include_internal": false }),
            action: None,
            timeout_ms: None,
        })
        .await;

    match result {
        Ok(value) => {
            let trigger_types: Vec<TriggerTypeRow> = serde_json::from_value(
                value
                    .get("triggers")
                    .cloned()
                    .unwrap_or(serde_json::Value::Array(vec![])),
            )
            .unwrap_or_default();
            println!("Found {} trigger types:\n", trigger_types.len());
            for tt in &trigger_types {
                println!("  [{}] ({}) {}", tt.id, tt.worker_name, tt.description);
            }
        }
        Err(e) => {
            println!("Failed to list trigger types: {e}");
        }
    }
}
