//! A worker's namespace is inherited by what it registers and what it calls.
//!
//! Wire-level, against the in-process mock: no engine needed, and the assertion
//! is the frame the SDK actually emits.
//!
//! The rule these pin down: a namespace declared once, at registration, holds
//! for everything the worker does afterwards. Before it, `register_trigger` and
//! `trigger` both defaulted to the engine's `default` namespace, so a worker in
//! `orders` registered a trigger that fired and resolved nothing, and its calls
//! left its own namespace without saying so.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use iii_sdk::protocol::{RegisterTriggerInput, TriggerAction, TriggerRequest};
use iii_sdk::{IIIClient, InitOptions, register_worker};

use common::mock_engine::{MockEngine, message_type};

fn worker_in(mock: &MockEngine, namespace: Option<&str>) -> IIIClient {
    register_worker(
        mock.url(),
        InitOptions {
            namespace: namespace.map(str::to_string),
            ..Default::default()
        },
    )
}

/// Finds the first frame of `kind` and returns its `namespace`, distinguishing
/// "absent" from "present and null" by returning `Value::Null` only for the
/// latter.
async fn namespace_of(mock: &MockEngine, kind: &str) -> Option<Value> {
    let frames = mock.wait_for_count(1, Duration::from_secs(3)).await;
    let _ = frames;
    mock.received_messages()
        .into_iter()
        .find(|m| message_type(m) == Some(kind))
        .and_then(|m| m.get("namespace").cloned())
}

#[tokio::test]
async fn a_trigger_registers_in_the_workers_namespace() {
    let mock = MockEngine::start().await;
    let iii = worker_in(&mock, Some("orders"));

    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "cron".to_string(),
        function_id: "api::process".to_string(),
        config: json!({}),
        metadata: None,
        namespace: None,
    })
    .expect("registertrigger");

    assert_eq!(
        namespace_of(&mock, "registertrigger").await,
        Some(json!("orders")),
        "an unset namespace means the worker's, not the engine's default"
    );
}

#[tokio::test]
async fn an_explicit_namespace_still_wins() {
    let mock = MockEngine::start().await;
    let iii = worker_in(&mock, Some("orders"));

    // Including `default`: a worker that means the engine's namespace says so,
    // which is the only way to bind a trigger to a builtin from inside a
    // namespace.
    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "cron".to_string(),
        function_id: "state::sweep".to_string(),
        config: json!({}),
        metadata: None,
        namespace: Some("default".to_string()),
    })
    .expect("registertrigger");

    assert_eq!(
        namespace_of(&mock, "registertrigger").await,
        Some(json!("default"))
    );
}

#[tokio::test]
async fn a_worker_without_a_namespace_is_unchanged() {
    let mock = MockEngine::start().await;
    let iii = worker_in(&mock, None);

    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "cron".to_string(),
        function_id: "api::process".to_string(),
        config: json!({}),
        metadata: None,
        namespace: None,
    })
    .expect("registertrigger");

    // Absent, not null: the whole fleet runs this way today and the frame it
    // sends must not change shape.
    assert_eq!(namespace_of(&mock, "registertrigger").await, None);
}

#[tokio::test]
async fn an_invocation_stays_in_the_callers_namespace() {
    let mock = MockEngine::start().await;
    let iii = worker_in(&mock, Some("orders"));

    // Void: fire-and-forget, so the frame goes out without waiting on a reply
    // the mock will never send.
    let _ = iii
        .trigger(TriggerRequest {
            function_id: "api::ping".to_string(),
            payload: json!({}),
            action: Some(TriggerAction::Void),
            timeout_ms: None,
        })
        .await;

    assert_eq!(
        namespace_of(&mock, "invokefunction").await,
        Some(json!("orders"))
    );
}

#[tokio::test]
async fn an_invocation_can_still_name_another_namespace() {
    let mock = MockEngine::start().await;
    let iii = worker_in(&mock, Some("orders"));

    // Reaching an engine builtin from inside a namespace: `configuration` and
    // `engine::*` are compiled into the engine and only ever exist in
    // `default`, so a worker that needs one names it.
    let _ = iii
        .trigger(
            TriggerRequest {
                function_id: "configuration::get".to_string(),
                payload: json!({}),
                action: Some(TriggerAction::Void),
                timeout_ms: None,
            }
            .namespace("default"),
        )
        .await;

    assert_eq!(
        namespace_of(&mock, "invokefunction").await,
        Some(json!("default"))
    );
}

/// The SDK's own registration call must stay in `default`.
///
/// `engine::workers::register` is how a worker announces itself, and it is an
/// engine builtin. Sending it into the worker's own namespace would route the
/// announcement to a namespace the engine does not serve it in, so nothing
/// would ever register. It does not travel through `trigger`, and this is the
/// guard on that.
#[tokio::test]
async fn the_workers_own_registration_is_not_redirected() {
    let mock = MockEngine::start().await;
    let _iii = worker_in(&mock, Some("orders"));
    tokio::time::sleep(Duration::from_millis(800)).await;

    let register = mock
        .received_messages()
        .into_iter()
        .find(|m| m.get("function_id").and_then(Value::as_str) == Some("engine::workers::register"))
        .expect("the worker announces itself");

    assert_eq!(
        register.get("namespace"),
        None,
        "the announcement must reach the engine's own namespace: {register}"
    );
    // The namespace still travels, as data the engine files the worker under.
    assert_eq!(register.pointer("/data/namespace"), Some(&json!("orders")));
}
