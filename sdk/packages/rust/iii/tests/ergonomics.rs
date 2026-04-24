//! Unit tests for the small ergonomic helpers on protocol types.
//! These do not touch the engine — pure struct construction.

use iii_sdk::{IIIError, TriggerAction, TriggerRequest};
use serde_json::json;

#[test]
fn not_connected_error_is_actionable() {
    let msg = IIIError::NotConnected.to_string();
    assert!(msg.contains("iii is not connected"), "message: {msg}");
    assert!(msg.contains("engine unreachable"), "message: {msg}");
    assert!(msg.contains("register_worker"), "message: {msg}");
    assert!(msg.contains("ws://localhost:49134"), "message: {msg}");
    assert!(
        msg.contains("https://iii.dev/docs/install"),
        "message: {msg}"
    );
}

#[test]
fn trigger_request_new_sets_required_fields_and_defaults() {
    let req = TriggerRequest::new("math::add", json!({ "a": 1, "b": 2 }));
    assert_eq!(req.function_id, "math::add");
    assert_eq!(req.payload, json!({ "a": 1, "b": 2 }));
    assert!(req.action.is_none());
    assert!(req.timeout_ms.is_none());
}

#[test]
fn trigger_request_new_accepts_string_and_str() {
    let a = TriggerRequest::new("greet", json!({}));
    let b = TriggerRequest::new(String::from("greet"), json!({}));
    assert_eq!(a.function_id, b.function_id);
}

#[test]
fn trigger_request_with_action_void() {
    let req = TriggerRequest::new("notify", json!({})).with_action(TriggerAction::Void);
    assert!(matches!(req.action, Some(TriggerAction::Void)));
    assert!(req.timeout_ms.is_none());
}

#[test]
fn trigger_request_with_action_enqueue() {
    let req = TriggerRequest::new("orders::process", json!({ "id": 1 })).with_action(
        TriggerAction::Enqueue {
            queue: "payments".into(),
        },
    );
    match req.action {
        Some(TriggerAction::Enqueue { queue }) => assert_eq!(queue, "payments"),
        _ => panic!("expected Enqueue action"),
    }
}

#[test]
fn trigger_request_with_timeout_ms_sets_field() {
    let req = TriggerRequest::new("slow::job", json!({})).with_timeout_ms(15_000);
    assert_eq!(req.timeout_ms, Some(15_000));
}

#[test]
fn trigger_request_builder_is_chainable() {
    let req = TriggerRequest::new("x", json!({}))
        .with_action(TriggerAction::Void)
        .with_timeout_ms(5_000);
    assert_eq!(req.function_id, "x");
    assert!(matches!(req.action, Some(TriggerAction::Void)));
    assert_eq!(req.timeout_ms, Some(5_000));
}
