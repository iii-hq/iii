use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use iii::{
    bridge_api::{self, token::BridgeTokenRegistry},
    config::{BridgeApiConfig, SecurityConfig},
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::{FunctionHandler, FunctionResult},
    protocol::{ErrorBody, Message},
};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn test_token_creation_and_validation() {
    let secret = "test-secret-key";
    let registry = BridgeTokenRegistry::new(secret.to_string());

    let token = registry.create_token("inv-123", "test.function", "trace-456");

    let token_data = registry.validate_token(&token);
    assert!(token_data.is_some());

    let data = token_data.unwrap();
    assert_eq!(data.invocation_id, "inv-123");
    assert_eq!(data.function_path, "test.function");
    assert_eq!(data.trace_id, "trace-456");
}

#[tokio::test]
async fn test_token_invalidation() {
    let registry = BridgeTokenRegistry::new("test-secret".to_string());
    let token = registry.create_token("inv-123", "test.function", "trace-456");

    assert!(registry.validate_token(&token).is_some());

    registry.invalidate_token(&token);

    assert!(registry.validate_token(&token).is_none());
}

#[tokio::test]
async fn test_multiple_active_tokens() {
    let registry = BridgeTokenRegistry::new("test-secret".to_string());

    let token1 = registry.create_token("inv-1", "func1", "trace-1");
    let token2 = registry.create_token("inv-2", "func2", "trace-2");
    let token3 = registry.create_token("inv-3", "func3", "trace-3");

    assert_eq!(registry.active_token_count(), 3);

    assert!(registry.validate_token(&token1).is_some());
    assert!(registry.validate_token(&token2).is_some());
    assert!(registry.validate_token(&token3).is_some());

    registry.invalidate_token(&token2);

    assert_eq!(registry.active_token_count(), 2);
    assert!(registry.validate_token(&token1).is_some());
    assert!(registry.validate_token(&token2).is_none());
    assert!(registry.validate_token(&token3).is_some());
}

#[tokio::test]
async fn test_token_cleanup() {
    let registry = BridgeTokenRegistry::new("test-secret".to_string());

    registry.create_token("inv-1", "func1", "trace-1");
    registry.create_token("inv-2", "func2", "trace-2");

    assert_eq!(registry.active_token_count(), 2);

    registry.cleanup_old_tokens(0);

    assert_eq!(registry.active_token_count(), 0);
}

#[tokio::test]
async fn test_engine_initialization_with_bridge_config() {
    unsafe {
        std::env::set_var("III_BRIDGE_SECRET", "test-bridge-secret");
    }

    let bridge_config = BridgeApiConfig {
        enabled: true,
        public_url: Some("https://engine.example.com".to_string()),
        token_secret_env: "III_BRIDGE_SECRET".to_string(),
    };

    let engine = Engine::new_with_security(
        SecurityConfig::default(),
        None,
        Some(bridge_config),
    )
    .unwrap();

    let token_registry = engine.webhook_dispatcher.token_registry();
    assert!(token_registry.is_some());

    unsafe {
        std::env::remove_var("III_BRIDGE_SECRET");
    }
}

#[tokio::test]
async fn test_engine_initialization_without_bridge_config() {
    let engine = Engine::new_with_security(SecurityConfig::default(), None, None).unwrap();

    let token_registry = engine.webhook_dispatcher.token_registry();
    assert!(token_registry.is_none());
}

#[tokio::test]
async fn test_bridge_config_disabled() {
    let bridge_config = BridgeApiConfig {
        enabled: false,
        public_url: Some("https://engine.example.com".to_string()),
        token_secret_env: "III_BRIDGE_SECRET".to_string(),
    };

    let engine = Engine::new_with_security(
        SecurityConfig::default(),
        None,
        Some(bridge_config),
    )
    .unwrap();

    let token_registry = engine.webhook_dispatcher.token_registry();
    assert!(token_registry.is_none());
}

#[tokio::test]
async fn test_token_uniqueness() {
    let registry = BridgeTokenRegistry::new("test-secret".to_string());

    let token1 = registry.create_token("inv-1", "func1", "trace-1");
    let token2 = registry.create_token("inv-2", "func1", "trace-1");

    assert_ne!(token1, token2);
}

#[tokio::test]
async fn test_token_signature_validation() {
    let registry = BridgeTokenRegistry::new("test-secret".to_string());
    let token = registry.create_token("inv-123", "test.function", "trace-456");

    let different_registry = BridgeTokenRegistry::new("different-secret".to_string());
    assert!(different_registry.validate_token(&token).is_none());
}

#[tokio::test]
async fn test_bridge_endpoint_invoke_function() {
    let engine = Arc::new(Engine::new());
    let registry = Arc::new(BridgeTokenRegistry::new("test-secret".to_string()));

    engine.register_function(
        RegisterFunctionRequest {
            function_path: "test.echo".to_string(),
            description: Some("Echo test function".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            worker_id: None,
        },
        Box::new(EchoHandler),
    );

    let token = registry.create_token("inv-123", "test.caller", "trace-456");

    let app = bridge_api::router(engine.clone(), registry.clone());

    let message = Message::InvokeFunction {
        invocation_id: None,
        function_path: "test.echo".to_string(),
        data: json!({"message": "hello"}),
    };

    let request = Request::builder()
        .method("POST")
        .uri("/bridge")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&message).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: Message = serde_json::from_slice(&body).unwrap();

    match result {
        Message::InvocationResult {
            result: Some(result_data),
            error: None,
            ..
        } => {
            assert_eq!(result_data["message"], "hello");
        }
        _ => panic!("Expected InvocationResult with result"),
    }
}

#[tokio::test]
async fn test_bridge_endpoint_invalid_token() {
    let engine = Arc::new(Engine::new());
    let registry = Arc::new(BridgeTokenRegistry::new("test-secret".to_string()));

    let app = bridge_api::router(engine.clone(), registry.clone());

    let message = Message::InvokeFunction {
        invocation_id: None,
        function_path: "test.echo".to_string(),
        data: json!({"message": "hello"}),
    };

    let request = Request::builder()
        .method("POST")
        .uri("/bridge")
        .header("Authorization", "Bearer invalid-token")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&message).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_bridge_endpoint_missing_auth_header() {
    let engine = Arc::new(Engine::new());
    let registry = Arc::new(BridgeTokenRegistry::new("test-secret".to_string()));

    let app = bridge_api::router(engine.clone(), registry.clone());

    let message = Message::InvokeFunction {
        invocation_id: None,
        function_path: "test.echo".to_string(),
        data: json!({"message": "hello"}),
    };

    let request = Request::builder()
        .method("POST")
        .uri("/bridge")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&message).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

use std::{future::Future, pin::Pin};
use uuid::Uuid;

struct EchoHandler;

impl FunctionHandler for EchoHandler {
    fn handle_function<'a>(
        &'a self,
        _invocation_id: Option<Uuid>,
        _function_path: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'a>> {
        Box::pin(async move { FunctionResult::Success(Some(input)) })
    }
}
