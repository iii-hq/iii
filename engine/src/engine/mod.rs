// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::ws::{Message as WsMessage, WebSocket},
    http::{HeaderMap, Uri},
};
use chrono::Utc;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot::error::RecvError};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;

use crate::{
    function::{Function, FunctionHandler, FunctionResult, FunctionsRegistry},
    invocation::{InvocationHandler, http_function::HttpFunctionConfig},
    protocol::{ErrorBody, Message},
    services::{Service, ServicesRegistry},
    telemetry::{
        SpanExt, ingest_otlp_json, ingest_otlp_logs, ingest_otlp_metrics,
        inject_baggage_from_context, inject_traceparent_from_context,
    },
    trigger::{Trigger, TriggerRegistry, TriggerType},
    worker_connections::{RuntimeWorkerInfo, WorkerConnection, WorkerConnectionRegistry},
    workers::worker::rbac_session::Session,
    workers::{
        engine_fn::TRIGGER_WORKERS_AVAILABLE,
        http_functions::HttpFunctionsWorker,
        worker::{WorkerManagerConfig, channels::ChannelManager, rbac_session},
    },
};

/// Abstraction for enqueuing messages to named queues.
///
/// This trait decouples the Engine from the concrete QueueWorker
/// so that dispatch routing can push work onto a named queue without
/// creating a circular dependency.
#[async_trait::async_trait]
pub trait QueueEnqueuer: Send + Sync {
    async fn enqueue_to_function_queue(
        &self,
        queue_name: &str,
        function_id: &str,
        data: serde_json::Value,
        message_id: String,
        traceparent: Option<String>,
        baggage: Option<String>,
    ) -> anyhow::Result<()>;

    async fn function_queue_dlq_count(&self, _queue_name: &str) -> anyhow::Result<u64> {
        Ok(0)
    }

    async fn function_queue_dlq_messages(
        &self,
        _queue_name: &str,
        _count: usize,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        Ok(vec![])
    }
}

/// Magic prefix for OTLP binary frames (used by SDKs for trace spans)
const OTLP_WS_PREFIX: &[u8] = b"OTLP";
/// Magic prefix for metrics binary frames (used by SDKs for OTEL metrics)
const MTRC_WS_PREFIX: &[u8] = b"MTRC";
/// Magic prefix for logs binary frames (used by SDKs for OTEL logs)
const LOGS_WS_PREFIX: &[u8] = b"LOGS";

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
    ) -> Result<Option<Value>, ErrorBody>;
    async fn register_trigger_type(&self, trigger_type: TriggerType);
    fn register_function(
        &self,
        request: RegisterFunctionRequest,
        handler: Box<dyn FunctionHandler + Send + Sync>,
    );
    fn register_function_handler<H, F>(
        &self,
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

#[derive(Clone)]
pub struct Engine {
    pub worker_registry: Arc<WorkerConnectionRegistry>,
    pub runtime_workers: Arc<DashMap<String, RuntimeWorkerInfo>>,
    pub functions: Arc<FunctionsRegistry>,
    pub trigger_registry: Arc<TriggerRegistry>,
    pub service_registry: Arc<ServicesRegistry>,
    pub invocations: Arc<InvocationHandler>,
    pub channel_manager: Arc<ChannelManager>,
    pub queue_module: Arc<tokio::sync::RwLock<Option<Arc<dyn QueueEnqueuer>>>>,
    /// Records the current owning WS worker for each registered function id.
    /// Populated when a worker sends `Message::RegisterFunction`; used by
    /// `cleanup_worker` and `remove_worker_registrations` to atomically skip
    /// removal of registrations that have been overwritten by a different,
    /// still-live worker (the fast-restart race). In-process workers do not
    /// populate this map, so the absence of an entry means "no WS owner."
    pub(crate) function_owners: Arc<DashMap<String, Uuid>>,
    /// HTTP-invocation variant of `function_owners`, separate because external
    /// functions live in their own per-worker set on `WorkerConnection`.
    pub(crate) external_function_owners: Arc<DashMap<String, Uuid>>,
    pub(crate) active_scope: Arc<std::sync::Mutex<Option<crate::workers::reload::ScopeBuilder>>>,
    /// Effective `iii-worker-manager` port, resolved from config at build
    /// time. Set once by `EngineBuilder::build`; subsequent reads see the
    /// same value for the engine's lifetime. Used by `registry_worker::
    /// ExternalWorkerProcess::spawn` so externally-spawned workers connect
    /// back to the actual configured port, not a hardcoded DEFAULT_PORT.
    worker_manager_port: Arc<std::sync::OnceLock<u16>>,
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
        Self {
            worker_registry: Arc::new(WorkerConnectionRegistry::new()),
            runtime_workers: Arc::new(DashMap::new()),
            functions: Arc::new(FunctionsRegistry::with_scope(active_scope.clone())),
            trigger_registry: Arc::new(TriggerRegistry::new()),
            service_registry: Arc::new(ServicesRegistry::new()),
            invocations: Arc::new(InvocationHandler::new()),
            channel_manager: Arc::new(ChannelManager::new()),
            queue_module: Arc::new(tokio::sync::RwLock::new(None)),
            function_owners: Arc::new(DashMap::new()),
            external_function_owners: Arc::new(DashMap::new()),
            active_scope,
            worker_manager_port: Arc::new(std::sync::OnceLock::new()),
        }
    }

    pub async fn set_queue_module(&self, module: Arc<dyn QueueEnqueuer>) {
        *self.queue_module.write().await = Some(module);
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
    /// Skips ids currently owned by a WS worker via `function_owners`
    /// (non-invocation path) or `external_function_owners` (HTTP-invocation
    /// path) — without this, destroying an in-process worker that happened
    /// to share a function id with a connected WS worker would tear out the
    /// WS worker's live registration (or its `service_registry` entry, in
    /// the HTTP case). In-process workers themselves do not populate either
    /// owner map, so the absence of an entry in both is the "no WS owner"
    /// signal that means we can safely remove.
    pub fn remove_worker_registrations(&self, regs: &crate::workers::reload::WorkerRegistrations) {
        for id in &regs.function_ids {
            if self.function_owners.contains_key(id)
                || self.external_function_owners.contains_key(id)
            {
                tracing::debug!(
                    function_id = %id,
                    "Skipping in-process registration removal — a WS worker currently owns this id"
                );
                continue;
            }
            self.remove_function_from_engine(id);
        }
    }

    async fn send_msg(&self, worker: &WorkerConnection, msg: Message) -> bool {
        worker.channel.send(Outbound::Protocol(msg)).await.is_ok()
    }

    fn remove_function(&self, function_id: &str) {
        self.functions.remove(function_id);
    }

    fn remove_function_from_engine(&self, function_id: &str) {
        self.remove_function(function_id);
        self.service_registry
            .remove_function_from_services(function_id);
    }

    async fn remember_invocation(
        &self,
        worker: &WorkerConnection,
        invocation_id: Option<Uuid>,
        function_id: &str,
        body: Value,
        traceparent: Option<String>,
        baggage: Option<String>,
    ) -> Result<Result<Option<Value>, ErrorBody>, RecvError> {
        tracing::debug!(
            worker_id = %worker.id,
            ?invocation_id,
            function_id = function_id,
            traceparent = ?traceparent,
            baggage = ?baggage,
            "Remembering invocation for worker"
        );

        if let Some(function) = self.functions.get(function_id) {
            if !crate::workers::telemetry::is_iii_builtin_function_id(function_id) {
                crate::workers::telemetry::collector::notify_user_function_invoked();
            }

            if let Some(invocation_id) = invocation_id {
                worker.add_invocation(invocation_id).await;
            }

            let session = worker.session.clone();

            self.invocations
                .handle_invocation(
                    invocation_id,
                    Some(worker.id),
                    function_id.to_string(),
                    body,
                    function,
                    traceparent,
                    baggage,
                    session,
                )
                .await
        } else {
            tracing::error!(function_id = %function_id, "Function not found");

            Ok(Err(ErrorBody {
                code: "function_not_found".into(),
                message: format!("Function {} not found", function_id),
                stacktrace: None,
            }))
        }
    }

    /// Spawns the standard invoke-function flow as a background task.
    ///
    /// When `invocation_id` is `Some`, an `InvocationResult` is sent back
    /// to the caller once the function completes.  When `None`, the call
    /// is fire-and-forget (used by the `Void` action).
    fn spawn_invoke_function(
        &self,
        worker: &WorkerConnection,
        function_id: &str,
        data: &Value,
        traceparent: &Option<String>,
        baggage: &Option<String>,
        invocation_id: Option<Uuid>,
    ) {
        let span = tracing::info_span!(
            "handle_invocation",
            otel.name = %format!("handle_invocation {}", function_id),
            worker_id = %worker.id,
            function_id = %function_id,
            invocation_id = ?invocation_id,
            otel.kind = "server",
            otel.status_code = tracing::field::Empty,
        )
        .with_parent_headers(traceparent.as_deref(), baggage.as_deref());

        let engine = self.clone();
        let worker = worker.clone();
        let function_id = function_id.to_string();

        // Add caller's worker_id to invocation data as standard metadata
        let data = {
            let mut data = data.clone();
            if let Some(obj) = data.as_object_mut() {
                obj.insert(
                    "_caller_worker_id".to_string(),
                    serde_json::json!(worker.id.to_string()),
                );
            }
            data
        };
        let incoming_traceparent = traceparent.clone();
        let incoming_baggage = baggage.clone();

        tokio::spawn(
            async move {
                let result = engine
                    .remember_invocation(
                        &worker,
                        invocation_id,
                        &function_id,
                        data,
                        incoming_traceparent.clone(),
                        incoming_baggage.clone(),
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
                                            function_id: function_id.clone(),
                                            result: result.clone(),
                                            error: None,
                                            traceparent: response_traceparent.clone(),
                                            baggage: response_baggage.clone(),
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
                                            function_id: function_id.clone(),
                                            result: None,
                                            error: Some(err.clone()),
                                            traceparent: response_traceparent.clone(),
                                            baggage: response_baggage.clone(),
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
                                        function_id: function_id.clone(),
                                        result: None,
                                        error: Some(ErrorBody {
                                            code: "invocation_error".into(),
                                            message: err.to_string(),
                                            stacktrace: None,
                                        }),
                                        traceparent: response_traceparent,
                                        baggage: response_baggage,
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

    #[doc(hidden)]
    pub async fn router_msg(&self, worker: &WorkerConnection, msg: &Message) -> anyhow::Result<()> {
        match msg {
            Message::TriggerRegistrationResult {
                id,
                trigger_type,
                function_id,
                error,
            } => {
                tracing::debug!(id = %id, trigger_type = %trigger_type, function_id = %function_id, error = ?error, "TriggerRegistrationResult");
                Ok(())
            }
            Message::RegisterTriggerType {
                id,
                description,
                trigger_request_format,
                call_request_format,
            } => {
                tracing::debug!(
                    worker_id = %worker.id,
                    trigger_type_id = %id,
                    description = %description,
                    "RegisterTriggerType"
                );

                let mut reg_id = id.clone();
                let mut reg_description = description.clone();

                if let Some(session) = &worker.session {
                    if !session.allow_trigger_type_registration {
                        tracing::warn!(
                            worker_id = %worker.id,
                            trigger_type_id = %id,
                            "trigger type registration not allowed for this session"
                        );
                        return Ok(());
                    }

                    if let Some(hook_fn_id) = session
                        .config
                        .rbac
                        .as_ref()
                        .and_then(|c| c.on_trigger_type_registration_function_id.as_ref())
                    {
                        let hook_input = serde_json::json!({
                            "trigger_type_id": id,
                            "description": description,
                            "context": session.context,
                        });
                        match self.call(hook_fn_id, hook_input).await {
                            Ok(Some(v)) if v.is_object() => {
                                if let Some(s) = v.get("trigger_type_id").and_then(|v| v.as_str()) {
                                    reg_id = s.to_string();
                                }
                                if let Some(s) = v.get("description").and_then(|v| v.as_str()) {
                                    reg_description = s.to_string();
                                }
                            }
                            other => {
                                tracing::warn!(
                                    worker_id = %worker.id,
                                    trigger_type_id = %id,
                                    result = ?other,
                                    "trigger type registration denied by hook"
                                );
                                return Ok(());
                            }
                        }
                    }
                }

                let mut trigger_type = TriggerType::new(
                    reg_id,
                    reg_description,
                    Box::new(worker.clone()),
                    Some(worker.id),
                );
                // Allow SDK workers to override formats from the protocol message
                if let Some(fmt) = trigger_request_format {
                    trigger_type.trigger_request_format = Some(fmt.clone());
                }
                if let Some(fmt) = call_request_format {
                    trigger_type.call_request_format = Some(fmt.clone());
                }

                let _ = self
                    .trigger_registry
                    .register_trigger_type(trigger_type)
                    .await;

                Ok(())
            }
            Message::RegisterTrigger {
                id,
                trigger_type,
                function_id,
                config,
                metadata,
            } => {
                tracing::debug!(
                    trigger_id = %id,
                    trigger_type = %trigger_type,
                    function_id = %function_id,
                    config = ?config,
                    "RegisterTrigger"
                );

                let mut reg_trigger_id = id.clone();
                let mut reg_trigger_type = trigger_type.clone();
                let mut reg_function_id = function_id.clone();
                let mut reg_config = config.clone();

                if let Some(session) = &worker.session {
                    if let Some(allowed_trigger_types) = &session.allowed_trigger_types
                        && !allowed_trigger_types.iter().any(|t| t == trigger_type)
                    {
                        tracing::warn!(
                            worker_id = %worker.id,
                            trigger_type = %trigger_type,
                            "trigger registration not allowed for type"
                        );
                        return Ok(());
                    }

                    if let Some(hook_fn_id) = session
                        .config
                        .rbac
                        .as_ref()
                        .and_then(|c| c.on_trigger_registration_function_id.as_ref())
                    {
                        let hook_input = serde_json::json!({
                            "trigger_id": id,
                            "trigger_type": trigger_type,
                            "function_id": function_id,
                            "config": config,
                            "metadata": metadata,
                            "context": session.context,
                        });
                        match self.call(hook_fn_id, hook_input).await {
                            Ok(Some(v)) if v.is_object() => {
                                if let Some(s) = v.get("trigger_id").and_then(|v| v.as_str()) {
                                    reg_trigger_id = s.to_string();
                                }
                                if let Some(s) = v.get("trigger_type").and_then(|v| v.as_str()) {
                                    reg_trigger_type = s.to_string();
                                }
                                if let Some(s) = v.get("function_id").and_then(|v| v.as_str()) {
                                    reg_function_id = s.to_string();
                                }
                                if let Some(c) = v.get("config").cloned() {
                                    reg_config = c;
                                }
                            }
                            other => {
                                tracing::warn!(
                                    worker_id = %worker.id,
                                    trigger_id = %id,
                                    result = ?other,
                                    "trigger registration denied by hook"
                                );
                                return Ok(());
                            }
                        }
                    }
                }

                if let Some(prefix) = worker
                    .session
                    .as_ref()
                    .and_then(|s| s.function_registration_prefix.as_ref())
                {
                    reg_function_id = format!("{prefix}::{reg_function_id}");
                }

                let _ = self
                    .trigger_registry
                    .register_trigger(Trigger {
                        id: reg_trigger_id,
                        trigger_type: reg_trigger_type,
                        function_id: reg_function_id,
                        config: reg_config,
                        worker_id: Some(worker.id),
                        metadata: metadata.clone(),
                    })
                    .await;
                crate::workers::telemetry::collector::track_trigger_registered();

                Ok(())
            }
            Message::UnregisterTrigger { id, trigger_type } => {
                tracing::debug!(
                    trigger_id = %id,
                    trigger_type = %trigger_type.as_deref().unwrap_or("<missing>"),
                    "UnregisterTrigger"
                );

                let _ = self
                    .trigger_registry
                    .unregister_trigger(id.clone(), trigger_type.clone())
                    .await;

                Ok(())
            }

            Message::InvokeFunction {
                invocation_id,
                function_id,
                data,
                traceparent,
                baggage,
                action,
            } => {
                tracing::debug!(
                    worker_id = %worker.id,
                    invocation_id = ?invocation_id,
                    function_id = %function_id,
                    traceparent = ?traceparent,
                    baggage = ?baggage,
                    action = ?action,
                    payload = ?data,
                    "InvokeFunction"
                );

                if let Some(session) = &worker.session {
                    let function = self.functions.get(function_id);
                    if !crate::workers::worker::rbac_config::is_function_allowed(
                        function_id,
                        session.config.rbac.clone(),
                        &session.allowed_functions,
                        &session.forbidden_functions,
                        function.as_ref(),
                    ) {
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
                            },
                        )
                        .await;
                        return Ok(());
                    }

                    if let Some(middleware_id) = &session.config.middleware_function_id
                        && !function_id.starts_with("engine::")
                    {
                        let inv_id = (*invocation_id).unwrap_or_else(Uuid::new_v4);
                        let middleware_input = serde_json::json!({
                            "function_id": function_id,
                            "payload": data,
                            "action": action,
                            "context": session.context,
                        });
                        let engine = self.clone();
                        let w = worker.clone();
                        let middleware_id = middleware_id.clone();
                        let function_id = function_id.clone();
                        let traceparent = traceparent.clone();
                        let baggage = baggage.clone();

                        tokio::spawn(async move {
                            let response = match engine.call(&middleware_id, middleware_input).await
                            {
                                Ok(result) => Message::InvocationResult {
                                    invocation_id: inv_id,
                                    function_id,
                                    result,
                                    error: None,
                                    traceparent,
                                    baggage,
                                },
                                Err(err) => Message::InvocationResult {
                                    invocation_id: inv_id,
                                    function_id,
                                    result: None,
                                    error: Some(err),
                                    traceparent,
                                    baggage,
                                },
                            };
                            engine.send_msg(&w, response).await;
                        });
                        return Ok(());
                    }
                }

                match action {
                    Some(crate::protocol::TriggerAction::Enqueue { queue }) => {
                        let engine = self.clone();
                        let worker = worker.clone();
                        let invocation_id = *invocation_id;
                        let function_id = function_id.to_string();
                        let queue = queue.to_string();
                        let message_receipt_id = Uuid::new_v4().to_string();
                        let data = data.clone();
                        let traceparent = traceparent.clone();
                        let baggage = baggage.clone();

                        let span = tracing::info_span!(
                            "enqueue_action",
                            otel.name = %format!("enqueue {} → {}", function_id, queue),
                            function_id = %function_id,
                            queue = %queue,
                        )
                        .with_parent_headers(traceparent.as_deref(), baggage.as_deref());

                        tokio::spawn(
                            async move {
                                let queue_module = engine.queue_module.read().await;
                                let result = match queue_module.as_ref() {
                                    Some(qm) => {
                                        qm.enqueue_to_function_queue(
                                            &queue,
                                            &function_id,
                                            data.clone(),
                                            message_receipt_id.clone(),
                                            traceparent.clone(),
                                            baggage.clone(),
                                        )
                                        .await
                                    }
                                    None => Err(anyhow::anyhow!("QueueModule not loaded")),
                                };

                                if let Some(invocation_id) = invocation_id {
                                    match result {
                                        Ok(()) => {
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
                                                    },
                                                )
                                                .await;
                                        }
                                        Err(err) => {
                                            engine
                                                .send_msg(
                                                    &worker,
                                                    Message::InvocationResult {
                                                        invocation_id,
                                                        function_id: function_id.clone(),
                                                        result: None,
                                                        error: Some(ErrorBody::new(
                                                            "enqueue_error",
                                                            err.to_string(),
                                                        )),
                                                        traceparent: traceparent.clone(),
                                                        baggage: baggage.clone(),
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

                    Some(crate::protocol::TriggerAction::Void) => {
                        // Fire-and-forget: invoke function but never send
                        // InvocationResult back to the caller.
                        self.spawn_invoke_function(
                            worker,
                            function_id,
                            data,
                            traceparent,
                            baggage,
                            None, // force invocation_id to None — no result sent
                        );
                        Ok(())
                    }

                    None => {
                        // Default behavior: invoke and (optionally) return result.
                        self.spawn_invoke_function(
                            worker,
                            function_id,
                            data,
                            traceparent,
                            baggage,
                            *invocation_id,
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
            } => {
                tracing::debug!(
                    function_id = %function_id,
                    invocation_id = %invocation_id,
                    result = ?result,
                    error = ?error,
                    "InvocationResult"
                );

                worker.remove_invocation(invocation_id).await;

                if let Some(invocation) = self.invocations.remove(invocation_id) {
                    if let Some(err) = error {
                        let _ = invocation.sender.send(Err(err.clone()));
                    } else {
                        let _ = invocation.sender.send(Ok(result.clone()));
                    };
                    return Ok(());
                } else {
                    // Expected when the caller disconnected before the
                    // executor finished (client-side trigger timeout,
                    // Ctrl-C, etc). `cleanup_worker` already halted the
                    // invocation, so the late result has nowhere to go.
                    // Kept at debug to avoid log noise for a normal
                    // condition — previously this was warn and produced
                    // a pair of scary lines on every slow sandbox::exec.
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
                if worker.has_external_function_id(&resolved_id).await {
                    worker.remove_external_function_id(&resolved_id).await;
                    // Only tear down the engine-global registration if this
                    // worker is still the recorded owner. Without the gate,
                    // an Unregister from a worker whose id was already
                    // hijacked by a fresher worker would wipe the live
                    // worker's http_module + service_registry entries — the
                    // same bug shape `cleanup_worker` guards against.
                    if !self.release_external_function_if_owner(&worker.id, &resolved_id) {
                        tracing::debug!(
                            worker_id = %worker.id,
                            function_id = %id,
                            "Skipping external UnregisterFunction — owner changed"
                        );
                        return Ok(());
                    }
                    if let Some(http_module) = self
                        .service_registry
                        .get_service::<HttpFunctionsWorker>("http_functions")
                    {
                        match http_module.unregister_http_function(&resolved_id).await {
                            Ok(()) => {
                                tracing::debug!(
                                    worker_id = %worker.id,
                                    function_id = %id,
                                    "Unregistered external function"
                                );
                            }
                            Err(err) => {
                                tracing::error!(
                                    worker_id = %worker.id,
                                    function_id = %id,
                                    error = ?err,
                                    "Failed to unregister external function"
                                );
                            }
                        }
                        self.service_registry
                            .remove_function_from_services(&resolved_id);
                    } else {
                        self.remove_function_from_engine(&resolved_id);
                    }
                } else {
                    worker.remove_function_id(&resolved_id).await;
                    // Same ownership gate as the external branch above.
                    if !self.release_function_if_owner(&worker.id, &resolved_id) {
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
                        let hook_input = serde_json::json!({
                            "function_id": id,
                            "description": description,
                            "metadata": metadata,
                            "context": session.context,
                        });
                        match self.call(hook_fn_id, hook_input).await {
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

                // Claim ownership BEFORE mutating any engine-global state. An
                // old worker's `cleanup_worker` running on another task can
                // see the pre-claim `function_owners` entry, match its own
                // id, and tear down the registration we're about to write.
                // Claiming first makes the CAS release in cleanup see the new
                // owner and bail out for every subsequent step.
                if invocation.is_some() {
                    self.claim_external_function(worker.id, &reg_id);
                } else {
                    self.claim_function(worker.id, &reg_id);
                }

                self.service_registry
                    .register_service_from_function_id(&reg_id);

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
                        self.release_external_function_if_owner(&worker.id, &reg_id);
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

                    if let Err(err) = http_module.register_http_function(config).await {
                        tracing::error!(
                            worker_id = %worker.id,
                            function_id = %reg_id,
                            error = ?err,
                            "Failed to register HTTP invocation function"
                        );
                        self.release_external_function_if_owner(&worker.id, &reg_id);
                        return Ok(());
                    }

                    worker.include_external_function_id(&reg_id).await;
                    return Ok(());
                }

                self.register_function(
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

                self.service_registry.insert_service(Service::with_parent(
                    effective_name.to_string(),
                    id.clone(),
                    parent_service_id.clone(),
                ));

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
        }
    }

    pub async fn fire_triggers(&self, trigger_type: &str, data: Value) {
        let triggers: Vec<crate::trigger::Trigger> = self
            .trigger_registry
            .triggers
            .iter()
            .filter(|entry| entry.value().trigger_type == trigger_type)
            .map(|entry| entry.value().clone())
            .collect();

        let current_span = tracing::Span::current();

        for trigger in triggers {
            let engine = self.clone();
            let function_id = trigger.function_id.clone();
            let data = data.clone();
            let parent = current_span.clone();
            let span_function_id = function_id.clone();
            tokio::spawn(
                async move {
                    match engine.call(&function_id, data).await {
                        Ok(_) => { tracing::Span::current().record("otel.status_code", "OK"); }
                        Err(_) => { tracing::Span::current().record("otel.status_code", "ERROR"); }
                    }
                }
                .instrument(tracing::info_span!(parent: parent, "trigger", otel.name = %format!("trigger {}", span_function_id), function_id = %span_function_id, otel.status_code = tracing::field::Empty))
            );
        }
    }

    pub async fn handle_worker(
        &self,
        socket: WebSocket,
        peer: SocketAddr,
        uri: Uri,
        headers: HeaderMap,
        config: Arc<WorkerManagerConfig>,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        tracing::debug!(peer = %peer, "Worker connected via WebSocket");
        let (mut ws_tx, mut ws_rx) = socket.split();

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

        let writer = tokio::spawn(async move {
            while let Some(outbound) = rx.recv().await {
                let send_result = match outbound {
                    Outbound::Protocol(msg) => match serde_json::to_string(&msg) {
                        Ok(payload) => ws_tx.send(WsMessage::Text(payload.into())).await,
                        Err(err) => {
                            tracing::error!(peer = %peer, error = ?err, "serialize error");
                            continue;
                        }
                    },
                    Outbound::Raw(frame) => ws_tx.send(frame).await,
                };

                if send_result.is_err() {
                    break;
                }
            }
        });

        let worker = WorkerConnection::with_session(tx.clone(), session);

        tracing::debug!(worker_id = %worker.id, peer = %peer, "Assigned worker ID");
        self.worker_registry.register_worker(worker.clone());

        // Send worker ID back to the worker
        self.send_msg(
            &worker,
            Message::WorkerRegistered {
                worker_id: worker.id.to_string(),
            },
        )
        .await;

        let workers_data = serde_json::json!({
            "event": "worker_connected",
            "worker_id": worker.id.to_string(),
        });
        self.fire_triggers(TRIGGER_WORKERS_AVAILABLE, workers_data)
            .await;

        loop {
            tokio::select! {
                frame = ws_rx.next() => {
                    match frame {
                        Some(Ok(WsMessage::Text(text))) => {
                            if text.trim().is_empty() {
                                continue;
                            }
                            match serde_json::from_str::<Message>(&text) {
                                Ok(msg) => self.router_msg(&worker, &msg).await?,
                                Err(err) => tracing::warn!(peer = %peer, error = ?err, "json decode error"),
                            }
                        }
                        Some(Ok(WsMessage::Binary(bytes))) => {
                            // Check for OTEL telemetry frames (OTLP, MTRC, LOGS prefixes)
                            if !handle_telemetry_frame(&bytes, &peer).await {
                                // Not a telemetry frame, try to decode as regular protocol message
                                match serde_json::from_slice::<Message>(&bytes) {
                                    Ok(msg) => self.router_msg(&worker, &msg).await?,
                                    Err(err) => {
                                        tracing::warn!(peer = %peer, error = ?err, "binary decode error")
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
                        Some(Err(_)) | None => {
                            break;
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    tracing::info!(peer = %peer, "Shutdown signal received, closing worker connection");
                    break;
                }
            }
        }

        writer.abort();
        self.cleanup_worker(&worker).await;
        tracing::debug!(peer = %peer, "Worker disconnected (writer aborted)");
        Ok(())
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
        let regular_functions = worker.get_regular_function_ids().await;
        let external_functions = worker.get_external_function_ids().await;

        tracing::debug!(worker_id = %worker.id, functions = ?regular_functions, "Worker registered functions");
        for function_id in regular_functions.iter() {
            if !self.release_function_if_owner(&worker.id, function_id) {
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
            for function_id in external_functions.iter() {
                // Snapshot ownership without releasing — releasing first would
                // open a window where a racing `RegisterFunction` can claim
                // ownership mid-teardown, and the remaining teardown steps
                // (service_registry + http_module) would then wipe the new
                // owner's fresh state. Keep ownership through teardown and
                // CAS-release at the end so a racing claim reliably aborts
                // us at the next ownership check.
                if !self
                    .external_function_owners
                    .get(function_id)
                    .is_some_and(|r| *r == worker.id)
                {
                    tracing::debug!(
                        worker_id = %worker.id,
                        function_id = %function_id,
                        "Skipping external function removal — owner changed"
                    );
                    continue;
                }
                match &http_module {
                    Some(module) => {
                        if let Err(err) = module.unregister_http_function(function_id).await {
                            tracing::error!(
                                worker_id = %worker.id,
                                function_id = %function_id,
                                error = ?err,
                                "Failed to unregister external function during worker cleanup"
                            );
                            self.remove_function(function_id);
                        }
                        // Re-check before wiping service_registry: the
                        // `.await` above is a yield point a racing claim can
                        // slip through, and service_registry is shared with
                        // the claimant's setup path (router_msg populates it
                        // before claim completes).
                        if self
                            .external_function_owners
                            .get(function_id)
                            .is_some_and(|r| *r == worker.id)
                        {
                            self.service_registry
                                .remove_function_from_services(function_id);
                        }
                    }
                    None => self.remove_function_from_engine(function_id),
                }
                // CAS-release ownership. A racing claim will have overwritten
                // the entry with a new owner id; that predicate fails and we
                // leave their ownership intact.
                self.external_function_owners
                    .remove_if(function_id, |_, owner| *owner == worker.id);
            }
        }

        let worker_invocations = worker.invocations.read().await;
        tracing::debug!(worker_id = %worker.id, invocations = ?worker_invocations, "Worker invocations");
        for invocation_id in worker_invocations.iter() {
            tracing::debug!(invocation_id = %invocation_id, "Halting invocation");
            self.invocations.halt_invocation(invocation_id);
        }

        self.trigger_registry.unregister_worker(&worker.id).await;
        self.channel_manager.remove_channels_by_worker(&worker.id);
        self.worker_registry.unregister_worker(&worker.id);

        let workers_data = serde_json::json!({
            "event": "worker_disconnected",
            "worker_id": worker.id.to_string(),
        });
        self.fire_triggers(TRIGGER_WORKERS_AVAILABLE, workers_data)
            .await;

        tracing::debug!(worker_id = %worker.id, "Worker triggers unregistered");
    }

    /// Records `worker_id` as the current owner of `function_id`. Logs a warning
    /// when the previous owner was a different worker still in the registry —
    /// that is the cross-worker overwrite shape that, post-defensive-cleanup,
    /// can leave a hijacked entry behind. Operators can grep for this WARN
    /// to detect potential function-id squatting.
    fn claim_function(&self, worker_id: Uuid, function_id: &str) {
        if let Some(previous) = self
            .function_owners
            .insert(function_id.to_string(), worker_id)
            && previous != worker_id
            && self.worker_registry.workers.contains_key(&previous)
        {
            tracing::warn!(
                new_owner = %worker_id,
                previous_owner = %previous,
                function_id = %function_id,
                "Function ownership transferred between two live workers — possible cross-worker overwrite"
            );
        }
    }

    /// HTTP-invocation variant of `claim_function`.
    fn claim_external_function(&self, worker_id: Uuid, function_id: &str) {
        if let Some(previous) = self
            .external_function_owners
            .insert(function_id.to_string(), worker_id)
            && previous != worker_id
            && self.worker_registry.workers.contains_key(&previous)
        {
            tracing::warn!(
                new_owner = %worker_id,
                previous_owner = %previous,
                function_id = %function_id,
                "External function ownership transferred between two live workers — possible cross-worker overwrite"
            );
        }
    }

    /// Atomically removes `function_id` from the engine ONLY if `worker_id`
    /// is still the recorded owner. Returns true if the removal occurred.
    /// `DashMap::remove_if` gives compare-and-swap semantics: no other thread
    /// can race the predicate against the actual remove, which closes the
    /// TOCTOU window a check-then-remove pair would leave between reading
    /// ownership and `remove_function_from_engine`.
    fn release_function_if_owner(&self, worker_id: &Uuid, function_id: &str) -> bool {
        let removed = self
            .function_owners
            .remove_if(function_id, |_, owner| owner == worker_id);
        if removed.is_some() {
            self.remove_function_from_engine(function_id);
            true
        } else {
            false
        }
    }

    /// External-function variant of `release_function_if_owner`. Caller is
    /// responsible for unregistering from `http_functions` and the service
    /// registry on success — this helper only releases the owner index.
    fn release_external_function_if_owner(&self, worker_id: &Uuid, function_id: &str) -> bool {
        self.external_function_owners
            .remove_if(function_id, |_, owner| owner == worker_id)
            .is_some()
    }
}

impl EngineTrait for Engine {
    /// Internal call path used by hooks, middleware, and fire_triggers — not direct
    /// user invocations over WebSocket. We intentionally skip
    /// `notify_user_function_invoked` here because this path serves engine
    /// orchestration; the boot-heartbeat wakeup should only fire for actual
    /// user-initiated invocations arriving via `remember_invocation` and not
    /// things the engine can fire itself without user involvement, such as cron.
    async fn call(
        &self,
        function_id: &str,
        input: impl Serialize + Send,
    ) -> Result<Option<Value>, ErrorBody> {
        let input = serde_json::to_value(input).map_err(|e| ErrorBody {
            code: "serialization_error".into(),
            message: e.to_string(),
            stacktrace: None,
        })?;
        let function_opt = self.functions.get(function_id);

        if let Some(function) = function_opt {
            // Inject current trace context and baggage to link spans as parent-child
            // Use the tracing span's context directly to ensure proper propagation in async code
            let ctx = tracing::Span::current().context();
            let traceparent = inject_traceparent_from_context(&ctx);
            let baggage = inject_baggage_from_context(&ctx);

            let result = self
                .invocations
                .handle_invocation(
                    None,
                    None,
                    function_id.to_string(),
                    input,
                    function,
                    traceparent,
                    baggage,
                    None,
                )
                .await;

            match result {
                Ok(result) => result,
                Err(err) => Err(ErrorBody {
                    code: "invocation_error".into(),
                    message: err.to_string(),
                    stacktrace: None,
                }),
            }
        } else {
            Err(ErrorBody {
                code: "function_not_found".into(),
                message: format!("Function {} not found", function_id),
                stacktrace: None,
            })
        }
    }

    async fn register_trigger_type(&self, trigger_type: TriggerType) {
        let trigger_type_id = &trigger_type.id;
        if self
            .trigger_registry
            .trigger_types
            .contains_key(trigger_type_id)
        {
            tracing::warn!(trigger_type_id = %trigger_type_id, "Trigger type already registered");
            return;
        }

        let _ = self
            .trigger_registry
            .register_trigger_type(trigger_type)
            .await;
    }

    fn register_function(
        &self,
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
            handler: Arc::new(move |invocation_id, input, _session| {
                let handler = handler_arc.clone();
                let path = handler_function_id.clone();
                Box::pin(async move { handler.handle_function(invocation_id, path, input).await })
            }),
            _function_id: function_id.clone(),
            _description: description,
            request_format,
            response_format,
            metadata,
        };

        self.functions.register_function(function_id, function);
        crate::workers::telemetry::collector::track_function_registered();
    }

    fn register_function_handler<H, F>(&self, request: RegisterFunctionRequest, handler: Handler<H>)
    where
        H: HandlerFn<F>,
        F: Future<Output = HandlerOutput> + Send + 'static,
    {
        let handler_arc: Arc<H> = Arc::new(handler.f);

        let function = Function {
            handler: Arc::new(move |_id, input, _session| {
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
            .register_function(request.function_id, function);
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
            handler: Arc::new(move |_id, input, session| {
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

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc,
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
        protocol::{HttpInvocationRef, Message},
        worker_connections::WorkerConnection,
        workers::{
            engine_fn::TRIGGER_WORKERS_AVAILABLE,
            http_functions::{HttpFunctionsWorker, config::HttpFunctionsConfig},
            observability::metrics::ensure_default_meter,
            traits::Worker,
        },
    };

    use super::{Engine, EngineTrait, Outbound};

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
        };

        engine
            .router_msg(&worker, &register_message)
            .await
            .expect("register function");

        assert!(engine.functions.get("external.my_lambda").is_some());
        assert!(worker.has_external_function_id("external.my_lambda").await);

        let http_module = engine
            .service_registry
            .get_service::<HttpFunctionsWorker>("http_functions")
            .expect("http_functions service registered");

        assert!(
            http_module
                .http_functions()
                .contains_key("external.my_lambda")
        );

        engine.cleanup_worker(&worker).await;

        assert!(engine.functions.get("external.my_lambda").is_none());

        assert!(
            !http_module
                .http_functions()
                .contains_key("external.my_lambda")
        );
    }

    // ---------------------------------------------------------------
    // 1. router_msg tests for different message types
    // ---------------------------------------------------------------

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
        };

        engine
            .router_msg(&worker, &msg)
            .await
            .expect("register function should succeed");

        // Function should be registered in the engine
        assert!(
            engine.functions.get("my_func").is_some(),
            "function should be registered"
        );

        // Worker should track the function id
        let function_ids = worker.get_regular_function_ids().await;
        assert!(
            function_ids.contains(&"my_func".to_string()),
            "worker should track the function id"
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
        };
        engine
            .router_msg(&worker, &register_msg)
            .await
            .expect("register should succeed");

        assert!(engine.functions.get("removable_func").is_some());

        // Now unregister it
        let unregister_msg = Message::UnregisterFunction {
            id: "removable_func".to_string(),
        };
        engine
            .router_msg(&worker, &unregister_msg)
            .await
            .expect("unregister should succeed");

        assert!(
            engine.functions.get("removable_func").is_none(),
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
            allowed_functions: vec![],
            forbidden_functions: vec![],
            allowed_trigger_types: None,
            allow_function_registration: true,
            allow_trigger_type_registration: true,
            context: serde_json::json!({}),
            function_registration_prefix: Some(prefix.to_string()),
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
        };
        engine
            .router_msg(&worker, &register_msg)
            .await
            .expect("register should succeed");

        assert!(
            engine
                .functions
                .get("test-prefix::removable_func")
                .is_some(),
            "function should be stored under the prefixed id"
        );
        assert!(
            engine.functions.get("removable_func").is_none(),
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
                .get("test-prefix::removable_func")
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
        };
        engine
            .router_msg(&worker, &register_msg)
            .await
            .expect("register external function should succeed");

        assert!(engine.functions.get("test-prefix::my_lambda").is_some());
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
            http_module
                .http_functions()
                .contains_key("test-prefix::my_lambda"),
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
            !http_module
                .http_functions()
                .contains_key("test-prefix::my_lambda"),
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
    async fn test_router_msg_register_trigger() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        // First register a trigger type so RegisterTrigger can succeed
        let register_type_msg = Message::RegisterTriggerType {
            id: "my_trigger_type".to_string(),
            description: "A test trigger type".to_string(),
            trigger_request_format: None,
            call_request_format: None,
        };
        engine
            .router_msg(&worker, &register_type_msg)
            .await
            .expect("register trigger type should succeed");

        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key("my_trigger_type"),
            "trigger type should be registered"
        );

        // Now register a trigger of that type
        let register_trigger_msg = Message::RegisterTrigger {
            id: "trigger_1".to_string(),
            trigger_type: "my_trigger_type".to_string(),
            function_id: "handler_func".to_string(),
            config: serde_json::json!({"key": "value"}),
            metadata: None,
        };
        engine
            .router_msg(&worker, &register_trigger_msg)
            .await
            .expect("register trigger should succeed");

        assert!(
            engine.trigger_registry.triggers.contains_key("trigger_1"),
            "trigger should be registered"
        );

        // Drain the channel - the trigger type registrator (worker) sends a RegisterTrigger message
        // back through the channel when a trigger is registered against the type
        while rx.try_recv().is_ok() {}
    }

    #[tokio::test]
    async fn test_router_msg_unregister_trigger() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        // Register trigger type first
        let register_type_msg = Message::RegisterTriggerType {
            id: "unreg_type".to_string(),
            description: "Trigger type for unregister test".to_string(),
            trigger_request_format: None,
            call_request_format: None,
        };
        engine
            .router_msg(&worker, &register_type_msg)
            .await
            .expect("register trigger type should succeed");

        // Register a trigger
        let register_trigger_msg = Message::RegisterTrigger {
            id: "unreg_trigger".to_string(),
            trigger_type: "unreg_type".to_string(),
            function_id: "handler_func".to_string(),
            config: serde_json::json!({}),
            metadata: None,
        };
        engine
            .router_msg(&worker, &register_trigger_msg)
            .await
            .expect("register trigger should succeed");

        assert!(
            engine
                .trigger_registry
                .triggers
                .contains_key("unreg_trigger")
        );

        // Drain channel messages from register
        while rx.try_recv().is_ok() {}

        // Now unregister the trigger
        let unregister_trigger_msg = Message::UnregisterTrigger {
            id: "unreg_trigger".to_string(),
            trigger_type: Some("unreg_type".to_string()),
        };
        engine
            .router_msg(&worker, &unregister_trigger_msg)
            .await
            .expect("unregister trigger should succeed");

        assert!(
            !engine
                .trigger_registry
                .triggers
                .contains_key("unreg_trigger"),
            "trigger should be removed after unregister"
        );

        // Drain channel messages from unregister
        while rx.try_recv().is_ok() {}
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
        assert_eq!(
            engine.trigger_registry.triggers.len(),
            0,
            "triggers should be empty"
        );
        assert_eq!(
            engine.trigger_registry.trigger_types.len(),
            0,
            "trigger types should be empty"
        );
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
    async fn test_register_trigger_type_duplicate_is_noop() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        engine
            .register_trigger_type(crate::trigger::TriggerType::new(
                "duplicate",
                "first",
                Box::new(worker.clone()),
                Some(worker.id),
            ))
            .await;
        engine
            .register_trigger_type(crate::trigger::TriggerType::new(
                "duplicate",
                "second",
                Box::new(worker.clone()),
                Some(worker.id),
            ))
            .await;

        assert_eq!(engine.trigger_registry.trigger_types.len(), 1);
        let trigger_type = engine
            .trigger_registry
            .trigger_types
            .get("duplicate")
            .expect("trigger type should remain registered");
        assert_eq!(trigger_type._description, "first");
    }

    #[tokio::test]
    async fn test_fire_triggers_invokes_only_matching_trigger_type() {
        ensure_default_meter();
        let engine = Engine::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let matching_counter = call_count.clone();

        engine.register_function_handler(
            make_request("engine::fire"),
            super::Handler::new(move |_input| {
                let matching_counter = matching_counter.clone();
                async move {
                    matching_counter.fetch_add(1, Ordering::SeqCst);
                    FunctionResult::Success(None)
                }
            }),
        );

        engine.trigger_registry.triggers.insert(
            "matching".to_string(),
            crate::trigger::Trigger {
                id: "matching".to_string(),
                trigger_type: TRIGGER_WORKERS_AVAILABLE.to_string(),
                function_id: "engine::fire".to_string(),
                config: json!({}),
                worker_id: None,
                metadata: None,
            },
        );
        engine.trigger_registry.triggers.insert(
            "other".to_string(),
            crate::trigger::Trigger {
                id: "other".to_string(),
                trigger_type: "engine::other".to_string(),
                function_id: "engine::fire".to_string(),
                config: json!({}),
                worker_id: None,
                metadata: None,
            },
        );

        engine
            .fire_triggers(TRIGGER_WORKERS_AVAILABLE, json!({ "event": "test" }))
            .await;

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
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
        };

        engine
            .router_msg(&worker, &register_message)
            .await
            .expect("register message should not fail");

        assert!(engine.functions.get("external.without_module").is_none());
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

        engine.register_function_handler(
            make_request("external.cleanup"),
            super::Handler::new(|_input| async move { FunctionResult::Success(None) }),
        );
        engine
            .service_registry
            .register_service_from_function_id("external.cleanup");
        worker
            .include_external_function_id("external.cleanup")
            .await;
        // Ownership gate on UnregisterFunction requires the worker to be the
        // recorded owner. This test sidesteps `router_msg` to seed state, so
        // populate the owner map directly to match the production invariant.
        engine.claim_external_function(worker.id, "external.cleanup");

        engine
            .router_msg(
                &worker,
                &Message::UnregisterFunction {
                    id: "external.cleanup".to_string(),
                },
            )
            .await
            .expect("unregister should succeed");

        assert!(engine.functions.get("external.cleanup").is_none());
        assert!(!worker.has_external_function_id("external.cleanup").await);
        assert!(!engine.service_registry.services.contains_key("external"));
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
                serde_json::json!({}),
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
        };
        engine
            .router_msg(&worker, &msg)
            .await
            .expect("register should succeed");

        assert!(engine.functions.get("to_remove").is_some());

        // Remove it directly
        engine.remove_function("to_remove");

        assert!(
            engine.functions.get("to_remove").is_none(),
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
            };
            engine
                .router_msg(&worker, &msg)
                .await
                .expect("register should succeed");
        }

        assert!(engine.functions.get("cleanup_func_a").is_some());
        assert!(engine.functions.get("cleanup_func_b").is_some());
        assert!(engine.functions.get("cleanup_func_c").is_some());

        // Cleanup the worker
        engine.cleanup_worker(&worker).await;

        // All functions should be removed
        assert!(
            engine.functions.get("cleanup_func_a").is_none(),
            "cleanup_func_a should be removed"
        );
        assert!(
            engine.functions.get("cleanup_func_b").is_none(),
            "cleanup_func_b should be removed"
        );
        assert!(
            engine.functions.get("cleanup_func_c").is_none(),
            "cleanup_func_c should be removed"
        );

        // Worker should be unregistered from worker registry
        assert!(
            engine.worker_registry.get_worker(&worker.id).is_none(),
            "worker should be unregistered"
        );
    }

    #[tokio::test]
    async fn test_cleanup_worker_removes_triggers() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        engine.worker_registry.register_worker(worker.clone());

        // Register a trigger type
        let register_type_msg = Message::RegisterTriggerType {
            id: "cleanup_trigger_type".to_string(),
            description: "Trigger type for cleanup test".to_string(),
            trigger_request_format: None,
            call_request_format: None,
        };
        engine
            .router_msg(&worker, &register_type_msg)
            .await
            .expect("register trigger type should succeed");

        // Register a trigger
        let register_trigger_msg = Message::RegisterTrigger {
            id: "cleanup_trigger".to_string(),
            trigger_type: "cleanup_trigger_type".to_string(),
            function_id: "some_func".to_string(),
            config: serde_json::json!({}),
            metadata: None,
        };
        engine
            .router_msg(&worker, &register_trigger_msg)
            .await
            .expect("register trigger should succeed");

        assert!(
            engine
                .trigger_registry
                .triggers
                .contains_key("cleanup_trigger")
        );
        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key("cleanup_trigger_type")
        );

        // Drain channel messages
        while rx.try_recv().is_ok() {}

        // Cleanup the worker
        engine.cleanup_worker(&worker).await;

        // Triggers and trigger types owned by this worker should be removed
        assert!(
            !engine
                .trigger_registry
                .triggers
                .contains_key("cleanup_trigger"),
            "trigger should be removed after worker cleanup"
        );
        assert!(
            !engine
                .trigger_registry
                .trigger_types
                .contains_key("cleanup_trigger_type"),
            "trigger type should be removed after worker cleanup"
        );

        // Drain any remaining channel messages from cleanup
        while rx.try_recv().is_ok() {}
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
    // router_msg: Ping / Pong / WorkerRegistered / TriggerRegistrationResult
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

    #[tokio::test]
    async fn test_router_msg_trigger_registration_result_is_noop() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let msg = Message::TriggerRegistrationResult {
            id: "trigger-1".to_string(),
            trigger_type: "my-type".to_string(),
            function_id: "my-func".to_string(),
            error: None,
        };

        engine
            .router_msg(&worker, &msg)
            .await
            .expect("TriggerRegistrationResult should succeed");

        // Should not produce any response
        assert!(
            rx.try_recv().is_err(),
            "TriggerRegistrationResult should not produce any outbound message"
        );
    }

    #[tokio::test]
    async fn test_router_msg_trigger_registration_result_with_error() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let msg = Message::TriggerRegistrationResult {
            id: "trigger-1".to_string(),
            trigger_type: "my-type".to_string(),
            function_id: "my-func".to_string(),
            error: Some(crate::protocol::ErrorBody {
                code: "registration_failed".to_string(),
                message: "registration failed".to_string(),
                stacktrace: None,
            }),
        };

        // Should still succeed (just logs the error)
        engine
            .router_msg(&worker, &msg)
            .await
            .expect("TriggerRegistrationResult with error should succeed");
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
            engine.service_registry.services.contains_key("my-service"),
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

        assert!(
            engine
                .service_registry
                .services
                .contains_key("minimal-service")
        );
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
        };
        engine
            .router_msg(&worker, &msg)
            .await
            .expect("register should succeed");

        assert!(engine.functions.get("cleanup_func").is_some());

        // Now cleanup
        engine.cleanup_worker(&worker).await;

        // Function should be removed
        assert!(engine.functions.get("cleanup_func").is_none());
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
        };
        engine
            .router_msg(&old_worker, &register_msg)
            .await
            .expect("old worker register should succeed");
        assert!(engine.functions.get("shared_func").is_some());

        // NEW worker connects, re-registers the same function_id. This
        // simulates the fast-restart race where the host watcher
        // spawns a new VM process that races the engine's disconnect
        // handling for the old process.
        let (tx_new, _rx_new) = mpsc::channel::<Outbound>(8);
        let new_worker = WorkerConnection::new(tx_new);
        engine.worker_registry.register_worker(new_worker.clone());
        engine
            .router_msg(&new_worker, &register_msg)
            .await
            .expect("new worker register should succeed");
        assert!(
            engine.functions.get("shared_func").is_some(),
            "new worker's registration should be in the function_registry"
        );

        // Now the OLD worker's cleanup fires — simulating the late
        // disconnect-detection path. Without the ownership check,
        // this would remove shared_func out from under the new
        // worker.
        engine.cleanup_worker(&old_worker).await;

        // Function must still be registered: the new worker owns it.
        assert!(
            engine.functions.get("shared_func").is_some(),
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
        };

        let (tx_old, _rx_old) = mpsc::channel::<Outbound>(8);
        let old_worker = WorkerConnection::new(tx_old);
        engine.worker_registry.register_worker(old_worker.clone());
        engine
            .router_msg(&old_worker, &make_msg())
            .await
            .expect("old worker register should succeed");
        assert!(engine.functions.get("external.shared").is_some());
        let http_module = engine
            .service_registry
            .get_service::<HttpFunctionsWorker>("http_functions")
            .expect("http_functions service registered");
        assert!(http_module.http_functions().contains_key("external.shared"));

        let (tx_new, _rx_new) = mpsc::channel::<Outbound>(8);
        let new_worker = WorkerConnection::new(tx_new);
        engine.worker_registry.register_worker(new_worker.clone());
        engine
            .router_msg(&new_worker, &make_msg())
            .await
            .expect("new worker register should succeed");

        engine.cleanup_worker(&old_worker).await;

        assert!(
            engine.functions.get("external.shared").is_some(),
            "cleanup of old worker must not remove an external function still owned by a live new worker"
        );
        assert!(
            http_module.http_functions().contains_key("external.shared"),
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
        engine
            .service_registry
            .register_service_from_function_id("external.shared.no_http");

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
        // worker is currently the owner of one of those ids (either
        // non-invocation via `function_owners` or HTTP-invocation via
        // `external_function_owners`), the removal must be skipped so
        // the live WS registration survives. This covers the new
        // ownership-aware branch added at engine/src/engine/mod.rs:347.
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
                },
            )
            .await
            .expect("WS worker register should succeed");
        assert!(engine.functions.get("ws_fn").is_some());
        assert!(engine.function_owners.contains_key("ws_fn"));

        // Also seed an external-owned id directly — we don't run the
        // HTTP registration path here (it requires the http_functions
        // service), so we populate `external_function_owners` by hand
        // to exercise the `|| external_function_owners` leg of the
        // skip branch.
        engine
            .external_function_owners
            .insert("ext_fn".to_string(), ws_worker.id);

        // Simulate an in-process worker teardown whose scope captured
        // both ids. Before the ownership check was added, this loop
        // would unconditionally call `remove_function_from_engine`
        // and wipe the WS worker's live registrations.
        let regs = crate::workers::reload::WorkerRegistrations {
            function_ids: vec!["ws_fn".to_string(), "ext_fn".to_string()],
        };
        engine.remove_worker_registrations(&regs);

        assert!(
            engine.functions.get("ws_fn").is_some(),
            "WS-owned non-invocation function must survive in-process teardown"
        );
        assert!(
            engine.function_owners.contains_key("ws_fn"),
            "ownership entry for the WS worker must be intact"
        );
        assert!(
            engine.external_function_owners.contains_key("ext_fn"),
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
                    },
                )
                .await
                .expect("old worker register should succeed");

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
                engine.functions.get("raced_fn").is_some(),
                "new worker's registration must survive concurrent cleanup of old worker"
            );
            let owner = engine
                .function_owners
                .get("raced_fn")
                .expect("function_owners must still have raced_fn");
            assert_eq!(
                *owner, new_worker.id,
                "the new worker should be the recorded owner after the race"
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
                },
            )
            .await
            .expect("A register should succeed");

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
                },
            )
            .await
            .expect("B register should succeed");

        // Ownership has transferred to B even though A still has the id in
        // its local function_ids set (A's set was not cleared by B's register).
        assert_eq!(
            *engine
                .function_owners
                .get("reg_hijacked")
                .expect("owner present"),
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
            engine.functions.get("reg_hijacked").is_some(),
            "B's live registration must survive A's stale UnregisterFunction"
        );
        assert_eq!(
            *engine
                .function_owners
                .get("reg_hijacked")
                .expect("owner still present"),
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
        };

        let (tx_a, _rx_a) = mpsc::channel::<Outbound>(8);
        let worker_a = WorkerConnection::new(tx_a);
        engine.worker_registry.register_worker(worker_a.clone());
        engine
            .router_msg(&worker_a, &make_msg())
            .await
            .expect("A register should succeed");

        let (tx_b, _rx_b) = mpsc::channel::<Outbound>(8);
        let worker_b = WorkerConnection::new(tx_b);
        engine.worker_registry.register_worker(worker_b.clone());
        engine
            .router_msg(&worker_b, &make_msg())
            .await
            .expect("B register should succeed");

        assert_eq!(
            *engine
                .external_function_owners
                .get("ext_hijacked")
                .expect("external owner present"),
            worker_b.id,
            "external owner should be B after hijacking register"
        );
        let http_module = engine
            .service_registry
            .get_service::<HttpFunctionsWorker>("http_functions")
            .expect("http_functions service registered");
        assert!(http_module.http_functions().contains_key("ext_hijacked"));

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
            engine.functions.get("ext_hijacked").is_some(),
            "B's live engine.functions entry must survive A's stale UnregisterFunction"
        );
        assert!(
            http_module.http_functions().contains_key("ext_hijacked"),
            "B's http_module entry must survive A's stale UnregisterFunction"
        );
        assert_eq!(
            *engine
                .external_function_owners
                .get("ext_hijacked")
                .expect("external owner still present"),
            worker_b.id,
            "external owner entry must remain B"
        );
    }

    #[tokio::test]
    async fn test_cleanup_worker_with_triggers() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        engine.worker_registry.register_worker(worker.clone());

        // Register trigger type
        let tt_msg = Message::RegisterTriggerType {
            id: "cleanup_trigger_type".to_string(),
            description: "Test trigger type for cleanup".to_string(),
            trigger_request_format: None,
            call_request_format: None,
        };
        engine
            .router_msg(&worker, &tt_msg)
            .await
            .expect("register trigger type should succeed");

        // Register a trigger
        let t_msg = Message::RegisterTrigger {
            id: "cleanup_trigger".to_string(),
            trigger_type: "cleanup_trigger_type".to_string(),
            function_id: "handler_func".to_string(),
            config: serde_json::json!({}),
            metadata: None,
        };
        engine
            .router_msg(&worker, &t_msg)
            .await
            .expect("register trigger should succeed");

        // Drain channel
        while rx.try_recv().is_ok() {}

        assert!(
            engine
                .trigger_registry
                .triggers
                .contains_key("cleanup_trigger")
        );

        // Cleanup
        engine.cleanup_worker(&worker).await;

        // Trigger should be removed (unregister_worker removes all triggers for the worker)
        assert!(
            !engine
                .trigger_registry
                .triggers
                .contains_key("cleanup_trigger")
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

        // cleanup_worker halts each invocation but does not clear the worker's
        // invocation list, so the count remains unchanged.
        assert_eq!(worker.invocation_count().await, 2);
    }

    // =========================================================================
    // Engine state tests
    // =========================================================================

    #[test]
    fn test_engine_new_defaults() {
        ensure_default_meter();
        let engine = Engine::new();

        assert!(engine.functions.get("nonexistent").is_none());
        assert!(!engine.trigger_registry.triggers.contains_key("anything"));
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

    #[tokio::test]
    async fn test_router_msg_register_trigger_with_metadata() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = WorkerConnection::new(tx);

        let register_type_msg = Message::RegisterTriggerType {
            id: "metadata_type".to_string(),
            description: "Trigger type for metadata test".to_string(),
            trigger_request_format: None,
            call_request_format: None,
        };
        engine
            .router_msg(&worker, &register_type_msg)
            .await
            .unwrap();

        let register_trigger_msg = Message::RegisterTrigger {
            id: "trigger_meta_1".to_string(),
            trigger_type: "metadata_type".to_string(),
            function_id: "handler_func".to_string(),
            config: serde_json::json!({"key": "value"}),
            metadata: Some(serde_json::json!({"team": "platform", "env": "staging"})),
        };
        engine
            .router_msg(&worker, &register_trigger_msg)
            .await
            .unwrap();

        assert!(
            engine
                .trigger_registry
                .triggers
                .contains_key("trigger_meta_1")
        );
        let trigger = engine
            .trigger_registry
            .triggers
            .get("trigger_meta_1")
            .unwrap();
        assert_eq!(
            trigger.metadata,
            Some(serde_json::json!({"team": "platform", "env": "staging"}))
        );

        while rx.try_recv().is_ok() {}
    }
}
