//! Wire-level test for the reconnect reattach handshake (iii-hq/iii#1975).
//!
//! On reconnect the SDK must present the engine-assigned worker id from the
//! previous connection (via a `reattach` frame) BEFORE replaying its
//! registration batch, so the engine can retire the old connection and the
//! replay lands on a clean slate instead of racing its cleanup. On the first
//! connect there is no prior id, so no reattach is sent.

mod common;

use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use iii_sdk::{Error, InitOptions, RegisterFunction, register_worker};

use common::mock_engine::{MockEngine, count_register, count_type, message_id, message_type};

#[derive(Deserialize, JsonSchema)]
struct GreetInput {
    name: String,
}

fn greet(input: GreetInput) -> Result<String, Error> {
    Ok(format!("Hello, {}", input.name))
}

#[tokio::test]
async fn reattach_sent_on_reconnect_and_precedes_registration_replay() {
    let mock = MockEngine::start().await;
    let iii = register_worker(mock.url(), InitOptions::default());
    iii.register_function("reattach::fn", RegisterFunction::new(greet));

    // First connect: the function is registered and NO reattach is sent
    // (the SDK has no prior worker id yet).
    let msgs = mock
        .wait_for(
            |m| count_register(m, "registerfunction", "reattach::fn") >= 1,
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(
        count_type(&msgs, "reattach"),
        0,
        "no reattach must be sent on the first connect: {msgs:#?}"
    );

    // The engine assigns this connection an identity + reattach secret.
    mock.send_to_client(
        json!({"type":"workerregistered","worker_id":"w-old","reattach_token":"tok-old"}),
    );
    // Give the receive loop time to store the id before we drop the socket.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Drop the socket; the SDK reconnects and replays. It must now lead with
    // a reattach carrying the previous id.
    mock.clear();
    mock.close_active_connection();

    let msgs = mock
        .wait_for(
            |m| count_register(m, "registerfunction", "reattach::fn") >= 1,
            Duration::from_secs(5),
        )
        .await;

    let reattach_idx = msgs
        .iter()
        .position(|m| message_type(m) == Some("reattach"))
        .expect("a reattach frame must be sent on reconnect");
    let regfn_idx = msgs
        .iter()
        .position(|m| {
            message_type(m) == Some("registerfunction") && message_id(m) == Some("reattach::fn")
        })
        .expect("the function must be replayed on reconnect");

    assert!(
        reattach_idx < regfn_idx,
        "reattach must precede the registration replay: {msgs:#?}"
    );
    assert_eq!(
        msgs[reattach_idx]
            .get("previous_worker_id")
            .and_then(|v| v.as_str()),
        Some("w-old"),
        "reattach must carry the previous connection's worker id"
    );
    assert_eq!(
        msgs[reattach_idx]
            .get("reattach_token")
            .and_then(|v| v.as_str()),
        Some("tok-old"),
        "reattach must carry the previous connection's token"
    );

    iii.shutdown_async().await;
}
