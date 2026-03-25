//! Integration tests for the console engine proxy.
//!
//! These tests spin up a mock "engine" server and verify that
//! the console proxy correctly forwards requests and responses.

use axum::{routing::get, Json, Router};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;

/// Helper: start a mock engine server that returns a known response.
async fn start_mock_engine() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route(
            "/_console/health",
            get(|| async { Json(json!({"status": "ok"})) }),
        )
        .route(
            "/_console/functions",
            get(|| async {
                Json(json!({
                    "status_code": 200,
                    "headers": [],
                    "body": {"functions": []}
                }))
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (addr, handle)
}

#[tokio::test]
async fn test_proxy_forwards_get_request() {
    let (engine_addr, _handle) = start_mock_engine().await;

    // Build proxy state pointing at our mock engine
    let proxy_state = Arc::new(iii_console::proxy::http::ProxyState {
        config: iii_console::proxy::http::ProxyConfig {
            engine_host: "127.0.0.1".to_string(),
            engine_port: engine_addr.port(),
            ws_port: 0,
        },
        client: reqwest::Client::new(),
    });

    // Build a minimal app with just the proxy route
    let app = Router::new()
        .route(
            "/api/engine/{*path}",
            axum::routing::any(iii_console::proxy::http::http_proxy_handler),
        )
        .with_state(proxy_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Make a request through the proxy
    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "http://127.0.0.1:{}/api/engine/_console/health",
            proxy_addr.port()
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn test_proxy_preserves_query_params() {
    let app = Router::new().route(
        "/_console/functions",
        get(|axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>| async move {
            Json(json!({"received_params": params}))
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let engine_addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let proxy_state = Arc::new(iii_console::proxy::http::ProxyState {
        config: iii_console::proxy::http::ProxyConfig {
            engine_host: "127.0.0.1".to_string(),
            engine_port: engine_addr.port(),
            ws_port: 0,
        },
        client: reqwest::Client::new(),
    });

    let proxy_app = Router::new()
        .route(
            "/api/engine/{*path}",
            axum::routing::any(iii_console::proxy::http::http_proxy_handler),
        )
        .with_state(proxy_state);

    let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(proxy_listener, proxy_app).await.unwrap() });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "http://127.0.0.1:{}/api/engine/_console/functions?type=http&active=true",
            proxy_addr.port()
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["received_params"]["type"], "http");
    assert_eq!(body["received_params"]["active"], "true");
}

#[tokio::test]
async fn test_proxy_returns_502_when_engine_unreachable() {
    let proxy_state = Arc::new(iii_console::proxy::http::ProxyState {
        config: iii_console::proxy::http::ProxyConfig {
            engine_host: "127.0.0.1".to_string(),
            engine_port: 1, // nothing should be listening here
            ws_port: 0,
        },
        client: reqwest::Client::new(),
    });

    let proxy_app = Router::new()
        .route(
            "/api/engine/{*path}",
            axum::routing::any(iii_console::proxy::http::http_proxy_handler),
        )
        .with_state(proxy_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, proxy_app).await.unwrap() });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "http://127.0.0.1:{}/api/engine/_console/health",
            proxy_addr.port()
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 502);
}
