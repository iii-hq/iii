// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{net::SocketAddr, sync::Arc};

use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot::error::RecvError};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;

use crate::{
    function::{Function, FunctionHandler, FunctionResult, FunctionsRegistry},
    invocation::InvocationHandler,
    modules::worker::TRIGGER_WORKERS_AVAILABLE,
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
    let (prefix, name, result) = if bytes.starts_with(OTLP_WS_PREFIX) {
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
    let _ = prefix; // Suppress unused warning
    true
}

#[derive(Debug)]
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
    async fn call(&self, function_id: &str, input: Value) -> Result<Option<Value>, ErrorBody>;
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
}

impl Engine {
    pub fn new() -> Self {
        Self {
            worker_registry: Arc::new(WorkerRegistry::new()),
            functions: Arc::new(FunctionsRegistry::new()),
            trigger_registry: Arc::new(TriggerRegistry::new()),
            service_registry: Arc::new(ServicesRegistry::new()),
            invocations: Arc::new(InvocationHandler::new()),
        }
    }

    async fn send_msg(&self, worker: &Worker, msg: Message) -> bool {
        worker.channel.send(Outbound::Protocol(msg)).await.is_ok()
    }

    fn remove_function(&self, function_id: &str) {
        self.functions.remove(function_id);
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

                Ok(())
            }
            Message::UnregisterTrigger { id, trigger_type } => {
                tracing::debug!(
                    trigger_id = %id,
                    trigger_type = %trigger_type,
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
                    worker_id = %worker.id,
                    function_id = %function_id,
                    invocation_id = ?invocation_id,
                    otel.kind = "server"
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
            Message::RegisterFunction {
                id,
                description,
                request_format: req,
                response_format: res,
                metadata,
            } => {
                tracing::debug!(
                    worker_id = %worker.id,
                    function_id = %id,
                    description = ?description,
                    "RegisterFunction"
                );

                self.service_registry.register_service_from_func_path(id);

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

        for trigger in triggers {
            let engine = self.clone();
            let function_id = trigger.function_id.clone();
            let data = data.clone();
            tokio::spawn(async move {
                let _ = engine.call(&function_id, data).await;
            });
        }
    }

    pub async fn handle_worker(&self, socket: WebSocket, peer: SocketAddr) -> anyhow::Result<()> {
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

        while let Some(frame) = ws_rx.next().await {
            match frame {
                Ok(WsMessage::Text(text)) => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Message>(&text) {
                        Ok(msg) => self.router_msg(&worker, &msg).await?,
                        Err(err) => tracing::warn!(peer = %peer, error = ?err, "json decode error"),
                    }
                }
                Ok(WsMessage::Binary(bytes)) => {
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
                Ok(WsMessage::Close(_)) => {
                    tracing::debug!(peer = %peer, "Worker disconnected");
                    break;
                }
                Ok(WsMessage::Ping(payload)) => {
                    let _ = tx.send(Outbound::Raw(WsMessage::Pong(payload))).await;
                }
                Ok(WsMessage::Pong(_)) => {}
                Err(_err) => {
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
        let worker_functions = worker
            .function_ids
            .read()
            .await
            .iter()
            .cloned()
            .collect::<Vec<String>>();

        tracing::debug!(worker_id = %worker.id, functions = ?worker_functions, "Worker registered functions");
        for function_id in worker_functions.iter() {
            self.remove_function(function_id);
            self.service_registry
                .remove_function_from_services(function_id);
        }

        let worker_invocations = worker.invocations.read().await;
        tracing::debug!(worker_id = %worker.id, invocations = ?worker_invocations, "Worker invocations");
        for invocation_id in worker_invocations.iter() {
            tracing::debug!(invocation_id = %invocation_id, "Halting invocation");
            self.invocations.halt_invocation(invocation_id);
        }

        self.trigger_registry.unregister_worker(&worker.id).await;
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
    async fn call(&self, function_id: &str, input: Value) -> Result<Option<Value>, ErrorBody> {
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
