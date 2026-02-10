// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    body::Bytes,
    extract::{Extension, Query},
    http::{StatusCode, Uri, header::HeaderMap},
    response::IntoResponse,
};
use serde_json::{Value, json};
use tracing::Instrument;

/// Generates a short error ID for correlation between client responses and internal logs.
fn generate_error_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", timestamp & 0xFFFFFFFFFFFF) // 12 hex chars
}

use super::{
    RestApiCoreModule,
    types::{APIrequest, APIresponse},
};
use crate::engine::{Engine, EngineTrait};

// Helper function to extract all path parameters from a route pattern and actual path
// Returns a HashMap<String, String> where keys are parameter names (without ':') and values are their corresponding values
// Example:
//   registered_path = "/users/:id/posts/:post_id"
//   actual_path = "/users/123/posts/456"
//   Returns: {"id": "123", "post_id": "456"}
fn extract_path_params(registered_path: &str, actual_path: &str) -> HashMap<String, String> {
    let registered_segments: Vec<&str> = registered_path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let actual_segments: Vec<&str> = actual_path.split('/').filter(|s| !s.is_empty()).collect();

    let mut params = HashMap::new();

    // Only proceed if we have the same number of segments
    if registered_segments.len() != actual_segments.len() {
        return params;
    }

    // Match segments and extract parameters (segments starting with :)
    for (i, registered_seg) in registered_segments.iter().enumerate() {
        if registered_seg.starts_with(':') {
            // Extract parameter name (remove the ':')
            let param_name = registered_seg
                .strip_prefix(':')
                .unwrap_or(registered_seg)
                .to_string();

            // Extract the corresponding value from actual path
            if let Some(actual_value) = actual_segments.get(i) {
                params.insert(param_name, actual_value.to_string());
            }
        }
    }

    params
}

/// Headers that should have their values redacted in logs
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "api-key",
    "x-auth-token",
    "x-access-token",
    "x-secret",
    "x-csrf-token",
    "proxy-authorization",
];

/// Sanitizes headers for safe logging by redacting sensitive header values
fn sanitize_headers_for_logging(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(name, value)| {
            let name_lower = name.as_str().to_lowercase();
            let sanitized_value = if SENSITIVE_HEADERS.contains(&name_lower.as_str()) {
                "[REDACTED]".to_string()
            } else {
                value.to_str().unwrap_or("[non-utf8]").to_string()
            };
            (name.to_string(), sanitized_value)
        })
        .collect()
}

/// Sanitizes query parameters for safe logging by showing only keys
fn sanitize_query_params_for_logging(params: &HashMap<String, String>) -> Vec<String> {
    params.keys().cloned().collect()
}

#[allow(clippy::too_many_arguments)]
#[axum::debug_handler]
pub async fn dynamic_handler(
    method: axum::http::Method,
    uri: Uri,
    headers: HeaderMap,
    Extension(engine): Extension<Arc<Engine>>,
    Extension(api_handler): Extension<Arc<RestApiCoreModule>>,
    Extension(registered_path): Extension<String>,
    Query(query_params): Query<HashMap<String, String>>,
    body: Bytes,
) -> impl IntoResponse {
    let actual_path = uri.path().to_string();
    let query_string = uri.query().unwrap_or("").to_string();
    let request_body_size = body.len();

    // Extract common headers for span attributes
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Extract X-Forwarded-Proto or default to http
    let url_scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http")
        .to_string();

    // Build full URL for the url.full attribute
    let url_full = if query_string.is_empty() {
        format!("{}://{}{}", url_scheme, host, actual_path)
    } else {
        format!("{}://{}{}?{}", url_scheme, host, actual_path, query_string)
    };

    // Create HTTP span with OTEL semantic convention attributes
    let span = tracing::info_span!(
        "HTTP",
        otel.name = %format!("{} {}", method, registered_path),
        otel.kind = "server",
        otel.status_code = tracing::field::Empty,
        "http.request.method" = %method,
        "http.route" = %registered_path,
        "url.path" = %actual_path,
        "url.query" = %query_string,
        "url.scheme" = %url_scheme,
        "url.full" = %url_full,
        "server.address" = %host,
        "user_agent.original" = %user_agent,
        "http.request.header.content_type" = %content_type,
        "http.request.body.size" = %request_body_size,
        "http.response.status_code" = tracing::field::Empty,
    );

    async move {
        tracing::debug!("Registered route path: {}", registered_path);
        tracing::debug!("Actual path: {}", actual_path);
        tracing::debug!("HTTP Method: {}", method);
        tracing::debug!(
            "Query parameters (keys only): {:?}",
            sanitize_query_params_for_logging(&query_params)
        );
        tracing::debug!(
            "Headers (sensitive values redacted): {:?}",
            sanitize_headers_for_logging(&headers)
        );
        let path_parameters: HashMap<String, String> =
            extract_path_params(&registered_path, &actual_path);

        if let Some((function_id, condition_function_id)) =
            api_handler.get_router(method.as_str(), &registered_path)

        {
            let parsed_body = if body.is_empty() {
            None
        } else {
            match serde_json::from_slice::<Value>(&body) {
                Ok(json) => Some(Json(json)),
                Err(e) => {
                    tracing::error!("Failed to parse request body as JSON: {}", e);
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error": format!("Failed to parse the request body as JSON: {}", e)})),
                    )
                        .into_response();
                }
            }
        };

        let api_request = APIrequest::new(
            query_params.clone(),
            path_parameters.clone(),
            headers,
            registered_path.clone(),
            method.as_str().to_string(),
            parsed_body,
            );

            let api_request_value = serde_json::to_value(api_request).unwrap_or(serde_json::json!({}));

            if let Some(condition_function_id) = condition_function_id.as_ref() {
            tracing::debug!(
                condition_function_id = %condition_function_id,
                "Checking trigger conditions"
            );

            match engine
                .invoke_function(condition_function_id, api_request_value.clone())
                .await
            {
                Ok(Some(result)) => {
                    if let Some(passed) = result.as_bool()
                        && !passed
                    {
                        tracing::debug!(
                            function_id = %function_id,
                            "Condition check failed, skipping handler"
                        );
                        return (
                            StatusCode::UNPROCESSABLE_ENTITY,
                            Json(json!({"error": "Request condition not met", "skipped": true})),
                        )
                            .into_response();
                    }
                }
                Ok(None) => {
                    tracing::warn!(
                        condition_function_id = %condition_function_id,
                        "Condition function returned no result"
                    );
                }
                Err(err) => {
                    let error_id = generate_error_id();
                    tracing::error!(
                        condition_function_id = %condition_function_id,
                        error = ?err,
                        error_id = %error_id,
                        "Error invoking condition function"
                    );
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "internal server error", "error_id": error_id})),
                    )
                        .into_response();
                }
            }
        }

        let func_result = engine
            .invoke_function(&function_id, api_request_value)
            .await;

            return match func_result {
                Ok(result) => {
                    let result = result.unwrap_or(json!({}));
                    let status_code = result
                        .get("status_code")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(200) as u16;

                    // Record response status code
                    tracing::Span::current().record("http.response.status_code", status_code);

                    let api_response = APIresponse::from_function_return(result);

                    // Set span status based on HTTP status code (2xx = OK, otherwise ERROR)
                    if (200..300).contains(&status_code) {
                        tracing::Span::current().record("otel.status_code", "OK");
                    } else {
                        tracing::Span::current().record("otel.status_code", "ERROR");
                        // Log error metadata only, not the full response body
                        let response_body_len = serde_json::to_string(&api_response.body)
                            .map(|s| s.len())
                            .unwrap_or(0);
                        tracing::error!(
                            exception.type = "HttpError",
                            exception.message = %format!("HTTP {}", status_code),
                            response_body_len,
                            "Request failed"
                        );
                    }
                    (
                        StatusCode::from_u16(status_code).unwrap_or(StatusCode::OK),
                        Json(api_response.body),
                    )
                        .into_response()
                }
                Err(err) => {
                    let error_id = generate_error_id();

                    // Record 500 status code and error span status
                    tracing::Span::current().record("http.response.status_code", 500u16);
                    tracing::Span::current().record("otel.status_code", "ERROR");

                    // Log full error details internally with error_id for correlation
                    tracing::error!(
                        exception.type = "InternalServerError",
                        exception.message = %format!("{:?}", err),
                        error_id = %error_id,
                        "Internal server error"
                    );

                    // Return generic error to client with error_id for support reference
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "internal server error", "error_id": error_id})),
                    )
                        .into_response()
                }
            };
        }

        // Record 404 status code and error span status for not found
        tracing::Span::current().record("http.response.status_code", 404u16);
        tracing::Span::current().record("otel.status_code", "ERROR");

        // Record exception event for 404
        tracing::error!(
            exception.type = "NotFoundError",
            exception.message = %format!("Route not found: {} {}", method, actual_path),
            "Route not found"
        );

        (StatusCode::NOT_FOUND, "Not Found").into_response()
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn test_extract_path_params_simple() {
        let params = extract_path_params("/users/:id", "/users/123");
        assert_eq!(params.len(), 1);
        assert_eq!(params.get("id"), Some(&"123".to_string()));
    }

    #[test]
    fn test_extract_path_params_multiple() {
        let params = extract_path_params("/users/:id/posts/:post_id", "/users/123/posts/456");
        assert_eq!(params.len(), 2);
        assert_eq!(params.get("id"), Some(&"123".to_string()));
        assert_eq!(params.get("post_id"), Some(&"456".to_string()));
    }

    #[test]
    fn test_extract_path_params_nested() {
        let params = extract_path_params("/api/v1/users/:id/profile/:section", "/api/v1/users/123/profile/settings");
        assert_eq!(params.len(), 2);
        assert_eq!(params.get("id"), Some(&"123".to_string()));
        assert_eq!(params.get("section"), Some(&"settings".to_string()));
    }

    #[test]
    fn test_extract_path_params_no_params() {
        let params = extract_path_params("/users/list", "/users/list");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_extract_path_params_mismatched_segments() {
        let params = extract_path_params("/users/:id", "/users/123/posts");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_extract_path_params_trailing_slash() {
        let params = extract_path_params("/users/:id/", "/users/123/");
        assert_eq!(params.len(), 1);
        assert_eq!(params.get("id"), Some(&"123".to_string()));
    }

    #[test]
    fn test_extract_path_params_empty_paths() {
        let params = extract_path_params("", "");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_extract_path_params_root() {
        let params = extract_path_params("/", "/");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_sanitize_headers_for_logging_sensitive() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer token123"));
        headers.insert("cookie", HeaderValue::from_static("session=abc123"));
        headers.insert("x-api-key", HeaderValue::from_static("secret-key"));

        let sanitized = sanitize_headers_for_logging(&headers);
        assert_eq!(sanitized.get("authorization"), Some(&"[REDACTED]".to_string()));
        assert_eq!(sanitized.get("cookie"), Some(&"[REDACTED]".to_string()));
        assert_eq!(sanitized.get("x-api-key"), Some(&"[REDACTED]".to_string()));
    }

    #[test]
    fn test_sanitize_headers_for_logging_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", HeaderValue::from_static("Bearer token123"));
        headers.insert("COOKIE", HeaderValue::from_static("session=abc123"));
        headers.insert("X-Api-Key", HeaderValue::from_static("secret-key"));

        let sanitized = sanitize_headers_for_logging(&headers);
        assert_eq!(sanitized.get("authorization"), Some(&"[REDACTED]".to_string()));
        assert_eq!(sanitized.get("cookie"), Some(&"[REDACTED]".to_string()));
        assert_eq!(sanitized.get("x-api-key"), Some(&"[REDACTED]".to_string()));
    }

    #[test]
    fn test_sanitize_headers_for_logging_non_sensitive() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("user-agent", HeaderValue::from_static("Mozilla/5.0"));
        headers.insert("accept", HeaderValue::from_static("text/html"));

        let sanitized = sanitize_headers_for_logging(&headers);
        assert_eq!(sanitized.get("content-type"), Some(&"application/json".to_string()));
        assert_eq!(sanitized.get("user-agent"), Some(&"Mozilla/5.0".to_string()));
        assert_eq!(sanitized.get("accept"), Some(&"text/html".to_string()));
    }

    #[test]
    fn test_sanitize_headers_for_logging_mixed() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer token"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("x-csrf-token", HeaderValue::from_static("csrf123"));

        let sanitized = sanitize_headers_for_logging(&headers);
        assert_eq!(sanitized.get("authorization"), Some(&"[REDACTED]".to_string()));
        assert_eq!(sanitized.get("content-type"), Some(&"application/json".to_string()));
        assert_eq!(sanitized.get("x-csrf-token"), Some(&"[REDACTED]".to_string()));
    }

    #[test]
    fn test_sanitize_headers_for_logging_non_utf8() {
        let mut headers = HeaderMap::new();
        let non_utf8 = HeaderValue::from_bytes(b"\xFF\xFE").unwrap();
        headers.insert("custom-header", non_utf8);

        let sanitized = sanitize_headers_for_logging(&headers);
        assert_eq!(sanitized.get("custom-header"), Some(&"[non-utf8]".to_string()));
    }

    #[test]
    fn test_sanitize_query_params_for_logging() {
        let mut params = HashMap::new();
        params.insert("key1".to_string(), "value1".to_string());
        params.insert("key2".to_string(), "value2".to_string());
        params.insert("key3".to_string(), "value3".to_string());

        let sanitized = sanitize_query_params_for_logging(&params);
        assert_eq!(sanitized.len(), 3);
        assert!(sanitized.contains(&"key1".to_string()));
        assert!(sanitized.contains(&"key2".to_string()));
        assert!(sanitized.contains(&"key3".to_string()));
    }

    #[test]
    fn test_sanitize_query_params_for_logging_empty() {
        let params = HashMap::new();
        let sanitized = sanitize_query_params_for_logging(&params);
        assert_eq!(sanitized.len(), 0);
    }

    #[test]
    fn test_generate_error_id() {
        let id1 = generate_error_id();
        assert!(!id1.is_empty());
        assert!(id1.len() <= 12);

        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = generate_error_id();
        assert!(!id2.is_empty());
    }
}
