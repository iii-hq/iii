/// Pattern: Trigger Actions (Invocation Modes)
/// Comparable to: Synchronous calls, async queues, fire-and-forget messaging
///
/// Every iii.trigger() call can specify an invocation mode via the `action`
/// parameter. There are exactly three modes:
///   1. Synchronous (default) - blocks until the target returns a result.
///   2. Fire-and-forget (TriggerAction::Void) - returns null immediately.
///   3. Enqueue (TriggerAction::Enqueue { queue }) - durably enqueues and
///      returns { messageReceiptId }.
///
/// This file shows each mode in isolation and then combines all three in a
/// realistic checkout workflow.

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    builtin_triggers::*, IIITrigger, Logger,
};
use serde_json::json;
use std::time::Duration;

use serde;
use schemars;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CartInput {
    cart_id: String,
    items: Option<Vec<CartItem>>,
}

#[derive(serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
struct CartItem {
    price: f64,
    qty: i64,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ChargeInput {
    cart_id: String,
    total: f64,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ConfirmationInput {
    email: String,
    order_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SyncCallInput {
    cart_id: String,
    items: Vec<CartItem>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct VoidCallInput {
    email: String,
    order_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct EnqueueCallInput {
    cart_id: String,
    total: f64,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CheckoutInput {
    cart_id: String,
    items: Vec<CartItem>,
    email: String,
}

fn main() {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // ---
    // Helper functions used by the examples below
    // ---
    iii.register_function(
        RegisterFunction::new("checkout::validate-cart", |data: CartInput| -> Result<serde_json::Value, String> {
            let logger = Logger::new();
            logger.info("Validating cart", &json!({ "cartId": data.cart_id }));

            let items = data.items.unwrap_or_default();
            if items.is_empty() {
                return Ok(json!({ "valid": false, "reason": "Cart is empty" }));
            }

            let total: f64 = items.iter().map(|i| i.price * i.qty as f64).sum();
            Ok(json!({ "valid": true, "cart_id": data.cart_id, "total": total }))
        })
        .description("Validate a shopping cart"),
    );

    iii.register_function(
        RegisterFunction::new("checkout::charge-payment", |data: ChargeInput| -> Result<serde_json::Value, String> {
            let logger = Logger::new();
            logger.info("Charging payment", &json!({ "cart_id": data.cart_id, "total": data.total }));
            Ok(json!({ "charged": true, "transaction_id": format!("txn_{}", chrono::Utc::now().timestamp_millis()) }))
        })
        .description("Charge payment for cart"),
    );

    iii.register_function(
        RegisterFunction::new("checkout::send-confirmation", |data: ConfirmationInput| -> Result<serde_json::Value, String> {
            let logger = Logger::new();
            logger.info("Sending order confirmation email", &json!({ "email": data.email }));
            Ok(json!({ "sent": true }))
        })
        .description("Send order confirmation email"),
    );

    // ---
    // Mode 1 - Synchronous (default)
    // Blocks until the target function returns. The result is the function's
    // return value. Use this when the caller needs the result to continue.
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("examples::sync-call", move |data: SyncCallInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                let result = iii
                    .trigger(TriggerRequest {
                        function_id: "checkout::validate-cart".into(),
                        payload: json!({ "cart_id": data.cart_id, "items": data.items }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                logger.info("Sync result received", &json!({ "valid": result["valid"], "total": result["total"] }));
                Ok(result)
            }
        })
        .description("Example: synchronous trigger call"),
    );

    // ---
    // Mode 2 - Fire-and-forget (TriggerAction::Void)
    // Returns null immediately. The target function runs asynchronously and its
    // return value is discarded. Use for side-effects like logging, notifications,
    // or analytics where the caller does not need to wait.
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("examples::void-call", move |data: VoidCallInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                iii.trigger(TriggerRequest {
                    function_id: "checkout::send-confirmation".into(),
                    payload: json!({ "email": data.email, "order_id": data.order_id }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                })
                .await
                .ok();

                logger.info("Confirmation dispatched (fire-and-forget)", &json!({}));
                Ok(json!({ "dispatched": true }))
            }
        })
        .description("Example: fire-and-forget trigger call"),
    );

    // ---
    // Mode 3 - Enqueue (TriggerAction::Enqueue { queue })
    // Durably enqueues the payload onto a named queue. Returns immediately with
    // { messageReceiptId }. The target function processes the message when a
    // worker picks it up. Use for work that must survive crashes and be retried.
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("examples::enqueue-call", move |data: EnqueueCallInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                let receipt = iii
                    .trigger(TriggerRequest {
                        function_id: "checkout::charge-payment".into(),
                        payload: json!({ "cart_id": data.cart_id, "total": data.total }),
                        action: Some(TriggerAction::Enqueue { queue: "payments".into() }),
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                logger.info("Payment enqueued", &json!({ "messageReceiptId": receipt["messageReceiptId"] }));
                Ok(receipt)
            }
        })
        .description("Example: enqueue trigger call"),
    );

    // ---
    // Realistic workflow - Checkout combining all three modes
    //   1. Validate cart  (sync)    - need the result to decide whether to proceed
    //   2. Charge payment (enqueue) - durable, retryable, must not be lost
    //   3. Send email     (void)    - best-effort notification, don't block
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("checkout::process", move |data: CheckoutInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                let validation = iii
                    .trigger(TriggerRequest {
                        function_id: "checkout::validate-cart".into(),
                        payload: json!({ "cart_id": data.cart_id, "items": data.items }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                if validation["valid"] != true {
                    return Ok(json!({ "error": validation["reason"] }));
                }

                let total = validation["total"].as_f64().unwrap_or(0.0);

                let receipt = iii
                    .trigger(TriggerRequest {
                        function_id: "checkout::charge-payment".into(),
                        payload: json!({ "cart_id": data.cart_id, "total": total }),
                        action: Some(TriggerAction::Enqueue { queue: "payments".into() }),
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                logger.info("Payment queued", &json!({ "receiptId": receipt["messageReceiptId"] }));

                iii.trigger(TriggerRequest {
                    function_id: "checkout::send-confirmation".into(),
                    payload: json!({ "email": data.email, "order_id": data.cart_id }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                })
                .await
                .ok();

                Ok(json!({
                    "status": "accepted",
                    "cart_id": data.cart_id,
                    "total": total,
                    "payment_receipt": receipt["messageReceiptId"],
                }))
            }
        })
        .description("Full checkout workflow combining sync, enqueue, and void"),
    );

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/checkout").method(HttpMethod::Post))
            .for_function("checkout::process"),
    )
    .expect("failed");

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::signal::ctrl_c().await.ok();
    });
    iii.shutdown();
}
