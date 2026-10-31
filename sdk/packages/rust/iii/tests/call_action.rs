//! Contract tests for the SDK-owned [`CallAction`] wire shape.
//!
//! Queue delivery, ordering, retries, and durable fan-out are covered by the
//! standalone queue worker. The SDK only owns serialization of the action
//! sent to the engine.

use iii_sdk::CallAction;
use serde_json::json;

#[test]
fn enqueue_serializes_to_type_and_queue() {
    let action = CallAction::Enqueue {
        queue: "orders".to_string(),
    };

    assert_eq!(
        serde_json::to_value(action).unwrap(),
        json!({
            "type": "enqueue",
            "queue": "orders",
        })
    );
}

#[test]
fn void_serializes_to_type() {
    assert_eq!(
        serde_json::to_value(CallAction::Void).unwrap(),
        json!({ "type": "void" })
    );
}

/// `TriggerAction` is `CallAction` under its 0.25 name.
#[test]
#[allow(deprecated)]
fn trigger_action_is_call_action() {
    let action: CallAction = iii_sdk::TriggerAction::Void;
    assert_eq!(
        serde_json::to_value(action).unwrap(),
        json!({ "type": "void" })
    );
}
