use serde_json::{Value, json};

use iii::{
    engine::{Engine, EngineTrait, Handler},
    function::FunctionResult,
    protocol::ErrorBody,
};

/// Creates a test handler that wraps a simple transform function, hiding the
/// verbose `Pin<Box<dyn Future ...>>` type coercion that would otherwise be
/// repeated in every test.
type HandlerFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send>>;

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
async fn test_basic_invocation_flow() {
    let engine = Engine::new();

    let handler = make_test_handler(|input| Some(json!({ "processed": input })));

    engine.register_function_handler(
        iii::engine::RegisterFunctionRequest {
            function_id: "test.processor".to_string(),
            description: Some("Test processor".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        },
        handler,
    );

    let function = engine.functions.get("test.processor").unwrap();
    assert_eq!(function._function_id, "test.processor");
}

#[tokio::test]
async fn test_engine_cloning_preserves_functions() {
    let engine1 = Engine::new();

    let handler = make_test_handler(Some);

    engine1.register_function_handler(
        iii::engine::RegisterFunctionRequest {
            function_id: "cloned.function".to_string(),
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
