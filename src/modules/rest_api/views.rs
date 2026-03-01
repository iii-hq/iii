// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, convert::Infallible, sync::Arc};

use axum::{
    Json,
    body::Body,
    extract::{Extension, Query},
    http::{StatusCode, Uri, header::HeaderMap},
    response::IntoResponse,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tracing::Instrument;

use crate::{
    channels::ChannelItem,
    modules::rest_api::types::{HttpRequest, HttpResponse},
};

fn generate_error_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", timestamp & 0xFFFFFFFFFFFF)
}

use super::{RestApiCoreModule, types::TriggerMetadata};
use crate::engine::{Engine, EngineTrait};

fn apply_control_message(
    msg: &str,
    status_code: &mut u16,
    response_headers: &mut HashMap<String, String>,
) {
    if let Ok(ctrl) = serde_json::from_str::<Value>(msg) {
        match ctrl.get("type").and_then(|t| t.as_str()) {
            Some("set_status") => {
                if let Some(code) = ctrl.get("status_code").and_then(|v| v.as_u64()) {
                    if (100..=599).contains(&code) {
                        *status_code = code as u16;
                        tracing::debug!(
                            status_code = *status_code,
                            "Response channel: received set_status"
                        );
                    } else {
                        tracing::warn!(code, "Response channel: ignoring out-of-range status code");
                    }
                }
            }
            Some("set_headers") => {
                if let Some(hdrs) = ctrl.get("headers").and_then(|v| v.as_object()) {
                    for (k, v) in hdrs {
                        if let Some(v_str) = v.as_str() {
                            response_headers.insert(k.clone(), v_str.to_string());
                        }
                    }
                }
            }
            _ => {
                tracing::debug!(msg = %msg, "Response channel: unknown text message");
            }
        }
    }
}

fn extract_path_params(registered_path: &str, actual_path: &str) -> HashMap<String, String> {
    let registered_segments: Vec<&str> = registered_path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let actual_segments: Vec<&str> = actual_path.split('/').filter(|s| !s.is_empty()).collect();

    let mut params = HashMap::new();

    if registered_segments.len() != actual_segments.len() {
        return params;
    }

    for (i, registered_seg) in registered_segments.iter().enumerate() {
        if registered_seg.starts_with(':') {
            let param_name = registered_seg
                .strip_prefix(':')
                .unwrap_or(registered_seg)
                .to_string();

            if let Some(actual_value) = actual_segments.get(i) {
                params.insert(param_name, actual_value.to_string());
            }
        }
    }

    params
}

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

fn sanitize_query_params_for_logging(params: &HashMap<String, String>) -> Vec<String> {
    params.keys().cloned().collect()
}

fn serialize_headers(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect()
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
    body: Body,
) -> impl IntoResponse {
    let actual_path = uri.path().to_string();
    let query_string = uri.query().unwrap_or("").to_string();
    let request_body_size = headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);

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

    let url_scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http")
        .to_string();

    let url_full = if query_string.is_empty() {
        format!("{}://{}{}", url_scheme, host, actual_path)
    } else {
        format!("{}://{}{}?{}", url_scheme, host, actual_path, query_string)
    };

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
        "iii.function.kind" = tracing::field::Empty,
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
            let function_kind = if function_id.starts_with("engine::") {
                "internal"
            } else {
                "user"
            };
            tracing::Span::current().record("iii.function.kind", function_kind);

            let channel_mgr = &engine.channel_manager;

            // Create request body channel (worker reads from it)
            let (req_writer_ref, req_reader_ref) = channel_mgr.create_channel(64, None);
            // Create response body channel (worker writes to it)
            let (res_writer_ref, res_reader_ref) = channel_mgr.create_channel(64, None);

            let req_ch_id = req_writer_ref.channel_id.clone();
            let req_ch_key = req_writer_ref.access_key.clone();
            let res_ch_id = res_reader_ref.channel_id.clone();
            let res_ch_key = res_reader_ref.access_key.clone();

            // Stream the collected body bytes into the channel
            let req_tx = channel_mgr
                .take_sender(&req_ch_id, &req_ch_key)
                .await;

            let parsed_body: Value = if content_type.contains("application/json") {
                // Collect the raw request body so we can both parse it for
                // backward-compatible `body` field AND stream it via the channel.
                let body_bytes = {
                    let mut buf = Vec::new();
                    let mut body = body;
                    while let Some(frame_result) = body.frame().await {
                        match frame_result {
                            Ok(frame) => {
                                if let Ok(data) = frame.into_data() {
                                    buf.extend_from_slice(&data);
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    axum::body::Bytes::from(buf)
                };
                serde_json::from_slice(&body_bytes).unwrap_or(Value::Null)
            } else if let Some(req_tx) = req_tx {
                let mut body = body;

                tokio::spawn(async move {
                    while let Some(frame_result) = body.frame().await {
                        match frame_result {
                            Ok(frame) => {
                                if let Ok(data) = frame.into_data() {
                                    let _ = req_tx.send(ChannelItem::Binary(data)).await;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });

                Value::Null
            } else {
                Value::Null
            };

            let api_request_value = HttpRequest {
                query_params,
                path_params: path_parameters,
                headers: serialize_headers(&headers),
                path: registered_path.clone(),
                method: method.as_str().to_string(),
                body: parsed_body,
                trigger: Some(TriggerMetadata {
                    trigger_type: "http".to_string(),
                    path: Some(registered_path.clone()),
                    method: Some(method.as_str().to_string()),
                }),
                request_body: req_reader_ref,
                response: res_writer_ref,
            };

            // Take the response receiver BEFORE engine.call() so the channel
            // can drain concurrently -- the worker sends text control messages
            // (set_status, set_headers) followed by binary body data.
            let res_rx = channel_mgr
                .take_receiver(&res_ch_id, &res_ch_key)
                .await;

            // Check condition function (with metadata only, no body or stream refs)
            if let Some(condition_function_id) = condition_function_id.as_ref() {
                tracing::debug!(
                    condition_function_id = %condition_function_id,
                    "Checking trigger conditions"
                );

                let mut condition_input = serde_json::to_value(&api_request_value).unwrap_or(Value::Null);
                if let Some(obj) = condition_input.as_object_mut() {
                    obj.remove("request_body");
                    obj.remove("response");
                }

                match engine
                    .call(condition_function_id, condition_input)
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
                            channel_mgr.remove_channel(&req_ch_id);
                            channel_mgr.remove_channel(&res_ch_id);
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
                        channel_mgr.remove_channel(&req_ch_id);
                        channel_mgr.remove_channel(&res_ch_id);
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({"error": "internal server error", "error_id": error_id})),
                        )
                            .into_response();
                    }
                }
            }

            // Spawn engine.call() so the handler runs concurrently while we
            // read control messages + body from the response channel.
            let engine_clone = engine.clone();
            let mut call_handle = tokio::spawn(async move {
                engine_clone.call(&function_id, api_request_value).await
            });

            crate::modules::telemetry::collector::track_api_request();

            // Read from the response channel: text messages carry control
            // commands (set_status / set_headers), binary frames carry body.
            let mut status_code: u16 = 200;
            let mut response_headers: HashMap<String, String> = HashMap::new();
            let mut first_binary_chunk: Option<axum::body::Bytes> = None;

            if let Some(mut rx) = res_rx {
                // Phase 1: Race between response channel items and engine.call().
                // We break out as soon as we get a binary chunk, the channel
                // closes, OR call_handle finishes (whichever comes first).
                'select_loop: loop {
                    tokio::select! {
                        biased;
                        item = rx.recv() => {
                            match item {
                                Some(ChannelItem::Text(msg)) => {
                                    apply_control_message(&msg, &mut status_code, &mut response_headers);
                                }
                                Some(ChannelItem::Binary(data)) => {
                                    first_binary_chunk = Some(data);
                                    break 'select_loop;
                                }
                                None => break 'select_loop,
                            }
                        }
                        result = &mut call_handle => {
                            match result {
                                Ok(Ok(Some(result))) => {
                                    let http_response = HttpResponse::from_function_return(result);
                                    let status_code = http_response.status_code;

                                    tracing::Span::current().record("http.response.status_code", status_code);
                                    let otel = if (200..300).contains(&status_code) { "OK" } else { "ERROR" };
                                    tracing::Span::current().record("otel.status_code", otel);

                                    channel_mgr.remove_channel(&res_ch_id);
                                    channel_mgr.remove_channel(&req_ch_id);
                                    return (StatusCode::from_u16(status_code).unwrap_or(StatusCode::OK), Json(http_response.body)).into_response();
                                }
                                Ok(Ok(None)) => {
                                    let response_stream_claimed = if let Some(channel) =
                                        channel_mgr.get_channel(&res_ch_id, &res_ch_key)
                                    {
                                        channel.tx.lock().await.is_none()
                                    } else {
                                        false
                                    };

                                    if !response_stream_claimed {
                                        tracing::Span::current()
                                            .record("http.response.status_code", 200u16);
                                        tracing::Span::current().record("otel.status_code", "OK");
                                        channel_mgr.remove_channel(&res_ch_id);
                                        channel_mgr.remove_channel(&req_ch_id);
                                        return (StatusCode::OK, Json(json!({}))).into_response();
                                    }

                                    // Invocation finished after handing off the response
                                    // stream to a worker. Drain remaining channel items.
                                    break 'select_loop;
                                }
                                Ok(Err(err)) => {
                                    let error_id = generate_error_id();
                                    channel_mgr.remove_channel(&res_ch_id);
                                    channel_mgr.remove_channel(&req_ch_id);
                                    tracing::Span::current().record("http.response.status_code", 500u16);
                                    tracing::Span::current().record("otel.status_code", "ERROR");
                                    tracing::error!(
                                        exception.type = "InternalServerError",
                                        exception.message = %format!("{:?}", err),
                                        error_id = %error_id,
                                        "Internal server error"
                                    );
                                    return (
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        Json(json!({"error": "internal server error", "error_id": error_id})),
                                    ).into_response();
                                }
                                Err(join_err) => {
                                    let error_id = generate_error_id();
                                    channel_mgr.remove_channel(&res_ch_id);
                                    channel_mgr.remove_channel(&req_ch_id);
                                    tracing::Span::current().record("http.response.status_code", 500u16);
                                    tracing::Span::current().record("otel.status_code", "ERROR");
                                    tracing::error!(
                                        exception.type = "InternalServerError",
                                        exception.message = %format!("{:?}", join_err),
                                        error_id = %error_id,
                                        "Internal server error (task panic)"
                                    );
                                    return (
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        Json(json!({"error": "internal server error", "error_id": error_id})),
                                    ).into_response();
                                }
                            }
                        }
                    }
                }

                // Phase 2: If call_handle completed before the binary body
                // arrived, drain remaining channel items without select.
                while first_binary_chunk.is_none() {
                    match rx.recv().await {
                        Some(ChannelItem::Text(msg)) => {
                            apply_control_message(&msg, &mut status_code, &mut response_headers);
                        }
                        Some(ChannelItem::Binary(data)) => {
                            first_binary_chunk = Some(data);
                        }
                        None => break,
                    }
                }

                // Build streaming response from accumulated control messages + binary body
                tracing::Span::current().record("http.response.status_code", status_code);
                let otel = if (200..300).contains(&status_code) { "OK" } else { "ERROR" };
                tracing::Span::current().record("otel.status_code", otel);

                channel_mgr.remove_channel(&res_ch_id);
                channel_mgr.remove_channel(&req_ch_id);

                let stream = futures::stream::unfold(
                    (first_binary_chunk, rx),
                    |(pending, mut rx)| async move {
                        if let Some(chunk) = pending {
                            return Some((Ok::<_, Infallible>(chunk), (None, rx)));
                        }
                        loop {
                            match rx.recv().await {
                                Some(ChannelItem::Binary(data)) => {
                                    return Some((Ok(data), (None, rx)));
                                }
                                Some(ChannelItem::Text(_)) => continue,
                                None => return None,
                            }
                        }
                    },
                );

                let mut response_builder = axum::http::Response::builder()
                    .status(StatusCode::from_u16(status_code).unwrap_or(StatusCode::OK));

                for (k, v) in &response_headers {
                    match (
                        axum::http::header::HeaderName::try_from(k.as_str()),
                        axum::http::header::HeaderValue::try_from(v.as_str()),
                    ) {
                        (Ok(name), Ok(value)) => {
                            response_builder = response_builder.header(name, value);
                        }
                        _ => {
                            tracing::warn!(header_name = %k, "Skipping invalid response header");
                        }
                    }
                }

                return match response_builder.body(Body::from_stream(stream)) {
                    Ok(resp) => resp.into_response(),
                    Err(err) => {
                        tracing::error!(error = ?err, "Failed to build streaming response");
                        (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
                    }
                };
            }

            // No response receiver available -- wait for engine.call() result
            let func_result = call_handle.await;
            channel_mgr.remove_channel(&res_ch_id);
            channel_mgr.remove_channel(&req_ch_id);

            return match func_result {
                Ok(Ok(result)) => {
                    let result = result.unwrap_or(json!({}));
                    let sc = result.get("status_code").and_then(|v| v.as_u64()).unwrap_or(200) as u16;
                    tracing::Span::current().record("http.response.status_code", sc);
                    let otel = if (200..300).contains(&sc) { "OK" } else { "ERROR" };
                    tracing::Span::current().record("otel.status_code", otel);
                    let body = result.get("body").cloned().unwrap_or(json!({}));
                    (StatusCode::from_u16(sc).unwrap_or(StatusCode::OK), Json(body)).into_response()
                }
                Ok(Err(err)) => {
                    let error_id = generate_error_id();
                    tracing::Span::current().record("http.response.status_code", 500u16);
                    tracing::Span::current().record("otel.status_code", "ERROR");
                    tracing::error!(
                        exception.type = "InternalServerError",
                        exception.message = %format!("{:?}", err),
                        error_id = %error_id,
                        "Internal server error"
                    );
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "internal server error", "error_id": error_id})),
                    ).into_response()
                }
                Err(join_err) => {
                    let error_id = generate_error_id();
                    tracing::Span::current().record("http.response.status_code", 500u16);
                    tracing::Span::current().record("otel.status_code", "ERROR");
                    tracing::error!(
                        exception.type = "InternalServerError",
                        exception.message = %format!("{:?}", join_err),
                        error_id = %error_id,
                        "Internal server error (task panic)"
                    );
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "internal server error", "error_id": error_id})),
                    ).into_response()
                }
            };
        }

        tracing::Span::current().record("http.response.status_code", 404u16);
        tracing::Span::current().record("otel.status_code", "ERROR");

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
    use axum::http::{HeaderName, HeaderValue, header::HeaderMap};

    // ── generate_error_id ──────────────────────────────────────────────

    #[test]
    fn test_generate_error_id_is_unique() {
        let id1 = generate_error_id();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = generate_error_id();
        assert_ne!(id1, id2, "Two consecutive error IDs should differ");
    }

    #[test]
    fn test_generate_error_id_format() {
        let id = generate_error_id();
        // The id is a lower-hex encoding of (nanos & 0xFFFF_FFFF_FFFF).
        // 0xFFFF_FFFF_FFFF in hex is 12 chars, so length should be <= 12
        // and every character must be a valid hex digit.
        assert!(!id.is_empty(), "Error ID should not be empty");
        assert!(
            id.len() <= 12,
            "Error ID should be at most 12 hex characters, got {} ('{}')",
            id.len(),
            id
        );
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "Error ID should contain only hex digits, got '{}'",
            id
        );
    }

    // ── extract_path_params ────────────────────────────────────────────

    #[test]
    fn test_extract_path_params_basic() {
        let params = extract_path_params("/users/:id", "/users/123");
        assert_eq!(params.len(), 1);
        assert_eq!(params.get("id").unwrap(), "123");
    }

    #[test]
    fn test_extract_path_params_multiple() {
        let params = extract_path_params("/users/:userId/posts/:postId", "/users/42/posts/99");
        assert_eq!(params.len(), 2);
        assert_eq!(params.get("userId").unwrap(), "42");
        assert_eq!(params.get("postId").unwrap(), "99");
    }

    #[test]
    fn test_extract_path_params_no_params() {
        let params = extract_path_params("/health/check", "/health/check");
        assert!(
            params.is_empty(),
            "Route without params should return empty map"
        );
    }

    #[test]
    fn test_extract_path_params_mismatch() {
        // Different segment counts -> should return empty map
        let params = extract_path_params("/users/:id", "/users/123/extra");
        assert!(
            params.is_empty(),
            "Mismatched segment count should return empty map"
        );
    }

    // ── apply_control_message ──────────────────────────────────────────

    #[test]
    fn test_apply_control_message_set_status() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "type": "set_status",
            "status_code": 404
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert_eq!(status_code, 404);
    }

    #[test]
    fn test_apply_control_message_set_headers() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "type": "set_headers",
            "headers": {
                "content-type": "application/json",
                "x-request-id": "abc-123"
            }
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert_eq!(status_code, 200, "Status code should remain unchanged");
        assert_eq!(headers.get("content-type").unwrap(), "application/json");
        assert_eq!(headers.get("x-request-id").unwrap(), "abc-123");
    }

    // ── sanitize_headers_for_logging ───────────────────────────────────

    #[test]
    fn test_sanitize_headers_redacts_auth() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer secret-token"),
        );

        let sanitized = sanitize_headers_for_logging(&headers);
        assert_eq!(sanitized.get("authorization").unwrap(), "[REDACTED]");
    }

    #[test]
    fn test_sanitize_headers_redacts_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("cookie"),
            HeaderValue::from_static("session=abc123"),
        );

        let sanitized = sanitize_headers_for_logging(&headers);
        assert_eq!(sanitized.get("cookie").unwrap(), "[REDACTED]");
    }

    #[test]
    fn test_sanitize_headers_preserves_safe() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            HeaderName::from_static("accept"),
            HeaderValue::from_static("text/html"),
        );

        let sanitized = sanitize_headers_for_logging(&headers);
        assert_eq!(sanitized.get("content-type").unwrap(), "application/json");
        assert_eq!(sanitized.get("accept").unwrap(), "text/html");
    }

    #[test]
    fn test_sanitize_headers_case_insensitive() {
        // HTTP header names are case-insensitive. The `http` crate normalises
        // them to lowercase, so inserting "Authorization" is stored as
        // "authorization". This test verifies the SENSITIVE_HEADERS list
        // (which is all-lowercase) still catches them.
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_static("my-secret-key"),
        );
        headers.insert(
            HeaderName::from_static("proxy-authorization"),
            HeaderValue::from_static("Basic creds"),
        );

        let sanitized = sanitize_headers_for_logging(&headers);
        assert_eq!(sanitized.get("x-api-key").unwrap(), "[REDACTED]");
        assert_eq!(sanitized.get("proxy-authorization").unwrap(), "[REDACTED]");
    }

    // ── serialize_headers ──────────────────────────────────────────────

    #[test]
    fn test_serialize_headers_basic() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            HeaderName::from_static("x-request-id"),
            HeaderValue::from_static("req-001"),
        );

        let map = serialize_headers(&headers);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("content-type").unwrap(), "application/json");
        assert_eq!(map.get("x-request-id").unwrap(), "req-001");
    }

    #[test]
    fn test_serialize_headers_empty() {
        let headers = HeaderMap::new();
        let map = serialize_headers(&headers);
        assert!(
            map.is_empty(),
            "Empty HeaderMap should produce empty HashMap"
        );
    }

    // ── Additional apply_control_message tests ──────────────────────────

    #[test]
    fn test_apply_control_message_set_status_boundary_100() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "type": "set_status",
            "status_code": 100
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert_eq!(status_code, 100);
    }

    #[test]
    fn test_apply_control_message_set_status_boundary_599() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "type": "set_status",
            "status_code": 599
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert_eq!(status_code, 599);
    }

    #[test]
    fn test_apply_control_message_set_status_out_of_range_below() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "type": "set_status",
            "status_code": 99
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert_eq!(
            status_code, 200,
            "Status code should remain unchanged for out-of-range"
        );
    }

    #[test]
    fn test_apply_control_message_set_status_out_of_range_above() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "type": "set_status",
            "status_code": 600
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert_eq!(
            status_code, 200,
            "Status code should remain unchanged for out-of-range"
        );
    }

    #[test]
    fn test_apply_control_message_set_status_missing_status_code() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "type": "set_status"
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert_eq!(
            status_code, 200,
            "Status code should remain unchanged when missing"
        );
    }

    #[test]
    fn test_apply_control_message_set_status_non_numeric() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "type": "set_status",
            "status_code": "abc"
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert_eq!(
            status_code, 200,
            "Status code should remain unchanged for non-numeric"
        );
    }

    #[test]
    fn test_apply_control_message_set_headers_overwrites_existing() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();
        headers.insert("content-type".to_string(), "text/plain".to_string());

        let msg = serde_json::json!({
            "type": "set_headers",
            "headers": {
                "content-type": "application/json"
            }
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert_eq!(headers.get("content-type").unwrap(), "application/json");
    }

    #[test]
    fn test_apply_control_message_set_headers_missing_headers_field() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "type": "set_headers"
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert!(
            headers.is_empty(),
            "Headers should remain empty when headers field is missing"
        );
    }

    #[test]
    fn test_apply_control_message_set_headers_non_string_values_skipped() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "type": "set_headers",
            "headers": {
                "valid": "value",
                "numeric": 123,
                "null-val": null
            }
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers.get("valid").unwrap(), "value");
    }

    #[test]
    fn test_apply_control_message_unknown_type() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "type": "unknown_type",
            "data": "something"
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert_eq!(
            status_code, 200,
            "Status code should not change for unknown type"
        );
        assert!(
            headers.is_empty(),
            "Headers should not change for unknown type"
        );
    }

    #[test]
    fn test_apply_control_message_missing_type() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "data": "no type field"
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert_eq!(status_code, 200);
        assert!(headers.is_empty());
    }

    #[test]
    fn test_apply_control_message_invalid_json() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        apply_control_message("not valid json {{{", &mut status_code, &mut headers);
        assert_eq!(
            status_code, 200,
            "Status code should not change for invalid JSON"
        );
        assert!(
            headers.is_empty(),
            "Headers should not change for invalid JSON"
        );
    }

    #[test]
    fn test_apply_control_message_empty_string() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        apply_control_message("", &mut status_code, &mut headers);
        assert_eq!(status_code, 200);
        assert!(headers.is_empty());
    }

    #[test]
    fn test_apply_control_message_set_headers_empty_object() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        let msg = serde_json::json!({
            "type": "set_headers",
            "headers": {}
        })
        .to_string();

        apply_control_message(&msg, &mut status_code, &mut headers);
        assert!(headers.is_empty());
    }

    #[test]
    fn test_apply_control_message_combined_sequence() {
        let mut status_code: u16 = 200;
        let mut headers: HashMap<String, String> = HashMap::new();

        // First set status
        let msg1 = serde_json::json!({
            "type": "set_status",
            "status_code": 201
        })
        .to_string();
        apply_control_message(&msg1, &mut status_code, &mut headers);

        // Then set headers
        let msg2 = serde_json::json!({
            "type": "set_headers",
            "headers": {"x-foo": "bar"}
        })
        .to_string();
        apply_control_message(&msg2, &mut status_code, &mut headers);

        assert_eq!(status_code, 201);
        assert_eq!(headers.get("x-foo").unwrap(), "bar");
    }

    // ── Additional extract_path_params tests ─────────────────────────────

    #[test]
    fn test_extract_path_params_empty_paths() {
        let params = extract_path_params("", "");
        assert!(params.is_empty());
    }

    #[test]
    fn test_extract_path_params_root_paths() {
        let params = extract_path_params("/", "/");
        assert!(params.is_empty());
    }

    #[test]
    fn test_extract_path_params_fewer_actual_segments() {
        let params = extract_path_params("/users/:userId/posts/:postId", "/users/42");
        assert!(
            params.is_empty(),
            "Should return empty when actual has fewer segments"
        );
    }

    #[test]
    fn test_extract_path_params_mixed_static_and_param() {
        let params =
            extract_path_params("/api/v1/:resource/:id/details", "/api/v1/users/42/details");
        assert_eq!(params.len(), 2);
        assert_eq!(params.get("resource").unwrap(), "users");
        assert_eq!(params.get("id").unwrap(), "42");
    }

    #[test]
    fn test_extract_path_params_trailing_slashes() {
        // Trailing slashes produce empty segments which are filtered out
        let params = extract_path_params("/users/:id/", "/users/42/");
        assert_eq!(params.len(), 1);
        assert_eq!(params.get("id").unwrap(), "42");
    }

    #[test]
    fn test_extract_path_params_special_characters_in_value() {
        let params = extract_path_params("/items/:id", "/items/hello%20world");
        assert_eq!(params.get("id").unwrap(), "hello%20world");
    }

    // ── Additional sanitize_headers_for_logging tests ────────────────────

    #[test]
    fn test_sanitize_headers_redacts_all_sensitive() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer x"),
        );
        headers.insert(
            HeaderName::from_static("cookie"),
            HeaderValue::from_static("s=1"),
        );
        headers.insert(
            HeaderName::from_static("set-cookie"),
            HeaderValue::from_static("s=2"),
        );
        headers.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_static("key"),
        );
        headers.insert(
            HeaderName::from_static("api-key"),
            HeaderValue::from_static("key2"),
        );
        headers.insert(
            HeaderName::from_static("x-auth-token"),
            HeaderValue::from_static("tok"),
        );
        headers.insert(
            HeaderName::from_static("x-access-token"),
            HeaderValue::from_static("tok2"),
        );
        headers.insert(
            HeaderName::from_static("x-secret"),
            HeaderValue::from_static("sec"),
        );
        headers.insert(
            HeaderName::from_static("x-csrf-token"),
            HeaderValue::from_static("csrf"),
        );
        headers.insert(
            HeaderName::from_static("proxy-authorization"),
            HeaderValue::from_static("basic"),
        );

        let sanitized = sanitize_headers_for_logging(&headers);
        for (_, v) in &sanitized {
            assert_eq!(v, "[REDACTED]", "All sensitive headers should be redacted");
        }
    }

    #[test]
    fn test_sanitize_headers_empty() {
        let headers = HeaderMap::new();
        let sanitized = sanitize_headers_for_logging(&headers);
        assert!(sanitized.is_empty());
    }

    // ── sanitize_query_params_for_logging ──────────────────────────────────

    #[test]
    fn test_sanitize_query_params_returns_keys() {
        let mut params = HashMap::new();
        params.insert("page".to_string(), "1".to_string());
        params.insert("filter".to_string(), "active".to_string());

        let keys = sanitize_query_params_for_logging(&params);
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"page".to_string()));
        assert!(keys.contains(&"filter".to_string()));
    }

    #[test]
    fn test_sanitize_query_params_empty() {
        let params = HashMap::new();
        let keys = sanitize_query_params_for_logging(&params);
        assert!(keys.is_empty());
    }

    #[test]
    fn test_sanitize_query_params_does_not_include_values() {
        let mut params = HashMap::new();
        params.insert("secret_key".to_string(), "super_secret_value".to_string());

        let keys = sanitize_query_params_for_logging(&params);
        assert!(keys.contains(&"secret_key".to_string()));
        assert!(!keys.contains(&"super_secret_value".to_string()));
    }

    // ── SENSITIVE_HEADERS constant ─────────────────────────────────────────

    #[test]
    fn test_sensitive_headers_list_is_complete() {
        assert_eq!(SENSITIVE_HEADERS.len(), 10);
        assert!(SENSITIVE_HEADERS.contains(&"authorization"));
        assert!(SENSITIVE_HEADERS.contains(&"cookie"));
        assert!(SENSITIVE_HEADERS.contains(&"set-cookie"));
        assert!(SENSITIVE_HEADERS.contains(&"x-api-key"));
        assert!(SENSITIVE_HEADERS.contains(&"api-key"));
        assert!(SENSITIVE_HEADERS.contains(&"x-auth-token"));
        assert!(SENSITIVE_HEADERS.contains(&"x-access-token"));
        assert!(SENSITIVE_HEADERS.contains(&"x-secret"));
        assert!(SENSITIVE_HEADERS.contains(&"x-csrf-token"));
        assert!(SENSITIVE_HEADERS.contains(&"proxy-authorization"));
    }

    #[test]
    fn test_sensitive_headers_all_lowercase() {
        for header in SENSITIVE_HEADERS {
            assert_eq!(
                *header,
                header.to_lowercase(),
                "Sensitive header '{}' should be all lowercase",
                header
            );
        }
    }

    // ── serialize_headers edge cases ─────────────────────────────────────

    #[test]
    fn test_serialize_headers_single_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("accept"),
            HeaderValue::from_static("*/*"),
        );
        let map = serialize_headers(&headers);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("accept").unwrap(), "*/*");
    }

    #[test]
    fn test_serialize_headers_many_headers() {
        let mut headers = HeaderMap::new();
        for i in 0..10 {
            let name_str = format!("x-header-{}", i);
            let val_str = format!("value-{}", i);
            headers.insert(
                HeaderName::try_from(name_str.as_str()).unwrap(),
                HeaderValue::try_from(val_str.as_str()).unwrap(),
            );
        }
        let map = serialize_headers(&headers);
        assert_eq!(map.len(), 10);
        for i in 0..10 {
            assert_eq!(
                map.get(&format!("x-header-{}", i)).unwrap(),
                &format!("value-{}", i)
            );
        }
    }

    // ── dynamic_handler integration tests ──────────────────────────────────

    use crate::channels::ChannelItem;
    use crate::engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest};
    use crate::function::FunctionResult;
    use crate::modules::observability::metrics::ensure_default_meter;
    use crate::modules::rest_api::api_core::{PathRouter, RestApiCoreModule};
    use crate::modules::rest_api::types::HttpRequest;
    use crate::protocol::ErrorBody;
    use axum::http::Method;

    fn make_test_engine() -> Arc<Engine> {
        ensure_default_meter();
        Arc::new(Engine::new())
    }

    fn make_test_api_handler(engine: Arc<Engine>) -> Arc<RestApiCoreModule> {
        use crate::modules::rest_api::config::RestApiConfig;
        let config = RestApiConfig::default();
        Arc::new(RestApiCoreModule::new_for_tests(engine, config))
    }

    async fn register_test_route(
        api_handler: &Arc<RestApiCoreModule>,
        path: &str,
        method: &str,
        function_id: &str,
    ) {
        api_handler
            .register_router(PathRouter::new(
                path.to_string(),
                method.to_string(),
                function_id.to_string(),
                None,
            ))
            .await
            .unwrap();
    }

    async fn register_test_route_with_condition(
        api_handler: &Arc<RestApiCoreModule>,
        path: &str,
        method: &str,
        function_id: &str,
        condition_function_id: &str,
    ) {
        api_handler
            .register_router(PathRouter::new(
                path.to_string(),
                method.to_string(),
                function_id.to_string(),
                Some(condition_function_id.to_string()),
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_dynamic_handler_route_not_found() {
        let engine = make_test_engine();
        let api_handler = make_test_api_handler(engine.clone());

        let headers = HeaderMap::new();
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::GET,
            "/nonexistent".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/nonexistent".to_string()),
            Query(query_params),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_dynamic_handler_returns_function_result() {
        let engine = make_test_engine();

        // Register a function that returns a JSON body
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::handler".to_string(),
                description: Some("Test handler".to_string()),
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {"message": "hello"}
                })))
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/test", "GET", "test::handler").await;

        let headers = HeaderMap::new();
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::GET,
            "/test".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/test".to_string()),
            Query(query_params),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dynamic_handler_custom_status_code() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::created".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Success(Some(json!({
                    "status_code": 201,
                    "body": {"id": 42}
                })))
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/items", "POST", "test::created").await;

        let headers = HeaderMap::new();
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::POST,
            "/items".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/items".to_string()),
            Query(query_params),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_dynamic_handler_function_returns_none() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::no_body".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move { FunctionResult::Success(None) }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/empty", "GET", "test::no_body").await;

        let headers = HeaderMap::new();
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::GET,
            "/empty".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/empty".to_string()),
            Query(query_params),
            Body::empty(),
        )
        .await
        .into_response();

        // When function returns None, result.unwrap_or(json!({})) gives {},
        // and status_code defaults to 200
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dynamic_handler_function_error() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::fail".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Failure(ErrorBody {
                    code: "TEST_ERR".to_string(),
                    message: "something went wrong".to_string(),
                })
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/fail", "GET", "test::fail").await;

        let headers = HeaderMap::new();
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::GET,
            "/fail".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/fail".to_string()),
            Query(query_params),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_dynamic_handler_with_query_params() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::search".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|input: Value| async move {
                // The handler receives the input which includes query_params
                let q = input
                    .get("query_params")
                    .and_then(|v| v.get("q"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {"query": q}
                })))
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/search", "GET", "test::search").await;

        let headers = HeaderMap::new();
        let mut query_params: HashMap<String, String> = HashMap::new();
        query_params.insert("q".to_string(), "hello".to_string());

        let response = dynamic_handler(
            Method::GET,
            "/search?q=hello".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/search".to_string()),
            Query(query_params),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dynamic_handler_with_path_params() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::get_user".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|input: Value| async move {
                let id = input
                    .get("path_params")
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {"user_id": id}
                })))
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/users/:id", "GET", "test::get_user").await;

        let headers = HeaderMap::new();
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::GET,
            "/users/42".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/users/:id".to_string()),
            Query(query_params),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dynamic_handler_with_content_length_header() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::echo".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {"ok": true}
                })))
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/echo", "POST", "test::echo").await;

        let mut headers = HeaderMap::new();
        headers.insert("content-length", HeaderValue::from_static("42"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("user-agent", HeaderValue::from_static("TestAgent/1.0"));
        headers.insert("host", HeaderValue::from_static("localhost:3000"));
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::POST,
            "/echo".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/echo".to_string()),
            Query(query_params),
            Body::from(r#"{"data":"test"}"#),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dynamic_handler_with_x_forwarded_proto() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::proto".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {}
                })))
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/proto", "GET", "test::proto").await;

        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        headers.insert("host", HeaderValue::from_static("example.com"));
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::GET,
            "/proto".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/proto".to_string()),
            Query(query_params),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dynamic_handler_with_query_string_in_uri() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::qs".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {}
                })))
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/qs", "GET", "test::qs").await;

        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("localhost"));
        let mut query_params: HashMap<String, String> = HashMap::new();
        query_params.insert("key".to_string(), "value".to_string());

        let response = dynamic_handler(
            Method::GET,
            "/qs?key=value".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/qs".to_string()),
            Query(query_params),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dynamic_handler_function_returns_500_status() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::error_status".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Success(Some(json!({
                    "status_code": 500,
                    "body": {"error": "server error"}
                })))
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/error_status", "GET", "test::error_status").await;

        let headers = HeaderMap::new();
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::GET,
            "/error_status".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/error_status".to_string()),
            Query(query_params),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_dynamic_handler_non_json_body() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::text_upload".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {"received": true}
                })))
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/upload", "POST", "test::text_upload").await;

        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("text/plain"));
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::POST,
            "/upload".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/upload".to_string()),
            Query(query_params),
            Body::from("raw text body"),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dynamic_handler_internal_function_kind() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "engine::internal_fn".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {"internal": true}
                })))
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/internal", "GET", "engine::internal_fn").await;

        let headers = HeaderMap::new();
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::GET,
            "/internal".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/internal".to_string()),
            Query(query_params),
            Body::empty(),
        )
        .await
        .into_response();

        // Internal function still works - the "kind" is set on the span
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dynamic_handler_json_body_parsing() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::json_body".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|input: Value| async move {
                // Check that the body was properly parsed from JSON
                let body = input.get("body").cloned().unwrap_or(Value::Null);
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {"parsed_body": body}
                })))
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/json", "POST", "test::json_body").await;

        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::POST,
            "/json".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/json".to_string()),
            Query(query_params),
            Body::from(r#"{"key":"value"}"#),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dynamic_handler_invalid_json_body() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::invalid_json".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|input: Value| async move {
                // Invalid JSON should result in body being null
                let body = input.get("body").cloned().unwrap_or(Value::Null);
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {"is_null": body.is_null()}
                })))
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/invalid_json", "POST", "test::invalid_json").await;

        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        let query_params: HashMap<String, String> = HashMap::new();

        let response = dynamic_handler(
            Method::POST,
            "/invalid_json".parse().unwrap(),
            headers,
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/invalid_json".to_string()),
            Query(query_params),
            Body::from("not valid json {{{"),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dynamic_handler_condition_false_returns_422() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::handler".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {"ok": true}
                })))
            }),
        );

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::condition".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(
                |_input: Value| async move { FunctionResult::Success(Some(json!(false))) },
            ),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route_with_condition(
            &api_handler,
            "/guarded",
            "GET",
            "test::handler",
            "test::condition",
        )
        .await;

        let response = dynamic_handler(
            Method::GET,
            "/guarded".parse().unwrap(),
            HeaderMap::new(),
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/guarded".to_string()),
            Query(HashMap::new()),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_dynamic_handler_condition_none_still_allows_handler_execution() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::allow".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {"ok": true}
                })))
            }),
        );

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::condition_none".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move { FunctionResult::Success(None) }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route_with_condition(
            &api_handler,
            "/condition-none",
            "GET",
            "test::allow",
            "test::condition_none",
        )
        .await;

        let response = dynamic_handler(
            Method::GET,
            "/condition-none".parse().unwrap(),
            HeaderMap::new(),
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/condition-none".to_string()),
            Query(HashMap::new()),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dynamic_handler_condition_error_returns_500() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::guarded".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Success(Some(json!({
                    "status_code": 200,
                    "body": {"ok": true}
                })))
            }),
        );

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::condition_error".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                FunctionResult::Failure(ErrorBody {
                    code: "COND".to_string(),
                    message: "nope".to_string(),
                })
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route_with_condition(
            &api_handler,
            "/condition-error",
            "GET",
            "test::guarded",
            "test::condition_error",
        )
        .await;

        let response = dynamic_handler(
            Method::GET,
            "/condition-error".parse().unwrap(),
            HeaderMap::new(),
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/condition-error".to_string()),
            Query(HashMap::new()),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_dynamic_handler_streaming_response_applies_controls_and_skips_invalid_headers() {
        let engine = make_test_engine();
        let engine_for_handler = engine.clone();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::streaming".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |input: Value| {
                let engine = engine_for_handler.clone();
                async move {
                    let request: HttpRequest = serde_json::from_value(input).unwrap();
                    let tx = engine
                        .channel_manager
                        .take_sender(&request.response.channel_id, &request.response.access_key)
                        .await
                        .expect("response sender");

                    tx.send(ChannelItem::Text(
                        json!({"type": "set_status", "status_code": 206}).to_string(),
                    ))
                    .await
                    .unwrap();
                    tx.send(ChannelItem::Text(
                        json!({
                            "type": "set_headers",
                            "headers": {
                                "content-type": "text/plain",
                                "bad\nheader": "ignored"
                            }
                        })
                        .to_string(),
                    ))
                    .await
                    .unwrap();
                    tx.send(ChannelItem::Binary(axum::body::Bytes::from_static(
                        b"hello",
                    )))
                    .await
                    .unwrap();

                    FunctionResult::Success(None)
                }
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/stream", "GET", "test::streaming").await;

        let response = dynamic_handler(
            Method::GET,
            "/stream".parse().unwrap(),
            HeaderMap::new(),
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/stream".to_string()),
            Query(HashMap::new()),
            Body::empty(),
        )
        .await
        .into_response();

        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(headers.get("content-type").unwrap(), "text/plain");
        assert_eq!(body, axum::body::Bytes::from_static(b"hello"));
    }

    #[tokio::test]
    async fn test_dynamic_handler_stream_claimed_without_binary_returns_empty_body() {
        let engine = make_test_engine();
        let engine_for_handler = engine.clone();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::stream_no_body".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |input: Value| {
                let engine = engine_for_handler.clone();
                async move {
                    let request: HttpRequest = serde_json::from_value(input).unwrap();
                    let tx = engine
                        .channel_manager
                        .take_sender(&request.response.channel_id, &request.response.access_key)
                        .await
                        .expect("response sender");

                    tx.send(ChannelItem::Text(
                        json!({"type": "set_status", "status_code": 202}).to_string(),
                    ))
                    .await
                    .unwrap();
                    tx.send(ChannelItem::Text(
                        json!({
                            "type": "set_headers",
                            "headers": {"x-test": "yes"}
                        })
                        .to_string(),
                    ))
                    .await
                    .unwrap();
                    drop(tx);

                    FunctionResult::Success(None)
                }
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/stream-empty", "GET", "test::stream_no_body").await;

        let response = dynamic_handler(
            Method::GET,
            "/stream-empty".parse().unwrap(),
            HeaderMap::new(),
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/stream-empty".to_string()),
            Query(HashMap::new()),
            Body::empty(),
        )
        .await
        .into_response();

        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(headers.get("x-test").unwrap(), "yes");
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn test_dynamic_handler_task_panic_returns_500() {
        let engine = make_test_engine();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::panic".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input: Value| async move {
                panic!("boom");
                #[allow(unreachable_code)]
                FunctionResult::Success(None)
            }),
        );

        let api_handler = make_test_api_handler(engine.clone());
        register_test_route(&api_handler, "/panic", "GET", "test::panic").await;

        let response = dynamic_handler(
            Method::GET,
            "/panic".parse().unwrap(),
            HeaderMap::new(),
            axum::extract::Extension(engine),
            axum::extract::Extension(api_handler),
            axum::extract::Extension("/panic".to_string()),
            Query(HashMap::new()),
            Body::empty(),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
