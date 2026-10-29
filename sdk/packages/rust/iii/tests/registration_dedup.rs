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

use iii_sdk::runtime::IIIConnectionState;
use iii_sdk::{Error, IIIClient, InitOptions, RegisterFunction, Source, register_worker};

use common::mock_engine::{MockEngine, count_register, count_type};

#[derive(Deserialize, JsonSchema)]
struct GreetInput {
    name: String,
}

fn greet(input: GreetInput) -> Result<String, Error> {
    Ok(format!("Hello, {}", input.name))
}

#[derive(Deserialize, JsonSchema)]
struct EchoInput {
    payload: Value,
}

fn echo(input: EchoInput) -> Result<Value, Error> {
    Ok(input.payload)
}

/// The engine's hello, advertising `observe.v2`.
fn hello(mock: &MockEngine) {
    mock.send_to_client(json!({
        "type": "workerregistered", "worker_id": "w-1",
        "features": ["observe.v1", "observe.v2", "local-dispatch.v1"]
    }));
}

fn workers_register_at(msgs: &[Value]) -> Option<usize> {
    msgs.iter()
        .position(|m| m["function_id"] == "engine::workers::register")
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

async fn shutdown(iii: &IIIClient) {
    iii.shutdown_async().await;
}

#[tokio::test]
async fn single_function_before_connect_sends_one_registerfunction() {
    let mock = MockEngine::start().await;
    let iii = register_worker(mock.url(), InitOptions::default());

    iii.register_function("dedup::greet", RegisterFunction::new(greet));

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

    iii.register_function("dedup::a", RegisterFunction::new(greet));
    iii.register_function("dedup::b", RegisterFunction::new(echo));
    iii.register_function("dedup::c", RegisterFunction::new(greet));

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

    iii.register_function("dedup::mixed::fn", RegisterFunction::new(greet));
    iii.register_function(
        "dedup::mixed::private",
        RegisterFunction::new(echo).private(),
    );
    let observer = iii.observe(Source::function("dedup::mixed::fn"), "dedup::mixed::target");

    let _ = mock
        .wait_for(
            |msgs| workers_register_at(msgs).is_some(),
            Duration::from_secs(5),
        )
        .await;
    hello(&mock);

    let msgs = assert_stable(
        &mock,
        |msgs| {
            count_register(msgs, "registerfunction", "dedup::mixed::fn") >= 1
                && count_register(msgs, "registerfunction", "dedup::mixed::private") >= 1
                && count_register(msgs, "observe", observer.id()) >= 1
        },
        Duration::from_millis(400),
        Duration::from_secs(5),
    )
    .await;

    for (ty, id) in [
        ("registerfunction", "dedup::mixed::fn"),
        ("registerfunction", "dedup::mixed::private"),
        ("observe", observer.id()),
    ] {
        assert_eq!(
            count_register(&msgs, ty, id),
            1,
            "{ty} {id} sent more than once: {msgs:#?}"
        );
    }
    let private_at = msgs
        .iter()
        .position(|m| m["type"] == "registerfunction" && m["id"] == "dedup::mixed::private")
        .unwrap();
    assert!(
        workers_register_at(&msgs).unwrap() < private_at,
        "a registration that needs observe.v2 waits for the engine's hello: {msgs:#?}"
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

    iii.register_function("dedup::late", RegisterFunction::new(greet));

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

    iii.register_function("dedup::reconnect::a", RegisterFunction::new(greet));
    iii.register_function("dedup::reconnect::b", RegisterFunction::new(echo));
    iii.register_function(
        "dedup::reconnect::v2",
        RegisterFunction::new(echo).observable(false),
    );
    let all = |msgs: &[Value]| {
        [
            "dedup::reconnect::a",
            "dedup::reconnect::b",
            "dedup::reconnect::v2",
        ]
        .iter()
        .all(|id| count_register(msgs, "registerfunction", id) >= 1)
    };

    let _ = mock
        .wait_for(
            |msgs| workers_register_at(msgs).is_some(),
            Duration::from_secs(5),
        )
        .await;
    hello(&mock);
    let _ = assert_stable(
        &mock,
        all,
        Duration::from_millis(400),
        Duration::from_secs(5),
    )
    .await;

    mock.clear();
    mock.close_active_connection();

    let _ = mock
        .wait_for(
            |msgs| workers_register_at(msgs).is_some(),
            Duration::from_secs(10),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        count_register(
            &mock.received_messages(),
            "registerfunction",
            "dedup::reconnect::v2"
        ),
        0,
        "replayed only after the new connection's hello"
    );
    hello(&mock);
    let msgs = assert_stable(
        &mock,
        all,
        Duration::from_millis(500),
        Duration::from_secs(10),
    )
    .await;

    for id in [
        "dedup::reconnect::a",
        "dedup::reconnect::b",
        "dedup::reconnect::v2",
    ] {
        let count = count_register(&msgs, "registerfunction", id);
        assert_eq!(
            count, 1,
            "expected exactly one RegisterFunction frame for {id} on reconnect, got {count}.\nframes: {msgs:#?}"
        );
    }

    shutdown(&iii).await;
}
