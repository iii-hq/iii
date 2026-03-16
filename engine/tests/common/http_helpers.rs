use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method as AxumMethod, StatusCode},
    routing::any,
};
use serde_json::Value;
use tokio::net::TcpListener;

use iii::invocation::http_invoker::{HttpInvoker, HttpInvokerConfig};
use iii::invocation::url_validator::UrlValidatorConfig;

#[derive(Clone, Default)]
pub struct RequestCapture {
    pub headers: Arc<Mutex<HashMap<String, String>>>,
    pub body: Arc<Mutex<Option<Value>>>,
    pub method: Arc<Mutex<Option<String>>>,
}

pub fn permissive_invoker() -> HttpInvoker {
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

pub fn strict_invoker() -> HttpInvoker {
    HttpInvoker::new(HttpInvokerConfig {
        url_validator: UrlValidatorConfig {
            require_https: true,
            block_private_ips: true,
            allowlist: vec!["*".to_string()],
        },
        ..HttpInvokerConfig::default()
    })
    .expect("create strict invoker")
}

pub fn store_request(
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

pub async fn spawn_server(capture: RequestCapture) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr = listener.local_addr().expect("resolve local addr");

    async fn success_handler(
        State(capture): State<RequestCapture>,
        method: AxumMethod,
        headers: HeaderMap,
        body: Bytes,
    ) -> (StatusCode, Json<Value>) {
        store_request(&capture, method, headers, body);
        (StatusCode::OK, Json(serde_json::json!({ "ok": true })))
    }

    async fn empty_handler() -> StatusCode {
        StatusCode::NO_CONTENT
    }

    let app = Router::new()
        .route("/success", any(success_handler))
        .route("/empty", any(empty_handler))
        .with_state(capture);

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test app");
    });

    format!("http://{addr}")
}
