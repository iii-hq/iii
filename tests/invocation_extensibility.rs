use std::sync::Arc;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use iii::{
    config::SecurityConfig,
    engine::Engine,
    function::{Function, RegistrationSource},
    invocation::{
        invoker::Invoker,
        method::{HttpMethod, InvocationMethod},
        InvokerRegistry,
    },
    protocol::ErrorBody,
};

struct MockGrpcInvoker {
    should_fail: bool,
}

#[async_trait]
impl Invoker for MockGrpcInvoker {
    fn method_type(&self) -> &'static str {
        "grpc"
    }

    async fn invoke(
        &self,
        function: &Function,
        invocation_id: Uuid,
        data: Value,
        _caller_function: Option<&str>,
        _trace_id: Option<&str>,
    ) -> Result<Option<Value>, ErrorBody> {
        if self.should_fail {
            return Err(ErrorBody {
                code: "grpc_error".into(),
                message: "Mock gRPC invocation failed".into(),
            });
        }

        Ok(Some(json!({
            "method": "grpc",
            "function": function.function_path,
            "invocation_id": invocation_id.to_string(),
            "input": data,
            "status": "success"
        })))
    }
}

struct MockMqttInvoker;

#[async_trait]
impl Invoker for MockMqttInvoker {
    fn method_type(&self) -> &'static str {
        "mqtt"
    }

    async fn invoke(
        &self,
        function: &Function,
        invocation_id: Uuid,
        data: Value,
        _caller_function: Option<&str>,
        _trace_id: Option<&str>,
    ) -> Result<Option<Value>, ErrorBody> {
        Ok(Some(json!({
            "method": "mqtt",
            "function": function.function_path,
            "invocation_id": invocation_id.to_string(),
            "message": data
        })))
    }
}

#[tokio::test]
async fn test_invoker_registry_registration() {
    let registry = InvokerRegistry::new();
    let grpc_invoker = Arc::new(MockGrpcInvoker { should_fail: false });

    registry.register(grpc_invoker.clone()).await;

    let retrieved = registry.get("grpc").await;
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().method_type(), "grpc");
}

#[tokio::test]
async fn test_invoker_registry_get_nonexistent() {
    let registry = InvokerRegistry::new();
    let result = registry.get("nonexistent").await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_invoker_registry_multiple_invokers() {
    let registry = InvokerRegistry::new();

    let grpc_invoker = Arc::new(MockGrpcInvoker { should_fail: false });
    let mqtt_invoker = Arc::new(MockMqttInvoker);

    registry.register(grpc_invoker).await;
    registry.register(mqtt_invoker).await;

    assert!(registry.get("grpc").await.is_some());
    assert!(registry.get("mqtt").await.is_some());
    assert!(registry.get("websocket").await.is_none());
}

#[tokio::test]
async fn test_engine_registers_http_invoker_by_default() {
    let engine = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();

    let http_invoker = engine.invoker_registry.get("http").await;
    assert!(http_invoker.is_some());
    assert_eq!(http_invoker.unwrap().method_type(), "http");
}

#[tokio::test]
async fn test_engine_register_custom_invoker() {
    let engine = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();

    let grpc_invoker = Arc::new(MockGrpcInvoker { should_fail: false });
    engine.register_invoker(grpc_invoker).await;

    let retrieved = engine.invoker_registry.get("grpc").await;
    assert!(retrieved.is_some());
}

#[tokio::test]
async fn test_custom_invoker_success() {
    let grpc_invoker = MockGrpcInvoker { should_fail: false };

    let function = Function {
        function_path: "test.grpc.function".to_string(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation_method: InvocationMethod::WebSocket {
            worker_id: Uuid::new_v4(),
        },
        registered_at: Utc::now(),
        registration_source: RegistrationSource::Config,
        handler: None,
    };

    let invocation_id = Uuid::new_v4();
    let data = json!({"test": "data"});

    let result = grpc_invoker
        .invoke(&function, invocation_id, data, None, None)
        .await;

    assert!(result.is_ok());
    let response = result.unwrap().unwrap();
    assert_eq!(response["method"], "grpc");
    assert_eq!(response["function"], "test.grpc.function");
    assert_eq!(response["status"], "success");
}

#[tokio::test]
async fn test_custom_invoker_failure() {
    let grpc_invoker = MockGrpcInvoker { should_fail: true };

    let function = Function {
        function_path: "test.grpc.failing".to_string(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation_method: InvocationMethod::WebSocket {
            worker_id: Uuid::new_v4(),
        },
        registered_at: Utc::now(),
        registration_source: RegistrationSource::Config,
        handler: None,
    };

    let result = grpc_invoker
        .invoke(&function, Uuid::new_v4(), json!({}), None, None)
        .await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert_eq!(error.code, "grpc_error");
    assert_eq!(error.message, "Mock gRPC invocation failed");
}

#[tokio::test]
async fn test_invocation_method_type() {
    let ws_method = InvocationMethod::WebSocket {
        worker_id: Uuid::new_v4(),
    };
    assert_eq!(ws_method.method_type(), "websocket");

    let http_method = InvocationMethod::Http {
        url: "https://example.com".to_string(),
        method: HttpMethod::Post,
        timeout_ms: Some(5000),
        headers: std::collections::HashMap::new(),
        auth: None,
    };
    assert_eq!(http_method.method_type(), "http");
}

#[tokio::test]
async fn test_multiple_custom_invokers_different_methods() {
    let engine = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();

    let grpc_invoker = Arc::new(MockGrpcInvoker { should_fail: false });
    let mqtt_invoker = Arc::new(MockMqttInvoker);

    engine.register_invoker(grpc_invoker).await;
    engine.register_invoker(mqtt_invoker).await;

    let grpc = engine.invoker_registry.get("grpc").await.unwrap();
    let mqtt = engine.invoker_registry.get("mqtt").await.unwrap();
    let http = engine.invoker_registry.get("http").await.unwrap();

    assert_eq!(grpc.method_type(), "grpc");
    assert_eq!(mqtt.method_type(), "mqtt");
    assert_eq!(http.method_type(), "http");
}

#[tokio::test]
async fn test_invoker_with_context_parameters() {
    let mqtt_invoker = MockMqttInvoker;

    let function = Function {
        function_path: "mqtt.sensor.data".to_string(),
        description: Some("MQTT sensor data publisher".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
        invocation_method: InvocationMethod::WebSocket {
            worker_id: Uuid::new_v4(),
        },
        registered_at: Utc::now(),
        registration_source: RegistrationSource::Config,
        handler: None,
    };

    let invocation_id = Uuid::new_v4();
    let data = json!({"temperature": 25.5, "humidity": 60});
    let caller = Some("weather.aggregator");
    let trace_id = Some("trace-123-456");

    let result = mqtt_invoker
        .invoke(&function, invocation_id, data.clone(), caller, trace_id)
        .await;

    assert!(result.is_ok());
    let response = result.unwrap().unwrap();
    assert_eq!(response["method"], "mqtt");
    assert_eq!(response["message"]["temperature"], 25.5);
}

#[tokio::test]
async fn test_invoker_registry_replacement() {
    let registry = InvokerRegistry::new();

    let invoker1 = Arc::new(MockGrpcInvoker { should_fail: false });
    registry.register(invoker1).await;

    let invoker2 = Arc::new(MockGrpcInvoker { should_fail: true });
    registry.register(invoker2).await;

    let retrieved = registry.get("grpc").await.unwrap();

    let function = Function {
        function_path: "test.replacement".to_string(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation_method: InvocationMethod::WebSocket {
            worker_id: Uuid::new_v4(),
        },
        registered_at: Utc::now(),
        registration_source: RegistrationSource::Config,
        handler: None,
    };

    let result = retrieved
        .invoke(&function, Uuid::new_v4(), json!({}), None, None)
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_concurrent_invoker_registration() {
    let engine = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let engine_clone = engine.clone();
            tokio::spawn(async move {
                if i % 2 == 0 {
                    let invoker = Arc::new(MockGrpcInvoker { should_fail: false });
                    engine_clone.register_invoker(invoker).await;
                } else {
                    let invoker = Arc::new(MockMqttInvoker);
                    engine_clone.register_invoker(invoker).await;
                }
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap();
    }

    assert!(engine.invoker_registry.get("grpc").await.is_some());
    assert!(engine.invoker_registry.get("mqtt").await.is_some());
}

#[tokio::test]
async fn test_invoker_with_empty_response() {
    struct EmptyResponseInvoker;

    #[async_trait]
    impl Invoker for EmptyResponseInvoker {
        fn method_type(&self) -> &'static str {
            "empty"
        }

        async fn invoke(
            &self,
            _function: &Function,
            _invocation_id: Uuid,
            _data: Value,
            _caller_function: Option<&str>,
            _trace_id: Option<&str>,
        ) -> Result<Option<Value>, ErrorBody> {
            Ok(None)
        }
    }

    let invoker = EmptyResponseInvoker;
    let function = Function {
        function_path: "test.empty".to_string(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation_method: InvocationMethod::WebSocket {
            worker_id: Uuid::new_v4(),
        },
        registered_at: Utc::now(),
        registration_source: RegistrationSource::Config,
        handler: None,
    };

    let result = invoker
        .invoke(&function, Uuid::new_v4(), json!({}), None, None)
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}
