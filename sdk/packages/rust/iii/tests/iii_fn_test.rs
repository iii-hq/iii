use iii_sdk::{
    IIIError, IntoFunctionHandler, RegisterFunctionMessage, Value, iii_async_fn, iii_fn,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct GreetInput {
    name: String,
}

fn greet(input: GreetInput) -> Result<String, String> {
    Ok(format!("Hello, {}!", input.name))
}

#[tokio::test]
async fn test_basic_single_arg() {
    let mut msg = RegisterFunctionMessage {
        id: "test".into(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation: None,
    };
    let handler = iii_fn(greet).into_parts(&mut msg).unwrap();
    let result = handler(json!({"name": "World"})).await;
    assert_eq!(result.unwrap(), json!("Hello, World!"));
}

// Quick smoke test for 0-arg
fn health() -> Result<String, String> {
    Ok("ok".into())
}

#[tokio::test]
async fn test_zero_arg_smoke() {
    let mut msg = RegisterFunctionMessage {
        id: "test".into(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation: None,
    };
    let handler = iii_fn(health).into_parts(&mut msg).unwrap();
    let result = handler(json!(null)).await.unwrap();
    assert_eq!(result, json!("ok"));
}

// ---------------------------------------------------------------------------
// Helper to build a default RegisterFunctionMessage for tests
// ---------------------------------------------------------------------------
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

// 1. 1-arg Value passthrough
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

// 2. 1-arg error propagation
fn always_fail(_input: Value) -> Result<Value, String> {
    Err("something went wrong".into())
}

#[tokio::test]
async fn test_sync_1arg_error_propagation() {
    let mut msg = test_msg();
    let handler = iii_fn(always_fail).into_parts(&mut msg).unwrap();
    let result = handler(json!({})).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        IIIError::Handler(msg) => assert!(msg.contains("something went wrong"), "got: {msg}"),
        other => panic!("expected IIIError::Handler, got: {other:?}"),
    }
}

// 3. 1-arg deserialization errors
#[tokio::test]
async fn test_sync_1arg_deser_wrong_type() {
    // greet expects {"name": String}, pass an integer instead
    let mut msg = test_msg();
    let handler = iii_fn(greet).into_parts(&mut msg).unwrap();
    let result = handler(json!(42)).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        IIIError::Handler(msg) => assert!(!msg.is_empty(), "error message should not be empty"),
        other => panic!("expected IIIError::Handler, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_sync_1arg_deser_missing_field() {
    // greet expects {"name": String}, pass empty object
    let mut msg = test_msg();
    let handler = iii_fn(greet).into_parts(&mut msg).unwrap();
    let result = handler(json!({})).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        IIIError::Handler(msg) => {
            assert!(
                msg.contains("name"),
                "error should mention missing field 'name', got: {msg}"
            )
        }
        other => panic!("expected IIIError::Handler, got: {other:?}"),
    }
}

// 4. 2-arg tuple
fn add(a: i32, b: i32) -> Result<i32, String> {
    Ok(a + b)
}

#[tokio::test]
async fn test_sync_2arg_tuple() {
    let mut msg = test_msg();
    let handler = iii_fn(add).into_parts(&mut msg).unwrap();
    let result = handler(json!([3, 4])).await.unwrap();
    assert_eq!(result, json!(7));
}

// 5. 3-arg tuple
fn concat3(a: String, sep: String, b: String) -> Result<String, String> {
    Ok(format!("{}{}{}", a, sep, b))
}

#[tokio::test]
async fn test_sync_3arg_tuple() {
    let mut msg = test_msg();
    let handler = iii_fn(concat3).into_parts(&mut msg).unwrap();
    let result = handler(json!(["hello", " ", "world"])).await.unwrap();
    assert_eq!(result, json!("hello world"));
}

// 6. 4-arg tuple
fn sum4(a: i32, b: i32, c: i32, d: i32) -> Result<i32, String> {
    Ok(a + b + c + d)
}

#[tokio::test]
async fn test_sync_4arg_tuple() {
    let mut msg = test_msg();
    let handler = iii_fn(sum4).into_parts(&mut msg).unwrap();
    let result = handler(json!([1, 2, 3, 4])).await.unwrap();
    assert_eq!(result, json!(10));
}

// 7. Tuple deserialization errors
#[tokio::test]
async fn test_sync_tuple_deser_wrong_length() {
    // add expects [i32, i32] but we pass [1, 2, 3]
    let mut msg = test_msg();
    let handler = iii_fn(add).into_parts(&mut msg).unwrap();
    let result = handler(json!([1, 2, 3])).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        IIIError::Handler(_) => {} // expected
        other => panic!("expected IIIError::Handler, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_sync_tuple_deser_not_array() {
    // add expects an array [i32, i32] but we pass an object
    let mut msg = test_msg();
    let handler = iii_fn(add).into_parts(&mut msg).unwrap();
    let result = handler(json!({"a": 1, "b": 2})).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        IIIError::Handler(_) => {} // expected
        other => panic!("expected IIIError::Handler, got: {other:?}"),
    }
}

// 8. Mixed types
fn repeat_str(text: String, count: usize) -> Result<String, String> {
    Ok(text.repeat(count))
}

#[tokio::test]
async fn test_sync_mixed_types() {
    let mut msg = test_msg();
    let handler = iii_fn(repeat_str).into_parts(&mut msg).unwrap();
    let result = handler(json!(["ab", 3])).await.unwrap();
    assert_eq!(result, json!("ababab"));
}

// 9. Complex output — returning Vec<String>
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

// 10. 0-arg async
async fn async_health() -> Result<String, String> {
    Ok("async-ok".into())
}

#[tokio::test]
async fn test_async_0arg() {
    let mut msg = test_msg();
    let handler = iii_async_fn(async_health).into_parts(&mut msg).unwrap();
    let result = handler(json!(null)).await.unwrap();
    assert_eq!(result, json!("async-ok"));
}

// 11. 1-arg async struct
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

// 12. 1-arg async error
async fn async_fail(_input: Value) -> Result<Value, String> {
    Err("async failure".into())
}

#[tokio::test]
async fn test_async_1arg_error() {
    let mut msg = test_msg();
    let handler = iii_async_fn(async_fail).into_parts(&mut msg).unwrap();
    let result = handler(json!({})).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        IIIError::Handler(msg) => assert!(msg.contains("async failure"), "got: {msg}"),
        other => panic!("expected IIIError::Handler, got: {other:?}"),
    }
}

// 13. 2-arg async tuple
async fn async_add(a: i32, b: i32) -> Result<i32, String> {
    Ok(a + b)
}

#[tokio::test]
async fn test_async_2arg_tuple() {
    let mut msg = test_msg();
    let handler = iii_async_fn(async_add).into_parts(&mut msg).unwrap();
    let result = handler(json!([10, 20])).await.unwrap();
    assert_eq!(result, json!(30));
}

// 14. 3-arg async tuple
async fn async_concat(a: String, sep: String, b: String) -> Result<String, String> {
    Ok(format!("{}{}{}", a, sep, b))
}

#[tokio::test]
async fn test_async_3arg_tuple() {
    let mut msg = test_msg();
    let handler = iii_async_fn(async_concat).into_parts(&mut msg).unwrap();
    let result = handler(json!(["foo", "-", "bar"])).await.unwrap();
    assert_eq!(result, json!("foo-bar"));
}

// ===========================================================================
// Integration tests — verify register_function compiles with iii_fn/iii_async_fn
// ===========================================================================

// 15. register_function with iii_fn (struct arg)
#[tokio::test]
async fn test_register_function_iii_fn_struct() {
    let iii = iii_sdk::register_worker("ws://localhost:1234", iii_sdk::InitOptions::default());
    let _ref = iii.register_function(
        RegisterFunctionMessage {
            id: "test.greet".into(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        iii_fn(greet),
    );
    // Compiles = compatible
}

// 16. register_function with iii_async_fn (struct arg)
#[tokio::test]
async fn test_register_function_iii_async_fn_struct() {
    let iii = iii_sdk::register_worker("ws://localhost:1234", iii_sdk::InitOptions::default());
    let _ref = iii.register_function(
        RegisterFunctionMessage {
            id: "test.async_greet".into(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        iii_async_fn(async_greet),
    );
    // Compiles = compatible
}

// 17. register_function with iii_fn (positional 2-arg)
#[tokio::test]
async fn test_register_function_iii_fn_positional() {
    let iii = iii_sdk::register_worker("ws://localhost:1234", iii_sdk::InitOptions::default());
    let _ref = iii.register_function(
        RegisterFunctionMessage {
            id: "test.add".into(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        iii_fn(add),
    );
    // Compiles = compatible
}

// 18. register_function with iii_async_fn (positional 2-arg)
#[tokio::test]
async fn test_register_function_iii_async_fn_positional() {
    let iii = iii_sdk::register_worker("ws://localhost:1234", iii_sdk::InitOptions::default());
    let _ref = iii.register_function(
        RegisterFunctionMessage {
            id: "test.async_add".into(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        iii_async_fn(async_add),
    );
    // Compiles = compatible
}
