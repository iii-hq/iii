/// Pattern: Trigger Conditions
/// Comparable to: Event filters, guard clauses, conditional routing
///
/// A trigger condition is a regular function that returns a boolean. When
/// attached to a trigger via condition_function_id, the engine calls the
/// condition first - if it returns true the handler runs, otherwise the
/// event is silently skipped. The condition receives the same event data
/// as the handler.

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    builtin_triggers::*, IIITrigger, Logger,
};
use serde_json::json;
use chrono::Datelike;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct StateChangeEvent {
    new_value: Option<serde_json::Value>,
    old_value: Option<serde_json::Value>,
    key: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct HttpRequestEvent {
    headers: Option<std::collections::HashMap<String, String>>,
    path: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct QueueEvent {
    event_type: Option<String>,
    order_id: Option<String>,
}

fn main() {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // ---
    // Example 1 - State trigger with a high-value order condition
    // Only fires the handler when the order total exceeds $500.
    // ---

    iii.register_function(
        RegisterFunction::new("conditions::is-high-value", |data: StateChangeEvent| -> Result<serde_json::Value, String> {
            let is_high = data
                .new_value
                .as_ref()
                .and_then(|v| v["total"].as_f64())
                .map(|total| total > 500.0)
                .unwrap_or(false);
            Ok(json!(is_high))
        })
        .description("Condition: order total exceeds $500"),
    );

    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("orders::flag-high-value", move |data: StateChangeEvent| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                let total = data.new_value.as_ref().and_then(|v| v["total"].as_f64()).unwrap_or(0.0);
                logger.info("High-value order detected", &json!({ "key": data.key, "total": total }));

                iii.trigger(TriggerRequest {
                    function_id: "state::update".into(),
                    payload: json!({
                        "scope": "orders",
                        "key": data.key,
                        "ops": [{ "type": "set", "path": "flagged", "value": true }],
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({ "flagged": true, "order_id": data.key }))
            }
        })
        .description("Flag high-value orders"),
    );

    iii.register_trigger(
        IIITrigger::State(
            StateTriggerConfig::new()
                .scope("orders")
                .condition("conditions::is-high-value"),
        )
        .for_function("orders::flag-high-value"),
    )
    .expect("failed");

    // ---
    // Example 2 - HTTP trigger with request validation condition
    // Rejects requests missing a required API key header.
    // ---

    iii.register_function(
        RegisterFunction::new("conditions::has-api-key", |data: HttpRequestEvent| -> Result<serde_json::Value, String> {
            let has_key = data
                .headers
                .as_ref()
                .and_then(|h| h.get("x-api-key"))
                .map(|k| !k.is_empty())
                .unwrap_or(false);
            Ok(json!(has_key))
        })
        .description("Condition: request has x-api-key header"),
    );

    iii.register_function(
        RegisterFunction::new("api::protected-endpoint", |data: HttpRequestEvent| -> Result<serde_json::Value, String> {
            let logger = Logger::new();
            logger.info("Authenticated request", &json!({ "path": data.path }));
            Ok(json!({ "message": "Access granted" }))
        })
        .description("Protected API endpoint"),
    );

    iii.register_trigger(
        IIITrigger::Http(
            HttpTriggerConfig::new("/api/protected")
                .method(HttpMethod::Get)
                .condition("conditions::has-api-key"),
        )
        .for_function("api::protected-endpoint"),
    )
    .expect("failed");

    // ---
    // Example 3 - Queue trigger with event type filter condition
    // Only processes messages whose `event_type` is "order.placed".
    // ---

    iii.register_function(
        RegisterFunction::new("conditions::is-order-placed", |data: QueueEvent| -> Result<serde_json::Value, String> {
            let is_placed = data.event_type.as_deref() == Some("order.placed");
            Ok(json!(is_placed))
        })
        .description("Condition: event_type is order.placed"),
    );

    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("orders::on-placed", move |data: QueueEvent| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                let order_id = data.order_id.clone().unwrap_or_default();
                logger.info("Processing order.placed event", &json!({ "orderId": order_id }));

                iii.trigger(TriggerRequest {
                    function_id: "orders::fulfill".into(),
                    payload: json!({ "order_id": order_id }),
                    action: Some(TriggerAction::Enqueue { queue: "fulfillment".into() }),
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({ "processed": true, "order_id": order_id }))
            }
        })
        .description("Handle order.placed events"),
    );

    iii.register_function(
        RegisterFunction::new("orders::fulfill", |data: serde_json::Value| -> Result<serde_json::Value, String> {
            let logger = Logger::new();
            logger.info("Fulfilling order", &json!({ "orderId": data["order_id"] }));
            Ok(json!({ "fulfilled": true }))
        })
        .description("Fulfill an order"),
    );

    iii.register_trigger(
        IIITrigger::Queue(
            QueueTriggerConfig::new("order-events")
                .condition("conditions::is-order-placed"),
        )
        .for_function("orders::on-placed"),
    )
    .expect("failed");

    // ---
    // Example 4 - Condition with shared data
    // The condition and handler receive identical event data, so a condition can
    // enrich or validate any field the handler will use.
    // ---

    iii.register_function(
        RegisterFunction::new("conditions::is-weekday", |_: serde_json::Value| -> Result<serde_json::Value, String> {
            let day = chrono::Utc::now().weekday().num_days_from_monday();
            Ok(json!(day < 5))
        })
        .description("Condition: current day is a weekday"),
    );

    iii.register_function(
        RegisterFunction::new("reports::weekday-digest", |_: serde_json::Value| -> Result<serde_json::Value, String> {
            let logger = Logger::new();
            logger.info("Running weekday digest", &json!({}));
            Ok(json!({ "generated": true }))
        })
        .description("Generate weekday digest report"),
    );

    iii.register_trigger(
        IIITrigger::Cron(
            CronTriggerConfig::new("0 8 * * *")
                .condition("conditions::is-weekday"),
        )
        .for_function("reports::weekday-digest"),
    )
    .expect("failed");

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::signal::ctrl_c().await.ok();
    });
    iii.shutdown();
}
