/**
 * Pattern: Functions & Triggers (Rust)
 * Comparable to: Core primitives of iii
 *
 * Demonstrates every fundamental building block in Rust: registering functions
 * with the RegisterFunction builder, binding triggers of each built-in type
 * (http, durable:subscriber, cron, state, subscribe), cross-function invocation, and
 * fire-and-forget calls.
 *
 * How-to references:
 *   - Functions & Triggers: https://iii.dev/docs/how-to/use-functions-and-triggers
 */

use std::time::Duration;

use iii_sdk::{
    InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    register_worker,
    builtin_triggers::*,
    IIITrigger,
};
use serde_json::json;

// ---------------------------------------------------------------------------
// Typed request structs — derive JsonSchema for auto-generated request format
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ValidateOrderInput {
    order_id: String,
    items: Vec<serde_json::Value>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct FulfillOrderInput {
    order_id: String,
    items: Vec<serde_json::Value>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CreateOrderInput {
    order_id: String,
    items: Vec<serde_json::Value>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct StateChangeEvent {
    key: String,
    value: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn validate_order(input: ValidateOrderInput) -> Result<serde_json::Value, String> {
    if input.order_id.is_empty() || input.items.is_empty() {
        return Ok(json!({ "valid": false, "reason": "Missing order_id or items" }));
    }
    Ok(json!({ "valid": true, "order_id": input.order_id }))
}

fn fulfill_order(input: FulfillOrderInput) -> Result<serde_json::Value, String> {
    Ok(json!({ "fulfilled": true, "order_id": input.order_id }))
}

fn on_status_change(input: StateChangeEvent) -> Result<serde_json::Value, String> {
    Ok(json!({ "notified": true, "key": input.key }))
}

fn on_order_complete(input: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(json!({ "processed": true, "event": input }))
}

fn daily_summary(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(json!({ "generated_at": "now" }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // -----------------------------------------------------------------------
    // 1. Register a simple function with the builder API
    // -----------------------------------------------------------------------
    iii.register_function(
        RegisterFunction::new("orders::validate", validate_order)
            .description("Validate an incoming order"),
    );

    // -----------------------------------------------------------------------
    // 2. HTTP trigger — expose a function as a REST endpoint
    // -----------------------------------------------------------------------
    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/orders/validate").method(HttpMethod::Post))
            .for_function("orders::validate"),
    )
    .expect("failed to register http trigger");

    // -----------------------------------------------------------------------
    // 3. Queue trigger — process items from a named queue
    // -----------------------------------------------------------------------
    iii.register_function(
        RegisterFunction::new("orders::fulfill", fulfill_order)
            .description("Fulfill a validated order"),
    );

    iii.register_trigger(
        IIITrigger::Queue(QueueTriggerConfig::new("fulfillment"))
            .for_function("orders::fulfill"),
    )
    .expect("failed to register queue trigger");

    // -----------------------------------------------------------------------
    // 4. Cron trigger — run a function on a schedule
    // -----------------------------------------------------------------------
    iii.register_function(
        RegisterFunction::new("reports::daily-summary", daily_summary)
            .description("Generate daily summary report"),
    );

    iii.register_trigger(
        IIITrigger::Cron(CronTriggerConfig::new("0 0 9 * * *"))
            .for_function("reports::daily-summary"),
    )
    .expect("failed to register cron trigger");

    // -----------------------------------------------------------------------
    // 5. State trigger — react when a state scope/key changes
    // -----------------------------------------------------------------------
    iii.register_function(
        RegisterFunction::new("orders::on-status-change", on_status_change)
            .description("React to order status changes"),
    );

    iii.register_trigger(
        IIITrigger::State(StateTriggerConfig::new().scope("orders"))
            .for_function("orders::on-status-change"),
    )
    .expect("failed to register state trigger");

    // -----------------------------------------------------------------------
    // 6. Subscribe trigger — listen for pubsub messages on a topic
    // -----------------------------------------------------------------------
    iii.register_function(
        RegisterFunction::new("notifications::on-order-complete", on_order_complete)
            .description("Process order completion events"),
    );

    iii.register_trigger(
        IIITrigger::Subscribe(SubscribeTriggerConfig::new("orders.completed"))
            .for_function("notifications::on-order-complete"),
    )
    .expect("failed to register subscribe trigger");

    // -----------------------------------------------------------------------
    // 7. Cross-function invocation — one function calling another
    // -----------------------------------------------------------------------
    let iii_clone = iii.clone();
    iii.register_function(RegisterFunction::new_async(
        "orders::create",
        move |input: CreateOrderInput| {
            let iii = iii_clone.clone();
            async move {
                // Synchronous call — blocks until validate returns
                let order_id = input.order_id.clone();
                let items = input.items;

                let validation = iii
                    .trigger(TriggerRequest {
                        function_id: "orders::validate".into(),
                        payload: json!({ "order_id": order_id, "items": items }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                if validation["valid"] == false {
                    return Ok(json!({ "error": validation["reason"] }));
                }

                let _ = iii
                    .trigger(TriggerRequest {
                        function_id: "notifications::on-order-complete".into(),
                        payload: json!({ "order_id": order_id.clone() }),
                        action: Some(TriggerAction::Void),
                        timeout_ms: None,
                    })
                    .await;

                iii.trigger(TriggerRequest {
                    function_id: "orders::fulfill".into(),
                    payload: json!({ "order_id": order_id.clone(), "items": items }),
                    action: Some(TriggerAction::Enqueue {
                        queue: "fulfillment".into(),
                    }),
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({ "order_id": order_id, "status": "accepted" }))
            }
        },
    ));

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/orders").method(HttpMethod::Post))
            .for_function("orders::create"),
    )
    .expect("failed to register http trigger");

    // Keep the process alive for event processing
    tokio::time::sleep(Duration::from_secs(u64::MAX)).await;
    iii.shutdown();
    Ok(())
}
