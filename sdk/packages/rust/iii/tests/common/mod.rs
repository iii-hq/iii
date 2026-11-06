#![allow(dead_code)]

pub mod mock_engine;
pub mod proxy;

use std::sync::OnceLock;
use std::time::Duration;

use iii_sdk::{IIIClient, InitOptions, ObserverHandle, Source, register_worker};
use serde_json::{Value, json};

static SHARED_RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static SHARED_III: OnceLock<IIIClient> = OnceLock::new();

#[ctor::dtor]
fn shutdown() {
    let iii = shared_iii();
    iii.shutdown();
}

pub fn engine_ws_url() -> String {
    std::env::var("III_URL").unwrap_or_else(|_| "ws://localhost:49199".to_string())
}

pub fn engine_http_url() -> String {
    std::env::var("III_HTTP_URL").unwrap_or_else(|_| "http://localhost:3199".to_string())
}

pub async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::new()
}

pub fn shared_iii() -> &'static IIIClient {
    SHARED_III.get_or_init(|| {
        let rt = SHARED_RT.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to create shared runtime")
        });
        let _guard = rt.enter();

        register_worker(&engine_ws_url(), InitOptions::default())
    })
}

/// Routes `method path` of the HTTP worker to `function_id`: an observer of
/// `http::on-request` with the route as its args. The target receives the
/// request and returns the response.
pub async fn route(iii: &IIIClient, method: &str, path: &str, function_id: &str) -> ObserverHandle {
    route_with(iii, json!({ "method": method, "path": path }), function_id).await
}

/// [`route`] with the full args, `middleware_function_ids` included.
pub async fn route_with(iii: &IIIClient, args: Value, function_id: &str) -> ObserverHandle {
    let observer = iii.observe(Source::function("http::on-request").args(args), function_id);
    observer.ready().await.expect("route");
    observer
}
