use serde_json::{Value, json};

use iii::{
    engine::{Engine, EngineTrait, Handler},
    function::FunctionResult,
    protocol::ErrorBody,
};

#[tokio::test]
async fn test_basic_invocation_flow() {
    let engine = Engine::new();

    let handler = Handler::new(move |input: Value| {
        Box::pin(async move {
            FunctionResult::Success(Some(json!({
                "processed": input
            })))
        })
            as std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = FunctionResult<Option<Value>, ErrorBody>>
                        + Send,
                >,
            >
    });

    engine.register_function_handler(
        iii::engine::RegisterFunctionRequest {
            function_path: "test.processor".to_string(),
            description: Some("Test processor".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    let function = engine.functions.get("test.processor").unwrap();
    assert_eq!(function._function_path, "test.processor");
}

#[tokio::test]
async fn test_engine_cloning_preserves_functions() {
    let engine1 = Engine::new();

    let handler = Handler::new(move |input: Value| {
        Box::pin(async move { FunctionResult::Success(Some(input)) })
            as std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = FunctionResult<Option<Value>, ErrorBody>>
                        + Send,
                >,
            >
    });

    engine1.register_function_handler(
        iii::engine::RegisterFunctionRequest {
            function_path: "cloned.function".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    let engine2 = engine1.clone();

    assert!(engine1.functions.get("cloned.function").is_some());
    assert!(engine2.functions.get("cloned.function").is_some());
}
