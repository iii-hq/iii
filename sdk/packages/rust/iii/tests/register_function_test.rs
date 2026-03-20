use iii_sdk::{
    IntoFunctionHandler, RegisterFunction, RegisterFunctionMessage, iii_async_fn, iii_fn,
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

#[derive(Deserialize)]
struct AddInput {
    a: i32,
    b: i32,
}

fn add(input: AddInput) -> Result<i32, String> {
    Ok(input.a + input.b)
}

// === iii_fn auto-fills schemas ===

#[tokio::test]
async fn test_iii_fn_sets_request_format() {
    let mut msg = RegisterFunctionMessage {
        id: "test".into(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation: None,
    };
    let _handler = iii_fn(greet).into_parts(&mut msg);
    assert!(msg.request_format.is_some());
    let rf = msg.request_format.unwrap();
    assert_eq!(rf["name"], "request");
    assert_eq!(rf["type"], "object");
}

#[tokio::test]
async fn test_iii_fn_fills_format_on_old_api() {
    let mut msg = RegisterFunctionMessage {
        id: "test".into(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation: None,
    };
    let _handler = iii_fn(add).into_parts(&mut msg);
    assert!(msg.request_format.is_some());
    assert!(msg.response_format.is_some());
}

#[tokio::test]
async fn test_iii_fn_does_not_overwrite_existing_format() {
    let custom_format = json!({"custom": true});
    let mut msg = RegisterFunctionMessage {
        id: "test".into(),
        description: None,
        request_format: Some(custom_format.clone()),
        response_format: None,
        metadata: None,
        invocation: None,
    };
    let _handler = iii_fn(add).into_parts(&mut msg);
    assert_eq!(msg.request_format.unwrap(), custom_format);
    assert!(msg.response_format.is_some());
}

// === RegisterFunction builder ===

#[tokio::test]
async fn test_register_function_builder_sync() {
    let iii = iii_sdk::register_worker("ws://localhost:1234", iii_sdk::InitOptions::default());
    let func_ref = iii
        .register_function(RegisterFunction::new("test::add", add).description("Add two numbers"));
    assert_eq!(func_ref.id, "test::add");
}

#[tokio::test]
async fn test_register_function_builder_has_schema() {
    let reg = RegisterFunction::new("test::add", add);
    assert!(reg.request_format().is_some());
    let rf = reg.request_format().unwrap();
    assert_eq!(rf["name"], "request");
    assert_eq!(rf["type"], "object");
    let res = reg.response_format().unwrap();
    assert_eq!(res["name"], "response");
    assert_eq!(res["type"], "number");
}

#[tokio::test]
async fn test_schema_1arg_struct() {
    let reg = RegisterFunction::new("test", greet);
    let rf = reg.request_format().unwrap();
    assert_eq!(rf["name"], "request");
    assert_eq!(rf["type"], "object");
    let res = reg.response_format().unwrap();
    assert_eq!(res["name"], "response");
    assert_eq!(res["type"], "string");
}

// === Async schema ===

async fn async_greet(input: GreetInput) -> Result<String, String> {
    Ok(format!("Hello, {}!", input.name))
}

#[tokio::test]
async fn test_schema_async_1arg() {
    let reg = RegisterFunction::new_async("test", async_greet);
    let rf = reg.request_format().unwrap();
    assert_eq!(rf["name"], "request");
    assert_eq!(rf["type"], "object");
}

// === Builder methods ===

#[tokio::test]
async fn test_builder_description() {
    let iii = iii_sdk::register_worker("ws://localhost:1234", iii_sdk::InitOptions::default());
    let _ref = iii.register_function(
        RegisterFunction::new("test::greet", greet)
            .description("Greet by name")
            .metadata(json!({"version": 1})),
    );
}

#[tokio::test]
async fn test_register_async() {
    let iii = iii_sdk::register_worker("ws://localhost:1234", iii_sdk::InitOptions::default());
    let _ref = iii.register_function(
        RegisterFunction::new_async("test::async_greet", async_greet).description("Async greet"),
    );
}

// === Handler still works correctly ===

#[tokio::test]
async fn test_registered_handler_works() {
    let mut msg = RegisterFunctionMessage {
        id: "test".into(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation: None,
    };
    let handler = iii_fn(greet).into_parts(&mut msg).unwrap();
    let result = handler(json!({"name": "World"})).await.unwrap();
    assert_eq!(result, json!("Hello, World!"));
}
