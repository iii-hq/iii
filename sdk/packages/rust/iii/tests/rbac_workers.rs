//! Integration tests for Worker RBAC.
//!
//! Requires a running III engine with WorkerModule RBAC configured on port 49135.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};
use serial_test::serial;

use iii_sdk::{InitOptions, RegisterFunctionMessage, TriggerRequest, register_worker};

fn ew_url() -> String {
    std::env::var("III_RBAC_WORKER_URL").unwrap_or_else(|_| "ws://localhost:49135".to_string())
}

static RBAC_AUTH_CALLS: OnceLock<Arc<Mutex<Vec<Value>>>> = OnceLock::new();
static RBAC_FUNCS_REGISTERED: OnceLock<()> = OnceLock::new();

fn auth_calls() -> &'static Arc<Mutex<Vec<Value>>> {
    RBAC_AUTH_CALLS.get_or_init(|| Arc::new(Mutex::new(Vec::new())))
}

fn ensure_functions_registered() {
    RBAC_FUNCS_REGISTERED.get_or_init(|| {
        let iii = common::shared_iii();
        let auth_calls = auth_calls().clone();

        iii.register_function((
            RegisterFunctionMessage::with_id("test::rbac-worker::auth".to_string()),
            move |input: Value| {
                let auth_calls = auth_calls.clone();
                async move {
                    auth_calls.lock().unwrap().push(input.clone());

                    let token = input
                        .get("headers")
                        .and_then(|h| h.get("x-test-token"))
                        .and_then(|v| v.as_str());

                    match token {
                        None => Ok(json!({
                            "allowed_functions": [],
                            "forbidden_functions": [],
                            "context": { "role": "anonymous", "user_id": "anonymous" }
                        })),
                        Some("valid-token") => Ok(json!({
                            "allowed_functions": ["test::ew::valid-token-echo"],
                            "forbidden_functions": [],
                            "context": { "role": "admin", "user_id": "user-1" }
                        })),
                        Some("restricted-token") => Ok(json!({
                            "allowed_functions": [],
                            "forbidden_functions": ["test::ew::echo"],
                            "context": { "role": "restricted", "user_id": "user-2" }
                        })),
                        _ => Err(iii_sdk::IIIError::Handler("invalid token".to_string())),
                    }
                }
            },
        ));

        iii.register_function((
            RegisterFunctionMessage::with_id("test::rbac-worker::middleware".to_string()),
            move |input: Value| {
                let iii = common::shared_iii().clone();
                async move {
                    let function_id = input["function_id"].as_str().unwrap().to_string();
                    let payload = input["payload"].clone();
                    let context = input["context"].clone();

                    let mut enriched = payload.as_object().cloned().unwrap_or_default();
                    enriched.insert("_intercepted".to_string(), json!(true));
                    enriched.insert(
                        "_caller".to_string(),
                        json!(
                            context
                                .get("user_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                        ),
                    );

                    let result = iii
                        .trigger(TriggerRequest {
                            function_id,
                            payload: json!(enriched),
                            action: None,
                            timeout_ms: None,
                        })
                        .await?;

                    Ok(result)
                }
            },
        ));

        iii.register_function((
            RegisterFunctionMessage::with_id("test::ew::public::echo".to_string()),
            |input: Value| async move { Ok(json!({ "echoed": input })) },
        ));

        iii.register_function((
            RegisterFunctionMessage::with_id("test::ew::valid-token-echo".to_string()),
            |input: Value| async move { Ok(json!({ "echoed": input, "valid_token": true })) },
        ));

        let mut meta_msg = RegisterFunctionMessage::with_id("test::ew::meta-public".to_string());
        meta_msg.metadata = Some(json!({ "ew_public": true }));
        iii.register_function((meta_msg, |input: Value| async move {
            Ok(json!({ "meta_echoed": input }))
        }));

        iii.register_function((
            RegisterFunctionMessage::with_id("test::ew::private".to_string()),
            |_input: Value| async move { Ok(json!({ "private": true })) },
        ));
    });
}

// --- RBAC Workers ---

#[tokio::test(flavor = "current_thread")]
#[serial]
async fn should_return_auth_result_for_valid_token() {
    ensure_functions_registered();
    auth_calls().lock().unwrap().clear();
    common::settle().await;
    tokio::time::sleep(Duration::from_millis(700)).await;

    let mut headers = HashMap::new();
    headers.insert("x-test-token".to_string(), "valid-token".to_string());

    let iii_client = register_worker(
        &ew_url(),
        InitOptions {
            headers: Some(headers),
            ..Default::default()
        },
    );

    tokio::time::sleep(Duration::from_millis(500)).await;

    let result = iii_client
        .trigger(TriggerRequest {
            function_id: "test::ew::valid-token-echo".to_string(),
            payload: json!({ "msg": "hello" }),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("trigger should succeed");

    assert_eq!(result["valid_token"], true);
    assert_eq!(result["echoed"]["msg"], "hello");
    assert_eq!(result["echoed"]["_caller"], "user-1");

    {
        let calls = auth_calls().lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["headers"]["x-test-token"], "valid-token");
    }

    iii_client.shutdown_async().await;
}

#[tokio::test(flavor = "current_thread")]
#[serial]
async fn should_return_error_for_private_function() {
    ensure_functions_registered();
    common::settle().await;
    tokio::time::sleep(Duration::from_millis(700)).await;

    let mut headers = HashMap::new();
    headers.insert("x-test-token".to_string(), "valid-token".to_string());

    let iii_client = register_worker(
        &ew_url(),
        InitOptions {
            headers: Some(headers),
            ..Default::default()
        },
    );

    tokio::time::sleep(Duration::from_millis(500)).await;

    let result = iii_client
        .trigger(TriggerRequest {
            function_id: "test::ew::private".to_string(),
            payload: json!({ "msg": "hello" }),
            action: None,
            timeout_ms: None,
        })
        .await;

    assert!(result.is_err(), "triggering a private function should fail");

    iii_client.shutdown_async().await;
}

#[tokio::test(flavor = "current_thread")]
#[serial]
async fn should_return_forbidden_functions_for_restricted_token() {
    ensure_functions_registered();
    common::settle().await;
    tokio::time::sleep(Duration::from_millis(700)).await;

    let mut headers = HashMap::new();
    headers.insert("x-test-token".to_string(), "restricted-token".to_string());

    let iii_client = register_worker(
        &ew_url(),
        InitOptions {
            headers: Some(headers),
            ..Default::default()
        },
    );

    tokio::time::sleep(Duration::from_millis(500)).await;

    let result = iii_client
        .trigger(TriggerRequest {
            function_id: "test::ew::echo".to_string(),
            payload: json!({ "msg": "hello" }),
            action: None,
            timeout_ms: None,
        })
        .await;

    assert!(
        result.is_err(),
        "triggering a forbidden function should fail"
    );

    iii_client.shutdown_async().await;
}
