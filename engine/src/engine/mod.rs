// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{borrow::Cow, collections::VecDeque, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::ws::WebSocket,
    http::{HeaderMap, Uri},
};
use chrono::Utc;
use dashmap::DashMap;
use futures_util::{FutureExt, SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot::error::RecvError};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;

use crate::{
    function::{Function, FunctionHandler, FunctionResult, FunctionsRegistry},
    invocation::{InvocationHandler, http_function::HttpFunctionConfig},
    protocol::{
        DEFAULT_NAMESPACE, ErrorBody, FUNCTION_NAMESPACE_CONFLICT, Message, NAMESPACE_FORBIDDEN,
        WORKER_NAMESPACE_CONFLICT, effective_namespace,
    },
    services::{Service, ServicesRegistry},
    telemetry::{
        ingest_otlp_json, ingest_otlp_logs, ingest_otlp_metrics, inject_baggage_from_context,
        inject_traceparent_from_context,
    },
    worker_connections::{RuntimeWorkerInfo, WorkerConnection, WorkerConnectionRegistry},
    workers::worker::rbac_session::Session,
    workers::{
        http_functions::HttpFunctionsWorker,
        worker::{
            WorkerManagerConfig,
            channels::ChannelManager,
            inbound::{Inbound, Oversized},
            rbac_session,
            worker_socket::{WorkerSocket, send_json},
        },
    },
};

/// Magic prefix for OTLP binary frames (used by SDKs for trace spans)
const OTLP_WS_PREFIX: &[u8] = b"OTLP";
/// Magic prefix for metrics binary frames (used by SDKs for OTEL metrics)
const MTRC_WS_PREFIX: &[u8] = b"MTRC";
/// Magic prefix for logs binary frames (used by SDKs for OTEL logs)
const LOGS_WS_PREFIX: &[u8] = b"LOGS";

/// How long a `Reattach` waits for the evicted previous connection's read
/// loop to exit and finish its own cleanup before tearing the connection
/// down directly (wedged reader guard).
const REATTACH_EVICT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Function provided by the standalone `queue` worker in each project
/// namespace. The engine owns receipt generation and uses this function only
/// as the durable transport boundary.
const ENQUEUE_PROVIDER_FUNCTION_ID: &str = "engine::queue::enqueue";

/// Engine-shaped functions that the standalone queue worker provides inside
/// each project namespace. These IDs are safe exceptions to the reserved
/// `engine::*` rule because queue dispatch resolves them in an explicit target
/// namespace; all other `engine::*` registrations remain restricted to
/// `default`.
const NAMESPACE_SCOPED_QUEUE_FUNCTION_IDS: &[&str] = &[
    ENQUEUE_PROVIDER_FUNCTION_ID,
    "engine::queue::list_topics",
    "engine::queue::topic_stats",
    "engine::queue::dlq_topics",
    "engine::queue::dlq_messages",
];

/// Returns whether a function is an official namespace-scoped queue provider.
fn is_namespace_scoped_queue_function(function_id: &str) -> bool {
    NAMESPACE_SCOPED_QUEUE_FUNCTION_IDS.contains(&function_id)
}

/// How long a freshly connected worker's registrations wait for the namespace
/// to arrive before the engine gives up and files them under
/// [`DEFAULT_NAMESPACE`].
///
/// A connection's namespace does not arrive as a protocol message: it rides on
/// the `engine::workers::register` engine call. SDKs that predate the reorder
/// flush their `RegisterFunction` queue *before* that call, so registrations
/// routinely arrive while the namespace is still unknown. Every real SDK does
/// send `engine::workers::register` eventually, so the buffer drains in
/// microseconds; this grace only bounds a hand-rolled client that never sends
/// it at all.
pub const REGISTRATION_NAMESPACE_GRACE: Duration = Duration::from_secs(5);

/// Per-connection namespace resolution state.
///
/// `Draining` is not just bookkeeping: the drain runs on the task handling
/// `engine::workers::register` (or on the grace timer), concurrently with the
/// connection's WS read loop. Flipping straight to `Resolved` before the queue
/// empties would let a message arriving mid-drain register immediately and
/// overtake the ones still queued ahead of it. Keeping the connection in
/// `Draining` routes those arrivals to the back of the same queue, so the
/// drainer applies everything in arrival order and only publishes `Resolved`
/// once the queue is empty.
enum NamespaceState {
    /// Namespace unknown; registration messages queue in arrival order.
    Pending(VecDeque<Message>),
    /// Namespace known, queue not yet empty.
    Draining(String, VecDeque<Message>),
    /// Namespace known and queue drained; registrations take the direct path.
    Resolved(String),
    /// The connection is being torn down. Carries the namespace so
    /// `cleanup_worker` still releases registrations from where the drain put
    /// them. A drain observing this stops rather than registering functions
    /// for a worker that is already gone.
    Aborted(String),
}

/// The refusal every blank-namespace guard sends back, naming the field so the
/// caller knows which namespace field was the empty one.
fn blank_namespace_message(field: &str) -> String {
    format!(
        "`{field}` is empty: it was named and left blank. Give it a name, or omit it to use \
         `{}`.",
        DEFAULT_NAMESPACE
    )
}

/// Registration messages are the only ones whose effect depends on the
/// connection's namespace, so they are the only ones worth buffering.
///
/// `UnregisterFunction` is included even though it is not itself a
/// registration: leaving it unbuffered would let it run against an empty
/// registry while the matching `RegisterFunction` still sits in the queue,
/// inverting the pair and resurrecting a function the worker retired.
///
/// `Observe` / `Unobserve` are buffered to preserve arrival order relative to
/// the same connection's `RegisterFunction`s: an observer's target is often a
/// function the same connection registered just before, and draining in order
/// means it is already in the registry by the time the observer's RBAC gate
/// inspects it.
fn is_namespaced_registration(msg: &Message) -> bool {
    matches!(
        msg,
        Message::RegisterFunction { .. }
            | Message::RegisterService { .. }
            | Message::UnregisterFunction { .. }
            | Message::Observe { .. }
            | Message::Unobserve { .. }
    )
}

/// The 0.25 message types this engine does not decode, with what replaces
/// each. A worker that sends one gets one `warn` per connection and type
/// (`WorkerConnection::first_removed_message`), and the frame is dropped.
pub(crate) const REMOVED_MESSAGES: [(&str, &str); 5] = [
    (
        "registertriggertype",
        "register an observable function instead (`args_format` for its args, `private: true` \
         for an event source)",
    ),
    (
        "unregistertriggertype",
        "unregister the observable function that replaces the type instead",
    ),
    (
        "registertrigger",
        "observe the function that replaces the type with iii.observe(source, target) instead",
    ),
    ("unregistertrigger", "dispose the observer instead"),
    (
        "triggerregistrationresult",
        "acknowledge observers with `observerack` instead",
    ),
];

/// Handles binary frames with OTEL telemetry prefixes.
/// Returns true if the frame was handled (matched a known prefix), false otherwise.
async fn handle_telemetry_frame(bytes: &[u8], peer: &SocketAddr) -> bool {
    // Match on the prefix to determine which handler to use
    let (_prefix, name, result) = if bytes.starts_with(OTLP_WS_PREFIX) {
        let payload = &bytes[OTLP_WS_PREFIX.len()..];
        match std::str::from_utf8(payload) {
            Ok(json_str) => (OTLP_WS_PREFIX, "OTLP", ingest_otlp_json(json_str).await),
            Err(err) => {
                tracing::warn!(peer = %peer, error = ?err, "OTLP payload is not valid UTF-8");
                return true;
            }
        }
    } else if bytes.starts_with(MTRC_WS_PREFIX) {
        let payload = &bytes[MTRC_WS_PREFIX.len()..];
        match std::str::from_utf8(payload) {
            Ok(json_str) => (
                MTRC_WS_PREFIX,
                "Metrics",
                ingest_otlp_metrics(json_str).await,
            ),
            Err(err) => {
                tracing::warn!(peer = %peer, error = ?err, "Metrics payload is not valid UTF-8");
                return true;
            }
        }
    } else if bytes.starts_with(LOGS_WS_PREFIX) {
        let payload = &bytes[LOGS_WS_PREFIX.len()..];
        match std::str::from_utf8(payload) {
            Ok(json_str) => (LOGS_WS_PREFIX, "Logs", ingest_otlp_logs(json_str).await),
            Err(err) => {
                tracing::warn!(peer = %peer, error = ?err, "Logs payload is not valid UTF-8");
                return true;
            }
        }
    } else {
        return false;
    };

    // Log any ingestion errors
    if let Err(err) = result {
        tracing::warn!(peer = %peer, error = ?err, "{} ingestion error", name);
    }
    true
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Outbound {
    Protocol(Message),
    Raw(WsMessage),
}

/// A registration refused because a live worker already holds the same worker
/// name or function id in the target namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceConflict {
    pub namespace: String,
    pub name: String,
    pub owner_worker_id: String,
}

#[derive(Debug)]
pub struct RegisterFunctionRequest {
    pub function_id: String,
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
}

pub type HandlerOutput = FunctionResult<Option<Value>, ErrorBody>;

pub trait HandlerFn<F: Future<Output = HandlerOutput> + Send + 'static>:
    Fn(Value) -> F + Send + Sync + 'static
{
}

impl<H, F> HandlerFn<F> for H
where
    H: Fn(Value) -> F + Send + Sync + 'static,
    F: Future<Output = HandlerOutput> + Send + 'static,
{
}

pub trait SessionHandlerFn<F: Future<Output = HandlerOutput> + Send + 'static>:
    Fn(Value, Option<Arc<Session>>) -> F + Send + Sync + 'static
{
}

impl<H, F> SessionHandlerFn<F> for H
where
    H: Fn(Value, Option<Arc<Session>>) -> F + Send + Sync + 'static,
    F: Future<Output = HandlerOutput> + Send + 'static,
{
}

pub struct Handler<H> {
    pub f: H,
}

impl<H, F> Handler<H>
where
    H: Fn(Value) -> F + Send + Sync + 'static,
    F: Future<Output = HandlerOutput> + Send + 'static,
{
    pub fn new(f: H) -> Self {
        Self { f }
    }

    pub fn call(&self, input: Value) -> F {
        (self.f)(input)
    }
}

pub struct SessionHandler<H> {
    pub f: H,
}

impl<H, F> SessionHandler<H>
where
    H: Fn(Value, Option<Arc<Session>>) -> F + Send + Sync + 'static,
    F: Future<Output = HandlerOutput> + Send + 'static,
{
    pub fn new(f: H) -> Self {
        Self { f }
    }
}

#[allow(async_fn_in_trait)]
pub trait EngineTrait: Send + Sync {
    async fn call(
        &self,
        function_id: &str,
        input: impl Serialize + Send,
    ) -> Result<Option<Value>, ErrorBody> {
        self.call_with_metadata(function_id, input, None).await
    }

    /// Engine-orchestration call in the default namespace.
    async fn call_with_metadata(
        &self,
        function_id: &str,
        input: impl Serialize + Send,
        metadata: Option<Value>,
    ) -> Result<Option<Value>, ErrorBody> {
        self.call_with_metadata_ns(DEFAULT_NAMESPACE, function_id, input, metadata)
            .await
    }

    /// Like [`Self::call_with_metadata`] but resolves the target function in an
    /// explicit `namespace`, rather than unconditionally in `default`.
    async fn call_with_metadata_ns(
        &self,
        namespace: &str,
        function_id: &str,
        input: impl Serialize + Send,
        metadata: Option<Value>,
    ) -> Result<Option<Value>, ErrorBody>;

    /// Registers into [`DEFAULT_NAMESPACE`]. Convenience for builtin /
    /// engine-internal workers that are not namespace-aware.
    fn register_function(
        &self,
        request: RegisterFunctionRequest,
        handler: Box<dyn FunctionHandler + Send + Sync>,
    ) {
        self.register_function_ns(DEFAULT_NAMESPACE, request, handler);
    }

    /// Registers into `namespace`. Callers holding a `WorkerConnection` must
    /// use this with the connection's effective namespace, so the registration
    /// lands where the unregister/cleanup paths will look for it.
    fn register_function_ns(
        &self,
        namespace: &str,
        request: RegisterFunctionRequest,
        handler: Box<dyn FunctionHandler + Send + Sync>,
    );
    /// Registers into [`DEFAULT_NAMESPACE`]. Convenience for builtin /
    /// engine-internal workers that are not namespace-aware.
    fn register_function_handler<H, F>(&self, request: RegisterFunctionRequest, handler: Handler<H>)
    where
        H: HandlerFn<F>,
        F: Future<Output = HandlerOutput> + Send + 'static,
    {
        self.register_function_handler_ns(DEFAULT_NAMESPACE, request, handler);
    }
    /// Registers into `namespace`. Callers acting on behalf of a WS connection
    /// (today: `HttpFunctionsWorker`, for HTTP-invocation registrations) must
    /// use this with the connection's effective namespace, so the function lands
    /// where the service registry, the unregister path and `resolve_function`
    /// all agree to look for it.
    fn register_function_handler_ns<H, F>(
        &self,
        namespace: &str,
        request: RegisterFunctionRequest,
        handler: Handler<H>,
    ) where
        H: HandlerFn<F>,
        F: Future<Output = HandlerOutput> + Send + 'static;
    fn register_function_handler_with_session<H, F>(
        &self,
        request: RegisterFunctionRequest,
        handler: SessionHandler<H>,
    ) where
        H: SessionHandlerFn<F>,
        F: Future<Output = HandlerOutput> + Send + 'static;
}

/// How the owner of a `function_owners` entry registered the function: over the
/// regular WebSocket invocation path, or as an HTTP-invocation (external)
/// function. Recorded alongside the owner so a release can assert or branch on
/// it; the two kinds still share one `FunctionsRegistry`, which is why they must
/// share one owner map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvocationKind {
    Regular,
    External,
}

#[derive(Clone)]
pub struct Engine {
    pub worker_registry: Arc<WorkerConnectionRegistry>,
    pub runtime_workers: Arc<DashMap<String, RuntimeWorkerInfo>>,
    pub functions: Arc<FunctionsRegistry>,
    pub service_registry: Arc<ServicesRegistry>,
    pub invocations: Arc<InvocationHandler>,
    pub channel_manager: Arc<ChannelManager>,
    /// Observers (`iii.observe`): registry, routed hook and activations.
    pub observers: Arc<crate::observer::ObserverRegistry>,
    /// The engine's own workers listening to their configuration entry
    /// (`iii-observability`, `iii-stream`).
    pub configuration_listeners:
        Arc<crate::workers::configuration::listeners::ConfigurationListeners>,
    /// Records the current owning WS worker for each registered function,
    /// keyed by `(namespace, function_id)` to match `FunctionsRegistry`.
    /// Populated when a worker sends `Message::RegisterFunction`; used by
    /// `cleanup_worker` and `remove_worker_registrations` to atomically skip
    /// removal of registrations that have been overwritten by a different,
    /// still-live worker (the fast-restart race). In-process workers do not
    /// populate this map, so the absence of an entry means "no WS owner."
    ///
    /// The namespace is part of the key, not decoration: keying by bare id
    /// would let a worker in one namespace take the lease on an id another
    /// namespace's worker legitimately owns, leaving the loser's registration
    /// orphaned when it disconnects (its CAS release would never match).
    ///
    /// The value carries the owning worker plus the [`InvocationKind`] it
    /// registered under. Regular and HTTP-invocation registrations share this
    /// ONE map (and one `FunctionsRegistry`), so claim and release are a single
    /// `DashMap` entry operation — check-and-write atomic by construction. Two
    /// concurrent claims of the same key, one Regular and one External, cannot
    /// both observe a free lease: whichever wins the entry lock is seen by the
    /// other. The `kind` records how the current owner registered so a release
    /// can branch on it if needed; the CAS key for release is the owner id.
    pub(crate) function_owners: Arc<DashMap<(String, String), (Uuid, InvocationKind)>>,
    /// Records the current owning WS connection for each live worker name,
    /// keyed by `(namespace, worker_name)`. Populated when a worker sends
    /// `engine::workers::register` (see `claim_worker_name`) and CAS-released by
    /// `cleanup_worker`. One live name per namespace: a second live connection
    /// claiming a name already held is rejected and closed. A lease held by a
    /// worker that has gone away — or whose connection is tearing down — is
    /// taken over, so a worker restart reclaims its own name.
    pub(crate) worker_name_owners: Arc<DashMap<(String, String), Uuid>>,
    /// Namespace resolution state per WS connection, seeded by
    /// `begin_namespace_resolution` when the connection is accepted and dropped
    /// by `cleanup_worker`. A connection with no entry here (in-process
    /// workers, direct `router_msg` callers) never buffers.
    namespace_states: Arc<DashMap<Uuid, NamespaceState>>,
    /// The per-connection namespace-grace timer task, so it can be cancelled the
    /// moment the namespace is fixed (`engine::workers::register`), the worker is
    /// rejected, or the connection tears down — instead of lingering (with its
    /// `engine`/`worker` clones) for the full grace, or, worse, firing after a
    /// rejection and draining a rejected worker's buffered registrations.
    namespace_grace_tasks: Arc<DashMap<Uuid, tokio::task::JoinHandle<()>>>,
    pub(crate) active_scope: Arc<std::sync::Mutex<Option<crate::workers::reload::ScopeBuilder>>>,
    /// Effective `iii-worker-manager` port, resolved from config at build
    /// time. Set once by `EngineBuilder::build`; subsequent reads see the
    /// same value for the engine's lifetime. Engine-owned external workers
    /// use it to connect back to the actual configured port.
    worker_manager_port: Arc<std::sync::OnceLock<u16>>,
    /// Absolute path of the config file this engine was built from, when
    /// file-backed. Set once by `EngineBuilder::build`. Handed to spawned
    /// engine-owned child processes as `III_CONFIG_PATH`.
    config_path: Arc<std::sync::OnceLock<std::path::PathBuf>>,
    /// Registration namespace grace in milliseconds, resolved from the global
    /// engine config by `EngineBuilder::build`. Read through
    /// [`Engine::registration_namespace_grace`], where the `III_NAMESPACE_GRACE_MS`
    /// env var takes precedence and an unset lock falls back to the 5s default.
    registration_namespace_grace_ms: Arc<std::sync::OnceLock<u64>>,
}

fn resolve_registration_id(worker: &WorkerConnection, id: &str) -> String {
    if let Some(prefix) = worker
        .session
        .as_ref()
        .and_then(|s| s.function_registration_prefix.as_ref())
    {
        format!("{prefix}::{id}")
    } else {
        id.to_string()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        let active_scope = Arc::new(std::sync::Mutex::new(None));
        let worker_registry = Arc::new(WorkerConnectionRegistry::new());
        let functions = Arc::new(FunctionsRegistry::with_scope(active_scope.clone()));
        let invocations = Arc::new(InvocationHandler::new());
        let observers = Arc::new(crate::observer::ObserverRegistry::new());
        let function_owners = Arc::new(DashMap::new());
        observers.attach(
            functions.clone(),
            worker_registry.clone(),
            Arc::downgrade(&invocations),
            Arc::clone(&function_owners),
        );
        invocations.set_observers(observers.clone());
        // Weak: the registry is owned by this engine through `observers`.
        let listener = Arc::downgrade(&observers);
        functions.set_observer_listener(Arc::new(
            move |change: crate::function::FunctionChange, namespace: &str, function_id: &str| {
                if let Some(observers) = listener.upgrade() {
                    observers.on_function_change(change, namespace, function_id);
                }
            },
        ));
        Self {
            worker_registry,
            runtime_workers: Arc::new(DashMap::new()),
            functions,
            service_registry: Arc::new(ServicesRegistry::new()),
            invocations,
            channel_manager: Arc::new(ChannelManager::new()),
            observers,
            configuration_listeners: Arc::new(Default::default()),
            function_owners,
            worker_name_owners: Arc::new(DashMap::new()),
            namespace_states: Arc::new(DashMap::new()),
            namespace_grace_tasks: Arc::new(DashMap::new()),
            active_scope,
            worker_manager_port: Arc::new(std::sync::OnceLock::new()),
            config_path: Arc::new(std::sync::OnceLock::new()),
            registration_namespace_grace_ms: Arc::new(std::sync::OnceLock::new()),
        }
    }

    /// The effective registration namespace grace. Precedence: the
    /// `III_NAMESPACE_GRACE_MS` env var (runtime override), then the value
    /// resolved from the global engine config by `EngineBuilder::build`,
    /// then [`REGISTRATION_NAMESPACE_GRACE`] (5s).
    pub fn registration_namespace_grace(&self) -> Duration {
        if let Some(ms) = std::env::var("III_NAMESPACE_GRACE_MS")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
        {
            return Duration::from_millis(ms);
        }
        match self.registration_namespace_grace_ms.get().copied() {
            Some(ms) => Duration::from_millis(ms),
            None => REGISTRATION_NAMESPACE_GRACE,
        }
    }

    /// Records the configured grace in milliseconds. Called once by
    /// `EngineBuilder::build`; later calls are ignored (OnceLock semantics).
    pub fn set_registration_namespace_grace_ms(&self, ms: u64) {
        let _ = self.registration_namespace_grace_ms.set(ms);
    }

    /// Returns the effective `iii-worker-manager` port. Resolved from config
    /// by `EngineBuilder::build` (see `set_worker_manager_port`); falls back
    /// to `workers::worker::DEFAULT_PORT` if never set (direct `Engine::new`
    /// paths used only by tests).
    pub fn worker_manager_port(&self) -> u16 {
        self.worker_manager_port
            .get()
            .copied()
            .unwrap_or(crate::workers::worker::DEFAULT_PORT)
    }

    /// Records the effective port. Called once by `EngineBuilder::build`
    /// after scanning the worker list for an `iii-worker-manager` entry.
    /// Subsequent calls are ignored (OnceLock semantics) so the port cannot
    /// drift mid-lifetime.
    pub fn set_worker_manager_port(&self, port: u16) {
        let _ = self.worker_manager_port.set(port);
    }

    /// Absolute config file path when the engine is file-backed; `None` for
    /// programmatic/in-memory configs. See the field doc for why spawned
    /// workers need it.
    pub fn config_path(&self) -> Option<&std::path::Path> {
        self.config_path.get().map(|p| p.as_path())
    }

    /// Records the config file path. Called once by `EngineBuilder::build`;
    /// later calls are ignored (OnceLock semantics).
    pub fn set_config_path(&self, path: std::path::PathBuf) {
        let _ = self.config_path.set(path);
    }

    pub fn upsert_runtime_worker(&self, worker: RuntimeWorkerInfo) {
        self.runtime_workers.insert(worker.id.clone(), worker);
    }

    pub fn remove_runtime_worker(&self, worker_id: &str) {
        self.runtime_workers.remove(worker_id);
    }

    pub fn list_runtime_workers(&self) -> Vec<RuntimeWorkerInfo> {
        self.runtime_workers
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Opens a scope so that registrations made between here and
    /// [`Self::end_worker_scope`] are attributed to `worker_name`. Panics if a
    /// scope is already active -- scopes do not nest.
    ///
    /// FIXME: `active_scope` is process-wide. During the window between
    /// `begin_worker_scope` and `end_worker_scope`, a concurrent
    /// `RegisterFunction` call from an unrelated WebSocket-connected worker
    /// could be captured into this scope, causing `remove_worker_registrations`
    /// to later delete a function that doesn't belong to the scoped worker.
    /// The practical risk is low because `register_functions` is synchronous
    /// and the window is very short, but for correctness a per-worker
    /// registrar token (or equivalent isolation) should replace the global
    /// `Arc<Mutex<Option<ScopeBuilder>>>`.
    pub fn begin_worker_scope(&self, worker_name: &str) {
        let mut scope = self.active_scope.lock().expect("scope mutex poisoned");
        assert!(
            scope.is_none(),
            "begin_worker_scope called while a scope was already active"
        );
        tracing::trace!(worker = %worker_name, "begin_worker_scope");
        *scope = Some(crate::workers::reload::ScopeBuilder::new());
    }

    /// Closes the current scope and returns the registrations captured inside
    /// it. Panics if no scope is active.
    pub fn end_worker_scope(&self) -> crate::workers::reload::WorkerRegistrations {
        let mut scope = self.active_scope.lock().expect("scope mutex poisoned");
        scope
            .take()
            .expect("end_worker_scope called without active scope")
            .into_registrations()
    }

    /// Removes every registration recorded in `regs` from the engine's global
    /// registries. Used during in-process worker destroy and reload.
    ///
    /// Skips ids currently owned by a WS worker via `function_owners` (regular
    /// or HTTP-invocation, now one map) — without this, destroying an in-process
    /// worker that happened to share a function id with a connected WS worker
    /// would tear out the WS worker's live registration (or its
    /// `service_registry` entry, in the HTTP case). In-process workers
    /// themselves do not populate the owner map, so the absence of an entry is
    /// the "no WS owner" signal that means we can safely remove.
    pub fn remove_worker_registrations(&self, regs: &crate::workers::reload::WorkerRegistrations) {
        for id in &regs.function_ids {
            // In-process workers only ever register into the default namespace,
            // so only a WS owner of the *same* `(default, id)` key can be
            // clobbered by the removal below.
            let key = (DEFAULT_NAMESPACE.to_string(), id.clone());
            if self.function_owners.contains_key(&key) {
                tracing::debug!(
                    function_id = %id,
                    "Skipping in-process registration removal — a WS worker currently owns this id"
                );
                continue;
            }
            // In-process workers always register into the default namespace.
            self.remove_function_from_engine(DEFAULT_NAMESPACE, id);
        }
    }

    /// Whether `function_id` is an `engine::*` builtin the engine registered
    /// in-process. Only WS/HTTP worker registrations populate `function_owners`,
    /// so an absent entry is the "engine-owned, not worker-owned" signal that
    /// lets trusted engine builtins bypass the operator's middleware while a
    /// worker-registered `engine::foo` does not.
    fn is_engine_owned_builtin(&self, namespace: &str, function_id: &str) -> bool {
        function_id.starts_with("engine::")
            && !self
                .function_owners
                .contains_key(&(namespace.to_string(), function_id.to_string()))
    }

    async fn send_msg(&self, worker: &WorkerConnection, msg: Message) -> bool {
        worker.channel.send(Outbound::Protocol(msg)).await.is_ok()
    }

    async fn guard_outbound(
        &self,
        worker: &WorkerConnection,
        msg: Message,
        serialized: String,
    ) -> Option<(Message, String)> {
        let size = serialized.len();
        let limit = worker
            .receive_message_limit
            .load(std::sync::atomic::Ordering::Relaxed)
            .min(crate::workers::worker::MAX_JSON_MESSAGE_BYTES);
        if size <= limit {
            return Some((msg, serialized));
        }

        let upgrade = worker
            .receive_limit_requires_upgrade
            .load(std::sync::atomic::Ordering::Relaxed);
        tracing::warn!(
            worker_id = %worker.id,
            version = ?worker.receive_sdk_version.read().unwrap_or_else(|error| error.into_inner()).as_ref(),
            size_bytes = size,
            limit_bytes = limit,
            message_type = protocol_message_type(&msg),
            "Outbound JSON envelope exceeds target worker receive limit"
        );
        match msg {
            Message::InvokeFunction {
                invocation_id: Some(invocation_id),
                ..
            } => {
                if let Some(invocation) = self.invocations.remove(&invocation_id) {
                    if let Some(caller_id) = invocation.worker_id
                        && let Some(caller) = self.worker_registry.get_worker(&caller_id)
                    {
                        caller.remove_invocation(&invocation_id).await;
                    }
                    // An observer activation's observer counts a `TOO_LARGE`
                    // drop rather than the target's error.
                    self.observers.refused_send(invocation_id, size, limit);
                    let _ = invocation
                        .sender
                        .send(crate::invocation::Completion::from(Err(
                            payload_too_large_error(size, limit, upgrade),
                        )));
                }
                worker.remove_invocation(&invocation_id).await;
                None
            }
            Message::InvocationResult {
                invocation_id,
                function_id,
                traceparent,
                baggage,
                ..
            } => {
                let fallback = Message::InvocationResult {
                    invocation_id,
                    function_id,
                    result: None,
                    error: Some(payload_too_large_error(size, limit, upgrade)),
                    traceparent,
                    baggage,
                    observed: Vec::new(),
                };
                let fallback_payload = serde_json::to_string(&fallback).ok()?;
                (fallback_payload.len() <= limit).then_some((fallback, fallback_payload))
            }
            Message::InvokeFunction {
                invocation_id: None,
                ..
            } => None,
            _ => None,
        }
    }

    fn remove_function(&self, namespace: &str, function_id: &str) {
        self.functions.remove(namespace, function_id);
    }

    fn remove_function_from_engine(&self, namespace: &str, function_id: &str) {
        self.remove_function(namespace, function_id);
        self.service_registry
            .remove_function_from_services(namespace, function_id);
    }

    /// Resolves `function_id`'s handler in exactly one namespace, and names
    /// that namespace.
    ///
    /// `Some(ns)` resolves only in `ns`; `None` resolves only in
    /// [`DEFAULT_NAMESPACE`]. There is deliberately no best-fit and no
    /// cross-namespace search, and the *caller's* namespace never participates:
    /// a worker in `analytics` that invokes without naming a namespace gets the
    /// `default` function, not its own. Namespaces are a routing dimension the
    /// caller addresses explicitly, not an ambient scope — anything else makes
    /// the target of an invoke depend on who is asking, which is exactly the
    /// ambiguity namespaces exist to remove.
    ///
    /// A miss keeps the `function_not_found` code (SDKs and the `Enqueue`
    /// provider-missing DX branch both match on it) and enriches the message
    /// with the namespaces where the id *does* exist.
    fn resolve_function_handler<'a>(
        &self,
        requested_ns: Option<&'a str>,
        function_id: &str,
    ) -> Result<(&'a str, Arc<crate::function::HandlerFn>), ErrorBody> {
        let namespace = requested_ns.unwrap_or(DEFAULT_NAMESPACE);
        self.resolve_function_with(requested_ns, function_id, FunctionsRegistry::get_handler)
            .map(|handler| (namespace, handler))
    }

    pub(crate) fn is_function_allowed(
        &self,
        session: &crate::workers::worker::rbac_session::Session,
        requested_ns: Option<&str>,
        function_id: &str,
    ) -> bool {
        let namespace = requested_ns.unwrap_or(DEFAULT_NAMESPACE);
        let check = |function: Option<&Function>| {
            crate::workers::worker::rbac_config::is_function_allowed_ref(
                function_id,
                namespace,
                session.config.rbac.as_ref(),
                &session.namespaces,
                &session.allowed_functions,
                &session.forbidden_functions,
                function,
            )
        };
        self.resolve_function_with(requested_ns, function_id, |registry, namespace, id| {
            registry.with_function(namespace, id, |function| {
                function.map(|function| check(Some(function)))
            })
        })
        .unwrap_or_else(|_| check(None))
    }

    fn resolve_function_with<T>(
        &self,
        requested_ns: Option<&str>,
        function_id: &str,
        lookup: impl Fn(&FunctionsRegistry, &str, &str) -> Option<T>,
    ) -> Result<T, ErrorBody> {
        let namespace = requested_ns.unwrap_or(DEFAULT_NAMESPACE);
        if let Some(function) = lookup(&self.functions, namespace, function_id) {
            return Ok(function);
        }

        let known = self.functions.namespaces_for(function_id);
        let message = if known.is_empty() {
            crate::legacy_worker_functions::migration_message(function_id).unwrap_or_else(|| {
                format!("Function {function_id} not found in namespace {namespace}.")
            })
        } else {
            format!(
                "Function {function_id} not found in namespace {namespace}. It is registered in \
                 namespace(s): {}.",
                known.join(", ")
            )
        };
        Err(ErrorBody {
            code: "function_not_found".into(),
            message,
            stacktrace: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn remember_invocation(
        &self,
        worker: &WorkerConnection,
        invocation_id: Option<Uuid>,
        function_id: &str,
        namespace: Option<&str>,
        body: Value,
        traceparent: Option<String>,
        baggage: Option<String>,
        metadata: Option<Value>,
        caller: Option<Box<crate::protocol::Caller>>,
        observe_depth: Option<u32>,
    ) -> Result<Result<Option<Value>, ErrorBody>, RecvError> {
        tracing::debug!(
            worker_id = %worker.id,
            invocation_id = %crate::logging::display_option(&invocation_id),
            function_id = function_id,
            namespace = %namespace.unwrap_or(DEFAULT_NAMESPACE),
            traceparent = %crate::logging::display_option(&traceparent),
            baggage = %crate::logging::display_option(&baggage),
            "Remembering invocation for worker"
        );

        match self.resolve_function_handler(namespace, function_id) {
            // `resolved_namespace` is where the function was found: the
            // observer hook keys on it.
            Ok((resolved_namespace, function)) => {
                if !crate::workers::telemetry::is_iii_builtin_function_id(function_id) {
                    crate::workers::telemetry::collector::mark_user_function_invoked();
                }

                let session = worker.session.clone();

                // `worker_*` is stamped from this connection (D9); the rest of
                // the caller is what its SDK asserted. A connection without
                // `observe.v1` never sends a depth: the loop guard takes the
                // deepest one it is running (a heuristic, see
                // `WorkerConnection::legacy_depth`).
                let observe_depth = match observe_depth {
                    None if !worker.features.observe_v1() => worker.inflight_depth(),
                    depth => depth,
                };
                let ctx = crate::observer::InvocationCtx::from_connection(
                    worker.id,
                    self.worker_registry.clone(),
                    caller,
                    observe_depth,
                );
                self.invocations
                    .handle_invocation_with_handler(
                        invocation_id,
                        Some(worker),
                        function_id.to_string(),
                        resolved_namespace,
                        body,
                        function,
                        traceparent,
                        baggage,
                        session,
                        metadata,
                        ctx,
                    )
                    .await
            }
            Err(err) => {
                tracing::error!(
                    function_id = %function_id,
                    namespace = %namespace.unwrap_or(DEFAULT_NAMESPACE),
                    "Function not found"
                );
                Ok(Err(err))
            }
        }
    }

    /// Spawns the standard invoke-function flow as a background task.
    ///
    /// When `invocation_id` is `Some`, an `InvocationResult` is sent back
    /// to the caller once the function completes.  When `None`, the call
    /// is fire-and-forget (used by the `Void` action).
    ///
    /// `namespace` is the namespace the *invoke message* asked for, not the
    /// caller connection's — see [`Self::resolve_function`].
    #[allow(clippy::too_many_arguments)]
    fn spawn_invoke_function(
        &self,
        worker: &WorkerConnection,
        function_id: String,
        namespace: Option<String>,
        mut data: Value,
        traceparent: Option<String>,
        baggage: Option<String>,
        invocation_id: Option<Uuid>,
        metadata: Option<Value>,
        caller: Option<Box<crate::protocol::Caller>>,
        observe_depth: Option<u32>,
    ) {
        // The canonical engine span for an invocation is the `call <fn>` span
        // opened in `InvocationHandler::handle_invocation`. We intentionally do
        // NOT open a separate `handle_invocation` span here: it produced a
        // redundant second engine SERVER span for every worker-initiated call
        // (`handle_invocation <fn>` -> `call <fn>`), doubling engine span
        // volume. A disabled span keeps the instrumented task structure intact
        // while emitting nothing.
        let span = tracing::Span::none();

        let engine = self.clone();
        let worker = worker.clone();
        // Add caller's worker_id to invocation data as standard metadata
        if let Some(obj) = data.as_object_mut() {
            obj.insert(
                "_caller_worker_id".to_string(),
                serde_json::json!(worker.id.to_string()),
            );
        }
        let incoming_traceparent = traceparent.clone();
        let incoming_baggage = baggage.clone();
        // Pass the incoming caller context straight through so the downstream
        // `call <fn>` span nests directly under the caller's span instead of an
        // intermediate engine wrapper.
        let downstream_traceparent = traceparent;
        let downstream_baggage = baggage;

        tokio::spawn(
            async move {
                let result = engine
                    .remember_invocation(
                        &worker,
                        invocation_id,
                        &function_id,
                        namespace.as_deref(),
                        data,
                        downstream_traceparent,
                        downstream_baggage,
                        metadata,
                        caller,
                        observe_depth,
                    )
                    .await;

                if let Some(invocation_id) = invocation_id {
                    let current_ctx = tracing::Span::current().context();
                    let response_traceparent =
                        inject_traceparent_from_context(&current_ctx).or(incoming_traceparent);
                    let response_baggage =
                        inject_baggage_from_context(&current_ctx).or(incoming_baggage);

                    match result {
                        Ok(result) => match result {
                            Ok(result) => {
                                tracing::Span::current().record("otel.status_code", "OK");
                                engine
                                    .send_msg(
                                        &worker,
                                        Message::InvocationResult {
                                            invocation_id,
                                            function_id,
                                            result,
                                            error: None,
                                            traceparent: response_traceparent,
                                            baggage: response_baggage,
                                            observed: Vec::new(),
                                        },
                                    )
                                    .await;
                            }
                            Err(err) => {
                                tracing::Span::current().record("otel.status_code", "ERROR");
                                engine
                                    .send_msg(
                                        &worker,
                                        Message::InvocationResult {
                                            invocation_id,
                                            function_id,
                                            result: None,
                                            error: Some(err),
                                            traceparent: response_traceparent,
                                            baggage: response_baggage,
                                            observed: Vec::new(),
                                        },
                                    )
                                    .await;
                            }
                        },
                        Err(err) => {
                            tracing::Span::current().record("otel.status_code", "ERROR");
                            tracing::error!(error = ?err, "Error remembering invocation");
                            engine
                                .send_msg(
                                    &worker,
                                    Message::InvocationResult {
                                        invocation_id,
                                        function_id,
                                        result: None,
                                        error: Some(ErrorBody {
                                            code: "invocation_error".into(),
                                            message: err.to_string(),
                                            stacktrace: None,
                                        }),
                                        traceparent: response_traceparent,
                                        baggage: response_baggage,
                                        observed: Vec::new(),
                                    },
                                )
                                .await;
                        }
                    }

                    worker.remove_invocation(&invocation_id).await;
                }
            }
            .instrument(span),
        );
    }

    /// Starts buffering `worker`'s registrations until its namespace is known,
    /// and arms the [registration namespace grace](Engine::registration_namespace_grace)
    /// fallback.
    ///
    /// Called once per accepted WS connection. Connections that never call this
    /// (in-process workers, tests driving `router_msg` directly) keep
    /// registering immediately.
    #[doc(hidden)]
    pub fn begin_namespace_resolution(&self, worker: &WorkerConnection) {
        self.namespace_states
            .insert(worker.id, NamespaceState::Pending(VecDeque::new()));

        let engine = self.clone();
        let worker = worker.clone();
        let grace = self.registration_namespace_grace();
        let worker_id = worker.id;
        let handle = tokio::spawn(async move {
            tokio::time::sleep(grace).await;
            // `resolve_connection_namespace` transitions `Pending` -> `Draining`
            // atomically and returns whether THIS call won. Warn only when the
            // timer actually performed the fallback — never on a false positive
            // where `engine::workers::register` resolved the namespace first.
            let fired = engine
                .resolve_connection_namespace(&worker, DEFAULT_NAMESPACE)
                .await;
            if fired {
                tracing::warn!(
                    worker_id = %worker.id,
                    grace_ms = grace.as_millis() as u64,
                    "namespace registration grace of {}ms expired without an \
                     `engine::workers::register` announce; drained this connection's \
                     buffered registrations into the `{}` namespace. To allow more time, \
                     raise the global `registration_namespace_grace_ms` config \
                     or set the `III_NAMESPACE_GRACE_MS` env var.",
                    grace.as_millis(),
                    DEFAULT_NAMESPACE,
                );
            }
        });
        self.namespace_grace_tasks.insert(worker_id, handle);
    }

    /// Cancel the namespace-grace timer task for `worker_id`, if still armed.
    /// Called once the namespace is fixed by `engine::workers::register`, on a
    /// fatal registration rejection, and from `cleanup_worker` — so the timer
    /// and its captured `engine`/`worker` clones drop immediately and can never
    /// fire after the connection is done with. Distinct from
    /// [`Self::abort_namespace_resolution`], which flips the connection *state*
    /// to `Aborted`; this cancels the spawned sleep task itself.
    pub(crate) fn cancel_namespace_grace_timer(&self, worker_id: &Uuid) {
        if let Some((_, handle)) = self.namespace_grace_tasks.remove(worker_id) {
            handle.abort();
        }
    }

    /// The namespace `worker`'s registrations belong to.
    ///
    /// Deliberately ignores `worker.namespace`. The `WorkerConnection` handed
    /// to `router_msg` and `cleanup_worker` is a snapshot taken at connect
    /// time, and `engine::workers::register` writes the namespace into the
    /// *registry's* copy, not that one — so reading the field would silently
    /// yield `None` for the entire life of every WS connection. The resolution
    /// state is the only source that reflects what the worker declared.
    ///
    /// No entry means a connection that never entered the state machine
    /// (in-process workers, direct `router_msg` callers); those have no
    /// declared namespace, so they belong in [`DEFAULT_NAMESPACE`].
    fn connection_namespace(&self, worker: &WorkerConnection) -> String {
        match self.namespace_states.get(&worker.id).as_deref() {
            Some(
                NamespaceState::Resolved(ns)
                | NamespaceState::Draining(ns, _)
                | NamespaceState::Aborted(ns),
            ) => ns.clone(),
            // `Pending` is unreachable: `router_msg` buffers those messages
            // instead of dispatching them.
            _ => DEFAULT_NAMESPACE.to_string(),
        }
    }

    /// The namespace this connection was already committed to, or `None` while it
    /// is still `Pending`. Unlike [`Self::connection_namespace`], this
    /// distinguishes "not yet fixed" (`Pending` → `None`) from "fixed to
    /// `default`" (`Some("default")`), which is what tells a *late*
    /// `engine::workers::register` (arriving after the grace timer fired) apart
    /// from the normal in-time one.
    pub(crate) fn already_fixed_namespace(&self, worker_id: &Uuid) -> Option<String> {
        match self.namespace_states.get(worker_id).as_deref() {
            Some(
                NamespaceState::Resolved(ns)
                | NamespaceState::Draining(ns, _)
                | NamespaceState::Aborted(ns),
            ) => Some(ns.clone()),
            _ => None,
        }
    }

    /// Queues `msg` when the connection's namespace is not known yet.
    /// Returns `true` when the message was queued and must not be dispatched.
    fn buffer_until_namespace_known(&self, worker: &WorkerConnection, msg: &Message) -> bool {
        if !is_namespaced_registration(msg) {
            return false;
        }
        let Some(mut state) = self.namespace_states.get_mut(&worker.id) else {
            return false;
        };
        match &mut *state {
            NamespaceState::Pending(queue) | NamespaceState::Draining(_, queue) => {
                queue.push_back(msg.clone());
                true
            }
            NamespaceState::Resolved(_) => false,
            // Teardown has started; swallow rather than dispatch, so a late
            // message cannot register into a connection being cleaned up.
            NamespaceState::Aborted(_) => true,
        }
    }

    /// Marks the connection as torn down, so a drain racing `cleanup_worker`
    /// stops instead of registering functions cleanup has already released.
    ///
    /// Returns without effect when the connection was never tracked.
    #[doc(hidden)]
    pub fn abort_namespace_resolution(&self, worker: &WorkerConnection) {
        let Some(mut state) = self.namespace_states.get_mut(&worker.id) else {
            return;
        };
        // A connection torn down before its namespace ever resolved has no
        // declared namespace; nothing it queued will be registered, so the
        // value only has to be consistent for the teardown that follows.
        let namespace = match &*state {
            NamespaceState::Resolved(ns)
            | NamespaceState::Draining(ns, _)
            | NamespaceState::Aborted(ns) => ns.clone(),
            NamespaceState::Pending(_) => DEFAULT_NAMESPACE.to_string(),
        };
        *state = NamespaceState::Aborted(namespace);
    }

    /// Pins the connection to `namespace` and replays everything buffered for
    /// it, in arrival order.
    ///
    /// Racing callers — `engine::workers::register` and the grace timer — are
    /// serialized by the `Pending` -> `Draining` transition below: exactly one
    /// observes `Pending`, and the loser returns without draining. First writer
    /// wins, and a connection never leaves `Draining`/`Resolved` afterwards.
    ///
    /// Both the transition and the per-message pop re-read the state, so a
    /// concurrent `abort_namespace_resolution` (i.e. `cleanup_worker`) stops
    /// the drain at the next message rather than registering into a connection
    /// that is being torn down.
    ///
    /// Returns `true` when THIS call performed the `Pending` -> `Draining`
    /// transition (i.e. won the race). The grace timer uses this to warn only
    /// when the timeout actually fired — never when `engine::workers::register`
    /// resolved the namespace first.
    #[doc(hidden)]
    pub async fn resolve_connection_namespace(
        &self,
        worker: &WorkerConnection,
        namespace: &str,
    ) -> bool {
        {
            let Some(mut state) = self.namespace_states.get_mut(&worker.id) else {
                return false;
            };
            let NamespaceState::Pending(queue) = &mut *state else {
                return false;
            };
            let queue = std::mem::take(queue);
            *state = NamespaceState::Draining(namespace.to_string(), queue);
        }

        tracing::debug!(
            worker_id = %worker.id,
            namespace = %namespace,
            "Connection namespace resolved; draining buffered registrations"
        );

        loop {
            // The map guard is taken and dropped around each message, never
            // held across the dispatch await: messages arriving mid-drain must
            // be able to append to the queue we are consuming.
            let next = {
                let Some(mut state) = self.namespace_states.get_mut(&worker.id) else {
                    return true;
                };
                let NamespaceState::Draining(ns, queue) = &mut *state else {
                    return true;
                };
                match queue.pop_front() {
                    Some(msg) => msg,
                    None => {
                        // Publish `Resolved` only with an empty queue, so no
                        // arrival can bypass the drain and overtake it.
                        *state = NamespaceState::Resolved(ns.clone());
                        return true;
                    }
                }
            };

            // Behavior change worth naming: on the direct path a failing
            // registration propagates out of `router_msg` via `?` and drops the
            // connection. A buffered one cannot — the drain runs on the
            // `engine::workers::register` task (or the grace timer), where
            // there is no connection to drop and returning early would strand
            // every message still queued behind this one. So the drain logs and
            // continues, and one bad registration no longer kills a worker that
            // happened to register before its namespace was known.
            if let Err(err) = self.dispatch_msg(worker, &next).await {
                tracing::warn!(
                    worker_id = %worker.id,
                    error = ?err,
                    "Buffered registration failed to apply"
                );
            }
        }
    }

    /// Routes one frame read from a worker's socket. A report is taken by
    /// value: its payloads move into the activations instead of being copied.
    async fn route_frame(
        &self,
        worker: &WorkerConnection,
        msg: Message,
        hold: &mut crate::observer::ReportHold,
    ) -> anyhow::Result<()> {
        match msg {
            Message::InvocationsObserved { items } => {
                let full = self.observers.report(
                    worker.id,
                    &self.connection_namespace(worker),
                    &worker.routed_answers,
                    items,
                );
                // Backpressure: nothing more is read from this worker until
                // the queues its report overflowed have room again, and at
                // most four fifths of the time (`hold`).
                if !full.is_empty() {
                    self.observers.wait_for_room(hold, &full).await;
                }
                Ok(())
            }
            // A (un)registration made while the worker handles an activation
            // is part of that chain: `engine::on-functions-changed` takes its
            // depth (see `WorkerConnection::inflight_depth`).
            msg @ (Message::RegisterFunction { .. } | Message::UnregisterFunction { .. }) => {
                match worker.inflight_depth() {
                    Some(depth) => {
                        crate::observer::InvocationCtx::scope(
                            Arc::new(crate::observer::InvocationCtx::engine_internal_at(depth)),
                            self.router_msg_owned(worker, msg),
                        )
                        .await
                    }
                    None => self.router_msg_owned(worker, msg).await,
                }
            }
            msg => self.router_msg_owned(worker, msg).await,
        }
    }

    #[doc(hidden)]
    pub async fn router_msg(&self, worker: &WorkerConnection, msg: &Message) -> anyhow::Result<()> {
        if self.buffer_until_namespace_known(worker, msg) {
            tracing::debug!(
                worker_id = %worker.id,
                "Buffering registration until the connection namespace is known"
            );
            return Ok(());
        }
        self.dispatch_msg(worker, msg).await
    }

    // WebSocket decoding already owns the message. Preserve that ownership so
    // dispatch can forward request and response JSON without copying it.
    async fn router_msg_owned(
        &self,
        worker: &WorkerConnection,
        msg: Message,
    ) -> anyhow::Result<()> {
        if self.buffer_until_namespace_known(worker, &msg) {
            tracing::debug!(
                worker_id = %worker.id,
                "Buffering registration until the connection namespace is known"
            );
            return Ok(());
        }
        self.dispatch_msg_inner(worker, Cow::Owned(msg)).await
    }

    /// Applies `msg` unconditionally. The drain calls this directly to bypass
    /// the buffer check that `router_msg` performs.
    async fn dispatch_msg(&self, worker: &WorkerConnection, msg: &Message) -> anyhow::Result<()> {
        self.dispatch_msg_inner(worker, Cow::Borrowed(msg)).await
    }

    async fn dispatch_msg_inner(
        &self,
        worker: &WorkerConnection,
        msg: Cow<'_, Message>,
    ) -> anyhow::Result<()> {
        match msg.as_ref() {
            Message::InvokeFunction {
                invocation_id,
                function_id,
                data,
                traceparent,
                baggage,
                action,
                metadata,
                namespace,
                caller,
                observe_depth,
                activation,
            } => {
                // A call through an observer (v2 §4.6): checked against the
                // observer and routed to its stored target. It runs like an
                // activation, with no session: creation-time RBAC on the
                // observed function and the target is the gate, and the
                // request's `function_id` and namespace are not read (D13).
                if let Some(activation) = activation {
                    let engine = self.clone();
                    let worker = worker.clone();
                    let mut data = data.clone();
                    if let Some(obj) = data.as_object_mut() {
                        obj.insert(
                            "_caller_worker_id".to_string(),
                            serde_json::json!(worker.id.to_string()),
                        );
                    }
                    // Nobody waits for the answer to a `void` call, or to one
                    // without an id: the engine drops a refusal of it.
                    let answer = (*invocation_id)
                        .filter(|_| !matches!(action, Some(crate::protocol::CallAction::Void)));
                    let call = crate::observer::ObserverCall {
                        invocation_id: *invocation_id,
                        data,
                        caller: caller.clone(),
                        observe_depth: *observe_depth,
                        activation: activation.clone(),
                        traceparent: traceparent.clone(),
                        baggage: baggage.clone(),
                        fire_and_forget: answer.is_none(),
                    };
                    let function_id = function_id.clone();
                    tokio::spawn(async move {
                        let outcome = engine.observers.observer_call(&worker, call).await;
                        if let Some(invocation_id) = answer {
                            let (result, error) = match outcome {
                                Ok(result) => (result, None),
                                Err(error) => (None, Some(error)),
                            };
                            engine
                                .send_msg(
                                    &worker,
                                    Message::InvocationResult {
                                        invocation_id,
                                        function_id,
                                        result,
                                        error,
                                        traceparent: None,
                                        baggage: None,
                                        observed: Vec::new(),
                                    },
                                )
                                .await;
                            worker.remove_invocation(&invocation_id).await;
                        }
                    });
                    return Ok(());
                }
                tracing::debug!(
                    worker_id = %worker.id,
                    invocation_id = %crate::logging::display_option(invocation_id),
                    function_id = %function_id,
                    namespace = %crate::logging::display_option(namespace),
                    traceparent = %crate::logging::display_option(traceparent),
                    baggage = %crate::logging::display_option(baggage),
                    action = ?action,
                    payload = ?data,
                    "InvokeFunction"
                );

                // A named-but-empty namespace is a mistake, not a request for
                // `default`. Answering the caller beats resolving somewhere
                // they did not ask for: the call would otherwise land in
                // whatever the empty string routes to and look like a miss.
                if crate::protocol::is_blank_namespace(namespace) {
                    if let Some(invocation_id) = invocation_id {
                        let _ = self
                            .send_msg(
                                worker,
                                Message::InvocationResult {
                                    invocation_id: *invocation_id,
                                    function_id: function_id.clone(),
                                    result: None,
                                    error: Some(crate::protocol::ErrorBody::new(
                                        crate::protocol::INVALID_NAMESPACE,
                                        blank_namespace_message("namespace"),
                                    )),
                                    traceparent: None,
                                    baggage: None,
                                    observed: Vec::new(),
                                },
                            )
                            .await;
                    } else {
                        tracing::warn!(
                            worker_id = %worker.id,
                            function_id = %function_id,
                            "InvokeFunction carried an empty namespace; dropped"
                        );
                    }
                    return Ok(());
                }

                if let Some(session) = &worker.session {
                    // Resolve strictly in the requested namespace first: RBAC is
                    // still keyed by function id, but the `Function` it inspects
                    // must be the one this invoke would actually reach. A miss
                    // hands RBAC `None` exactly as before, so a forbidden id that
                    // does not exist still answers FORBIDDEN rather than leaking
                    // its absence.
                    if !self.is_function_allowed(session, namespace.as_deref(), function_id) {
                        let inv_id = (*invocation_id).unwrap_or_else(Uuid::new_v4);
                        let explicitly_forbidden =
                            session.forbidden_functions.iter().any(|f| f == function_id);
                        let remediation = if explicitly_forbidden {
                            "remove from rbac.forbidden_functions"
                        } else {
                            "add to rbac.expose_functions"
                        };
                        self.send_msg(
                            worker,
                            Message::InvocationResult {
                                invocation_id: inv_id,
                                function_id: function_id.clone(),
                                result: None,
                                error: Some(ErrorBody::new(
                                    "FORBIDDEN",
                                    format!(
                                        "function '{}' not allowed ({})",
                                        function_id, remediation
                                    ),
                                )),
                                traceparent: traceparent.clone(),
                                baggage: baggage.clone(),
                                observed: Vec::new(),
                            },
                        )
                        .await;
                        return Ok(());
                    }

                    // The namespace a connection declares is an authorization
                    // decision, not a preference. Once the auth function scopes
                    // a session to a set of namespaces, registering outside them
                    // is refused here — before the lease is claimed, so nothing
                    // has to be undone. An unscoped session (empty map) declares
                    // freely, which is every deployment that has not adopted
                    // scoped grants.
                    if function_id == "engine::workers::register" && !session.namespaces.is_empty()
                    {
                        let declared = data
                            .get("namespace")
                            .and_then(|v| v.as_str())
                            .unwrap_or(crate::protocol::DEFAULT_NAMESPACE);
                        if !session.namespaces.contains_key(declared) {
                            let mut permitted: Vec<&str> =
                                session.namespaces.keys().map(String::as_str).collect();
                            permitted.sort_unstable();
                            // A caller that awaits the call still gets its error.
                            if let Some(inv_id) = *invocation_id {
                                self.send_msg(
                                    worker,
                                    Message::InvocationResult {
                                        invocation_id: inv_id,
                                        function_id: function_id.clone(),
                                        result: None,
                                        error: Some(ErrorBody::new(
                                            "FORBIDDEN",
                                            format!(
                                                "namespace '{}' not allowed for this session \
                                                 (permitted: {})",
                                                declared,
                                                permitted.join(", ")
                                            ),
                                        )),
                                        traceparent: traceparent.clone(),
                                        baggage: baggage.clone(),
                                        observed: Vec::new(),
                                    },
                                )
                                .await;
                            }
                            // SDKs register fire-and-forget, so the refusal must
                            // also end the connection: otherwise the worker keeps
                            // running, and registering, in `default`. Drop what it
                            // buffered and anything it sends next first, so a peer
                            // that never acknowledges the Close can't have the grace
                            // timer drain its registrations into `default`.
                            self.cancel_namespace_grace_timer(&worker.id);
                            self.abort_namespace_resolution(worker);
                            self.send_msg(
                                worker,
                                Message::RegistrationRejected {
                                    code: NAMESPACE_FORBIDDEN.to_string(),
                                    namespace: declared.to_string(),
                                    worker_name: data
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .map(str::to_string),
                                    function_id: None,
                                    owner_worker_id: String::new(),
                                },
                            )
                            .await;
                            let _ = worker
                                .channel
                                .send(Outbound::Raw(WsMessage::Close(None)))
                                .await;
                            return Ok(());
                        }
                    }

                    // Bypass middleware for the engine's own `engine::*` builtins
                    // (introspection, logging, queue ops). Proxying those through
                    // the operator's middleware re-invokes them from the middleware
                    // worker's connection, discarding the caller's RBAC session so
                    // a filtered surface like `engine::functions::list` leaks every
                    // function. A worker-registered `engine::foo` still owns a
                    // `function_owners` entry, so it does NOT bypass — the operator's
                    // middleware keeps intercepting user code.
                    if let Some(middleware_id) = &session.config.middleware_function_id
                        && !self
                            .is_engine_owned_builtin(effective_namespace(namespace), function_id)
                    {
                        let inv_id = (*invocation_id).unwrap_or_else(Uuid::new_v4);
                        // Resolve the middleware in the caller's namespace and hand
                        // it that namespace, so a namespaced invoke reaches the
                        // matching middleware and can re-target the same namespace.
                        let ns = effective_namespace(namespace).to_string();
                        let middleware_input = serde_json::json!({
                            "function_id": function_id,
                            "payload": data,
                            "action": action,
                            "context": session.context,
                            "namespace": ns,
                        });
                        let engine = self.clone();
                        let w = worker.clone();
                        let middleware_id = middleware_id.clone();
                        let function_id = function_id.clone();
                        let traceparent = traceparent.clone();
                        let baggage = baggage.clone();
                        let middleware_metadata = metadata.clone();

                        tokio::spawn(async move {
                            let response = match engine
                                .call_value_with_metadata_ns(
                                    &ns,
                                    &middleware_id,
                                    middleware_input,
                                    middleware_metadata,
                                )
                                .await
                            {
                                Ok(result) => Message::InvocationResult {
                                    invocation_id: inv_id,
                                    function_id,
                                    result,
                                    error: None,
                                    traceparent,
                                    baggage,
                                    observed: Vec::new(),
                                },
                                Err(err) => Message::InvocationResult {
                                    invocation_id: inv_id,
                                    function_id,
                                    result: None,
                                    error: Some(err),
                                    traceparent,
                                    baggage,
                                    observed: Vec::new(),
                                },
                            };
                            engine.send_msg(&w, response).await;
                        });
                        return Ok(());
                    }
                }

                match action {
                    Some(crate::protocol::CallAction::Enqueue { queue }) => {
                        let engine = self.clone();
                        let worker = worker.clone();
                        let invocation_id = *invocation_id;
                        let function_id = function_id.to_string();
                        let queue = queue.to_string();
                        let message_receipt_id = Uuid::new_v4().to_string();
                        let data = data.clone();
                        // Capture the invoke's namespace so the queue provider
                        // enqueues (and the consumer later resolves) the target
                        // in the caller's namespace rather than `default`.
                        let target_namespace = effective_namespace(namespace).to_string();
                        let traceparent = traceparent.clone();
                        let baggage = baggage.clone();
                        let queued_baggage = crate::telemetry::baggage_with_function_id(
                            baggage.as_deref(),
                            &function_id,
                        );

                        let span = {
                            // Parent context must be on `Context::current()`
                            // BEFORE span creation; `set_parent` after is too
                            // late for `SpanProcessor::on_start`.
                            let parent_cx = crate::telemetry::extract_context(
                                traceparent.as_deref(),
                                baggage.as_deref(),
                            );
                            // The enqueuer's baggage names the enqueuer; rewrite
                            // `iii.function.id` to the function being enqueued so
                            // `BaggageSpanProcessor` stamps this span with the
                            // target, not the caller (the `fn_queue` consumer span
                            // gets the same correction via the stored baggage).
                            let parent_cx = crate::telemetry::with_function_id_baggage(
                                &parent_cx,
                                &function_id,
                            );
                            let _guard = parent_cx.attach();
                            tracing::info_span!(
                                "enqueue_action",
                                otel.name = %format!("enqueue {} → {}", function_id, queue),
                                function_id = %function_id,
                                queue = %queue,
                            )
                        };

                        tokio::spawn(
                            async move {
                                let has_target_provider = engine
                                    .functions
                                    .get(&target_namespace, ENQUEUE_PROVIDER_FUNCTION_ID)
                                    .is_some();
                                let has_legacy_default_provider = target_namespace
                                    != DEFAULT_NAMESPACE
                                    && engine
                                        .functions
                                        .get(DEFAULT_NAMESPACE, ENQUEUE_PROVIDER_FUNCTION_ID)
                                        .is_some();

                                // A legacy provider in `default` rejects the
                                // additive namespace field. Omitting the field
                                // would enqueue the target function in the
                                // wrong namespace, so fail before invoking it.
                                let result = if !has_target_provider && has_legacy_default_provider
                                {
                                    Err(ErrorBody::new(
                                        "queue_provider_namespace_unsupported",
                                        format!(
                                            "The queue provider in `{DEFAULT_NAMESPACE}` cannot \
                                             enqueue functions in namespace `{target_namespace}`. \
                                             Update the queue worker to a namespace-aware release \
                                             and register it in `{target_namespace}`."
                                        ),
                                    ))
                                } else {
                                    let mut enqueue_input = serde_json::json!({
                                        "queue": queue.clone(),
                                        "function_id": function_id.clone(),
                                        "data": data,
                                        "messageReceiptId": message_receipt_id.clone(),
                                        "traceparent": traceparent.clone(),
                                        "baggage": queued_baggage,
                                    });
                                    // The default namespace remains compatible
                                    // with old providers. A scoped provider gets
                                    // the namespace needed by its consumer.
                                    if target_namespace != DEFAULT_NAMESPACE {
                                        enqueue_input["namespace"] =
                                            Value::String(target_namespace.clone());
                                    }
                                    engine
                                        .call_value_with_metadata_ns(
                                            &target_namespace,
                                            ENQUEUE_PROVIDER_FUNCTION_ID,
                                            enqueue_input,
                                            None,
                                        )
                                        .await
                                };

                                if let Some(invocation_id) = invocation_id {
                                    match result {
                                        Ok(_) => {
                                            engine
                                                .send_msg(
                                                    &worker,
                                                    Message::InvocationResult {
                                                        invocation_id,
                                                        function_id: function_id.clone(),
                                                        result: Some(serde_json::json!({
                                                            "messageReceiptId": message_receipt_id
                                                        })),
                                                        error: None,
                                                        traceparent: traceparent.clone(),
                                                        baggage: baggage.clone(),
                                                        observed: Vec::new(),
                                                    },
                                                )
                                                .await;
                                        }
                                        Err(err) => {
                                            // DX: a `function_not_found` on the
                                            // provider means no queue worker is
                                            // installed. Turn the bare error into
                                            // an actionable fix instead of leaking
                                            // the internal provider id alone.
                                            let message = if err.code == "function_not_found" {
                                                format!(
                                                    "No queue provider is installed. \
                                                     `CallAction::Enqueue` routes through the \
                                                     `{}` provider, which the standalone queue \
                                                     worker registers. Add it with \
                                                     `iii call -n <compose-daemon-namespace> \
                                                     compose::add worker=queue`. \
                                                     (underlying: {})",
                                                    ENQUEUE_PROVIDER_FUNCTION_ID, err.message
                                                )
                                            } else {
                                                err.to_string()
                                            };
                                            engine
                                                .send_msg(
                                                    &worker,
                                                    Message::InvocationResult {
                                                        invocation_id,
                                                        function_id: function_id.clone(),
                                                        result: None,
                                                        error: Some(ErrorBody::new(
                                                            "enqueue_error",
                                                            message,
                                                        )),
                                                        traceparent: traceparent.clone(),
                                                        baggage: baggage.clone(),
                                                        observed: Vec::new(),
                                                    },
                                                )
                                                .await;
                                        }
                                    }
                                }
                            }
                            .instrument(span),
                        );

                        Ok(())
                    }

                    Some(crate::protocol::CallAction::Void) | None => {
                        // Void never sends a result, even if the caller supplied an ID.
                        let invocation_id = if action.is_none() {
                            *invocation_id
                        } else {
                            None
                        };
                        let Message::InvokeFunction {
                            function_id,
                            namespace,
                            data,
                            traceparent,
                            baggage,
                            metadata,
                            caller,
                            observe_depth,
                            ..
                        } = msg.into_owned()
                        else {
                            unreachable!("matched InvokeFunction above");
                        };
                        self.spawn_invoke_function(
                            worker,
                            function_id,
                            namespace,
                            data,
                            traceparent,
                            baggage,
                            invocation_id,
                            metadata,
                            caller,
                            observe_depth,
                        );
                        Ok(())
                    }
                }
            }
            Message::InvocationResult {
                invocation_id,
                function_id,
                result,
                error,
                traceparent: _,
                baggage: _,
                observed,
            } => {
                tracing::debug!(
                    function_id = %function_id,
                    invocation_id = %invocation_id,
                    result = ?result,
                    error = ?error,
                    observed = observed.len(),
                    "InvocationResult"
                );

                // A routed call an `observe.v2` host answered: its report
                // items for it are refused after the answer too (decision 38).
                if worker.remove_invocation(invocation_id).await && worker.features.observe_v2() {
                    worker.routed_answers.record(*invocation_id);
                }

                if let Some(invocation) = self.invocations.remove(invocation_id) {
                    let Message::InvocationResult {
                        result,
                        error,
                        observed,
                        ..
                    } = msg.into_owned()
                    else {
                        unreachable!("matched InvocationResult above");
                    };
                    // An `observe.v2` host delivered the call itself: the
                    // routed hook activates the observers it lists (v2
                    // §8.2). The list is read from no other connection, and
                    // never reaches the caller.
                    let delivered =
                        worker
                            .features
                            .observe_v2()
                            .then_some(crate::observer::HostDelivery {
                                host: worker.id,
                                observed,
                            });
                    let result = match error {
                        Some(err) => Err(err),
                        None => Ok(result),
                    };
                    let _ = invocation
                        .sender
                        .send(crate::invocation::Completion { result, delivered });
                    return Ok(());
                } else {
                    // Expected when the caller disconnected before the
                    // executor finished (client-side call timeout,
                    // Ctrl-C, etc). `cleanup_worker` already halted the
                    // invocation, so the late result has nowhere to go.
                    // Logged at debug: it is a normal condition, and a
                    // slow call such as sandbox::exec meets it routinely.
                    tracing::debug!(
                        invocation_id = %invocation_id,
                        "Did not find caller for invocation (caller already disconnected)"
                    );
                }
                Ok(())
            }
            Message::UnregisterFunction { id } => {
                tracing::debug!(
                    function_id = %id,
                    "UnregisterFunction"
                );

                let resolved_id = resolve_registration_id(worker, id);
                // Resolved once: every removal below must target the same
                // namespace the registration was written to, and re-reading it
                // per call site invites the two halves to drift apart.
                let namespace = self.connection_namespace(worker);
                if worker.has_external_function_id(&resolved_id).await {
                    worker.remove_external_function_id(&resolved_id).await;
                    // Only tear down the engine-global registration if this
                    // worker is still the recorded owner, checked and removed
                    // under the owner entry's lock, then released: a fresher
                    // worker that took the id over keeps its http_module and
                    // service_registry entries and its observe descriptor —
                    // the same bug shape `cleanup_worker` guards against.
                    let http_module = self
                        .service_registry
                        .get_service::<HttpFunctionsWorker>("http_functions");
                    if !self.remove_external_function_if_owner(
                        &worker.id,
                        &namespace,
                        &resolved_id,
                        http_module.as_deref(),
                    ) {
                        tracing::debug!(
                            worker_id = %worker.id,
                            function_id = %id,
                            "Skipping external UnregisterFunction — owner changed"
                        );
                        return Ok(());
                    }
                    if http_module.is_some() {
                        self.service_registry
                            .remove_function_from_services(&namespace, &resolved_id);
                    }
                    self.release_external_function_if_owner(&worker.id, &namespace, &resolved_id);
                    tracing::debug!(
                        worker_id = %worker.id,
                        function_id = %id,
                        "Unregistered external function"
                    );
                } else {
                    worker.remove_function_id(&resolved_id).await;
                    // Same ownership gate as the external branch above.
                    if !self.release_function_if_owner(&worker.id, &namespace, &resolved_id) {
                        tracing::debug!(
                            worker_id = %worker.id,
                            function_id = %id,
                            "Skipping UnregisterFunction — owner changed"
                        );
                    }
                }

                Ok(())
            }
            Message::RegisterFunction {
                id,
                description,
                request_format: req,
                response_format: res,
                metadata,
                invocation,
                args_format,
                private,
                observable,
                observe,
            } => {
                tracing::debug!(
                    worker_id = %worker.id,
                    function_id = %id,
                    description = ?description,
                    "RegisterFunction"
                );

                let mut reg_id = id.clone();
                let mut reg_description = description.clone();
                let mut reg_metadata = metadata.clone();
                // A 0.25 SDK says `observable: false` as `observe.allow: false`.
                let mut options = crate::observer::FunctionOptions {
                    args_format: args_format.clone(),
                    private: *private,
                    observable: *observable
                        && observe.as_ref().is_none_or(|descriptor| descriptor.allow()),
                };

                if let Some(session) = &worker.session {
                    if !session.allow_function_registration {
                        tracing::warn!(
                            worker_id = %worker.id,
                            function_id = %id,
                            "function registration not allowed for this session"
                        );
                        return Ok(());
                    }

                    if let Some(hook_fn_id) = session
                        .config
                        .rbac
                        .as_ref()
                        .and_then(|c| c.on_function_registration_function_id.as_ref())
                    {
                        let mut hook_input = serde_json::json!({
                            "function_id": id,
                            "description": description,
                            "metadata": metadata,
                            // The namespace this function registers in (RegisterFunction
                            // is buffered until the connection namespace resolves, so
                            // this is the real one), letting the hook authorize per
                            // namespace since the same id can exist in several.
                            "namespace": self.connection_namespace(worker),
                            "private": options.private,
                            "observable": options.observable,
                            "context": session.context,
                        });
                        if let Some(schema) = &options.args_format {
                            hook_input["args_format"] = schema.clone();
                        }
                        match self
                            .call_value_with_metadata_ns(
                                DEFAULT_NAMESPACE,
                                hook_fn_id,
                                hook_input,
                                None,
                            )
                            .await
                        {
                            Ok(Some(v)) if v.is_object() => {
                                if let Some(s) = v.get("function_id").and_then(|v| v.as_str()) {
                                    reg_id = s.to_string();
                                }
                                if let Some(s) = v.get("description").and_then(|v| v.as_str()) {
                                    reg_description = Some(s.to_string());
                                }
                                if let Some(m) = v.get("metadata").cloned() {
                                    reg_metadata = Some(m);
                                }
                                if let Some(b) = v.get("private").and_then(|v| v.as_bool()) {
                                    options.private = b;
                                }
                                if let Some(b) = v.get("observable").and_then(|v| v.as_bool()) {
                                    options.observable = b;
                                }
                                if let Some(schema) = v.get("args_format") {
                                    options.args_format =
                                        (!schema.is_null()).then(|| schema.clone());
                                }
                            }
                            other => {
                                tracing::warn!(
                                    worker_id = %worker.id,
                                    function_id = %id,
                                    result = ?other,
                                    "function registration denied by hook"
                                );
                                return Ok(());
                            }
                        }
                    }
                }

                reg_id = resolve_registration_id(worker, &reg_id);

                let namespace = self.connection_namespace(worker);

                // `engine::*` is reserved for engine/builtin infrastructure, which
                // lives in `default`. Refuse a worker registering an `engine::*`
                // id in any other namespace: otherwise it would inherit the
                // infrastructure RBAC carve-out AND the `engine::*` middleware
                // bypass (both keyed by bare id) that are meant only for the real
                // builtins. In `default` the claim below refuses an id the engine
                // hosts (`claim_in`); outside it there is nothing to conflict
                // with, so reserve the prefix here.
                if reg_id.starts_with("engine::")
                    && namespace != DEFAULT_NAMESPACE
                    && !is_namespace_scoped_queue_function(&reg_id)
                {
                    tracing::warn!(
                        worker_id = %worker.id,
                        function_id = %reg_id,
                        namespace = %namespace,
                        "refusing a reserved `engine::*` function id outside the `default` namespace"
                    );
                    // Tell the worker (non-fatal, same shape as the conflict path):
                    // otherwise it believes the registration succeeded and only
                    // learns otherwise when invocations return function_not_found.
                    self.send_msg(
                        worker,
                        Message::RegistrationRejected {
                            code: FUNCTION_NAMESPACE_CONFLICT.to_string(),
                            namespace: namespace.clone(),
                            worker_name: None,
                            function_id: Some(reg_id.clone()),
                            // Reserved-prefix refusal, not a real conflict: no
                            // worker owns `engine::*` (the engine reserves it), so
                            // don't name the rejected worker as its own owner.
                            owner_worker_id: String::new(),
                        },
                    )
                    .await;
                    return Ok(());
                }

                // Options the engine cannot honour refuse this one
                // registration, before anything is claimed: a 0.25 `observe`
                // object asking for a delivery other than `auto`, and an
                // `args_format` that is not a JSON Schema.
                let refusal = if observe
                    .as_ref()
                    .is_some_and(|descriptor| !descriptor.is_supported())
                {
                    Some((
                        crate::protocol::observe_codes::UNSUPPORTED,
                        "`observe.delivery` other than `auto`: this engine delivers every \
                         function's calls one way; register the function with \
                         `args_format` and `private` instead"
                            .to_string(),
                    ))
                } else {
                    options.args_format.as_ref().and_then(|schema| {
                        crate::observer::validate_args_format(schema)
                            .err()
                            .map(|reason| {
                                (crate::protocol::observe_codes::INVALID_ARGS_FORMAT, reason)
                            })
                    })
                };
                if let Some((code, reason)) = refusal {
                    tracing::warn!(
                        worker_id = %worker.id,
                        function_id = %reg_id,
                        code = %code,
                        reason = %reason,
                        "refusing a registration"
                    );
                    self.send_msg(
                        worker,
                        Message::RegistrationRejected {
                            code: code.to_string(),
                            namespace: namespace.clone(),
                            worker_name: None,
                            function_id: Some(reg_id.clone()),
                            owner_worker_id: String::new(),
                        },
                    )
                    .await;
                    return Ok(());
                }

                // Claim ownership BEFORE mutating any engine-global state. An
                // old worker's `cleanup_worker` running on another task can
                // see the pre-claim `function_owners` entry, match its own
                // id, and tear down the registration we're about to write.
                // Claiming first makes the CAS release in cleanup see the new
                // owner and bail out for every subsequent step.
                //
                // It is also the gate: a rejected claim must leave every
                // registry untouched, so nothing below this point runs.
                let claim = if invocation.is_some() {
                    self.claim_external_function(&namespace, worker.id, &reg_id)
                } else {
                    self.claim_function(&namespace, worker.id, &reg_id)
                };
                if let Err(conflict) = claim {
                    // Unlike a worker-name conflict, this costs the worker one
                    // function, not its connection: the rest of its exports are
                    // still valid, so it stays connected and serving them.
                    self.send_msg(
                        worker,
                        Message::RegistrationRejected {
                            code: FUNCTION_NAMESPACE_CONFLICT.to_string(),
                            namespace: conflict.namespace,
                            worker_name: None,
                            function_id: Some(conflict.name),
                            owner_worker_id: conflict.owner_worker_id,
                        },
                    )
                    .await;
                    return Ok(());
                }

                self.service_registry
                    .register_service_from_function_id(&namespace, &reg_id);

                // Before the function lands in the registry: the observers
                // that registration activates must see these options.
                self.observers.set_options(&namespace, &reg_id, options);

                if let Some(invocation) = invocation {
                    let Some(http_module) = self
                        .service_registry
                        .get_service::<HttpFunctionsWorker>("http_functions")
                    else {
                        tracing::error!(
                            worker_id = %worker.id,
                            function_id = %reg_id,
                            "HTTP functions module not loaded"
                        );
                        self.release_external_function_if_owner(&worker.id, &namespace, &reg_id);
                        // Nothing was registered: neither are its options.
                        self.observers.set_options(
                            &namespace,
                            &reg_id,
                            crate::observer::FunctionOptions::default(),
                        );
                        return Ok(());
                    };

                    let config = HttpFunctionConfig {
                        function_path: reg_id.clone(),
                        url: invocation.url.clone(),
                        method: invocation.method.clone(),
                        timeout_ms: invocation.timeout_ms,
                        headers: invocation.headers.clone(),
                        auth: invocation.auth.clone(),
                        description: reg_description.clone(),
                        request_format: req.clone(),
                        response_format: res.clone(),
                        metadata: reg_metadata.clone(),
                        registered_at: Some(Utc::now()),
                        updated_at: None,
                    };

                    if let Err(err) = http_module.register_http_function(&namespace, config).await {
                        tracing::error!(
                            worker_id = %worker.id,
                            function_id = %reg_id,
                            error = ?err,
                            "Failed to register HTTP invocation function"
                        );
                        self.release_external_function_if_owner(&worker.id, &namespace, &reg_id);
                        self.observers.set_options(
                            &namespace,
                            &reg_id,
                            crate::observer::FunctionOptions::default(),
                        );
                        return Ok(());
                    }

                    worker.include_external_function_id(&reg_id).await;
                    return Ok(());
                }

                // Must match the namespace used for the service above, and the
                // one the unregister/cleanup paths resolve from the connection.
                self.register_function_ns(
                    &namespace,
                    RegisterFunctionRequest {
                        function_id: reg_id.clone(),
                        description: reg_description,
                        request_format: req.clone(),
                        response_format: res.clone(),
                        metadata: reg_metadata,
                    },
                    Box::new(worker.clone()),
                );

                worker.include_function_id(&reg_id).await;
                // The success ack an `observe.v1` owner gets, with the
                // function's observers (protocol.md §2.3).
                self.observers
                    .function_registered(self, worker, &namespace, &reg_id);
                Ok(())
            }
            Message::RegisterService {
                id,
                name,
                description,
                parent_service_id,
            } => {
                let effective_name = if name.is_empty() { &id } else { &name };
                tracing::debug!(
                    service_id = %id,
                    service_name = %effective_name,
                    description = ?description,
                    parent_service_id = ?parent_service_id,
                    "RegisterService"
                );
                let services = self
                    .service_registry
                    .services
                    .iter()
                    .map(|entry| entry.key().clone())
                    .collect::<Vec<_>>();
                tracing::debug!(services = ?services, "Current services");

                self.service_registry.insert_service(
                    &self.connection_namespace(worker),
                    Service::with_parent(
                        effective_name.to_string(),
                        id.clone(),
                        parent_service_id.clone(),
                    ),
                );

                Ok(())
            }
            Message::Reattach {
                previous_worker_id,
                reattach_token,
            } => {
                // First message of a reconnect replay: the worker presents
                // the identity of its previous connection so the engine can
                // retire it before processing the replayed registrations
                // that follow on this sequential read loop — they must land
                // on a clean slate instead of racing the old connection's
                // cleanup (the reconnect ownership kill loop).
                let Ok(prev) = Uuid::parse_str(previous_worker_id) else {
                    tracing::warn!(
                        worker_id = %worker.id,
                        previous_worker_id = %previous_worker_id,
                        "Reattach with malformed previous worker id; ignoring"
                    );
                    return Ok(());
                };
                if prev == worker.id {
                    return Ok(());
                }
                let Some(old) = self.worker_registry.get_worker(&prev) else {
                    // The previous connection is already torn down; its
                    // observers may still be in their grace period, and the
                    // registry checks the token it kept for them.
                    self.observers
                        .reattach(prev, reattach_token.as_deref(), worker);
                    return Ok(());
                };
                // Worker ids are public (engine::workers::list,
                // engine::on-workers-changed), so the id alone must never be
                // able to evict a live worker. The token was only ever sent
                // over the previous connection's own socket — presenting it
                // proves this is the same worker reconnecting.
                if reattach_token.as_deref() != Some(old.reattach_token.to_string().as_str()) {
                    tracing::warn!(
                        worker_id = %worker.id,
                        previous_worker = %prev,
                        "Reattach with wrong or missing token; ignoring"
                    );
                    return Ok(());
                }
                tracing::info!(
                    new_worker = %worker.id,
                    previous_worker = %prev,
                    "Reattach: retiring previous connection of reconnected worker"
                );
                // Evict FIRST: the old read loop exits at its next poll
                // (biased select checks eviction before draining more
                // frames), discarding any still-buffered stale
                // registrations, and its exit path runs the connection's
                // cleanup. Await that cleanup's COMPLETION — running it here
                // concurrently with a still-draining reader could snapshot
                // before an in-flight registration commits and orphan it.
                old.evict.notify_one();
                let cleaned = if old.has_reader.load(std::sync::atomic::Ordering::SeqCst) {
                    let mut done = old.cleanup_done.subscribe();
                    matches!(
                        tokio::time::timeout(REATTACH_EVICT_TIMEOUT, done.wait_for(|d| *d)).await,
                        Ok(Ok(_))
                    )
                } else {
                    false
                };
                if !cleaned {
                    // No reader on this connection (or it is wedged
                    // mid-message): tear down directly. The ownership guards
                    // in the release paths keep any late stale write from
                    // clobbering the replay that follows.
                    self.cleanup_worker(&old).await;
                }
                // The teardown suspended the previous connection's observers;
                // they move to this one, counters intact, before its replayed
                // `observe`s arrive.
                self.observers
                    .reattach(prev, reattach_token.as_deref(), worker);
                Ok(())
            }
            Message::Ping => {
                self.send_msg(worker, Message::Pong).await;
                Ok(())
            }
            Message::Pong => Ok(()),
            Message::WorkerRegistered { .. } => {
                // This message is sent from engine to worker, not the other way around
                // If we receive it here, just ignore it
                Ok(())
            }
            Message::RegistrationRejected { .. } => {
                // This message is sent from engine to worker, not the other way around
                // If we receive it here, just ignore it
                Ok(())
            }
            // Observe messages from workers (protocol.md §2.2).
            Message::Observe {
                id,
                source,
                target,
                metadata,
                max_fires,
                expires_in_ms,
                max_in_flight,
            } => {
                tracing::debug!(
                    worker_id = %worker.id,
                    observer_id = %id,
                    function_id = %source.function_id,
                    target = %target.function_id,
                    "Observe"
                );
                self.observers
                    .observe(
                        self,
                        worker,
                        crate::observer::ObserveRequest {
                            id: id.clone(),
                            source: (**source).clone(),
                            target: target.clone(),
                            metadata: metadata.clone(),
                            max_fires: *max_fires,
                            expires_in_ms: *expires_in_ms,
                            max_in_flight: *max_in_flight,
                        },
                    )
                    .await;
                Ok(())
            }
            Message::Unobserve { id } => {
                tracing::debug!(worker_id = %worker.id, observer_id = %id, "Unobserve");
                self.observers.unobserve(worker.id, id);
                Ok(())
            }
            Message::ObserverAck {
                function_id,
                namespace,
                id,
                revision,
                error,
            } => {
                tracing::debug!(
                    worker_id = %worker.id,
                    function_id = %function_id,
                    observer_id = %id,
                    revision,
                    rejected = error.is_some(),
                    "ObserverAck"
                );
                self.observers.observer_ack(
                    worker.id,
                    &self.connection_namespace(worker),
                    function_id,
                    namespace.as_deref(),
                    id,
                    *revision,
                    error.clone(),
                );
                Ok(())
            }
            Message::InvocationsObserved { items } => {
                let full = self.observers.report(
                    worker.id,
                    &self.connection_namespace(worker),
                    &worker.routed_answers,
                    items.clone(),
                );
                // Not read from a socket: one hold at most.
                if !full.is_empty() {
                    self.observers
                        .wait_for_room(&mut crate::observer::ReportHold::default(), &full)
                        .await;
                }
                Ok(())
            }
            Message::LocalInvocationStats {
                functions,
                dropped_reports,
                ..
            } => {
                self.observers.local_stats(
                    worker.id,
                    &self.connection_namespace(worker),
                    functions,
                    dropped_reports,
                );
                Ok(())
            }
            // Engine → worker messages. A worker sending one is ignored, like
            // `WorkerRegistered` above.
            Message::ObserveResult { .. }
            | Message::ObserverStateChanged { .. }
            | Message::FunctionRegistered { .. }
            | Message::ObserverAdded { .. }
            | Message::ObserverRemoved { .. } => Ok(()),
        }
    }

    // Callers that already own a JSON value do not need to serialize its tree again.
    pub(crate) async fn call_value_with_metadata_ns(
        &self,
        namespace: &str,
        function_id: &str,
        input: Value,
        metadata: Option<Value>,
    ) -> Result<Option<Value>, ErrorBody> {
        let function = self.internal_function_handler(namespace, function_id)?;
        self.call_handler_with_metadata(
            namespace,
            function_id.to_owned(),
            function,
            input,
            metadata,
        )
        .await
    }

    fn internal_function_handler(
        &self,
        namespace: &str,
        function_id: &str,
    ) -> Result<Arc<crate::function::HandlerFn>, ErrorBody> {
        self.functions
            .get_handler(namespace, function_id)
            .ok_or_else(|| ErrorBody {
                code: "function_not_found".into(),
                message: format!(
                    "Function {} not found in namespace {}",
                    function_id, namespace
                ),
                stacktrace: None,
            })
    }

    fn call_handler_with_metadata<'a>(
        &'a self,
        namespace: &'a str,
        function_id: String,
        function: Arc<crate::function::HandlerFn>,
        input: Value,
        metadata: Option<Value>,
    ) -> impl Future<Output = Result<Option<Value>, ErrorBody>> + Send + 'a {
        // Use the current tracing context to preserve parent-child links.
        let ctx = tracing::Span::current().context();
        let traceparent = inject_traceparent_from_context(&ctx);
        let baggage = inject_baggage_from_context(&ctx);
        // An engine-internal call made inside an observed chain keeps the
        // chain's depth.
        let invocation_ctx = crate::observer::InvocationCtx::engine_internal_at(
            crate::observer::InvocationCtx::current_depth(),
        );

        let invocation = self.invocations.handle_invocation_with_handler(
            None,
            None,
            function_id,
            namespace,
            input,
            function,
            traceparent,
            baggage,
            None,
            metadata,
            invocation_ctx,
        );
        // Adapt the existing future without an extra async frame for its arguments.
        invocation.map(|result| match result {
            Ok(result) => result,
            Err(err) => Err(ErrorBody {
                code: "invocation_error".into(),
                message: err.to_string(),
                stacktrace: None,
            }),
        })
    }

    pub(crate) async fn handle_worker(
        &self,
        socket: WorkerSocket,
        peer: SocketAddr,
        uri: Uri,
        headers: HeaderMap,
        config: Arc<WorkerManagerConfig>,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        tracing::debug!(peer = %peer, "Worker connected via WebSocket");
        let WorkerSocket {
            reader: mut ws_rx,
            writer: mut ws_tx,
        } = socket;

        let session =
            match rbac_session::handle_session(peer, Arc::new(self.clone()), config, uri, headers)
                .await
            {
                Ok(session) => session,
                Err(err) => {
                    let error_msg = serde_json::json!({
                        "type": "error",
                        "error": { "code": err.code, "message": err.message }
                    });
                    let _ = ws_tx
                        .send(WsMessage::Text(error_msg.to_string().into()))
                        .await;
                    let _ = ws_tx.send(WsMessage::Close(None)).await;
                    return Ok(());
                }
            };

        let (tx, mut rx) = mpsc::channel::<Outbound>(64);
        let worker = WorkerConnection::with_session(tx.clone(), session);
        let writer_worker = worker.clone();
        let writer_engine = self.clone();

        let writer = tokio::spawn(async move {
            while let Some(outbound) = rx.recv().await {
                let send_result = match outbound {
                    Outbound::Protocol(msg) => {
                        let serialized = match serde_json::to_string(&msg) {
                            Ok(payload) => payload,
                            Err(err) => {
                                tracing::error!(peer = %peer, error = ?err, "serialize error");
                                continue;
                            }
                        };
                        let Some((_msg, payload)) = writer_engine
                            .guard_outbound(&writer_worker, msg, serialized)
                            .await
                        else {
                            continue;
                        };
                        send_json(&mut ws_tx, payload).await
                    }
                    Outbound::Raw(frame) => ws_tx.send(frame).await,
                };

                if send_result.is_err() {
                    break;
                }
            }
        });

        // Mark before registering: from the moment this connection is
        // discoverable, a Reattach targeting it can rely on this read loop
        // to exit on eviction and run the cleanup.
        worker
            .has_reader
            .store(true, std::sync::atomic::Ordering::SeqCst);

        tracing::debug!(worker_id = %worker.id, peer = %peer, "Assigned worker ID");
        self.worker_registry.register_worker(worker.clone());
        // Must precede the read loop: the connection's namespace arrives later,
        // on `engine::workers::register`, and registrations sent before it must
        // queue rather than land in `DEFAULT_NAMESPACE`.
        self.begin_namespace_resolution(&worker);

        // Send worker ID back to the worker, along with the secret it must
        // present to reattach (retire this connection) after a reconnect.
        self.send_msg(
            &worker,
            Message::WorkerRegistered {
                worker_id: worker.id.to_string(),
                reattach_token: Some(worker.reattach_token.to_string()),
                // Older SDKs ignore the field; new ones enable `observe()` and
                // local dispatch only when it lists the feature.
                features: crate::protocol::engine_features(),
            },
        )
        .await;

        self.observers.emit(
            DEFAULT_NAMESPACE,
            crate::observer::events::ON_WORKERS_CHANGED,
            || {
                Some(serde_json::json!({
                    "event": "worker_connected",
                    "worker_id": worker.id.to_string(),
                }))
            },
        );

        let mut oversized_warnings = OversizedWarnings::default();
        let mut report_hold = crate::observer::ReportHold::default();
        loop {
            tokio::select! {
                // `biased` + eviction first: once a reattaching successor
                // evicts this connection, the loop must exit at the very next
                // iteration instead of draining more buffered frames — a
                // stale registration replayed from this socket after eviction
                // would re-claim ids for a retired identity.
                biased;
                _ = worker.evict.notified() => {
                    tracing::info!(worker_id = %worker.id, peer = %peer, "Worker evicted by reattaching successor");
                    break;
                }
                _ = shutdown_rx.changed() => {
                    tracing::info!(peer = %peer, "Shutdown signal received, closing worker connection");
                    break;
                }
                frame = ws_rx.next() => {
                    let frame = match frame {
                        Some(Ok(Inbound::Message(message))) => Some(Ok(message)),
                        Some(Ok(Inbound::Oversized(message))) => {
                            self.answer_oversized(&worker, peer, message, &mut oversized_warnings)
                                .await;
                            continue;
                        }
                        Some(Err(error)) => Some(Err(error)),
                        None => None,
                    };
                    match frame {
                        Some(Ok(WsMessage::Text(text))) => {
                            if text.trim().is_empty() {
                                continue;
                            }
                            match serde_json::from_str::<Message>(&text) {
                                Ok(msg) => self.route_frame(&worker, msg, &mut report_hold).await?,
                                Err(err) => {
                                    self.undecodable_frame(&worker, peer, text.as_bytes(), &err)
                                        .await;
                                }
                            }
                        }
                        Some(Ok(WsMessage::Binary(bytes))) => {
                            // Check for OTEL telemetry frames (OTLP, MTRC, LOGS prefixes).
                            // Records ingested while the worker handles an
                            // activation carry its depth, for the loop guard of
                            // `observability::on-log`.
                            let handled = match worker.inflight_depth() {
                                Some(depth) => {
                                    crate::observer::InvocationCtx::scope(
                                        Arc::new(
                                            crate::observer::InvocationCtx::engine_internal_at(
                                                depth,
                                            ),
                                        ),
                                        handle_telemetry_frame(&bytes, &peer),
                                    )
                                    .await
                                }
                                None => handle_telemetry_frame(&bytes, &peer).await,
                            };
                            if !handled {
                                // Not a telemetry frame, try to decode as regular protocol message
                                match serde_json::from_slice::<Message>(&bytes) {
                                    Ok(msg) => self.route_frame(&worker, msg, &mut report_hold).await?,
                                    Err(err) => {
                                        self.undecodable_frame(&worker, peer, &bytes, &err).await;
                                    }
                                }
                            }
                        }
                        Some(Ok(WsMessage::Close(_))) => {
                            tracing::debug!(peer = %peer, "Worker disconnected");
                            break;
                        }
                        Some(Ok(WsMessage::Ping(payload))) => {
                            let _ = tx.send(Outbound::Raw(WsMessage::Pong(payload))).await;
                        }
                        Some(Ok(WsMessage::Pong(_))) => {}
                        // Reads always yield whole messages, never raw frames.
                        Some(Ok(WsMessage::Frame(_))) => {}
                        Some(Err(error)) => {
                            tracing::warn!(
                                worker_id = %worker.id,
                                peer = %peer,
                                error = %error,
                                "Worker WebSocket read failed"
                            );
                            break;
                        }
                        None => break,
                    }
                }
            }
        }

        writer.abort();
        self.cleanup_worker(&worker).await;
        tracing::debug!(peer = %peer, "Worker disconnected (writer aborted)");
        Ok(())
    }

    /// Answers a message the worker sent over the inbound limit, which the
    /// reader dropped without buffering it: the sender of an `invokefunction`
    /// gets a `payload_too_large` result, and the caller waiting on an
    /// `invocationresult` gets `payload_too_large` in place of the result, as
    /// if the target had returned it. Anything else is only logged. The
    /// connection stays open either way.
    async fn answer_oversized(
        &self,
        worker: &WorkerConnection,
        peer: SocketAddr,
        message: Oversized,
        warnings: &mut OversizedWarnings,
    ) {
        let limit = crate::workers::worker::MAX_JSON_MESSAGE_BYTES;
        let envelope = message.envelope();
        let function_id = envelope.function_id.clone().unwrap_or_default();
        let error = inbound_payload_too_large_error(message.size, limit);
        let outcome = match (envelope.message_type.as_deref(), envelope.invocation_id) {
            (Some("invokefunction"), Some(invocation_id)) => {
                self.send_msg(
                    worker,
                    Message::InvocationResult {
                        invocation_id,
                        function_id,
                        result: None,
                        error: Some(error),
                        traceparent: None,
                        baggage: None,
                        observed: Vec::new(),
                    },
                )
                .await;
                "answered the caller"
            }
            (Some("invocationresult"), Some(invocation_id)) => {
                let result = Message::InvocationResult {
                    invocation_id,
                    function_id,
                    result: None,
                    error: Some(error),
                    traceparent: None,
                    baggage: None,
                    observed: Vec::new(),
                };
                // A result never waits on report backpressure, so it takes a
                // hold of its own.
                let mut hold = crate::observer::ReportHold::default();
                if let Err(error) = self.route_frame(worker, result, &mut hold).await {
                    tracing::warn!(worker_id = %worker.id, error = ?error, "Failed to complete an invocation whose result was too large");
                }
                "completed the invocation"
            }
            _ => "dropped",
        };
        if let Some(suppressed) = warnings.admit() {
            tracing::warn!(
                worker_id = %worker.id,
                peer = %peer,
                size_bytes = message.size,
                limit_bytes = limit,
                message_type = log_excerpt(envelope.message_type.as_deref().unwrap_or("unknown")),
                invocation_id = %crate::logging::display_option(&envelope.invocation_id),
                function_id = log_excerpt(envelope.function_id.as_deref().unwrap_or("")),
                outcome,
                suppressed,
                "Worker message exceeds the inbound limit"
            );
        }
    }

    /// A frame that is not a valid `Message`. A removed message type (see
    /// [`REMOVED_MESSAGES`]) is dropped with one `warn` per connection and
    /// type, naming what replaces it. Anything else is logged, and answered
    /// when it is a request whose sender is waiting: an `observe` with an `id`
    /// gets an `observeresult` error, an `invokefunction` with an
    /// `invocation_id` an `InvocationResult` error.
    async fn undecodable_frame(
        &self,
        worker: &WorkerConnection,
        peer: SocketAddr,
        frame: &[u8],
        error: &serde_json::Error,
    ) {
        let frame = match serde_json::from_slice::<Value>(frame) {
            Ok(Value::Object(frame)) => frame,
            _ => {
                tracing::warn!(peer = %peer, error = ?error, "frame decode error");
                return;
            }
        };
        let message_type = frame.get("type").and_then(Value::as_str);
        if let Some((index, (removed, replacement))) = message_type.and_then(|message_type| {
            REMOVED_MESSAGES
                .iter()
                .enumerate()
                .find(|(_, (removed, _))| *removed == message_type)
        }) {
            if worker.first_removed_message(index) {
                tracing::warn!(
                    worker_id = %worker.id,
                    peer = %peer,
                    message_type = %removed,
                    "dropping `{removed}` frames from this worker: this engine has no such \
                     message; {replacement}"
                );
            }
            return;
        }
        tracing::warn!(peer = %peer, error = ?error, "frame decode error");
        match message_type {
            Some("observe") => {
                if let Some(id) = frame.get("id").and_then(Value::as_str) {
                    self.observers.notify(
                        worker.id,
                        Message::ObserveResult {
                            id: id.to_string(),
                            state: crate::protocol::ObserverState::Error,
                            error: Some(ErrorBody::new(
                                crate::protocol::observe_codes::INVALID_ARGS,
                                format!("the observe message does not decode: {error}"),
                            )),
                            warnings: Vec::new(),
                        },
                    );
                }
            }
            Some("invokefunction") => {
                if let Some(invocation_id) = frame
                    .get("invocation_id")
                    .and_then(Value::as_str)
                    .and_then(|id| Uuid::parse_str(id).ok())
                {
                    let function_id = frame
                        .get("function_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    self.send_msg(
                        worker,
                        Message::InvocationResult {
                            invocation_id,
                            function_id,
                            result: None,
                            error: Some(ErrorBody::new(
                                "deserialization_error",
                                format!("the invokefunction message does not decode: {error}"),
                            )),
                            traceparent: None,
                            baggage: None,
                            observed: Vec::new(),
                        },
                    )
                    .await;
                }
            }
            _ => {}
        }
    }

    /// Handles OTEL-only WebSocket connections.
    ///
    /// SDKs open a second WS exclusively for OpenTelemetry (OTLP/MTRC/LOGS
    /// binary frames). Routing that traffic through `handle_worker` would
    /// pollute `worker_registry` with ghost rows that have no metadata, no
    /// functions, and no pid — doubling the worker count, inflating the
    /// `workers_active` metric, and adding noise to `Worker registered`
    /// logs. This handler performs the same RBAC handshake as a normal
    /// worker connection but skips `worker_registry.register_worker`, and
    /// only accepts telemetry binary frames on the inbound side.
    pub async fn handle_otel(
        &self,
        socket: WebSocket,
        peer: SocketAddr,
        uri: Uri,
        headers: HeaderMap,
        config: Arc<WorkerManagerConfig>,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        // `/otel` keeps axum's socket; only worker connections need raw frames.
        use axum::extract::ws::Message as WsMessage;

        tracing::debug!(peer = %peer, "OTEL connection opened");
        let (mut ws_tx, mut ws_rx) = socket.split();

        // Reuse the worker RBAC gate so OTEL traffic can't bypass auth.
        if let Err(err) =
            rbac_session::handle_session(peer, Arc::new(self.clone()), config, uri, headers).await
        {
            let error_msg = serde_json::json!({
                "type": "error",
                "error": { "code": err.code, "message": err.message }
            });
            let _ = ws_tx
                .send(WsMessage::Text(error_msg.to_string().into()))
                .await;
            let _ = ws_tx.send(WsMessage::Close(None)).await;
            return Ok(());
        }

        loop {
            tokio::select! {
                frame = ws_rx.next() => {
                    match frame {
                        Some(Ok(WsMessage::Binary(bytes))) => {
                            if !handle_telemetry_frame(&bytes, &peer).await {
                                tracing::warn!(peer = %peer, "Unrecognized binary frame on /otel (dropping)");
                            }
                        }
                        Some(Ok(WsMessage::Text(_))) => {
                            // /otel is binary-only. Text frames here are a
                            // protocol mistake — ignore rather than crash.
                            tracing::debug!(peer = %peer, "Ignoring text frame on /otel");
                        }
                        Some(Ok(WsMessage::Close(_))) => {
                            tracing::debug!(peer = %peer, "OTEL peer closed");
                            break;
                        }
                        Some(Ok(WsMessage::Ping(payload))) => {
                            let _ = ws_tx.send(WsMessage::Pong(payload)).await;
                        }
                        Some(Ok(WsMessage::Pong(_))) => {}
                        Some(Err(_)) | None => break,
                    }
                }
                _ = shutdown_rx.changed() => {
                    tracing::debug!(peer = %peer, "Shutdown signal received, closing OTEL connection");
                    let _ = ws_tx.send(WsMessage::Close(None)).await;
                    break;
                }
            }
        }

        tracing::debug!(peer = %peer, "OTEL connection closed");
        Ok(())
    }

    async fn cleanup_worker(&self, worker: &WorkerConnection) {
        // Run this connection's teardown exactly once, with COMPLETION
        // visible to every caller: the loser of the claim waits for the
        // winner to finish instead of returning early. "cleanup_worker
        // returned" must always mean "the slate is clean" — a reattach that
        // returned while a concurrent exit-path teardown was still mid-flight
        // would let the successor's replay race the remaining removals, which
        // is exactly the bug this path exists to close. No double
        // worker_disconnected event, no double death metrics.
        if worker
            .cleanup_claimed
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            tracing::debug!(worker_id = %worker.id, "Cleanup already running or done; awaiting completion");
            let mut done = worker.cleanup_done.subscribe();
            let _ = done.wait_for(|done| *done).await;
            return;
        }

        // Before snapshotting anything: a drain can be running concurrently on
        // the `engine::workers::register` task (worker disconnects right after
        // sending it), and would otherwise keep registering functions for the
        // rest of the teardown — after the snapshot below has decided what to
        // release. Those would leak, with `function_owners` pointing at a dead
        // worker. Aborting stops the drain at its next message.
        self.abort_namespace_resolution(worker);

        // Observers this connection created wait (suspended) for a Reattach;
        // observers on or targeting its functions follow the function removals
        // below.
        self.observers
            .connection_closed(worker.id, worker.reattach_token);

        let regular_functions = worker.get_regular_function_ids().await;
        let external_functions = worker.get_external_function_ids().await;

        tracing::debug!(worker_id = %worker.id, functions = ?regular_functions, "Worker registered functions");
        for function_id in regular_functions.iter() {
            if !self.release_function_if_owner(
                &worker.id,
                &self.connection_namespace(worker),
                function_id,
            ) {
                tracing::debug!(
                    worker_id = %worker.id,
                    function_id = %function_id,
                    "Skipping function removal — owner changed (another worker registered after)"
                );
            }
        }

        if !external_functions.is_empty() {
            let http_module = self
                .service_registry
                .get_service::<HttpFunctionsWorker>("http_functions");
            let namespace = self.connection_namespace(worker);
            for function_id in external_functions.iter() {
                let owner_key = (namespace.clone(), function_id.clone());
                // The ownership check and the removal happen under the owner
                // entry's lock (as `release_function_if_owner`): a racing
                // `RegisterFunction` claim waits for it, so the new owner's
                // registration — and its options — is never what goes. Ownership itself is kept through the rest of the
                // teardown and CAS-released at the end, so a claim landing
                // after this aborts the remaining steps.
                if !self.remove_external_function_if_owner(
                    &worker.id,
                    &namespace,
                    function_id,
                    http_module.as_deref(),
                ) {
                    tracing::debug!(
                        worker_id = %worker.id,
                        function_id = %function_id,
                        "Skipping external function removal — owner changed"
                    );
                    continue;
                }
                if self
                    .function_owners
                    .get(&owner_key)
                    .is_some_and(|r| r.0 == worker.id)
                {
                    self.service_registry
                        .remove_function_from_services(&namespace, function_id);
                }
                // CAS-release ownership. A racing claim will have overwritten
                // the entry with a new owner id; that predicate fails and we
                // leave their ownership intact.
                self.function_owners
                    .remove_if(&owner_key, |_, (owner, _kind)| *owner == worker.id);
            }
        }

        {
            let mut worker_invocations = worker.invocations.write().await;
            tracing::debug!(worker_id = %worker.id, invocations = ?worker_invocations, "Worker invocations");
            for invocation_id in worker_invocations.drain() {
                tracing::debug!(invocation_id = %invocation_id, "Halting invocation");
                self.invocations.halt_invocation(&invocation_id);
            }
        }

        self.channel_manager
            .remove_channels_by_worker(&worker.id)
            .await;
        // Release the worker-name lease before the registry entry (its source of
        // truth for the name) is gone. Read the name back through the registry:
        // the `worker` snapshot handed to `cleanup_worker` never carries it —
        // `engine::workers::register` writes into the registry's copy. CAS so a
        // restart that already reclaimed the name is not clobbered.
        if let Some(worker_name) = self.worker_registry.get_worker_name(&worker.id) {
            let namespace = self.connection_namespace(worker);
            self.release_worker_name_if_owner(&worker.id, &namespace, &worker_name);
        }
        self.worker_registry.unregister_worker(&worker.id);
        // Cancel the grace timer task (a no-op if it already fired) so its
        // `engine`/`worker` clones drop now instead of at the grace deadline.
        // (The connection state was already flipped to `Aborted` above, which is
        // what stops a still-running drain; this frees the idle sleep task.)
        self.cancel_namespace_grace_timer(&worker.id);

        // Dropped last: every teardown step above resolves the connection's
        // namespace through `connection_namespace`, which reads this entry.
        // Any registration still buffered here dies with the connection — its
        // worker is gone, so there is nothing to register.
        self.namespace_states.remove(&worker.id);

        self.observers.emit(
            DEFAULT_NAMESPACE,
            crate::observer::events::ON_WORKERS_CHANGED,
            || {
                Some(serde_json::json!({
                    "event": "worker_disconnected",
                    "worker_id": worker.id.to_string(),
                }))
            },
        );

        worker.cleanup_done.send_replace(true);
        tracing::debug!(worker_id = %worker.id, "Worker cleaned up");
    }

    /// Takes the ownership lease on `(namespace, function_id)` for `worker_id`.
    ///
    /// Fails when a *different* worker still present in `worker_registry` holds
    /// the lease: two live workers in one namespace exporting the same id is
    /// ambiguous routing, and silently letting the newcomer win would strand
    /// the incumbent's handler. A lease held by a worker that has already gone
    /// away is taken over (the fast-restart path), and a re-claim by the same
    /// worker is idempotent. An id the engine registered itself (a builtin,
    /// an engine-hosted source) has the engine as its host, so it is never
    /// free.
    ///
    /// The check and the write share one `DashMap` entry lock, so two
    /// concurrent claims cannot both observe a free lease.
    fn claim_function(
        &self,
        namespace: &str,
        worker_id: Uuid,
        function_id: &str,
    ) -> Result<(), NamespaceConflict> {
        self.claim_in(
            namespace,
            worker_id,
            function_id,
            InvocationKind::Regular,
            "Function",
        )
    }

    /// True when `worker_id`'s connection has entered teardown — its namespace
    /// resolution has been `Aborted` (the first step of `cleanup_worker`). Such
    /// a connection is on its way out even if `worker_registry` has not swept it
    /// yet, so it must not block a fresh claim on the name it held.
    fn is_connection_tearing_down(&self, worker_id: &Uuid) -> bool {
        matches!(
            self.namespace_states.get(worker_id).as_deref(),
            Some(NamespaceState::Aborted(_))
        )
    }

    /// Takes the ownership lease on `(namespace, worker_name)` for `worker_id`.
    ///
    /// Fails when a *different* worker that is still live in this namespace holds
    /// the name: one live worker name per namespace, period (this includes the
    /// SDK's default `hostname:pid`). The loser is rejected and its connection
    /// closed by the caller.
    ///
    /// "Live" is deliberately narrower than "present in `worker_registry`": an
    /// owner whose connection is tearing down (`is_connection_tearing_down`) is
    /// treated as gone, so a worker that reconnects during its own teardown
    /// window reclaims its name instead of being rejected. A lease held by a
    /// worker already removed from the registry is likewise taken over, and a
    /// re-claim by the same worker is idempotent.
    ///
    /// The check and the write share one `DashMap` entry lock, so two concurrent
    /// claims cannot both observe a free lease.
    pub(crate) fn claim_worker_name(
        &self,
        namespace: &str,
        worker_id: Uuid,
        worker_name: &str,
    ) -> Result<(), NamespaceConflict> {
        let key = (namespace.to_string(), worker_name.to_string());
        match self.worker_name_owners.entry(key) {
            dashmap::mapref::entry::Entry::Occupied(mut occupied) => {
                let previous = *occupied.get();
                if previous != worker_id
                    && self.worker_registry.workers.contains_key(&previous)
                    && !self.is_connection_tearing_down(&previous)
                {
                    tracing::warn!(
                        rejected_worker = %worker_id,
                        owner = %previous,
                        namespace = %namespace,
                        worker_name = %worker_name,
                        "Worker name already held by a live worker in this namespace — registration rejected"
                    );
                    return Err(NamespaceConflict {
                        namespace: namespace.to_string(),
                        name: worker_name.to_string(),
                        owner_worker_id: previous.to_string(),
                    });
                }
                occupied.insert(worker_id);
            }
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                vacant.insert(worker_id);
            }
        }
        Ok(())
    }

    /// CAS-releases a worker-name lease held by `worker_id`. A racing restart
    /// that already took the name over will have overwritten the owner, so the
    /// predicate fails and its claim is left intact.
    fn release_worker_name_if_owner(&self, worker_id: &Uuid, namespace: &str, worker_name: &str) {
        self.worker_name_owners.remove_if(
            &(namespace.to_string(), worker_name.to_string()),
            |_, owner| owner == worker_id,
        );
    }

    /// Rejects a worker registration that lost a `(namespace, worker_name)`
    /// conflict: tells the loser why, then closes its connection. Unlike a
    /// function-id conflict (one refused id, connection kept), a duplicate live
    /// worker name is fatal — the SDKs treat `RegistrationRejected` as a
    /// no-reconnect error.
    pub(crate) async fn reject_worker_registration(
        &self,
        worker_id: Uuid,
        conflict: NamespaceConflict,
    ) {
        let Some(worker) = self.worker_registry.get_worker(&worker_id) else {
            return;
        };
        self.send_msg(
            &worker,
            Message::RegistrationRejected {
                code: WORKER_NAMESPACE_CONFLICT.to_string(),
                namespace: conflict.namespace,
                worker_name: Some(conflict.name),
                function_id: None,
                owner_worker_id: conflict.owner_worker_id,
            },
        )
        .await;
        // Close the connection: the writer task forwards this Close frame, the
        // read loop then ends and `cleanup_worker` runs.
        let _ = worker
            .channel
            .send(Outbound::Raw(WsMessage::Close(None)))
            .await;
    }

    /// HTTP-invocation variant of `claim_function`.
    fn claim_external_function(
        &self,
        namespace: &str,
        worker_id: Uuid,
        function_id: &str,
    ) -> Result<(), NamespaceConflict> {
        self.claim_in(
            namespace,
            worker_id,
            function_id,
            InvocationKind::External,
            "External function",
        )
    }

    /// Claims `(namespace, function_id)` for `worker_id` under `kind`, rejecting
    /// if a DIFFERENT live worker already owns it — regardless of the kind that
    /// owner registered under. Regular and HTTP-invocation registrations share
    /// this one owner map (and one `FunctionsRegistry`), so the check and the
    /// write are a single `DashMap` entry lock: two concurrent claims of the
    /// same key — one Regular, one External — cannot both observe a free lease,
    /// which is what the prior two-map design allowed. On takeover (a departed
    /// or tearing-down previous owner) the entry is overwritten with the new
    /// `(worker_id, kind)`, so a later restart that changes kind is reflected.
    ///
    /// A key with no owner entry whose function is registered anyway was
    /// registered by the engine (only worker registrations take an entry):
    /// the engine hosts it, so the claim fails and the engine's function
    /// stays. A worker's own registrations leave the registry before their
    /// entry does, under the entry's lock, so a key a worker released is
    /// never mistaken for one the engine hosts.
    fn claim_in(
        &self,
        namespace: &str,
        worker_id: Uuid,
        function_id: &str,
        kind: InvocationKind,
        kind_label: &str,
    ) -> Result<(), NamespaceConflict> {
        let key = (namespace.to_string(), function_id.to_string());
        match self.function_owners.entry(key) {
            dashmap::mapref::entry::Entry::Occupied(mut occupied) => {
                let (previous, _prev_kind) = *occupied.get();
                if previous != worker_id
                    && self.worker_registry.workers.contains_key(&previous)
                    && !self.is_connection_tearing_down(&previous)
                {
                    return Err(self.conflict(
                        kind_label,
                        namespace,
                        function_id,
                        worker_id,
                        previous,
                    ));
                }
                occupied.insert((worker_id, kind));
            }
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                if self
                    .functions
                    .with_function(namespace, function_id, |function| function.is_some())
                {
                    return Err(self.engine_hosted_conflict(
                        kind_label,
                        namespace,
                        function_id,
                        worker_id,
                    ));
                }
                vacant.insert((worker_id, kind));
            }
        }
        Ok(())
    }

    /// The conflict of a worker registering an id the engine hosts. No
    /// worker owns the id, so the rejection names none.
    fn engine_hosted_conflict(
        &self,
        kind: &str,
        namespace: &str,
        function_id: &str,
        rejected: Uuid,
    ) -> NamespaceConflict {
        let rejected_name = self.worker_registry.get_worker_name(&rejected);
        tracing::warn!(
            namespace = %namespace,
            function_id = %function_id,
            rejected_worker_name = %rejected_name.as_deref().unwrap_or("<unknown>"),
            rejected_worker_id = %rejected,
            "{kind} '{function_id}' in namespace '{namespace}' is hosted by the engine; \
             rejecting registration from worker {rejected}. Register the function under \
             another id."
        );
        NamespaceConflict {
            namespace: namespace.to_string(),
            name: function_id.to_string(),
            owner_worker_id: String::new(),
        }
    }

    fn conflict(
        &self,
        kind: &str,
        namespace: &str,
        function_id: &str,
        rejected: Uuid,
        owner: Uuid,
    ) -> NamespaceConflict {
        use colored::Colorize;
        let owner_name = self.worker_registry.get_worker_name(&owner);
        let rejected_name = self.worker_registry.get_worker_name(&rejected);
        let owner_label = owner_name.as_deref().unwrap_or("<unknown>");
        let rejected_label = rejected_name.as_deref().unwrap_or("<unknown>");
        // Colored inline; the structured fields below stay plain for parsing.
        // `colored` auto-disables when the sink is not a TTY (e.g. a log file).
        let fid_c = function_id.yellow();
        let ns_c = namespace.cyan();
        let owner_c = owner_label.green();
        let rejected_c = rejected_label.red();
        tracing::warn!(
            namespace = %namespace,
            function_id = %function_id,
            owner_worker_name = %owner_label,
            owner_worker_id = %owner,
            rejected_worker_name = %rejected_label,
            rejected_worker_id = %rejected,
            "{kind} '{fid_c}' in namespace '{ns_c}' is already owned by live worker \
             '{owner_c}' ({owner}); rejecting registration from worker '{rejected_c}' \
             ({rejected}). Only one worker per namespace may own a function id — rename the \
             function or use a different namespace."
        );
        NamespaceConflict {
            namespace: namespace.to_string(),
            name: function_id.to_string(),
            owner_worker_id: owner.to_string(),
        }
    }

    /// Atomically removes `function_id` from the engine ONLY if `worker_id`
    /// is still the recorded owner. Returns true if the removal occurred.
    /// Holds the `function_owners` entry (shard write lock) across the
    /// engine-global removal: `claim_function`'s `insert` serializes behind
    /// the same lock, so a racing claim can never land between the ownership
    /// check and `remove_function_from_engine` — the window that let a stale
    /// connection's cleanup delete its reconnected successor's fresh
    /// registration. NOTE: `remove_function_from_engine` must never touch
    /// `function_owners`, or this deadlocks.
    /// `namespace` must be the owning connection's effective namespace — the
    /// caller holds the `WorkerConnection`, so it is passed in rather than
    /// re-derived from the registry (which would silently fall back to
    /// `DEFAULT_NAMESPACE` for a connection already unregistered).
    fn release_function_if_owner(
        &self,
        worker_id: &Uuid,
        namespace: &str,
        function_id: &str,
    ) -> bool {
        match self
            .function_owners
            .entry((namespace.to_string(), function_id.to_string()))
        {
            dashmap::mapref::entry::Entry::Occupied(entry) if entry.get().0 == *worker_id => {
                self.remove_function_from_engine(namespace, function_id);
                entry.remove();
                true
            }
            _ => false,
        }
    }

    /// Removes an HTTP-invoked registration (the `http_functions` entry and
    /// the function) only if `worker_id` still owns it, while holding the
    /// owner entry, as [`Self::release_function_if_owner`] does; ownership
    /// itself is kept. Returns whether it removed. Without the module, the
    /// function is removed from the engine directly.
    fn remove_external_function_if_owner(
        &self,
        worker_id: &Uuid,
        namespace: &str,
        function_id: &str,
        http_module: Option<&HttpFunctionsWorker>,
    ) -> bool {
        match self
            .function_owners
            .entry((namespace.to_string(), function_id.to_string()))
        {
            dashmap::mapref::entry::Entry::Occupied(entry) if entry.get().0 == *worker_id => {
                match http_module {
                    Some(module) => {
                        module
                            .http_functions()
                            .remove(&(namespace.to_string(), function_id.to_string()));
                        self.remove_function(namespace, function_id);
                    }
                    None => self.remove_function_from_engine(namespace, function_id),
                }
                true
            }
            _ => false,
        }
    }

    /// External-function variant of `release_function_if_owner`. Caller is
    /// responsible for unregistering from `http_functions` and the service
    /// registry on success — this helper only releases the owner index.
    ///
    /// The CAS predicate matches on the OWNER, not the kind: once a restart that
    /// changes invocation kind has overwritten the entry with the new worker's
    /// `(new_id, new_kind)`, this old-kind release finds `owner != worker_id`
    /// and leaves the new registration intact.
    fn release_external_function_if_owner(
        &self,
        worker_id: &Uuid,
        namespace: &str,
        function_id: &str,
    ) -> bool {
        self.function_owners
            .remove_if(
                &(namespace.to_string(), function_id.to_string()),
                |_, (owner, _kind)| owner == worker_id,
            )
            .is_some()
    }
}

impl EngineTrait for Engine {
    /// Internal call path used by hooks and middleware — not direct user
    /// invocations over WebSocket. We intentionally skip
    /// `mark_user_function_invoked` here because this path serves engine
    /// orchestration; the heartbeat should only count actual user-initiated
    /// invocations arriving via `remember_invocation` and not things the engine
    /// can fire itself without user involvement, such as cron.
    ///
    /// `namespace` is the namespace the target function is resolved in. Hooks
    /// and middleware pass [`DEFAULT_NAMESPACE`] (via `call_with_metadata`).
    async fn call_with_metadata_ns(
        &self,
        namespace: &str,
        function_id: &str,
        input: impl Serialize + Send,
        metadata: Option<Value>,
    ) -> Result<Option<Value>, ErrorBody> {
        let input = serde_json::to_value(input).map_err(|e| ErrorBody {
            code: "serialization_error".into(),
            message: e.to_string(),
            stacktrace: None,
        })?;
        self.call_value_with_metadata_ns(namespace, function_id, input, metadata)
            .await
    }

    fn register_function_ns(
        &self,
        namespace: &str,
        request: RegisterFunctionRequest,
        handler: Box<dyn FunctionHandler + Send + Sync>,
    ) {
        let RegisterFunctionRequest {
            function_id,
            description,
            request_format,
            response_format,
            metadata,
        } = request;

        let handler_arc: Arc<dyn FunctionHandler + Send + Sync> = handler.into();
        let handler_function_id = function_id.clone();

        let function = Function {
            handler: Arc::new(move |invocation_id, input, _session, metadata| {
                let handler = handler_arc.clone();
                let path = handler_function_id.clone();
                Box::pin(async move {
                    handler
                        .handle_function(invocation_id, path, input, metadata)
                        .await
                })
            }),
            _function_id: function_id.clone(),
            _description: description,
            request_format,
            response_format,
            metadata,
        };

        self.functions
            .register_function_ns(namespace, function_id, function);
        crate::workers::telemetry::collector::track_function_registered();
    }

    fn register_function_handler_ns<H, F>(
        &self,
        namespace: &str,
        request: RegisterFunctionRequest,
        handler: Handler<H>,
    ) where
        H: HandlerFn<F>,
        F: Future<Output = HandlerOutput> + Send + 'static,
    {
        let handler_arc: Arc<H> = Arc::new(handler.f);

        let function = Function {
            handler: Arc::new(move |_id, input, _session, _metadata| {
                let handler = handler_arc.clone();
                Box::pin(async move { handler(input).await })
            }),
            _function_id: request.function_id.clone(),
            _description: request.description,
            request_format: request.request_format,
            response_format: request.response_format,
            metadata: request.metadata,
        };

        self.functions
            .register_function_ns(namespace, request.function_id, function);
    }

    fn register_function_handler_with_session<H, F>(
        &self,
        request: RegisterFunctionRequest,
        handler: SessionHandler<H>,
    ) where
        H: SessionHandlerFn<F>,
        F: Future<Output = HandlerOutput> + Send + 'static,
    {
        let handler_arc: Arc<H> = Arc::new(handler.f);

        let function = Function {
            handler: Arc::new(move |_id, input, session, _metadata| {
                let handler = handler_arc.clone();
                let session = session.clone();
                Box::pin(async move { handler(input, session).await })
            }),
            _function_id: request.function_id.clone(),
            _description: request.description,
            request_format: request.request_format,
            response_format: request.response_format,
            metadata: request.metadata,
        };

        self.functions
            .register_function(request.function_id, function);
    }
}

fn payload_too_large_error(size: usize, limit: usize, requires_upgrade: bool) -> ErrorBody {
    let remediation = if requires_upgrade {
        " It did not declare large-messages.v1 (SDK 0.25.0 or later), and the version it reports is older than 0.24.5 or unknown. Upgrade the target worker SDK to 0.25.0 or later, or use channels: https://iii.dev/docs/creating-workers/channels#payload-size"
    } else {
        " Use channels for larger payloads: https://iii.dev/docs/creating-workers/channels#payload-size"
    };
    ErrorBody::new(
        "payload_too_large",
        format!(
            "Serialized JSON envelope is {size} bytes; target worker accepts up to {limit} bytes.{remediation}"
        ),
    )
}

/// The answer to a worker message over the engine's inbound limit.
fn inbound_payload_too_large_error(size: u64, limit: usize) -> ErrorBody {
    ErrorBody::new(
        "payload_too_large",
        format!(
            "Serialized JSON envelope is {size} bytes; the engine reads up to {limit} bytes from a worker. Use channels for larger payloads: https://iii.dev/docs/creating-workers/channels#payload-size"
        ),
    )
}

/// At most the first 128 bytes of `value`: fields recovered from a dropped
/// message are whatever the worker sent.
fn log_excerpt(value: &str) -> &str {
    let mut end = value.len().min(128);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

/// How often a connection logs the messages over the inbound limit it sends.
const OVERSIZED_WARN_INTERVAL: Duration = Duration::from_secs(10);

/// Rate limit for the warning about a connection's oversized messages: the
/// first is logged, then at most one per [`OVERSIZED_WARN_INTERVAL`], with the
/// number left out since the last one.
#[derive(Default)]
struct OversizedWarnings {
    next_at: Option<tokio::time::Instant>,
    suppressed: u64,
}

impl OversizedWarnings {
    /// `Some(suppressed)` when this message is to be logged.
    fn admit(&mut self) -> Option<u64> {
        let now = tokio::time::Instant::now();
        if self.next_at.is_some_and(|next_at| now < next_at) {
            self.suppressed += 1;
            return None;
        }
        self.next_at = Some(now + OVERSIZED_WARN_INTERVAL);
        Some(std::mem::take(&mut self.suppressed))
    }
}

fn protocol_message_type(message: &Message) -> &'static str {
    match message {
        Message::InvokeFunction { .. } => "InvokeFunction",
        Message::InvocationResult { .. } => "InvocationResult",
        Message::RegisterFunction { .. } => "RegisterFunction",
        Message::RegisterService { .. } => "RegisterService",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use serde::Serialize;
    use serde_json::json;
    use tokio::sync::mpsc;

    use crate::{
        config::SecurityConfig,
        function::FunctionResult,
        protocol::{DEFAULT_NAMESPACE, HttpInvocationRef, Message},
        worker_connections::WorkerConnection,
        workers::{
            http_functions::{HttpFunctionsWorker, config::HttpFunctionsConfig},
            observability::metrics::ensure_default_meter,
            traits::Worker,
        },
    };

    use super::{
        Engine, EngineTrait, Outbound, REGISTRATION_NAMESPACE_GRACE, payload_too_large_error,
        protocol_message_type,
    };

    fn make_request(function_id: &str) -> crate::engine::RegisterFunctionRequest {
        crate::engine::RegisterFunctionRequest {
            function_id: function_id.to_string(),
            description: Some(format!("test handler for {function_id}")),
            request_format: None,
            response_format: None,
            metadata: None,
        }
    }

    struct FailingSerialize;

    impl Serialize for FailingSerialize {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("serialization exploded"))
        }
    }

    #[test]
    fn runtime_worker_registry_upserts_lists_and_removes() {
        let engine = Engine::new();
        engine.upsert_runtime_worker(crate::worker_connections::RuntimeWorkerInfo {
            id: "iii-state".to_string(),
            name: "iii-state".to_string(),
            description: None,
            worker_type: "iii-state".to_string(),
            connected_at: chrono::Utc::now(),
            function_ids: vec!["state::get".to_string()],
            internal: false,
        });

        let workers = engine.list_runtime_workers();
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].id, "iii-state");

        engine.remove_runtime_worker("iii-state");
        assert!(engine.list_runtime_workers().is_empty());
    }

    #[tokio::test]
    async fn register_function_with_http_invocation_registers_and_cleans_up() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());

        let http_functions_config = HttpFunctionsConfig {
            security: SecurityConfig {
                require_https: false,
                block_private_ips: false,
                url_allowlist: vec!["*".to_string()],
            },
        };

        let http_functions_module = HttpFunctionsWorker::create(
            engine.clone(),
            Some(serde_json::to_value(&http_functions_config).expect("serialize config")),
        )
        .await
        .expect("create module");
        http_functions_module
            .initialize()
            .await
            .expect("initialize module");

        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let register_message = Message::RegisterFunction {
            id: "external.my_lambda".to_string(),
            description: Some("external lambda".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: Some(HttpInvocationRef {
                url: "http://example.com/lambda".to_string(),
                method: crate::invocation::method::HttpMethod::Post,
                timeout_ms: Some(30000),
                headers: HashMap::new(),
                auth: None,
            }),
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };

        engine
            .router_msg(&worker, &register_message)
            .await
            .expect("register function");

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "external.my_lambda")
                .is_some()
        );
        assert!(worker.has_external_function_id("external.my_lambda").await);

        let http_module = engine
            .service_registry
            .get_service::<HttpFunctionsWorker>("http_functions")
            .expect("http_functions service registered");

        assert!(http_module.http_functions().contains_key(&(
            DEFAULT_NAMESPACE.to_string(),
            "external.my_lambda".to_string()
        )));

        engine.cleanup_worker(&worker).await;

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "external.my_lambda")
                .is_none()
        );

        assert!(!http_module.http_functions().contains_key(&(
            DEFAULT_NAMESPACE.to_string(),
            "external.my_lambda".to_string()
        )));
    }

    /// A function registered with `invocation` is called over HTTP: no
    /// connection answers it, so the engine evaluates its observers itself
    /// (decision 43), whatever the registering connection declared.
    #[tokio::test]
    async fn the_engine_evaluates_the_calls_of_an_http_invoked_function() {
        use crate::observer::events::test_support;
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let http_functions = HttpFunctionsWorker::create(
            engine.clone(),
            Some(
                serde_json::to_value(HttpFunctionsConfig {
                    security: SecurityConfig {
                        require_https: false,
                        block_private_ips: false,
                        url_allowlist: vec!["*".to_string()],
                    },
                })
                .expect("serialize config"),
            ),
        )
        .await
        .expect("create module");
        http_functions
            .initialize()
            .await
            .expect("initialize module");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let app = axum::Router::new().route(
            "/lambda",
            axum::routing::post(|| async { axum::Json(serde_json::json!({ "ok": true })) }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await });

        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        assert!(
            worker
                .features
                .set(&["observe.v1".to_string(), "observe.v2".to_string()])
        );
        engine.worker_registry.register_worker(worker.clone());
        engine
            .router_msg(
                &worker,
                &Message::RegisterFunction {
                    id: "external::lambda".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: Some(HttpInvocationRef {
                        url: format!("http://{address}/lambda"),
                        method: crate::invocation::method::HttpMethod::Post,
                        timeout_ms: Some(5_000),
                        headers: HashMap::new(),
                        auth: None,
                    }),
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("register function");

        let mut sink = test_support::capture(&engine, "test::sink");
        let _observing = test_support::observe(
            &engine,
            "obs-http",
            "external::lambda",
            "test::sink",
            Some(serde_json::json!({ "output.ok": { "$eq": true } })),
        )
        .await;
        let result = engine
            .call("external::lambda", serde_json::json!({ "n": 1 }))
            .await
            .expect("the endpoint answers");
        assert_eq!(result, Some(serde_json::json!({ "ok": true })));
        assert_eq!(
            test_support::next(&mut sink).await,
            serde_json::json!({ "n": 1 }),
            "the engine matched its filter on the endpoint's answer"
        );
    }

    // ---------------------------------------------------------------
    // 1. router_msg tests for different message types
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn is_engine_owned_builtin_distinguishes_engine_from_worker() {
        use super::InvocationKind;
        ensure_default_meter();
        let engine = Engine::new();

        // Engine builtins never populate `function_owners`, so an `engine::*` id
        // with no owner entry is engine-owned and bypasses middleware.
        assert!(engine.is_engine_owned_builtin(DEFAULT_NAMESPACE, "engine::functions::list"));

        // A worker-registered `engine::foo` owns a `function_owners` entry, so it
        // must NOT bypass — the operator's middleware keeps intercepting it.
        engine.function_owners.insert(
            (DEFAULT_NAMESPACE.to_string(), "engine::foo".to_string()),
            (uuid::Uuid::new_v4(), InvocationKind::Regular),
        );
        assert!(!engine.is_engine_owned_builtin(DEFAULT_NAMESPACE, "engine::foo"));

        // Non-`engine::` ids are never engine-owned builtins.
        assert!(!engine.is_engine_owned_builtin(DEFAULT_NAMESPACE, "state::get"));
    }

    #[tokio::test]
    async fn test_router_msg_register_function() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let msg = Message::RegisterFunction {
            id: "my_func".to_string(),
            description: Some("A test function".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };

        engine
            .router_msg(&worker, &msg)
            .await
            .expect("register function should succeed");

        // Function should be registered in the engine
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "my_func")
                .is_some(),
            "function should be registered"
        );

        // Worker should track the function id
        let function_ids = worker.get_regular_function_ids().await;
        assert!(
            function_ids.contains(&"my_func".to_string()),
            "worker should track the function id"
        );
    }

    /// An HTTP-invoked registration that fails leaves none of its options
    /// behind.
    #[tokio::test]
    async fn a_failed_http_invoked_registration_leaves_no_options() {
        ensure_default_meter();
        // No `http_functions` module: the HTTP-invoked registration fails.
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        let msg: Message = serde_json::from_value(serde_json::json!({
            "type": "registerfunction",
            "id": "ext::hook",
            "invocation": { "url": "http://127.0.0.1:9/hook" },
            "private": true
        }))
        .expect("registerfunction");
        engine.router_msg(&worker, &msg).await.expect("handled");
        assert!(
            engine
                .functions
                .get(DEFAULT_NAMESPACE, "ext::hook")
                .is_none()
        );
        assert!(
            engine
                .observers
                .function_entry(&(DEFAULT_NAMESPACE.to_string(), "ext::hook".to_string()))
                .is_none(),
            "its options went with it"
        );
        assert!(
            engine
                .observers
                .route(DEFAULT_NAMESPACE, "ext::hook", None)
                .is_ok()
        );
    }

    #[tokio::test]
    async fn router_msg_register_function_uses_connection_namespace() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        // Drive the real mechanism rather than doctoring `worker.namespace`:
        // that field is never set on a connection the engine dispatches
        // against, so writing it would prove nothing about production.
        engine.begin_namespace_resolution(&worker);
        engine.resolve_connection_namespace(&worker, "orders").await;

        let msg = Message::RegisterFunction {
            id: "svc::my_func".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };

        engine
            .router_msg(&worker, &msg)
            .await
            .expect("register function should succeed");

        assert!(
            engine.functions.get("orders", "svc::my_func").is_some(),
            "function must be registered in the connection's namespace"
        );
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "svc::my_func")
                .is_none(),
            "function must NOT leak into the default namespace"
        );
        assert!(
            engine.service_registry.get("orders", "svc").is_some(),
            "service must be registered in the same namespace as its function"
        );
    }

    /// `claim_worker_name` enforces one live name per namespace: a second live
    /// worker is refused, a re-claim by the incumbent is idempotent, and a name
    /// keyed to a worker no longer in the registry is taken over (restart).
    #[test]
    fn claim_worker_name_rejects_live_duplicate_and_takes_over_dead_owner() {
        ensure_default_meter();
        let engine = Engine::new();

        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let owner = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(owner.clone());

        engine
            .claim_worker_name("orders", owner.id, "state")
            .expect("first claim must succeed");

        // A different live worker cannot take a held name.
        let other = uuid::Uuid::new_v4();
        let conflict = engine
            .claim_worker_name("orders", other, "state")
            .expect_err("a second live worker must be rejected");
        assert_eq!(conflict.namespace, "orders");
        assert_eq!(conflict.name, "state");
        assert_eq!(conflict.owner_worker_id, owner.id.to_string());

        // The incumbent re-claiming its own name is idempotent.
        engine
            .claim_worker_name("orders", owner.id, "state")
            .expect("re-claim by the same worker must succeed");

        // A lease pointing at a worker that is not in the registry is taken over.
        let dead = uuid::Uuid::new_v4();
        engine
            .worker_name_owners
            .insert(("analytics".to_string(), "state".to_string()), dead);
        engine
            .claim_worker_name("analytics", uuid::Uuid::new_v4(), "state")
            .expect("a dead owner's name must be claimable (worker restart)");
    }

    /// A held name whose owner's connection is tearing down
    /// (`abort_namespace_resolution` ran) must not block a fresh claim, even
    /// though the owner is still present in the registry — the fast-restart
    /// window.
    #[tokio::test]
    async fn claim_worker_name_takes_over_a_tearing_down_owner() {
        ensure_default_meter();
        let engine = Engine::new();

        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let owner = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(owner.clone());
        engine.begin_namespace_resolution(&owner);
        engine.resolve_connection_namespace(&owner, "orders").await;
        engine
            .claim_worker_name("orders", owner.id, "state")
            .expect("owner claims the name");

        // While still live, the owner blocks a fresh claim.
        let restart = uuid::Uuid::new_v4();
        engine
            .claim_worker_name("orders", restart, "state")
            .expect_err("a live owner must block the claim");

        // Once its connection begins teardown, it must not block anymore.
        engine.abort_namespace_resolution(&owner);
        assert!(
            engine.worker_registry.workers.contains_key(&owner.id),
            "precondition: the owner is still in the registry, only tearing down"
        );
        engine
            .claim_worker_name("orders", restart, "state")
            .expect("a tearing-down owner must not block a restart");
    }

    #[tokio::test]
    async fn a_function_id_owned_via_one_invocation_path_blocks_the_other() {
        ensure_default_meter();
        let engine = Engine::new();

        let (tx_a, _rx_a) = mpsc::channel::<Outbound>(8);
        let a = WorkerConnection::new(tx_a);
        engine.worker_registry.register_worker(a.clone());
        let (tx_b, _rx_b) = mpsc::channel::<Outbound>(8);
        let b = WorkerConnection::new(tx_b);
        engine.worker_registry.register_worker(b.clone());

        // A owns the id as a REGULAR function.
        engine
            .claim_function("orders", a.id, "svc::f")
            .expect("A claims the regular function");

        // B tries to own the SAME (ns, id) as an HTTP-invocation function.
        // Different owner map, but the same registry entry would be
        // overwritten — so it must be rejected, not silently accepted.
        let conflict = engine
            .claim_external_function("orders", b.id, "svc::f")
            .expect_err("an id already owned via the regular path must block the external claim");
        assert_eq!(conflict.owner_worker_id, a.id.to_string());

        // ...and symmetrically.
        engine
            .claim_external_function("orders", b.id, "svc::g")
            .expect("B claims an external function");
        engine
            .claim_function("orders", a.id, "svc::g")
            .expect_err("an id already owned via the external path must block the regular claim");
    }

    /// The same worker re-registering the same id is idempotent, not a conflict:
    /// the `previous == worker_id` branch in `claim_in` re-claims instead of
    /// rejecting, so a hot-reload that re-exports a function keeps ownership and
    /// the registry's last-write-wins overwrite applies.
    #[tokio::test]
    async fn claim_function_is_idempotent_for_the_same_worker() {
        ensure_default_meter();
        let engine = Engine::new();

        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let a = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(a.clone());

        engine
            .claim_function("orders", a.id, "svc::f")
            .expect("first claim by A");
        // Same worker, same (namespace, id): must succeed, not conflict.
        engine
            .claim_function("orders", a.id, "svc::f")
            .expect("re-claim by the same worker must be idempotent, not rejected");

        let owner = engine
            .function_owners
            .get(&("orders".to_string(), "svc::f".to_string()))
            .map(|e| e.value().0);
        assert_eq!(owner, Some(a.id), "the same worker must still own the id");
    }

    #[tokio::test]
    async fn claim_function_takes_over_a_tearing_down_owner() {
        ensure_default_meter();
        let engine = Engine::new();

        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let owner = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(owner.clone());
        engine.begin_namespace_resolution(&owner);
        engine.resolve_connection_namespace(&owner, "orders").await;
        engine
            .claim_function("orders", owner.id, "state::get")
            .expect("owner claims the function");

        // While still live, the owner blocks a fresh claim of the same function.
        let restart = uuid::Uuid::new_v4();
        engine
            .claim_function("orders", restart, "state::get")
            .expect_err("a live owner must block the function claim");

        // Once its connection begins teardown, it must not block the restart's
        // functions any more than it blocks the restart's name.
        engine.abort_namespace_resolution(&owner);
        assert!(
            engine.worker_registry.workers.contains_key(&owner.id),
            "precondition: the owner is still in the registry, only tearing down"
        );
        engine
            .claim_function("orders", restart, "state::get")
            .expect("a tearing-down owner must not block a restart's function");
    }

    /// `cleanup_worker` releases the worker-name lease, reading the name back
    /// from the registry (the connection snapshot never carries it).
    #[tokio::test]
    async fn cleanup_worker_releases_the_name_lease() {
        ensure_default_meter();
        let engine = Engine::new();

        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(worker.clone());
        engine.worker_registry.update_worker_metadata(
            &worker.id,
            "node".to_string(),
            None,
            Some("state".to_string()),
            None,
            None,
            None,
            None,
            None,
            Some("orders".to_string()),
        );
        engine.begin_namespace_resolution(&worker);
        engine.resolve_connection_namespace(&worker, "orders").await;
        engine
            .claim_worker_name("orders", worker.id, "state")
            .expect("claim the name");

        assert!(
            engine
                .worker_name_owners
                .contains_key(&("orders".to_string(), "state".to_string())),
            "precondition: the name lease is held"
        );

        engine.cleanup_worker(&worker).await;

        assert!(
            !engine
                .worker_name_owners
                .contains_key(&("orders".to_string(), "state".to_string())),
            "cleanup_worker must release the name lease"
        );
    }

    /// A channel made through `engine::channels::create` belongs to the calling
    /// connection: its teardown releases the channel (aborting a reader still
    /// waiting on it) and leaves other connections' channels alone.
    #[tokio::test]
    async fn cleanup_worker_releases_channels_the_worker_created() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        crate::workers::engine_fn::EngineFunctionsWorker::new(engine.clone())
            .register_functions(engine.clone());

        async fn create_channel(
            engine: &Engine,
            worker: &WorkerConnection,
            rx: &mut mpsc::Receiver<Outbound>,
        ) -> (String, String) {
            let invoke = Message::InvokeFunction {
                invocation_id: Some(uuid::Uuid::new_v4()),
                function_id: "engine::channels::create".to_string(),
                data: json!({}),
                traceparent: None,
                baggage: None,
                action: None,
                metadata: None,
                namespace: None,
                caller: None,
                observe_depth: None,
                activation: None,
            };
            engine.router_msg(worker, &invoke).await.expect("invoke");
            match tokio::time::timeout(Duration::from_secs(1), rx.recv())
                .await
                .expect("timed out waiting for the channel")
                .expect("channel open")
            {
                Outbound::Protocol(Message::InvocationResult {
                    result: Some(result),
                    ..
                }) => (
                    result["reader"]["channel_id"].as_str().unwrap().to_string(),
                    result["reader"]["access_key"].as_str().unwrap().to_string(),
                ),
                other => panic!("expected InvocationResult, got {other:?}"),
            }
        }

        let (tx, mut owner_rx) = mpsc::channel::<Outbound>(8);
        let owner = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(owner.clone());
        let (tx, mut other_rx) = mpsc::channel::<Outbound>(8);
        let other = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(other.clone());

        let (owned_id, owned_key) = create_channel(&engine, &owner, &mut owner_rx).await;
        let (kept_id, kept_key) = create_channel(&engine, &other, &mut other_rx).await;
        let mut waiting_reader = engine
            .channel_manager
            .take_receiver(&owned_id, &owned_key)
            .await
            .expect("reader attaches");

        engine.cleanup_worker(&owner).await;

        assert!(
            engine
                .channel_manager
                .get_channel(&owned_id, &owned_key)
                .is_none(),
            "the creator's channel must go with its connection"
        );
        let ended = tokio::time::timeout(Duration::from_secs(1), waiting_reader.recv())
            .await
            .expect("a reader must not keep waiting for a writer that can no longer attach");
        // The owner may have been the writer: its reader must not see a clean end.
        assert!(
            matches!(
                ended,
                Some(crate::workers::worker::channels::ChannelItem::Abort(_))
            ),
            "expected an abort, got {ended:?}"
        );
        assert!(waiting_reader.recv().await.is_none());
        assert!(
            engine
                .channel_manager
                .get_channel(&kept_id, &kept_key)
                .is_some(),
            "another connection's channel must survive"
        );
    }

    /// The removal path must find what the registration path wrote. Before the
    /// write path agreed on a namespace, this leaked the function permanently.
    #[tokio::test]
    async fn router_msg_unregister_function_in_namespace_removes_it() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        // See `router_msg_register_function_uses_connection_namespace`: the
        // namespace must reach the connection the way production delivers it.
        engine.begin_namespace_resolution(&worker);
        engine.resolve_connection_namespace(&worker, "orders").await;

        engine
            .router_msg(
                &worker,
                &Message::RegisterFunction {
                    id: "svc::my_func".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("register should succeed");
        assert!(engine.functions.get("orders", "svc::my_func").is_some());

        engine
            .router_msg(
                &worker,
                &Message::UnregisterFunction {
                    id: "svc::my_func".to_string(),
                },
            )
            .await
            .expect("unregister should succeed");

        assert!(
            engine.functions.get("orders", "svc::my_func").is_none(),
            "function must be removed from the namespace it was registered in"
        );
    }

    #[tokio::test]
    async fn test_router_msg_unregister_function() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        // First register a function
        let register_msg = Message::RegisterFunction {
            id: "removable_func".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };
        engine
            .router_msg(&worker, &register_msg)
            .await
            .expect("register should succeed");

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "removable_func")
                .is_some()
        );

        // Now unregister it
        let unregister_msg = Message::UnregisterFunction {
            id: "removable_func".to_string(),
        };
        engine
            .router_msg(&worker, &unregister_msg)
            .await
            .expect("unregister should succeed");

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "removable_func")
                .is_none(),
            "function should be removed after unregister"
        );

        let function_ids = worker.get_regular_function_ids().await;
        assert!(
            !function_ids.contains(&"removable_func".to_string()),
            "worker should no longer track the function id"
        );
    }

    /// Builds a session whose `function_registration_prefix` is set, with
    /// the rest of the fields at sensible defaults for register/unregister
    /// tests. Used by the prefix regression tests for iii-hq/iii#1508.
    fn session_with_prefix(prefix: &str) -> crate::workers::worker::rbac_session::Session {
        use crate::workers::worker::{WorkerManagerConfig, rbac_session::Session};
        use uuid::Uuid;
        Session {
            engine: Arc::new(Engine::new()),
            config: Arc::new(WorkerManagerConfig::default()),
            ip_address: "127.0.0.1".to_string(),
            session_id: Uuid::new_v4(),
            namespaces: Default::default(),
            allowed_functions: vec![],
            forbidden_functions: vec![],
            allow_function_registration: true,
            context: serde_json::json!({}),
            function_registration_prefix: Some(prefix.to_string()),
        }
    }

    /// `engine::*` is reserved for infrastructure (which lives in `default`). A
    /// worker registering an `engine::*` id in another namespace is refused, so
    /// it cannot inherit the infra RBAC carve-out / middleware bypass keyed by
    /// bare id. In `default` the claim refuses the ids the engine hosts, so an
    /// `engine::*` id the engine does not host is allowed there.
    #[tokio::test]
    async fn register_function_reserves_non_provider_engine_prefix_outside_default() {
        ensure_default_meter();
        let engine = Engine::new();

        // A worker resolved to a non-default namespace.
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(worker.clone());
        engine.begin_namespace_resolution(&worker);
        engine.resolve_connection_namespace(&worker, "orders").await;

        for function_id in ["engine::log::info", "engine::queue::custom"] {
            let reserved = Message::RegisterFunction {
                id: function_id.to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
                invocation: None,
                args_format: None,
                private: false,
                observable: true,
                observe: None,
            };
            engine
                .dispatch_msg(&worker, &reserved)
                .await
                .expect("dispatch");
            assert!(
                engine.functions.get("orders", function_id).is_none(),
                "reserved function {function_id} must not register outside default"
            );
        }

        // Queue providers are the narrow exception: enqueue dispatch and the
        // queue administration API resolve them in the project namespace.
        for function_id in super::NAMESPACE_SCOPED_QUEUE_FUNCTION_IDS {
            let queue_provider = Message::RegisterFunction {
                id: (*function_id).to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
                invocation: None,
                args_format: None,
                private: false,
                observable: true,
                observe: None,
            };
            engine
                .dispatch_msg(&worker, &queue_provider)
                .await
                .expect("dispatch queue provider");
            assert!(
                engine.functions.get("orders", function_id).is_some(),
                "queue provider {function_id} must register in a project namespace"
            );
        }

        // The same-shaped id in `default` is allowed.
        let (tx2, _rx2) = mpsc::channel::<Outbound>(8);
        let dflt = WorkerConnection::new(tx2);
        engine.worker_registry.register_worker(dflt.clone());
        engine.begin_namespace_resolution(&dflt);
        engine
            .resolve_connection_namespace(&dflt, DEFAULT_NAMESPACE)
            .await;
        let allowed = Message::RegisterFunction {
            id: "engine::custom::probe".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };
        engine
            .dispatch_msg(&dflt, &allowed)
            .await
            .expect("dispatch");
        assert!(
            engine
                .functions
                .get(DEFAULT_NAMESPACE, "engine::custom::probe")
                .is_some(),
            "engine::* in default is allowed"
        );
    }

    /// An id the engine registered has the engine as its host (v2 §7): a
    /// worker registering it in `default` is refused with
    /// `FUNCTION_NAMESPACE_CONFLICT`, naming no owner, and the engine's
    /// function and its options stay. Holds for every engine-hosted source
    /// and for a builtin.
    #[tokio::test]
    async fn a_worker_cannot_register_an_id_the_engine_hosts() {
        use crate::observer::events;
        ensure_default_meter();
        let engine = Engine::new();
        events::register_engine_event_sources(&engine);
        events::register_configuration_event_source(&engine);
        events::register_observability_event_sources(&engine);
        events::register_stream_event_sources(&engine);
        engine.register_function_handler(
            super::RegisterFunctionRequest {
                function_id: "engine::functions::list".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            super::Handler::new(|_: serde_json::Value| async move {
                FunctionResult::Success(Some(serde_json::json!({ "from": "engine" })))
            }),
        );
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(worker.clone());

        let sources = [
            events::ON_FUNCTIONS_CHANGED,
            events::ON_WORKERS_CHANGED,
            events::ON_OBSERVERS_CHANGED,
            events::CONFIGURATION_ON_UPDATED,
            events::OBSERVABILITY_ON_LOG,
            events::OBSERVABILITY_ON_SPAN,
            events::STREAM_ON_CHANGED,
            events::STREAM_ON_JOIN,
            events::STREAM_ON_LEAVE,
        ];
        for function_id in sources.iter().chain(&["engine::functions::list"]) {
            let shadow = Message::RegisterFunction {
                id: function_id.to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
                invocation: None,
                args_format: None,
                private: false,
                observable: true,
                observe: None,
            };
            engine.router_msg(&worker, &shadow).await.unwrap();
            let Message::RegistrationRejected {
                code,
                namespace,
                function_id: rejected,
                owner_worker_id,
                ..
            } = next_protocol_frame(&mut rx).await
            else {
                panic!("{function_id}: a rejection");
            };
            assert_eq!(
                code,
                crate::protocol::FUNCTION_NAMESPACE_CONFLICT,
                "{function_id}"
            );
            assert_eq!(namespace, DEFAULT_NAMESPACE);
            assert_eq!(rejected.as_deref(), Some(*function_id));
            assert!(owner_worker_id.is_empty(), "no worker owns {function_id}");
            assert!(
                !engine
                    .function_owners
                    .contains_key(&(DEFAULT_NAMESPACE.to_string(), function_id.to_string())),
                "{function_id}: the claim left no lease"
            );
            assert!(
                !worker
                    .get_regular_function_ids()
                    .await
                    .iter()
                    .any(|id| id == function_id),
                "{function_id}: not one of the worker's functions"
            );
        }

        for source in sources {
            assert!(
                engine.observers.options(DEFAULT_NAMESPACE, source).private,
                "{source} stays private"
            );
            assert_eq!(
                engine
                    .call(source, serde_json::json!({ "forged": true }))
                    .await
                    .unwrap_err()
                    .code,
                "FORBIDDEN",
                "{source}: the engine's handler answers"
            );
        }
        assert_eq!(
            engine
                .call("engine::functions::list", serde_json::json!({}))
                .await
                .unwrap(),
            Some(serde_json::json!({ "from": "engine" }))
        );

        // Its teardown leaves the engine's functions in place.
        engine.cleanup_worker(&worker).await;
        for function_id in sources.iter().chain(&["engine::functions::list"]) {
            assert!(
                engine
                    .functions
                    .get(DEFAULT_NAMESPACE, function_id)
                    .is_some(),
                "{function_id} survives the worker"
            );
        }
    }

    /// Regression: `function.unregister()` must honor
    /// `function_registration_prefix`. Before the fix, register prepended the
    /// prefix but unregister looked up the raw id, so the entry stayed in
    /// `engine.functions` forever (iii-hq/iii#1508).
    #[tokio::test]
    async fn test_router_msg_unregister_function_with_prefix() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::with_session(tx, session_with_prefix("test-prefix"));

        let register_msg = Message::RegisterFunction {
            id: "removable_func".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };
        engine
            .router_msg(&worker, &register_msg)
            .await
            .expect("register should succeed");

        assert!(
            engine
                .functions
                .get(
                    crate::protocol::DEFAULT_NAMESPACE,
                    "test-prefix::removable_func"
                )
                .is_some(),
            "function should be stored under the prefixed id"
        );
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "removable_func")
                .is_none(),
            "raw (unprefixed) id must not be used as the storage key"
        );

        let unregister_msg = Message::UnregisterFunction {
            id: "removable_func".to_string(),
        };
        engine
            .router_msg(&worker, &unregister_msg)
            .await
            .expect("unregister should succeed");

        assert!(
            engine
                .functions
                .get(
                    crate::protocol::DEFAULT_NAMESPACE,
                    "test-prefix::removable_func"
                )
                .is_none(),
            "prefixed function must be removed after unregister (iii-hq/iii#1508)"
        );
        let function_ids = worker.get_regular_function_ids().await;
        assert!(
            !function_ids.contains(&"test-prefix::removable_func".to_string()),
            "worker should no longer track the prefixed function id"
        );
    }

    /// Regression: the external-function branch of `UnregisterFunction`
    /// touches `http_module.unregister_http_function` and
    /// `service_registry.remove_function_from_services`, which are distinct
    /// call-sites from the regular branch. Verify the prefix is applied
    /// there too (iii-hq/iii#1508).
    #[tokio::test]
    async fn test_router_msg_unregister_external_function_with_prefix() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());

        let http_functions_config = HttpFunctionsConfig {
            security: SecurityConfig {
                require_https: false,
                block_private_ips: false,
                url_allowlist: vec!["*".to_string()],
            },
        };
        let http_functions_module = HttpFunctionsWorker::create(
            engine.clone(),
            Some(serde_json::to_value(&http_functions_config).expect("serialize config")),
        )
        .await
        .expect("create module");
        http_functions_module
            .initialize()
            .await
            .expect("initialize module");

        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::with_session(tx, session_with_prefix("test-prefix"));

        let register_msg = Message::RegisterFunction {
            id: "my_lambda".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: Some(HttpInvocationRef {
                url: "http://example.com/lambda".to_string(),
                method: crate::invocation::method::HttpMethod::Post,
                timeout_ms: Some(30000),
                headers: HashMap::new(),
                auth: None,
            }),
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };
        engine
            .router_msg(&worker, &register_msg)
            .await
            .expect("register external function should succeed");

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "test-prefix::my_lambda")
                .is_some()
        );
        assert!(
            worker
                .has_external_function_id("test-prefix::my_lambda")
                .await,
            "worker should track the prefixed external id"
        );
        let http_module = engine
            .service_registry
            .get_service::<HttpFunctionsWorker>("http_functions")
            .expect("http_functions service registered");
        assert!(
            http_module.http_functions().contains_key(&(
                DEFAULT_NAMESPACE.to_string(),
                "test-prefix::my_lambda".to_string()
            )),
            "http module should have the prefixed registration"
        );

        let unregister_msg = Message::UnregisterFunction {
            id: "my_lambda".to_string(),
        };
        engine
            .router_msg(&worker, &unregister_msg)
            .await
            .expect("unregister external function should succeed");

        assert!(
            !worker
                .has_external_function_id("test-prefix::my_lambda")
                .await,
            "worker must no longer track the prefixed external id"
        );
        assert!(
            !http_module.http_functions().contains_key(&(
                DEFAULT_NAMESPACE.to_string(),
                "test-prefix::my_lambda".to_string()
            )),
            "http module must drop the prefixed registration on unregister (iii-hq/iii#1508)"
        );
    }

    #[tokio::test]
    async fn test_router_msg_invoke_result() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let invocation_id = uuid::Uuid::new_v4();

        // Add the invocation to the worker so remove_invocation works
        worker.add_invocation(invocation_id).await;

        // Send an InvocationResult message without a matching invocation in the handler.
        // This exercises the "Did not find caller" branch but should still succeed.
        let msg = Message::InvocationResult {
            invocation_id,
            function_id: "some_func".to_string(),
            result: Some(serde_json::json!({"ok": true})),
            error: None,
            traceparent: None,
            baggage: None,
            observed: Vec::new(),
        };

        engine
            .router_msg(&worker, &msg)
            .await
            .expect("invoke result should succeed");

        // The invocation should have been removed from the worker
        let invocations = worker.invocations.read().await;
        assert!(
            !invocations.contains(&invocation_id),
            "invocation should be removed from worker"
        );
    }

    #[tokio::test]
    async fn test_router_msg_defer_invocation() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        // Register a function via the worker so it becomes a deferred handler
        let register_msg = Message::RegisterFunction {
            id: "deferred_func".to_string(),
            description: Some("Deferred function".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };
        engine
            .router_msg(&worker, &register_msg)
            .await
            .expect("register function should succeed");

        let invocation_id = uuid::Uuid::new_v4();

        // Send InvokeFunction which will go through the deferred path
        // (Worker-based handlers return FunctionResult::Deferred)
        let invoke_msg = Message::InvokeFunction {
            invocation_id: Some(invocation_id),
            function_id: "deferred_func".to_string(),
            data: serde_json::json!({"input": "test"}),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
            caller: None,
            observe_depth: None,
            activation: None,
        };

        engine
            .router_msg(&worker, &invoke_msg)
            .await
            .expect("invoke function should succeed");

        // Give the spawned task a chance to run
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // The worker channel should have received an InvokeFunction message
        // (the worker handler forwards the invocation to the worker via its channel)
        let mut found_invoke = false;
        while let Ok(outbound) = rx.try_recv() {
            if let Outbound::Protocol(Message::InvokeFunction { .. }) = outbound {
                found_invoke = true;
                break;
            }
        }
        assert!(
            found_invoke,
            "worker should receive an InvokeFunction message for the deferred invocation"
        );
    }

    #[tokio::test]
    async fn a_null_result_from_the_executor_reaches_the_caller_as_null() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        engine
            .router_msg(
                &worker,
                &Message::RegisterFunction {
                    id: "kv::get".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("register");

        let caller_invocation_id = uuid::Uuid::new_v4();
        engine
            .router_msg_owned(
                &worker,
                Message::InvokeFunction {
                    invocation_id: Some(caller_invocation_id),
                    function_id: "kv::get".to_string(),
                    data: json!({ "key": "missing" }),
                    traceparent: None,
                    baggage: None,
                    action: None,
                    metadata: None,
                    namespace: None,
                    caller: None,
                    observe_depth: None,
                    activation: None,
                },
            )
            .await
            .expect("invoke");

        // The executor side of the same worker receives the dispatch.
        let dispatched = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for the dispatch")
            .expect("channel open");
        let executor_invocation_id = match dispatched {
            Outbound::Protocol(Message::InvokeFunction {
                invocation_id: Some(id),
                ..
            }) => id,
            other => panic!("expected InvokeFunction, got {other:?}"),
        };

        // Reply exactly as the wire carries it: the frame says `null`.
        let frame = format!(
            r#"{{"type":"invocationresult","invocation_id":"{executor_invocation_id}","function_id":"kv::get","result":null}}"#
        );
        let reply: Message = serde_json::from_str(&frame).expect("frame parses");
        engine
            .router_msg_owned(&worker, reply)
            .await
            .expect("reply");

        let answered = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for the caller's result")
            .expect("channel open");
        match &answered {
            Outbound::Protocol(Message::InvocationResult {
                invocation_id,
                result,
                error,
                ..
            }) => {
                assert_eq!(*invocation_id, caller_invocation_id);
                assert_eq!(*result, Some(serde_json::Value::Null));
                assert!(error.is_none());
            }
            other => panic!("expected InvocationResult, got {other:?}"),
        }
        let Outbound::Protocol(message) = answered else {
            unreachable!()
        };
        let wire = serde_json::to_string(&message).expect("serializes");
        assert!(
            wire.contains(r#""result":null"#),
            "caller must see null: {wire}"
        );
    }

    #[tokio::test]
    async fn owned_remote_invocation_preserves_namespace_payload_and_metadata() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut caller_rx) = mpsc::channel::<Outbound>(8);
        let caller = WorkerConnection::new(tx);
        let (tx, mut executor_rx) = mpsc::channel::<Outbound>(8);
        let executor = WorkerConnection::new(tx);
        engine.begin_namespace_resolution(&executor);
        engine
            .resolve_connection_namespace(&executor, "orders")
            .await;
        engine.cancel_namespace_grace_timer(&executor.id);
        engine
            .router_msg_owned(
                &executor,
                Message::RegisterFunction {
                    id: "orders::echo".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("register remote function");

        let invocation_id = uuid::Uuid::new_v4();
        let payload = json!({"items": [1, null, {"name": "example"}]});
        let metadata = json!({"routing": {"team": "fulfillment"}});
        engine
            .router_msg_owned(
                &caller,
                Message::InvokeFunction {
                    invocation_id: Some(invocation_id),
                    function_id: "orders::echo".to_string(),
                    data: payload.clone(),
                    traceparent: None,
                    baggage: None,
                    action: None,
                    metadata: Some(metadata.clone()),
                    namespace: Some("orders".to_string()),
                    caller: None,
                    observe_depth: None,
                    activation: None,
                },
            )
            .await
            .expect("invoke remote function in its namespace");

        let dispatched = tokio::time::timeout(Duration::from_secs(1), executor_rx.recv())
            .await
            .expect("remote dispatch should complete")
            .expect("executor channel is open");
        let Outbound::Protocol(Message::InvokeFunction {
            invocation_id: Some(executor_invocation_id),
            function_id,
            mut data,
            metadata: received_metadata,
            ..
        }) = dispatched
        else {
            panic!("expected remote invocation, got {dispatched:?}");
        };
        assert_eq!(function_id, "orders::echo");
        assert_eq!(received_metadata, Some(metadata));
        assert_eq!(
            data.as_object_mut().unwrap().remove("_caller_worker_id"),
            Some(json!(caller.id.to_string()))
        );
        assert_eq!(data, payload);

        engine
            .router_msg_owned(
                &executor,
                Message::InvocationResult {
                    invocation_id: executor_invocation_id,
                    function_id,
                    result: Some(data),
                    error: None,
                    traceparent: None,
                    baggage: None,
                    observed: Vec::new(),
                },
            )
            .await
            .expect("deliver remote response");
        let response = tokio::time::timeout(Duration::from_secs(1), caller_rx.recv())
            .await
            .expect("caller should receive a response")
            .expect("caller channel is open");
        let Outbound::Protocol(Message::InvocationResult {
            invocation_id: response_id,
            result,
            error,
            ..
        }) = response
        else {
            panic!("expected invocation result, got {response:?}");
        };
        assert_eq!(response_id, invocation_id);
        assert_eq!(result, Some(payload));
        assert!(error.is_none());
        assert_eq!(caller.invocation_count().await, 0);
        assert_eq!(executor.invocation_count().await, 0);
    }

    #[tokio::test]
    async fn owned_void_invocation_executes_without_replying_to_supplied_id() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let caller = WorkerConnection::new(tx);
        let (executed_tx, mut executed_rx) = mpsc::channel(1);
        engine.register_function_handler(
            make_request("orders::notify"),
            super::Handler::new(move |input| {
                let executed_tx = executed_tx.clone();
                async move {
                    executed_tx.send(input).await.expect("record execution");
                    FunctionResult::Success(Some(json!({"sent": true})))
                }
            }),
        );
        engine
            .router_msg_owned(
                &caller,
                Message::InvokeFunction {
                    invocation_id: Some(uuid::Uuid::new_v4()),
                    function_id: "orders::notify".to_string(),
                    data: json!(["first", "second"]),
                    traceparent: None,
                    baggage: None,
                    action: Some(crate::protocol::CallAction::Void),
                    metadata: None,
                    namespace: None,
                    caller: None,
                    observe_depth: None,
                    activation: None,
                },
            )
            .await
            .expect("dispatch void call");

        let executed = tokio::time::timeout(Duration::from_secs(1), executed_rx.recv())
            .await
            .expect("handler should execute");
        assert_eq!(executed, Some(json!(["first", "second"])));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), rx.recv())
                .await
                .is_err(),
            "a void call must not produce an invocation result"
        );
        assert_eq!(caller.invocation_count().await, 0);
    }

    #[tokio::test]
    async fn test_router_msg_invoke_function_success_sends_invocation_result() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        engine.register_function_handler(
            make_request("engine::success"),
            super::Handler::new(|input| async move {
                FunctionResult::Success(Some(json!({ "echo": input })))
            }),
        );

        let invocation_id = uuid::Uuid::new_v4();
        let invoke_msg = Message::InvokeFunction {
            invocation_id: Some(invocation_id),
            function_id: "engine::success".to_string(),
            data: json!({ "value": 1 }),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
            caller: None,
            observe_depth: None,
            activation: None,
        };

        engine
            .router_msg(&worker, &invoke_msg)
            .await
            .expect("invoke should succeed");

        let outbound = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for invocation result")
            .expect("channel should produce invocation result");

        match outbound {
            Outbound::Protocol(Message::InvocationResult {
                invocation_id: got_invocation_id,
                function_id,
                result,
                error,
                ..
            }) => {
                assert_eq!(got_invocation_id, invocation_id);
                assert_eq!(function_id, "engine::success");
                assert_eq!(
                    result,
                    Some(json!({
                        "echo": {
                            "_caller_worker_id": worker.id.to_string(),
                            "value": 1
                        }
                    }))
                );
                assert!(error.is_none());
            }
            other => panic!("expected InvocationResult, got {other:?}"),
        }

        assert_eq!(worker.invocation_count().await, 0);
    }

    #[tokio::test]
    async fn enqueue_action_routes_through_registered_queue_provider() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        let captured = Arc::new(Mutex::new(None));
        let captured_for_handler = captured.clone();

        engine.register_function_handler(
            make_request(super::ENQUEUE_PROVIDER_FUNCTION_ID),
            super::Handler::new(move |input| {
                let captured = captured_for_handler.clone();
                async move {
                    *captured.lock().unwrap() = Some(input);
                    FunctionResult::Success(Some(json!({
                        "messageReceiptId": "provider-must-not-own-the-receipt"
                    })))
                }
            }),
        );

        let invocation_id = uuid::Uuid::new_v4();
        engine
            .router_msg(
                &worker,
                &Message::InvokeFunction {
                    invocation_id: Some(invocation_id),
                    function_id: "harness::turn".to_string(),
                    data: json!({"session_id": "s1"}),
                    traceparent: Some(
                        "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01".to_string(),
                    ),
                    baggage: Some("iii.session.id=s1,iii.function.id=harness::send".to_string()),
                    action: Some(crate::protocol::CallAction::Enqueue {
                        queue: "harness-turn".to_string(),
                    }),
                    metadata: None,
                    namespace: None,
                    caller: None,
                    observe_depth: None,
                    activation: None,
                },
            )
            .await
            .unwrap();

        let outbound = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for enqueue acknowledgement")
            .expect("channel should produce an invocation result");
        let result = match outbound {
            Outbound::Protocol(Message::InvocationResult {
                invocation_id: got_id,
                result,
                error,
                ..
            }) => {
                assert_eq!(got_id, invocation_id);
                assert!(error.is_none());
                result.unwrap()
            }
            other => panic!("expected InvocationResult, got {other:?}"),
        };
        let receipt = result["messageReceiptId"]
            .as_str()
            .expect("engine should return a receipt");
        assert_ne!(receipt, "provider-must-not-own-the-receipt");

        let input = captured.lock().unwrap().clone().unwrap();
        assert_eq!(input["queue"], "harness-turn");
        assert_eq!(input["function_id"], "harness::turn");
        assert_eq!(input["data"], json!({"session_id": "s1"}));
        assert!(
            input.get("namespace").is_none(),
            "default namespace must be omitted for legacy queue providers: {input}"
        );
        assert_eq!(input["messageReceiptId"], receipt);
        assert_eq!(
            input["traceparent"],
            "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01"
        );
        let queued_baggage = input["baggage"].as_str().unwrap();
        assert!(queued_baggage.contains("iii.session.id=s1"));
        assert!(queued_baggage.contains("iii.function.id=harness::turn"));
        assert!(!queued_baggage.contains("harness::send"));
    }

    #[tokio::test]
    async fn enqueue_action_prefers_provider_in_target_namespace() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        let captured = Arc::new(Mutex::new(None));
        let captured_for_handler = captured.clone();
        let legacy_calls = Arc::new(AtomicUsize::new(0));
        let legacy_calls_for_handler = legacy_calls.clone();

        engine.register_function_handler(
            make_request(super::ENQUEUE_PROVIDER_FUNCTION_ID),
            super::Handler::new(move |_input| {
                legacy_calls_for_handler.fetch_add(1, Ordering::SeqCst);
                async move { FunctionResult::Success(None) }
            }),
        );
        engine.register_function_handler_ns(
            "project-a",
            make_request(super::ENQUEUE_PROVIDER_FUNCTION_ID),
            super::Handler::new(move |input| {
                let captured = captured_for_handler.clone();
                async move {
                    *captured.lock().unwrap() = Some(input);
                    FunctionResult::Success(None)
                }
            }),
        );

        let invocation_id = uuid::Uuid::new_v4();
        engine
            .router_msg(
                &worker,
                &Message::InvokeFunction {
                    invocation_id: Some(invocation_id),
                    function_id: "harness::turn".to_string(),
                    data: json!({"session_id": "s1"}),
                    traceparent: None,
                    baggage: None,
                    action: Some(crate::protocol::CallAction::Enqueue {
                        queue: "harness-turn".to_string(),
                    }),
                    metadata: None,
                    namespace: Some("project-a".to_string()),
                    caller: None,
                    observe_depth: None,
                    activation: None,
                },
            )
            .await
            .unwrap();

        let outbound = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for enqueue acknowledgement")
            .expect("channel should produce an invocation result");
        match outbound {
            Outbound::Protocol(Message::InvocationResult { error, .. }) => {
                assert!(error.is_none());
            }
            other => panic!("expected InvocationResult, got {other:?}"),
        }

        let input = captured.lock().unwrap().clone().unwrap();
        assert_eq!(input["namespace"], "project-a");
        assert_eq!(legacy_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn enqueue_action_rejects_legacy_provider_for_namespaced_target() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        let legacy_calls = Arc::new(AtomicUsize::new(0));
        let legacy_calls_for_handler = legacy_calls.clone();

        engine.register_function_handler(
            make_request(super::ENQUEUE_PROVIDER_FUNCTION_ID),
            super::Handler::new(move |input| {
                legacy_calls_for_handler.fetch_add(1, Ordering::SeqCst);
                async move {
                    if input.get("namespace").is_some() {
                        FunctionResult::Failure(crate::protocol::ErrorBody::new(
                            "unknown_field",
                            "legacy provider rejects the namespace field",
                        ))
                    } else {
                        FunctionResult::Success(None)
                    }
                }
            }),
        );

        engine
            .router_msg(
                &worker,
                &Message::InvokeFunction {
                    invocation_id: Some(uuid::Uuid::new_v4()),
                    function_id: "harness::turn".to_string(),
                    data: json!({"session_id": "s1"}),
                    traceparent: None,
                    baggage: None,
                    action: Some(crate::protocol::CallAction::Enqueue {
                        queue: "harness-turn".to_string(),
                    }),
                    metadata: None,
                    namespace: Some("project-a".to_string()),
                    caller: None,
                    observe_depth: None,
                    activation: None,
                },
            )
            .await
            .unwrap();

        let outbound = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for enqueue acknowledgement")
            .expect("channel should produce an invocation result");
        match outbound {
            Outbound::Protocol(Message::InvocationResult { error, .. }) => {
                let error = error.expect("namespaced enqueue must reject a legacy provider");
                assert_eq!(error.code, "enqueue_error");
                assert!(error.message.contains("namespace-aware release"));
                assert!(error.message.contains("project-a"));
            }
            other => panic!("expected InvocationResult, got {other:?}"),
        }

        assert_eq!(
            legacy_calls.load(Ordering::SeqCst),
            0,
            "legacy provider must not receive a namespaced enqueue"
        );
    }

    #[tokio::test]
    async fn enqueue_action_fails_closed_without_queue_provider() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        engine
            .router_msg(
                &worker,
                &Message::InvokeFunction {
                    invocation_id: Some(uuid::Uuid::new_v4()),
                    function_id: "harness::turn".to_string(),
                    data: json!({"session_id": "s1"}),
                    traceparent: None,
                    baggage: None,
                    action: Some(crate::protocol::CallAction::Enqueue {
                        queue: "harness-turn".to_string(),
                    }),
                    metadata: None,
                    namespace: None,
                    caller: None,
                    observe_depth: None,
                    activation: None,
                },
            )
            .await
            .unwrap();

        let outbound = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for enqueue failure")
            .expect("channel should produce an invocation result");
        match outbound {
            Outbound::Protocol(Message::InvocationResult { error, .. }) => {
                let error = error.expect("enqueue should fail without a provider");
                assert_eq!(error.code, "enqueue_error");
                assert!(error.message.contains("engine::queue::enqueue"));
                // DX: the fail-closed error must tell the user how to fix it.
                assert!(
                    error.message.contains(
                        "iii call -n <compose-daemon-namespace> compose::add worker=queue"
                    ),
                    "expected install guidance, got: {}",
                    error.message
                );
                assert!(
                    !error.message.contains("iii call -n default"),
                    "target function namespace must not be presented as the Compose daemon namespace: {}",
                    error.message
                );
            }
            other => panic!("expected InvocationResult, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_router_msg_invoke_function_failure_sends_invocation_error() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        engine.register_function_handler(
            make_request("engine::failure"),
            super::Handler::new(|_input| async move {
                FunctionResult::Failure(crate::protocol::ErrorBody {
                    code: "boom".to_string(),
                    message: "handler failed".to_string(),
                    stacktrace: None,
                })
            }),
        );

        let invocation_id = uuid::Uuid::new_v4();
        let invoke_msg = Message::InvokeFunction {
            invocation_id: Some(invocation_id),
            function_id: "engine::failure".to_string(),
            data: json!({ "value": 2 }),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
            caller: None,
            observe_depth: None,
            activation: None,
        };

        engine
            .router_msg(&worker, &invoke_msg)
            .await
            .expect("invoke should succeed");

        let outbound = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for invocation result")
            .expect("channel should produce invocation result");

        match outbound {
            Outbound::Protocol(Message::InvocationResult {
                invocation_id: got_invocation_id,
                function_id,
                result,
                error,
                ..
            }) => {
                assert_eq!(got_invocation_id, invocation_id);
                assert_eq!(function_id, "engine::failure");
                assert!(result.is_none());
                let error = error.expect("error should be present");
                assert_eq!(error.code, "boom");
                assert_eq!(error.message, "handler failed");
            }
            other => panic!("expected InvocationResult, got {other:?}"),
        }

        assert_eq!(worker.invocation_count().await, 0);
    }

    #[tokio::test]
    async fn test_router_msg_invoke_function_missing_handler_sends_not_found() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let invocation_id = uuid::Uuid::new_v4();
        let invoke_msg = Message::InvokeFunction {
            invocation_id: Some(invocation_id),
            function_id: "engine::missing".to_string(),
            data: json!({}),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
            caller: None,
            observe_depth: None,
            activation: None,
        };

        engine
            .router_msg(&worker, &invoke_msg)
            .await
            .expect("invoke should succeed");

        let outbound = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for invocation result")
            .expect("channel should produce invocation result");

        match outbound {
            Outbound::Protocol(Message::InvocationResult {
                invocation_id: got_invocation_id,
                function_id,
                result,
                error,
                ..
            }) => {
                assert_eq!(got_invocation_id, invocation_id);
                assert_eq!(function_id, "engine::missing");
                assert!(result.is_none());
                let error = error.expect("error should be present");
                assert_eq!(error.code, "function_not_found");
            }
            other => panic!("expected InvocationResult, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn removed_worker_function_points_to_compose_and_migration_guide() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let invocation_id = uuid::Uuid::new_v4();
        engine
            .router_msg(
                &worker,
                &Message::InvokeFunction {
                    invocation_id: Some(invocation_id),
                    function_id: "worker::add".to_string(),
                    data: json!({}),
                    traceparent: None,
                    baggage: None,
                    action: None,
                    metadata: None,
                    namespace: None,
                    caller: None,
                    observe_depth: None,
                    activation: None,
                },
            )
            .await
            .expect("invoke should produce a structured error");

        let outbound = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for invocation result")
            .expect("channel should produce invocation result");

        let Outbound::Protocol(Message::InvocationResult { error, .. }) = outbound else {
            panic!("expected InvocationResult");
        };
        let error = error.expect("removed function should return an error");
        assert_eq!(error.code, "function_not_found");
        assert!(error.message.contains("Use compose::add instead"));
        assert!(
            error
                .message
                .contains("https://iii.dev/docs/upgrading/workers-to-compose")
        );
    }

    // ---------------------------------------------------------------
    // 2. Engine state management tests
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_engine_new() {
        ensure_default_meter();
        let engine = Engine::new();

        // All registries should be empty
        assert_eq!(
            engine.functions.functions.len(),
            0,
            "functions registry should be empty"
        );
        assert!(engine.observers.observers().is_empty(), "no observers");
        assert_eq!(
            engine.worker_registry.workers.len(),
            0,
            "worker registry should be empty"
        );
    }

    #[tokio::test]
    async fn test_engine_send_msg() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let sent = engine.send_msg(&worker, Message::Ping).await;
        assert!(sent, "send_msg should return true on success");

        let received = rx.recv().await.expect("should receive a message");
        match received {
            Outbound::Protocol(Message::Ping) => {} // expected
            other => panic!("expected Ping, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_engine_send_msg_returns_false_when_channel_closed() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, rx) = mpsc::channel::<Outbound>(1);
        drop(rx);
        let worker = WorkerConnection::new(tx);

        let sent = engine.send_msg(&worker, Message::Ping).await;
        assert!(!sent, "send_msg should return false on closed channels");
    }

    #[tokio::test]
    async fn test_engine_call_success_failure_missing_and_serialization_error() {
        ensure_default_meter();
        let engine = Engine::new();

        engine.register_function_handler(
            make_request("engine::call_ok"),
            super::Handler::new(|input| async move {
                FunctionResult::Success(Some(json!({ "payload": input })))
            }),
        );
        engine.register_function_handler(
            make_request("engine::call_fail"),
            super::Handler::new(|_input| async move {
                FunctionResult::Failure(crate::protocol::ErrorBody {
                    code: "call_failed".to_string(),
                    message: "call handler failed".to_string(),
                    stacktrace: None,
                })
            }),
        );

        let ok = engine
            .call("engine::call_ok", json!({ "hello": "world" }))
            .await
            .expect("success call should succeed");
        assert_eq!(ok, Some(json!({ "payload": { "hello": "world" } })));

        let err = engine
            .call("engine::call_fail", json!({ "hello": "world" }))
            .await
            .expect_err("failure call should return ErrorBody");
        assert_eq!(err.code, "call_failed");

        let missing = engine
            .call("engine::does_not_exist", json!({}))
            .await
            .expect_err("missing function should return ErrorBody");
        assert_eq!(missing.code, "function_not_found");

        let serialization = engine
            .call("engine::call_ok", FailingSerialize)
            .await
            .expect_err("serialize failure should return ErrorBody");
        assert_eq!(serialization.code, "serialization_error");
    }

    #[tokio::test]
    async fn test_router_msg_register_http_invocation_without_http_module_is_ignored() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let register_message = Message::RegisterFunction {
            id: "external.without_module".to_string(),
            description: Some("external function".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: Some(HttpInvocationRef {
                url: "http://example.com/lambda".to_string(),
                method: crate::invocation::method::HttpMethod::Post,
                timeout_ms: Some(30000),
                headers: HashMap::new(),
                auth: None,
            }),
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };

        engine
            .router_msg(&worker, &register_message)
            .await
            .expect("register message should not fail");

        assert!(
            engine
                .functions
                .get(
                    crate::protocol::DEFAULT_NAMESPACE,
                    "external.without_module"
                )
                .is_none()
        );
        assert!(
            !worker
                .has_external_function_id("external.without_module")
                .await
        );
    }

    #[tokio::test]
    async fn test_router_msg_unregister_external_without_http_module_removes_function() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        // Ownership gate on UnregisterFunction requires the worker to be the
        // recorded owner. This test sidesteps `router_msg` to seed state, so
        // it claims the id before registering it, as the registration does.
        engine
            .claim_external_function(
                crate::protocol::DEFAULT_NAMESPACE,
                worker.id,
                "external.cleanup",
            )
            .expect("uncontested claim");
        engine.register_function_handler(
            make_request("external.cleanup"),
            super::Handler::new(|_input| async move { FunctionResult::Success(None) }),
        );
        engine.service_registry.register_service_from_function_id(
            crate::protocol::DEFAULT_NAMESPACE,
            "external.cleanup",
        );
        worker
            .include_external_function_id("external.cleanup")
            .await;

        engine
            .router_msg(
                &worker,
                &Message::UnregisterFunction {
                    id: "external.cleanup".to_string(),
                },
            )
            .await
            .expect("unregister should succeed");

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "external.cleanup")
                .is_none()
        );
        assert!(!worker.has_external_function_id("external.cleanup").await);
        assert!(!engine.service_registry.services.contains_key(&(
            crate::protocol::DEFAULT_NAMESPACE.to_string(),
            "external".to_string()
        )));
    }

    #[tokio::test]
    async fn test_engine_remember_invocation() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        // Attempting to remember an invocation for a non-existent function
        // should return a function_not_found error
        let result = engine
            .remember_invocation(
                &worker,
                Some(uuid::Uuid::new_v4()),
                "nonexistent_func",
                None,
                serde_json::json!({}),
                None,
                None,
                None,
                None,
                None,
            )
            .await;

        match result {
            Ok(Err(err)) => {
                assert_eq!(err.code, "function_not_found");
            }
            other => panic!(
                "expected Ok(Err(function_not_found)), got {:?}",
                other.map(|r| r.map(|_| "Ok(...)").map_err(|e| e.code))
            ),
        }
    }

    #[tokio::test]
    async fn test_engine_remove_function() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        // Register a function
        let msg = Message::RegisterFunction {
            id: "to_remove".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };
        engine
            .router_msg(&worker, &msg)
            .await
            .expect("register should succeed");

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "to_remove")
                .is_some()
        );

        // Remove it directly
        engine.remove_function(crate::protocol::DEFAULT_NAMESPACE, "to_remove");

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "to_remove")
                .is_none(),
            "function should be removed"
        );
    }

    // ---------------------------------------------------------------
    // 3. Worker cleanup tests
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_cleanup_worker_removes_functions() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        // Register the worker in the registry so cleanup can unregister it
        engine.worker_registry.register_worker(worker.clone());

        // Register multiple functions via router_msg
        for name in &["cleanup_func_a", "cleanup_func_b", "cleanup_func_c"] {
            let msg = Message::RegisterFunction {
                id: name.to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
                invocation: None,
                args_format: None,
                private: false,
                observable: true,
                observe: None,
            };
            engine
                .router_msg(&worker, &msg)
                .await
                .expect("register should succeed");
        }

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "cleanup_func_a")
                .is_some()
        );
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "cleanup_func_b")
                .is_some()
        );
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "cleanup_func_c")
                .is_some()
        );

        // Cleanup the worker
        engine.cleanup_worker(&worker).await;

        // All functions should be removed
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "cleanup_func_a")
                .is_none(),
            "cleanup_func_a should be removed"
        );
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "cleanup_func_b")
                .is_none(),
            "cleanup_func_b should be removed"
        );
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "cleanup_func_c")
                .is_none(),
            "cleanup_func_c should be removed"
        );

        // Worker should be unregistered from worker registry
        assert!(
            engine.worker_registry.get_worker(&worker.id).is_none(),
            "worker should be unregistered"
        );
    }

    // ---------------------------------------------------------------
    // 4. handle_telemetry_frame tests
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_handle_telemetry_frame_traces() {
        ensure_default_meter();
        let peer: std::net::SocketAddr = "127.0.0.1:9999".parse().unwrap();

        // Construct a frame with the OTLP prefix followed by valid (but empty) JSON
        let mut frame = Vec::from(b"OTLP" as &[u8]);
        frame.extend_from_slice(b"{}");

        let handled = super::handle_telemetry_frame(&frame, &peer).await;
        assert!(handled, "OTLP-prefixed frame should be handled");
    }

    #[tokio::test]
    async fn test_handle_telemetry_frame_metrics() {
        ensure_default_meter();
        let peer: std::net::SocketAddr = "127.0.0.1:9999".parse().unwrap();

        // Construct a frame with the MTRC prefix followed by valid (but empty) JSON
        let mut frame = Vec::from(b"MTRC" as &[u8]);
        frame.extend_from_slice(b"{}");

        let handled = super::handle_telemetry_frame(&frame, &peer).await;
        assert!(handled, "MTRC-prefixed frame should be handled");
    }

    #[tokio::test]
    async fn test_handle_telemetry_frame_logs() {
        ensure_default_meter();
        let peer: std::net::SocketAddr = "127.0.0.1:9999".parse().unwrap();

        // Construct a frame with the LOGS prefix followed by valid (but empty) JSON
        let mut frame = Vec::from(b"LOGS" as &[u8]);
        frame.extend_from_slice(b"{}");

        let handled = super::handle_telemetry_frame(&frame, &peer).await;
        assert!(handled, "LOGS-prefixed frame should be handled");
    }

    #[tokio::test]
    async fn test_handle_telemetry_frame_unknown_prefix() {
        ensure_default_meter();
        let peer: std::net::SocketAddr = "127.0.0.1:9999".parse().unwrap();

        // A frame without any known telemetry prefix should not be handled
        let frame = b"UNKNOWN some data here";

        let handled = super::handle_telemetry_frame(frame, &peer).await;
        assert!(!handled, "unknown prefix should not be handled");
    }

    #[tokio::test]
    async fn test_handle_telemetry_frame_invalid_utf8() {
        ensure_default_meter();
        let peer: std::net::SocketAddr = "127.0.0.1:9999".parse().unwrap();

        // OTLP prefix followed by invalid UTF-8 bytes
        let mut frame = Vec::from(b"OTLP" as &[u8]);
        frame.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x80]);

        let handled = super::handle_telemetry_frame(&frame, &peer).await;
        assert!(
            handled,
            "OTLP frame with invalid UTF-8 should still be handled (returns early with true)"
        );
    }

    // =========================================================================
    // router_msg: Ping / Pong / WorkerRegistered
    // =========================================================================

    #[tokio::test]
    async fn test_router_msg_ping_sends_pong() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        engine
            .router_msg(&worker, &Message::Ping)
            .await
            .expect("Ping should succeed");

        // Engine should send Pong back through the channel
        let outbound = rx.try_recv().expect("should have received a message");
        match outbound {
            Outbound::Protocol(msg) => {
                assert!(
                    matches!(msg, Message::Pong),
                    "Expected Pong message, got {:?}",
                    msg
                );
            }
            other => panic!("Expected Protocol message, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_router_msg_pong_is_noop() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        engine
            .router_msg(&worker, &Message::Pong)
            .await
            .expect("Pong should succeed");

        // No message should be sent back
        assert!(
            rx.try_recv().is_err(),
            "Pong should not produce any outbound message"
        );
    }

    #[tokio::test]
    async fn test_router_msg_worker_registered_is_noop() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let msg = Message::WorkerRegistered {
            worker_id: "some-worker-id".to_string(),
            reattach_token: None,
            features: Vec::new(),
        };

        engine
            .router_msg(&worker, &msg)
            .await
            .expect("WorkerRegistered should succeed (no-op)");

        // Should not produce any response
        assert!(
            rx.try_recv().is_err(),
            "WorkerRegistered should not produce any outbound message"
        );
    }

    // ── A namespace named and left blank ─────────────────────────────────
    //
    // The SDKs each refuse this at construction, in four different ways, and
    // one of them (the browser) did not refuse it at all. The engine is the
    // one party every client goes through, so it refuses too -- and it refuses
    // rather than reading blank as absent, because the two ask for opposite
    // things.

    #[tokio::test]
    async fn a_blank_namespace_on_invoke_is_answered_not_routed() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(worker.clone());

        engine
            .router_msg(
                &worker,
                &Message::InvokeFunction {
                    invocation_id: Some(uuid::Uuid::new_v4()),
                    function_id: "svc::run".to_string(),
                    data: serde_json::json!({}),
                    traceparent: None,
                    baggage: None,
                    action: None,
                    metadata: None,
                    namespace: Some(String::new()),
                    caller: None,
                    observe_depth: None,
                    activation: None,
                },
            )
            .await
            .expect("the message is handled, not dropped");

        let out = rx.try_recv().expect("the caller must be answered");
        let Outbound::Protocol(Message::InvocationResult { error, .. }) = out else {
            panic!("expected an InvocationResult, got {out:?}");
        };
        let error = error.expect("a blank namespace is an error, not a miss");
        assert_eq!(error.code, crate::protocol::INVALID_NAMESPACE);
    }

    // =========================================================================
    // router_msg: RegisterService
    // =========================================================================

    #[tokio::test]
    async fn test_router_msg_register_service() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let msg = Message::RegisterService {
            id: "service-1".to_string(),
            name: "my-service".to_string(),
            description: Some("A test service".to_string()),
            parent_service_id: None,
        };

        engine
            .router_msg(&worker, &msg)
            .await
            .expect("RegisterService should succeed");

        // Verify the service was registered
        assert!(
            engine.service_registry.services.contains_key(&(
                crate::protocol::DEFAULT_NAMESPACE.to_string(),
                "my-service".to_string()
            )),
            "Service should be registered in the service registry"
        );
    }

    #[tokio::test]
    async fn test_router_msg_register_service_without_description() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let msg = Message::RegisterService {
            id: "service-2".to_string(),
            name: "minimal-service".to_string(),
            description: None,
            parent_service_id: None,
        };

        engine
            .router_msg(&worker, &msg)
            .await
            .expect("RegisterService without description should succeed");

        assert!(engine.service_registry.services.contains_key(&(
            crate::protocol::DEFAULT_NAMESPACE.to_string(),
            "minimal-service".to_string()
        )));
    }

    // =========================================================================
    // router_msg: InvocationResult with error
    // =========================================================================

    #[tokio::test]
    async fn test_router_msg_invocation_result_with_error() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let invocation_id = uuid::Uuid::new_v4();
        worker.add_invocation(invocation_id).await;

        let msg = Message::InvocationResult {
            invocation_id,
            function_id: "some_func".to_string(),
            result: None,
            error: Some(crate::protocol::ErrorBody {
                code: "timeout".to_string(),
                message: "Function timed out".to_string(),
                stacktrace: None,
            }),
            traceparent: None,
            baggage: None,
            observed: Vec::new(),
        };

        engine
            .router_msg(&worker, &msg)
            .await
            .expect("InvocationResult with error should succeed");

        // Invocation should have been removed from worker
        let invocations = worker.invocations.read().await;
        assert!(!invocations.contains(&invocation_id));
    }

    // =========================================================================
    // cleanup_worker: no functions registered (empty worker)
    // =========================================================================

    #[tokio::test]
    async fn test_cleanup_worker_empty_worker() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        // Register the worker
        engine.worker_registry.register_worker(worker.clone());
        assert!(engine.worker_registry.workers.contains_key(&worker.id));

        // Cleanup an empty worker (no functions, no invocations)
        engine.cleanup_worker(&worker).await;

        // Worker should be unregistered
        assert!(!engine.worker_registry.workers.contains_key(&worker.id));
    }

    #[tokio::test]
    async fn test_cleanup_worker_with_registered_functions() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        // Register the worker
        engine.worker_registry.register_worker(worker.clone());

        // Register a function via the worker
        let msg = Message::RegisterFunction {
            id: "cleanup_func".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };
        engine
            .router_msg(&worker, &msg)
            .await
            .expect("register should succeed");

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "cleanup_func")
                .is_some()
        );

        // Now cleanup
        engine.cleanup_worker(&worker).await;

        // Function should be removed
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "cleanup_func")
                .is_none()
        );
        // Worker should be unregistered
        assert!(!engine.worker_registry.workers.contains_key(&worker.id));
    }

    #[tokio::test]
    async fn test_cleanup_worker_preserves_function_owned_by_another_worker() {
        // Regression guard for the dev-loop reload race: the old worker's
        // disconnect cleanup used to fire after a new worker had already
        // re-registered the same function_id, and cleanup_worker would
        // unconditionally remove the function from the engine's global
        // registry — deleting the new worker's fresh registration. The
        // observable symptom was "change a file and endpoints stop
        // working until the next reload". Post-fix, cleanup_worker walks
        // worker_registry to see if any other live worker still claims
        // the function_id and skips the remove in that case.
        ensure_default_meter();
        let engine = Engine::new();

        // OLD worker — connected first, registers `shared_func`.
        let (tx_old, _rx_old) = mpsc::channel::<Outbound>(8);
        let old_worker = WorkerConnection::new(tx_old);
        engine.worker_registry.register_worker(old_worker.clone());
        let register_msg = Message::RegisterFunction {
            id: "shared_func".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };
        engine
            .router_msg(&old_worker, &register_msg)
            .await
            .expect("old worker register should succeed");
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "shared_func")
                .is_some()
        );

        // The old connection drops out of the registry, but its
        // `cleanup_worker` has not run yet — the fast-restart race, where
        // the host watcher spawns a new VM process that beats the engine's
        // disconnect handling for the old one. Only a departed incumbent's
        // lease is takeable; while it is still registered, the newcomer is a
        // namespace conflict and `claim_function` rejects it.
        engine.worker_registry.unregister_worker(&old_worker.id);

        // NEW worker connects, re-registers the same function_id.
        let (tx_new, _rx_new) = mpsc::channel::<Outbound>(8);
        let new_worker = WorkerConnection::new(tx_new);
        engine.worker_registry.register_worker(new_worker.clone());
        engine
            .router_msg(&new_worker, &register_msg)
            .await
            .expect("new worker register should succeed");
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "shared_func")
                .is_some(),
            "new worker's registration should be in the function_registry"
        );

        // Now the OLD worker's cleanup fires — simulating the late
        // disconnect-detection path. Without the ownership check,
        // this would remove shared_func out from under the new
        // worker.
        engine.cleanup_worker(&old_worker).await;

        // Function must still be registered: the new worker owns it.
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "shared_func")
                .is_some(),
            "cleanup of old worker must not remove a function owned by a live new worker"
        );
        // The old worker itself should be unregistered.
        assert!(!engine.worker_registry.workers.contains_key(&old_worker.id));
        // And the new worker should still be in the registry.
        assert!(
            engine.worker_registry.workers.contains_key(&new_worker.id),
            "new worker should remain registered after old worker cleanup"
        );
    }

    #[tokio::test]
    async fn test_cleanup_worker_preserves_external_function_owned_by_another_worker() {
        // HTTP-invocation variant of the regular-function regression: when an
        // HTTP-invocation function is re-registered by a fresh worker before
        // the old worker's cleanup fires, cleanup_worker must not unregister
        // the function from the http_functions module nor from the engine's
        // global registry. Covers the http_module-present branch of
        // cleanup_worker's external_functions loop.
        ensure_default_meter();
        let engine = Arc::new(Engine::new());

        let http_functions_config = HttpFunctionsConfig {
            security: SecurityConfig {
                require_https: false,
                block_private_ips: false,
                url_allowlist: vec!["*".to_string()],
            },
        };
        let http_functions_module = HttpFunctionsWorker::create(
            engine.clone(),
            Some(serde_json::to_value(&http_functions_config).expect("serialize config")),
        )
        .await
        .expect("create module");
        http_functions_module
            .initialize()
            .await
            .expect("initialize module");

        let make_msg = || Message::RegisterFunction {
            id: "external.shared".to_string(),
            description: Some("shared external".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: Some(HttpInvocationRef {
                url: "http://example.com/shared".to_string(),
                method: crate::invocation::method::HttpMethod::Post,
                timeout_ms: Some(30000),
                headers: HashMap::new(),
                auth: None,
            }),
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };

        let (tx_old, _rx_old) = mpsc::channel::<Outbound>(8);
        let old_worker = WorkerConnection::new(tx_old);
        engine.worker_registry.register_worker(old_worker.clone());
        engine
            .router_msg(&old_worker, &make_msg())
            .await
            .expect("old worker register should succeed");
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "external.shared")
                .is_some()
        );
        let http_module = engine
            .service_registry
            .get_service::<HttpFunctionsWorker>("http_functions")
            .expect("http_functions service registered");
        assert!(
            http_module
                .http_functions()
                .contains_key(&(DEFAULT_NAMESPACE.to_string(), "external.shared".to_string()))
        );

        // As in the regular-function variant: the old connection is gone from
        // the registry but its cleanup has not fired yet, which is what makes
        // the new worker's claim a takeover rather than a conflict.
        engine.worker_registry.unregister_worker(&old_worker.id);

        let (tx_new, _rx_new) = mpsc::channel::<Outbound>(8);
        let new_worker = WorkerConnection::new(tx_new);
        engine.worker_registry.register_worker(new_worker.clone());
        engine
            .router_msg(&new_worker, &make_msg())
            .await
            .expect("new worker register should succeed");

        engine.cleanup_worker(&old_worker).await;

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "external.shared")
                .is_some(),
            "cleanup of old worker must not remove an external function still owned by a live new worker"
        );
        assert!(
            http_module
                .http_functions()
                .contains_key(&(DEFAULT_NAMESPACE.to_string(), "external.shared".to_string())),
            "http_functions module must still have the entry the new worker owns"
        );
        assert!(!engine.worker_registry.workers.contains_key(&old_worker.id));
        assert!(engine.worker_registry.workers.contains_key(&new_worker.id));
    }

    #[tokio::test]
    async fn test_cleanup_worker_preserves_external_function_without_http_module() {
        // Fallback branch of cleanup_worker's external_functions loop: when
        // the http_functions service is not registered, the cleanup falls
        // through to remove_function_from_engine. Verify the ownership
        // check still skips removal in that path.
        ensure_default_meter();
        let engine = Engine::new();

        // Two workers, both manually placed in the registry with an external
        // function id present. We set up state directly because RegisterFunction
        // with `invocation: Some(_)` requires the http_functions service.
        let (tx_old, _rx_old) = mpsc::channel::<Outbound>(8);
        let old_worker = WorkerConnection::new(tx_old);
        old_worker
            .include_external_function_id("external.shared.no_http")
            .await;
        engine.worker_registry.register_worker(old_worker.clone());

        let (tx_new, _rx_new) = mpsc::channel::<Outbound>(8);
        let new_worker = WorkerConnection::new(tx_new);
        new_worker
            .include_external_function_id("external.shared.no_http")
            .await;
        engine.worker_registry.register_worker(new_worker.clone());

        // Seed the global functions registry as if the new worker had
        // registered it (the http_functions branch is what would normally
        // populate this; the fallback path's job is to avoid clobbering it).
        engine.service_registry.register_service_from_function_id(
            crate::protocol::DEFAULT_NAMESPACE,
            "external.shared.no_http",
        );

        engine.cleanup_worker(&old_worker).await;

        // service_registry entries persist across cleanup of old; new owns it.
        // The key assertion: cleanup did NOT call remove_function_from_engine
        // for this id. We can't directly observe the call site, but we can
        // check that cleanup proceeded (old removed) and the new worker's
        // external_function_ids set is intact.
        assert!(!engine.worker_registry.workers.contains_key(&old_worker.id));
        let live_new = engine
            .worker_registry
            .get_worker(&new_worker.id)
            .expect("new worker still registered");
        assert!(
            live_new
                .has_external_function_id("external.shared.no_http")
                .await,
            "new worker's external function id must survive old worker cleanup"
        );
    }

    #[tokio::test]
    async fn test_remove_worker_registrations_skips_ws_owned_ids() {
        // The in-process reload path calls `remove_worker_registrations`
        // with a set of function_ids captured during a scope. If a WS
        // worker is currently the owner of one of those ids (regular or
        // HTTP-invocation — now one `function_owners` map), the removal
        // must be skipped so the live WS registration survives.
        ensure_default_meter();
        let engine = Engine::new();

        // WS worker owns `ws_fn` via the non-invocation path.
        let (tx_ws, _rx_ws) = mpsc::channel::<Outbound>(8);
        let ws_worker = WorkerConnection::new(tx_ws);
        engine.worker_registry.register_worker(ws_worker.clone());
        engine
            .router_msg(
                &ws_worker,
                &Message::RegisterFunction {
                    id: "ws_fn".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("WS worker register should succeed");
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "ws_fn")
                .is_some()
        );
        assert!(engine.function_owners.contains_key(&(
            crate::protocol::DEFAULT_NAMESPACE.to_string(),
            "ws_fn".to_string()
        )));

        // Also seed an External-owned id directly — we don't run the
        // HTTP registration path here (it requires the http_functions
        // service), so we populate `function_owners` with an External
        // entry by hand to exercise the skip branch for that kind.
        engine.function_owners.insert(
            (
                crate::protocol::DEFAULT_NAMESPACE.to_string(),
                "ext_fn".to_string(),
            ),
            (ws_worker.id, super::InvocationKind::External),
        );

        // Simulate an in-process worker teardown whose scope captured
        // both ids. Before the ownership check was added, this loop
        // would unconditionally call `remove_function_from_engine`
        // and wipe the WS worker's live registrations.
        let regs = crate::workers::reload::WorkerRegistrations {
            function_ids: vec!["ws_fn".to_string(), "ext_fn".to_string()],
        };
        engine.remove_worker_registrations(&regs);

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "ws_fn")
                .is_some(),
            "WS-owned non-invocation function must survive in-process teardown"
        );
        assert!(
            engine.function_owners.contains_key(&(
                crate::protocol::DEFAULT_NAMESPACE.to_string(),
                "ws_fn".to_string()
            )),
            "ownership entry for the WS worker must be intact"
        );
        assert!(
            engine.function_owners.contains_key(&(
                crate::protocol::DEFAULT_NAMESPACE.to_string(),
                "ext_fn".to_string()
            )),
            "external ownership entry for the WS worker must be intact"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_concurrent_register_and_cleanup_preserves_function() {
        // Regression guard for the fast-restart race at its actual
        // interleaving. The sequential tests above verify post-conditions
        // but never exercise the TOCTOU window that `DashMap::remove_if`
        // and the claim-before-register ordering were meant to close.
        // This test runs register-on-new-worker concurrently with
        // cleanup-of-old-worker across many iterations on fresh Engines,
        // asserting the new worker's registration survives every time.
        //
        // If someone reverted `remove_if` back to `contains_key`+`remove`,
        // or moved `claim_function` back below `include_function_id.await`,
        // this test would start failing intermittently under load.
        ensure_default_meter();

        for _ in 0..50 {
            let engine = Arc::new(Engine::new());

            let (tx_old, _rx_old) = mpsc::channel::<Outbound>(8);
            let old_worker = WorkerConnection::new(tx_old);
            engine.worker_registry.register_worker(old_worker.clone());
            engine
                .router_msg(
                    &old_worker,
                    &Message::RegisterFunction {
                        id: "raced_fn".to_string(),
                        description: None,
                        request_format: None,
                        response_format: None,
                        metadata: None,
                        invocation: None,
                        args_format: None,
                        private: false,
                        observable: true,
                        observe: None,
                    },
                )
                .await
                .expect("old worker register should succeed");

            // The old connection has dropped out of the registry — that is
            // what makes its lease claimable at all — but its `cleanup_worker`
            // has not run yet. That gap is the TOCTOU window under test.
            engine.worker_registry.unregister_worker(&old_worker.id);

            let (tx_new, _rx_new) = mpsc::channel::<Outbound>(8);
            let new_worker = WorkerConnection::new(tx_new);
            engine.worker_registry.register_worker(new_worker.clone());

            let engine_for_register = engine.clone();
            let new_worker_for_register = new_worker.clone();
            let register_handle = tokio::spawn(async move {
                engine_for_register
                    .router_msg(
                        &new_worker_for_register,
                        &Message::RegisterFunction {
                            id: "raced_fn".to_string(),
                            description: None,
                            request_format: None,
                            response_format: None,
                            metadata: None,
                            invocation: None,
                            args_format: None,
                            private: false,
                            observable: true,
                            observe: None,
                        },
                    )
                    .await
                    .expect("new worker register should succeed");
            });

            let engine_for_cleanup = engine.clone();
            let old_worker_for_cleanup = old_worker.clone();
            let cleanup_handle = tokio::spawn(async move {
                engine_for_cleanup
                    .cleanup_worker(&old_worker_for_cleanup)
                    .await;
            });

            register_handle.await.expect("register task");
            cleanup_handle.await.expect("cleanup task");

            assert!(
                engine
                    .functions
                    .get(crate::protocol::DEFAULT_NAMESPACE, "raced_fn")
                    .is_some(),
                "new worker's registration must survive concurrent cleanup of old worker"
            );
            let owner = engine
                .function_owners
                .get(&(
                    crate::protocol::DEFAULT_NAMESPACE.to_string(),
                    "raced_fn".to_string(),
                ))
                .expect("function_owners must still have raced_fn");
            assert_eq!(
                owner.0, new_worker.id,
                "the new worker should be the recorded owner after the race"
            );
        }
    }

    #[tokio::test]
    async fn test_reattach_evicts_old_worker_and_cleanup_is_idempotent() {
        // Reconnect kill loop fix (iii-hq/iii#1975): a reconnecting worker
        // presents its previous connection's id + reattach token via
        // `Message::Reattach`. With no reader attached, the engine must
        // retire the old connection directly (so the replay lands on a clean
        // slate) and signal eviction; a later exit-path `cleanup_worker`
        // must then be a completed no-op.
        ensure_default_meter();
        let engine = Engine::new();

        // OLD connection registers fn1.
        let (tx_old, _rx_old) = mpsc::channel::<Outbound>(8);
        let old_worker = WorkerConnection::new(tx_old);
        engine.worker_registry.register_worker(old_worker.clone());
        engine
            .router_msg(
                &old_worker,
                &Message::RegisterFunction {
                    id: "fn1".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("old register");
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "fn1")
                .is_some()
        );

        // NEW connection reattaches, presenting the old worker's id + token.
        let (tx_new, _rx_new) = mpsc::channel::<Outbound>(8);
        let new_worker = WorkerConnection::new(tx_new);
        engine.worker_registry.register_worker(new_worker.clone());
        engine
            .router_msg(
                &new_worker,
                &Message::Reattach {
                    previous_worker_id: old_worker.id.to_string(),
                    reattach_token: Some(old_worker.reattach_token.to_string()),
                },
            )
            .await
            .expect("reattach");

        // Old connection retired: gone from the registry, its fn released,
        // and its read loop woken to exit.
        assert!(!engine.worker_registry.workers.contains_key(&old_worker.id));
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "fn1")
                .is_none(),
            "old connection's function released on reattach"
        );
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            old_worker.evict.notified(),
        )
        .await
        .expect("evicted old worker's read loop must be signalled to exit");

        // NEW connection replays fn1 (owned by new now).
        engine
            .router_msg(
                &new_worker,
                &Message::RegisterFunction {
                    id: "fn1".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("new register");
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "fn1")
                .is_some()
        );
        assert_eq!(
            engine
                .function_owners
                .get(&(
                    crate::protocol::DEFAULT_NAMESPACE.to_string(),
                    "fn1".to_string()
                ))
                .unwrap()
                .0,
            new_worker.id
        );

        // The evicted old socket task now runs its own exit-path cleanup. It
        // must NOT tear down fn1 (now owned by the new connection).
        engine.cleanup_worker(&old_worker).await;
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "fn1")
                .is_some(),
            "idempotent cleanup of the evicted old connection must not touch the live function"
        );
        assert_eq!(
            engine
                .function_owners
                .get(&(
                    crate::protocol::DEFAULT_NAMESPACE.to_string(),
                    "fn1".to_string()
                ))
                .unwrap()
                .0,
            new_worker.id
        );
        assert!(engine.worker_registry.workers.contains_key(&new_worker.id));
    }

    #[tokio::test]
    async fn test_reattach_with_reader_awaits_the_readers_own_cleanup() {
        // When the old connection has a live read loop (the production
        // path), Reattach must evict it and WAIT for the reader's own
        // exit-path cleanup to complete before returning — never run
        // teardown concurrently with a still-draining reader.
        ensure_default_meter();
        let engine = Arc::new(Engine::new());

        let (tx_old, _rx_old) = mpsc::channel::<Outbound>(8);
        let old_worker = WorkerConnection::new(tx_old);
        old_worker
            .has_reader
            .store(true, std::sync::atomic::Ordering::SeqCst);
        engine.worker_registry.register_worker(old_worker.clone());
        engine
            .router_msg(
                &old_worker,
                &Message::RegisterFunction {
                    id: "reader_fn".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("old register");

        // Simulated read loop: wait for eviction, then run the exit-path
        // cleanup — exactly what handle_worker does.
        let reader = {
            let engine = engine.clone();
            let old = old_worker.clone();
            tokio::spawn(async move {
                old.evict.notified().await;
                engine.cleanup_worker(&old).await;
            })
        };

        let (tx_new, _rx_new) = mpsc::channel::<Outbound>(8);
        let new_worker = WorkerConnection::new(tx_new);
        engine.worker_registry.register_worker(new_worker.clone());
        engine
            .router_msg(
                &new_worker,
                &Message::Reattach {
                    previous_worker_id: old_worker.id.to_string(),
                    reattach_token: Some(old_worker.reattach_token.to_string()),
                },
            )
            .await
            .expect("reattach");

        // Reattach returned => the reader-driven cleanup COMPLETED: old
        // connection gone, its registration released.
        assert!(!engine.worker_registry.workers.contains_key(&old_worker.id));
        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "reader_fn")
                .is_none()
        );
        assert!(*old_worker.cleanup_done.borrow());
        tokio::time::timeout(std::time::Duration::from_secs(1), reader)
            .await
            .expect("reader task must have exited")
            .expect("reader task must not panic");
    }

    #[tokio::test]
    async fn test_reattach_requires_matching_token() {
        // Worker ids are publicly discoverable (engine::workers::list,
        // engine::on-workers-changed), so an id without the per-connection
        // secret must never evict a live worker.
        ensure_default_meter();
        let engine = Engine::new();

        let (tx_old, _rx_old) = mpsc::channel::<Outbound>(8);
        let old_worker = WorkerConnection::new(tx_old);
        engine.worker_registry.register_worker(old_worker.clone());

        let (tx_new, _rx_new) = mpsc::channel::<Outbound>(8);
        let new_worker = WorkerConnection::new(tx_new);
        engine.worker_registry.register_worker(new_worker.clone());

        // Missing token.
        engine
            .router_msg(
                &new_worker,
                &Message::Reattach {
                    previous_worker_id: old_worker.id.to_string(),
                    reattach_token: None,
                },
            )
            .await
            .expect("reattach without token ok");
        assert!(engine.worker_registry.workers.contains_key(&old_worker.id));

        // Wrong token.
        engine
            .router_msg(
                &new_worker,
                &Message::Reattach {
                    previous_worker_id: old_worker.id.to_string(),
                    reattach_token: Some(uuid::Uuid::new_v4().to_string()),
                },
            )
            .await
            .expect("reattach with wrong token ok");
        assert!(engine.worker_registry.workers.contains_key(&old_worker.id));
        assert!(!*old_worker.cleanup_done.borrow());
    }

    #[tokio::test]
    async fn test_reattach_self_garbage_and_unknown_are_noops() {
        ensure_default_meter();
        let engine = Engine::new();

        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(worker.clone());

        // Reattaching to itself is a no-op (it must not evict its own loop).
        engine
            .router_msg(
                &worker,
                &Message::Reattach {
                    previous_worker_id: worker.id.to_string(),
                    reattach_token: Some(worker.reattach_token.to_string()),
                },
            )
            .await
            .expect("self reattach ok");
        assert!(engine.worker_registry.workers.contains_key(&worker.id));

        // Malformed id: ignored, no error, worker untouched.
        engine
            .router_msg(
                &worker,
                &Message::Reattach {
                    previous_worker_id: "not-a-uuid".to_string(),
                    reattach_token: None,
                },
            )
            .await
            .expect("garbage reattach ok");
        assert!(engine.worker_registry.workers.contains_key(&worker.id));

        // Unknown (never-registered) id: ignored, no error.
        engine
            .router_msg(
                &worker,
                &Message::Reattach {
                    previous_worker_id: uuid::Uuid::new_v4().to_string(),
                    reattach_token: None,
                },
            )
            .await
            .expect("unknown reattach ok");
        assert!(engine.worker_registry.workers.contains_key(&worker.id));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_concurrent_replay_and_cleanup_preserves_reregistered_state() {
        // The reconnect race at its real interleaving, exercising the function
        // guard (F2): an old connection's `cleanup_worker` runs concurrently
        // with the new connection's replayed RegisterFunction. The function
        // must end up owned by the new connection.
        ensure_default_meter();

        for _ in 0..50 {
            let engine = Arc::new(Engine::new());

            let (tx_old, _rx_old) = mpsc::channel::<Outbound>(8);
            let old_worker = WorkerConnection::new(tx_old);
            engine.worker_registry.register_worker(old_worker.clone());
            let reg_fn = Message::RegisterFunction {
                id: "raced_fn".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
                invocation: None,
                args_format: None,
                private: false,
                observable: true,
                observe: None,
            };
            engine
                .router_msg(&old_worker, &reg_fn)
                .await
                .expect("old fn");

            let (tx_new, _rx_new) = mpsc::channel::<Outbound>(8);
            let new_worker = WorkerConnection::new(tx_new);
            engine.worker_registry.register_worker(new_worker.clone());

            let replay = {
                let engine = engine.clone();
                let new_worker = new_worker.clone();
                let old_worker = old_worker.clone();
                let reg_fn = reg_fn.clone();
                tokio::spawn(async move {
                    // The reconnect replay opens with Reattach — the same first
                    // message the SDK sends — so the engine retires the previous
                    // connection and the registrations below take over on a clean
                    // slate (one live worker per namespace still holds otherwise).
                    engine
                        .router_msg(
                            &new_worker,
                            &Message::Reattach {
                                previous_worker_id: old_worker.id.to_string(),
                                reattach_token: Some(old_worker.reattach_token.to_string()),
                            },
                        )
                        .await
                        .expect("reattach");
                    engine
                        .router_msg(&new_worker, &reg_fn)
                        .await
                        .expect("new fn");
                })
            };
            let cleanup = {
                let engine = engine.clone();
                let old_worker = old_worker.clone();
                tokio::spawn(async move { engine.cleanup_worker(&old_worker).await })
            };
            replay.await.expect("replay task");
            cleanup.await.expect("cleanup task");

            assert!(
                engine
                    .functions
                    .get(crate::protocol::DEFAULT_NAMESPACE, "raced_fn")
                    .is_some(),
                "function must survive concurrent cleanup"
            );
            assert_eq!(
                engine
                    .function_owners
                    .get(&(
                        crate::protocol::DEFAULT_NAMESPACE.to_string(),
                        "raced_fn".to_string()
                    ))
                    .unwrap()
                    .0,
                new_worker.id
            );
        }
    }

    #[tokio::test]
    async fn test_unregister_function_regular_skips_when_owner_hijacked() {
        // UnregisterFunction on the non-invocation path: if worker A was the
        // registered owner but worker B has since claimed the id, A's
        // Unregister must NOT wipe B's live registration. Gate the teardown
        // on `release_function_if_owner` — when the CAS fails, skip silently.
        ensure_default_meter();
        let engine = Engine::new();

        let (tx_a, _rx_a) = mpsc::channel::<Outbound>(8);
        let worker_a = WorkerConnection::new(tx_a);
        engine.worker_registry.register_worker(worker_a.clone());
        engine
            .router_msg(
                &worker_a,
                &Message::RegisterFunction {
                    id: "reg_hijacked".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("A register should succeed");

        // A's connection drops — the fast-restart shape. Ownership is only
        // transferable once the incumbent is out of the registry; a live
        // incumbent is a namespace conflict and B would be rejected instead
        // (see `claim_function`).
        engine.worker_registry.unregister_worker(&worker_a.id);

        let (tx_b, _rx_b) = mpsc::channel::<Outbound>(8);
        let worker_b = WorkerConnection::new(tx_b);
        engine.worker_registry.register_worker(worker_b.clone());
        engine
            .router_msg(
                &worker_b,
                &Message::RegisterFunction {
                    id: "reg_hijacked".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("B register should succeed");

        // Ownership has transferred to B even though A still has the id in
        // its local function_ids set (A's set was not cleared by B's register).
        assert_eq!(
            engine
                .function_owners
                .get(&(
                    crate::protocol::DEFAULT_NAMESPACE.to_string(),
                    "reg_hijacked".to_string(),
                ))
                .expect("owner present")
                .0,
            worker_b.id,
            "owner should be B after hijacking register"
        );

        // A sends a stale UnregisterFunction. Before the gate, this would
        // have called `remove_function_from_engine` and wiped B's live
        // registration.
        engine
            .router_msg(
                &worker_a,
                &Message::UnregisterFunction {
                    id: "reg_hijacked".to_string(),
                },
            )
            .await
            .expect("A's stale unregister should not error");

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "reg_hijacked")
                .is_some(),
            "B's live registration must survive A's stale UnregisterFunction"
        );
        assert_eq!(
            engine
                .function_owners
                .get(&(
                    crate::protocol::DEFAULT_NAMESPACE.to_string(),
                    "reg_hijacked".to_string(),
                ))
                .expect("owner still present")
                .0,
            worker_b.id,
            "owner entry must remain B — gate must not release B's ownership"
        );
    }

    #[tokio::test]
    async fn test_unregister_function_external_skips_when_owner_hijacked() {
        // HTTP-invocation variant of the hijacked-Unregister test. Worker A
        // registers an external function, worker B claims it, then A sends
        // UnregisterFunction. The gate on `release_external_function_if_owner`
        // must abort the teardown before `http_module.unregister_http_function`
        // and `service_registry.remove_function_from_services` run — either
        // would otherwise wipe B's live entries.
        ensure_default_meter();
        let engine = Arc::new(Engine::new());

        let http_functions_config = HttpFunctionsConfig {
            security: SecurityConfig {
                require_https: false,
                block_private_ips: false,
                url_allowlist: vec!["*".to_string()],
            },
        };
        let http_functions_module = HttpFunctionsWorker::create(
            engine.clone(),
            Some(serde_json::to_value(&http_functions_config).expect("serialize config")),
        )
        .await
        .expect("create module");
        http_functions_module
            .initialize()
            .await
            .expect("initialize module");

        let make_msg = || Message::RegisterFunction {
            id: "ext_hijacked".to_string(),
            description: Some("hijacked external".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: Some(HttpInvocationRef {
                url: "http://example.com/hijacked".to_string(),
                method: crate::invocation::method::HttpMethod::Post,
                timeout_ms: Some(30000),
                headers: HashMap::new(),
                auth: None,
            }),
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        };

        let (tx_a, _rx_a) = mpsc::channel::<Outbound>(8);
        let worker_a = WorkerConnection::new(tx_a);
        engine.worker_registry.register_worker(worker_a.clone());
        engine
            .router_msg(&worker_a, &make_msg())
            .await
            .expect("A register should succeed");

        // As above: only a departed incumbent's lease can be taken over.
        engine.worker_registry.unregister_worker(&worker_a.id);

        let (tx_b, _rx_b) = mpsc::channel::<Outbound>(8);
        let worker_b = WorkerConnection::new(tx_b);
        engine.worker_registry.register_worker(worker_b.clone());
        engine
            .router_msg(&worker_b, &make_msg())
            .await
            .expect("B register should succeed");

        assert_eq!(
            engine
                .function_owners
                .get(&(
                    crate::protocol::DEFAULT_NAMESPACE.to_string(),
                    "ext_hijacked".to_string(),
                ))
                .expect("external owner present")
                .0,
            worker_b.id,
            "external owner should be B after hijacking register"
        );
        let http_module = engine
            .service_registry
            .get_service::<HttpFunctionsWorker>("http_functions")
            .expect("http_functions service registered");
        assert!(
            http_module
                .http_functions()
                .contains_key(&(DEFAULT_NAMESPACE.to_string(), "ext_hijacked".to_string()))
        );

        // A's stale UnregisterFunction — A still has the id in its local
        // external_function_ids set. Before the gate, the teardown would
        // have wiped http_module + service_registry entries that now belong
        // to B.
        engine
            .router_msg(
                &worker_a,
                &Message::UnregisterFunction {
                    id: "ext_hijacked".to_string(),
                },
            )
            .await
            .expect("A's stale external unregister should not error");

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "ext_hijacked")
                .is_some(),
            "B's live engine.functions entry must survive A's stale UnregisterFunction"
        );
        assert!(
            http_module
                .http_functions()
                .contains_key(&(DEFAULT_NAMESPACE.to_string(), "ext_hijacked".to_string())),
            "B's http_module entry must survive A's stale UnregisterFunction"
        );
        assert_eq!(
            engine
                .function_owners
                .get(&(
                    crate::protocol::DEFAULT_NAMESPACE.to_string(),
                    "ext_hijacked".to_string(),
                ))
                .expect("external owner still present")
                .0,
            worker_b.id,
            "external owner entry must remain B"
        );
    }

    /// A worker restart that changes a function's invocation kind must not let
    /// the OLD registration's release remove the NEW registration. With two
    /// owner maps this leaked: the old Regular entry lived in `function_owners`
    /// while the new External entry lived in `external_function_owners`, so the
    /// old-kind release matched its own stale entry and could tear down state
    /// the new owner had just written. One `(owner, kind)` map plus a
    /// CAS-on-owner release closes it — after takeover the entry names the new
    /// worker, so the old worker's release predicate no longer matches.
    #[tokio::test]
    async fn release_after_restart_changing_kind_does_not_remove_new_registration() {
        ensure_default_meter();
        let engine = Engine::new();

        // A_old registers `svc::f` as a REGULAR function, then its connection
        // begins teardown (aborted namespace resolution) — the fast-restart
        // window where the incumbent is still in the registry but on its way out.
        let (tx_old, _rx_old) = mpsc::channel::<Outbound>(8);
        let a_old = WorkerConnection::new(tx_old);
        engine.worker_registry.register_worker(a_old.clone());
        engine.begin_namespace_resolution(&a_old);
        engine.resolve_connection_namespace(&a_old, "orders").await;
        engine
            .claim_function("orders", a_old.id, "svc::f")
            .expect("A_old claims the regular function");
        engine.abort_namespace_resolution(&a_old);

        // A_new takes over the SAME id as an EXTERNAL function — allowed because
        // A_old's connection is tearing down.
        let (tx_new, _rx_new) = mpsc::channel::<Outbound>(8);
        let a_new = WorkerConnection::new(tx_new);
        engine.worker_registry.register_worker(a_new.clone());
        engine
            .claim_external_function("orders", a_new.id, "svc::f")
            .expect("A_new takes over as an external function");
        assert_eq!(
            engine
                .function_owners
                .get(&("orders".to_string(), "svc::f".to_string()))
                .expect("owner present")
                .0,
            a_new.id,
            "precondition: A_new is the recorded owner after takeover"
        );

        // A_old's late (regular) release must NOT match — the entry names A_new.
        assert!(
            !engine.release_function_if_owner(&a_old.id, "orders", "svc::f"),
            "A_old's stale release must not fire — it no longer owns the id"
        );
        // The external release path (had A_old been external) is the same CAS.
        assert!(
            !engine.release_external_function_if_owner(&a_old.id, "orders", "svc::f"),
            "A_old's stale external release must not fire either"
        );

        // A_new's registration is intact and still owned by A_new.
        assert_eq!(
            engine
                .function_owners
                .get(&("orders".to_string(), "svc::f".to_string()))
                .expect("A_new's ownership entry must survive A_old's stale release")
                .0,
            a_new.id,
            "owner must remain A_new — the old-kind release must not remove it"
        );
    }

    #[tokio::test]
    async fn test_cleanup_worker_with_pending_invocations() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        engine.worker_registry.register_worker(worker.clone());

        // Add some invocations to the worker
        let inv1 = uuid::Uuid::new_v4();
        let inv2 = uuid::Uuid::new_v4();
        worker.add_invocation(inv1).await;
        worker.add_invocation(inv2).await;

        // Cleanup the worker
        engine.cleanup_worker(&worker).await;

        // Worker should be unregistered
        assert!(!engine.worker_registry.workers.contains_key(&worker.id));
    }

    #[tokio::test]
    async fn test_cleanup_worker_clears_worker_invocation_state() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        engine.worker_registry.register_worker(worker.clone());

        let inv1 = uuid::Uuid::new_v4();
        let inv2 = uuid::Uuid::new_v4();
        worker.add_invocation(inv1).await;
        worker.add_invocation(inv2).await;

        engine.cleanup_worker(&worker).await;

        // Cleanup releases worker-local tracking as well as global waiters.
        assert_eq!(worker.invocation_count().await, 0);
    }

    // =========================================================================
    // Engine state tests
    // =========================================================================

    #[test]
    fn test_engine_new_defaults() {
        ensure_default_meter();
        let engine = Engine::new();

        assert!(
            engine
                .functions
                .get(crate::protocol::DEFAULT_NAMESPACE, "nonexistent")
                .is_none()
        );
        assert!(
            !engine
                .worker_registry
                .workers
                .contains_key(&uuid::Uuid::new_v4())
        );
    }

    // =========================================================================
    // handle_telemetry_frame: MTRC with invalid UTF-8
    // =========================================================================

    #[tokio::test]
    async fn test_handle_telemetry_frame_mtrc_invalid_utf8() {
        ensure_default_meter();
        let peer: std::net::SocketAddr = "127.0.0.1:9999".parse().unwrap();

        let mut frame = Vec::from(b"MTRC" as &[u8]);
        frame.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x80]);

        let handled = super::handle_telemetry_frame(&frame, &peer).await;
        assert!(
            handled,
            "MTRC frame with invalid UTF-8 should still be handled"
        );
    }

    #[tokio::test]
    async fn test_handle_telemetry_frame_logs_invalid_utf8() {
        ensure_default_meter();
        let peer: std::net::SocketAddr = "127.0.0.1:9999".parse().unwrap();

        let mut frame = Vec::from(b"LOGS" as &[u8]);
        frame.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x80]);

        let handled = super::handle_telemetry_frame(&frame, &peer).await;
        assert!(
            handled,
            "LOGS frame with invalid UTF-8 should still be handled"
        );
    }

    #[tokio::test]
    async fn test_handle_telemetry_frame_empty_payload() {
        ensure_default_meter();
        let peer: std::net::SocketAddr = "127.0.0.1:9999".parse().unwrap();

        // Prefix only, no payload - should still be handled but might fail ingestion
        let frame = b"OTLP";
        let handled = super::handle_telemetry_frame(frame, &peer).await;
        assert!(
            handled,
            "OTLP prefix with empty payload should still be handled"
        );
    }

    // ── namespace registration buffering ────────────────────────────────

    fn register_function_msg(id: &str) -> Message {
        Message::RegisterFunction {
            id: id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            args_format: None,
            private: false,
            observable: true,
            observe: None,
        }
    }

    /// A worker that declares `namespace: "orders"` sends its
    /// `RegisterFunction` messages before `engine::workers::register` on every
    /// SDK that predates the reorder. The registration must wait for the
    /// namespace rather than land in `default`.
    #[tokio::test]
    async fn registrations_before_the_namespace_is_known_land_in_the_resolved_namespace() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.begin_namespace_resolution(&worker);

        engine
            .router_msg(&worker, &register_function_msg("orders::create"))
            .await
            .expect("buffer register");

        // Nothing is registered yet: the namespace is still unknown.
        assert!(
            engine
                .functions
                .get(DEFAULT_NAMESPACE, "orders::create")
                .is_none()
        );
        assert!(engine.functions.get("orders", "orders::create").is_none());

        engine.resolve_connection_namespace(&worker, "orders").await;

        assert!(engine.functions.get("orders", "orders::create").is_some());
        assert!(
            engine
                .functions
                .get(DEFAULT_NAMESPACE, "orders::create")
                .is_none()
        );
        assert!(engine.service_registry.get("orders", "orders").is_some());
        assert!(
            engine
                .service_registry
                .get(DEFAULT_NAMESPACE, "orders")
                .is_none()
        );
    }

    /// The buffer is a queue, not a set: replaying it out of order would
    /// resurrect a function the worker already unregistered.
    #[tokio::test]
    async fn buffered_registrations_drain_in_arrival_order() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.begin_namespace_resolution(&worker);

        for msg in [
            register_function_msg("orders::create"),
            register_function_msg("orders::cancel"),
            Message::UnregisterFunction {
                id: "orders::create".to_string(),
            },
        ] {
            engine.router_msg(&worker, &msg).await.expect("buffer");
        }

        engine.resolve_connection_namespace(&worker, "orders").await;

        // `create` was registered and then unregistered; inverting the drain
        // would leave it alive.
        assert!(engine.functions.get("orders", "orders::create").is_none());
        assert!(engine.functions.get("orders", "orders::cancel").is_some());
    }

    /// Safety net for a hand-rolled client that never sends
    /// `engine::workers::register`: the buffer must not hold registrations
    /// forever.
    #[tokio::test(start_paused = true)]
    async fn registrations_fall_back_to_default_when_the_grace_expires() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.begin_namespace_resolution(&worker);

        engine
            .router_msg(&worker, &register_function_msg("legacy::ping"))
            .await
            .expect("buffer register");

        // Paused time auto-advances only once no task is runnable, so this
        // sleep resumes strictly after the (earlier-deadline) grace timer has
        // fired and its drain has run to completion. No wall-clock time passes.
        tokio::time::sleep(REGISTRATION_NAMESPACE_GRACE + Duration::from_secs(1)).await;

        assert!(
            engine
                .functions
                .get(DEFAULT_NAMESPACE, "legacy::ping")
                .is_some()
        );
    }

    /// Whichever resolver fires first wins; the loser must be a no-op rather
    /// than a second drain.
    #[tokio::test]
    async fn a_second_resolution_neither_re_drains_nor_moves_the_namespace() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.begin_namespace_resolution(&worker);

        engine
            .router_msg(&worker, &register_function_msg("orders::create"))
            .await
            .expect("buffer register");

        engine.resolve_connection_namespace(&worker, "orders").await;
        // The grace timer firing late must not move the function to `default`
        // nor register it twice.
        engine
            .resolve_connection_namespace(&worker, DEFAULT_NAMESPACE)
            .await;

        assert!(engine.functions.get("orders", "orders::create").is_some());
        assert!(
            engine
                .functions
                .get(DEFAULT_NAMESPACE, "orders::create")
                .is_none()
        );
    }

    /// Once resolved, a connection stays resolved: later registrations take
    /// the direct path with no buffering.
    #[tokio::test]
    async fn registrations_after_resolution_are_not_buffered() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.begin_namespace_resolution(&worker);

        engine.resolve_connection_namespace(&worker, "orders").await;

        engine
            .router_msg(&worker, &register_function_msg("orders::create"))
            .await
            .expect("register");

        assert!(engine.functions.get("orders", "orders::create").is_some());
    }

    /// Connections that never enter the state machine (in-process workers,
    /// direct `router_msg` callers) must keep registering immediately.
    #[tokio::test]
    async fn registrations_on_an_untracked_connection_are_not_buffered() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        engine
            .router_msg(&worker, &register_function_msg("plain::ping"))
            .await
            .expect("register");

        assert!(
            engine
                .functions
                .get(DEFAULT_NAMESPACE, "plain::ping")
                .is_some()
        );
    }

    /// `cleanup_worker` must release from the namespace the drain registered
    /// into, not from `default`.
    #[tokio::test]
    async fn cleanup_releases_functions_from_the_resolved_namespace() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.begin_namespace_resolution(&worker);

        engine
            .router_msg(&worker, &register_function_msg("orders::create"))
            .await
            .expect("buffer register");
        engine.resolve_connection_namespace(&worker, "orders").await;
        assert!(engine.functions.get("orders", "orders::create").is_some());

        engine.cleanup_worker(&worker).await;

        assert!(engine.functions.get("orders", "orders::create").is_none());
    }

    /// A worker can disconnect while its drain is still running (it dropped
    /// right after sending `engine::workers::register`). `cleanup_worker`
    /// snapshots the function ids it will release up front, so a drain that
    /// keeps going registers functions nobody will ever release: they leak with
    /// `function_owners` pointing at a dead worker. Cleanup aborts the
    /// resolution first to stop that.
    #[tokio::test]
    async fn aborting_a_connection_stops_its_drain_from_registering() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.begin_namespace_resolution(&worker);

        engine
            .router_msg(&worker, &register_function_msg("orders::create"))
            .await
            .expect("buffer register");

        // What `cleanup_worker` does before it snapshots anything.
        engine.abort_namespace_resolution(&worker);

        // The in-flight `engine::workers::register` lands after teardown began.
        engine.resolve_connection_namespace(&worker, "orders").await;

        assert!(
            engine.functions.get("orders", "orders::create").is_none(),
            "a drain must not register into a connection being torn down"
        );
        assert!(
            engine
                .functions
                .get(DEFAULT_NAMESPACE, "orders::create")
                .is_none()
        );
    }

    /// `aborting_a_connection_stops_its_drain_from_registering` proves the
    /// mechanism; this proves `cleanup_worker` actually *uses* it, and uses it
    /// early enough. Deleting `abort_namespace_resolution` from `cleanup_worker`
    /// must fail a test — otherwise the call site is a no-op nobody notices.
    ///
    /// The naive "cleanup, then resolve" version passes either way, because
    /// cleanup removes the state entry on its way out and an absent entry stops
    /// the drain regardless. So instead this pins cleanup mid-flight, at the
    /// only point where the ordering is observable.
    #[tokio::test]
    async fn cleanup_aborts_the_resolution_before_it_snapshots_functions() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.begin_namespace_resolution(&worker);

        // Deliberately a service registration, not a function one: the drain's
        // `RegisterService` arm writes only to `service_registry`, so it cannot
        // contend for the `function_ids` guard held below. `RegisterFunction`
        // would block in `include_function_id` and hang the test instead of
        // failing it cleanly.
        engine
            .router_msg(
                &worker,
                &Message::RegisterService {
                    id: "svc".to_string(),
                    name: "svc".to_string(),
                    description: None,
                    parent_service_id: None,
                },
            )
            .await
            .expect("buffer register service");

        // Pin `cleanup_worker` at its first await: `get_regular_function_ids`
        // takes a read guard on `function_ids`, so holding the write guard
        // parks the task AFTER `abort_namespace_resolution` and BEFORE the
        // snapshot that decides what to release — exactly the interleaving the
        // abort exists to establish.
        let snapshot_block = worker.function_ids.write().await;

        let cleanup = {
            let engine = engine.clone();
            let worker = worker.clone();
            tokio::spawn(async move { engine.cleanup_worker(&worker).await })
        };

        // Current-thread runtime: yielding hands control to the spawned task,
        // which runs to its first pending await and parks on the guard above.
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }

        // The worker's in-flight `engine::workers::register` lands mid-teardown.
        engine.resolve_connection_namespace(&worker, "orders").await;

        assert!(
            engine.service_registry.get("orders", "svc").is_none(),
            "cleanup must abort the resolution before it snapshots, so a drain \
             racing teardown registers nothing it will not release"
        );

        drop(snapshot_block);
        cleanup.await.expect("cleanup should complete");
    }

    /// The abort must not cost `cleanup_worker` the namespace it needs to
    /// release from — `Aborted` carries it.
    #[tokio::test]
    async fn aborting_preserves_the_resolved_namespace_for_teardown() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.begin_namespace_resolution(&worker);
        engine.resolve_connection_namespace(&worker, "orders").await;

        engine.abort_namespace_resolution(&worker);

        assert_eq!(engine.connection_namespace(&worker), "orders");
    }

    // ── observe messages before the observer registry ─────────────────────

    /// Every observe message a worker can send decodes and is accepted
    /// without side effects or replies, and engine→worker variants echoed
    /// back by a worker are ignored like `WorkerRegistered`.
    #[tokio::test]
    async fn observe_messages_from_a_worker_are_accepted_without_replies() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let frames = [
            r#"{"type":"observe","id":"o-1","source":{"function_id":"orders::archive"},"target":{"function_id":"search::reindex-order"}}"#,
            r#"{"type":"unobserve","id":"o-1"}"#,
            r#"{"type":"observerack","function_id":"cron::on-tick","id":"o-1","revision":1}"#,
            r#"{"type":"invocationsobserved","items":[]}"#,
            r#"{"type":"localinvocationstats","window_ms":10000,"functions":[],"dropped_reports":[]}"#,
            r#"{"type":"observeresult","id":"o-1","state":"active"}"#,
            r#"{"type":"observerstatechanged","id":"o-1","state":"active","fires":0}"#,
            r#"{"type":"functionregistered","id":"f","namespace":"default","revision":0,"local_dispatch":false,"observers":[]}"#,
            r#"{"type":"observerremoved","function_id":"f","namespace":"default","revision":1,"id":"o-1"}"#,
        ];
        for frame in frames {
            let msg: Message = serde_json::from_str(frame).expect(frame);
            engine.router_msg(&worker, &msg).await.expect(frame);
        }

        assert!(rx.try_recv().is_err(), "no replies before observers exist");
        assert!(engine.functions.iter().next().is_none());
    }

    /// `Observe` / `Unobserve` wait behind namespace resolution with the
    /// registrations, in arrival order: an observe and its unobserve sent
    /// before `engine::workers::register` still end disposed, not alive.
    #[tokio::test]
    async fn observe_and_unobserve_wait_for_the_namespace_in_order() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(16);
        let worker = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(worker.clone());
        engine.begin_namespace_resolution(&worker);

        let observe: Message = serde_json::from_value(serde_json::json!({
            "type": "observe",
            "id": "o-buffered",
            "source": { "function_id": "orders::create" },
            "target": { "function_id": "sink::orders" }
        }))
        .unwrap();
        engine
            .router_msg(&worker, &observe)
            .await
            .expect("buffered");
        engine
            .router_msg(
                &worker,
                &Message::Unobserve {
                    id: "o-buffered".to_string(),
                },
            )
            .await
            .expect("buffered");
        assert!(
            engine.observers.is_empty(),
            "nothing applies before the namespace"
        );

        engine.resolve_connection_namespace(&worker, "orders").await;
        assert!(
            engine.observers.get("o-buffered").is_none(),
            "created, then disposed"
        );

        let mut frames = Vec::new();
        while frames.len() < 2 {
            match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
                Ok(Some(Outbound::Protocol(msg))) => frames.push(msg),
                Ok(Some(_)) => {}
                other => panic!("expected two frames, got {frames:?} then {other:?}"),
            }
        }
        assert!(
            matches!(&frames[0], Message::ObserveResult { id, .. } if id == "o-buffered"),
            "{frames:?}"
        );
        assert!(
            matches!(
                &frames[1],
                Message::ObserverStateChanged { id, state, .. }
                    if id == "o-buffered" && *state == crate::protocol::ObserverState::Disposed
            ),
            "{frames:?}"
        );
        engine.cancel_namespace_grace_timer(&worker.id);
    }

    fn session_with_hooks(
        function_hook: Option<&str>,
        observer_hook: Option<&str>,
    ) -> crate::workers::worker::rbac_session::Session {
        use crate::workers::worker::{
            WorkerManagerConfig, rbac_config::RbacConfig, rbac_session::Session,
        };
        Session {
            engine: Arc::new(Engine::new()),
            config: Arc::new(WorkerManagerConfig {
                rbac: Some(RbacConfig {
                    auth_function_id: None,
                    expose_functions: vec![
                        crate::workers::worker::rbac_config::FunctionFilter::match_pattern("*"),
                    ],
                    on_function_registration_function_id: function_hook.map(str::to_string),
                    on_observer_registration_function_id: observer_hook.map(str::to_string),
                }),
                ..WorkerManagerConfig::default()
            }),
            ip_address: "127.0.0.1".to_string(),
            session_id: uuid::Uuid::new_v4(),
            namespaces: Default::default(),
            allowed_functions: vec![],
            forbidden_functions: vec![],
            allow_function_registration: true,
            context: serde_json::json!({ "tenant": "acme" }),
            function_registration_prefix: None,
        }
    }

    fn register_hook(engine: &Engine, id: &str, answer: serde_json::Value) {
        engine.register_function_handler(
            super::RegisterFunctionRequest {
                function_id: id.to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            super::Handler::new(move |_input| {
                let answer = answer.clone();
                async move { FunctionResult::Success(Some(answer)) }
            }),
        );
    }

    async fn next_protocol_frame(rx: &mut mpsc::Receiver<Outbound>) -> Message {
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
                Ok(Some(Outbound::Protocol(msg))) => return msg,
                Ok(Some(_)) => continue,
                other => panic!("no frame: {other:?}"),
            }
        }
    }

    /// `on_observer_registration_function_id`: an object answer rewrites the
    /// observer, anything else denies it.
    #[tokio::test]
    async fn the_observer_registration_hook_rewrites_or_denies() {
        ensure_default_meter();
        let engine = Engine::new();
        register_hook(
            &engine,
            "rbac::observer-hook",
            serde_json::json!({ "target_function_id": "acme::audit" }),
        );
        register_hook(
            &engine,
            "rbac::observer-deny",
            serde_json::Value::Bool(false),
        );

        let observe: Message = serde_json::from_value(serde_json::json!({
            "type": "observe",
            "id": "o-hooked",
            "source": { "function_id": "orders::create" },
            "target": { "function_id": "audit::write" }
        }))
        .unwrap();

        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::with_session(
            tx,
            session_with_hooks(None, Some("rbac::observer-hook")),
        );
        engine.worker_registry.register_worker(worker.clone());
        engine.router_msg(&worker, &observe).await.expect("handled");
        let reply = next_protocol_frame(&mut rx).await;
        assert!(
            matches!(&reply, Message::ObserveResult { error: None, .. }),
            "{reply:?}"
        );
        let observer = engine.observers.get("o-hooked").expect("created");
        assert_eq!(observer.target.1, "acme::audit", "rewritten by the hook");

        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let denied = WorkerConnection::with_session(
            tx,
            session_with_hooks(None, Some("rbac::observer-deny")),
        );
        engine.worker_registry.register_worker(denied.clone());
        let mut other = observe.clone();
        if let Message::Observe { id, .. } = &mut other {
            *id = "o-denied".to_string();
        }
        engine.router_msg(&denied, &other).await.expect("handled");
        match next_protocol_frame(&mut rx).await {
            Message::ObserveResult {
                error: Some(error), ..
            } => assert_eq!(error.code, "FORBIDDEN"),
            other => panic!("expected a FORBIDDEN observeresult, got {other:?}"),
        }
        assert!(engine.observers.get("o-denied").is_none());
    }

    #[tokio::test]
    async fn oversized_invoke_completes_caller_and_cleans_caller_and_target_state() {
        ensure_default_meter();
        let engine = Engine::new();
        let (caller_tx, _caller_rx) = mpsc::channel::<Outbound>(8);
        let caller = WorkerConnection::new(caller_tx);
        engine.worker_registry.register_worker(caller.clone());
        let (target_tx, mut target_rx) = mpsc::channel::<Outbound>(8);
        let target = WorkerConnection::new(target_tx);
        let invocation_id = uuid::Uuid::new_v4();
        caller.add_invocation(invocation_id).await;
        target.add_invocation(invocation_id).await;
        let handler: std::sync::Arc<crate::function::HandlerFn> =
            std::sync::Arc::new(|_, _, _, _| Box::pin(async { FunctionResult::Deferred }));
        let invocation_task = {
            let engine = engine.clone();
            let caller = caller.clone();
            tokio::spawn(async move {
                engine
                    .invocations
                    .handle_invocation_with_handler(
                        Some(invocation_id),
                        Some(&caller),
                        "caller::fn".into(),
                        "default",
                        serde_json::json!({}),
                        handler,
                        None,
                        None,
                        None,
                        None,
                        crate::observer::InvocationCtx::engine_internal_at(0),
                    )
                    .await
            })
        };
        tokio::task::yield_now().await;
        assert_eq!(caller.invocation_count().await, 1);
        caller.receive_message_limit.store(1024, Ordering::Relaxed);
        target.receive_message_limit.store(1024, Ordering::Relaxed);
        let invoke = Message::InvokeFunction {
            invocation_id: Some(invocation_id),
            function_id: "target::fn".into(),
            data: serde_json::json!({ "payload": "x".repeat(4096) }),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
            observe_depth: None,
            caller: None,
            activation: None,
        };

        assert!(
            engine
                .guard_outbound(
                    &target,
                    invoke.clone(),
                    serde_json::to_string(&invoke).unwrap()
                )
                .await
                .is_none()
        );
        let caller_result = tokio::time::timeout(Duration::from_millis(100), invocation_task)
            .await
            .expect("caller completes promptly")
            .expect("invocation task");
        assert_eq!(
            caller_result.unwrap().unwrap_err().code,
            "payload_too_large"
        );
        assert_eq!(caller.invocation_count().await, 0);
        assert_eq!(target.invocation_count().await, 0);
        assert!(
            target_rx.try_recv().is_err(),
            "oversized invoke is not queued to the target"
        );
        let next = Message::WorkerRegistered {
            worker_id: caller.id.to_string(),
            reattach_token: None,
            features: Vec::new(),
        };
        assert!(
            engine
                .guard_outbound(&caller, next.clone(), serde_json::to_string(&next).unwrap())
                .await
                .is_some()
        );
    }

    /// An observer activation larger than its target worker accepts is not
    /// sent, and the target never sees the event: the observer counts a
    /// `TOO_LARGE` drop, not the target's error. A target that answers
    /// `payload_too_large` itself (its result was too large) still counts as
    /// `TARGET_ERROR`.
    #[tokio::test]
    async fn an_activation_too_large_for_its_target_is_a_too_large_drop() {
        use crate::observer::events::test_support;

        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        engine.register_function_handler(
            make_request("test::src"),
            crate::engine::Handler::new(|input: serde_json::Value| async move {
                FunctionResult::Success(Some(input))
            }),
        );
        let (target_tx, mut target_rx) = mpsc::channel::<Outbound>(8);
        let target = WorkerConnection::new(target_tx);
        engine.worker_registry.register_worker(target.clone());
        engine
            .router_msg(
                &target,
                &Message::RegisterFunction {
                    id: "test::far".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    invocation: None,
                    args_format: None,
                    private: false,
                    observable: true,
                    observe: None,
                },
            )
            .await
            .expect("register the target");
        target.receive_message_limit.store(1024, Ordering::Relaxed);
        let (watcher, _socket) = test_support::worker(&engine);
        test_support::observe_request(
            &engine,
            &watcher,
            test_support::request("obs", "test::src", "test::far", None),
        )
        .await;

        // What the target connection's writer does with the next activation.
        async fn next_activation(
            engine: &Engine,
            target: &WorkerConnection,
            rx: &mut mpsc::Receiver<Outbound>,
        ) -> Option<(Message, String)> {
            loop {
                let outbound = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                    .await
                    .expect("an activation")
                    .expect("open");
                if let Outbound::Protocol(msg @ Message::InvokeFunction { .. }) = outbound {
                    let serialized = serde_json::to_string(&msg).unwrap();
                    return engine.guard_outbound(target, msg, serialized).await;
                }
            }
        }
        let info = |engine: &Engine| engine.observers.get_info(engine, None, "obs").unwrap();
        let settled = |engine: &Engine, drops: u64, failures: u64| {
            let info = info(engine);
            info.dropped.get("TOO_LARGE").copied().unwrap_or(0) == drops
                && info.activation_failures.values().sum::<u64>() == failures
        };

        engine
            .call("test::src", json!({ "big": "x".repeat(4096) }))
            .await
            .unwrap();
        assert!(
            next_activation(&engine, &target, &mut target_rx)
                .await
                .is_none(),
            "not sent"
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while !settled(&engine, 1, 0) {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("a TOO_LARGE drop, not a failure");

        engine
            .call("test::src", json!({ "small": 1 }))
            .await
            .unwrap();
        let Some((Message::InvokeFunction { invocation_id, .. }, _)) =
            next_activation(&engine, &target, &mut target_rx).await
        else {
            panic!("a small activation is sent");
        };
        engine
            .router_msg(
                &target,
                &Message::InvocationResult {
                    invocation_id: invocation_id.expect("an activation carries an id"),
                    function_id: "test::far".to_string(),
                    result: None,
                    error: Some(payload_too_large_error(70_000_000, 67_108_864, false)),
                    traceparent: None,
                    baggage: None,
                    observed: Vec::new(),
                },
            )
            .await
            .expect("the target answers");
        tokio::time::timeout(Duration::from_secs(5), async {
            while !settled(&engine, 1, 1) {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("the target's own error");
        assert_eq!(
            info(&engine).activation_failures.get("TARGET_ERROR"),
            Some(&1)
        );
    }

    #[tokio::test]
    async fn oversized_invocation_result_is_replaced_by_correlated_error() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        worker.receive_message_limit.store(2048, Ordering::Relaxed);
        let invocation_id = uuid::Uuid::new_v4();
        let msg = Message::InvocationResult {
            invocation_id,
            function_id: "target::fn".into(),
            result: Some(serde_json::json!({ "payload": "x".repeat(4096) })),
            error: None,
            traceparent: None,
            baggage: None,
            observed: Vec::new(),
        };

        let (fallback, fallback_payload) = engine
            .guard_outbound(&worker, msg.clone(), serde_json::to_string(&msg).unwrap())
            .await
            .expect("fallback error");
        assert!(fallback_payload.len() <= 2048);
        match fallback {
            Message::InvocationResult {
                invocation_id: actual_id,
                function_id,
                result,
                error,
                ..
            } => {
                assert_eq!(actual_id, invocation_id);
                assert_eq!(function_id, "target::fn");
                assert!(result.is_none());
                assert_eq!(error.expect("error").code, "payload_too_large");
            }
            other => panic!("expected InvocationResult fallback, got {other:?}"),
        }
        assert!(
            rx.try_recv().is_err(),
            "fallback is returned to the single writer"
        );
    }

    #[tokio::test]
    async fn outbound_at_limit_is_unchanged_and_other_oversized_is_dropped() {
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        let message = Message::WorkerRegistered {
            worker_id: worker.id.to_string(),
            reattach_token: None,
            features: Vec::new(),
        };
        let exact_size = serde_json::to_string(&message).unwrap().len();
        worker
            .receive_message_limit
            .store(exact_size, Ordering::Relaxed);
        let (at_limit, serialized) = engine
            .guard_outbound(
                &worker,
                message.clone(),
                serde_json::to_string(&message).unwrap(),
            )
            .await
            .expect("in-limit message is accepted");
        assert!(matches!(at_limit, Message::WorkerRegistered { .. }));
        assert_eq!(serialized.len(), exact_size);

        worker.receive_message_limit.store(1, Ordering::Relaxed);
        assert!(
            engine
                .guard_outbound(
                    &worker,
                    message.clone(),
                    serde_json::to_string(&message).unwrap()
                )
                .await
                .is_none()
        );
    }

    #[test]
    fn oversized_error_text_and_message_type_are_stable() {
        let error = payload_too_large_error(20, 16, true);
        assert_eq!(error.code, "payload_too_large");

        assert_eq!(
            error.message,
            "Serialized JSON envelope is 20 bytes; target worker accepts up to 16 bytes. It did not declare large-messages.v1 (SDK 0.25.0 or later), and the version it reports is older than 0.24.5 or unknown. Upgrade the target worker SDK to 0.25.0 or later, or use channels: https://iii.dev/docs/creating-workers/channels#payload-size"
        );
        assert_eq!(
            protocol_message_type(&Message::WorkerRegistered {
                worker_id: "id".into(),
                reattach_token: None,
                features: Vec::new(),
            }),
            "other"
        );
    }

    #[tokio::test]
    async fn sdk_0245_worker_never_receives_envelopes_over_64_mib() {
        ensure_default_meter();
        let registry = crate::worker_connections::WorkerConnectionRegistry::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(1);
        let worker = WorkerConnection::new(tx);
        let worker_id = worker.id;
        registry.register_worker(worker.clone());
        registry.update_worker_metadata(
            &worker_id,
            "rust".into(),
            Some("0.24.5".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(
            worker.receive_message_limit.load(Ordering::Relaxed),
            crate::workers::worker::MAX_JSON_MESSAGE_BYTES
        );

        let invocation_id = uuid::Uuid::new_v4();
        let message = Message::InvocationResult {
            invocation_id,
            function_id: "target::fn".into(),
            result: Some(
                serde_json::json!({ "payload": "x".repeat(crate::workers::worker::MAX_JSON_MESSAGE_BYTES) }),
            ),
            error: None,
            traceparent: None,
            baggage: None,
            observed: Vec::new(),
        };
        let serialized = serde_json::to_string(&message).expect("serialize oversized result");
        assert!(serialized.len() > crate::workers::worker::MAX_JSON_MESSAGE_BYTES);

        let (fallback, _payload) = Engine::new()
            .guard_outbound(&worker, message, serialized)
            .await
            .expect("correlated error fallback");
        assert!(matches!(
            fallback,
            Message::InvocationResult { invocation_id: actual, result: None, error: Some(error), .. }
                if actual == invocation_id && error.code == "payload_too_large" && error.message.contains("67108864 bytes")
        ));
        assert!(
            rx.try_recv().is_err(),
            "fallback is returned to the sole writer"
        );
    }

    // ── Removed messages and function options (observe v2) ────────────────

    fn fixture_frame(name: &str) -> Message {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../sdk/fixtures/protocol/observe")
            .join(format!("{name}.json"));
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect(name)
    }

    /// Each removed message type is dropped with one warning per connection,
    /// whatever the number of frames, and nothing is answered.
    #[tokio::test]
    async fn removed_frames_are_dropped_with_one_warning_per_type() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        let peer: std::net::SocketAddr = "127.0.0.1:1".parse().unwrap();
        for _ in 0..2 {
            for (message_type, _) in super::REMOVED_MESSAGES {
                let frame = serde_json::json!({
                    "type": message_type,
                    "id": "t-1",
                    "trigger_type": "cron",
                    "function_id": "reports::daily",
                    "config": {}
                })
                .to_string();
                let error = serde_json::from_str::<Message>(&frame).expect_err("not a message");
                engine
                    .undecodable_frame(&worker, peer, frame.as_bytes(), &error)
                    .await;
            }
        }
        assert!(rx.try_recv().is_err(), "dropped, never answered");
        for index in 0..super::REMOVED_MESSAGES.len() {
            assert!(
                !worker.first_removed_message(index),
                "warned once already for type {index}"
            );
        }
        let other = WorkerConnection::new(mpsc::channel::<Outbound>(1).0);
        assert!(other.first_removed_message(0), "per connection");
    }

    /// A 0.25 SDK's `observe: { delivery: owner }` is refused with
    /// `UNSUPPORTED`; only that registration, which leaves nothing behind.
    #[tokio::test]
    async fn an_owner_delivered_registration_is_unsupported() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine
            .router_msg(&worker, &fixture_frame("registerfunction-v1-owner"))
            .await
            .unwrap();
        let Message::RegistrationRejected {
            code,
            namespace,
            function_id,
            owner_worker_id,
            ..
        } = next_protocol_frame(&mut rx).await
        else {
            panic!("a rejection");
        };
        assert_eq!(code, "UNSUPPORTED");
        assert_eq!(namespace, DEFAULT_NAMESPACE);
        assert_eq!(function_id.as_deref(), Some("cron::on-tick"));
        assert!(owner_worker_id.is_empty());
        assert!(
            engine
                .functions
                .get(DEFAULT_NAMESPACE, "cron::on-tick")
                .is_none()
        );
    }

    /// `observe: { delivery: auto, allow: false }` registers the function as
    /// `observable: false`; top-level `observable: false` does the same.
    #[tokio::test]
    async fn allow_false_registers_a_function_that_is_not_observable() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        for fixture in [
            "registerfunction-v1-not-observable",
            "registerfunction-not-observable",
        ] {
            engine
                .router_msg(&worker, &fixture_frame(fixture))
                .await
                .unwrap();
            assert!(
                engine
                    .functions
                    .get(DEFAULT_NAMESPACE, "vault::read-secret")
                    .is_some(),
                "{fixture}"
            );
            assert!(
                !engine
                    .observers
                    .options(DEFAULT_NAMESPACE, "vault::read-secret")
                    .observable,
                "{fixture}"
            );
        }
    }

    /// An `args_format` that is not a JSON Schema refuses the registration
    /// with `INVALID_ARGS_FORMAT`.
    #[tokio::test]
    async fn an_invalid_args_format_refuses_the_registration() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        let msg: Message = serde_json::from_value(serde_json::json!({
            "type": "registerfunction",
            "id": "queue::on-message",
            "args_format": { "type": "not-a-type" }
        }))
        .unwrap();
        engine.router_msg(&worker, &msg).await.unwrap();
        let Message::RegistrationRejected { code, .. } = next_protocol_frame(&mut rx).await else {
            panic!("a rejection");
        };
        assert_eq!(code, "INVALID_ARGS_FORMAT");
        assert!(
            engine
                .functions
                .get(DEFAULT_NAMESPACE, "queue::on-message")
                .is_none()
        );
    }

    /// Top-level `args_format` and `private` are the registration's options:
    /// a private function is called by its host only, and its options go
    /// with the registration.
    #[tokio::test]
    async fn function_options_go_with_the_registration() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(worker.clone());
        engine
            .router_msg(&worker, &fixture_frame("registerfunction-private"))
            .await
            .unwrap();
        let options = engine.observers.options(DEFAULT_NAMESPACE, "cron::on-tick");
        assert!(options.private && options.observable);
        assert!(options.args_format.is_some());
        assert!(
            engine
                .observers
                .route(DEFAULT_NAMESPACE, "cron::on-tick", Some(worker.id))
                .is_ok()
        );
        assert_eq!(
            engine
                .call("cron::on-tick", serde_json::json!({}))
                .await
                .unwrap_err()
                .code,
            "FORBIDDEN",
            "the engine is not the host"
        );

        engine
            .router_msg(
                &worker,
                &Message::UnregisterFunction {
                    id: "cron::on-tick".to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            engine.observers.options(DEFAULT_NAMESPACE, "cron::on-tick"),
            crate::observer::FunctionOptions::default()
        );
    }

    /// `on_function_registration_function_id` gets the registration's
    /// `private`, `observable` and `args_format`, and an answer that carries
    /// any of them replaces it.
    #[tokio::test]
    async fn the_function_registration_hook_sees_and_replaces_the_options() {
        ensure_default_meter();
        let engine = Engine::new();
        let (input_tx, mut input_rx) = mpsc::unbounded_channel::<serde_json::Value>();
        engine.register_function_handler(
            super::RegisterFunctionRequest {
                function_id: "rbac::function-hook".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            super::Handler::new(move |input: serde_json::Value| {
                let input_tx = input_tx.clone();
                async move {
                    let _ = input_tx.send(input.clone());
                    FunctionResult::Success(Some(serde_json::json!({
                        "function_id": input["function_id"],
                        "private": true,
                        "observable": false,
                        "args_format": { "type": "object" }
                    })))
                }
            }),
        );
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::with_session(
            tx,
            session_with_hooks(Some("rbac::function-hook"), None),
        );
        engine
            .router_msg(&worker, &fixture_frame("registerfunction"))
            .await
            .unwrap();

        let input = input_rx.recv().await.expect("the hook ran");
        assert_eq!(input["private"], false);
        assert_eq!(input["observable"], true);
        assert!(input.get("args_format").is_none(), "absent when not set");
        let options = engine
            .observers
            .options(DEFAULT_NAMESPACE, "orders::archive");
        assert!(options.private && !options.observable);
        assert_eq!(
            options.args_format,
            Some(serde_json::json!({ "type": "object" }))
        );
    }
}
