//! Contract tests for the SDK-owned [`TriggerAction`] wire shape.
//!
//! Queue delivery, ordering, retries, and durable fan-out are covered by the
//! standalone queue worker. The SDK only owns serialization of the action
//! sent to the engine.

use iii_sdk::TriggerAction;
use serde_json::json;

#[test]
fn enqueue_serializes_to_type_and_queue() {
    let action = TriggerAction::Enqueue {
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
        serde_json::to_value(TriggerAction::Void).unwrap(),
        json!({ "type": "void" })
    );
}
