#![allow(dead_code)]

use std::sync::OnceLock;
use std::time::Duration;

use iii_sdk::{III, InitOptions, register_worker};

static SHARED_RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static SHARED_III: OnceLock<III> = OnceLock::new();

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

pub fn shared_iii() -> &'static III {
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
