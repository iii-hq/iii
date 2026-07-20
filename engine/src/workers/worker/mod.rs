// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod channels;
pub mod rbac_config;
pub mod rbac_session;
pub mod ws_handler;

use std::{
    io,
    net::SocketAddr,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use axum::{
    Router,
    extract::{ConnectInfo, State, ws::WebSocketUpgrade},
    http::{HeaderMap, Uri},
    response::IntoResponse,
    routing::get,
};
use colored::Colorize;
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{net::TcpListener, task::AbortHandle};
use tower::Service as _;

use crate::{
    engine::Engine,
    protocol::StreamChannelRef,
    workers::{traits::Worker, worker::ws_handler::channel_ws_upgrade},
};

pub const DEFAULT_PORT: u16 = 49134;

/// Default deadline for an accepted TCP connection to complete its HTTP
/// request (the WS upgrade). Generous for a handshake that normally takes
/// milliseconds; the point is bounding it at all (MOT-3967).
pub const DEFAULT_HANDSHAKE_TIMEOUT_MS: u64 = 10_000;

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct CreateChannelInput {
    /// Maximum number of pending messages buffered between writer and reader.
    /// Defaults to an engine-chosen value when omitted.
    #[serde(default)]
    pub buffer_size: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct CreateChannelOutput {
    pub writer: StreamChannelRef,
    pub reader: StreamChannelRef,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerManagerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default)]
    pub middleware_function_id: Option<String>,
    pub rbac: Option<rbac_config::RbacConfig>,
    /// Deadline in milliseconds for an accepted TCP connection to complete
    /// its HTTP request (the WS upgrade). Sockets that stall pre-upgrade
    /// are closed and logged instead of leaking the fd (MOT-3967).
    #[serde(default = "default_handshake_timeout_ms")]
    pub handshake_timeout_ms: u64,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_handshake_timeout_ms() -> u64 {
    DEFAULT_HANDSHAKE_TIMEOUT_MS
}

impl Default for WorkerManagerConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            middleware_function_id: None,
            rbac: None,
            handshake_timeout_ms: default_handshake_timeout_ms(),
        }
    }
}

#[derive(Clone)]
pub struct WorkerManager {
    engine: Arc<Engine>,
    config: WorkerManagerConfig,
    server_abort: Arc<StdMutex<Option<AbortHandle>>>,
}

#[async_trait::async_trait]
impl Worker for WorkerManager {
    fn name(&self) -> &'static str {
        "WorkerManager"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        let config: WorkerManagerConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        Ok(Box::new(WorkerManager {
            engine,
            config,
            server_abort: Arc::new(StdMutex::new(None)),
        }))
    }

    async fn start_background_tasks(
        &self,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
        shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        let config = Arc::new(self.config.clone());
        let state = AppState {
            engine: self.engine.clone(),
            config: config.clone(),
            shutdown_rx: shutdown_rx.clone(),
        };

        let addr = format!("{}:{}", config.host, config.port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|err| crate::workers::traits::bind_address_error(&addr, err))?;
        tracing::info!("Engine listening on address: {}", addr.purple());

        let app = Router::new()
            .route("/", get(ws_handler))
            .route("/otel", get(otel_ws_handler))
            .route("/ws/channels/{channel_id}", get(channel_ws_upgrade))
            .with_state(state);

        let handshake_timeout = Duration::from_millis(config.handshake_timeout_ms);
        let handle = tokio::spawn(async move {
            let shutdown = async move {
                tokio::select! {
                    _ = shutdown_signal() => {
                        let _ = shutdown_tx.send(true);
                    }
                    _ = async {
                        while shutdown_rx.changed().await.is_ok() {
                            if *shutdown_rx.borrow() {
                                break;
                            }
                        }
                    } => {}
                }
            };
            tokio::pin!(shutdown);

            // Fans shutdown out to every spawned connection task, mirroring
            // what axum::serve did internally: sent when the accept loop
            // exits, and if this task is aborted (`destroy()`), the dropped
            // sender wakes subscribers the same way.
            let (conn_shutdown_tx, _) = tokio::sync::watch::channel(false);

            // Serve connections manually instead of via `axum::serve`:
            // hyper's `header_read_timeout` is the handshake deadline that
            // reaps sockets stalling before the WS upgrade completes
            // (MOT-3967), and axum::serve exposes neither it nor the timer
            // it requires. Established WS sessions are unaffected — the
            // deadline only bounds reading request headers.
            let mut make_service = app.into_make_service_with_connect_info::<SocketAddr>();
            loop {
                let (stream, peer) = tokio::select! {
                    res = listener.accept() => match res {
                        Ok(conn) => conn,
                        Err(err) => {
                            // ECONNABORTED/RESET are normal client churn;
                            // anything else (e.g. EMFILE) gets a pause so
                            // the accept loop doesn't spin.
                            if !is_connection_error(&err) {
                                tracing::warn!(error = ?err, "worker listener accept error");
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                            continue;
                        }
                    },
                    _ = &mut shutdown => break,
                };
                let service = match make_service.call(peer).await {
                    Ok(service) => service,
                    Err(infallible) => match infallible {},
                };
                let mut conn_shutdown_rx = conn_shutdown_tx.subscribe();
                tokio::spawn(async move {
                    let conn = http1::Builder::new()
                        .timer(TokioTimer::new())
                        .header_read_timeout(handshake_timeout)
                        .serve_connection(TokioIo::new(stream), TowerToHyperService::new(service))
                        .with_upgrades();
                    tokio::pin!(conn);
                    // Completes on shutdown send or sender drop (abort).
                    let conn_shutdown = async move {
                        while conn_shutdown_rx.changed().await.is_ok() {
                            if *conn_shutdown_rx.borrow() {
                                break;
                            }
                        }
                    };
                    tokio::pin!(conn_shutdown);
                    let mut graceful_sent = false;
                    loop {
                        tokio::select! {
                            res = conn.as_mut() => {
                                if let Err(err) = res {
                                    if err.is_timeout() {
                                        tracing::warn!(
                                            peer = %peer,
                                            timeout_ms = handshake_timeout.as_millis() as u64,
                                            "closed worker socket: WS upgrade not completed within handshake deadline"
                                        );
                                    } else {
                                        tracing::debug!(peer = %peer, error = ?err, "worker connection error");
                                    }
                                }
                                break;
                            }
                            // Close idle keep-alive connections immediately
                            // and mark in-flight ones close-after-response,
                            // then keep polling the connection to completion.
                            _ = &mut conn_shutdown, if !graceful_sent => {
                                conn.as_mut().graceful_shutdown();
                                graceful_sent = true;
                            }
                        }
                    }
                });
            }

            let _ = conn_shutdown_tx.send(true);
        });

        *self
            .server_abort
            .lock()
            .expect("server_abort mutex poisoned") = Some(handle.abort_handle());

        Ok(())
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing WorkerManager");
        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        let abort = self
            .server_abort
            .lock()
            .expect("server_abort mutex poisoned")
            .take();
        if let Some(abort) = abort {
            abort.abort();
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Engine>,
    pub config: Arc<WorkerManagerConfig>,
    pub(crate) shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

/// Accept errors caused by the remote end (mirrors `axum::serve`'s internal
/// accept loop) — these are normal churn and don't warrant a warning or a
/// pause of the accept loop.
fn is_connection_error(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    )
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

/// WS upgrade handler for the OTEL-only endpoint (`/otel`).
///
/// Keeps telemetry traffic off the worker registry. See
/// `Engine::handle_otel` for the rationale.
async fn otel_ws_handler(
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
            .handle_otel(socket, addr, uri, headers, config, state.shutdown_rx)
            .await
        {
            tracing::error!(addr = %addr, error = ?err, "otel connection error");
        }
    })
}

crate::register_worker!(
    "iii-worker-manager",
    WorkerManager,
    description = "WebSocket listener that SDK workers connect to. Supports RBAC, middleware, registration hooks, and channels.",
    mandatory
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_config_default_values() {
        let config = WorkerManagerConfig::default();

        assert_eq!(config.port, DEFAULT_PORT);
        assert_eq!(config.host, "0.0.0.0");
        assert!(config.middleware_function_id.is_none());
        assert!(config.rbac.is_none());
        assert_eq!(config.handshake_timeout_ms, DEFAULT_HANDSHAKE_TIMEOUT_MS);
    }

    #[test]
    fn worker_config_deserialize_empty_json_uses_defaults() {
        let config: WorkerManagerConfig = serde_json::from_str("{}").unwrap();

        assert_eq!(config.port, DEFAULT_PORT);
        assert_eq!(config.host, "0.0.0.0");
        assert!(config.middleware_function_id.is_none());
        assert!(config.rbac.is_none());
        assert_eq!(config.handshake_timeout_ms, DEFAULT_HANDSHAKE_TIMEOUT_MS);
    }
}
