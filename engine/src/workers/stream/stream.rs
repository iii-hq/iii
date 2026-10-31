// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc, Mutex as StdMutex, RwLock as SyncRwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use axum::{
    Router,
    extract::{ConnectInfo, State, WebSocketUpgrade, ws::WebSocket},
    http::{HeaderMap, Uri},
    response::IntoResponse,
    routing::get,
};
use chrono::Utc;
use colored::Colorize;
use function_macros::{function, service};
use iii_helpers::stream::{StreamDeleteResult, StreamSetResult, StreamUpdateResult};
use once_cell::sync::Lazy;
use serde_json::Value;
use tokio::{net::TcpListener, task::AbortHandle};

use crate::{
    deprecation::{self, Caller},
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    event_formats::StreamChangedEvent,
    function::FunctionResult,
    observer::{
        Emission,
        events::{STREAM_ON_CHANGED, payload_built, register_stream_event_sources},
    },
    protocol::{DEFAULT_NAMESPACE, ErrorBody},
    telemetry::inject_traceparent_from_context,
    workers::{
        stream::{
            StreamOutboundMessage, StreamSocketManager, StreamWrapperMessage,
            adapters::StreamAdapter,
            config::StreamModuleConfig,
            structs::{
                EventData, StreamAuthContext, StreamAuthInput, StreamDeleteInput, StreamGetInput,
                StreamListAllInput, StreamListAllResult, StreamListGroupsInput, StreamListInput,
                StreamSendInput, StreamSetInput, StreamUpdateInput,
            },
            utils::{headers_to_map, query_to_multi_map},
        },
        traits::{AdapterFactory, ConfigurableWorker, Worker},
    },
};

/// The runtime-swappable pieces of the worker: the live configuration and the
/// live pub/sub adapter. Held together behind one lock so an adapter hot-swap
/// publishes the new config and backend atomically.
struct LiveState {
    config: Arc<StreamModuleConfig>,
    adapter: Arc<dyn StreamAdapter>,
}

#[derive(Clone)]
pub struct StreamWorker {
    pub(super) engine: Arc<Engine>,
    /// Live config + adapter, swapped atomically by `apply_config`. Readers
    /// clone the inner `Arc`s via `config_snapshot`/`adapter_snapshot`.
    live: Arc<SyncRwLock<LiveState>>,
    /// The config.yaml block; used only for `register_config`'s initial value
    /// and the boot bind/adapter fallback.
    seed: Option<StreamModuleConfig>,
    /// Abort handle for the running axum server task (rebound on host/port
    /// change).
    server_abort: Arc<StdMutex<Option<AbortHandle>>>,
    /// Abort handle for the adapter `watch_events` pump (restarted on an
    /// adapter hot-swap).
    watch_abort: Arc<StdMutex<Option<AbortHandle>>>,
    /// Stored once the server is live; `apply_config` refuses to rebind/swap
    /// while this is `None`, and `destroy` clears it to block late applies.
    shutdown_rx: Arc<StdMutex<Option<tokio::sync::watch::Receiver<bool>>>>,
    /// Serializes overlapping `apply_config` runs (last write wins).
    apply_lock: Arc<tokio::sync::Mutex<()>>,
    /// Set by `destroy` so a late retry can't resurrect the server/adapter.
    destroyed: Arc<AtomicBool>,
}

/// True for hosts that restrict the listener to the local machine
/// ("localhost", 127.0.0.0/8, ::1). Used by the boot bind fallback to refuse
/// widening a loopback-only stored address to a non-loopback seed.
fn is_loopback_host(host: &str) -> bool {
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

/// Deprecation entry points that are not function ids.
const WORKER_ENTRY: &str = "iii-stream worker";
const WS_LISTENER_ENTRY: &str = "iii-stream WebSocket listener";
const WS_CONNECTION_ENTRY: &str = "iii-stream WebSocket connection";

impl StreamWorker {
    /// Rate-limited deprecation warning for a `stream::*` function call,
    /// attributed to the engine-injected `_caller_worker_id` when present.
    fn warn_deprecated_call(&self, entry: &'static str, caller_worker_id: Option<&str>) {
        deprecation::warn_stream_deprecated(
            entry,
            Caller::from_worker_id(caller_worker_id),
            &self.engine.worker_registry,
        );
    }
}

async fn ws_handler(
    State(module): State<Arc<StreamSocketManager>>,
    ws: WebSocketUpgrade,
    uri: Uri,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let module = module.clone();
    // Once per client IP per interval, never per frame. The IP is the only
    // client detail logged: no headers, path or query string.
    deprecation::warn_stream_deprecated(
        WS_CONNECTION_ENTRY,
        Caller::Remote(addr.ip()),
        &module.engine.worker_registry,
    );

    if let Some(auth_function) = module.auth_function() {
        let engine = module.engine.clone();
        let input = StreamAuthInput {
            headers: headers_to_map(&headers),
            path: uri.path().to_string(),
            query_params: query_to_multi_map(uri.query()),
            addr: addr.to_string(),
        };
        let input = serde_json::to_value(input);

        match input {
            Ok(input) => match engine.call(&auth_function, input).await {
                Ok(Some(result)) => {
                    let context = serde_json::from_value::<StreamAuthContext>(result);

                    match context {
                        Ok(context) => {
                            return ws.on_upgrade(move |socket: WebSocket| async move {
                                    if let Err(err) = module.socket_handler(socket, Some(context)).await {
                                        tracing::error!(addr = %addr, error = ?err, "stream socket error");
                                    }
                                });
                        }
                        Err(err) => {
                            tracing::error!(error = ?err, "Failed to convert result to context");
                        }
                    }
                }
                Ok(None) => {
                    tracing::debug!("No result from auth function");
                }
                Err(err) => {
                    tracing::error!(error = ?err, "Failed to invoke auth function");
                }
            },
            Err(err) => {
                tracing::error!(error = ?err, "Failed to convert input to value");
            }
        }
    }

    ws.on_upgrade(move |socket: WebSocket| async move {
        if let Err(err) = module.socket_handler(socket, None).await {
            tracing::error!(addr = %addr, error = ?err, "stream socket error");
        }
    })
}

#[async_trait::async_trait]
impl Worker for StreamWorker {
    fn name(&self) -> &'static str {
        "StreamWorker"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    fn hosted_sources(&self) -> Vec<&'static str> {
        use crate::observer::events;
        vec![
            events::STREAM_ON_CHANGED,
            events::STREAM_ON_JOIN,
            events::STREAM_ON_LEAVE,
        ]
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying StreamWorker");
        // The listener belongs to the engine, so it is stopped explicitly to
        // keep ReloadManager restarts duplicate-free.
        super::configuration::stop_listening(&self.engine);

        // Serialize with any in-flight `apply_config`, then block future
        // applies/rebinds: `destroyed` short-circuits apply, and clearing
        // `shutdown_rx` makes the rebind/swap path refuse outright.
        let _guard = self.apply_lock.lock().await;
        self.destroyed.store(true, Ordering::SeqCst);
        self.shutdown_rx
            .lock()
            .expect("stream shutdown_rx mutex poisoned")
            .take();

        if let Some(server) = self
            .server_abort
            .lock()
            .expect("stream server_abort mutex poisoned")
            .take()
        {
            server.abort();
        }
        if let Some(watch) = self
            .watch_abort
            .lock()
            .expect("stream watch_abort mutex poisoned")
            .take()
        {
            watch.abort();
        }
        let _ = self.adapter_snapshot().destroy().await;
        Ok(())
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing StreamWorker");
        // Once per process: a reload re-runs initialize but must not repeat it.
        deprecation::warn_stream_deprecated_once(WORKER_ENTRY);

        register_stream_event_sources(&self.engine);

        Ok(())
    }

    async fn start_background_tasks(
        &self,
        shutdown_rx: tokio::sync::watch::Receiver<bool>,
        _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        // Adopt the configuration worker as the runtime source of truth. Both
        // bus calls are time-bounded: worker startup is awaited serially by the
        // boot and reload pipelines, so a hung `configuration::*` provider must
        // not wedge every other worker behind this one. Failures degrade to the
        // static config.yaml block so the stream server stays up.
        let register = tokio::time::timeout(
            super::configuration::CONFIG_BUS_TIMEOUT,
            super::configuration::register_config(self.engine.as_ref(), self.seed.as_ref()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("configuration::register timed out"))
        .and_then(|result| result);
        if let Err(err) = register {
            tracing::warn!(
                error = %err,
                "iii-stream: configuration::register failed; continuing with static config"
            );
        }
        let fetched = tokio::time::timeout(
            super::configuration::CONFIG_BUS_TIMEOUT,
            super::configuration::fetch_config(self.engine.as_ref(), &self.config_snapshot()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("configuration::get timed out"))
        .and_then(|result| result);
        match fetched {
            Ok(config) => self.set_config(Arc::new(config)),
            Err(err) => tracing::warn!(
                error = %err,
                "iii-stream: failed to read configuration; continuing with static config"
            ),
        }

        // Bind after the fetch so a runtime-edited host/port survives restarts.
        // If the stored address can't be bound, fall back to the seed address
        // rather than failing the whole worker start — GUARDED: requires an
        // explicit seed, the addresses must differ, and it must not widen a
        // loopback-only stored host to a non-loopback seed. The fallback may
        // revert `live.config` to the seed, so adapter adoption is deferred
        // until AFTER the bind: it must reconcile against the config actually
        // being served, never a stored value we failed to honor.
        let config = self.config_snapshot();
        let addr = format!("{}:{}", config.host, config.port);
        let listener = match TcpListener::bind(&addr).await {
            Ok(listener) => listener,
            Err(err) => {
                let Some(fallback) = self.seed.clone() else {
                    return Err(crate::workers::traits::bind_address_error(&addr, err));
                };
                let fallback_addr = format!("{}:{}", fallback.host, fallback.port);
                let widens_loopback =
                    is_loopback_host(&config.host) && !is_loopback_host(&fallback.host);
                if fallback_addr == addr || widens_loopback {
                    if widens_loopback {
                        tracing::error!(
                            stored = %addr,
                            seed = %fallback_addr,
                            "iii-stream: refusing seed fallback that would widen a \
                             loopback-only address to a non-loopback interface"
                        );
                    }
                    return Err(crate::workers::traits::bind_address_error(&addr, err));
                }
                tracing::error!(
                    error = %err,
                    stored = %addr,
                    fallback = %fallback_addr,
                    "iii-stream: stored configuration address cannot be bound; serving on the \
                     seed address — fix the stored `iii-stream` configuration value"
                );
                self.set_config(Arc::new(fallback));
                TcpListener::bind(&fallback_addr).await.map_err(|err| {
                    crate::workers::traits::bind_address_error(&fallback_addr, err)
                })?
            }
        };
        let addr = {
            let config = self.config_snapshot();
            format!("{}:{}", config.host, config.port)
        };
        tracing::info!("Starting StreamWorker on {}", addr.purple());
        deprecation::warn_stream_deprecated_once(WS_LISTENER_ENTRY);

        // Boot adoption of the adapter, reconciled against the config actually
        // served (post-bind, so a fallback's reverted config is honored).
        // `build()` resolved the adapter from the seed; the served config may
        // select a different backend, so resolve and adopt it — keeping config
        // and adapter consistent. A resolve failure keeps the seed adapter
        // (logged) rather than failing worker start.
        let served_config = self.config_snapshot();
        let adapter_outdated = self.seed.as_ref().is_none_or(|seed| {
            Self::effective_adapter(seed) != Self::effective_adapter(&served_config)
        });
        if adapter_outdated {
            // Time-bounded like the boot `configuration::*` calls above: a
            // backend whose `build` hangs must not wedge the serial worker
            // startup loop. A timeout degrades like any resolve failure —
            // keep the seed adapter.
            let resolved = tokio::time::timeout(
                super::configuration::CONFIG_BUS_TIMEOUT,
                Self::resolve_adapter(&self.engine, &served_config),
            )
            .await
            .map_err(|_| anyhow::anyhow!("stream adapter build timed out"))
            .and_then(|result| result);
            match resolved {
                Ok(adapter) => self.set_adapter(adapter),
                Err(err) => {
                    // Keep the live config consistent with the adapter actually
                    // being served: revert just the adapter field to the seed's
                    // (the backend `build` resolved, still running), so
                    // `config_snapshot()` never advertises a backend that isn't
                    // up — which would also let a future no-op apply comparison
                    // treat the unresolved adapter as already applied and never
                    // retry it.
                    tracing::warn!(
                        error = %err,
                        "iii-stream: stored adapter could not be resolved at boot; keeping seed adapter"
                    );
                    let mut reconciled = (*served_config).clone();
                    reconciled.adapter = self.seed.as_ref().and_then(|seed| seed.adapter.clone());
                    self.set_config(Arc::new(reconciled));
                }
            }
        }

        // Spawn the server and the adapter watch pump.
        let server = self.spawn_server(listener, addr, shutdown_rx.clone());
        *self
            .server_abort
            .lock()
            .expect("stream server_abort mutex poisoned") = Some(server);
        let watch = self.spawn_watch(self.adapter_snapshot(), shutdown_rx.clone());
        *self
            .watch_abort
            .lock()
            .expect("stream watch_abort mutex poisoned") = Some(watch);

        // Store the receiver only once the server is live: `apply_config`
        // refuses to rebind/swap while this is `None`, so a stray
        // configuration change cannot act during (or instead of) the boot
        // sequence.
        *self
            .shutdown_rx
            .lock()
            .expect("stream shutdown_rx mutex poisoned") = Some(shutdown_rx);

        super::configuration::listen_for_changes(self);
        // Catch-up pass: replay any `configuration::set` that landed between
        // the boot fetch above and the subscription. Routed through
        // `on_config_change` so a timed-out catch-up gets the one-shot retry.
        super::configuration::on_config_change(self).await;

        Ok(())
    }
}

#[async_trait::async_trait]
impl ConfigurableWorker for StreamWorker {
    type Config = StreamModuleConfig;
    type Adapter = dyn StreamAdapter;
    type AdapterRegistration = super::registry::StreamAdapterRegistration;
    const DEFAULT_ADAPTER_NAME: &'static str = "kv";

    async fn registry() -> &'static SyncRwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<SyncRwLock<HashMap<String, AdapterFactory<dyn StreamAdapter>>>> =
            Lazy::new(|| SyncRwLock::new(StreamWorker::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        let seed = Some(config.clone());
        Self {
            engine,
            live: Arc::new(SyncRwLock::new(LiveState {
                config: Arc::new(config),
                adapter,
            })),
            seed,
            server_abort: Arc::new(StdMutex::new(None)),
            watch_abort: Arc::new(StdMutex::new(None)),
            shutdown_rx: Arc::new(StdMutex::new(None)),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
            destroyed: Arc::new(AtomicBool::new(false)),
        }
    }

    fn adapter_name_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.name.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

impl StreamWorker {
    /// Snapshot the live configuration (cheap `Arc` clone, no lock held by the
    /// caller).
    pub fn config_snapshot(&self) -> Arc<StreamModuleConfig> {
        self.live
            .read()
            .expect("stream live state poisoned")
            .config
            .clone()
    }

    /// Snapshot the live pub/sub adapter (cheap `Arc` clone).
    pub fn adapter_snapshot(&self) -> Arc<dyn StreamAdapter> {
        self.live
            .read()
            .expect("stream live state poisoned")
            .adapter
            .clone()
    }

    fn set_config(&self, config: Arc<StreamModuleConfig>) {
        self.live
            .write()
            .expect("stream live state poisoned")
            .config = config;
    }

    fn set_adapter(&self, adapter: Arc<dyn StreamAdapter>) {
        self.live
            .write()
            .expect("stream live state poisoned")
            .adapter = adapter;
    }

    fn set_live(&self, config: Arc<StreamModuleConfig>, adapter: Arc<dyn StreamAdapter>) {
        let mut guard = self.live.write().expect("stream live state poisoned");
        guard.config = config;
        guard.adapter = adapter;
    }

    /// The effective `(adapter_name, adapter_config)`, so `None` and an explicit
    /// built-in default compare equal — no false "changed" verdict on boot
    /// adoption or an `auth_function`-only edit.
    fn effective_adapter(config: &StreamModuleConfig) -> (String, Option<Value>) {
        (
            Self::adapter_name_from_config(config)
                .unwrap_or_else(|| Self::DEFAULT_ADAPTER_NAME.to_string()),
            Self::adapter_config_from_config(config),
        )
    }

    /// Resolve an `Arc<dyn StreamAdapter>` from a config, mirroring steps 3–4 of
    /// `create_with_adapters` so a runtime adapter edit builds the same backend
    /// the boot path would.
    async fn resolve_adapter(
        engine: &Arc<Engine>,
        config: &StreamModuleConfig,
    ) -> anyhow::Result<Arc<dyn StreamAdapter>> {
        let adapter_name = Self::adapter_name_from_config(config)
            .unwrap_or_else(|| Self::DEFAULT_ADAPTER_NAME.to_string());
        let factory = Self::get_adapter(&adapter_name).await.ok_or_else(|| {
            anyhow::anyhow!("stream adapter factory '{}' not found", adapter_name)
        })?;
        let adapter_config = Self::adapter_config_from_config(config);
        factory(engine.clone(), adapter_config).await
    }

    /// Build a worker straight from a JSON config for tests (async because
    /// adapter resolution is). Mirrors `create_with_adapters` then `build`.
    pub async fn for_test(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Self> {
        let parsed: StreamModuleConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let adapter = Self::resolve_adapter(&engine, &parsed).await?;
        Ok(Self::build(engine, parsed, adapter))
    }

    /// Re-applies the stored `iii-stream` entry now, as a change of it does.
    #[doc(hidden)]
    pub async fn on_configuration_change(&self) {
        super::configuration::on_config_change(self).await;
    }

    /// Spawn the axum WebSocket server for an already-bound listener and return
    /// its abort handle. The socket manager reads the worker's live config and
    /// adapter per connection, so `auth_function`/adapter edits apply without a
    /// respawn; only a host/port change needs a rebind.
    fn spawn_server(
        &self,
        listener: TcpListener,
        addr_display: String,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> AbortHandle {
        let socket_manager = Arc::new(StreamSocketManager::new(
            self.engine.clone(),
            Arc::new(self.clone()),
        ));
        let app = Router::new()
            .route("/", get(ws_handler))
            .with_state(socket_manager);
        let handle = tokio::spawn(async move {
            tracing::info!("Stream API listening on address: {}", addr_display.purple());
            let serve = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                while shutdown_rx.changed().await.is_ok() {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
            });
            if let Err(e) = serve.await {
                tracing::error!(address = %addr_display, error = %e, "Stream server exited with error");
            }
        });
        handle.abort_handle()
    }

    /// Spawn the adapter `watch_events` pump for the given adapter and return
    /// its abort handle. Restarted on an adapter hot-swap; the captured `Arc`
    /// keeps the backend alive for as long as the pump runs.
    fn spawn_watch(
        &self,
        adapter: Arc<dyn StreamAdapter>,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> AbortHandle {
        let handle = tokio::spawn(async move {
            tokio::select! {
                result = adapter.watch_events() => {
                    if let Err(e) = result {
                        tracing::error!(error = %e, "Failed to watch events");
                    }
                }
                _ = async {
                    while shutdown_rx.changed().await.is_ok() {
                        if *shutdown_rx.borrow() {
                            break;
                        }
                    }
                } => {
                    tracing::info!("Stream watch_events shutdown signal received");
                }
            }
        });
        handle.abort_handle()
    }

    /// Schedule graceful teardown of an adapter that was just swapped out.
    ///
    /// Each WebSocket connection captures its own `Arc` to the backend it
    /// subscribed to and holds it for its lifetime (see
    /// `StreamSocketManager::socket_handler`), so the previous backend must stay
    /// alive while any orphaned connection still uses it. We poll until only
    /// this task's `Arc` remains — every orphaned connection (and the aborted
    /// watch pump) has released it — then run `StreamAdapter::destroy()` so
    /// stateful backends (redis closes connections, bridge stops its socket
    /// thread) release their resources. A plain `Arc` drop would NOT run
    /// `destroy()`. Bounded so a never-closing connection can't keep the task
    /// (and the backend) alive forever.
    fn schedule_adapter_teardown(previous_adapter: Arc<dyn StreamAdapter>) {
        const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
        const MAX_DRAIN: std::time::Duration = std::time::Duration::from_secs(600);
        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + MAX_DRAIN;
            // strong_count == 1 means only this task holds the previous backend.
            while Arc::strong_count(&previous_adapter) > 1 {
                if tokio::time::Instant::now() >= deadline {
                    tracing::warn!(
                        "iii-stream: previous adapter still referenced after {MAX_DRAIN:?}; \
                         tearing it down anyway"
                    );
                    break;
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            if let Err(err) = previous_adapter.destroy().await {
                tracing::warn!(error = %err, "iii-stream: previous adapter teardown failed");
            }
        });
    }

    /// Re-fetch the authoritative configuration and hot-apply it under
    /// `apply_lock` (so overlapping events can't apply a stale value last).
    /// All-or-nothing on the fallible prerequisites: a failed adapter resolve or
    /// listener bind keeps the previous config, adapter, and server.
    ///
    /// `auth_function`-only change → swap the config snapshot (the running
    /// server reads it per connection). `adapter` change → build the new
    /// backend, swap it in, and restart the `watch_events` pump (existing
    /// connections stay bound to the previous backend until they close).
    /// `host`/`port` change → bind the new address, respawn the server on it,
    /// then abort the old listener.
    pub(super) async fn apply_config(&self) -> anyhow::Result<()> {
        let _guard = self.apply_lock.lock().await;

        // `destroy` sets this and aborts the server; a late retry must not
        // resurrect anything (or build a throwaway adapter below).
        if self.destroyed.load(Ordering::SeqCst) {
            return Ok(());
        }

        let new_config = match tokio::time::timeout(
            super::configuration::CONFIG_BUS_TIMEOUT,
            super::configuration::fetch_config(self.engine.as_ref(), &self.config_snapshot()),
        )
        .await
        {
            Ok(result) => result?,
            // Keep the `Elapsed` error downcastable: `on_config_change`
            // schedules a one-shot retry for timeouts specifically.
            Err(elapsed) => {
                return Err(anyhow::Error::new(elapsed)
                    .context("configuration::get timed out; keeping previous config"));
            }
        };

        let old = self.config_snapshot();
        let addr_changed =
            (old.host.as_str(), old.port) != (new_config.host.as_str(), new_config.port);
        let adapter_changed = Self::effective_adapter(&old) != Self::effective_adapter(&new_config);

        if !addr_changed && !adapter_changed {
            // Pure `auth_function` (or no-op) change: the server reads the live
            // config per connection, so a snapshot swap is all that is needed.
            self.set_config(Arc::new(new_config));
            return Ok(());
        }

        // Resolve every fallible prerequisite BEFORE mutating live state, so a
        // failure leaves the previous config/adapter/server untouched. The
        // adapter build is time-bounded (like the `configuration::*` bus calls):
        // a custom backend whose `build` hangs must not wedge `apply_lock` —
        // and with it `destroy`, which also takes that lock.
        let new_adapter = if adapter_changed {
            match tokio::time::timeout(
                super::configuration::CONFIG_BUS_TIMEOUT,
                Self::resolve_adapter(&self.engine, &new_config),
            )
            .await
            {
                Ok(result) => Some(result?),
                // Keep the `Elapsed` downcastable so `on_config_change` retries
                // a transient build timeout once, like a `configuration::get`
                // timeout.
                Err(elapsed) => {
                    return Err(anyhow::Error::new(elapsed)
                        .context("stream adapter build timed out; keeping previous config"));
                }
            }
        } else {
            None
        };

        // The `shutdown_rx` check doubles as the started/destroyed guard
        // (`destroy` clears it). Checking it before binding means a post-destroy
        // retry can never transiently bind the stored address.
        let shutdown_rx = self
            .shutdown_rx
            .lock()
            .expect("stream shutdown_rx mutex poisoned")
            .clone()
            .ok_or_else(|| anyhow::anyhow!("stream server was never started; cannot apply"))?;

        let new_addr = format!("{}:{}", new_config.host, new_config.port);
        let listener = if addr_changed {
            Some(
                TcpListener::bind(&new_addr)
                    .await
                    .map_err(|err| crate::workers::traits::bind_address_error(&new_addr, err))?,
            )
        } else {
            None
        };

        // Capture the outgoing adapter before the swap so it can be torn down
        // once it has drained (see `schedule_adapter_teardown`). Only relevant
        // when the adapter actually changes.
        let previous_adapter = new_adapter.as_ref().map(|_| self.adapter_snapshot());

        // Past the gate — commit.
        let new_config = Arc::new(new_config);
        match &new_adapter {
            Some(adapter) => self.set_live(new_config, adapter.clone()),
            None => self.set_config(new_config),
        }

        // Adapter swap: restart the watch pump on the new backend, abort the
        // old pump (releasing its old-adapter `Arc`). Existing connections keep
        // their own old-adapter `Arc` alive until they close, so they stay bound
        // to the previous backend (accepted: they no longer receive new events).
        if let Some(adapter) = new_adapter {
            let new_watch = self.spawn_watch(adapter, shutdown_rx.clone());
            let previous = self
                .watch_abort
                .lock()
                .expect("stream watch_abort mutex poisoned")
                .replace(new_watch);
            if let Some(previous) = previous {
                previous.abort();
            }
            // Tear down the swapped-out backend once no live connection still
            // references it, so stateful backends (redis/bridge) release their
            // resources without cutting off orphaned connections mid-swap.
            if let Some(previous_adapter) = previous_adapter {
                Self::schedule_adapter_teardown(previous_adapter);
            }
        }

        // Address change: respawn the server on the new listener, then abort
        // the old one (the addresses differ, so no self-conflict). Live
        // connections on the old listener are dropped — same semantics as
        // `destroy`.
        if let Some(listener) = listener {
            let new_server = self.spawn_server(listener, new_addr.clone(), shutdown_rx);
            let previous = self
                .server_abort
                .lock()
                .expect("stream server_abort mutex poisoned")
                .replace(new_server);
            if let Some(previous) = previous {
                previous.abort();
            }
            tracing::info!(new = %new_addr, "iii-stream server rebound after configuration change");
        }

        Ok(())
    }
}

impl StreamWorker {
    /// One call of `stream::on-changed` for `message`, a write or a custom
    /// event the stream's subscribers get too. Nothing is built while nothing
    /// observes the source. A delivery on a devtools stream (`iii:devtools:*`,
    /// e.g. the console's state feed) carries no trace context: a traced
    /// delivery of observability-derived data would re-enter the span store it
    /// was derived from, an endless loop. Every other one runs in the
    /// writer's trace.
    fn emit_changed(&self, message: &StreamWrapperMessage) {
        let observers = &self.engine.observers;
        if !observers.is_observed(DEFAULT_NAMESPACE, STREAM_ON_CHANGED) {
            return;
        }
        let traceparent = if message.stream_name.starts_with("iii:devtools:") {
            None
        } else {
            inject_traceparent_from_context(&opentelemetry::Context::current())
        };
        observers.emit_with(
            DEFAULT_NAMESPACE,
            STREAM_ON_CHANGED,
            Emission {
                depth: None,
                traceparent,
            },
            || {
                payload_built();
                changed_event(message)
            },
        );
    }
}

/// The `stream::on-changed` event of `message`.
fn changed_event(message: &StreamWrapperMessage) -> Option<Value> {
    let event_type = match &message.event {
        StreamOutboundMessage::Sync { .. } => "sync",
        StreamOutboundMessage::Create { .. } => "create",
        StreamOutboundMessage::Update { .. } => "update",
        StreamOutboundMessage::Delete { .. } => "delete",
        StreamOutboundMessage::Event { .. } => "event",
    };
    serde_json::to_value(StreamChangedEvent {
        event_type: event_type.to_string(),
        timestamp: message.timestamp,
        stream_name: message.stream_name.clone(),
        group_id: message.group_id.clone(),
        id: message.id.clone(),
        event: serde_json::to_value(&message.event).ok()?,
    })
    .ok()
}

#[service(name = "stream")]
impl StreamWorker {
    #[function(id = "stream::set", description = "Set a value in a stream")]
    pub async fn set(&self, input: StreamSetInput) -> FunctionResult<StreamSetResult, ErrorBody> {
        self.warn_deprecated_call("stream::set", input.caller_worker_id.as_deref());
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;
        let data = input.data;

        let function_id = format!("stream::set({})", stream_name);
        let function = self
            .engine
            .functions
            .get(crate::protocol::DEFAULT_NAMESPACE, &function_id);
        let adapter = self.adapter_snapshot();

        let result: anyhow::Result<StreamSetResult> = match function {
            Some(_) => {
                tracing::debug!(function_id = %function_id, "Calling custom stream.set function");

                let input = match serde_json::to_value(cloned_input) {
                    Ok(input) => input,
                    Err(e) => {
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to convert input to value: {}", e),
                            code: "JSON_ERROR".to_string(),
                            stacktrace: None,
                        });
                    }
                };
                let result = self.engine.call(&function_id, input).await;

                match result {
                    Ok(Some(result)) => match serde_json::from_value::<StreamSetResult>(result) {
                        Ok(result) => Ok(result),
                        Err(e) => {
                            return FunctionResult::Failure(ErrorBody {
                                message: format!("Failed to convert result to value: {}", e),
                                code: "JSON_ERROR".to_string(),
                                stacktrace: None,
                            });
                        }
                    },
                    Ok(None) => Err(anyhow::anyhow!("Function returned no result")),
                    Err(error) => Err(anyhow::anyhow!("Failed to invoke function: {:?}", error)),
                }
            }
            None => {
                adapter
                    .set(&stream_name, &group_id, &item_id, data.clone())
                    .await
            }
        };

        crate::workers::telemetry::collector::track_stream_set();
        match result {
            Ok(result) => {
                let event = if result.old_value.is_some() {
                    StreamOutboundMessage::Update {
                        data: result.new_value.clone(),
                    }
                } else {
                    StreamOutboundMessage::Create {
                        data: result.new_value.clone(),
                    }
                };

                let message = StreamWrapperMessage {
                    event_type: "stream".to_string(),
                    id: Some(item_id.clone()),
                    timestamp: Utc::now().timestamp_millis(),
                    stream_name: stream_name.clone(),
                    group_id: group_id.clone(),
                    event,
                };

                self.emit_changed(&message);

                if let Err(e) = adapter.emit_event(message).await {
                    // The value is stored; only live subscribers miss this
                    // event. The Success response is kept (contract unchanged).
                    tracing::error!(
                        entry_point = "stream::set",
                        error = %e,
                        "stream::set stored the value but failed to emit the change event; \
                         WebSocket subscribers will not receive it"
                    );
                }

                FunctionResult::Success(result)
            }
            Err(error) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to set value: {}", error),
                code: "STREAM_SET_ERROR".to_string(),
                stacktrace: None,
            }),
        }
    }

    #[function(id = "stream::get", description = "Get a value from a stream")]
    pub async fn get(&self, input: StreamGetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.warn_deprecated_call("stream::get", input.caller_worker_id.as_deref());
        self.get_item(input).await
    }

    /// `stream::get` without the deprecation warning, for the WebSocket join
    /// sync (that connection is already warned about once per client IP).
    pub(crate) async fn get_item(
        &self,
        input: StreamGetInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;

        let function_id = format!("stream::get({})", stream_name);
        let function = self
            .engine
            .functions
            .get(crate::protocol::DEFAULT_NAMESPACE, &function_id);
        let adapter = self.adapter_snapshot();

        crate::workers::telemetry::collector::track_stream_get();
        match function {
            Some(_) => {
                tracing::debug!(function_id = %function_id, "Calling custom stream.get function");

                let input = match serde_json::to_value(cloned_input) {
                    Ok(input) => input,
                    Err(e) => {
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to convert input to value: {}", e),
                            code: "JSON_ERROR".to_string(),
                            stacktrace: None,
                        });
                    }
                };

                let result = self.engine.call(&function_id, input).await;

                match result {
                    Ok(result) => FunctionResult::Success(result),
                    Err(error) => FunctionResult::Failure(error),
                }
            }
            None => match adapter.get(&stream_name, &group_id, &item_id).await {
                Ok(value) => FunctionResult::Success(value),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get value from stream");
                    FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to get value: {}", e),
                        code: "STREAM_GET_ERROR".to_string(),
                        stacktrace: None,
                    })
                }
            },
        }
    }

    #[function(id = "stream::delete", description = "Delete a value from a stream")]
    pub async fn delete(
        &self,
        input: StreamDeleteInput,
    ) -> FunctionResult<StreamDeleteResult, ErrorBody> {
        self.warn_deprecated_call("stream::delete", input.caller_worker_id.as_deref());
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;
        let function_id = format!("stream::delete({})", stream_name);
        let function = self
            .engine
            .functions
            .get(crate::protocol::DEFAULT_NAMESPACE, &function_id);
        let adapter = self.adapter_snapshot();

        let result: anyhow::Result<StreamDeleteResult> = match function {
            Some(_) => {
                tracing::debug!(function_id = %function_id, "Calling custom stream.delete function");

                let input = match serde_json::to_value(cloned_input) {
                    Ok(input) => input,
                    Err(e) => {
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to convert input to value: {}", e),
                            code: "JSON_ERROR".to_string(),
                            stacktrace: None,
                        });
                    }
                };
                let result = self.engine.call(&function_id, input).await;
                match result {
                    Ok(Some(result)) => {
                        let result = match serde_json::from_value::<StreamDeleteResult>(result) {
                            Ok(result) => result,
                            Err(e) => {
                                return FunctionResult::Failure(ErrorBody {
                                    message: format!("Failed to convert result to value: {}", e),
                                    code: "JSON_ERROR".to_string(),
                                    stacktrace: None,
                                });
                            }
                        };
                        Ok(result)
                    }
                    Ok(None) => Err(anyhow::anyhow!("Function returned no result")),
                    Err(error) => Err(anyhow::anyhow!("Failed to invoke function: {:?}", error)),
                }
            }
            None => adapter.delete(&stream_name, &group_id, &item_id).await,
        };

        crate::workers::telemetry::collector::track_stream_delete();
        match result {
            Ok(result) => {
                if let Some(old_value) = result.old_value.clone() {
                    let message = StreamWrapperMessage {
                        event_type: "stream".to_string(),
                        id: Some(item_id.clone()),
                        timestamp: Utc::now().timestamp_millis(),
                        stream_name: stream_name.clone(),
                        group_id: group_id.clone(),
                        event: StreamOutboundMessage::Delete { data: old_value },
                    };

                    self.emit_changed(&message);

                    if let Err(e) = adapter.emit_event(message).await {
                        // The delete happened; only live subscribers miss
                        // this event. The Success response is kept.
                        tracing::error!(
                            entry_point = "stream::delete",
                            error = %e,
                            "stream::delete removed the value but failed to emit the change \
                             event; WebSocket subscribers will not receive it"
                        );
                    }
                }

                FunctionResult::Success(result)
            }
            Err(error) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to delete value: {}", error),
                code: "STREAM_DELETE_ERROR".to_string(),
                stacktrace: None,
            }),
        }
    }

    #[function(id = "stream::list", description = "List all items in a stream group")]
    pub async fn list(&self, input: StreamListInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.warn_deprecated_call("stream::list", input.caller_worker_id.as_deref());
        self.list_items(input).await
    }

    /// `stream::list` without the deprecation warning, for the WebSocket join
    /// sync (that connection is already warned about once per client IP).
    pub(crate) async fn list_items(
        &self,
        input: StreamListInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;

        let function_id = format!("stream::list({})", stream_name);
        let function = self
            .engine
            .functions
            .get(crate::protocol::DEFAULT_NAMESPACE, &function_id);
        let adapter = self.adapter_snapshot();

        crate::workers::telemetry::collector::track_stream_list();
        match function {
            Some(_) => {
                tracing::debug!(function_id = %function_id, "Calling custom stream.getGroup function");

                let input = match serde_json::to_value(cloned_input) {
                    Ok(input) => input,
                    Err(e) => {
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to convert input to value: {}", e),
                            code: "JSON_ERROR".to_string(),
                            stacktrace: None,
                        });
                    }
                };

                let result = self.engine.call(&function_id, input).await;

                match result {
                    Ok(result) => FunctionResult::Success(result),
                    Err(error) => FunctionResult::Failure(error),
                }
            }
            None => match adapter.get_group(&stream_name, &group_id).await {
                Ok(values) => FunctionResult::Success(Some(Value::Array(values))),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get group from stream");
                    FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to get group: {}", e),
                        code: "STREAM_GET_GROUP_ERROR".to_string(),
                        stacktrace: None,
                    })
                }
            },
        }
    }

    #[function(
        id = "stream::list_groups",
        description = "List all groups in a stream"
    )]
    pub async fn list_groups(
        &self,
        input: StreamListGroupsInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        self.warn_deprecated_call("stream::list_groups", input.caller_worker_id.as_deref());
        let cloned_input = input.clone();
        let stream_name = input.stream_name;

        let function_id = format!("stream::list_groups({})", stream_name);
        let function = self
            .engine
            .functions
            .get(crate::protocol::DEFAULT_NAMESPACE, &function_id);
        let adapter = self.adapter_snapshot();

        match function {
            Some(_) => {
                tracing::debug!(function_id = %function_id, "Calling custom stream.list_groups function");

                let input = match serde_json::to_value(cloned_input) {
                    Ok(input) => input,
                    Err(e) => {
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to convert input to value: {}", e),
                            code: "JSON_ERROR".to_string(),
                            stacktrace: None,
                        });
                    }
                };
                let result = self.engine.call(&function_id, input).await;

                match result {
                    Ok(result) => FunctionResult::Success(result),
                    Err(error) => FunctionResult::Failure(error),
                }
            }
            None => match adapter.list_groups(&stream_name).await {
                Ok(groups) => FunctionResult::Success(serde_json::to_value(groups).ok()),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to list groups from stream");
                    FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to list groups: {}", e),
                        code: "STREAM_LIST_GROUPS_ERROR".to_string(),
                        stacktrace: None,
                    })
                }
            },
        }
    }

    #[function(
        id = "stream::list_all",
        description = "List all available stream with metadata"
    )]
    pub async fn list_all(
        &self,
        input: StreamListAllInput,
    ) -> FunctionResult<StreamListAllResult, ErrorBody> {
        self.warn_deprecated_call("stream::list_all", input.caller_worker_id.as_deref());
        let adapter = self.adapter_snapshot();

        match adapter.list_all_stream().await {
            Ok(stream) => {
                let count = stream.len();
                FunctionResult::Success(StreamListAllResult { stream, count })
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to list all stream");
                FunctionResult::Failure(ErrorBody {
                    message: format!("Failed to list stream: {}", e),
                    code: "STREAM_LIST_ALL_ERROR".to_string(),
                    stacktrace: None,
                })
            }
        }
    }

    #[function(
        id = "stream::send",
        description = "Send a custom event to stream subscribers"
    )]
    pub async fn send(&self, input: StreamSendInput) -> FunctionResult<(), ErrorBody> {
        self.warn_deprecated_call("stream::send", input.caller_worker_id.as_deref());
        let message = StreamWrapperMessage {
            event_type: "stream".to_string(),
            timestamp: Utc::now().timestamp_millis(),
            stream_name: input.stream_name.clone(),
            group_id: input.group_id.clone(),
            id: input.id.clone(),
            event: StreamOutboundMessage::Event {
                event: EventData {
                    event_type: input.event_type,
                    data: input.data,
                },
            },
        };

        self.emit_changed(&message);

        if let Err(e) = self.adapter_snapshot().emit_event(message).await {
            tracing::error!(error = %e, "Failed to emit stream send event");
            return FunctionResult::Failure(ErrorBody {
                message: format!("Failed to send event: {}", e),
                code: "STREAM_SEND_ERROR".to_string(),
                stacktrace: None,
            });
        }

        FunctionResult::Success(())
    }

    #[function(
        id = "stream::update",
        description = "Atomically update a stream value with multiple operations"
    )]
    pub async fn update(
        &self,
        input: StreamUpdateInput,
    ) -> FunctionResult<StreamUpdateResult, ErrorBody> {
        self.warn_deprecated_call("stream::update", input.caller_worker_id.as_deref());
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;
        let ops = input.ops;

        tracing::debug!(stream_name = %stream_name, group_id = %group_id, item_id = %item_id, ops_count = ops.len(), "Executing atomic stream update");

        let function_id = format!("stream::update({})", stream_name);
        let function = self
            .engine
            .functions
            .get(crate::protocol::DEFAULT_NAMESPACE, &function_id);
        let adapter = self.adapter_snapshot();

        let result: anyhow::Result<StreamUpdateResult> = match function {
            Some(_) => {
                tracing::debug!(function_id = %function_id, "Calling custom stream.set function");

                let input = match serde_json::to_value(cloned_input) {
                    Ok(input) => input,
                    Err(e) => {
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to convert input to value: {}", e),
                            code: "JSON_ERROR".to_string(),
                            stacktrace: None,
                        });
                    }
                };
                let result = self.engine.call(&function_id, input).await;

                match result {
                    Ok(Some(result)) => {
                        match serde_json::from_value::<StreamUpdateResult>(result) {
                            Ok(result) => Ok(result),
                            Err(e) => {
                                return FunctionResult::Failure(ErrorBody {
                                    message: format!("Failed to convert result to value: {}", e),
                                    code: "JSON_ERROR".to_string(),
                                    stacktrace: None,
                                });
                            }
                        }
                    }
                    Ok(None) => Err(anyhow::anyhow!("Function returned no result")),
                    Err(error) => Err(anyhow::anyhow!("Failed to invoke function: {:?}", error)),
                }
            }
            None => adapter.update(&stream_name, &group_id, &item_id, ops).await,
        };

        crate::workers::telemetry::collector::track_stream_update();
        match result {
            Ok(result) => {
                let event = if result.old_value.is_some() {
                    StreamOutboundMessage::Update {
                        data: result.new_value.clone(),
                    }
                } else {
                    StreamOutboundMessage::Create {
                        data: result.new_value.clone(),
                    }
                };

                let message = StreamWrapperMessage {
                    event_type: "stream".to_string(),
                    id: Some(item_id.clone()),
                    timestamp: Utc::now().timestamp_millis(),
                    stream_name: stream_name.clone(),
                    group_id: group_id.clone(),
                    event,
                };

                self.emit_changed(&message);

                if let Err(e) = adapter.emit_event(message).await {
                    // The value is stored; only live subscribers miss this
                    // event. The Success response is kept (contract unchanged).
                    tracing::error!(
                        entry_point = "stream::update",
                        error = %e,
                        "stream::update stored the value but failed to emit the change event; \
                         WebSocket subscribers will not receive it"
                    );
                }

                FunctionResult::Success(result)
            }
            Err(error) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to update value: {}", error),
                code: "STREAM_UPDATE_ERROR".to_string(),
                stacktrace: None,
            }),
        }
    }
}

crate::register_worker!(
    "iii-stream",
    StreamWorker,
    description = "Build durable streams for real-time data subscriptions.",
    enabled_by_default = true
);

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    use async_trait::async_trait;
    use iii_helpers::stream::{StreamDeleteResult, StreamSetResult, StreamUpdateResult, UpdateOp};
    use serde_json::Value;
    use tokio::sync::mpsc;

    use crate::{
        builtins::pubsub_lite::Subscriber,
        engine::{Engine, EngineTrait},
        observer::events::test_support,
        protocol::ErrorBody,
        workers::stream::{
            StreamMetadata,
            adapters::{StreamAdapter, StreamConnection},
            config::StreamModuleConfig,
        },
    };

    use super::*;

    #[test]
    fn effective_adapter_treats_none_as_default() {
        use crate::workers::traits::AdapterEntry;

        // An absent adapter and an explicit built-in-default adapter must
        // compare equal, so boot adoption (or an auth_function-only edit) is not
        // mistaken for an adapter change that would trigger a needless hot-swap.
        let none = StreamModuleConfig::default();
        assert!(none.adapter.is_none());
        let explicit = StreamModuleConfig {
            adapter: Some(AdapterEntry {
                name: StreamWorker::DEFAULT_ADAPTER_NAME.to_string(),
                config: None,
            }),
            ..StreamModuleConfig::default()
        };
        assert_eq!(
            StreamWorker::effective_adapter(&none),
            StreamWorker::effective_adapter(&explicit),
            "None and the explicit default adapter must be the same effective adapter"
        );

        let other = StreamModuleConfig {
            adapter: Some(AdapterEntry {
                name: "redis".to_string(),
                config: None,
            }),
            ..StreamModuleConfig::default()
        };
        assert_ne!(
            StreamWorker::effective_adapter(&none),
            StreamWorker::effective_adapter(&other),
            "a different adapter name must be a different effective adapter"
        );
    }

    struct RecordingConnection {
        tx: mpsc::UnboundedSender<StreamWrapperMessage>,
    }

    #[async_trait]
    impl StreamConnection for RecordingConnection {
        async fn cleanup(&self) {}

        async fn handle_stream_message(&self, msg: &StreamWrapperMessage) -> anyhow::Result<()> {
            let _ = self.tx.send(msg.clone());
            Ok(())
        }
    }

    #[async_trait]
    impl Subscriber for RecordingConnection {
        async fn handle_message(&self, message: Arc<Value>) -> anyhow::Result<()> {
            let msg = match serde_json::from_value::<StreamWrapperMessage>((*message).clone()) {
                Ok(msg) => msg,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to deserialize stream message");
                    return Err(anyhow::anyhow!("Failed to deserialize stream message"));
                }
            };
            let _ = self.tx.send(msg);
            Ok(())
        }
    }

    fn create_test_module() -> StreamWorker {
        crate::workers::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let config = StreamModuleConfig {
            port: 0, // Use 0 for testing (OS will assign port)
            host: "127.0.0.1".to_string(),
            auth_function: None,
            adapter: Some(crate::workers::traits::AdapterEntry {
                name: "kv".to_string(),
                config: None,
            }),
        };

        // Create adapter directly using kv_store adapter
        let adapter: Arc<dyn StreamAdapter> =
            Arc::new(crate::workers::stream::adapters::kv_store::BuiltinKvStoreAdapter::new(None));

        StreamWorker::build(engine, config, adapter)
    }

    struct FakeStreamAdapter {
        set_result: Mutex<Result<StreamSetResult, String>>,
        get_result: Mutex<Result<Option<Value>, String>>,
        delete_result: Mutex<Result<StreamDeleteResult, String>>,
        get_group_result: Mutex<Result<Vec<Value>, String>>,
        list_groups_result: Mutex<Result<Vec<String>, String>>,
        list_all_result: Mutex<Result<Vec<StreamMetadata>, String>>,
        emit_event_result: Mutex<Result<(), String>>,
        update_result: Mutex<Result<StreamUpdateResult, String>>,
        emitted_messages: Mutex<Vec<StreamWrapperMessage>>,
        destroy_called: AtomicBool,
        watch_events_called: AtomicBool,
    }

    impl Default for FakeStreamAdapter {
        fn default() -> Self {
            Self {
                set_result: Mutex::new(Ok(StreamSetResult {
                    old_value: None,
                    new_value: serde_json::json!({}),
                })),
                get_result: Mutex::new(Ok(None)),
                delete_result: Mutex::new(Ok(StreamDeleteResult { old_value: None })),
                get_group_result: Mutex::new(Ok(Vec::new())),
                list_groups_result: Mutex::new(Ok(Vec::new())),
                list_all_result: Mutex::new(Ok(Vec::new())),
                emit_event_result: Mutex::new(Ok(())),
                update_result: Mutex::new(Ok(StreamUpdateResult {
                    old_value: None,
                    new_value: serde_json::json!({}),
                    errors: Vec::new(),
                })),
                emitted_messages: Mutex::new(Vec::new()),
                destroy_called: AtomicBool::new(false),
                watch_events_called: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl StreamAdapter for FakeStreamAdapter {
        async fn set(
            &self,
            _stream_name: &str,
            _group_id: &str,
            _item_id: &str,
            _data: Value,
        ) -> anyhow::Result<StreamSetResult> {
            self.set_result
                .lock()
                .expect("lock set_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn get(
            &self,
            _stream_name: &str,
            _group_id: &str,
            _item_id: &str,
        ) -> anyhow::Result<Option<Value>> {
            self.get_result
                .lock()
                .expect("lock get_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn delete(
            &self,
            _stream_name: &str,
            _group_id: &str,
            _item_id: &str,
        ) -> anyhow::Result<StreamDeleteResult> {
            self.delete_result
                .lock()
                .expect("lock delete_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn get_group(
            &self,
            _stream_name: &str,
            _group_id: &str,
        ) -> anyhow::Result<Vec<Value>> {
            self.get_group_result
                .lock()
                .expect("lock get_group_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn list_groups(&self, _stream_name: &str) -> anyhow::Result<Vec<String>> {
            self.list_groups_result
                .lock()
                .expect("lock list_groups_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn list_all_stream(&self) -> anyhow::Result<Vec<StreamMetadata>> {
            self.list_all_result
                .lock()
                .expect("lock list_all_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn emit_event(&self, message: StreamWrapperMessage) -> anyhow::Result<()> {
            self.emitted_messages
                .lock()
                .expect("lock emitted_messages")
                .push(message);
            self.emit_event_result
                .lock()
                .expect("lock emit_event_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn subscribe(
            &self,
            _id: String,
            _connection: Arc<dyn StreamConnection>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn unsubscribe(&self, _id: String) -> anyhow::Result<()> {
            Ok(())
        }

        async fn watch_events(&self) -> anyhow::Result<()> {
            self.watch_events_called.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn destroy(&self) -> anyhow::Result<()> {
            self.destroy_called.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn update(
            &self,
            _stream_name: &str,
            _group_id: &str,
            _item_id: &str,
            _ops: Vec<UpdateOp>,
        ) -> anyhow::Result<StreamUpdateResult> {
            self.update_result
                .lock()
                .expect("lock update_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }
    }

    fn create_module_with_adapter(adapter: Arc<dyn StreamAdapter>) -> StreamWorker {
        crate::workers::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let config = StreamModuleConfig {
            port: 0,
            host: "127.0.0.1".to_string(),
            auth_function: None,
            adapter: Some(crate::workers::traits::AdapterEntry {
                name: "test-adapter".to_string(),
                config: None,
            }),
        };

        StreamWorker::build(engine, config, adapter)
    }

    #[tokio::test]
    async fn test_stream_module_set_get_delete() {
        let module = create_test_module();
        let stream_name = "test_stream";
        let group_id = "test_group";
        let item_id = "item1";
        let data1 = serde_json::json!({"key": "value1"});
        let data2 = serde_json::json!({"key": "value2"});

        // Subscribe to events
        let (tx, mut rx) = mpsc::unbounded_channel();
        module
            .adapter_snapshot()
            .subscribe(
                "test-subscriber".to_string(),
                Arc::new(RecordingConnection { tx }),
            )
            .await
            .expect("Should subscribe successfully");

        // Start event watcher
        let watcher_adapter = module.adapter_snapshot();
        let watcher = tokio::spawn(async move {
            let _ = watcher_adapter.watch_events().await;
        });
        tokio::task::yield_now().await;

        // Test set (create)
        let set_result = module
            .set(StreamSetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                data: data1.clone(),
                caller_worker_id: None,
            })
            .await;

        assert!(matches!(set_result, FunctionResult::Success(_)));

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for create event")
            .expect("Should receive create event");

        assert!(matches!(msg.event, StreamOutboundMessage::Create { .. }));

        // Test get
        let get_result = module
            .get(StreamGetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                caller_worker_id: None,
            })
            .await;

        match get_result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, data1);
            }
            _ => panic!("Expected successful get with value"),
        }

        // Test set (update)
        let set_result = module
            .set(StreamSetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                data: data2.clone(),
                caller_worker_id: None,
            })
            .await;

        assert!(matches!(set_result, FunctionResult::Success(_)));

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for update event")
            .expect("Should receive update event");

        assert!(matches!(msg.event, StreamOutboundMessage::Update { .. }));

        // Verify updated value
        let get_result = module
            .get(StreamGetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                caller_worker_id: None,
            })
            .await;

        match get_result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, data2);
            }
            _ => panic!("Expected successful get with updated value"),
        }

        // Test delete
        let delete_result = module
            .delete(StreamDeleteInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                caller_worker_id: None,
            })
            .await;

        assert!(matches!(delete_result, FunctionResult::Success(_)));

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for delete event")
            .expect("Should receive delete event");

        assert!(matches!(msg.event, StreamOutboundMessage::Delete { .. }));

        // Verify deleted
        let get_result = module
            .get(StreamGetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                caller_worker_id: None,
            })
            .await;

        match get_result {
            FunctionResult::Success(None) => {
                // Expected - item was deleted
            }
            _ => panic!("Expected None after delete"),
        }

        watcher.abort();
    }

    #[tokio::test]
    async fn test_stream_module_update_existing_record() {
        let module = create_test_module();
        let stream_name = "test_stream";
        let group_id = "test_group";
        let item_id = "item1";
        let initial_data = serde_json::json!({"key": "value1", "count": 5});
        let updated_data = serde_json::json!({"key": "value2", "count": 10});

        // Subscribe to events
        let (tx, mut rx) = mpsc::unbounded_channel();
        module
            .adapter_snapshot()
            .subscribe(
                "test-subscriber".to_string(),
                Arc::new(RecordingConnection { tx }),
            )
            .await
            .expect("Should subscribe successfully");

        // Start event watcher
        let watcher_adapter = module.adapter_snapshot();
        let watcher = tokio::spawn(async move {
            let _ = watcher_adapter.watch_events().await;
        });
        tokio::task::yield_now().await;

        // Create initial record using set
        module
            .set(StreamSetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                data: initial_data.clone(),
                caller_worker_id: None,
            })
            .await;

        // Consume the create event
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for create event");

        // Update existing record
        let update_result = module
            .update(StreamUpdateInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                ops: vec![iii_helpers::stream::UpdateOp::set("", updated_data.clone())],
                caller_worker_id: None,
            })
            .await;

        assert!(matches!(update_result, FunctionResult::Success(_)));

        // Verify Update event was emitted
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for update event")
            .expect("Should receive update event");

        assert!(matches!(msg.event, StreamOutboundMessage::Update { .. }));

        // Verify the value was updated
        let get_result = module
            .get(StreamGetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                caller_worker_id: None,
            })
            .await;

        match get_result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, updated_data);
            }
            _ => panic!("Expected successful get with updated value"),
        }

        watcher.abort();
    }

    #[tokio::test]
    async fn test_stream_module_update_new_record() {
        let module = create_test_module();
        let stream_name = "test_stream";
        let group_id = "test_group";
        let item_id = "new_item";
        let new_data = serde_json::json!({"key": "new_value", "count": 1});

        // Subscribe to events
        let (tx, mut rx) = mpsc::unbounded_channel();
        module
            .adapter_snapshot()
            .subscribe(
                "test-subscriber".to_string(),
                Arc::new(RecordingConnection { tx }),
            )
            .await
            .expect("Should subscribe successfully");

        // Start event watcher
        let watcher_adapter = module.adapter_snapshot();
        let watcher = tokio::spawn(async move {
            let _ = watcher_adapter.watch_events().await;
        });
        tokio::task::yield_now().await;

        // Update non-existent record (should create it)
        let update_result = module
            .update(StreamUpdateInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                ops: vec![iii_helpers::stream::UpdateOp::set("", new_data.clone())],
                caller_worker_id: None,
            })
            .await;

        assert!(matches!(update_result, FunctionResult::Success(_)));

        // Verify Create event was emitted (not Update)
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for create event")
            .expect("Should receive create event");

        assert!(matches!(msg.event, StreamOutboundMessage::Create { .. }));

        // Verify the value was created
        let get_result = module
            .get(StreamGetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                caller_worker_id: None,
            })
            .await;

        match get_result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, new_data);
            }
            _ => panic!("Expected successful get with new value"),
        }

        watcher.abort();
    }

    #[tokio::test]
    async fn test_stream_list_groups_list_all_and_send_with_adapter() {
        let adapter = Arc::new(FakeStreamAdapter::default());
        *adapter
            .get_group_result
            .lock()
            .expect("lock get_group_result") = Ok(vec![
            serde_json::json!({ "item": 1 }),
            serde_json::json!({ "item": 2 }),
        ]);
        *adapter
            .list_groups_result
            .lock()
            .expect("lock list_groups_result") =
            Ok(vec!["group-a".to_string(), "group-b".to_string()]);
        *adapter
            .list_all_result
            .lock()
            .expect("lock list_all_result") = Ok(vec![
            StreamMetadata {
                id: "stream-a".to_string(),
                groups: vec!["group-a".to_string()],
            },
            StreamMetadata {
                id: "stream-b".to_string(),
                groups: vec!["group-b".to_string(), "group-c".to_string()],
            },
        ]);

        let module = create_module_with_adapter(adapter.clone());

        match module
            .list(StreamListInput {
                stream_name: "stream-a".to_string(),
                group_id: "group-a".to_string(),
                caller_worker_id: None,
            })
            .await
        {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, serde_json::json!([{ "item": 1 }, { "item": 2 }]));
            }
            _ => panic!("expected list success"),
        }

        match module
            .list_groups(StreamListGroupsInput {
                stream_name: "stream-a".to_string(),
                caller_worker_id: None,
            })
            .await
        {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, serde_json::json!(["group-a", "group-b"]));
            }
            _ => panic!("expected list_groups success"),
        }

        match module
            .list_all(StreamListAllInput {
                caller_worker_id: None,
            })
            .await
        {
            FunctionResult::Success(result) => {
                assert_eq!(result.count, 2);
                assert_eq!(result.stream.len(), 2);
                assert_eq!(result.stream[0].id, "stream-a");
                assert_eq!(result.stream[0].groups, vec!["group-a"]);
                assert_eq!(result.stream[1].id, "stream-b");
                assert_eq!(result.stream[1].groups, vec!["group-b", "group-c"]);
            }
            _ => panic!("expected list_all success"),
        }

        let send_result = module
            .send(StreamSendInput {
                stream_name: "stream-a".to_string(),
                group_id: "group-a".to_string(),
                id: Some("item-1".to_string()),
                event_type: "custom".to_string(),
                data: serde_json::json!({ "message": "hello" }),
                caller_worker_id: None,
            })
            .await;
        assert!(matches!(send_result, FunctionResult::Success(())));

        let messages = adapter
            .emitted_messages
            .lock()
            .expect("lock emitted_messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].stream_name, "stream-a");
        assert_eq!(messages[0].group_id, "group-a");
        assert_eq!(messages[0].id.as_deref(), Some("item-1"));
        assert!(matches!(
            messages[0].event,
            StreamOutboundMessage::Event {
                event: EventData {
                    ref event_type,
                    ..
                }
            } if event_type == "custom"
        ));
    }

    #[tokio::test]
    async fn test_stream_methods_return_adapter_failures() {
        let adapter = Arc::new(FakeStreamAdapter::default());
        *adapter.set_result.lock().expect("lock set_result") = Err("set failed".to_string());
        *adapter.get_result.lock().expect("lock get_result") = Err("get failed".to_string());
        *adapter.delete_result.lock().expect("lock delete_result") =
            Err("delete failed".to_string());
        *adapter
            .get_group_result
            .lock()
            .expect("lock get_group_result") = Err("group failed".to_string());
        *adapter
            .list_groups_result
            .lock()
            .expect("lock list_groups_result") = Err("list groups failed".to_string());
        *adapter
            .list_all_result
            .lock()
            .expect("lock list_all_result") = Err("list all failed".to_string());
        *adapter
            .emit_event_result
            .lock()
            .expect("lock emit_event_result") = Err("emit failed".to_string());
        *adapter.update_result.lock().expect("lock update_result") =
            Err("update failed".to_string());

        let module = create_module_with_adapter(adapter);

        assert!(matches!(
            module
                .set(StreamSetInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    data: serde_json::json!({ "k": "v" }),
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_SET_ERROR"
        ));
        assert!(matches!(
            module
                .get(StreamGetInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_GET_ERROR"
        ));
        assert!(matches!(
            module
                .delete(StreamDeleteInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_DELETE_ERROR"
        ));
        assert!(matches!(
            module
                .list(StreamListInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_GET_GROUP_ERROR"
        ));
        assert!(matches!(
            module
                .list_groups(StreamListGroupsInput {
                    stream_name: "stream".to_string(),
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_LIST_GROUPS_ERROR"
        ));
        assert!(matches!(
            module.list_all(StreamListAllInput { caller_worker_id: None }).await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_LIST_ALL_ERROR"
        ));
        assert!(matches!(
            module
                .send(StreamSendInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                    id: None,
                    event_type: "boom".to_string(),
                    data: serde_json::json!({}),
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_SEND_ERROR"
        ));
        assert!(matches!(
            module
                .update(StreamUpdateInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    ops: vec![UpdateOp::set("", serde_json::json!({ "count": 1 }))],
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_UPDATE_ERROR"
        ));
    }

    #[tokio::test]
    async fn test_stream_custom_functions_override_adapter() {
        let module = create_test_module();
        let stream_name = "custom_stream";

        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::get({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!({ "from": "custom-get" })))
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::list({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!([{ "from": "custom-list" }])))
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::list_groups({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!(["alpha", "beta"])))
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::set({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!({
                    "old_value": null,
                    "new_value": { "from": "custom-set" }
                })))
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::delete({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!({
                    "old_value": { "from": "custom-delete" }
                })))
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::update({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!({
                    "old_value": { "count": 1 },
                    "new_value": { "count": 2 }
                })))
            }),
        );

        assert!(matches!(
            module
                .get(StreamGetInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Success(Some(value)) if value == serde_json::json!({ "from": "custom-get" })
        ));
        assert!(matches!(
            module
                .list(StreamListInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Success(Some(value)) if value == serde_json::json!([{ "from": "custom-list" }])
        ));
        assert!(matches!(
            module
                .list_groups(StreamListGroupsInput {
                    stream_name: stream_name.to_string(),
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Success(Some(value)) if value == serde_json::json!(["alpha", "beta"])
        ));
        assert!(matches!(
            module
                .set(StreamSetInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    data: serde_json::json!({ "ignored": true }),
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Success(StreamSetResult { new_value, .. }) if new_value == serde_json::json!({ "from": "custom-set" })
        ));
        assert!(matches!(
            module
                .delete(StreamDeleteInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Success(StreamDeleteResult { old_value: Some(value) }) if value == serde_json::json!({ "from": "custom-delete" })
        ));
        assert!(matches!(
            module
                .update(StreamUpdateInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    ops: vec![UpdateOp::set("", serde_json::json!({ "count": 2 }))],
                    caller_worker_id: None,
                })
                .await,
            FunctionResult::Success(StreamUpdateResult { new_value, .. }) if new_value == serde_json::json!({ "count": 2 })
        ));
    }

    /// A stream write is a call of `stream::on-changed`, filtered on the
    /// stream name; unobserved, nothing is built.
    #[tokio::test]
    async fn stream_writes_are_an_engine_hosted_source() {
        let module = create_test_module();
        let engine = module.engine.clone();
        register_stream_event_sources(&engine);
        let set = |stream_name: &str, data: Value| StreamSetInput {
            stream_name: stream_name.to_string(),
            group_id: "group-a".to_string(),
            item_id: "item-1".to_string(),
            data,
            caller_worker_id: None,
        };
        let before = payload_built_count();
        let _ = module
            .set(set("orders", serde_json::json!({ "n": 0 })))
            .await;
        assert_eq!(payload_built_count(), before, "unobserved: nothing built");

        let mut sink = test_support::capture(&engine, "test::on-changed");
        let _socket = test_support::observe(
            &engine,
            "obs-changed",
            STREAM_ON_CHANGED,
            "test::on-changed",
            Some(serde_json::json!({ "input.stream_name": { "$eq": "orders" } })),
        )
        .await;
        let _ = module
            .set(set("invoices", serde_json::json!({ "n": 1 })))
            .await;
        let _ = module
            .set(set("orders", serde_json::json!({ "n": 2 })))
            .await;
        let event = test_support::next(&mut sink).await;
        assert_eq!(
            event["stream_name"], "orders",
            "the filter skipped invoices"
        );
        assert_eq!(event["group_id"], "group-a");
        assert_eq!(event["id"], "item-1");
        assert_eq!(event["event_type"], "update");
        assert_eq!(event["event"]["data"], serde_json::json!({ "n": 2 }));
    }

    fn payload_built_count() -> usize {
        crate::observer::events::payloads_built()
    }

    #[tokio::test]
    async fn test_stream_initialize_registers_its_sources_and_destroy_calls_adapter() {
        let adapter = Arc::new(FakeStreamAdapter::default());
        let module = create_module_with_adapter(adapter.clone());

        module
            .initialize()
            .await
            .expect("stream initialize should work");
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        module
            .start_background_tasks(shutdown_rx, shutdown_tx.clone())
            .await
            .expect("start_background_tasks should succeed");
        // Forget the Sender so it doesn't drop and trigger early shutdown via
        // Receiver::changed() returning Err (all senders gone).
        std::mem::forget(shutdown_tx);
        // Poll for adapter.watch_events_called instead of fixed sleep — under
        // load this otherwise flakes when the watch spawn hasn't scheduled yet.
        // Generous budget (5s) so parallel cargo runs don't trip it.
        for _ in 0..100 {
            tokio::task::yield_now().await;
            if adapter.watch_events_called.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        for source in [
            STREAM_ON_CHANGED,
            crate::observer::events::STREAM_ON_JOIN,
            crate::observer::events::STREAM_ON_LEAVE,
        ] {
            assert!(
                module
                    .engine
                    .functions
                    .get(crate::protocol::DEFAULT_NAMESPACE, source)
                    .is_some(),
                "{source}"
            );
        }
        assert!(adapter.watch_events_called.load(Ordering::SeqCst));

        module.destroy().await.expect("stream destroy should work");
        assert!(adapter.destroy_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn stream_start_background_tasks_returns_addr_in_use_error_with_address() {
        crate::workers::observability::metrics::ensure_default_meter();
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let port = occupied.local_addr().expect("local addr").port();

        let engine = Arc::new(Engine::new());
        let adapter: Arc<dyn StreamAdapter> = Arc::new(FakeStreamAdapter::default());
        let module = StreamWorker::build(
            engine,
            StreamModuleConfig {
                port,
                host: "127.0.0.1".to_string(),
                auth_function: None,
                adapter: Some(crate::workers::traits::AdapterEntry {
                    name: "test-adapter".to_string(),
                    config: None,
                }),
            },
            adapter,
        );

        module
            .initialize()
            .await
            .expect("stream initialize should succeed");

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let err = module
            .start_background_tasks(shutdown_rx, shutdown_tx.clone())
            .await
            .expect_err("stream bind should fail when the port is occupied");
        std::mem::forget(shutdown_tx);

        let message = err.to_string();
        assert!(message.contains(&format!("127.0.0.1:{port}")));
        assert!(message.contains("already in use"));
    }

    #[tokio::test]
    async fn list_preserves_complete_json_array_contract() {
        // The public contract remains an array, including null, numeric and
        // Unicode values and the empty case; allocation behavior is benchmarked.
        let adapter = Arc::new(FakeStreamAdapter::default());
        let expected = vec![
            Value::Null,
            serde_json::json!({"text":"中文\n\\\"","n":18446744073709551615u64}),
            serde_json::json!([true, -7, 1.25]),
        ];
        *adapter.get_group_result.lock().unwrap() = Ok(expected.clone());
        let worker = create_module_with_adapter(adapter.clone());
        match worker
            .list(StreamListInput {
                stream_name: "s".into(),
                group_id: "g".into(),
                caller_worker_id: None,
            })
            .await
        {
            FunctionResult::Success(Some(Value::Array(values))) => assert_eq!(values, expected),
            _ => panic!("legacy list must remain a complete JSON array"),
        }
        *adapter.get_group_result.lock().unwrap() = Ok(Vec::new());
        assert!(
            matches!(worker.list(StreamListInput {stream_name:"s".into(),group_id:"g".into(),caller_worker_id: None}).await,
            FunctionResult::Success(Some(Value::Array(values))) if values.is_empty())
        );
    }

    /// `stream::set` called the way a worker calls it (engine-injected
    /// `_caller_worker_id`) still succeeds, forwards the exact pre-change
    /// payload to a custom `stream::set(<name>)` function, and logs one
    /// rate-limited deprecation warning carrying the caller but no payload.
    #[tokio::test]
    async fn test_set_with_caller_worker_id_forwards_unchanged_payload_and_warns_once() {
        let _lock = crate::deprecation::test_support::GLOBAL_LIMITER_LOCK
            .lock()
            .await;

        let module = create_test_module();
        module.register_functions(module.engine.clone());
        let stream_name = "caller_payload_stream";
        let received = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let received_clone = received.clone();
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::set({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |input: Value| {
                let received = received_clone.clone();
                async move {
                    received.lock().unwrap().push(input);
                    FunctionResult::Success(Some(serde_json::json!({
                        "old_value": null,
                        "new_value": { "secret": "payload-value-123" }
                    })))
                }
            }),
        );

        let caller = uuid::Uuid::new_v4().to_string();
        let call_input = serde_json::json!({
            "stream_name": stream_name,
            "group_id": "group-xyz",
            "item_id": "item-xyz",
            "data": { "secret": "payload-value-123" },
            "_caller_worker_id": caller,
        });
        for _ in 0..2 {
            let value = module
                .engine
                .call("stream::set", call_input.clone())
                .await
                .expect("stream::set must still succeed")
                .expect("stream::set returns its result");
            assert_eq!(
                value,
                serde_json::json!({
                    "old_value": null,
                    "new_value": { "secret": "payload-value-123" }
                })
            );
        }

        let expected = serde_json::json!({
            "stream_name": stream_name,
            "group_id": "group-xyz",
            "item_id": "item-xyz",
            "data": { "secret": "payload-value-123" },
        });
        let received = received.lock().unwrap().clone();
        assert_eq!(received, vec![expected.clone(), expected.clone()]);
        assert_eq!(
            serde_json::to_string(&received[0]).unwrap(),
            serde_json::to_string(&expected).unwrap(),
            "forwarded payload must be byte-identical"
        );

        let warnings = crate::deprecation::test_support::emitted_for(&caller);
        assert_eq!(
            warnings.len(),
            1,
            "expected one rate-limited warning for two calls: {warnings:?}"
        );
        assert_eq!(warnings[0].entry_point, "stream::set");
        let rendered = format!("{:?}", warnings[0]);
        for secret in ["payload-value-123", "group-xyz", "item-xyz", stream_name] {
            assert!(
                !rendered.contains(secret),
                "warning leaked {secret}: {rendered}"
            );
        }
    }
}
