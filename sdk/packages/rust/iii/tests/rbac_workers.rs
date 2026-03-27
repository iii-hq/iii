//! Integration tests for Worker RBAC.
//!
//! Requires a running III engine with WorkerModule RBAC configured on port 49135.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};
use serial_test::serial;

use iii_sdk::{
    AuthInput, AuthResult, IIIConnectionState, InitOptions, MiddlewareFunctionInput,
    RegisterFunction, RegisterFunctionMessage, TriggerRequest, register_worker,
};

static RBAC_AUTH_CALLS: OnceLock<Arc<Mutex<Vec<AuthInput>>>> = OnceLock::new();
static RBAC_FUNCS_REGISTERED: OnceLock<()> = OnceLock::new();

fn auth_calls() -> &'static Arc<Mutex<Vec<AuthInput>>> {
    RBAC_AUTH_CALLS.get_or_init(|| Arc::new(Mutex::new(Vec::new())))
}

fn ew_url() -> String {
    std::env::var("III_RBAC_WORKER_URL").unwrap_or_else(|_| "ws://localhost:49135".to_string())
}

fn ensure_functions_registered() {
    RBAC_FUNCS_REGISTERED.get_or_init(|| {
        let iii = common::shared_iii();
        let mut refs = Vec::new();
        let auth_calls = auth_calls().clone();

        refs.push(iii.register_function(RegisterFunction::new_async(
            "test::rbac-worker::auth",
            move |auth_input: AuthInput| {
                let auth_calls = auth_calls.clone();

                async move {
                    let token = auth_input.headers.get("x-test-token").cloned();
                    auth_calls.lock().unwrap().push(auth_input);

                    match token.as_deref() {
                        None => Ok(AuthResult {
                            allowed_functions: vec![],
                            forbidden_functions: vec![],
                            allowed_trigger_types: None,
                            allow_trigger_type_registration: false,
                            context: json!({ "role": "anonymous", "user_id": "anonymous" }),
                        }),
                        Some("valid-token") => Ok(AuthResult {
                            allowed_functions: vec!["test::ew::valid-token-echo".to_string()],
                            forbidden_functions: vec![],
                            allowed_trigger_types: None,
                            allow_trigger_type_registration: false,
                            context: json!({ "role": "admin", "user_id": "user-1" }),
                        }),
                        Some("restricted-token") => Ok(AuthResult {
                            allowed_functions: vec![],
                            forbidden_functions: vec!["test::ew::echo".to_string()],
                            allowed_trigger_types: None,
                            allow_trigger_type_registration: false,
                            context: json!({ "role": "restricted", "user_id": "user-2" }),
                        }),
                        _ => Err(iii_sdk::IIIError::Handler("invalid token".to_string())),
                    }
                }
            },
        )));

        refs.push(iii.register_function(RegisterFunction::new_async(
            "test::rbac-worker::middleware",
            |input: MiddlewareFunctionInput| {
                let iii = common::shared_iii().clone();
                async move {
                    let mut enriched = input.payload.as_object().cloned().unwrap_or_default();
                    enriched.insert("_intercepted".to_string(), json!(true));
                    enriched.insert(
                        "_caller".to_string(),
                        json!(
                            input
                                .context
                                .get("user_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                        ),
                    );

                    iii.trigger(TriggerRequest {
                        function_id: input.function_id,
                        payload: json!(enriched),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                }
            },
        )));

        refs.push(iii.register_function((
            RegisterFunctionMessage::with_id("test::ew::public::echo".to_string()),
            |input: Value| async move { Ok(json!({ "echoed": input })) },
        )));

        refs.push(iii.register_function((
            RegisterFunctionMessage::with_id("test::ew::valid-token-echo".to_string()),
            |input: Value| async move { Ok(json!({ "echoed": input, "valid_token": true })) },
        )));

        let mut meta_msg = RegisterFunctionMessage::with_id("test::ew::meta-public".to_string());
        meta_msg.metadata = Some(json!({ "ew_public": true }));
        refs.push(iii.register_function((meta_msg, |input: Value| async move {
            Ok(json!({ "meta_echoed": input }))
        })));

        refs.push(iii.register_function((
            RegisterFunctionMessage::with_id("test::ew::private".to_string()),
            |_input: Value| async move { Ok(json!({ "private": true })) },
        )));
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
    assert_eq!(
        iii_client.get_connection_state(),
        IIIConnectionState::Connected
    );

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
        assert_eq!(calls[0].headers["x-test-token"], "valid-token");
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
