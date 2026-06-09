//! Wire-level tests for handler error mapping (no running engine needed).
//!
//! `IIIError::Remote` is a structured, expected error owned by the handler:
//! its `code` must reach the wire `ErrorBody.code` verbatim and
//! `stacktrace: None` must NOT be backfilled with a dispatch-loop backtrace.
//! Non-`Remote` handler errors keep the legacy `invocation_failed` code with
//! a backfilled backtrace.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::mock_engine::{MockEngine, count_type};
use iii_sdk::{IIIError, InitOptions, RegisterFunction, register_worker};

fn find_result<'a>(msgs: &'a [Value], invocation_id: &str) -> Option<&'a Value> {
    msgs.iter()
        .find(|m| m["type"] == "invocationresult" && m["invocation_id"] == invocation_id)
}

#[tokio::test]
async fn remote_error_passes_code_through_without_backfilled_stacktrace() {
    let mock = MockEngine::start().await;
    let iii = register_worker(mock.url(), InitOptions::default());

    let _f = iii.register_function(
        "test::remote_err",
        RegisterFunction::new_async(|_: Value| async move {
            Err::<Value, _>(IIIError::Remote {
                code: "W105".to_string(),
                message: "{\"code\":\"W105\",\"type\":\"WorkerOpError\"}".to_string(),
                stacktrace: None,
            })
        }),
    );

    mock.wait_for(
        |msgs| count_type(msgs, "registerfunction") >= 1,
        Duration::from_secs(5),
    )
    .await;

    let inv_id = "00000000-0000-4000-8000-000000000001";
    mock.send_to_client(json!({
        "type": "invokefunction",
        "invocation_id": inv_id,
        "function_id": "test::remote_err",
        "data": {}
    }));

    let msgs = mock
        .wait_for(
            |msgs| find_result(msgs, inv_id).is_some(),
            Duration::from_secs(5),
        )
        .await;
    let result = find_result(&msgs, inv_id).expect("invocationresult frame for remote_err");
    let error = &result["error"];
    assert_eq!(
        error["code"], "W105",
        "Remote code must reach the wire: {error}"
    );
    assert_eq!(
        error["message"],
        "{\"code\":\"W105\",\"type\":\"WorkerOpError\"}"
    );
    assert!(
        error.get("stacktrace").is_none_or(Value::is_null),
        "Remote stacktrace: None must not be backfilled with a dispatch backtrace: {error}"
    );

    iii.shutdown_async().await;
}

#[tokio::test]
async fn non_remote_error_keeps_invocation_failed_with_backfilled_stacktrace() {
    let mock = MockEngine::start().await;
    let iii = register_worker(mock.url(), InitOptions::default());

    let _f = iii.register_function(
        "test::handler_err",
        RegisterFunction::new_async(|_: Value| async move {
            Err::<Value, _>(IIIError::Handler("boom".to_string()))
        }),
    );

    mock.wait_for(
        |msgs| count_type(msgs, "registerfunction") >= 1,
        Duration::from_secs(5),
    )
    .await;

    let inv_id = "00000000-0000-4000-8000-000000000002";
    mock.send_to_client(json!({
        "type": "invokefunction",
        "invocation_id": inv_id,
        "function_id": "test::handler_err",
        "data": {}
    }));

    let msgs = mock
        .wait_for(
            |msgs| find_result(msgs, inv_id).is_some(),
            Duration::from_secs(5),
        )
        .await;
    let result = find_result(&msgs, inv_id).expect("invocationresult frame for handler_err");
    let error = &result["error"];
    assert_eq!(error["code"], "invocation_failed");
    assert!(
        error["message"].as_str().unwrap_or("").contains("boom"),
        "handler message preserved: {error}"
    );
    assert!(
        error["stacktrace"].as_str().is_some_and(|s| !s.is_empty()),
        "non-Remote errors keep the backfilled stacktrace: {error}"
    );

    iii.shutdown_async().await;
}
