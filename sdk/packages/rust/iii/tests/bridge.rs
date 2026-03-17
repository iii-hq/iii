//! Integration tests for bridge operations.
//!
//! Requires a running III engine. Set III_URL or use ws://localhost:49134 default.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iii_sdk::{FunctionInfo, RegisterFunctionMessage, TriggerAction, TriggerRequest};

#[tokio::test]
async fn connect_successfully() {
    let iii = common::shared_iii();

    let functions: Vec<FunctionInfo> = iii.list_functions().await.expect("list_functions");
    // Just verify it returns a valid list (may be empty if no functions registered)
    let _ = functions;
}

#[tokio::test]
async fn register_and_invoke_function() {
    let iii = common::shared_iii();

    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    let fn_ref = iii.register_function(
        RegisterFunctionMessage {
            id: "test.bridge.rs.echo".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let received = received_clone.clone();
            async move {
                received.lock().await.push(input.clone());
                Ok(json!({ "echoed": input }))
            }
        },
    );

    common::settle().await;

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test.bridge.rs.echo".to_string(),
            payload: json!({"message": "hello"}),
            action: None,
            timeout_ms: None,
        })
        .await
        .expect("trigger");

    assert_eq!(result["echoed"]["message"], "hello");
    assert_eq!(received.lock().await[0]["message"], "hello");

    fn_ref.unregister();
}

#[tokio::test]
async fn invoke_function_fire_and_forget() {
    let iii = common::shared_iii();

    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx = Arc::new(Mutex::new(Some(tx)));

    let fn_ref = iii.register_function(
        RegisterFunctionMessage {
            id: "test.bridge.rs.receiver".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let received = received_clone.clone();
            let tx = tx.clone();
            async move {
                received.lock().await.push(input);
                if let Some(sender) = tx.lock().await.take() {
                    let _ = sender.send(());
                }
                Ok(json!({}))
            }
        },
    );

    common::settle().await;

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test.bridge.rs.receiver".to_string(),
            payload: json!({"value": 42}),
            action: Some(TriggerAction::Void),
            timeout_ms: None,
        })
        .await
        .expect("void trigger");

    assert!(result.is_null());

    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("timeout waiting for fire-and-forget")
        .expect("channel error");

    assert_eq!(received.lock().await[0]["value"], 42);

    fn_ref.unregister();
}

#[tokio::test]
async fn list_registered_functions() {
    let iii = common::shared_iii();

    let fn1 = iii.register_function(
        RegisterFunctionMessage {
            id: "test.bridge.rs.list.func1".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        |_: Value| async move { Ok(json!({})) },
    );
    let fn2 = iii.register_function(
        RegisterFunctionMessage {
            id: "test.bridge.rs.list.func2".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        |_: Value| async move { Ok(json!({})) },
    );

    common::settle().await;

    let functions: Vec<FunctionInfo> = iii.list_functions().await.expect("list_functions");
    let ids: Vec<&str> = functions.iter().map(|f| f.function_id.as_str()).collect();

    assert!(ids.contains(&"test.bridge.rs.list.func1"));
    assert!(ids.contains(&"test.bridge.rs.list.func2"));

    fn1.unregister();
    fn2.unregister();
}

#[tokio::test]
async fn reject_non_existent_function() {
    let iii = common::shared_iii();

    let result = iii
        .trigger(TriggerRequest {
            function_id: "nonexistent.function.rs".to_string(),
            payload: json!({}),
            action: None,
            timeout_ms: Some(2000),
        })
        .await;

    assert!(result.is_err());
}
