//! A worker-owned event source: an observable function whose owner reads
//! each observer's args and hands every event to the observers it belongs
//! to. This is what `register_trigger_type` + `register_trigger` did before
//! 0.25.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use iii_sdk::protocol::TriggerRequest;
use iii_sdk::{
    Error, IIIClient, Observe, ObserveHandler, ObserveRejection, Observed, Observer,
    RegisterFunction, Source,
};
use serde::Deserialize;
use serde_json::json;

/// Minimal deserialization target for `engine::triggers::list` rows used
/// only by this example. The SDK no longer carries a hand-written type for
/// this — the engine surface will be auto-generated later.
#[derive(Debug, Deserialize)]
struct TriggerTypeRow {
    id: String,
    worker_name: String,
    description: String,
}

// ── The observers' args and the event, as typed structs ─────────────────

/// What an observer of `example::on-webhook` asks for (was the trigger
/// config). The engine validates it against this schema (`INVALID_ARGS`).
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookArgs {
    /// URL to listen for incoming webhooks
    pub url: String,
    /// Optional secret for HMAC signature verification
    pub secret: Option<String>,
}

/// One incoming webhook: the function's input, what filters read as `input.*`.
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct WebhookEvent {
    /// The URL it arrived on
    pub url: String,
    /// HTTP method of the incoming webhook
    pub method: String,
    /// Request headers
    pub headers: HashMap<String, String>,
    /// Request body
    pub body: serde_json::Value,
    /// Whether the HMAC signature was verified
    pub signature_verified: bool,
}

// ── The owner's index: url → observers ──────────────────────────────────

#[derive(Clone, Default)]
struct WebhookRoutes(Arc<Mutex<HashMap<String, Vec<Observer>>>>);

impl WebhookRoutes {
    fn observers(&self, url: &str) -> Vec<Observer> {
        self.0.lock().unwrap().get(url).cloned().unwrap_or_default()
    }
}

#[async_trait::async_trait]
impl ObserveHandler for WebhookRoutes {
    async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
        let WebhookArgs { url, .. } = obs.args()?;
        println!("[webhook] {} now receives webhooks on {url}", obs.target());
        self.0
            .lock()
            .unwrap()
            .entry(url)
            .or_default()
            .push(obs.clone());
        Ok(())
    }

    async fn on_observer_removed(&self, obs: &Observer) {
        println!("[webhook] observer {} removed", obs.id());
        for observers in self.0.lock().unwrap().values_mut() {
            observers.retain(|o| o != obs);
        }
    }
}

// ── Setup ───────────────────────────────────────────────────────────────

pub fn setup(iii: &IIIClient) {
    let routes = WebhookRoutes::default();

    // The source: one invocation per incoming webhook. Its handler routes the
    // event to the observers of that URL; their filters are checked in emit.
    iii.register_function(
        "example::on-webhook",
        RegisterFunction::new({
            let routes = routes.clone();
            move |event: WebhookEvent| {
                for obs in routes.observers(&event.url) {
                    obs.emit(&event)?;
                }
                Ok::<_, Error>(())
            }
        })
        .description("Invoked once per incoming webhook. Observe it with args { url, secret? }.")
        .observe(
            Observe::owner()
                .args_format::<WebhookArgs>()
                .handler(routes),
        ),
    );

    // The target, and the observer that connects them: args for the owner,
    // filters for which webhooks reach the target.
    iii.register_function(
        "example::webhook_handler",
        RegisterFunction::new(handle_webhook),
    );
    iii.observe(
        Source::function("example::on-webhook")
            .args(json!({ "url": "/hooks/my-service", "secret": "my-secret-key" }))
            .filters(json!({ "input.method": { "$in": ["POST", "PUT"] } })),
        "example::webhook_handler",
    );
}

fn handle_webhook(event: Observed<WebhookEvent>) -> Result<serde_json::Value, Error> {
    let input = event.input.ok_or("the observer captures the input")?;
    Ok(json!({
        "processed": true,
        "method": input.method,
        "body": input.body,
    }))
}

// ── List trigger types example ──────────────────────────────────────────

pub async fn print_trigger_type_catalog(iii: &IIIClient) {
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
