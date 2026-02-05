use std::sync::Arc;
use serde_json::{Value, json};

use iii::{
    engine::{Engine, EngineTrait, Handler},
    function::FunctionResult,
    protocol::ErrorBody,
};

#[tokio::test]
async fn test_basic_function_invocation() {
    let engine = Arc::new(Engine::new());

    let handler = Handler::new(move |input: Value| {
        Box::pin(async move {
            FunctionResult::Success(Some(json!({
                "input": input,
                "output": "processed"
            })))
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send>>
    });

    engine
        .register_function_handler(
            iii::engine::RegisterFunctionRequest {
                function_path: "test.function".to_string(),
                description: Some("Test function".to_string()),
                request_format: None,
                response_format: None,
                metadata: None,
            },
            handler,
        );

    let function = engine.functions.get("test.function").unwrap();
    assert_eq!(function._function_path, "test.function");
}

#[tokio::test]
async fn test_engine_function_registration() {
    let engine = Engine::new();
    
    let handler = Handler::new(move |input: Value| {
        Box::pin(async move {
            FunctionResult::Success(Some(json!({
                "input": input
            })))
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send>>
    });

    engine
        .register_function_handler(
            iii::engine::RegisterFunctionRequest {
                function_path: "another.test".to_string(),
                description: Some("Another test function".to_string()),
                request_format: None,
                response_format: None,
                metadata: None,
            },
            handler,
        );

    assert!(engine.functions.get("another.test").is_some());
}
