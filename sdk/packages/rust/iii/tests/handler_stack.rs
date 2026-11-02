//! A handler runs on the `iii-connection` thread, whose stack is the 8 MiB a
//! process's main thread has: a call routed to a handler that needs a few
//! MiB of stack (a deep recursion in a debug build) is answered. On a 2 MiB
//! thread the process aborts with a stack overflow, so this test has a binary
//! of its own.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::mock_engine::{MockEngine, message_type};
use iii_sdk::{Error, InitOptions, RegisterFunction, register_worker};

/// Recurses `depth` times with 64 KiB of stack per level.
#[inline(never)]
fn deep(depth: u32) -> u64 {
    let frame = std::hint::black_box([depth as u8; 64 * 1024]);
    if depth == 0 {
        return u64::from(frame[0]);
    }
    let below = deep(depth - 1);
    below + u64::from(std::hint::black_box(&frame)[frame.len() - 1])
}

#[tokio::test]
async fn a_handler_has_the_main_threads_stack() {
    let mock = MockEngine::start().await;
    let iii = register_worker(mock.url(), InitOptions::default());
    // 48 levels of 64 KiB: 3 MiB, more than a spawned thread's default 2 MiB.
    iii.register_function(
        "stack::deep",
        RegisterFunction::new(|_: Value| Ok::<_, Error>(json!({ "sum": deep(48) }))),
    );
    mock.wait_for(
        |m| {
            m.iter()
                .any(|f| message_type(f) == Some("registerfunction") && f["id"] == "stack::deep")
        },
        Duration::from_secs(5),
    )
    .await;

    mock.send_to_client(json!({
        "type": "invokefunction",
        "invocation_id": "0b7c1d6e-2f4a-4c8b-9e1d-3a5f7b9c1d2e",
        "function_id": "stack::deep",
        "data": {},
    }));
    let answers = mock
        .wait_for(
            |m| {
                m.iter()
                    .any(|f| message_type(f) == Some("invocationresult"))
            },
            Duration::from_secs(10),
        )
        .await;
    let answer = answers
        .iter()
        .find(|f| message_type(f) == Some("invocationresult"))
        .expect("an answer");
    assert_eq!(
        answer["invocation_id"],
        "0b7c1d6e-2f4a-4c8b-9e1d-3a5f7b9c1d2e"
    );
    assert_eq!(answer["result"]["sum"], json!(48 * 49 / 2), "{answer}");
    iii.shutdown_async().await;
}
