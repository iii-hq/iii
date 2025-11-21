use std::{net::SocketAddr, sync::Arc};

mod api;
mod engine;
mod function;
mod invocation;
mod logging;
mod pending_invocations;
mod protocol;
mod routers;
mod services;
mod trigger;
mod workers;
mod modules {
    pub mod event;
    pub mod redis_adapter;
}

use axum::{
    Router,
    extract::{ConnectInfo, State, ws::WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use tokio::net::TcpListener;

use engine::Engine;

use crate::modules::redis_adapter::RedisAdapter;

async fn ws_handler(
    State(engine): State<Arc<Engine>>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let engine = engine.clone();

    ws.on_upgrade(move |socket| async move {
        if let Err(err) = engine.handle_worker(socket, addr).await {
            tracing::error!(addr = %addr, error = ?err, "worker error");
        }
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init_tracing();

    let engine: Arc<Engine> = Arc::new(Engine::new());
    let engine_clone = engine.clone();
    tokio::spawn(async move {
        engine_clone.notify_new_functions(5).await;
    });

    let app = Router::new().route("/", get(ws_handler));
    // Merge API routes
    let api_routes = api::api_endpoints();
    let app = app.merge(api_routes).with_state(engine.clone());

    let addr = "127.0.0.1:49134";
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(address = addr, "Engine listening");

    let redis_adapter =
        RedisAdapter::new("redis://localhost:6379".to_string(), engine.clone()).await?;
    let event_module =
        modules::event::EventCoreModule::new(Arc::new(redis_adapter), engine.clone());

    tokio::spawn(async move {
        event_module.initialize().await;
    });

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
