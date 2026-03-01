// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::{Client, Method};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    invocation::{
        method::{HttpAuth, HttpMethod},
        signature::sign_request,
        url_validator::{SecurityError, UrlValidator, UrlValidatorConfig},
    },
    protocol::ErrorBody,
};

pub struct HttpEndpointParams<'a> {
    pub url: &'a str,
    pub method: &'a HttpMethod,
    pub timeout_ms: &'a Option<u64>,
    pub headers: &'a HashMap<String, String>,
    pub auth: &'a Option<HttpAuth>,
}

pub struct HttpInvokerConfig {
    pub url_validator: UrlValidatorConfig,
    pub default_timeout_ms: u64,
    pub pool_max_idle_per_host: usize,
    pub client_timeout_secs: u64,
}

impl Default for HttpInvokerConfig {
    fn default() -> Self {
        Self {
            url_validator: UrlValidatorConfig::default(),
            default_timeout_ms: 30000,
            pool_max_idle_per_host: 50,
            client_timeout_secs: 60,
        }
    }
}

pub struct HttpInvoker {
    client: Client,
    url_validator: UrlValidator,
    default_timeout_ms: u64,
}

impl HttpInvoker {
    pub fn new(config: HttpInvokerConfig) -> Result<Self, SecurityError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.client_timeout_secs))
            .pool_max_idle_per_host(config.pool_max_idle_per_host)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| SecurityError::InvalidUrl)?;
        let url_validator = UrlValidator::new(config.url_validator)?;
        Ok(Self {
            client,
            url_validator,
            default_timeout_ms: config.default_timeout_ms,
        })
    }

    pub fn url_validator(&self) -> &UrlValidator {
        &self.url_validator
    }

    fn build_base_request(
        &self,
        endpoint: &HttpEndpointParams,
        function_path: &str,
        invocation_id: &str,
        timestamp: u64,
        body: Vec<u8>,
    ) -> reqwest::RequestBuilder {
        let timeout = endpoint.timeout_ms.unwrap_or(self.default_timeout_ms);

        let mut request = self
            .client
            .request(http_method_to_reqwest(endpoint.method), endpoint.url)
            .timeout(Duration::from_millis(timeout))
            .header("Content-Type", "application/json")
            .header("x-iii-Function-Path", function_path)
            .header("x-iii-Invocation-ID", invocation_id)
            .header("x-iii-Timestamp", timestamp.to_string());

        for (key, value) in endpoint.headers {
            request = request.header(key, value);
        }

        request.body(body)
    }

    fn apply_auth(
        &self,
        request: reqwest::RequestBuilder,
        auth: &Option<HttpAuth>,
        body: &[u8],
        timestamp: u64,
    ) -> reqwest::RequestBuilder {
        match auth {
            Some(HttpAuth::Hmac { secret }) => {
                let signature = sign_request(body, secret, timestamp);
                request.header("x-iii-Signature", signature)
            }
            Some(HttpAuth::Bearer { token }) => request.bearer_auth(token),
            Some(HttpAuth::ApiKey { header, value }) => request.header(header, value),
            None => request,
        }
    }

    fn parse_error_response(status: reqwest::StatusCode, bytes: &[u8]) -> ErrorBody {
        let error_json: Option<Value> = serde_json::from_slice(bytes).ok();
        if let Some(error_json) = error_json
            && let Some(error_obj) = error_json.get("error")
        {
            let code = error_obj
                .get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("http_error")
                .to_string();
            let message = error_obj
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("HTTP request failed")
                .to_string();
            return ErrorBody { code, message };
        }

        ErrorBody {
            code: "http_error".into(),
            message: format!("HTTP {}", status),
        }
    }

    /// Computes a timestamp and serializes the payload to bytes.
    fn prepare_request(&self, data: &Value) -> Result<(u64, Vec<u8>), ErrorBody> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| ErrorBody {
                code: "timestamp_error".into(),
                message: err.to_string(),
            })?
            .as_secs();

        let body = serde_json::to_vec(data).map_err(|err| ErrorBody {
            code: "serialization_error".into(),
            message: err.to_string(),
        })?;

        Ok((timestamp, body))
    }

    async fn validate_url(&self, url: &str) -> Result<(), ErrorBody> {
        self.url_validator
            .validate(url)
            .await
            .map_err(|e| ErrorBody {
                code: "url_validation_failed".into(),
                message: e.to_string(),
            })
    }

    pub async fn deliver_webhook(
        &self,
        function_path: &str,
        endpoint: &HttpEndpointParams<'_>,
        trigger_type: &str,
        trigger_id: &str,
        payload: Value,
    ) -> Result<(), ErrorBody> {
        self.validate_url(endpoint.url).await?;
        let (timestamp, body) = self.prepare_request(&payload)?;

        let invocation_id = Uuid::new_v4().to_string();
        let trace_id_value = format!("trace-{}", Uuid::new_v4());

        let mut request = self.build_base_request(
            endpoint,
            function_path,
            &invocation_id,
            timestamp,
            body.clone(),
        );

        request = request
            .header("x-iii-Trace-ID", &trace_id_value)
            .header("x-iii-Trigger-Type", trigger_type)
            .header("x-iii-Trigger-ID", trigger_id);

        request = self.apply_auth(request, endpoint.auth, &body, timestamp);

        let response = request.send().await.map_err(|err| ErrorBody {
            code: "http_request_failed".into(),
            message: err.to_string(),
        })?;

        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let bytes = response.bytes().await.map_err(|err| ErrorBody {
            code: "http_response_failed".into(),
            message: err.to_string(),
        })?;

        Err(Self::parse_error_response(status, &bytes))
    }

    pub async fn invoke_http(
        &self,
        function_path: &str,
        endpoint: &HttpEndpointParams<'_>,
        invocation_id: Uuid,
        data: Value,
        caller_function: Option<&str>,
        trace_id: Option<&str>,
    ) -> Result<Option<Value>, ErrorBody> {
        self.validate_url(endpoint.url).await?;
        let (timestamp, body) = self.prepare_request(&data)?;

        let mut request = self.build_base_request(
            endpoint,
            function_path,
            &invocation_id.to_string(),
            timestamp,
            body.clone(),
        );

        if let Some(caller) = caller_function {
            request = request.header("x-iii-Caller-Function", caller);
        }
        if let Some(trace) = trace_id {
            request = request.header("x-iii-Trace-ID", trace);
        }

        request = self.apply_auth(request, endpoint.auth, &body, timestamp);

        let response = request.send().await.map_err(|err| ErrorBody {
            code: "http_request_failed".into(),
            message: err.to_string(),
        })?;

        let status = response.status();
        let bytes = response.bytes().await.map_err(|err| ErrorBody {
            code: "http_response_failed".into(),
            message: err.to_string(),
        })?;

        if status.is_success() {
            if bytes.is_empty() {
                return Ok(None);
            }
            let result: Value = serde_json::from_slice(&bytes).map_err(|err| ErrorBody {
                code: "invalid_response".into(),
                message: err.to_string(),
            })?;
            return Ok(Some(result));
        }

        Err(Self::parse_error_response(status, &bytes))
    }
}

// Invoker trait is no longer needed for the new design
// HTTP functions use handler wrappers instead

fn http_method_to_reqwest(method: &HttpMethod) -> Method {
    match method {
        HttpMethod::Get => Method::GET,
        HttpMethod::Post => Method::POST,
        HttpMethod::Put => Method::PUT,
        HttpMethod::Patch => Method::PATCH,
        HttpMethod::Delete => Method::DELETE,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use axum::{
        Json, Router,
        body::Bytes,
        extract::State,
        http::{HeaderMap, Method as AxumMethod, StatusCode},
        routing::any,
    };
    use serde_json::json;
    use tokio::net::TcpListener;

    use super::*;

    #[derive(Clone, Default)]
    struct RequestCapture {
        headers: Arc<Mutex<HashMap<String, String>>>,
        body: Arc<Mutex<Option<Value>>>,
        method: Arc<Mutex<Option<String>>>,
    }

    fn permissive_invoker() -> HttpInvoker {
        HttpInvoker::new(HttpInvokerConfig {
            url_validator: UrlValidatorConfig {
                require_https: false,
                block_private_ips: false,
                allowlist: vec!["*".to_string()],
            },
            ..HttpInvokerConfig::default()
        })
        .expect("create permissive invoker")
    }

    fn store_request(
        capture: &RequestCapture,
        method: AxumMethod,
        headers: HeaderMap,
        body: Bytes,
    ) {
        let mut header_map = HashMap::new();
        for (name, value) in &headers {
            header_map.insert(
                name.as_str().to_string(),
                value.to_str().unwrap_or_default().to_string(),
            );
        }

        *capture.method.lock().expect("lock method") = Some(method.to_string());
        *capture.headers.lock().expect("lock headers") = header_map;
        *capture.body.lock().expect("lock body") = Some(if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).expect("request body should be valid json")
        });
    }

    async fn success_handler(
        State(capture): State<RequestCapture>,
        method: AxumMethod,
        headers: HeaderMap,
        body: Bytes,
    ) -> (StatusCode, Json<Value>) {
        store_request(&capture, method, headers, body);
        (StatusCode::OK, Json(json!({ "ok": true })))
    }

    async fn empty_handler() -> StatusCode {
        StatusCode::NO_CONTENT
    }

    async fn invalid_handler() -> &'static str {
        "not-json"
    }

    async fn error_json_handler() -> (StatusCode, Json<Value>) {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": {
                    "code": "upstream_failure",
                    "message": "gateway failed"
                }
            })),
        )
    }

    async fn error_plain_handler() -> (StatusCode, &'static str) {
        (StatusCode::BAD_REQUEST, "bad request")
    }

    async fn webhook_handler(
        State(capture): State<RequestCapture>,
        method: AxumMethod,
        headers: HeaderMap,
        body: Bytes,
    ) -> StatusCode {
        store_request(&capture, method, headers, body);
        StatusCode::ACCEPTED
    }

    async fn spawn_server(capture: RequestCapture) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("resolve local addr");
        let app = Router::new()
            .route("/success", any(success_handler))
            .route("/empty", any(empty_handler))
            .route("/invalid", any(invalid_handler))
            .route("/error-json", any(error_json_handler))
            .route("/error-plain", any(error_plain_handler))
            .route("/webhook", any(webhook_handler))
            .with_state(capture);

        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        format!("http://{addr}")
    }

    #[test]
    fn http_method_to_reqwest_maps_all_variants() {
        assert_eq!(http_method_to_reqwest(&HttpMethod::Get), Method::GET);
        assert_eq!(http_method_to_reqwest(&HttpMethod::Post), Method::POST);
        assert_eq!(http_method_to_reqwest(&HttpMethod::Put), Method::PUT);
        assert_eq!(http_method_to_reqwest(&HttpMethod::Patch), Method::PATCH);
        assert_eq!(http_method_to_reqwest(&HttpMethod::Delete), Method::DELETE);
    }

    #[test]
    fn build_base_request_and_apply_auth_set_expected_headers() {
        let invoker = permissive_invoker();
        let url = "https://example.com/endpoint";
        let method = HttpMethod::Post;
        let timeout_ms = Some(1234);
        let mut headers = HashMap::new();
        headers.insert("x-extra".to_string(), "value".to_string());
        let no_auth = None;
        let endpoint = HttpEndpointParams {
            url,
            method: &method,
            timeout_ms: &timeout_ms,
            headers: &headers,
            auth: &no_auth,
        };
        let payload = br#"{"hello":"world"}"#.to_vec();
        let timestamp = 1_700_000_000;

        let base = invoker
            .build_base_request(&endpoint, "svc.echo", "inv-1", timestamp, payload.clone())
            .build()
            .expect("build base request");
        assert_eq!(base.method(), Method::POST);
        assert_eq!(base.url().as_str(), url);
        assert_eq!(
            base.headers()
                .get("x-iii-Function-Path")
                .expect("function path header"),
            "svc.echo"
        );
        assert_eq!(
            base.headers()
                .get("x-iii-Invocation-ID")
                .expect("invocation header"),
            "inv-1"
        );
        assert_eq!(
            base.headers()
                .get("x-iii-Timestamp")
                .expect("timestamp header"),
            timestamp.to_string().as_str()
        );
        assert_eq!(
            base.headers().get("x-extra").expect("custom header"),
            "value"
        );

        let hmac_request = invoker
            .apply_auth(
                invoker.build_base_request(
                    &endpoint,
                    "svc.echo",
                    "inv-1",
                    timestamp,
                    payload.clone(),
                ),
                &Some(HttpAuth::Hmac {
                    secret: "secret".to_string(),
                }),
                &payload,
                timestamp,
            )
            .build()
            .expect("build hmac request");
        assert_eq!(
            hmac_request
                .headers()
                .get("x-iii-Signature")
                .expect("signature header"),
            sign_request(&payload, "secret", timestamp).as_str()
        );

        let bearer_request = invoker
            .apply_auth(
                invoker.build_base_request(
                    &endpoint,
                    "svc.echo",
                    "inv-1",
                    timestamp,
                    payload.clone(),
                ),
                &Some(HttpAuth::Bearer {
                    token: "token-123".to_string(),
                }),
                &payload,
                timestamp,
            )
            .build()
            .expect("build bearer request");
        assert_eq!(
            bearer_request
                .headers()
                .get("authorization")
                .expect("authorization header"),
            "Bearer token-123"
        );

        let api_key_request = invoker
            .apply_auth(
                invoker.build_base_request(
                    &endpoint,
                    "svc.echo",
                    "inv-1",
                    timestamp,
                    payload.clone(),
                ),
                &Some(HttpAuth::ApiKey {
                    header: "x-api-key".to_string(),
                    value: "key-123".to_string(),
                }),
                &payload,
                timestamp,
            )
            .build()
            .expect("build api key request");
        assert_eq!(
            api_key_request
                .headers()
                .get("x-api-key")
                .expect("api key header"),
            "key-123"
        );
    }

    #[test]
    fn parse_error_response_uses_structured_error_and_fallbacks() {
        let structured = HttpInvoker::parse_error_response(
            reqwest::StatusCode::BAD_GATEWAY,
            br#"{"error":{"code":"upstream_failure","message":"gateway failed"}}"#,
        );
        assert_eq!(structured.code, "upstream_failure");
        assert_eq!(structured.message, "gateway failed");

        let missing_fields =
            HttpInvoker::parse_error_response(reqwest::StatusCode::BAD_REQUEST, br#"{"error":{}}"#);
        assert_eq!(missing_fields.code, "http_error");
        assert_eq!(missing_fields.message, "HTTP request failed");

        let fallback =
            HttpInvoker::parse_error_response(reqwest::StatusCode::INTERNAL_SERVER_ERROR, b"oops");
        assert_eq!(fallback.code, "http_error");
        assert_eq!(fallback.message, "HTTP 500 Internal Server Error");
    }

    #[test]
    fn prepare_request_serializes_payload_with_recent_timestamp() {
        let invoker = permissive_invoker();
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time should be after epoch")
            .as_secs();
        let (timestamp, body) = invoker
            .prepare_request(&json!({ "hello": "world" }))
            .expect("prepare request");
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time should be after epoch")
            .as_secs();

        assert!(timestamp >= before);
        assert!(timestamp <= after);
        assert_eq!(
            serde_json::from_slice::<Value>(&body).expect("deserialize request body"),
            json!({ "hello": "world" })
        );
    }

    #[tokio::test]
    async fn validate_url_and_invoke_http_cover_success_none_invalid_and_error_paths() {
        let capture = RequestCapture::default();
        let base_url = spawn_server(capture.clone()).await;
        let invoker = permissive_invoker();
        let success_url = format!("{base_url}/success");
        let empty_url = format!("{base_url}/empty");
        let invalid_url = format!("{base_url}/invalid");
        let error_url = format!("{base_url}/error-json");

        invoker
            .validate_url(&success_url)
            .await
            .expect("permissive validator should allow local url");

        let strict_invoker =
            HttpInvoker::new(HttpInvokerConfig::default()).expect("strict invoker");
        let validation_error = strict_invoker
            .validate_url(&success_url)
            .await
            .expect_err("strict validator should reject http localhost");
        assert_eq!(validation_error.code, "url_validation_failed");

        let method = HttpMethod::Post;
        let timeout_ms = Some(500);
        let mut headers = HashMap::new();
        headers.insert("x-extra".to_string(), "value".to_string());
        let auth = Some(HttpAuth::Bearer {
            token: "token-123".to_string(),
        });
        let success_endpoint = HttpEndpointParams {
            url: &success_url,
            method: &method,
            timeout_ms: &timeout_ms,
            headers: &headers,
            auth: &auth,
        };

        let success = invoker
            .invoke_http(
                "svc.echo",
                &success_endpoint,
                Uuid::nil(),
                json!({ "hello": "world" }),
                Some("caller.fn"),
                Some("trace-123"),
            )
            .await
            .expect("invoke success");
        assert_eq!(success, Some(json!({ "ok": true })));

        let captured_headers = capture.headers.lock().expect("lock headers").clone();
        assert_eq!(
            captured_headers
                .get("authorization")
                .expect("authorization header"),
            "Bearer token-123"
        );
        assert_eq!(
            captured_headers
                .get("x-iii-caller-function")
                .expect("caller function header"),
            "caller.fn"
        );
        assert_eq!(
            captured_headers
                .get("x-iii-trace-id")
                .expect("trace header"),
            "trace-123"
        );
        assert_eq!(
            capture.body.lock().expect("lock body").clone(),
            Some(json!({ "hello": "world" }))
        );

        let no_auth = None;
        let empty_headers = HashMap::new();
        let empty_timeout = None;
        let empty_endpoint = HttpEndpointParams {
            url: &empty_url,
            method: &HttpMethod::Get,
            timeout_ms: &empty_timeout,
            headers: &empty_headers,
            auth: &no_auth,
        };
        let empty = invoker
            .invoke_http(
                "svc.empty",
                &empty_endpoint,
                Uuid::nil(),
                json!({}),
                None,
                None,
            )
            .await
            .expect("empty body should return none");
        assert_eq!(empty, None);

        let invalid_headers = HashMap::new();
        let invalid_timeout = None;
        let invalid_endpoint = HttpEndpointParams {
            url: &invalid_url,
            method: &HttpMethod::Get,
            timeout_ms: &invalid_timeout,
            headers: &invalid_headers,
            auth: &no_auth,
        };
        let invalid = invoker
            .invoke_http(
                "svc.invalid",
                &invalid_endpoint,
                Uuid::nil(),
                json!({}),
                None,
                None,
            )
            .await
            .expect_err("invalid json should fail");
        assert_eq!(invalid.code, "invalid_response");

        let error_headers = HashMap::new();
        let error_timeout = None;
        let error_endpoint = HttpEndpointParams {
            url: &error_url,
            method: &HttpMethod::Get,
            timeout_ms: &error_timeout,
            headers: &error_headers,
            auth: &no_auth,
        };
        let error = invoker
            .invoke_http(
                "svc.error",
                &error_endpoint,
                Uuid::nil(),
                json!({}),
                None,
                None,
            )
            .await
            .expect_err("error response should fail");
        assert_eq!(error.code, "upstream_failure");
        assert_eq!(error.message, "gateway failed");
    }

    #[tokio::test]
    async fn deliver_webhook_sets_trigger_headers_and_hmac_signature() {
        let capture = RequestCapture::default();
        let base_url = spawn_server(capture.clone()).await;
        let invoker = permissive_invoker();
        let auth = Some(HttpAuth::Hmac {
            secret: "top-secret".to_string(),
        });
        let timeout_ms = Some(700);
        let headers = HashMap::new();
        let payload = json!({ "event": "created" });
        let expected_body = serde_json::to_vec(&payload).expect("serialize payload");
        let webhook_url = format!("{base_url}/webhook");
        let endpoint = HttpEndpointParams {
            url: &webhook_url,
            method: &HttpMethod::Post,
            timeout_ms: &timeout_ms,
            headers: &headers,
            auth: &auth,
        };

        invoker
            .deliver_webhook(
                "svc.webhook",
                &endpoint,
                "queue",
                "trigger-1",
                payload.clone(),
            )
            .await
            .expect("deliver webhook");

        let headers = capture.headers.lock().expect("lock headers").clone();
        let timestamp = headers
            .get("x-iii-timestamp")
            .expect("timestamp header")
            .parse::<u64>()
            .expect("timestamp should parse");
        assert_eq!(
            headers.get("x-iii-trigger-type"),
            Some(&"queue".to_string())
        );
        assert_eq!(
            headers.get("x-iii-trigger-id"),
            Some(&"trigger-1".to_string())
        );
        assert_eq!(
            headers.get("x-iii-function-path"),
            Some(&"svc.webhook".to_string())
        );
        assert!(
            headers
                .get("x-iii-trace-id")
                .expect("trace id header")
                .starts_with("trace-")
        );
        assert_eq!(
            headers.get("x-iii-signature"),
            Some(&sign_request(&expected_body, "top-secret", timestamp))
        );
        assert_eq!(
            capture.body.lock().expect("lock body").clone(),
            Some(payload)
        );
    }

    #[tokio::test]
    async fn deliver_webhook_propagates_plain_http_errors() {
        let capture = RequestCapture::default();
        let base_url = spawn_server(capture).await;
        let invoker = permissive_invoker();
        let no_auth = None;
        let timeout_ms = None;
        let headers = HashMap::new();
        let error_url = format!("{base_url}/error-plain");
        let endpoint = HttpEndpointParams {
            url: &error_url,
            method: &HttpMethod::Post,
            timeout_ms: &timeout_ms,
            headers: &headers,
            auth: &no_auth,
        };

        let error = invoker
            .deliver_webhook("svc.webhook", &endpoint, "queue", "trigger-1", json!({}))
            .await
            .expect_err("plain error response should fail");
        assert_eq!(error.code, "http_error");
        assert_eq!(error.message, "HTTP 400 Bad Request");
    }
}
