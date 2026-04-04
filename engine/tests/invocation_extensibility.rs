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

#[tokio::test]
async fn worker_pid_is_stored_and_listed() {
    use iii::{
        engine::Outbound,
        modules::{
            engine_fn::EngineFunctionsModule, module::Module,
            observability::metrics::ensure_default_meter,
        },
        workers::Worker,
    };

    ensure_default_meter();
    let engine = Arc::new(Engine::new());

    let engine_fn_module = EngineFunctionsModule::create(engine.clone(), None)
        .await
        .expect("create EngineFunctionsModule");
    engine_fn_module
        .initialize()
        .await
        .expect("initialize EngineFunctionsModule");
    engine_fn_module.register_functions(engine.clone());

    // Simulate worker connecting
    let (tx, _rx) = tokio::sync::mpsc::channel::<Outbound>(8);
    let worker = Worker::new(tx);
    let worker_id = worker.id.to_string();
    engine.worker_registry.register_worker(worker);

    // Register with pid
    engine
        .call(
            "engine::workers::register",
            serde_json::json!({
                "_caller_worker_id": worker_id,
                "runtime": "node",
                "version": "20.0.0",
                "pid": 42000u32,
            }),
        )
        .await
        .expect("register call should succeed");

    // List workers and verify pid is present
    let list_result = engine
        .call("engine::workers::list", serde_json::json!({}))
        .await
        .expect("list call succeeds")
        .expect("result is Some");

    let workers = list_result
        .get("workers")
        .and_then(|v| v.as_array())
        .expect("workers array");

    let found = workers
        .iter()
        .find(|w| w.get("id").and_then(|v| v.as_str()) == Some(worker_id.as_str()))
        .expect("worker in list");

    assert_eq!(found.get("pid").and_then(|v| v.as_u64()), Some(42000u64));
}

#[cfg(test)]
mod schema_tests {
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use std::sync::Arc;

    use function_macros::{function, service};
    use iii::{
        engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
        function::FunctionResult,
        protocol::ErrorBody,
    };

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct TestInput {
        name: String,
        count: u32,
    }

    #[derive(Debug, Clone, Serialize, JsonSchema)]
    struct TestOutput {
        result: String,
    }

    #[derive(Clone)]
    struct SchemaTestService {
        #[allow(dead_code)]
        engine: Arc<Engine>,
    }

    #[service]
    impl SchemaTestService {
        #[function(id = "test.typed_schema", description = "test typed schema")]
        pub async fn typed_fn(&self, input: TestInput) -> FunctionResult<TestOutput, ErrorBody> {
            FunctionResult::Success(TestOutput {
                result: format!("{}:{}", input.name, input.count),
            })
        }

        #[function(id = "test.raw_json", description = "test raw json")]
        pub async fn raw_fn(&self, input: Value) -> FunctionResult<Option<Value>, ErrorBody> {
            FunctionResult::Success(Some(input))
        }

        #[function(id = "test.no_args", description = "test no args")]
        pub async fn no_args_fn(&self, _input: ()) -> FunctionResult<Option<Value>, ErrorBody> {
            FunctionResult::NoResult
        }
    }

    #[tokio::test]
    async fn typed_function_has_request_and_response_schema() {
        let engine = Arc::new(Engine::new());
        let svc = SchemaTestService {
            engine: engine.clone(),
        };
        svc.register_functions(engine.clone());

        let func = engine
            .functions
            .get("test.typed_schema")
            .expect("function should be registered");

        // request_format should contain a JSON Schema for TestInput
        let req_schema = func
            .request_format
            .as_ref()
            .expect("request_format should be Some");
        assert_eq!(req_schema["type"], "object");
        let props = req_schema["properties"].as_object().unwrap();
        assert!(
            props.contains_key("name"),
            "schema should have 'name' property"
        );
        assert!(
            props.contains_key("count"),
            "schema should have 'count' property"
        );

        // response_format should contain a JSON Schema for TestOutput
        let res_schema = func
            .response_format
            .as_ref()
            .expect("response_format should be Some");
        assert_eq!(res_schema["type"], "object");
        let props = res_schema["properties"].as_object().unwrap();
        assert!(
            props.contains_key("result"),
            "schema should have 'result' property"
        );
    }

    #[tokio::test]
    async fn raw_json_function_has_no_response_schema() {
        let engine = Arc::new(Engine::new());
        let svc = SchemaTestService {
            engine: engine.clone(),
        };
        svc.register_functions(engine.clone());

        let func = engine
            .functions
            .get("test.raw_json")
            .expect("function should be registered");

        // request_format should be Some (Value has a JsonSchema impl)
        assert!(func.request_format.is_some());

        // response_format should be None for Option<Value>
        assert!(
            func.response_format.is_none(),
            "Option<Value> return should produce None response_format"
        );
    }

    #[tokio::test]
    async fn no_args_function_has_no_request_schema() {
        let engine = Arc::new(Engine::new());
        let svc = SchemaTestService {
            engine: engine.clone(),
        };
        svc.register_functions(engine.clone());

        let func = engine
            .functions
            .get("test.no_args")
            .expect("function should be registered");

        // request_format should be None for ()
        assert!(
            func.request_format.is_none(),
            "() input should produce None request_format"
        );
    }
}
