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
    http::{HeaderMap, HeaderName, HeaderValue, Uri},
    response::{IntoResponse, Response},
    routing::get,
};
use colored::Colorize;
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use iii_sdk::protocol::{ENGINE_CAPABILITIES_HEADER, ENGINE_CAPABILITY_TCP_NODELAY};
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

/// Main worker JSON frame: 16 MiB (16,777,216 bytes), not channel transport.
pub(crate) const MAX_JSON_FRAME_BYTES: usize = 16 * 1024 * 1024;
/// Main worker JSON message: 64 MiB (67,108,864 bytes), not channel transport.
pub(crate) const MAX_JSON_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

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
        // Lets `iii ps` and `iii kill --port` find this engine by its port.
        // Removed when the server task below ends.
        let marker = listener
            .local_addr()
            .ok()
            .and_then(|local| iii_compose::ps::announce_engine(&format!("ws://{local}")));

        let app = worker_router(state);

        let handshake_timeout = Duration::from_millis(config.handshake_timeout_ms);
        let handle = tokio::spawn(async move {
            let _marker = marker;
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
                    // Through accept_worker_connection: it sets TCP_NODELAY (MOT-5377).
                    res = accept_worker_connection(&listener) => match res {
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

/// Routes of the worker listener. Every response lists the engine's transport
/// capabilities in [`ENGINE_CAPABILITIES_HEADER`]: the accept loop sets
/// `TCP_NODELAY` on every connection, so workers may disable Nagle on their
/// end too. Workers wait for this before doing so, because Nagle on the
/// engine's end alone is slower than on both ends (MOT-5377).
///
/// The header is static, so this router must only be served through
/// [`accept_worker_connection`], which sets the `TCP_NODELAY` it promises.
fn worker_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(ws_handler))
        .route("/otel", get(otel_ws_handler))
        .route("/ws/channels/{channel_id}", get(channel_ws_upgrade))
        .layer(axum::middleware::map_response(advertise_capabilities))
        .with_state(state)
}

// `from_static` is a const fn, so an invalid name or value fails the build.
const CAPABILITIES_HEADER: HeaderName = HeaderName::from_static(ENGINE_CAPABILITIES_HEADER);
const CAPABILITIES: HeaderValue = HeaderValue::from_static(ENGINE_CAPABILITY_TCP_NODELAY);

async fn advertise_capabilities(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CAPABILITIES_HEADER, CAPABILITIES);
    response
}

/// Accepts a worker, channel or `/otel` connection with `TCP_NODELAY` set:
/// their small frames must not wait for the peer's delayed ACK (MOT-5377).
async fn accept_worker_connection(
    listener: &TcpListener,
) -> io::Result<(tokio::net::TcpStream, SocketAddr)> {
    let (stream, peer) = listener.accept().await?;
    if let Err(err) = stream.set_nodelay(true) {
        tracing::debug!(peer = %peer, error = ?err, "set_nodelay failed; small writes may stall");
    }
    Ok((stream, peer))
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

    ws.max_frame_size(MAX_JSON_FRAME_BYTES)
        .max_message_size(MAX_JSON_MESSAGE_BYTES)
        .on_upgrade(move |socket| async move {
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

    /// Serves the real worker router on an ephemeral port. Uses `axum::serve`,
    /// not the production hyper loop: `accept_worker_connection` (the
    /// `TCP_NODELAY` half) has its own test.
    async fn serve_worker_router() -> SocketAddr {
        // Engine::new() and the worker connection handler build metrics, which
        // require GLOBAL_METER. Under nextest each test is its own process, so
        // initialize the default (noop) meter here instead of relying on a
        // sibling test.
        crate::workers::observability::metrics::ensure_default_meter();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let state = AppState {
            engine: Arc::new(Engine::new()),
            config: Arc::new(WorkerManagerConfig::default()),
            shutdown_rx,
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _shutdown_tx = shutdown_tx;
            let app = worker_router(state).into_make_service_with_connect_info::<SocketAddr>();
            axum::serve(listener, app).await.unwrap();
        });
        address
    }

    fn advertised(headers: &HeaderMap) -> Option<&str> {
        headers
            .get(ENGINE_CAPABILITIES_HEADER)
            .and_then(|value| value.to_str().ok())
    }

    #[tokio::test]
    async fn worker_listener_advertises_tcp_nodelay_on_upgrade() {
        let address = serve_worker_router().await;
        for path in ["/", "/otel"] {
            let (_socket, response) =
                tokio_tungstenite::connect_async(format!("ws://{address}{path}"))
                    .await
                    .unwrap();
            assert_eq!(
                advertised(response.headers()),
                Some(ENGINE_CAPABILITY_TCP_NODELAY),
                "path {path}"
            );
        }
    }

    #[tokio::test]
    async fn worker_listener_advertises_tcp_nodelay_on_rejected_channel_upgrade() {
        let address = serve_worker_router().await;
        let url = format!("ws://{address}/ws/channels/missing?key=bad&dir=read");
        let error = tokio_tungstenite::connect_async(url).await.unwrap_err();
        let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
            panic!("expected an HTTP rejection, got {error:?}");
        };
        assert_eq!(
            advertised(response.headers()),
            Some(ENGINE_CAPABILITY_TCP_NODELAY)
        );
    }

    #[tokio::test]
    async fn accept_worker_connection_sets_tcp_nodelay() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (stream, _) = accept_worker_connection(&listener).await.unwrap();
        assert!(stream.nodelay().unwrap());
        drop(client);
    }

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

    #[test]
    fn worker_config_rejects_registration_namespace_grace() {
        let result = serde_json::from_str::<WorkerManagerConfig>(
            r#"{"registration_namespace_grace_ms":10000}"#,
        );

        assert!(result.is_err());
    }
}
