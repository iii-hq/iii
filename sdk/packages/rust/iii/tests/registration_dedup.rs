//! Wire-level tests for the SDK's registration replay logic.
//!
//! These tests do NOT require a running engine. They use an in-process
//! WebSocket mock (see `tests/common/mock_engine.rs`) and assert on the
//! exact frames the SDK emits during connect, reconnect, and post-connect
//! registration calls.
//!
//! The bug fixed by the registration drain: prior to the fix, every
//! `register_*` call queued a register frame in the outbound mpsc channel
//! AND inserted into the in-memory map. On the first connection, the
//! connection loop replayed the in-memory map (frame #1) and then the
//! select! loop drained the leftover queued copies (frame #2 — duplicate).

mod common;

use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use iii_sdk::{
    III, IIIConnectionState, InitOptions, RegisterFunction, RegisterServiceMessage,
    RegisterTriggerInput, RegisterTriggerType, TriggerConfig, TriggerHandler, register_worker,
};

use common::mock_engine::{MockEngine, count_register, count_type};

#[derive(Deserialize, JsonSchema)]
struct GreetInput {
    name: String,
}

fn greet(input: GreetInput) -> Result<String, String> {
    Ok(format!("Hello, {}", input.name))
}

#[derive(Deserialize, JsonSchema)]
struct EchoInput {
    payload: Value,
}

fn echo(input: EchoInput) -> Result<Value, String> {
    Ok(input.payload)
}

struct NoopHandler;

#[async_trait::async_trait]
impl TriggerHandler for NoopHandler {
    async fn register_trigger(&self, _: TriggerConfig) -> Result<(), iii_sdk::IIIError> {
        Ok(())
    }
    async fn unregister_trigger(&self, _: TriggerConfig) -> Result<(), iii_sdk::IIIError> {
        Ok(())
    }
}

/// Wait until `predicate` is satisfied, then sleep briefly and confirm it
/// still holds — guards against late-arriving duplicates.
async fn assert_stable(
    mock: &MockEngine,
    predicate: impl Fn(&[Value]) -> bool + Copy,
    grace: Duration,
    timeout: Duration,
) -> Vec<Value> {
    let _ = mock.wait_for(predicate, timeout).await;
    tokio::time::sleep(grace).await;
    let snap = mock.received_messages();
    assert!(
        predicate(&snap),
        "predicate stopped holding after grace period: {snap:#?}"
    );
    snap
}

async fn shutdown(iii: &III) {
    iii.shutdown_async().await;
}

#[tokio::test]
async fn single_function_before_connect_sends_one_registerfunction() {
    let mock = MockEngine::start().await;
    let iii = register_worker(mock.url(), InitOptions::default());

    iii.register_function(RegisterFunction::new("dedup::greet", greet));

    let msgs = assert_stable(
        &mock,
        |msgs| count_register(msgs, "registerfunction", "dedup::greet") >= 1,
        Duration::from_millis(400),
        Duration::from_secs(5),
    )
    .await;

    let count = count_register(&msgs, "registerfunction", "dedup::greet");
    assert_eq!(
        count, 1,
        "expected exactly one RegisterFunction frame, got {count}.\nframes: {msgs:#?}"
    );

    shutdown(&iii).await;
}

#[tokio::test]
async fn multiple_functions_before_connect_each_sent_once() {
    let mock = MockEngine::start().await;
    let iii = register_worker(mock.url(), InitOptions::default());

    iii.register_function(RegisterFunction::new("dedup::a", greet));
    iii.register_function(RegisterFunction::new("dedup::b", echo));
    iii.register_function(RegisterFunction::new("dedup::c", greet));

    let msgs = assert_stable(
        &mock,
        |msgs| {
            count_register(msgs, "registerfunction", "dedup::a") >= 1
                && count_register(msgs, "registerfunction", "dedup::b") >= 1
                && count_register(msgs, "registerfunction", "dedup::c") >= 1
        },
        Duration::from_millis(400),
        Duration::from_secs(5),
    )
    .await;

    for id in ["dedup::a", "dedup::b", "dedup::c"] {
        let count = count_register(&msgs, "registerfunction", id);
        assert_eq!(
            count, 1,
            "expected exactly one RegisterFunction frame for {id}, got {count}.\nframes: {msgs:#?}"
        );
    }

    shutdown(&iii).await;
}

#[tokio::test]
async fn mixed_registration_types_each_sent_once() {
    let mock = MockEngine::start().await;
    let iii = register_worker(mock.url(), InitOptions::default());

    iii.register_function(RegisterFunction::new("dedup::mixed::fn", greet));
    iii.register_service(RegisterServiceMessage {
        id: "dedup::mixed::svc".to_string(),
        name: "mixed-svc".to_string(),
        description: None,
        parent_service_id: None,
    });
    let trigger_type = iii.register_trigger_type(RegisterTriggerType::new(
        "dedup::mixed::tt",
        "noop",
        NoopHandler,
    ));
    drop(trigger_type);
    let trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "dedup::mixed::tt".to_string(),
            function_id: "dedup::mixed::fn".to_string(),
            config: json!({"foo": "bar"}),
            metadata: None,
        })
        .expect("register trigger");
    drop(trigger);

    let msgs = assert_stable(
        &mock,
        |msgs| {
            count_register(msgs, "registerfunction", "dedup::mixed::fn") >= 1
                && count_register(msgs, "registerservice", "dedup::mixed::svc") >= 1
                && count_register(msgs, "registertriggertype", "dedup::mixed::tt") >= 1
                && count_type(msgs, "registertrigger") >= 1
        },
        Duration::from_millis(400),
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(
        count_register(&msgs, "registerfunction", "dedup::mixed::fn"),
        1,
        "RegisterFunction sent multiple times: {msgs:#?}"
    );
    assert_eq!(
        count_register(&msgs, "registerservice", "dedup::mixed::svc"),
        1,
        "RegisterService sent multiple times: {msgs:#?}"
    );
    assert_eq!(
        count_register(&msgs, "registertriggertype", "dedup::mixed::tt"),
        1,
        "RegisterTriggerType sent multiple times: {msgs:#?}"
    );
    assert_eq!(
        count_type(&msgs, "registertrigger"),
        1,
        "RegisterTrigger sent multiple times: {msgs:#?}"
    );

    shutdown(&iii).await;
}

#[tokio::test]
async fn register_after_connected_sends_one_frame() {
    let mock = MockEngine::start().await;
    let iii = register_worker(mock.url(), InitOptions::default());

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while iii.get_connection_state() != IIIConnectionState::Connected {
        if tokio::time::Instant::now() >= deadline {
            panic!("never reached Connected state");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let _ = mock
        .wait_for(
            |msgs| count_type(msgs, "invokefunction") >= 1,
            Duration::from_secs(5),
        )
        .await;

    iii.register_function(RegisterFunction::new("dedup::late", greet));

    let msgs = assert_stable(
        &mock,
        |msgs| count_register(msgs, "registerfunction", "dedup::late") >= 1,
        Duration::from_millis(400),
        Duration::from_secs(5),
    )
    .await;

    let count = count_register(&msgs, "registerfunction", "dedup::late");
    assert_eq!(
        count, 1,
        "expected exactly one RegisterFunction frame for late registration, got {count}.\nframes: {msgs:#?}"
    );

    shutdown(&iii).await;
}

#[tokio::test]
async fn reconnect_resends_each_registration_once() {
    let mock = MockEngine::start().await;
    let iii = register_worker(mock.url(), InitOptions::default());

    iii.register_function(RegisterFunction::new("dedup::reconnect::a", greet));
    iii.register_function(RegisterFunction::new("dedup::reconnect::b", echo));

    let _ = assert_stable(
        &mock,
        |msgs| {
            count_register(msgs, "registerfunction", "dedup::reconnect::a") >= 1
                && count_register(msgs, "registerfunction", "dedup::reconnect::b") >= 1
        },
        Duration::from_millis(400),
        Duration::from_secs(5),
    )
    .await;

    mock.clear();
    mock.close_active_connection();

    let msgs = assert_stable(
        &mock,
        |msgs| {
            count_register(msgs, "registerfunction", "dedup::reconnect::a") >= 1
                && count_register(msgs, "registerfunction", "dedup::reconnect::b") >= 1
        },
        Duration::from_millis(500),
        Duration::from_secs(10),
    )
    .await;

    for id in ["dedup::reconnect::a", "dedup::reconnect::b"] {
        let count = count_register(&msgs, "registerfunction", id);
        assert_eq!(
            count, 1,
            "expected exactly one RegisterFunction frame for {id} on reconnect, got {count}.\nframes: {msgs:#?}"
        );
    }

    shutdown(&iii).await;
}
