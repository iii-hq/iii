use serde_json::{Value, json};
use std::sync::Arc;

use iii::{
    engine::{Engine, EngineTrait, Handler},
    function::FunctionResult,
    protocol::ErrorBody,
};

/// Creates a test handler that wraps a simple transform function, hiding the
/// verbose `Pin<Box<dyn Future ...>>` type coercion that would otherwise be
/// repeated in every test.
type HandlerFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send>,
>;

fn make_test_handler(
    f: impl Fn(Value) -> Option<Value> + Send + Sync + 'static,
) -> Handler<impl Fn(Value) -> HandlerFuture> {
    Handler::new(move |input: Value| {
        let result = f(input);
        Box::pin(async move { FunctionResult::Success(result) })
            as std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = FunctionResult<Option<Value>, ErrorBody>>
                        + Send,
                >,
            >
    })
}

#[tokio::test]
async fn test_basic_function_invocation() {
    let engine = Arc::new(Engine::new());

    let handler = make_test_handler(|input| {
        Some(json!({
            "input": input,
            "output": "processed"
        }))
    });

    engine.register_function_handler(
        iii::engine::RegisterFunctionRequest {
            function_id: "test.function".to_string(),
            description: Some("Test function".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    let function = engine.functions.get("test.function").unwrap();
    assert_eq!(function._function_id, "test.function");
}

#[tokio::test]
async fn test_engine_function_registration() {
    let engine = Engine::new();

    let handler = make_test_handler(|input| Some(json!({ "input": input })));

    engine.register_function_handler(
        iii::engine::RegisterFunctionRequest {
            function_id: "another.test".to_string(),
            description: Some("Another test function".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    assert!(engine.functions.get("another.test").is_some());
}
