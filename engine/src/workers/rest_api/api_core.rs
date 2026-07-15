// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{
    pin::Pin,
    sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock},
};

use anyhow::anyhow;
use axum::{
    Router,
    http::{Method, StatusCode},
};
use colored::Colorize;
use dashmap::DashMap;
use futures::Future;
use serde_json::Value;
use tokio::{
    net::TcpListener,
    sync::{RwLock, oneshot},
    task::AbortHandle,
};
use tower::limit::ConcurrencyLimitLayer;
use tower_http::{
    cors::{Any as HTTP_Any, CorsLayer},
    timeout::TimeoutLayer,
};

use super::{
    config::RestApiConfig,
    hot_router::{HotRouter, MakeHotRouterService},
    views::dynamic_handler,
};
use crate::{
    engine::{Engine, EngineTrait},
    trigger::{Trigger, TriggerRegistrator, TriggerType},
    workers::traits::Worker,
};

#[derive(Debug)]
pub struct PathRouter {
    pub http_path: String,
    pub http_method: String,
    pub function_id: String,
    pub condition_function_id: Option<String>,
    pub middleware_function_ids: Vec<String>,
    pub trigger_id: String,
    pub worker_id: Option<uuid::Uuid>,
    /// Per-trigger metadata, delivered to the handler as a distinct argument
    /// when the route fires (not folded into the request payload).
    pub metadata: Option<Value>,
}

impl PathRouter {
    pub fn new(
        http_path: String,
        http_method: String,
        function_id: String,
        condition_function_id: Option<String>,
        middleware_function_ids: Vec<String>,
    ) -> Self {
        Self {
            http_path,
            http_method,
            function_id,
            condition_function_id,
            middleware_function_ids,
            trigger_id: String::new(),
            worker_id: None,
            metadata: None,
        }
    }

    /// Set the trigger/worker that owns this route.
    pub fn with_owner(mut self, trigger_id: String, worker_id: Option<uuid::Uuid>) -> Self {
        self.trigger_id = trigger_id;
        self.worker_id = worker_id;
        self
    }

    /// Attach per-trigger metadata, delivered to the handler as a distinct
    /// argument when the route fires.
    pub fn with_metadata(mut self, metadata: Option<Value>) -> Self {
        self.metadata = metadata;
        self
    }
}

#[derive(Debug, Clone)]
pub struct RouterMatch {
    pub function_id: String,
    pub condition_function_id: Option<String>,
    pub middleware_function_ids: Vec<String>,
    pub metadata: Option<Value>,
}

#[derive(Clone)]
pub struct HttpWorker {
    engine: Arc<Engine>,
    /// Live config snapshot. The outer lock is held only for pointer swaps;
    /// readers clone the inner `Arc` once per request/function via
    /// `config_snapshot()`, which is what makes the value hot-swappable from
    /// the configuration worker without touching the request hot path.
    config: Arc<StdRwLock<Arc<RestApiConfig>>>,
    /// The config.yaml block passed to `create()`, if any. Used only as the
    /// seed for first-time `configuration::register`; the configuration
    /// worker entry is the runtime source of truth afterwards.
    seed: Option<RestApiConfig>,
    pub routers_registry: Arc<DashMap<String, PathRouter>>,
    shared_routers: Arc<RwLock<Router>>,
    server_abort: Arc<StdMutex<Option<AbortHandle>>>,
    /// Per-server graceful-shutdown trigger for the CURRENT server. Firing it
    /// gracefully shuts that one server down — closing its listener AND its open
    /// connections (including idle keep-alive connections a client would
    /// otherwise reuse to keep hitting the old address). A rebind fires the
    /// previous server's trigger so the old port is fully freed, not just
    /// stopped from accepting new connections.
    server_shutdown: Arc<StdMutex<Option<oneshot::Sender<()>>>>,
    /// Kept so `apply_config` can respawn the server task on host/port change.
    shutdown_rx: Arc<StdMutex<Option<tokio::sync::watch::Receiver<bool>>>>,
    /// Serializes concurrent `apply_config` runs (rapid configuration edits).
    apply_lock: Arc<tokio::sync::Mutex<()>>,
}

#[async_trait::async_trait]
impl Worker for HttpWorker {
    fn name(&self) -> &'static str {
        "HttpWorker"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        Ok(Box::new(Self::from_config(engine, config)?))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        // Registered here so the worker scope tracks the handler and removes it
        // automatically on destroy/reload. The hook order differs by pipeline:
        // initial boot runs `register_functions` BEFORE `start_background_tasks`
        // (workers/config.rs), reload runs it AFTER (reload.rs) — so
        // `start_background_tasks` also registers the handler (if absent)
        // before subscribing to configuration events.
        self.register_config_handler(&engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!(
            "Initializing API adapter on port {}",
            self.config_snapshot().port
        );

        self.engine
            .clone()
            .register_trigger_type(TriggerType::new(
                "http",
                "HTTP API trigger",
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
        // Adopt the configuration worker as the runtime source of truth.
        // `configuration::*` is callable here on both pipelines (initial boot
        // registers all worker functions before serving; reload starts the
        // mandatory configuration worker before optional ones). Failures
        // degrade to the static config.yaml block so HTTP stays up. Both bus
        // calls are time-bounded: worker startup is awaited serially by the
        // boot and reload pipelines, so a hung `configuration::*` provider
        // must not wedge every other worker behind this one.
        let register = tokio::time::timeout(
            super::configuration::CONFIG_BUS_TIMEOUT,
            super::configuration::register_config(self.engine.as_ref(), self.seed.as_ref()),
        )
        .await
        .map_err(|_| anyhow!("configuration::register timed out"))
        .and_then(|result| result);
        if let Err(err) = register {
            tracing::warn!(
                error = %err,
                "iii-http: configuration::register failed; continuing with static config"
            );
        }
        let fetched = tokio::time::timeout(
            super::configuration::CONFIG_BUS_TIMEOUT,
            super::configuration::fetch_config(self.engine.as_ref(), &self.config_snapshot()),
        )
        .await
        .map_err(|_| anyhow!("configuration::get timed out"))
        .and_then(|result| result);
        match fetched {
            Ok(config) => self.set_config(config),
            Err(err) => tracing::warn!(
                error = %err,
                "iii-http: failed to read configuration; continuing with static config"
            ),
        }

        // Bind after the fetch so a runtime-edited host/port survives restarts.
        // If the stored address cannot be bound (e.g. a bad host/port was
        // persisted by a runtime edit whose rebind failed), fall back to the
        // seed address instead of failing the whole worker start — otherwise
        // one bad configuration edit turns into an HTTP outage on the next
        // engine restart. The fallback is GUARDED: it requires an explicit
        // config.yaml seed (never the built-in 0.0.0.0 default), and it must
        // not widen a loopback-restricted stored host to a non-loopback
        // interface — an occupied local port must not be able to force the
        // API onto every interface.
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
                            "iii-http: refusing seed fallback that would widen a \
                             loopback-only address to a non-loopback interface"
                        );
                    }
                    return Err(crate::workers::traits::bind_address_error(&addr, err));
                }
                tracing::error!(
                    error = %err,
                    stored = %addr,
                    fallback = %fallback_addr,
                    "iii-http: stored configuration address cannot be bound; serving on the \
                     seed address — fix the stored `iii-http` configuration value"
                );
                self.set_config(fallback);
                TcpListener::bind(&fallback_addr).await.map_err(|err| {
                    crate::workers::traits::bind_address_error(&fallback_addr, err)
                })?
            }
        };
        let addr = {
            let config = self.config_snapshot();
            format!("{}:{}", config.host, config.port)
        };

        // Build initial router from registry
        self.update_routes().await?;

        let (handle, graceful_tx) = self.spawn_server(listener, addr, shutdown_rx.clone());
        *self
            .server_abort
            .lock()
            .expect("server_abort mutex poisoned") = Some(handle);
        *self
            .server_shutdown
            .lock()
            .expect("server_shutdown mutex poisoned") = Some(graceful_tx);

        // Store the receiver only once the server is live: `apply_config`
        // refuses to rebind while this is `None`, so a stray
        // `iii-http::on-config-change` invocation cannot spawn a server during
        // (or instead of) the boot sequence above.
        *self.shutdown_rx.lock().expect("shutdown_rx mutex poisoned") = Some(shutdown_rx);

        // Register the handler before the trigger so a configuration event can
        // never fan out to a missing function. On reload, `register_functions`
        // runs after this hook and re-registers the handler inside the worker
        // scope; the `get` check keeps the initial-boot path (where it already
        // ran) from logging a spurious "already registered" overwrite.
        if self
            .engine
            .functions
            .get(
                crate::protocol::DEFAULT_NAMESPACE,
                super::configuration::CONFIG_FN_ID,
            )
            .is_none()
        {
            self.register_config_handler(&self.engine);
        }
        if let Err(err) = super::configuration::register_config_trigger(&self.engine).await {
            tracing::warn!(
                error = %err,
                "iii-http: failed to watch configuration changes; hot-reload disabled"
            );
        } else {
            // Catch-up pass: replay any `configuration::set` that landed
            // between the boot fetch above and the trigger subscription —
            // without it that window's updates would be dropped until the
            // next edit. Routed through `on_config_change` so a timed-out
            // catch-up (slow configuration backend during boot) gets the
            // same one-shot delayed retry as a trigger-driven apply instead
            // of leaving the boot config stale until the next edit.
            super::configuration::on_config_change(self).await;
        }

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
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

        // Serialize with any in-flight `apply_config` so a rebind can't spawn
        // a replacement server after we abort below. Clearing `shutdown_rx`
        // makes any later apply refuse the rebind path entirely.
        let _guard = self.apply_lock.lock().await;
        self.shutdown_rx
            .lock()
            .expect("shutdown_rx mutex poisoned")
            .take();

        // Gracefully close the server first (drops the listener and its open
        // connections, idle keep-alive included), then hard-abort the task as
        // an immediate backstop — `destroy` is a stop-now path.
        let graceful = self
            .server_shutdown
            .lock()
            .expect("server_shutdown mutex poisoned")
            .take();
        if let Some(graceful) = graceful {
            let _ = graceful.send(());
        }
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

const ALLOW_ORIGIN_ANY: &str = "*";

/// After a host/port change the old server is gracefully shut down (which closes
/// its listener and open connections). This is how long we then wait before
/// hard-aborting the old server task as a safety net, so a connection that
/// refuses to drain cannot pin the old port indefinitely.
const OLD_SERVER_HARD_STOP_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

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

impl HttpWorker {
    fn from_config(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Self> {
        let seed: Option<RestApiConfig> = config
            .map(serde_json::from_value)
            .transpose()?
            .map(RestApiConfig::normalized);
        let config = seed.clone().unwrap_or_default();

        Ok(Self {
            engine,
            config: Arc::new(StdRwLock::new(Arc::new(config))),
            seed,
            routers_registry: Arc::new(DashMap::new()),
            // Empty router initially; updated when routes are registered.
            shared_routers: Arc::new(RwLock::new(Router::new())),
            server_abort: Arc::new(StdMutex::new(None)),
            server_shutdown: Arc::new(StdMutex::new(None)),
            shutdown_rx: Arc::new(StdMutex::new(None)),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    /// Construct a worker from a raw config value — mirrors
    /// `ConfigurationWorker::for_test` so integration tests in `engine/tests/`
    /// can drive the concrete worker without booting the full engine.
    #[doc(hidden)]
    pub fn for_test(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Self> {
        Self::from_config(engine, config)
    }

    #[cfg(test)]
    pub(crate) fn new_for_tests(engine: Arc<Engine>, config: RestApiConfig) -> Self {
        Self::from_config(
            engine,
            Some(serde_json::to_value(config).expect("RestApiConfig serializes")),
        )
        .expect("test config deserializes")
    }

    /// Register the `iii-http::on-config-change` handler. Idempotent
    /// (replace-by-id), so it is safe to call from both `register_functions`
    /// (which runs inside the worker scope for destroy/reload cleanup) and
    /// `start_background_tasks` (which registers the trigger and runs first).
    fn register_config_handler(&self, engine: &Arc<Engine>) {
        let worker = self.clone();
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: super::configuration::CONFIG_FN_ID.to_string(),
                description: Some(
                    "Internal: re-apply the iii-http server configuration when the \
                     authoritative configuration entry changes."
                        .to_string(),
                ),
                request_format: None,
                response_format: None,
                metadata: Some(serde_json::json!({ "internal": true })),
            },
            crate::engine::Handler::new(move |_payload: Value| {
                let worker = worker.clone();
                async move {
                    super::configuration::on_config_change(&worker).await;
                    crate::function::FunctionResult::Success(Some(
                        serde_json::json!({ "ok": true }),
                    ))
                }
            }),
        );
    }

    /// Cheap clone of the live config. Take one snapshot per request/function
    /// so all reads within it are consistent.
    pub fn config_snapshot(&self) -> Arc<RestApiConfig> {
        self.config.read().expect("config lock poisoned").clone()
    }

    fn set_config(&self, config: RestApiConfig) {
        *self.config.write().expect("config lock poisoned") = Arc::new(config);
    }

    /// Spawn the axum server task for an already-bound listener. Returns its
    /// abort handle plus a per-server graceful-shutdown trigger. Shared by the
    /// initial start and host/port rebinds.
    ///
    /// The server shuts down gracefully (closing its listener AND its open
    /// connections) when EITHER the returned trigger fires (a targeted stop — a
    /// rebind or `destroy`) OR the global `shutdown_rx` goes true (engine
    /// shutdown). Graceful shutdown is what actually severs idle keep-alive
    /// connections; a bare `abort()` would only drop the listener and leave
    /// those connections serving the old address.
    fn spawn_server(
        &self,
        listener: TcpListener,
        addr_display: String,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> (AbortHandle, oneshot::Sender<()>) {
        let hot_router = HotRouter {
            inner: self.shared_routers.clone(),
            engine: self.engine.clone(),
        };
        let make_service = MakeHotRouterService { router: hot_router };
        let (graceful_tx, graceful_rx) = oneshot::channel::<()>();

        let handle = tokio::spawn(async move {
            tracing::info!("API listening on address: {}", addr_display.purple());
            let serve = axum::serve(listener, make_service).with_graceful_shutdown(async move {
                tokio::select! {
                    // Targeted stop for THIS server (rebind / destroy). A dropped
                    // sender resolves the receiver too, which also means "stop".
                    _ = graceful_rx => {}
                    // Global engine shutdown.
                    _ = async {
                        while shutdown_rx.changed().await.is_ok() {
                            if *shutdown_rx.borrow() {
                                break;
                            }
                        }
                    } => {}
                }
            });
            if let Err(e) = serve.await {
                tracing::error!(address = %addr_display, error = %e, "API server exited with error");
            }
        });

        (handle.abort_handle(), graceful_tx)
    }

    /// Re-fetch the authoritative configuration and hot-apply it. The
    /// authoritative read happens under `apply_lock` so overlapping
    /// configuration events can't apply a stale value last (lost update).
    /// All-or-nothing: any failure keeps the previous config and the previous
    /// server running.
    ///
    /// Same address → swap the snapshot and rebuild router layers (CORS,
    /// timeout, concurrency); global middleware is read per-request from the
    /// snapshot and needs no rebuild. Address change → bind the new listener
    /// first (the addresses differ, so no self-conflict), then GRACEFULLY shut
    /// the old server down once the new one is live. Graceful shutdown closes
    /// the old listener AND its open connections — including idle keep-alive
    /// connections a browser would otherwise reuse to keep hitting the old port
    /// — so the old address is fully freed, not merely stopped from accepting
    /// new connections. A hard abort follows after a short grace as a safety net
    /// for a connection that refuses to drain.
    pub(super) async fn apply_config(&self) -> anyhow::Result<()> {
        let _guard = self.apply_lock.lock().await;

        // Fetch the authoritative value under the lock so concurrent
        // configuration events can't apply an older snapshot last. A malformed
        // stored value surfaces here and keeps the previous config and server.
        // The fetch is a bus call (`configuration::get` can be re-registered by
        // any connected worker), so bound it: a hung provider must not wedge
        // every future apply behind the lock.
        let new_config = match tokio::time::timeout(
            super::configuration::CONFIG_BUS_TIMEOUT,
            super::configuration::fetch_config(self.engine.as_ref(), &self.config_snapshot()),
        )
        .await
        {
            Ok(result) => result?,
            // Keep the `Elapsed` error downcastable: `on_config_change`
            // schedules a one-shot retry for timeouts specifically (the stored
            // value is valid but unapplied; without a retry the live config
            // silently diverges until the next unrelated edit).
            Err(elapsed) => {
                return Err(anyhow::Error::new(elapsed)
                    .context("configuration::get timed out; keeping previous config"));
            }
        };

        let old = self.config_snapshot();
        let old_addr = format!("{}:{}", old.host, old.port);
        let new_addr = format!("{}:{}", new_config.host, new_config.port);

        if old_addr == new_addr {
            self.set_config(new_config);
            self.update_routes().await?;
            return Ok(());
        }

        // Resolve every fallible prerequisite before mutating the live config,
        // so a failed rebind leaves the previous config and server untouched.
        // The shutdown_rx check comes FIRST: it doubles as the destroyed-worker
        // guard (destroy() clears it), and checking it before binding means a
        // post-destroy retry can never transiently bind the stored address
        // while a replacement worker is booting.
        let shutdown_rx = self
            .shutdown_rx
            .lock()
            .expect("shutdown_rx mutex poisoned")
            .clone()
            .ok_or_else(|| anyhow!("server was never started; cannot rebind"))?;

        let listener = TcpListener::bind(&new_addr).await.map_err(|err| {
            anyhow!(
                "failed to bind {}; keeping server on {}: {}",
                new_addr,
                old_addr,
                err
            )
        })?;

        self.set_config(new_config);
        self.update_routes().await?;

        let (new_handle, new_graceful) = self.spawn_server(listener, new_addr.clone(), shutdown_rx);

        let prev_abort = self
            .server_abort
            .lock()
            .expect("server_abort mutex poisoned")
            .replace(new_handle);
        let prev_graceful = self
            .server_shutdown
            .lock()
            .expect("server_shutdown mutex poisoned")
            .replace(new_graceful);

        // Gracefully shut the OLD server down now that the new one is live: this
        // closes the old listener and its open connections (idle keep-alive
        // included), so the old port stops serving entirely.
        if let Some(prev_graceful) = prev_graceful {
            let _ = prev_graceful.send(());
        }
        // Safety net: if a connection refuses to drain, hard-abort the old
        // server task after a grace period so it can't pin the old port forever.
        if let Some(prev_abort) = prev_abort {
            tokio::spawn(async move {
                tokio::time::sleep(OLD_SERVER_HARD_STOP_GRACE).await;
                prev_abort.abort();
            });
        }

        tracing::info!(
            old = %old_addr,
            new = %new_addr,
            "iii-http server rebound after configuration change; old address shutting down"
        );
        Ok(())
    }

    fn normalize_http_path_for_key(http_path: &str) -> String {
        if http_path == "/" {
            "/".to_string()
        } else {
            http_path.trim_start_matches('/').to_string()
        }
    }

    fn build_router_key(http_method: &str, http_path: &str) -> String {
        let method = http_method.to_uppercase();
        let path = Self::normalize_http_path_for_key(http_path);
        format!("{}:{}", method, path)
    }

    /// Structural signature of a route, used to detect axum matcher conflicts.
    ///
    /// Axum matches routes by position, not by parameter name, so
    /// `/sessions/{listId}/{userId}` and `/sessions/{userId}/{listId}` are the
    /// same route to its matcher and inserting both panics. Two routes share
    /// this signature exactly when they would collide: same method and same
    /// shape with every path parameter collapsed to a positional placeholder.
    fn route_signature(http_method: &str, http_path: &str) -> String {
        let axum_path = Self::build_router_for_axum(&http_path.to_string());
        let shape = axum_path
            .split('/')
            .map(|segment| {
                if segment.starts_with('{') && segment.ends_with('}') {
                    "{}".to_string()
                } else {
                    segment.to_string()
                }
            })
            .collect::<Vec<String>>()
            .join("/");
        format!("{}:{}", http_method.to_uppercase(), shape)
    }

    /// Updates the router with all routes from the registry and configurations
    async fn update_routes(&self) -> anyhow::Result<()> {
        let config = self.config_snapshot();

        // Build CORS layer
        let cors_layer = Self::build_cors_layer(&config);

        // Read the routers_registry and build the router
        let mut new_router = Self::build_routers_from_routers_registry(
            self.engine.clone(),
            Arc::new(self.clone()),
            &self.routers_registry,
        );

        // Unmatched URLs get the same stable error envelope as every other
        // engine-generated error (axum's default fallback is an empty body).
        new_router = new_router.fallback(super::views::not_found_handler);

        new_router = new_router.layer(cors_layer);

        new_router = new_router.layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            std::time::Duration::from_millis(config.default_timeout),
        ));

        new_router = new_router.layer(ConcurrencyLimitLayer::new(config.concurrency_request_limit));

        let mut shared_router = self.shared_routers.write().await;
        *shared_router = new_router;

        tracing::debug!("Routes updated successfully");
        Ok(())
    }

    fn build_router_for_axum(path: &String) -> String {
        // Axum requires paths to start with a leading slash
        // and convert :param to {param}, since axum 0.8 changed the syntax

        // update for axum 0.8, replacing todo/:id to todo/{id}
        let axum_path = path
            .clone()
            .split('/')
            .map(|segment| {
                if segment.strip_prefix(':').is_some() {
                    format!("{{{}}}", &segment[1..])
                } else {
                    segment.to_string()
                }
            })
            .collect::<Vec<String>>()
            .join("/");

        if axum_path != *path {
            tracing::debug!(
                "Converted path from {} to {}",
                path.purple(),
                axum_path.purple()
            );
        }

        //ensure the path starts with a leading slash
        if !axum_path.starts_with('/') {
            format!("/{}", axum_path)
        } else {
            axum_path
        }
    }
    fn build_routers_from_routers_registry(
        engine: Arc<Engine>,
        api_handler: Arc<HttpWorker>,
        routers_registry: &DashMap<String, PathRouter>,
    ) -> Router {
        use axum::{
            extract::Extension,
            routing::{delete, get, post, put},
        };

        let mut router = Router::new();

        // Defense in depth: `register_router` already rejects conflicting
        // routes, but skip any colliding signature here too so a stray
        // duplicate can never panic axum and crash the HTTP worker thread.
        let mut seen_signatures = std::collections::HashSet::new();

        for entry in routers_registry.iter() {
            let method = entry.http_method.to_ascii_uppercase();

            let signature = Self::route_signature(&method, &entry.http_path);
            if !seen_signatures.insert(signature) {
                tracing::warn!(
                    "Skipping route {} {} — conflicts with a previously registered route of the same structure",
                    method.purple(),
                    entry.http_path.purple()
                );
                continue;
            }

            let path = Self::build_router_for_axum(&entry.http_path);
            let path_for_extension = entry.http_path.clone();
            router = match method.as_str() {
                "GET" => router.route(
                    &path,
                    get(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                "POST" => router.route(
                    &path,
                    post(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                "PUT" => router.route(
                    &path,
                    put(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                "DELETE" => router.route(
                    &path,
                    delete(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                "PATCH" => router.route(
                    &path,
                    axum::routing::patch(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                "HEAD" => router.route(
                    &path,
                    axum::routing::head(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                "OPTIONS" => router.route(
                    &path,
                    axum::routing::options(dynamic_handler).layer(Extension(path_for_extension)),
                ),
                _ => {
                    tracing::warn!("Unsupported HTTP method: {}", method.purple());
                    router
                }
            };
        }

        router
            .layer(Extension(engine))
            .layer(Extension(api_handler))
    }
    /// Builds the CorsLayer based on configuration
    fn build_cors_layer(config: &RestApiConfig) -> CorsLayer {
        let Some(cors_config) = &config.cors else {
            return CorsLayer::permissive();
        };

        let mut cors = CorsLayer::new();

        // Origins
        let has_any_sentinel = cors_config
            .allowed_origins
            .iter()
            .any(|o| o == ALLOW_ORIGIN_ANY);

        if cors_config.allowed_origins.is_empty() || has_any_sentinel {
            if has_any_sentinel && cors_config.allowed_origins.len() > 1 {
                tracing::warn!(
                    "CORS config contains '{}' alongside explicit origins; all origins will be allowed",
                    ALLOW_ORIGIN_ANY
                );
            }
            cors = cors.allow_origin(HTTP_Any);
        } else {
            let origins: Vec<_> = cors_config
                .allowed_origins
                .iter()
                .filter_map(|o| o.parse().ok())
                .collect();
            cors = cors.allow_origin(origins);
        }

        // Methods
        if cors_config.allowed_methods.is_empty() {
            cors = cors.allow_methods(HTTP_Any);
        } else {
            let methods: Vec<Method> = cors_config
                .allowed_methods
                .iter()
                .filter_map(|m| m.parse().ok())
                .collect();
            cors = cors.allow_methods(methods);
        }

        cors.allow_headers(HTTP_Any)
    }

    pub fn get_router(&self, http_method: &str, http_path: &str) -> Option<RouterMatch> {
        let key = Self::build_router_key(http_method, http_path);
        tracing::debug!("Looking up router for key: {}", key);
        self.routers_registry.get(&key).map(|r| RouterMatch {
            function_id: r.function_id.clone(),
            condition_function_id: r.condition_function_id.clone(),
            middleware_function_ids: r.middleware_function_ids.clone(),
            metadata: r.metadata.clone(),
        })
    }

    pub async fn register_router(&self, router: PathRouter) -> anyhow::Result<()> {
        let function_id = router.function_id.clone();
        let http_path = router.http_path.clone();
        let method = router.http_method.to_uppercase();
        let key = Self::build_router_key(&method, &router.http_path);

        // Reject routes that collide with an already-registered route of the
        // same shape but different path-parameter names. Axum's matcher would
        // panic on such an insert, taking down the whole HTTP worker thread, so
        // we fail this single registration gracefully instead.
        let signature = Self::route_signature(&method, &http_path);
        let conflict = self
            .routers_registry
            .iter()
            .find(|entry| {
                entry.key() != &key
                    && Self::route_signature(&entry.http_method, &entry.http_path) == signature
            })
            .map(|entry| entry.http_path.clone());
        if let Some(existing_path) = conflict {
            anyhow::bail!(
                "Route '{} {}' conflicts with already-registered route '{} {}': \
                 routes with identical structure but different path-parameter \
                 names are not supported",
                method,
                http_path,
                method,
                existing_path
            );
        }

        tracing::debug!("Registering router {}", key.purple());
        self.routers_registry.insert(key, router);

        tracing::info!(
            "{} Endpoint {} → {}",
            "[REGISTERED]".green(),
            format!("{} {}", method, http_path).bright_yellow().bold(),
            function_id.purple()
        );

        // Update routes after registering
        self.update_routes().await?;

        Ok(())
    }

    pub async fn unregister_router(
        &self,
        http_method: &str,
        http_path: &str,
        trigger_id: &str,
        worker_id: Option<uuid::Uuid>,
    ) -> anyhow::Result<bool> {
        let key = Self::build_router_key(http_method, http_path);
        tracing::info!("Unregistering router {}", key.purple());

        let removed = self
            .routers_registry
            .remove_if(&key, |_, entry| {
                entry.trigger_id == trigger_id && entry.worker_id == worker_id
            })
            .is_some();

        if removed {
            // Update routes after unregistering
            self.update_routes().await?;
        } else if self.routers_registry.contains_key(&key) {
            tracing::info!(
                "Skipping unregister for {}: owner changed (trigger {} no longer owns this route)",
                key.purple(),
                trigger_id.purple()
            );
        } else {
            tracing::warn!("No router found for key: {}", key.purple());
        }

        Ok(removed)
    }
}

impl TriggerRegistrator for HttpWorker {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let adapter = self.clone();

        Box::pin(async move {
            let api_path = trigger
                .config
                .get("api_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("api_path is required for http triggers"))?;

            let http_method = trigger
                .config
                .get("http_method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET");

            let condition_function_id = trigger
                .config
                .get("condition_function_id")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());

            let middleware_function_ids = trigger
                .config
                .get("middleware_function_ids")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let router = PathRouter::new(
                api_path.to_string(),
                http_method.to_string(),
                trigger.function_id.clone(),
                condition_function_id,
                middleware_function_ids,
            )
            .with_owner(trigger.id.clone(), trigger.worker_id)
            .with_metadata(trigger.metadata.clone());

            adapter.register_router(router).await?;
            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let adapter = self.clone();

        Box::pin(async move {
            let api_path = trigger
                .config
                .get("api_path")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            let http_method = trigger
                .config
                .get("http_method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET");

            adapter
                .unregister_router(http_method, api_path, &trigger.id, trigger.worker_id)
                .await?;
            Ok(())
        })
    }
}

crate::register_worker!(
    "iii-http",
    HttpWorker,
    description = "Expose functions as HTTP endpoints.",
    enabled_by_default = true
);

#[cfg(test)]
mod tests {
    use super::super::config::{CorsConfig, RestApiConfig};
    use super::*;
    use crate::workers::observability::metrics::ensure_default_meter;
    use serde_json::json;

    #[test]
    fn build_router_key_normalizes_leading_slash() {
        let a = HttpWorker::build_router_key("GET", "users/:id");
        let b = HttpWorker::build_router_key("get", "/users/:id");
        assert_eq!(a, b);
        assert_eq!(a, "GET:users/:id");
    }

    #[test]
    fn build_router_key_keeps_root_path() {
        let key = HttpWorker::build_router_key("GET", "/");
        assert_eq!(key, "GET:/");
    }

    #[test]
    fn allow_origin_any_sentinel_is_recognized() {
        let origins = ["*".to_string()];
        assert!(origins.iter().any(|o| o == super::ALLOW_ORIGIN_ANY));
    }

    #[test]
    fn regular_origins_are_not_sentinel() {
        let origins = ["http://localhost:3000".to_string()];
        assert!(!origins.iter().any(|o| o == super::ALLOW_ORIGIN_ANY));
    }

    #[test]
    fn mixed_sentinel_and_explicit_origins_detected() {
        let origins = ["*".to_string(), "http://localhost:3000".to_string()];
        let has_any_sentinel = origins.iter().any(|o| o == super::ALLOW_ORIGIN_ANY);
        assert!(has_any_sentinel);
        assert!(origins.len() > 1);
    }

    // ---- normalize_http_path_for_key tests ----

    #[test]
    fn normalize_root_path_stays_root() {
        assert_eq!(HttpWorker::normalize_http_path_for_key("/"), "/");
    }

    #[test]
    fn normalize_strips_leading_slash() {
        assert_eq!(HttpWorker::normalize_http_path_for_key("/users"), "users");
    }

    #[test]
    fn normalize_no_leading_slash_unchanged() {
        assert_eq!(HttpWorker::normalize_http_path_for_key("users"), "users");
    }

    #[test]
    fn normalize_nested_path() {
        assert_eq!(
            HttpWorker::normalize_http_path_for_key("/api/v1/users"),
            "api/v1/users"
        );
    }

    #[test]
    fn normalize_path_with_params() {
        assert_eq!(
            HttpWorker::normalize_http_path_for_key("/users/:id/posts"),
            "users/:id/posts"
        );
    }

    // ---- build_router_key tests ----

    #[test]
    fn build_router_key_uppercases_method() {
        let key = HttpWorker::build_router_key("post", "items");
        assert_eq!(key, "POST:items");
    }

    #[test]
    fn build_router_key_various_methods() {
        assert_eq!(
            HttpWorker::build_router_key("delete", "/resource"),
            "DELETE:resource"
        );
        assert_eq!(
            HttpWorker::build_router_key("PUT", "/resource"),
            "PUT:resource"
        );
        assert_eq!(
            HttpWorker::build_router_key("patch", "resource"),
            "PATCH:resource"
        );
    }

    #[test]
    fn build_router_key_root_with_different_methods() {
        assert_eq!(HttpWorker::build_router_key("GET", "/"), "GET:/");
        assert_eq!(HttpWorker::build_router_key("POST", "/"), "POST:/");
    }

    // ---- route_signature tests ----

    #[test]
    fn route_signature_collapses_param_names() {
        // Same shape, swapped param names -> identical signature (would collide).
        let a = HttpWorker::route_signature("GET", "sessions/:listId/:userId/calls/:contactId");
        let b = HttpWorker::route_signature("GET", "sessions/:userId/:listId/calls/:contactId");
        assert_eq!(a, b);
        assert_eq!(a, "GET:/sessions/{}/{}/calls/{}");
    }

    #[test]
    fn route_signature_distinguishes_method() {
        let get = HttpWorker::route_signature("GET", "items/:id");
        let post = HttpWorker::route_signature("POST", "items/:id");
        assert_ne!(get, post);
    }

    #[test]
    fn route_signature_distinguishes_literal_segments() {
        let a = HttpWorker::route_signature("GET", "sessions/:id/calls");
        let b = HttpWorker::route_signature("GET", "sessions/:id/messages");
        assert_ne!(a, b);
    }

    #[test]
    fn route_signature_distinguishes_param_vs_literal() {
        let param = HttpWorker::route_signature("GET", "users/:id");
        let literal = HttpWorker::route_signature("GET", "users/me");
        assert_ne!(param, literal);
    }

    // ---- build_router_for_axum tests ----

    #[test]
    fn build_router_for_axum_converts_colon_params() {
        let result = HttpWorker::build_router_for_axum(&"todo/:id".to_string());
        assert_eq!(result, "/todo/{id}");
    }

    #[test]
    fn build_router_for_axum_multiple_params() {
        let result =
            HttpWorker::build_router_for_axum(&"users/:user_id/posts/:post_id".to_string());
        assert_eq!(result, "/users/{user_id}/posts/{post_id}");
    }

    #[test]
    fn build_router_for_axum_no_params() {
        let result = HttpWorker::build_router_for_axum(&"api/v1/health".to_string());
        assert_eq!(result, "/api/v1/health");
    }

    #[test]
    fn build_router_for_axum_already_has_leading_slash() {
        let result = HttpWorker::build_router_for_axum(&"/api/items".to_string());
        assert_eq!(result, "/api/items");
    }

    #[test]
    fn build_router_for_axum_root_path() {
        let result = HttpWorker::build_router_for_axum(&"/".to_string());
        assert_eq!(result, "/");
    }

    #[test]
    fn build_router_for_axum_param_at_root() {
        let result = HttpWorker::build_router_for_axum(&":id".to_string());
        assert_eq!(result, "/{id}");
    }

    #[test]
    fn build_router_for_axum_leading_slash_with_param() {
        let result = HttpWorker::build_router_for_axum(&"/:id".to_string());
        assert_eq!(result, "/{id}");
    }

    // ---- build_cors_layer tests ----

    fn make_worker_with_cors(cors: Option<CorsConfig>) -> HttpWorker {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let config = RestApiConfig {
            cors,
            ..RestApiConfig::default()
        };
        HttpWorker::new_for_tests(engine, config)
    }

    #[test]
    fn is_loopback_host_classification() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.5.4.3"));
        assert!(is_loopback_host("::1"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.10"));
        assert!(!is_loopback_host("example.com"));
    }

    #[test]
    fn build_cors_layer_no_config_returns_permissive() {
        let module = make_worker_with_cors(None);
        let _cors = HttpWorker::build_cors_layer(&module.config_snapshot());
    }

    #[test]
    fn build_cors_layer_empty_origins_allows_any() {
        let module = make_worker_with_cors(Some(CorsConfig {
            allowed_origins: vec![],
            allowed_methods: vec![],
        }));
        let _cors = HttpWorker::build_cors_layer(&module.config_snapshot());
    }

    #[test]
    fn build_cors_layer_wildcard_origin() {
        let module = make_worker_with_cors(Some(CorsConfig {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec!["GET".to_string()],
        }));
        let _cors = HttpWorker::build_cors_layer(&module.config_snapshot());
    }

    #[test]
    fn build_cors_layer_specific_origins() {
        let module = make_worker_with_cors(Some(CorsConfig {
            allowed_origins: vec![
                "http://localhost:3000".to_string(),
                "https://example.com".to_string(),
            ],
            allowed_methods: vec!["GET".to_string(), "POST".to_string()],
        }));
        let _cors = HttpWorker::build_cors_layer(&module.config_snapshot());
    }

    #[test]
    fn build_cors_layer_mixed_wildcard_and_explicit() {
        let module = make_worker_with_cors(Some(CorsConfig {
            allowed_origins: vec!["*".to_string(), "http://localhost:3000".to_string()],
            allowed_methods: vec![],
        }));
        let _cors = HttpWorker::build_cors_layer(&module.config_snapshot());
    }

    #[test]
    fn build_cors_layer_with_all_methods() {
        let module = make_worker_with_cors(Some(CorsConfig {
            allowed_origins: vec!["http://localhost:3000".to_string()],
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "PATCH".to_string(),
                "HEAD".to_string(),
                "OPTIONS".to_string(),
            ],
        }));
        let _cors = HttpWorker::build_cors_layer(&module.config_snapshot());
    }

    // ---- build_routers_from_routers_registry tests ----

    #[test]
    fn build_routers_from_empty_registry() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let module = make_worker_with_cors(None);
        let registry = DashMap::new();
        let _router =
            HttpWorker::build_routers_from_routers_registry(engine, Arc::new(module), &registry);
    }

    #[test]
    fn build_routers_from_registry_with_entries() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let module = make_worker_with_cors(None);
        let registry = DashMap::new();

        registry.insert(
            "GET:users".to_string(),
            PathRouter::new(
                "users".to_string(),
                "GET".to_string(),
                "fn::list_users".to_string(),
                None,
                Vec::new(),
            ),
        );
        registry.insert(
            "POST:users".to_string(),
            PathRouter::new(
                "users".to_string(),
                "POST".to_string(),
                "fn::create_user".to_string(),
                None,
                Vec::new(),
            ),
        );
        registry.insert(
            "PUT:users/:id".to_string(),
            PathRouter::new(
                "users/:id".to_string(),
                "PUT".to_string(),
                "fn::update_user".to_string(),
                None,
                Vec::new(),
            ),
        );
        registry.insert(
            "DELETE:users/:id".to_string(),
            PathRouter::new(
                "users/:id".to_string(),
                "DELETE".to_string(),
                "fn::delete_user".to_string(),
                None,
                Vec::new(),
            ),
        );
        registry.insert(
            "PATCH:users/:id".to_string(),
            PathRouter::new(
                "users/:id".to_string(),
                "PATCH".to_string(),
                "fn::patch_user".to_string(),
                None,
                Vec::new(),
            ),
        );
        registry.insert(
            "HEAD:health".to_string(),
            PathRouter::new(
                "health".to_string(),
                "HEAD".to_string(),
                "fn::health_check".to_string(),
                None,
                Vec::new(),
            ),
        );
        registry.insert(
            "OPTIONS:cors".to_string(),
            PathRouter::new(
                "cors".to_string(),
                "OPTIONS".to_string(),
                "fn::cors_preflight".to_string(),
                None,
                Vec::new(),
            ),
        );

        let _router =
            HttpWorker::build_routers_from_routers_registry(engine, Arc::new(module), &registry);
    }

    #[test]
    fn build_routers_unsupported_method_does_not_panic() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let module = make_worker_with_cors(None);
        let registry = DashMap::new();

        registry.insert(
            "TRACE:debug".to_string(),
            PathRouter::new(
                "debug".to_string(),
                "TRACE".to_string(),
                "fn::trace_handler".to_string(),
                None,
                Vec::new(),
            ),
        );

        let _router =
            HttpWorker::build_routers_from_routers_registry(engine, Arc::new(module), &registry);
    }

    // ---- get_router tests ----

    #[tokio::test]
    async fn test_get_router_found() {
        let module = make_worker_with_cors(None);

        module.routers_registry.insert(
            "GET:users/:id".to_string(),
            PathRouter::new(
                "users/:id".to_string(),
                "GET".to_string(),
                "fn::get_user".to_string(),
                Some("fn::auth_check".to_string()),
                Vec::new(),
            ),
        );

        let result = module.get_router("GET", "users/:id");
        assert!(result.is_some());
        let router_match = result.unwrap();
        assert_eq!(router_match.function_id, "fn::get_user");
        assert_eq!(
            router_match.condition_function_id.as_deref(),
            Some("fn::auth_check")
        );
    }

    #[tokio::test]
    async fn test_get_router_not_found() {
        let module = make_worker_with_cors(None);
        let result = module.get_router("GET", "nonexistent");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_router_normalizes_path() {
        let module = make_worker_with_cors(None);

        module.routers_registry.insert(
            "POST:items".to_string(),
            PathRouter::new(
                "items".to_string(),
                "POST".to_string(),
                "fn::create_item".to_string(),
                None,
                Vec::new(),
            ),
        );

        // Query with leading slash -- should be normalized
        let result = module.get_router("POST", "/items");
        assert!(result.is_some());
        let router_match = result.unwrap();
        assert_eq!(router_match.function_id, "fn::create_item");
    }

    #[tokio::test]
    async fn test_register_router_uses_uppercase_key_but_preserves_original_method() {
        let module = make_worker_with_cors(None);

        module
            .register_router(PathRouter::new(
                "/health".to_string(),
                "get".to_string(),
                "fn::health".to_string(),
                None,
                Vec::new(),
            ))
            .await
            .expect("register should succeed");

        // The registry key is uppercased for lookup...
        let router = module
            .routers_registry
            .get("GET:health")
            .expect("router should be stored under uppercased key");
        // ...but the stored PathRouter retains the original casing.
        assert_eq!(router.http_method, "get");
    }

    // ---- PathRouter tests ----

    #[test]
    fn path_router_new() {
        let router = PathRouter::new(
            "/api/test".to_string(),
            "GET".to_string(),
            "fn::handler".to_string(),
            Some("fn::condition".to_string()),
            Vec::new(),
        );
        assert_eq!(router.http_path, "/api/test");
        assert_eq!(router.http_method, "GET");
        assert_eq!(router.function_id, "fn::handler");
        assert_eq!(
            router.condition_function_id.as_deref(),
            Some("fn::condition")
        );
        assert!(router.middleware_function_ids.is_empty());
    }

    #[test]
    fn path_router_no_condition() {
        let router = PathRouter::new(
            "items".to_string(),
            "POST".to_string(),
            "fn::create".to_string(),
            None,
            Vec::new(),
        );
        assert!(router.condition_function_id.is_none());
        assert!(router.middleware_function_ids.is_empty());
    }

    #[tokio::test]
    async fn create_name_register_functions_and_initialize_work() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());

        let module = <HttpWorker as Worker>::create(
            engine.clone(),
            Some(json!({
                "host": "127.0.0.1",
                "port": 0,
                "default_timeout": 250,
                "concurrency_request_limit": 8
            })),
        )
        .await
        .expect("module should be created");

        assert_eq!(module.name(), "HttpWorker");

        module.register_functions(engine.clone());
        module.initialize().await.expect("module should initialize");

        assert!(engine.trigger_registry.trigger_types.contains_key("http"));
    }

    #[tokio::test]
    async fn rest_api_start_background_tasks_returns_addr_in_use_error_with_address() {
        ensure_default_meter();
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let port = occupied.local_addr().expect("local addr").port();
        let engine = Arc::new(crate::engine::Engine::new());

        let module = <HttpWorker as Worker>::create(
            engine,
            Some(json!({
                "host": "127.0.0.1",
                "port": port,
                "default_timeout": 250,
                "concurrency_request_limit": 8
            })),
        )
        .await
        .expect("module should be created");

        // initialize() no longer binds — only registers the trigger type.
        module
            .initialize()
            .await
            .expect("initialize should succeed");

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let err = module
            .start_background_tasks(shutdown_rx, shutdown_tx.clone())
            .await
            .expect_err("REST API bind should fail when the port is occupied");
        std::mem::forget(shutdown_tx);

        let message = err.to_string();
        assert!(message.contains(&format!("127.0.0.1:{port}")));
        assert!(message.contains("already in use"));
    }

    #[tokio::test]
    async fn rest_api_destroy_aborts_running_server() {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());

        let module = <HttpWorker as Worker>::create(
            engine,
            Some(json!({
                "host": "127.0.0.1",
                "port": 0,
                "default_timeout": 250,
                "concurrency_request_limit": 8
            })),
        )
        .await
        .expect("module should be created");

        module
            .initialize()
            .await
            .expect("initialize should succeed");

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        module
            .start_background_tasks(shutdown_rx, shutdown_tx.clone())
            .await
            .expect("start_background_tasks should succeed");
        std::mem::forget(shutdown_tx);

        module.destroy().await.expect("destroy should succeed");

        // Second destroy is a no-op (abort handle already taken).
        module
            .destroy()
            .await
            .expect("second destroy should be no-op");
    }

    #[tokio::test]
    async fn register_trigger_and_unregister_trigger_manage_routes() {
        let module = make_worker_with_cors(None);
        let trigger = Trigger {
            id: "http-users".to_string(),
            trigger_type: "http".to_string(),
            function_id: "fn::users".to_string(),
            config: json!({
                "api_path": "users/:id",
                "http_method": "post",
                "condition_function_id": "fn::condition"
            }),
            worker_id: None,
            metadata: None,
        };

        module
            .register_trigger(trigger.clone())
            .await
            .expect("trigger registration should succeed");

        let registered = module
            .get_router("POST", "/users/:id")
            .expect("router should exist");
        assert_eq!(registered.function_id, "fn::users");
        assert_eq!(
            registered.condition_function_id.as_deref(),
            Some("fn::condition")
        );

        module
            .unregister_trigger(trigger)
            .await
            .expect("trigger unregistration should succeed");

        assert!(module.get_router("POST", "/users/:id").is_none());
    }

    #[tokio::test]
    async fn unregister_stale_worker_keeps_route_owned_by_newer_worker() {
        // Repro for #1796: rolling deploy / reconnect.
        let module = make_worker_with_cors(None);

        let worker_a = uuid::Uuid::new_v4();
        let worker_b = uuid::Uuid::new_v4();

        let trigger_a = Trigger {
            id: "http-accounts-a".to_string(),
            trigger_type: "http".to_string(),
            function_id: "fn::accounts_a".to_string(),
            config: json!({
                "api_path": "/api/platform-accounts",
                "http_method": "GET"
            }),
            worker_id: Some(worker_a),
            metadata: None,
        };

        let trigger_b = Trigger {
            id: "http-accounts-b".to_string(),
            trigger_type: "http".to_string(),
            function_id: "fn::accounts_b".to_string(),
            config: json!({
                "api_path": "/api/platform-accounts",
                "http_method": "GET"
            }),
            worker_id: Some(worker_b),
            metadata: None,
        };

        // 1. Worker A registers the route.
        module
            .register_trigger(trigger_a.clone())
            .await
            .expect("A registration should succeed");

        // 2. Worker B re-registers the same method/path during reconnect.
        module
            .register_trigger(trigger_b)
            .await
            .expect("B registration should succeed");

        // 3. Worker A's stale cleanup unregisters its trigger.
        module
            .unregister_trigger(trigger_a)
            .await
            .expect("A unregistration should succeed");

        // 4. Route must still resolve to Worker B's function.
        let router = module
            .get_router("GET", "/api/platform-accounts")
            .expect("route should still exist after stale unregister");
        assert_eq!(router.function_id, "fn::accounts_b");
    }

    #[tokio::test]
    async fn unregister_stale_worker_keeps_parameterized_route_owned_by_newer_worker() {
        // Repro for #1796, parameterized route variant.
        let module = make_worker_with_cors(None);

        let worker_a = uuid::Uuid::new_v4();
        let worker_b = uuid::Uuid::new_v4();

        let trigger_a = Trigger {
            id: "http-connect-a".to_string(),
            trigger_type: "http".to_string(),
            function_id: "fn::connect_a".to_string(),
            config: json!({
                "api_path": "/api/platforms/:platform/connect",
                "http_method": "POST"
            }),
            worker_id: Some(worker_a),
            metadata: None,
        };

        let trigger_b = Trigger {
            id: "http-connect-b".to_string(),
            trigger_type: "http".to_string(),
            function_id: "fn::connect_b".to_string(),
            config: json!({
                "api_path": "/api/platforms/:platform/connect",
                "http_method": "POST"
            }),
            worker_id: Some(worker_b),
            metadata: None,
        };

        module
            .register_trigger(trigger_a.clone())
            .await
            .expect("A registration should succeed");
        module
            .register_trigger(trigger_b)
            .await
            .expect("B registration should succeed");

        // Stale cleanup for Worker A must not remove Worker B's route.
        let removed_a = module
            .unregister_router(
                "POST",
                "/api/platforms/:platform/connect",
                &trigger_a.id,
                trigger_a.worker_id,
            )
            .await
            .expect("A unregistration should succeed");
        assert!(!removed_a, "stale owner should not remove the route");

        let router = module
            .get_router("POST", "/api/platforms/:platform/connect")
            .expect("parameterized route should still exist after stale unregister");
        assert_eq!(router.function_id, "fn::connect_b");
    }

    #[tokio::test]
    async fn unregister_router_removes_route_when_owner_matches() {
        // Owner-aware unregister still removes the route for the real owner.
        let module = make_worker_with_cors(None);

        let worker = uuid::Uuid::new_v4();
        let trigger = Trigger {
            id: "http-owned".to_string(),
            trigger_type: "http".to_string(),
            function_id: "fn::owned".to_string(),
            config: json!({
                "api_path": "/api/owned",
                "http_method": "GET"
            }),
            worker_id: Some(worker),
            metadata: None,
        };

        module
            .register_trigger(trigger.clone())
            .await
            .expect("registration should succeed");

        module
            .unregister_trigger(trigger)
            .await
            .expect("unregistration should succeed");

        assert!(module.get_router("GET", "/api/owned").is_none());
    }

    #[tokio::test]
    async fn register_trigger_requires_api_path() {
        let module = make_worker_with_cors(None);
        let trigger = Trigger {
            id: "http-missing-path".to_string(),
            trigger_type: "http".to_string(),
            function_id: "fn::users".to_string(),
            config: json!({ "http_method": "GET" }),
            worker_id: None,
            metadata: None,
        };

        let err = module
            .register_trigger(trigger)
            .await
            .expect_err("api_path should be required");
        assert!(err.to_string().contains("api_path is required"));
    }

    #[tokio::test]
    async fn unregister_router_returns_false_when_missing() {
        let module = make_worker_with_cors(None);

        let removed = module
            .unregister_router("GET", "/missing", "missing-trigger", None)
            .await
            .expect("unregister should succeed");

        assert!(!removed);
    }

    // ---- PathRouter middleware tests ----

    #[test]
    fn test_path_router_with_middleware() {
        let middleware_ids = vec!["fn::auth_mw".to_string(), "fn::rate_limit_mw".to_string()];
        let router = PathRouter::new(
            "/api/secure".to_string(),
            "POST".to_string(),
            "fn::secure_handler".to_string(),
            None,
            middleware_ids.clone(),
        );
        assert_eq!(router.middleware_function_ids, middleware_ids);
        assert_eq!(router.middleware_function_ids.len(), 2);
        assert_eq!(router.middleware_function_ids[0], "fn::auth_mw");
        assert_eq!(router.middleware_function_ids[1], "fn::rate_limit_mw");
    }

    #[test]
    fn test_path_router_without_middleware() {
        let router = PathRouter::new(
            "/api/open".to_string(),
            "GET".to_string(),
            "fn::open_handler".to_string(),
            None,
            Vec::new(),
        );
        assert!(router.middleware_function_ids.is_empty());
    }

    #[tokio::test]
    async fn test_get_router_returns_middleware_ids() {
        let module = make_worker_with_cors(None);
        let middleware_ids = vec!["fn::jwt_mw".to_string(), "fn::log_mw".to_string()];

        module.routers_registry.insert(
            "GET:protected".to_string(),
            PathRouter::new(
                "protected".to_string(),
                "GET".to_string(),
                "fn::protected_handler".to_string(),
                None,
                middleware_ids.clone(),
            ),
        );

        let result = module
            .get_router("GET", "protected")
            .expect("router should exist");

        assert_eq!(result.function_id, "fn::protected_handler");
        assert_eq!(result.middleware_function_ids, middleware_ids);
        assert_eq!(result.middleware_function_ids.len(), 2);
        assert_eq!(result.middleware_function_ids[0], "fn::jwt_mw");
        assert_eq!(result.middleware_function_ids[1], "fn::log_mw");
    }
}
