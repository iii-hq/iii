mod common;

use std::collections::HashMap;
use std::time::Duration;

use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

use iii::invocation::http_invoker::HttpEndpointParams;
use iii::invocation::method::{HttpAuth, HttpMethod};

use common::http_helpers::permissive_invoker;

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
async fn timeout_returns_actionable_error() {
    let mock_server = MockServer::start().await;

    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .expect(1)
        .mount(&mock_server)
        .await;

    let invoker = permissive_invoker();
    let url = format!("{}/slow", mock_server.uri());
    let timeout_ms = Some(100_u64);
    let method = HttpMethod::Post;
    let headers = HashMap::new();
    let no_auth: Option<HttpAuth> = None;
    let endpoint = make_endpoint(&url, &timeout_ms, &method, &headers, &no_auth);

    let result = invoker
        .invoke_http(
            "test.timeout",
            &endpoint,
            Uuid::new_v4(),
            json!({"test": true}),
            None,
            None,
        )
        .await;

    let error = result.expect_err("should fail with timeout");
    assert_eq!(error.code, "http_request_failed");
    assert!(
        error.message.to_lowercase().contains("timed out"),
        "message should mention timeout, got: {}",
        error.message,
    );
}

#[tokio::test]
async fn connection_refused_returns_clear_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener); // Port is now closed

    let invoker = permissive_invoker();
    let url = format!("http://{}/endpoint", addr);
    let timeout_ms = Some(5000_u64);
    let method = HttpMethod::Post;
    let headers = HashMap::new();
    let no_auth: Option<HttpAuth> = None;
    let endpoint = make_endpoint(&url, &timeout_ms, &method, &headers, &no_auth);

    let result = invoker
        .invoke_http(
            "test.connection_refused",
            &endpoint,
            Uuid::new_v4(),
            json!({"test": true}),
            None,
            None,
        )
        .await;

    let error = result.expect_err("should fail with connection refused");
    assert_eq!(error.code, "http_request_failed");
    assert!(
        !error.message.is_empty(),
        "error message should be non-empty",
    );
    assert!(
        error.message.to_lowercase().contains("connect"),
        "message should mention connection failure, got: {}",
        error.message,
    );
}

#[tokio::test]
async fn structured_error_parsed_from_json() {
    let mock_server = MockServer::start().await;

    Mock::given(any())
        .respond_with(ResponseTemplate::new(502).set_body_json(json!({
            "error": {
                "code": "upstream_failure",
                "message": "gateway failed"
            }
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let invoker = permissive_invoker();
    let url = format!("{}/error-json", mock_server.uri());
    let timeout_ms: Option<u64> = None;
    let method = HttpMethod::Post;
    let headers = HashMap::new();
    let no_auth: Option<HttpAuth> = None;
    let endpoint = make_endpoint(&url, &timeout_ms, &method, &headers, &no_auth);

    let result = invoker
        .invoke_http(
            "test.structured_error",
            &endpoint,
            Uuid::new_v4(),
            json!({"test": true}),
            None,
            None,
        )
        .await;

    let error = result.expect_err("should fail with structured error");
    assert_eq!(error.code, "upstream_failure");
    assert_eq!(error.message, "gateway failed");
}

#[tokio::test]
async fn unstructured_error_falls_back_to_status() {
    let mock_server = MockServer::start().await;

    Mock::given(any())
        .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let invoker = permissive_invoker();
    let url = format!("{}/error-plain", mock_server.uri());
    let timeout_ms: Option<u64> = None;
    let method = HttpMethod::Post;
    let headers = HashMap::new();
    let no_auth: Option<HttpAuth> = None;
    let endpoint = make_endpoint(&url, &timeout_ms, &method, &headers, &no_auth);

    let result = invoker
        .invoke_http(
            "test.unstructured_error",
            &endpoint,
            Uuid::new_v4(),
            json!({"test": true}),
            None,
            None,
        )
        .await;

    let error = result.expect_err("should fail with unstructured error");
    assert_eq!(error.code, "http_error");
    assert_eq!(error.message, "HTTP 400 Bad Request");
}

#[tokio::test]
async fn empty_body_204_returns_none() {
    let mock_server = MockServer::start().await;

    Mock::given(any())
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&mock_server)
        .await;

    let invoker = permissive_invoker();
    let url = format!("{}/empty", mock_server.uri());
    let timeout_ms: Option<u64> = None;
    let method = HttpMethod::Post;
    let headers = HashMap::new();
    let no_auth: Option<HttpAuth> = None;
    let endpoint = make_endpoint(&url, &timeout_ms, &method, &headers, &no_auth);

    let result = invoker
        .invoke_http(
            "test.empty_body",
            &endpoint,
            Uuid::new_v4(),
            json!({"test": true}),
            None,
            None,
        )
        .await;

    let value = result.expect("204 should succeed");
    assert_eq!(value, None);
}
