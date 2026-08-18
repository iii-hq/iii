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

async fn invocation_namespace_of(mock: &MockEngine, function_id: &str) -> Option<Value> {
    let frames = mock
        .wait_for(
            |messages| {
                messages.iter().any(|message| {
                    message.get("function_id").and_then(Value::as_str) == Some(function_id)
                })
            },
            Duration::from_secs(3),
        )
        .await;

    frames
        .into_iter()
        .find(|message| message.get("function_id").and_then(Value::as_str) == Some(function_id))
        .and_then(|message| message.get("namespace").cloned())
}

#[tokio::test]
async fn a_trigger_registers_in_the_workers_namespace() {
    let mock = MockEngine::start().await;
    let iii = worker_in(&mock, Some("orders"));

    iii.register_trigger(RegisterTriggerInput::new(
        "cron".to_string(),
        "api::process".to_string(),
        json!({}),
    ))
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
    iii.register_trigger(
        RegisterTriggerInput::new("cron".to_string(), "state::sweep".to_string(), json!({}))
            .in_namespace("default".to_string()),
    )
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

    iii.register_trigger(RegisterTriggerInput::new(
        "cron".to_string(),
        "api::process".to_string(),
        json!({}),
    ))
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
async fn an_engine_invocation_stays_in_default() {
    let mock = MockEngine::start().await;
    let iii = worker_in(&mock, Some("orders"));

    let _ = iii
        .trigger(TriggerRequest {
            function_id: "engine::channels::create".to_string(),
            payload: json!({}),
            action: Some(TriggerAction::Void),
            timeout_ms: None,
        })
        .await;

    assert_eq!(
        invocation_namespace_of(&mock, "engine::channels::create").await,
        Some(json!("default"))
    );
}

#[tokio::test]
async fn an_explicit_namespace_wins_for_an_engine_invocation() {
    let mock = MockEngine::start().await;
    let iii = worker_in(&mock, Some("orders"));

    let _ = iii
        .trigger(
            TriggerRequest {
                function_id: "engine::channels::create".to_string(),
                payload: json!({}),
                action: Some(TriggerAction::Void),
                timeout_ms: None,
            }
            .namespace("sandbox"),
        )
        .await;

    assert_eq!(
        invocation_namespace_of(&mock, "engine::channels::create").await,
        Some(json!("sandbox"))
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

/// A namespace that was declared and left blank is a mistake, not a way to ask
/// for `default`.
///
/// Absent and blank mean opposite things, and read as the same they produce the
/// failure nobody can see: the worker registers in `default`, and since a
/// worker's calls and triggers now follow its namespace, the whole project
/// serves from a place the declaration never named.
///
/// Checked before any connection, so it fails at startup the way `iii compose`
/// refuses `--ns ""`.
mod blank_namespace {
    use iii_sdk::{InitOptions, register_worker};

    #[test]
    #[should_panic(expected = "namespace is empty")]
    fn an_empty_option_is_refused() {
        let _ = register_worker(
            "ws://127.0.0.1:1",
            InitOptions {
                namespace: Some(String::new()),
                ..Default::default()
            },
        );
    }

    #[test]
    #[should_panic(expected = "namespace is empty")]
    fn whitespace_only_is_refused_too() {
        let _ = register_worker(
            "ws://127.0.0.1:1",
            InitOptions {
                namespace: Some("   ".to_string()),
                ..Default::default()
            },
        );
    }

    /// `register_worker` resolved its namespace before handing it over, so the
    /// check never ran on the entry point a caller can reach directly.
    /// A blank namespace is the same mistake wherever it is made.
    #[test]
    #[should_panic(expected = "namespace is empty")]
    fn set_namespace_refuses_a_blank_one_too() {
        let client = iii_sdk::IIIClient::new("ws://127.0.0.1:1");
        client.set_namespace("");
    }

    #[test]
    fn set_namespace_still_takes_a_real_one() {
        // The control, so a run that refuses everything cannot pass.
        let client = iii_sdk::IIIClient::with_metadata(
            "ws://127.0.0.1:1",
            iii_sdk::iii::WorkerMetadata {
                name: "tester".to_string(),
                ..Default::default()
            },
        );
        client.set_namespace("orders");
        assert_eq!(client.namespace().as_deref(), Some("orders"));
    }
}

/// A namespace named and left blank on one call is the same mistake as one
/// declared blank at construction -- and until now each SDK made a different
/// one of it, because `??`, `or_else` and `or` disagree about the empty string.
///
/// An error rather than a panic: unlike `InitOptions.namespace`, a per-call
/// namespace can come from data, and the caller can do something with it.
mod blank_call_namespace {
    use iii_sdk::protocol::{RegisterTriggerInput, TriggerRequest};
    use iii_sdk::{IIIClient, InitOptions, register_worker};
    use serde_json::json;

    fn worker() -> IIIClient {
        register_worker(
            "ws://127.0.0.1:1",
            InitOptions {
                namespace: Some("orders".to_string()),
                ..Default::default()
            },
        )
    }

    #[test]
    fn register_trigger_refuses_it() {
        let err = worker()
            .register_trigger(
                RegisterTriggerInput::new(
                    "cron".to_string(),
                    "api::process".to_string(),
                    json!({}),
                )
                .in_namespace(String::new()),
            )
            .err()
            .expect("a blank namespace is a mistake, not the worker's");
        assert!(err.to_string().contains("namespace is empty"), "{err}");
    }

    #[tokio::test]
    async fn trigger_refuses_it() {
        let err = worker()
            .trigger(
                TriggerRequest {
                    function_id: "api::ping".to_string(),
                    payload: json!({}),
                    action: None,
                    timeout_ms: Some(500),
                }
                .namespace(""),
            )
            .await
            .expect_err("a blank namespace is a mistake");
        assert!(err.to_string().contains("namespace is empty"), "{err}");
    }

    #[test]
    fn an_absent_one_still_inherits() {
        // The control: absent is not blank, and still means this worker's.
        let bound = worker()
            .register_trigger(RegisterTriggerInput::new(
                "cron".to_string(),
                "api::process".to_string(),
                json!({}),
            ))
            .is_ok();
        assert!(bound, "absent is not a mistake");
    }
}
