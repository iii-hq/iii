use std::sync::Arc;

use iii::{
    admin_api::{AdminApiConfig, AdminApiModule},
    config::SecurityConfig,
    engine::Engine,
    modules::module::Module,
};
use reqwest::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn test_admin_api_requires_authentication() {
    // Don't set III_ADMIN_TOKEN - should fail
    unsafe { std::env::remove_var("III_ADMIN_TOKEN"); }

    let engine = Arc::new(Engine::new_with_security(SecurityConfig::default(), None).unwrap());
    let config = AdminApiConfig {
        port: 49136, // Different port to avoid conflicts
        host: "127.0.0.1".to_string(),
    };

    let module = AdminApiModule::create(
        engine.clone(),
        Some(serde_json::to_value(&config).unwrap()),
    )
    .await
    .unwrap();

    module.initialize().await.unwrap();

    // Wait for server to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let response = client
        .get("http://127.0.0.1:49136/admin/functions")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_admin_api_list_functions() {
    unsafe { std::env::set_var("III_ADMIN_TOKEN", "test-token-123"); }

    let engine = Arc::new(Engine::new_with_security(SecurityConfig::default(), None).unwrap());
    let config = AdminApiConfig {
        port: 49137,
        host: "127.0.0.1".to_string(),
    };

    let module = AdminApiModule::create(
        engine.clone(),
        Some(serde_json::to_value(&config).unwrap()),
    )
    .await
    .unwrap();

    module.initialize().await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let response = client
        .get("http://127.0.0.1:49137/admin/functions")
        .header("Authorization", "Bearer test-token-123")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert!(body.get("functions").is_some());
}

#[tokio::test]
async fn test_admin_api_register_function() {
    unsafe { std::env::set_var("III_ADMIN_TOKEN", "test-token-456"); }
    unsafe { std::env::set_var("TEST_HMAC_SECRET", "my-secret-key"); }

    let mut security = SecurityConfig::default();
    security.url_allowlist = vec!["example.com".to_string()];
    security.require_https = false;

    let engine = Arc::new(Engine::new_with_security(security, None).unwrap());
    let config = AdminApiConfig {
        port: 49138,
        host: "127.0.0.1".to_string(),
    };

    let module = AdminApiModule::create(
        engine.clone(),
        Some(serde_json::to_value(&config).unwrap()),
    )
    .await
    .unwrap();

    module.initialize().await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();

    // Register a new function
    let register_payload = json!({
        "function_path": "test.function",
        "description": "Test function",
        "invocation": {
            "url": "http://example.com/invoke",
            "method": "POST",
            "timeout_ms": 5000,
            "headers": {
                "X-Custom-Header": "value"
            },
            "auth": {
                "type": "hmac",
                "secret_key": "TEST_HMAC_SECRET"
            }
        }
    });

    let response = client
        .post("http://127.0.0.1:49138/admin/functions")
        .header("Authorization", "Bearer test-token-456")
        .json(&register_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["status"], "registered");
    assert_eq!(body["function_path"], "test.function");
    assert_eq!(body["persisted"], true);

    // Verify function was registered
    assert!(engine.functions.get("test.function").is_some());
}

#[tokio::test]
async fn test_admin_api_register_duplicate_function() {
    unsafe { std::env::set_var("III_ADMIN_TOKEN", "test-token-789"); }

    let mut security = SecurityConfig::default();
    security.url_allowlist = vec!["example.com".to_string()];
    security.require_https = false;

    let engine = Arc::new(Engine::new_with_security(security, None).unwrap());
    let config = AdminApiConfig {
        port: 49139,
        host: "127.0.0.1".to_string(),
    };

    let module = AdminApiModule::create(
        engine.clone(),
        Some(serde_json::to_value(&config).unwrap()),
    )
    .await
    .unwrap();

    module.initialize().await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();

    let register_payload = json!({
        "function_path": "duplicate.test",
        "invocation": {
            "url": "http://example.com/test",
            "method": "POST"
        }
    });

    // First registration should succeed
    let response1 = client
        .post("http://127.0.0.1:49139/admin/functions")
        .header("Authorization", "Bearer test-token-789")
        .json(&register_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);

    // Second registration should fail with CONFLICT
    let response2 = client
        .post("http://127.0.0.1:49139/admin/functions")
        .header("Authorization", "Bearer test-token-789")
        .json(&register_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_admin_api_update_function() {
    unsafe { std::env::set_var("III_ADMIN_TOKEN", "test-token-update"); }

    let mut security = SecurityConfig::default();
    security.url_allowlist = vec!["example.com".to_string()];
    security.require_https = false;

    let engine = Arc::new(Engine::new_with_security(security, None).unwrap());
    let config = AdminApiConfig {
        port: 49140,
        host: "127.0.0.1".to_string(),
    };

    let module = AdminApiModule::create(
        engine.clone(),
        Some(serde_json::to_value(&config).unwrap()),
    )
    .await
    .unwrap();

    module.initialize().await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();

    // First, register a function
    let register_payload = json!({
        "function_path": "update.test",
        "description": "Original description",
        "invocation": {
            "url": "http://example.com/original",
            "method": "POST"
        }
    });

    client
        .post("http://127.0.0.1:49140/admin/functions")
        .header("Authorization", "Bearer test-token-update")
        .json(&register_payload)
        .send()
        .await
        .unwrap();

    // Now update it
    let update_payload = json!({
        "description": "Updated description",
        "invocation": {
            "url": "http://example.com/updated",
            "method": "PUT",
            "timeout_ms": 10000
        }
    });

    let response = client
        .put("http://127.0.0.1:49140/admin/functions/update.test")
        .header("Authorization", "Bearer test-token-update")
        .json(&update_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["status"], "updated");
}

#[tokio::test]
async fn test_admin_api_delete_function() {
    unsafe { std::env::set_var("III_ADMIN_TOKEN", "test-token-delete"); }

    let mut security = SecurityConfig::default();
    security.url_allowlist = vec!["example.com".to_string()];
    security.require_https = false;

    let engine = Arc::new(Engine::new_with_security(security, None).unwrap());
    let config = AdminApiConfig {
        port: 49141,
        host: "127.0.0.1".to_string(),
    };

    let module = AdminApiModule::create(
        engine.clone(),
        Some(serde_json::to_value(&config).unwrap()),
    )
    .await
    .unwrap();

    module.initialize().await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();

    // Register a function
    let register_payload = json!({
        "function_path": "delete.test",
        "invocation": {
            "url": "http://example.com/test",
            "method": "POST"
        }
    });

    client
        .post("http://127.0.0.1:49141/admin/functions")
        .header("Authorization", "Bearer test-token-delete")
        .json(&register_payload)
        .send()
        .await
        .unwrap();

    // Verify it exists
    assert!(engine.functions.get("delete.test").is_some());

    // Delete it
    let response = client
        .delete("http://127.0.0.1:49141/admin/functions/delete.test")
        .header("Authorization", "Bearer test-token-delete")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // Verify it's gone
    assert!(engine.functions.get("delete.test").is_none());
}

#[tokio::test]
async fn test_admin_api_invalid_function_path() {
    unsafe { std::env::set_var("III_ADMIN_TOKEN", "test-token-invalid"); }

    let mut security = SecurityConfig::default();
    security.url_allowlist = vec!["*".to_string()];
    security.require_https = false;

    let engine = Arc::new(Engine::new_with_security(security, None).unwrap());
    let config = AdminApiConfig {
        port: 49142,
        host: "127.0.0.1".to_string(),
    };

    let module = AdminApiModule::create(
        engine.clone(),
        Some(serde_json::to_value(&config).unwrap()),
    )
    .await
    .unwrap();

    module.initialize().await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();

    // Try to register with invalid path (contains ..)
    let register_payload = json!({
        "function_path": "invalid..path",
        "invocation": {
            "url": "http://example.com/test",
            "method": "POST"
        }
    });

    let response = client
        .post("http://127.0.0.1:49142/admin/functions")
        .header("Authorization", "Bearer test-token-invalid")
        .json(&register_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
