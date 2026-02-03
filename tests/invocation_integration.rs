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
    },
    protocol::ErrorBody,
};

struct TestKafkaInvoker {
    topic: String,
}

#[async_trait]
impl Invoker for TestKafkaInvoker {
    fn method_type(&self) -> &'static str {
        "kafka"
    }

    async fn invoke(
        &self,
        function: &Function,
        invocation_id: Uuid,
        data: Value,
        caller_function: Option<&str>,
        trace_id: Option<&str>,
    ) -> Result<Option<Value>, ErrorBody> {
        Ok(Some(json!({
            "protocol": "kafka",
            "topic": self.topic,
            "function": function.function_path,
            "invocation_id": invocation_id.to_string(),
            "payload": data,
            "caller": caller_function,
            "trace_id": trace_id,
            "partition": 0,
            "offset": 12345
        })))
    }
}

#[tokio::test]
async fn test_invocation_handler_routes_to_http() {
    let engine = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();

    let http_invoker = engine.invoker_registry.get("http").await;
    assert!(http_invoker.is_some());
    assert_eq!(http_invoker.unwrap().method_type(), "http");
}

#[tokio::test]
async fn test_invocation_handler_routes_to_custom_invoker() {
    let engine = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();

    let kafka_invoker = Arc::new(TestKafkaInvoker {
        topic: "events".to_string(),
    });
    engine.register_invoker(kafka_invoker.clone()).await;

    let retrieved = engine.invoker_registry.get("kafka").await;
    assert!(retrieved.is_some());

    let function = Function {
        function_path: "events.publisher".to_string(),
        description: Some("Kafka event publisher".to_string()),
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
        .unwrap()
        .invoke(
            &function,
            Uuid::new_v4(),
            json!({"event": "user_login"}),
            Some("auth.service"),
            Some("trace-abc-123"),
        )
        .await;

    assert!(result.is_ok());
    let response = result.unwrap().unwrap();
    assert_eq!(response["protocol"], "kafka");
    assert_eq!(response["topic"], "events");
    assert_eq!(response["caller"], "auth.service");
    assert_eq!(response["trace_id"], "trace-abc-123");
}

#[tokio::test]
async fn test_unsupported_invocation_method() {
    let engine = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();

    let result = engine.invoker_registry.get("unsupported_protocol").await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_http_invocation_method_configuration() {
    let http_method = InvocationMethod::Http {
        url: "https://api.example.com/function".to_string(),
        method: HttpMethod::Post,
        timeout_ms: Some(10000),
        headers: vec![
            ("Authorization".to_string(), "Bearer token".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ]
        .into_iter()
        .collect(),
        auth: None,
    };

    assert_eq!(http_method.method_type(), "http");

    if let InvocationMethod::Http {
        url,
        method,
        timeout_ms,
        headers,
        ..
    } = http_method
    {
        assert_eq!(url, "https://api.example.com/function");
        assert!(matches!(method, HttpMethod::Post));
        assert_eq!(timeout_ms, Some(10000));
        assert_eq!(headers.len(), 2);
        assert_eq!(headers.get("Authorization"), Some(&"Bearer token".to_string()));
    } else {
        panic!("Expected HTTP invocation method");
    }
}

#[tokio::test]
async fn test_function_with_different_registration_sources() {
    let _engine = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();

    let config_function = Function {
        function_path: "config.function".to_string(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation_method: InvocationMethod::Http {
            url: "https://example.com".to_string(),
            method: HttpMethod::Post,
            timeout_ms: Some(5000),
            headers: Default::default(),
            auth: None,
        },
        registered_at: Utc::now(),
        registration_source: RegistrationSource::Config,
        handler: None,
    };

    let api_function = Function {
        function_path: "api.function".to_string(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation_method: InvocationMethod::Http {
            url: "https://example.com".to_string(),
            method: HttpMethod::Post,
            timeout_ms: Some(5000),
            headers: Default::default(),
            auth: None,
        },
        registered_at: Utc::now(),
        registration_source: RegistrationSource::AdminApi,
        handler: None,
    };

    assert!(matches!(
        config_function.registration_source,
        RegistrationSource::Config
    ));
    assert!(matches!(
        api_function.registration_source,
        RegistrationSource::AdminApi
    ));
}

#[tokio::test]
async fn test_multiple_invokers_same_engine() {
    let engine = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();

    struct Invoker1;
    struct Invoker2;
    struct Invoker3;

    #[async_trait]
    impl Invoker for Invoker1 {
        fn method_type(&self) -> &'static str {
            "protocol1"
        }
        async fn invoke(
            &self,
            _: &Function,
            _: Uuid,
            data: Value,
            _: Option<&str>,
            _: Option<&str>,
        ) -> Result<Option<Value>, ErrorBody> {
            Ok(Some(json!({"protocol": "1", "data": data})))
        }
    }

    #[async_trait]
    impl Invoker for Invoker2 {
        fn method_type(&self) -> &'static str {
            "protocol2"
        }
        async fn invoke(
            &self,
            _: &Function,
            _: Uuid,
            data: Value,
            _: Option<&str>,
            _: Option<&str>,
        ) -> Result<Option<Value>, ErrorBody> {
            Ok(Some(json!({"protocol": "2", "data": data})))
        }
    }

    #[async_trait]
    impl Invoker for Invoker3 {
        fn method_type(&self) -> &'static str {
            "protocol3"
        }
        async fn invoke(
            &self,
            _: &Function,
            _: Uuid,
            data: Value,
            _: Option<&str>,
            _: Option<&str>,
        ) -> Result<Option<Value>, ErrorBody> {
            Ok(Some(json!({"protocol": "3", "data": data})))
        }
    }

    engine.register_invoker(Arc::new(Invoker1)).await;
    engine.register_invoker(Arc::new(Invoker2)).await;
    engine.register_invoker(Arc::new(Invoker3)).await;

    let p1 = engine.invoker_registry.get("protocol1").await;
    let p2 = engine.invoker_registry.get("protocol2").await;
    let p3 = engine.invoker_registry.get("protocol3").await;
    let http = engine.invoker_registry.get("http").await;

    assert!(p1.is_some());
    assert!(p2.is_some());
    assert!(p3.is_some());
    assert!(http.is_some());
}

#[tokio::test]
async fn test_invoker_error_propagation() {
    struct FailingInvoker;

    #[async_trait]
    impl Invoker for FailingInvoker {
        fn method_type(&self) -> &'static str {
            "failing"
        }

        async fn invoke(
            &self,
            _: &Function,
            _: Uuid,
            _: Value,
            _: Option<&str>,
            _: Option<&str>,
        ) -> Result<Option<Value>, ErrorBody> {
            Err(ErrorBody {
                code: "custom_error".into(),
                message: "This invoker always fails".into(),
            })
        }
    }

    let engine = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();
    engine.register_invoker(Arc::new(FailingInvoker)).await;

    let invoker = engine.invoker_registry.get("failing").await.unwrap();
    let function = Function {
        function_path: "test.failing".to_string(),
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

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert_eq!(error.code, "custom_error");
    assert_eq!(error.message, "This invoker always fails");
}

#[tokio::test]
async fn test_invoker_with_large_payload() {
    let engine = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();

    let kafka_invoker = Arc::new(TestKafkaInvoker {
        topic: "large-data".to_string(),
    });
    engine.register_invoker(kafka_invoker).await;

    let invoker = engine.invoker_registry.get("kafka").await.unwrap();

    let large_payload = json!({
        "items": (0..1000).map(|i| json!({
            "id": i,
            "name": format!("Item {}", i),
            "description": "A".repeat(100)
        })).collect::<Vec<_>>()
    });

    let function = Function {
        function_path: "bulk.processor".to_string(),
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
        .invoke(&function, Uuid::new_v4(), large_payload.clone(), None, None)
        .await;

    assert!(result.is_ok());
    let response = result.unwrap().unwrap();
    assert_eq!(response["protocol"], "kafka");
    assert_eq!(response["payload"]["items"].as_array().unwrap().len(), 1000);
}

#[tokio::test]
async fn test_engine_cloning_preserves_invokers() {
    let engine1 = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();

    let kafka_invoker = Arc::new(TestKafkaInvoker {
        topic: "test".to_string(),
    });
    engine1.register_invoker(kafka_invoker).await;

    let engine2 = engine1.clone();

    let kafka1 = engine1.invoker_registry.get("kafka").await;
    let kafka2 = engine2.invoker_registry.get("kafka").await;

    assert!(kafka1.is_some());
    assert!(kafka2.is_some());
}
