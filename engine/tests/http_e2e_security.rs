mod common;

use std::collections::HashMap;

use serde_json::json;
use uuid::Uuid;

use iii::invocation::http_invoker::HttpEndpointParams;
use iii::invocation::method::{HttpAuth, HttpMethod};

use common::http_helpers::strict_invoker;

fn make_endpoint<'a>(
    url: &'a str,
    timeout_ms: &'a Option<u64>,
    method: &'a HttpMethod,
    headers: &'a HashMap<String, String>,
    auth: &'a Option<HttpAuth>,
) -> HttpEndpointParams<'a> {
    HttpEndpointParams {
        url,
        method,
        timeout_ms,
        headers,
        auth,
    }
}

#[tokio::test]
async fn private_ip_blocked_rejects_rfc1918_class_a() {
    let invoker = strict_invoker();
    let url = "https://10.0.0.1/api";
    let timeout_ms: Option<u64> = None;
    let method = HttpMethod::Post;
    let headers = HashMap::new();
    let no_auth: Option<HttpAuth> = None;
    let endpoint = make_endpoint(&url, &timeout_ms, &method, &headers, &no_auth);

    let result = invoker
        .invoke_http(
            "test.private_ip_class_a",
            &endpoint,
            Uuid::new_v4(),
            json!({"test": true}),
            None,
            None,
        )
        .await;

    let error = result.expect_err("should reject private IP 10.0.0.1");
    assert_eq!(error.code, "url_validation_failed");
    assert_eq!(error.message, "Private IP blocked");
}

#[tokio::test]
async fn private_ip_blocked_rejects_rfc1918_class_b() {
    let invoker = strict_invoker();
    let url = "https://172.16.0.1/api";
    let timeout_ms: Option<u64> = None;
    let method = HttpMethod::Post;
    let headers = HashMap::new();
    let no_auth: Option<HttpAuth> = None;
    let endpoint = make_endpoint(&url, &timeout_ms, &method, &headers, &no_auth);

    let result = invoker
        .invoke_http(
            "test.private_ip_class_b",
            &endpoint,
            Uuid::new_v4(),
            json!({"test": true}),
            None,
            None,
        )
        .await;

    let error = result.expect_err("should reject private IP 172.16.0.1");
    assert_eq!(error.code, "url_validation_failed");
    assert_eq!(error.message, "Private IP blocked");
}

#[tokio::test]
async fn private_ip_blocked_rejects_rfc1918_class_c() {
    let invoker = strict_invoker();
    let url = "https://192.168.1.1/api";
    let timeout_ms: Option<u64> = None;
    let method = HttpMethod::Post;
    let headers = HashMap::new();
    let no_auth: Option<HttpAuth> = None;
    let endpoint = make_endpoint(&url, &timeout_ms, &method, &headers, &no_auth);

    let result = invoker
        .invoke_http(
            "test.private_ip_class_c",
            &endpoint,
            Uuid::new_v4(),
            json!({"test": true}),
            None,
            None,
        )
        .await;

    let error = result.expect_err("should reject private IP 192.168.1.1");
    assert_eq!(error.code, "url_validation_failed");
    assert_eq!(error.message, "Private IP blocked");
}

#[tokio::test]
async fn https_enforcement_rejects_http_url() {
    let invoker = strict_invoker();
    let url = "http://example.com/api";
    let timeout_ms: Option<u64> = None;
    let method = HttpMethod::Post;
    let headers = HashMap::new();
    let no_auth: Option<HttpAuth> = None;
    let endpoint = make_endpoint(&url, &timeout_ms, &method, &headers, &no_auth);

    let result = invoker
        .invoke_http(
            "test.https_enforcement",
            &endpoint,
            Uuid::new_v4(),
            json!({"test": true}),
            None,
            None,
        )
        .await;

    let error = result.expect_err("should reject HTTP URL when HTTPS is required");
    assert_eq!(error.code, "url_validation_failed");
    assert_eq!(error.message, "HTTPS is required");
}

#[tokio::test]
async fn dns_lookup_failed_returns_security_error() {
    let invoker = strict_invoker();
    let url = "https://nonexistent.invalid/api";
    let timeout_ms: Option<u64> = None;
    let method = HttpMethod::Post;
    let headers = HashMap::new();
    let no_auth: Option<HttpAuth> = None;
    let endpoint = make_endpoint(&url, &timeout_ms, &method, &headers, &no_auth);

    let result = invoker
        .invoke_http(
            "test.dns_lookup_failed",
            &endpoint,
            Uuid::new_v4(),
            json!({"test": true}),
            None,
            None,
        )
        .await;

    let error = result.expect_err("should fail with DNS lookup error for .invalid TLD");
    assert_eq!(error.code, "url_validation_failed");
    assert!(
        error.message.contains("DNS lookup failed"),
        "message should contain 'DNS lookup failed', got: {}",
        error.message,
    );
}
