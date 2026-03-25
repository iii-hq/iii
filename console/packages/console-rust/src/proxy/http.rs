// Copyright 2025 Motia LLC. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::debug;

/// Configuration for the engine proxy.
#[derive(Clone)]
pub struct ProxyConfig {
    pub engine_host: String,
    pub engine_port: u16,
    pub ws_port: u16,
}

/// Shared state for proxy handlers.
pub struct ProxyState {
    pub config: ProxyConfig,
    pub client: reqwest::Client,
}

/// HTTP reverse proxy handler.
///
/// Forwards requests from `/api/engine/{path}` to `http://engine_host:engine_port/{path}`.
/// Preserves method, headers, query string, and body.
pub async fn http_proxy_handler(
    State(state): State<Arc<ProxyState>>,
    req: Request,
) -> Result<Response, StatusCode> {
    let (parts, body) = req.into_parts();

    // Extract the path after /api/engine/
    let path = parts
        .uri
        .path()
        .strip_prefix("/api/engine/")
        .unwrap_or(parts.uri.path().strip_prefix("/api/engine").unwrap_or(""));

    // Build target URL
    let query = parts
        .uri
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();
    let target_url = format!(
        "http://{}:{}/{}{}",
        state.config.engine_host, state.config.engine_port, path, query
    );

    debug!("Proxying {} {} -> {}", parts.method, parts.uri, target_url);

    // Build the upstream request
    let mut upstream_req = state.client.request(parts.method.clone(), &target_url);

    // Forward headers, filtering out hop-by-hop headers
    let forwarded_headers = filter_hop_by_hop(&parts.headers);
    upstream_req = upstream_req.headers(forwarded_headers);

    // Forward body unconditionally (DELETE can carry a body too).
    // Empty bodies for GET/HEAD are harmless.
    let body_bytes = axum::body::to_bytes(body, 10 * 1024 * 1024) // 10MB limit
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if !body_bytes.is_empty() {
        upstream_req = upstream_req.body(body_bytes);
    }

    // Send the request
    let upstream_resp = upstream_req.send().await.map_err(|e| {
        tracing::error!("Proxy request failed: {}", e);
        StatusCode::BAD_GATEWAY
    })?;

    // Build the response
    let status = StatusCode::from_u16(upstream_resp.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    let mut response_headers = HeaderMap::new();
    for (key, value) in upstream_resp.headers() {
        if !is_hop_by_hop(key.as_str()) {
            response_headers.append(key.clone(), value.clone());
        }
    }

    let response_body = upstream_resp.bytes().await.map_err(|e| {
        tracing::error!("Failed to read proxy response body: {}", e);
        StatusCode::BAD_GATEWAY
    })?;

    let mut response = (status, Body::from(response_body)).into_response();
    *response.headers_mut() = response_headers;

    Ok(response)
}

/// Filter out hop-by-hop headers that should not be forwarded.
fn filter_hop_by_hop(headers: &HeaderMap) -> reqwest::header::HeaderMap {
    let mut filtered = reqwest::header::HeaderMap::new();
    for (key, value) in headers {
        if !is_hop_by_hop(key.as_str()) && key.as_str() != "host" {
            if let Ok(name) = reqwest::header::HeaderName::from_bytes(key.as_ref()) {
                if let Ok(val) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                    filtered.append(name, val);
                }
            }
        }
    }
    filtered
}

fn is_hop_by_hop(header: &str) -> bool {
    // axum/hyper normalize header names to lowercase, so direct comparison is safe
    matches!(
        header,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
    )
}
