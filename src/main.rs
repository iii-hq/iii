use std::{net::SocketAddr, sync::Arc};

mod engine;
mod function;
mod invocation;
mod logging;

mod pending_invocations;
mod protocol;
mod services;
mod trigger;
mod workers;
mod modules {
    pub mod config;
    pub mod configurable;
    pub mod core_module;
    pub mod cron;
    pub mod event;
    pub mod loader;
    pub mod observability;
    pub mod rest_api;
    pub mod streams;
}

use axum::{
    Router,
    extract::{ConnectInfo, State, ws::WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use colored::Colorize;
use engine::Engine;
use tokio::net::TcpListener;

use crate::modules::{
    config::EngineConfig,
    core_module::CoreModule,
    cron::CronCoreModule,
    event::EventCoreModule,
    loader::{load_and_initialize, load_default_modules},
    observability::LoggerCoreModule,
    rest_api::RestApiCoreModule,
    streams::StreamCoreModule,
};

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

    // Load modules from config file or use defaults
    let config = EngineConfig::from_file_or_default("config.yaml")?;
    let _modules = config.load_modules(engine.clone()).await?;

    let app = Router::new()
        .route("/", get(ws_handler))
        .with_state(engine.clone());
    let addr = "127.0.0.1:49134";
    let listener = TcpListener::bind(addr).await?;

    let api_handler = RestApiCoreModule::new(engine.clone());
    let event_module = EventCoreModule::new(engine.clone());
    let logger_module = LoggerCoreModule::new(engine.clone());
    let cron_module = CronCoreModule::new(engine.clone());
    let streams_module = StreamCoreModule::new(engine.clone());

    let _ = event_module.initialize().await.unwrap();
    let _ = logger_module.initialize().await.unwrap();
    let _ = cron_module.initialize().await.unwrap();
    let _ = api_handler.initialize().await.unwrap();
    let _ = streams_module.initialize().await.unwrap();

    tracing::info!("Engine listening on address: {}", addr.purple());

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
