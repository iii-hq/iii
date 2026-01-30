use std::collections::HashMap;

use chrono::Utc;
use iii::{
    config::{
        SecurityConfig,
        persistence::{
            HttpFunctionConfig as KvHttpFunctionConfig, load_http_functions_from_kv,
            store_http_function_in_kv,
        },
    },
    engine::Engine,
    invocation::url_validator::{SecurityError, UrlValidator, UrlValidatorConfig},
    invocation::{
        method::HttpMethod,
        signature::{sign_request, verify_signature},
    },
};

#[tokio::test]
async fn test_signature_roundtrip() {
    let body = br#"{"key":"value"}"#;
    let secret = "test_secret";
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let signature = sign_request(body, secret, timestamp);
    let result = verify_signature(body, &signature, secret, timestamp, 300);
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_url_validator_private_ip() {
    let config = UrlValidatorConfig {
        allowlist: vec!["127.0.0.1".to_string()],
        block_private_ips: true,
        require_https: false,
    };
    let validator = UrlValidator::new(config).unwrap();
    let result = validator.validate("http://127.0.0.1/test").await;
    assert!(matches!(result, Err(SecurityError::PrivateIpBlocked)));
}

#[tokio::test]
async fn test_url_validator_allowlist() {
    let config = UrlValidatorConfig {
        allowlist: vec!["example.com".to_string()],
        block_private_ips: false,
        require_https: false,
    };
    let validator = UrlValidator::new(config).unwrap();
    let result = validator.validate("http://example.com/path").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_kv_persistence_load() {
    let engine = Engine::new_with_security(SecurityConfig::default(), None).unwrap();
    let config = KvHttpFunctionConfig {
        function_path: "geo.lookup".to_string(),
        url: "https://example.com/invoke".to_string(),
        method: HttpMethod::Post,
        timeout_ms: 1000,
        headers: HashMap::new(),
        auth: None,
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        registered_at: Utc::now(),
    };
    store_http_function_in_kv(&engine, &config).await.unwrap();
    load_http_functions_from_kv(&engine).await.unwrap();
    assert!(engine.functions.get("geo.lookup").is_some());
}
