//! Preserve present null, absent results, and size-error correlation on the wire.
mod common;

use std::time::Duration;

use common::mock_engine::{MockEngine, count_type};
use iii_sdk::{Error, InitOptions, RegisterFunction, register_worker};
use serde_json::{Value, json};

// Outbound JSON envelope: 16 MiB (16,777,216 bytes).
const JSON_FRAME_LIMIT_BYTES: usize = 16 * 1024 * 1024;

#[tokio::test]
async fn null_absent_and_oversized_results_preserve_wire_semantics() {
    let mock = MockEngine::start().await;
    let iii = register_worker(mock.url(), InitOptions::default());
    let _function = iii.register_function(
        "test::result_wire",
        RegisterFunction::new_async(|input: Value| async move {
            match input.as_str().unwrap() {
                "null" => Ok(Value::Null),
                "absent" => Err(Error::Remote {
                    code: "expected_error".into(),
                    message: "No result".into(),
                    stacktrace: None,
                }),
                "oversized" => Ok(Value::String("x".repeat(JSON_FRAME_LIMIT_BYTES))),
                _ => unreachable!(),
            }
        }),
    );
    mock.wait_for(
        |msgs| count_type(msgs, "registerfunction") >= 1,
        Duration::from_secs(5),
    )
    .await;

    for (mode, id) in [
        ("null", "00000000-0000-4000-8000-000000000001"),
        ("absent", "00000000-0000-4000-8000-000000000002"),
        ("oversized", "00000000-0000-4000-8000-000000000003"),
    ] {
        mock.send_to_client(json!({
            "type": "invokefunction",
            "invocation_id": id,
            "function_id": "test::result_wire",
            "data": mode,
        }));
        let msgs = mock
            .wait_for(
                |msgs| {
                    msgs.iter()
                        .any(|msg| msg["type"] == "invocationresult" && msg["invocation_id"] == id)
                },
                Duration::from_secs(5),
            )
            .await;
        let frame = msgs
            .iter()
            .find(|msg| msg["type"] == "invocationresult" && msg["invocation_id"] == id)
            .expect("correlated invocationresult frame");
        assert_eq!(frame["function_id"], "test::result_wire");
        match mode {
            "null" => {
                assert_eq!(frame.get("result"), Some(&Value::Null), "{frame}");
                assert!(frame.get("error").is_none(), "{frame}");
            }
            "absent" => {
                assert!(frame.get("result").is_none(), "{frame}");
                assert_eq!(frame["error"]["code"], "expected_error");
            }
            "oversized" => {
                assert!(frame.get("result").is_none(), "{frame}");
                assert_eq!(frame["error"]["code"], "payload_too_large");
                assert!(frame.get("baggage").is_none());
                assert!(frame.get("traceparent").is_none());
            }
            _ => unreachable!(),
        }
    }
    iii.shutdown_async().await;
}
