//! A worker-hosted event source whose deliver hook routes each event to one
//! observer and answers its caller with that observer's result: incoming
//! webhooks, each answered by the handler of its URL.

use std::collections::HashMap;

use iii_sdk::runtime::FunctionInfo;
use iii_sdk::{
    Call, CallRequest, Error, IIIClient, Match, ObserveRejection, Observer, RegisterFunction,
    Source,
};
use serde_json::json;

// ── The observers' args and the event, as typed structs ─────────────────

/// What an observer of `example::on-webhook` asks for. The engine validates
/// it against this schema (`INVALID_ARGS`).
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

// ── Setup ───────────────────────────────────────────────────────────────

pub fn setup(iii: &IIIClient) {
    // One handler per URL: a second observer of the same URL is rejected.
    iii.on_observer_added_for("example::on-webhook", {
        let iii = iii.clone();
        move |obs: Observer| {
            let iii = iii.clone();
            async move {
                let WebhookArgs { url, .. } = obs.args()?;
                let taken = iii
                    .observers("example::on-webhook")
                    .iter()
                    .any(|other| other.args_value()["url"] == url.as_str());
                if taken {
                    return Err(ObserveRejection::new(
                        "WEBHOOK_URL_TAKEN",
                        format!("{url} already has a handler"),
                    ));
                }
                println!("[webhook] {} now receives webhooks on {url}", obs.target());
                Ok(())
            }
        }
    })
    // Each webhook goes to the observer of its URL.
    .on_observer_match_for(
        "example::on-webhook",
        Match::keys(
            |obs| obs.args_value()["url"].as_str().into(),
            |call| call.input["url"].as_str().into(),
        ),
    )
    // The webhook's response is the result of its URL's handler, called
    // through the observer. The observer's filters decide whether it gets the
    // webhook at all.
    .on_observer_deliver_for("example::on-webhook", {
        let iii = iii.clone();
        move |call: Call, observers: Vec<Observer>| {
            let iii = iii.clone();
            async move {
                let Some(route) = observers.first() else {
                    return Ok(json!({ "status_code": 404 }));
                };
                if !route.matches(&call) {
                    return Ok(json!({ "status_code": 422 }));
                }
                iii.call(CallRequest::observer(route, call.input.clone()))
                    .await
            }
        }
    });

    // The source: one call per incoming webhook; its handler returns the
    // webhook, which is what observers observe.
    iii.register_function(
        "example::on-webhook",
        RegisterFunction::new(|event: WebhookEvent| Ok::<_, Error>(event))
            .description("Called once per incoming webhook. Observe it with args { url, secret? }.")
            .args_format::<WebhookArgs>()
            .private(),
    );

    // The handler, and the observer that connects it: args for the source's
    // worker, filters for which webhooks reach the handler.
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

/// Called through the observer with the webhook itself: its result is the
/// webhook's response.
fn handle_webhook(event: WebhookEvent) -> Result<serde_json::Value, Error> {
    Ok(json!({
        "status_code": 200,
        "body": { "processed": true, "method": event.method, "body": event.body },
    }))
}

// ── Listing functions and their observers ───────────────────────────────

pub async fn print_function_catalog(iii: &IIIClient) {
    println!("\n--- Listing functions ---");

    let result = iii
        .call(CallRequest::new(
            "engine::functions::list",
            json!({ "include_internal": false }),
        ))
        .await;

    match result {
        Ok(value) => {
            let functions: Vec<FunctionInfo> = serde_json::from_value(
                value
                    .get("functions")
                    .cloned()
                    .unwrap_or(serde_json::Value::Array(vec![])),
            )
            .unwrap_or_default();
            println!("Found {} functions:\n", functions.len());
            for function in &functions {
                println!(
                    "  [{}] {} observer(s){}",
                    function.function_id,
                    function.observer_count,
                    if function.private { ", private" } else { "" }
                );
            }
        }
        Err(e) => {
            println!("Failed to list functions: {e}");
        }
    }
}
