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
use serde_json::{json, Value};
use tracing::Instrument;

use crate::{channels::ChannelItem, modules::rest_api::types::{HttpRequest, HttpResponse}};

fn generate_error_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", timestamp & 0xFFFFFFFFFFFF)
}

use super::{
    RestApiCoreModule,
    types::TriggerMetadata,
};
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
                        tracing::debug!(status_code = *status_code, "Response channel: received set_status");
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
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                v.to_str().unwrap_or("").to_string(),
            )
        })
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
                                Ok(Ok(_)) => {
                                    // Invocation done, no direct body — break out of
                                    // select and drain remaining channel items below.
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
