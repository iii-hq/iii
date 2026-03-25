// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod channels;
pub mod rbac_config;
pub mod rbac_session;
pub mod ws_handler;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::{ConnectInfo, State, ws::WebSocketUpgrade},
    http::{HeaderMap, Uri},
    response::IntoResponse,
    routing::get,
};
use colored::Colorize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::TcpListener;

use crate::{
    engine::Engine,
    modules::{module::Module, worker::ws_handler::channel_ws_upgrade},
    protocol::StreamChannelRef,
};

pub const DEFAULT_PORT: u16 = 49134;

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct CreateChannelInput {
    #[serde(default)]
    pub buffer_size: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct CreateChannelOutput {
    pub writer: StreamChannelRef,
    pub reader: StreamChannelRef,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default)]
    pub middleware_function_id: Option<String>,
    pub rbac: Option<rbac_config::RbacConfig>,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

#[derive(Clone)]
pub struct WorkerModule {
    engine: Arc<Engine>,
    config: WorkerConfig,
}

#[async_trait::async_trait]
impl Module for WorkerModule {
    fn name(&self) -> &'static str {
        "WorkerModule"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        let config: WorkerConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        Ok(Box::new(WorkerModule { engine, config }))
    }

    async fn start_background_tasks(
        &self,
        shutdown_rx: tokio::sync::watch::Receiver<bool>,
        shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        let config = Arc::new(self.config.clone());
        let state = AppState {
            engine: self.engine.clone(),
            config: config.clone(),
            shutdown_rx: shutdown_rx.clone(),
        };

        tokio::spawn(async move {
            // Setup router
            let app = Router::new()
                .route("/", get(ws_handler))
                .route("/ws/channels/{channel_id}", get(channel_ws_upgrade))
                .with_state(state);

            // Bind and serve
            let addr = format!("{}:{}", config.host, config.port);
            let listener = TcpListener::bind(&addr).await.unwrap();
            tracing::info!("Engine listening on address: {}", addr.purple());

            let shutdown = async move {
                let _ = shutdown_signal().await;
                let _ = shutdown_tx.send(true);
            };

            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(shutdown)
            .await
            .unwrap();
        });

        Ok(())
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing WorkerModule");
        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Engine>,
    pub config: Arc<WorkerConfig>,
    pub(crate) shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

async fn shutdown_signal() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigint = signal(SignalKind::interrupt())?;

        tokio::select! {
            _ = sigterm.recv() => {},
            _ = sigint.recv() => {},
            _ = tokio::signal::ctrl_c() => {},
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
    }

    Ok(())
}

async fn ws_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
    uri: Uri,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let engine = state.engine.clone();
    let config = state.config.clone();

    ws.on_upgrade(move |socket| async move {
        if let Err(err) = engine
            .handle_worker(socket, addr, uri, headers, config, state.shutdown_rx)
            .await
        {
            tracing::error!(addr = %addr, error = ?err, "worker error");
        }
    })
}

crate::register_module!("modules::worker::WorkerModule", WorkerModule, mandatory);
