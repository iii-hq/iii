// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{net::SocketAddr, sync::Arc};

use axum::extract::ws::{Message as WsMessage, WebSocket};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot::error::RecvError};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;

use crate::{
    channels::ChannelManager,
    function::{Function, FunctionHandler, FunctionResult, FunctionsRegistry},
    invocation::{InvocationHandler, http_function::HttpFunctionConfig},
    modules::{http_functions::HttpFunctionsModule, worker::TRIGGER_WORKERS_AVAILABLE},
    protocol::{ErrorBody, Message},
    services::{Service, ServicesRegistry},
    telemetry::{
        SpanExt, ingest_otlp_json, ingest_otlp_logs, ingest_otlp_metrics,
        inject_baggage_from_context, inject_traceparent_from_context,
    },
    trigger::{Trigger, TriggerRegistry, TriggerType},
    workers::{Worker, WorkerRegistry},
};

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

pub struct RegisterFunctionRequest {
    pub function_id: String,
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
}

pub struct Handler<H> {
    f: H,
}

impl<H, F> Handler<H>
where
    H: Fn(Value) -> F + Send + Sync + 'static,
    F: Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'static,
{
    pub fn new(f: H) -> Self {
        Self { f }
    }

    pub fn call(&self, input: Value) -> F {
        (self.f)(input)
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
        H: Fn(Value) -> F + Send + Sync + 'static,
        F: Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'static;
}

#[derive(Default, Clone)]
pub struct Engine {
    pub worker_registry: Arc<WorkerRegistry>,
    pub functions: Arc<FunctionsRegistry>,
    pub trigger_registry: Arc<TriggerRegistry>,
    pub service_registry: Arc<ServicesRegistry>,
    pub invocations: Arc<InvocationHandler>,
    pub channel_manager: Arc<ChannelManager>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            worker_registry: Arc::new(WorkerRegistry::new()),
            functions: Arc::new(FunctionsRegistry::new()),
            trigger_registry: Arc::new(TriggerRegistry::new()),
            service_registry: Arc::new(ServicesRegistry::new()),
            invocations: Arc::new(InvocationHandler::new()),
            channel_manager: Arc::new(ChannelManager::new()),
        }
    }

    async fn send_msg(&self, worker: &Worker, msg: Message) -> bool {
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
        worker: &Worker,
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
            if let Some(invocation_id) = invocation_id {
                worker.add_invocation(invocation_id).await;
            }

            self.invocations
                .handle_invocation(
                    invocation_id,
                    Some(worker.id),
                    function_id.to_string(),
                    body,
                    function,
                    traceparent,
                    baggage,
                )
                .await
        } else {
            tracing::error!(function_id = %function_id, "Function not found");

            Ok(Err(ErrorBody {
                code: "function_not_found".into(),
                message: format!("Function {} not found", function_id),
            }))
        }
    }

    async fn router_msg(&self, worker: &Worker, msg: &Message) -> anyhow::Result<()> {
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
            Message::RegisterTriggerType { id, description } => {
                tracing::debug!(
                    worker_id = %worker.id,
                    trigger_type_id = %id,
                    description = %description,
                    "RegisterTriggerType"
                );
                let trigger_type = TriggerType {
                    id: id.clone(),
                    _description: description.clone(),
                    registrator: Box::new(worker.clone()),
                    worker_id: Some(worker.id),
                };

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
            } => {
                tracing::debug!(
                    trigger_id = %id,
                    trigger_type = %trigger_type,
                    function_id = %function_id,
                    config = ?config,
                    "RegisterTrigger"
                );

                let _ = self
                    .trigger_registry
                    .register_trigger(Trigger {
                        id: id.clone(),
                        trigger_type: trigger_type.clone(),
                        function_id: function_id.clone(),
                        config: config.clone(),
                        worker_id: Some(worker.id),
                    })
                    .await;
                crate::modules::telemetry::collector::track_trigger_registered();

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
            } => {
                tracing::debug!(
                    worker_id = %worker.id,
                    invocation_id = ?invocation_id,
                    function_id = %function_id,
                    traceparent = ?traceparent,
                    baggage = ?baggage,
                    payload = ?data,
                    "InvokeFunction"
                );

                // Create a span that's linked to the incoming trace context (if any)
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
                let invocation_id = *invocation_id;
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
                            // Inject traceparent/baggage from the span's explicit context
                            // (using tracing::Span::current().context() for reliable propagation)
                            let current_ctx = tracing::Span::current().context();
                            let response_traceparent =
                                inject_traceparent_from_context(&current_ctx)
                                    .or(incoming_traceparent);
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
                                        tracing::Span::current()
                                            .record("otel.status_code", "ERROR");
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

                Ok(())
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
                    tracing::warn!(
                        invocation_id = %invocation_id,
                        "Did not find caller for invocation"
                    );
                }
                Ok(())
            }
            Message::UnregisterFunction { id } => {
                tracing::debug!(
                    function_id = %id,
                    "UnregisterFunction"
                );
                if worker.has_external_function_id(id).await {
                    worker.remove_external_function_id(id).await;
                    if let Some(http_module) = self
                        .service_registry
                        .get_service::<HttpFunctionsModule>("http_functions")
                    {
                        match http_module.unregister_http_function(id).await {
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
                        self.service_registry.remove_function_from_services(id);
                    } else {
                        self.remove_function_from_engine(id);
                    }
                } else {
                    worker.remove_function_id(id).await;
                    self.remove_function_from_engine(id);
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

                self.service_registry.register_service_from_function_id(id);

                if let Some(invocation) = invocation {
                    let Some(http_module) = self
                        .service_registry
                        .get_service::<HttpFunctionsModule>("http_functions")
                    else {
                        tracing::error!(
                            worker_id = %worker.id,
                            function_id = %id,
                            "HTTP functions module not loaded"
                        );
                        return Ok(());
                    };

                    let config = HttpFunctionConfig {
                        function_path: id.clone(),
                        url: invocation.url.clone(),
                        method: invocation.method.clone(),
                        timeout_ms: invocation.timeout_ms,
                        headers: invocation.headers.clone(),
                        auth: invocation.auth.clone(),
                        description: description.clone(),
                        request_format: req.clone(),
                        response_format: res.clone(),
                        metadata: metadata.clone(),
                        registered_at: Some(Utc::now()),
                        updated_at: None,
                    };

                    if let Err(err) = http_module.register_http_function(config).await {
                        tracing::error!(
                            worker_id = %worker.id,
                            function_id = %id,
                            error = ?err,
                            "Failed to register HTTP invocation function"
                        );
                        return Ok(());
                    }

                    worker.include_external_function_id(id).await;
                    return Ok(());
                }

                self.register_function(
                    RegisterFunctionRequest {
                        function_id: id.clone(),
                        description: description.clone(),
                        request_format: req.clone(),
                        response_format: res.clone(),
                        metadata: metadata.clone(),
                    },
                    Box::new(worker.clone()),
                );

                worker.include_function_id(id).await;
                Ok(())
            }
            Message::RegisterService {
                id,
                name,
                description,
            } => {
                tracing::debug!(
                    service_id = %id,
                    service_name = %name,
                    description = ?description,
                    "RegisterService"
                );
                let services = self
                    .service_registry
                    .services
                    .iter()
                    .map(|entry| entry.key().clone())
                    .collect::<Vec<_>>();
                tracing::debug!(services = ?services, "Current services");

                self.service_registry
                    .insert_service(Service::new(name.clone(), id.clone()));

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
                .instrument(tracing::info_span!(parent: parent, "fire_trigger", function_id = %span_function_id, otel.status_code = tracing::field::Empty))
            );
        }
    }

    pub async fn handle_worker(
        &self,
        socket: WebSocket,
        peer: SocketAddr,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        tracing::debug!(peer = %peer, "Worker connected via WebSocket");
        let (mut ws_tx, mut ws_rx) = socket.split();
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

        let worker = Worker::with_ip(tx.clone(), peer.ip().to_string());

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

    async fn cleanup_worker(&self, worker: &Worker) {
        let regular_functions = worker.get_regular_function_ids().await;
        let external_functions = worker.get_external_function_ids().await;

        tracing::debug!(worker_id = %worker.id, functions = ?regular_functions, "Worker registered functions");
        for function_id in regular_functions.iter() {
            self.remove_function_from_engine(function_id);
        }

        if !external_functions.is_empty() {
            if let Some(http_module) = self
                .service_registry
                .get_service::<HttpFunctionsModule>("http_functions")
            {
                for function_id in external_functions.iter() {
                    if let Err(err) = http_module.unregister_http_function(function_id).await {
                        tracing::error!(
                            worker_id = %worker.id,
                            function_id = %function_id,
                            error = ?err,
                            "Failed to unregister external function during worker cleanup"
                        );
                        self.remove_function(function_id);
                    }
                    self.service_registry
                        .remove_function_from_services(function_id);
                }
            } else {
                for function_id in external_functions.iter() {
                    self.remove_function_from_engine(function_id);
                }
            }
        }

        let worker_invocations: Vec<Uuid> =
            worker.invocations.read().await.iter().copied().collect();
        tracing::debug!(worker_id = %worker.id, invocations = ?worker_invocations, "Worker invocations");
        for invocation_id in &worker_invocations {
            tracing::debug!(invocation_id = %invocation_id, "Halting invocation");
            self.invocations.halt_invocation(invocation_id);
        }
        worker.invocations.write().await.clear();

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
}

impl EngineTrait for Engine {
    async fn call(
        &self,
        function_id: &str,
        input: impl Serialize + Send,
    ) -> Result<Option<Value>, ErrorBody> {
        let input = serde_json::to_value(input).map_err(|e| ErrorBody {
            code: "serialization_error".into(),
            message: e.to_string(),
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
                )
                .await;

            match result {
                Ok(result) => result,
                Err(err) => Err(ErrorBody {
                    code: "invocation_error".into(),
                    message: err.to_string(),
                }),
            }
        } else {
            Err(ErrorBody {
                code: "function_not_found".into(),
                message: format!("Function {} not found", function_id),
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
            handler: Arc::new(move |invocation_id, input| {
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
        crate::modules::telemetry::collector::track_function_registered();
    }

    fn register_function_handler<H, F>(&self, request: RegisterFunctionRequest, handler: Handler<H>)
    where
        H: Fn(Value) -> F + Send + Sync + 'static,
        F: Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'static,
    {
        let handler_arc: Arc<H> = Arc::new(handler.f);

        let function = Function {
            handler: Arc::new(move |_id, input| {
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
        modules::{
            http_functions::{HttpFunctionsModule, config::HttpFunctionsConfig},
            module::Module,
            observability::metrics::ensure_default_meter,
            worker::TRIGGER_WORKERS_AVAILABLE,
        },
        protocol::{HttpInvocationRef, Message},
        workers::Worker,
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

        let http_functions_module = HttpFunctionsModule::create(
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
        let worker = Worker::new(tx);

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
            .get_service::<HttpFunctionsModule>("http_functions")
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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

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

    #[tokio::test]
    async fn test_router_msg_invoke_result() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, _rx) = mpsc::channel::<Outbound>(8);
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

        // First register a trigger type so RegisterTrigger can succeed
        let register_type_msg = Message::RegisterTriggerType {
            id: "my_trigger_type".to_string(),
            description: "A test trigger type".to_string(),
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
        let worker = Worker::new(tx);

        // Register trigger type first
        let register_type_msg = Message::RegisterTriggerType {
            id: "unreg_type".to_string(),
            description: "Trigger type for unregister test".to_string(),
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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

        engine.register_function_handler(
            make_request("engine::failure"),
            super::Handler::new(|_input| async move {
                FunctionResult::Failure(crate::protocol::ErrorBody {
                    code: "boom".to_string(),
                    message: "handler failed".to_string(),
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
        let worker = Worker::new(tx);

        let invocation_id = uuid::Uuid::new_v4();
        let invoke_msg = Message::InvokeFunction {
            invocation_id: Some(invocation_id),
            function_id: "engine::missing".to_string(),
            data: json!({}),
            traceparent: None,
            baggage: None,
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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

        engine
            .register_trigger_type(crate::trigger::TriggerType {
                id: "duplicate".to_string(),
                _description: "first".to_string(),
                registrator: Box::new(worker.clone()),
                worker_id: Some(worker.id),
            })
            .await;
        engine
            .register_trigger_type(crate::trigger::TriggerType {
                id: "duplicate".to_string(),
                _description: "second".to_string(),
                registrator: Box::new(worker.clone()),
                worker_id: Some(worker.id),
            })
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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

        engine.worker_registry.register_worker(worker.clone());

        // Register a trigger type
        let register_type_msg = Message::RegisterTriggerType {
            id: "cleanup_trigger_type".to_string(),
            description: "Trigger type for cleanup test".to_string(),
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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

        let msg = Message::TriggerRegistrationResult {
            id: "trigger-1".to_string(),
            trigger_type: "my-type".to_string(),
            function_id: "my-func".to_string(),
            error: Some(crate::protocol::ErrorBody {
                code: "registration_failed".to_string(),
                message: "registration failed".to_string(),
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
        let worker = Worker::new(tx);

        let msg = Message::RegisterService {
            id: "service-1".to_string(),
            name: "my-service".to_string(),
            description: Some("A test service".to_string()),
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
        let worker = Worker::new(tx);

        let msg = Message::RegisterService {
            id: "service-2".to_string(),
            name: "minimal-service".to_string(),
            description: None,
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
        let worker = Worker::new(tx);

        let invocation_id = uuid::Uuid::new_v4();
        worker.add_invocation(invocation_id).await;

        let msg = Message::InvocationResult {
            invocation_id,
            function_id: "some_func".to_string(),
            result: None,
            error: Some(crate::protocol::ErrorBody {
                code: "timeout".to_string(),
                message: "Function timed out".to_string(),
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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

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
    async fn test_cleanup_worker_with_triggers() {
        ensure_default_meter();
        let engine = Engine::new();
        let (tx, mut rx) = mpsc::channel::<Outbound>(8);
        let worker = Worker::new(tx);

        engine.worker_registry.register_worker(worker.clone());

        // Register trigger type
        let tt_msg = Message::RegisterTriggerType {
            id: "cleanup_trigger_type".to_string(),
            description: "Test trigger type for cleanup".to_string(),
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
        let worker = Worker::new(tx);

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
        let worker = Worker::new(tx);

        engine.worker_registry.register_worker(worker.clone());

        let inv1 = uuid::Uuid::new_v4();
        let inv2 = uuid::Uuid::new_v4();
        worker.add_invocation(inv1).await;
        worker.add_invocation(inv2).await;

        engine.cleanup_worker(&worker).await;

        assert_eq!(worker.invocation_count().await, 0);
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
}
