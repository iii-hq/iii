//! Synthetic single-frame receive coverage, not a claim of engine envelope growth.
mod common;
use common::mock_engine::{MockEngine, count_type};
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::{InitOptions, RegisterFunction, register_worker};
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};

// Outbound JSON envelope: 16 MiB (16,777,216 bytes).
const JSON_FRAME_LIMIT_BYTES: usize = 16 * 1024 * 1024;

#[tokio::test]
async fn receives_large_invocations_and_results_with_and_without_headers() {
    for headers in [false, true] {
        let mock = MockEngine::start().await;
        let sdk = register_worker(
            mock.url(),
            InitOptions {
                headers: headers.then(|| HashMap::from([("x-size-test".into(), "yes".into())])),
                ..Default::default()
            },
        );
        sdk.register_function(
            "size::length",
            RegisterFunction::new_async(|value: Value| async move {
                Ok(json!(value.as_str().unwrap().len()))
            }),
        );
        let msgs = mock
            .wait_for(
                |msgs| count_type(msgs, "registerfunction") == 1,
                Duration::from_secs(5),
            )
            .await;
        assert_eq!(count_type(&msgs, "registerfunction"), 1);
        let huge = "x".repeat(JSON_FRAME_LIMIT_BYTES + 1);
        for (id, input) in [
            ("00000000-0000-4000-8000-000000000001", huge.as_str()),
            ("00000000-0000-4000-8000-000000000002", "small"),
        ] {
            mock.send_to_client(json!({"type":"invokefunction","invocation_id":id,"function_id":"size::length","data":input}));
            let msgs = mock
                .wait_for(
                    |msgs| {
                        msgs.iter()
                            .any(|m| m["invocation_id"] == id && m["type"] == "invocationresult")
                    },
                    Duration::from_secs(10),
                )
                .await;
            let frame = msgs
                .iter()
                .find(|m| m["invocation_id"] == id && m["type"] == "invocationresult")
                .unwrap();
            assert_eq!(frame["result"], input.len());
            assert_eq!(count_type(&msgs, "registerfunction"), 1);
        }
        let caller = sdk.clone();
        let call = tokio::spawn(async move {
            caller
                .trigger(TriggerRequest {
                    function_id: "remote".into(),
                    payload: json!({}),
                    action: None,
                    timeout_ms: Some(10000),
                })
                .await
        });
        let msgs = mock
            .wait_for(
                |msgs| {
                    msgs.iter()
                        .any(|m| m["function_id"] == "remote" && m["type"] == "invokefunction")
                },
                Duration::from_secs(5),
            )
            .await;
        let request = msgs
            .iter()
            .find(|m| m["function_id"] == "remote" && m["type"] == "invokefunction")
            .unwrap();
        mock.send_to_client(json!({"type":"invocationresult","invocation_id":request["invocation_id"],"function_id":"remote","result":huge}));
        assert_eq!(
            call.await.unwrap().unwrap().as_str().unwrap().len(),
            huge.len()
        );
        assert_eq!(count_type(&mock.received_messages(), "registerfunction"), 1);
        sdk.shutdown_async().await;
    }
}
