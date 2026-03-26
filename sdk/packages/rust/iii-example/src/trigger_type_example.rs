use iii_sdk::{III, IIIError, RegisterTriggerType, TriggerConfig, TriggerHandler};

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

fn handle_webhook(input: WebhookCallRequest) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "processed": true,
        "method": input.method,
        "body": input.body,
    }))
}

// ── List trigger types example ──────────────────────────────────────────

pub async fn list_trigger_types_example(iii: &III) {
    println!("\n--- Listing all trigger types ---");

    match iii.list_trigger_types(false).await {
        Ok(trigger_types) => {
            println!("Found {} trigger types:\n", trigger_types.len());
            for tt in &trigger_types {
                println!("  [{}] {}", tt.id, tt.description);
                if let Some(config_fmt) = &tt.trigger_request_format {
                    println!("    trigger_request_format: {}", config_fmt);
                }
                if let Some(call_fmt) = &tt.call_request_format {
                    println!("    call_request_format: {}", call_fmt);
                }
                println!();
            }
        }
        Err(e) => {
            println!("Failed to list trigger types: {e}");
        }
    }
}
