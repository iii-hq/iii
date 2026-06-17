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
use iii_sdk::{
    UpdateResult,
    types::{DeleteResult, SetResult},
};
use once_cell::sync::Lazy;
use serde_json::Value;
use tokio::{net::TcpListener, task::AbortHandle};
use tracing::Instrument;

use crate::{
    condition::check_condition,
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    protocol::ErrorBody,
    trigger::TriggerType,
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
            trigger::{
                JOIN_TRIGGER_TYPE, LEAVE_TRIGGER_TYPE, STREAM_TRIGGER_TYPE, StreamTrigger,
                StreamTriggers,
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
    engine: Arc<Engine>,
    /// Live config + adapter, swapped atomically by `apply_config`. Readers
    /// clone the inner `Arc`s via `config_snapshot`/`adapter_snapshot`.
    live: Arc<SyncRwLock<LiveState>>,
    /// The config.yaml block; used only for `register_config`'s initial value
    /// and the boot bind/adapter fallback.
    seed: Option<StreamModuleConfig>,

    pub triggers: Arc<StreamTriggers>,
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

async fn ws_handler(
    State(module): State<Arc<StreamSocketManager>>,
    ws: WebSocketUpgrade,
    uri: Uri,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let module = module.clone();

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
        self.register_config_handler(&engine);
        self.register_functions(engine);
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying StreamWorker");
        // Best-effort: the trigger is registered outside the worker scope, so
        // remove it explicitly to keep ReloadManager restarts duplicate-free.
        let _ = self
            .engine
            .trigger_registry
            .unregister_trigger(
                super::configuration::CONFIG_TRIGGER_ID.to_string(),
                Some(super::configuration::CONFIG_TRIGGER_TYPE.to_string()),
            )
            .await;

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

        let _ = self
            .engine
            .register_trigger_type(TriggerType::new(
                JOIN_TRIGGER_TYPE,
                "Stream join trigger",
                Box::new(self.clone()),
                None,
            ))
            .await;

        let _ = self
            .engine
            .register_trigger_type(TriggerType::new(
                LEAVE_TRIGGER_TYPE,
                "Stream leave trigger",
                Box::new(self.clone()),
                None,
            ))
            .await;

        let _ = self
            .engine
            .register_trigger_type(TriggerType::new(
                STREAM_TRIGGER_TYPE,
                "Stream trigger",
                Box::new(self.clone()),
                None,
            ))
            .await;

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
            match Self::resolve_adapter(&self.engine, &served_config).await {
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
        // on-config-change cannot act during (or instead of) the boot sequence.
        *self
            .shutdown_rx
            .lock()
            .expect("stream shutdown_rx mutex poisoned") = Some(shutdown_rx);

        // Register the handler before the trigger so an event can never fan out
        // to a missing function. The `get` check keeps the initial-boot path
        // (where `register_functions` already ran it) from logging a spurious
        // overwrite.
        if self
            .engine
            .functions
            .get(super::configuration::CONFIG_FN_ID)
            .is_none()
        {
            self.register_config_handler(&self.engine);
        }
        if let Err(err) = super::configuration::register_config_trigger(&self.engine).await {
            tracing::warn!(
                error = %err,
                "iii-stream: failed to watch configuration changes; hot-reload disabled"
            );
        } else {
            // Catch-up pass: replay any `configuration::set` that landed between
            // the boot fetch above and the trigger subscription. Routed through
            // `on_config_change` so a timed-out catch-up gets the one-shot retry.
            super::configuration::on_config_change(self).await;
        }

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
            triggers: Arc::new(StreamTriggers::new()),
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

    /// Register the `iii-stream::on-config-change` handler (idempotent by id).
    fn register_config_handler(&self, engine: &Arc<Engine>) {
        let worker = self.clone();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: super::configuration::CONFIG_FN_ID.to_string(),
                description: Some("Apply iii-stream configuration changes".to_string()),
                request_format: None,
                response_format: None,
                metadata: Some(serde_json::json!({ "internal": true })),
            },
            Handler::new(move |_input: Value| {
                let worker = worker.clone();
                async move {
                    super::configuration::on_config_change(&worker).await;
                    FunctionResult::Success(Some(serde_json::json!({})))
                }
            }),
        );
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
            self.triggers.clone(),
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
        // failure leaves the previous config/adapter/server untouched.
        let new_adapter = if adapter_changed {
            Some(Self::resolve_adapter(&self.engine, &new_config).await?)
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
    /// Invoke triggers for a given event type with condition checks
    async fn invoke_triggers(&self, event_data: StreamWrapperMessage) {
        let engine = self.engine.clone();
        let event_stream_name = event_data.stream_name.clone();

        // Collect relevant trigger IDs and clone the triggers we need
        // Only triggers with matching stream_name are registered, so we only need to look up by stream_name
        let triggers_to_invoke: Vec<StreamTrigger> = {
            let by_name = self.triggers.stream_triggers_by_name.read().await;
            let triggers_map = self.triggers.stream_triggers.read().await;
            let mut triggers = Vec::new();

            // Get triggers for this specific stream_name
            if let Some(ids_for_stream) = by_name.get(&event_stream_name) {
                for trigger_id in ids_for_stream {
                    if let Some(trigger) = triggers_map.get(trigger_id) {
                        let group_id = trigger.config.group_id.clone().unwrap_or("".to_string());
                        let item_id = trigger.config.item_id.clone().unwrap_or("".to_string());
                        let event_item_id = event_data.id.clone().unwrap_or("".to_string());

                        if (!group_id.is_empty() && group_id != event_data.group_id)
                            || (!item_id.is_empty() && item_id != event_item_id)
                        {
                            continue;
                        }

                        triggers.push(trigger.clone());
                    }
                }
            }

            triggers
        };

        // No trigger matches this stream write (name index already filtered by
        // stream_name/group_id/item_id) → skip the `stream_triggers` eval span
        // and the spawn entirely. The engine fires one evaluation per stream
        // write, so this avoids the span/CPU/export for the common no-match case.
        if triggers_to_invoke.is_empty() {
            return;
        }

        // The engine attaches the writer's OTel context for the stream write
        // (even for suppressed builtins — see invocation::handle_invocation), so
        // parent the spawned trigger fan-out to it instead of orphaning
        // `stream_triggers` into a brand-new, disconnected trace.
        let parent_cx = opentelemetry::Context::current();

        if let Ok(event_data) = serde_json::to_value(event_data) {
            let trigger_span = {
                let _guard = parent_cx.attach();
                tracing::info_span!(
                    "stream_triggers",
                    "iii.function.kind" = "internal",
                    otel.status_code = tracing::field::Empty
                )
            };
            tokio::spawn(
                async move {
                    let mut has_error = false;

                    for stream_trigger in triggers_to_invoke {
                        let trigger = &stream_trigger.trigger;

                        // Check condition if specified (using pre-parsed value)
                        let condition_function_id =
                            stream_trigger.config.condition_function_id.clone();

                        if let Some(ref condition_id) = condition_function_id {
                            tracing::debug!(
                                condition_function_id = %condition_id,
                                "Checking trigger conditions"
                            );
                            match check_condition(engine.as_ref(), condition_id, event_data.clone())
                                .await
                            {
                                Ok(true) => {}
                                Ok(false) => {
                                    tracing::debug!(
                                        function_id = %trigger.function_id,
                                        "Condition check failed, skipping handler"
                                    );
                                    continue;
                                }
                                Err(err) => {
                                    tracing::error!(
                                        condition_function_id = %condition_id,
                                        error = ?err,
                                        "Error invoking condition function"
                                    );
                                    has_error = true;
                                    continue;
                                }
                            }
                        }

                        // Invoke the handler function
                        tracing::debug!(
                            function_id = %trigger.function_id,
                            "Invoking trigger"
                        );

                        let call_result =
                            engine.call(&trigger.function_id, event_data.clone()).await;

                        match call_result {
                            Ok(_) => {
                                tracing::debug!(
                                    function_id = %trigger.function_id,
                                    "Trigger handler invoked successfully"
                                );
                            }
                            Err(err) => {
                                has_error = true;
                                tracing::error!(
                                    function_id = %trigger.function_id,
                                    error = ?err,
                                    "Error invoking trigger handler"
                                );
                            }
                        }
                    }

                    if has_error {
                        tracing::Span::current().record("otel.status_code", "ERROR");
                    } else {
                        tracing::Span::current().record("otel.status_code", "OK");
                    }
                }
                .instrument(trigger_span),
            );
        } else {
            tracing::error!("Failed to convert event data to value");
        }
    }
}

#[service(name = "stream")]
impl StreamWorker {
    #[function(id = "stream::set", description = "Set a value in a stream")]
    pub async fn set(&self, input: StreamSetInput) -> FunctionResult<SetResult, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;
        let data = input.data;

        let function_id = format!("stream::set({})", stream_name);
        let function = self.engine.functions.get(&function_id);
        let adapter = self.adapter_snapshot();

        let result: anyhow::Result<SetResult> = match function {
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
                    Ok(Some(result)) => match serde_json::from_value::<SetResult>(result) {
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

                self.invoke_triggers(message.clone()).await;

                if let Err(e) = adapter.emit_event(message).await {
                    tracing::error!(error = %e, "Failed to emit event");
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
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;

        let function_id = format!("stream::get({})", stream_name);
        let function = self.engine.functions.get(&function_id);
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
    ) -> FunctionResult<DeleteResult, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;
        let function_id = format!("stream::delete({})", stream_name);
        let function = self.engine.functions.get(&function_id);
        let adapter = self.adapter_snapshot();

        let result: anyhow::Result<DeleteResult> = match function {
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
                        let result = match serde_json::from_value::<DeleteResult>(result) {
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

                    self.invoke_triggers(message.clone()).await;

                    if let Err(e) = adapter.emit_event(message).await {
                        tracing::error!(error = %e, "Failed to emit delete event");
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
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;

        let function_id = format!("stream::list({})", stream_name);
        let function = self.engine.functions.get(&function_id);
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
                Ok(values) => FunctionResult::Success(serde_json::to_value(values).ok()),
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
        let cloned_input = input.clone();
        let stream_name = input.stream_name;

        let function_id = format!("stream::list_groups({})", stream_name);
        let function = self.engine.functions.get(&function_id);
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
        _input: StreamListAllInput,
    ) -> FunctionResult<StreamListAllResult, ErrorBody> {
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

        self.invoke_triggers(message.clone()).await;

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
    ) -> FunctionResult<UpdateResult, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;
        let ops = input.ops;

        tracing::debug!(stream_name = %stream_name, group_id = %group_id, item_id = %item_id, ops_count = ops.len(), "Executing atomic stream update");

        let function_id = format!("stream::update({})", stream_name);
        let function = self.engine.functions.get(&function_id);
        let adapter = self.adapter_snapshot();

        let result: anyhow::Result<UpdateResult> = match function {
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
                    Ok(Some(result)) => match serde_json::from_value::<UpdateResult>(result) {
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

                self.invoke_triggers(message.clone()).await;

                if let Err(e) = adapter.emit_event(message).await {
                    tracing::error!(error = %e, "Failed to emit event");
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
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use iii_sdk::{
        UpdateOp, UpdateResult,
        types::{DeleteResult, SetResult},
    };
    use serde_json::Value;
    use tokio::sync::mpsc;

    use crate::{
        builtins::pubsub_lite::Subscriber,
        engine::{Engine, EngineTrait},
        protocol::ErrorBody,
        trigger::Trigger,
        workers::stream::{
            StreamMetadata,
            adapters::{StreamAdapter, StreamConnection},
            config::StreamModuleConfig,
            trigger::StreamTriggerConfig,
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
        set_result: Mutex<Result<SetResult, String>>,
        get_result: Mutex<Result<Option<Value>, String>>,
        delete_result: Mutex<Result<DeleteResult, String>>,
        get_group_result: Mutex<Result<Vec<Value>, String>>,
        list_groups_result: Mutex<Result<Vec<String>, String>>,
        list_all_result: Mutex<Result<Vec<StreamMetadata>, String>>,
        emit_event_result: Mutex<Result<(), String>>,
        update_result: Mutex<Result<UpdateResult, String>>,
        emitted_messages: Mutex<Vec<StreamWrapperMessage>>,
        destroy_called: AtomicBool,
        watch_events_called: AtomicBool,
    }

    impl Default for FakeStreamAdapter {
        fn default() -> Self {
            Self {
                set_result: Mutex::new(Ok(SetResult {
                    old_value: None,
                    new_value: serde_json::json!({}),
                })),
                get_result: Mutex::new(Ok(None)),
                delete_result: Mutex::new(Ok(DeleteResult { old_value: None })),
                get_group_result: Mutex::new(Ok(Vec::new())),
                list_groups_result: Mutex::new(Ok(Vec::new())),
                list_all_result: Mutex::new(Ok(Vec::new())),
                emit_event_result: Mutex::new(Ok(())),
                update_result: Mutex::new(Ok(UpdateResult {
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
        ) -> anyhow::Result<SetResult> {
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
        ) -> anyhow::Result<DeleteResult> {
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
        ) -> anyhow::Result<UpdateResult> {
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
                ops: vec![iii_sdk::UpdateOp::set("", updated_data.clone())],
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
                ops: vec![iii_sdk::UpdateOp::set("", new_data.clone())],
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
            })
            .await
        {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, serde_json::json!(["group-a", "group-b"]));
            }
            _ => panic!("expected list_groups success"),
        }

        match module.list_all(StreamListAllInput {}).await {
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
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_DELETE_ERROR"
        ));
        assert!(matches!(
            module
                .list(StreamListInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_GET_GROUP_ERROR"
        ));
        assert!(matches!(
            module
                .list_groups(StreamListGroupsInput {
                    stream_name: "stream".to_string(),
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_LIST_GROUPS_ERROR"
        ));
        assert!(matches!(
            module.list_all(StreamListAllInput {}).await,
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
                })
                .await,
            FunctionResult::Success(Some(value)) if value == serde_json::json!({ "from": "custom-get" })
        ));
        assert!(matches!(
            module
                .list(StreamListInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                })
                .await,
            FunctionResult::Success(Some(value)) if value == serde_json::json!([{ "from": "custom-list" }])
        ));
        assert!(matches!(
            module
                .list_groups(StreamListGroupsInput {
                    stream_name: stream_name.to_string(),
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
                })
                .await,
            FunctionResult::Success(SetResult { new_value, .. }) if new_value == serde_json::json!({ "from": "custom-set" })
        ));
        assert!(matches!(
            module
                .delete(StreamDeleteInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                })
                .await,
            FunctionResult::Success(DeleteResult { old_value: Some(value) }) if value == serde_json::json!({ "from": "custom-delete" })
        ));
        assert!(matches!(
            module
                .update(StreamUpdateInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    ops: vec![UpdateOp::set("", serde_json::json!({ "count": 2 }))],
                })
                .await,
            FunctionResult::Success(UpdateResult { new_value, .. }) if new_value == serde_json::json!({ "count": 2 })
        ));
    }

    #[tokio::test]
    async fn test_stream_invoke_triggers_covers_conditions_and_errors() {
        let module = create_test_module();
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let handler_calls_clone = handler_calls.clone();

        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::stream_handler".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |_input| {
                let handler_calls_clone = handler_calls_clone.clone();
                async move {
                    handler_calls_clone.fetch_add(1, Ordering::SeqCst);
                    FunctionResult::Success(None)
                }
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::stream_handler_fail".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Failure(ErrorBody {
                    code: "HANDLER".to_string(),
                    message: "handler failed".to_string(),
                    stacktrace: None,
                })
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::cond_false".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!(false)))
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::cond_none".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move { FunctionResult::Success(None) }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::cond_error".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Failure(ErrorBody {
                    code: "COND".to_string(),
                    message: "condition failed".to_string(),
                    stacktrace: None,
                })
            }),
        );

        let mut stream_triggers = module.triggers.stream_triggers.write().await;
        stream_triggers.insert(
            "ok".to_string(),
            StreamTrigger {
                trigger: Trigger {
                    id: "ok".to_string(),
                    trigger_type: STREAM_TRIGGER_TYPE.to_string(),
                    function_id: "test::stream_handler".to_string(),
                    config: serde_json::json!({}),
                    worker_id: None,
                    metadata: None,
                },
                config: StreamTriggerConfig {
                    stream_name: Some("events".to_string()),
                    group_id: None,
                    item_id: None,
                    condition_function_id: None,
                },
            },
        );
        stream_triggers.insert(
            "skip-false".to_string(),
            StreamTrigger {
                trigger: Trigger {
                    id: "skip-false".to_string(),
                    trigger_type: STREAM_TRIGGER_TYPE.to_string(),
                    function_id: "test::stream_handler".to_string(),
                    config: serde_json::json!({}),
                    worker_id: None,
                    metadata: None,
                },
                config: StreamTriggerConfig {
                    stream_name: Some("events".to_string()),
                    group_id: None,
                    item_id: None,
                    condition_function_id: Some("test::cond_false".to_string()),
                },
            },
        );
        stream_triggers.insert(
            "skip-none".to_string(),
            StreamTrigger {
                trigger: Trigger {
                    id: "skip-none".to_string(),
                    trigger_type: STREAM_TRIGGER_TYPE.to_string(),
                    function_id: "test::stream_handler".to_string(),
                    config: serde_json::json!({}),
                    worker_id: None,
                    metadata: None,
                },
                config: StreamTriggerConfig {
                    stream_name: Some("events".to_string()),
                    group_id: None,
                    item_id: None,
                    condition_function_id: Some("test::cond_none".to_string()),
                },
            },
        );
        stream_triggers.insert(
            "skip-error".to_string(),
            StreamTrigger {
                trigger: Trigger {
                    id: "skip-error".to_string(),
                    trigger_type: STREAM_TRIGGER_TYPE.to_string(),
                    function_id: "test::stream_handler".to_string(),
                    config: serde_json::json!({}),
                    worker_id: None,
                    metadata: None,
                },
                config: StreamTriggerConfig {
                    stream_name: Some("events".to_string()),
                    group_id: None,
                    item_id: None,
                    condition_function_id: Some("test::cond_error".to_string()),
                },
            },
        );
        stream_triggers.insert(
            "handler-error".to_string(),
            StreamTrigger {
                trigger: Trigger {
                    id: "handler-error".to_string(),
                    trigger_type: STREAM_TRIGGER_TYPE.to_string(),
                    function_id: "test::stream_handler_fail".to_string(),
                    config: serde_json::json!({}),
                    worker_id: None,
                    metadata: None,
                },
                config: StreamTriggerConfig {
                    stream_name: Some("events".to_string()),
                    group_id: None,
                    item_id: None,
                    condition_function_id: None,
                },
            },
        );
        drop(stream_triggers);

        module
            .triggers
            .stream_triggers_by_name
            .write()
            .await
            .insert(
                "events".to_string(),
                vec![
                    "ok".to_string(),
                    "skip-false".to_string(),
                    "skip-none".to_string(),
                    "skip-error".to_string(),
                    "handler-error".to_string(),
                ],
            );

        module
            .invoke_triggers(StreamWrapperMessage {
                event_type: "stream".to_string(),
                timestamp: Utc::now().timestamp_millis(),
                stream_name: "events".to_string(),
                group_id: "group".to_string(),
                id: Some("item-1".to_string()),
                event: StreamOutboundMessage::Create {
                    data: serde_json::json!({ "x": 1 }),
                },
            })
            .await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // Expected call count: 2
        // - "ok":          no condition, handler called           -> +1
        // - "skip-false":  condition returns false                -> skipped
        // - "skip-none":   condition returns None => Ok(true)     -> +1 (check_condition treats None as pass)
        // - "skip-error":  condition returns error                -> skipped
        // - "handler-error": no condition, handler fails (no add) -> +0
        assert_eq!(handler_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_stream_initialize_registers_trigger_types_and_destroy_calls_adapter() {
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
        assert!(
            module
                .engine
                .trigger_registry
                .trigger_types
                .contains_key(JOIN_TRIGGER_TYPE)
        );
        assert!(
            module
                .engine
                .trigger_registry
                .trigger_types
                .contains_key(LEAVE_TRIGGER_TYPE)
        );
        assert!(
            module
                .engine
                .trigger_registry
                .trigger_types
                .contains_key(STREAM_TRIGGER_TYPE)
        );
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
}
