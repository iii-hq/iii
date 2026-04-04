use iii_sdk::{
    IIIError, IntoFunctionHandler, RegisterFunctionMessage, Value, iii_async_fn, iii_fn,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, schemars::JsonSchema)]
struct GreetInput {
    name: String,
}

fn greet(input: GreetInput) -> Result<String, String> {
    Ok(format!("Hello, {}!", input.name))
}

fn test_msg() -> RegisterFunctionMessage {
    RegisterFunctionMessage {
        id: "test".into(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation: None,
    }
}

// ===========================================================================
// SYNC tests
// ===========================================================================

#[tokio::test]
async fn test_sync_1arg_struct() {
    let mut msg = test_msg();
    let handler = iii_fn(greet).into_parts(&mut msg).unwrap();
    let result = handler(json!({"name": "World"})).await;
    assert_eq!(result.unwrap(), json!("Hello, World!"));
}

fn echo(input: Value) -> Result<Value, String> {
    Ok(input)
}

#[tokio::test]
async fn test_sync_1arg_value_passthrough() {
    let mut msg = test_msg();
    let handler = iii_fn(echo).into_parts(&mut msg).unwrap();
    let payload = json!({"key": "value", "num": 42});
    let result = handler(payload.clone()).await.unwrap();
    assert_eq!(result, payload);
}

fn always_fail(_input: Value) -> Result<Value, String> {
    Err("something went wrong".into())
}

#[tokio::test]
async fn test_sync_error_propagation() {
    let mut msg = test_msg();
    let handler = iii_fn(always_fail).into_parts(&mut msg).unwrap();
    let result = handler(json!({})).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        IIIError::Handler(msg) => assert!(msg.contains("something went wrong")),
        other => panic!("expected IIIError::Handler, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_sync_deser_wrong_type() {
    let mut msg = test_msg();
    let handler = iii_fn(greet).into_parts(&mut msg).unwrap();
    let result = handler(json!(42)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_sync_deser_missing_field() {
    let mut msg = test_msg();
    let handler = iii_fn(greet).into_parts(&mut msg).unwrap();
    let result = handler(json!({})).await;
    assert!(result.is_err());
}

fn split_words(input: String) -> Result<Vec<String>, String> {
    Ok(input.split_whitespace().map(|s| s.to_string()).collect())
}

#[tokio::test]
async fn test_sync_complex_output() {
    let mut msg = test_msg();
    let handler = iii_fn(split_words).into_parts(&mut msg).unwrap();
    let result = handler(json!("hello beautiful world")).await.unwrap();
    assert_eq!(result, json!(["hello", "beautiful", "world"]));
}

// ===========================================================================
// ASYNC tests
// ===========================================================================

async fn async_greet(input: GreetInput) -> Result<String, String> {
    Ok(format!("Async hello, {}!", input.name))
}

#[tokio::test]
async fn test_async_1arg_struct() {
    let mut msg = test_msg();
    let handler = iii_async_fn(async_greet).into_parts(&mut msg).unwrap();
    let result = handler(json!({"name": "Alice"})).await.unwrap();
    assert_eq!(result, json!("Async hello, Alice!"));
}

async fn async_fail(_input: Value) -> Result<Value, String> {
    Err("async failure".into())
}

#[tokio::test]
async fn test_async_error_propagation() {
    let mut msg = test_msg();
    let handler = iii_async_fn(async_fail).into_parts(&mut msg).unwrap();
    let result = handler(json!({})).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        IIIError::Handler(msg) => assert!(msg.contains("async failure")),
        other => panic!("expected IIIError::Handler, got: {other:?}"),
    }
}

// ===========================================================================
// Integration tests
// ===========================================================================

#[tokio::test]
async fn test_register_function_iii_fn() {
    let iii = iii_sdk::register_worker("ws://localhost:1234", iii_sdk::InitOptions::default());
    let _ref = iii.register_function((test_msg(), iii_fn(greet)));
}

#[tokio::test]
async fn test_register_function_iii_async_fn() {
    let iii = iii_sdk::register_worker("ws://localhost:1234", iii_sdk::InitOptions::default());
    let _ref = iii.register_function((test_msg(), iii_async_fn(async_greet)));
}
