/// Pattern: State Reactions
/// Comparable to: Firebase onSnapshot, Convex mutations
///
/// Register functions that fire automatically when state changes
/// in a given scope. Optionally filter with a condition function
/// that returns a boolean.

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    builtin_triggers::*, IIITrigger, Logger,
};
use serde_json::json;
use std::time::Duration;

use serde;
use schemars;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct StateEvent {
    new_value: Option<serde_json::Value>,
    old_value: Option<serde_json::Value>,
    key: String,
    event_type: String,
}

fn main() {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // ---
    // Basic state reaction - fires on ANY change in the 'orders' scope
    // The handler receives: { new_value, old_value, key, event_type }
    //   event_type: "set" | "update" | "delete"
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("reactions::order-audit-log", move |event: StateEvent| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                let action = if event.old_value.is_none() {
                    "created"
                } else if event.new_value.is_none() {
                    "deleted"
                } else {
                    "updated"
                };

                logger.info("Order changed", &json!({ "key": event.key, "action": action, "event_type": event.event_type }));

                let audit_id = format!("audit-{}", chrono::Utc::now().timestamp_millis());
                iii.trigger(TriggerRequest {
                    function_id: "state::set".into(),
                    payload: json!({
                        "scope": "order-audit",
                        "key": audit_id,
                        "value": {
                            "auditId": audit_id,
                            "orderKey": event.key,
                            "action": action,
                            "event_type": event.event_type,
                            "before": event.old_value,
                            "after": event.new_value,
                            "timestamp": chrono::Utc::now().to_rfc3339(),
                        },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({ "auditId": audit_id, "action": action }))
            }
        })
        .description("Audit log for order changes"),
    );

    iii.register_trigger(
        IIITrigger::State(StateTriggerConfig::new().scope("orders"))
            .for_function("reactions::order-audit-log"),
    )
    .expect("failed");

    // ---
    // Conditional reaction - only fires when condition function returns true
    // The condition function receives the same event and must return a boolean.
    // ---
    iii.register_function(
        RegisterFunction::new("reactions::high-value-alert-condition", |event: StateEvent| -> Result<serde_json::Value, String> {
            let is_high_value = event
                .new_value
                .as_ref()
                .and_then(|v| v["total"].as_f64())
                .map(|total| total > 1000.0)
                .unwrap_or(false);
            Ok(json!(is_high_value))
        })
        .description("Condition: order total exceeds $1000"),
    );

    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("reactions::high-value-alert", move |event: StateEvent| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                let total = event.new_value.as_ref().and_then(|v| v["total"].as_f64()).unwrap_or(0.0);
                let customer = event.new_value.as_ref().and_then(|v| v["customer"].clone().into());

                logger.info("High-value order detected", &json!({ "key": event.key, "total": total }));

                iii.trigger(TriggerRequest {
                    function_id: "alerts::notify-manager".into(),
                    payload: json!({
                        "type": "high-value-order",
                        "orderId": event.key,
                        "total": total,
                        "customer": customer,
                    }),
                    action: Some(TriggerAction::Enqueue { queue: "alerts".into() }),
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({ "alerted": true, "orderId": event.key }))
            }
        })
        .description("Alert on high-value orders"),
    );

    iii.register_trigger(
        IIITrigger::State(
            StateTriggerConfig::new()
                .scope("orders")
                .condition("reactions::high-value-alert-condition"),
        )
        .for_function("reactions::high-value-alert"),
    )
    .expect("failed");

    // ---
    // Multiple independent reactions to the same scope
    // Each trigger registers a separate function on the same scope.
    // All registered reactions fire independently on every matching change.
    // ---

    // Reaction 1: Update aggregate metrics
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("reactions::order-metrics", move |event: StateEvent| {
            let iii = iii_clone.clone();
            async move {
                let mut ops = Vec::new();

                if event.new_value.is_some() && event.old_value.is_none() {
                    let total = event.new_value.as_ref().and_then(|v| v["total"].as_f64()).unwrap_or(0.0);
                    ops.push(json!({ "type": "increment", "path": "total_orders", "by": 1 }));
                    ops.push(json!({ "type": "increment", "path": "total_revenue", "by": total }));
                }

                if event.new_value.is_none() && event.old_value.is_some() {
                    let total = event.old_value.as_ref().and_then(|v| v["total"].as_f64()).unwrap_or(0.0);
                    ops.push(json!({ "type": "increment", "path": "total_orders", "by": -1 }));
                    ops.push(json!({ "type": "increment", "path": "total_revenue", "by": -total }));
                }

                if !ops.is_empty() {
                    iii.trigger(TriggerRequest {
                        function_id: "state::update".into(),
                        payload: json!({ "scope": "order-metrics", "key": "global", "ops": ops }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                }

                Ok(json!(null))
            }
        })
        .description("Update aggregate order metrics"),
    );

    iii.register_trigger(
        IIITrigger::State(StateTriggerConfig::new().scope("orders"))
            .for_function("reactions::order-metrics"),
    )
    .expect("failed");

    // Reaction 2: Push live update to connected clients
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("reactions::order-live-feed", move |event: StateEvent| {
            let iii = iii_clone.clone();
            async move {
                let action = if event.old_value.is_none() {
                    "created"
                } else if event.new_value.is_none() {
                    "deleted"
                } else {
                    "updated"
                };

                iii.trigger(TriggerRequest {
                    function_id: "stream::send".into(),
                    payload: json!({
                        "stream_name": "orders-live",
                        "group_id": "dashboard",
                        "id": format!("evt-{}", chrono::Utc::now().timestamp_millis()),
                        "event_type": "order_changed",
                        "data": { "action": action, "key": event.key, "order": event.new_value },
                    }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                })
                .await
                .ok();

                Ok(json!(null))
            }
        })
        .description("Push order changes to live feed"),
    );

    iii.register_trigger(
        IIITrigger::State(StateTriggerConfig::new().scope("orders"))
            .for_function("reactions::order-live-feed"),
    )
    .expect("failed");

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::signal::ctrl_c().await.ok();
    });
    iii.shutdown();
}
