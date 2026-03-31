/// Pattern: Custom Triggers
/// Comparable to: Custom event adapters, webhook connectors, polling integrators
///
/// Demonstrates how to define entirely new trigger types beyond the built-in
/// http, queue, cron, state, and subscribe triggers. A custom trigger type
/// registers handler callbacks that the engine invokes when triggers of that
/// type are created or removed, letting you bridge any external event source
/// (webhooks, pollers) into the iii function graph.
///
/// How-to references:
///   - Custom trigger types: https://iii.dev/docs/how-to/create-custom-trigger-type

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    TriggerConfig, TriggerHandler,
};
use serde_json::json;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

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
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), String> {
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
    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), String> {
        self.endpoints.lock().await.remove(&config.id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Custom trigger type — Polling with ETag
// Periodically fetches a URL and fires only when the content changes.
// ---------------------------------------------------------------------------

struct PollingTriggerHandler {
    iii: iii_sdk::III,
    tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

impl TriggerHandler for PollingTriggerHandler {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), String> {
        let trigger_id = config.id.clone();
        let function_id = config.function_id.clone();
        let url = config
            .config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or("missing url in config")?
            .to_string();
        let interval_ms = match config.config.get("interval_ms").and_then(|v| v.as_u64()) {
            Some(0) => return Err("interval_ms must be greater than 0".into()),
            Some(ms) => ms,
            None => 30_000,
        };

        let iii = self.iii.clone();

        let handle = tokio::spawn(async move {
            let client = reqwest::Client::new();
            let mut last_etag: Option<String> = None;

            loop {
                let mut req = client.get(&url);
                if let Some(ref etag) = last_etag {
                    req = req.header("If-None-Match", etag);
                }

                match req.send().await {
                    Ok(resp) => {
                        let status = resp.status().as_u16();

                        if status == 304 {
                            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
                            continue;
                        }

                        if !(200..300).contains(&status) {
                            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
                            continue;
                        }

                        let etag = resp
                            .headers()
                            .get("etag")
                            .and_then(|v| v.to_str().ok())
                            .map(|s| s.to_string());

                        if etag.is_some() && etag != last_etag {
                            if let Ok(body) = resp.json::<serde_json::Value>().await {
                                let result = iii
                                    .trigger(TriggerRequest {
                                        function_id: function_id.clone(),
                                        payload: json!({
                                            "source": "polling",
                                            "trigger_id": trigger_id,
                                            "etag": etag,
                                            "data": body,
                                        }),
                                        action: None,
                                        timeout_ms: None,
                                    })
                                    .await;

                                if result.is_ok() {
                                    last_etag = etag;
                                }
                            }
                        }
                    }
                    Err(_) => {}
                }

                tokio::time::sleep(Duration::from_millis(interval_ms)).await;
            }
        });

        self.tasks.lock().await.insert(config.id.clone(), handle);
        Ok(())
    }

    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), String> {
        if let Some(handle) = self.tasks.lock().await.remove(&config.id) {
            handle.abort();
        }
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
    iii.register_trigger_type("webhook", webhook_handler)
        .expect("failed to register webhook trigger type");

    // Register polling trigger type
    let polling_handler = PollingTriggerHandler {
        iii: iii.clone(),
        tasks: Arc::new(Mutex::new(HashMap::new())),
    };
    iii.register_trigger_type("polling", polling_handler)
        .expect("failed to register polling trigger type");

    // Bind triggers using the custom types
    iii.register_trigger_with_config(
        "webhook",
        "custom-triggers::on-event",
        json!({ "path": "/hooks/github" }),
    )
    .expect("failed to register webhook trigger");

    iii.register_trigger_with_config(
        "polling",
        "custom-triggers::on-event",
        json!({ "url": "https://api.example.com/status", "interval_ms": 60000 }),
    )
    .expect("failed to register polling trigger");

    tokio::signal::ctrl_c().await.ok();
    iii.shutdown();
    Ok(())
}
