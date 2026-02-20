// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use dashmap::DashMap;
use opentelemetry::KeyValue;
use serde_json::Value;
use tokio::sync::oneshot::{self, error::RecvError};
use tracing::Instrument;
use uuid::Uuid;

use crate::{
    function::{Function, FunctionResult},
    modules::observability::metrics::get_engine_metrics,
    protocol::{ErrorBody, MiddlewareAction},
};

#[derive(Debug, Clone)]
pub struct MiddlewareInvocationResult {
    pub action: MiddlewareAction,
    pub request: Option<Value>,
    pub context: Option<Value>,
    pub response: Option<Value>,
}

pub struct Invocation {
    pub id: Uuid,
    pub function_id: String,
    pub worker_id: Option<Uuid>,
    pub sender: oneshot::Sender<Result<Option<Value>, ErrorBody>>,
    /// W3C traceparent for distributed tracing context
    pub traceparent: Option<String>,
    /// W3C baggage for cross-cutting context propagation
    pub baggage: Option<String>,
}

type Invocations = Arc<DashMap<Uuid, Invocation>>;

#[derive(Default)]
pub struct InvocationHandler {
    invocations: Invocations,
    pending_middleware:
        Arc<DashMap<Uuid, oneshot::Sender<Result<MiddlewareInvocationResult, ErrorBody>>>>,
}
impl InvocationHandler {
    pub fn new() -> Self {
        Self {
            invocations: Arc::new(DashMap::new()),
            pending_middleware: Arc::new(DashMap::new()),
        }
    }

    pub fn add_middleware_invocation(
        &self,
        id: Uuid,
        sender: oneshot::Sender<Result<MiddlewareInvocationResult, ErrorBody>>,
    ) {
        self.pending_middleware.insert(id, sender);
    }

    pub fn remove_middleware_invocation(
        &self,
        id: &Uuid,
    ) -> Option<oneshot::Sender<Result<MiddlewareInvocationResult, ErrorBody>>> {
        self.pending_middleware.remove(id).map(|(_, sender)| sender)
    }

    pub fn remove(&self, invocation_id: &Uuid) -> Option<Invocation> {
        self.invocations
            .remove(invocation_id)
            .map(|(_, sender)| sender)
    }

    pub fn halt_invocation(&self, invocation_id: &Uuid) {
        let invocation = self.remove(invocation_id);

        if let Some(invocation) = invocation {
            let _ = invocation.sender.send(Err(ErrorBody {
                code: "invocation_stopped".into(),
                message: "Invocation stopped".into(),
            }));
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn handle_invocation(
        &self,
        invocation_id: Option<Uuid>,
        worker_id: Option<Uuid>,
        function_id: String,
        body: Value,
        function_handler: Function,
        traceparent: Option<String>,
        baggage: Option<String>,
    ) -> Result<Result<Option<Value>, ErrorBody>, RecvError> {
        // Create span with dynamic name using the function_id
        // Using OTEL semantic conventions for FaaS (Function as a Service)
        let function_kind = if function_id.starts_with("engine::") {
            "internal"
        } else {
            "user"
        };

        let span = tracing::info_span!(
            "call",
            otel.name = %format!("call {}", function_id),
            otel.kind = "server",
            otel.status_code = tracing::field::Empty,
            // FAAS semantic conventions (https://opentelemetry.io/docs/specs/semconv/faas/)
            "faas.invoked_name" = %function_id,
            "faas.trigger" = "other",  // III Engine uses its own invocation mechanism
            // Keep function_id for backward compatibility
            function_id = %function_id,
            // Tag internal vs user functions for filtering
            "iii.function.kind" = %function_kind,
        );

        async {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let invocation_id = invocation_id.unwrap_or(Uuid::new_v4());
            let invocation = Invocation {
                id: invocation_id,
                function_id: function_id.clone(),
                worker_id,
                sender,
                traceparent,
                baggage,
            };

            // Start timer for invocation duration
            let start_time = std::time::Instant::now();
            let metrics = get_engine_metrics();

            let result = function_handler
                .call_handler(Some(invocation_id), body)
                .await;

            // Calculate duration
            let duration = start_time.elapsed().as_secs_f64();

            match result {
                FunctionResult::Success(result) => {
                    tracing::debug!(invocation_id = %invocation_id, function_id = %function_id, "Function completed successfully");
                    tracing::Span::current().record("otel.status_code", "OK");

                    // Record metrics
                    metrics.invocations_total.add(
                        1,
                        &[
                            KeyValue::new("function_id", function_id.clone()),
                            KeyValue::new("status", "ok"),
                        ],
                    );
                    metrics.invocation_duration.record(
                        duration,
                        &[
                            KeyValue::new("function_id", function_id.clone()),
                            KeyValue::new("status", "ok"),
                        ],
                    );

                    // Update accumulator for readable metrics
                    let acc = crate::modules::observability::metrics::get_metrics_accumulator();
                    acc.invocations_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.invocations_success.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.increment_function(&function_id);

                    let _ = invocation.sender.send(Ok(result));
                }
                FunctionResult::Failure(error) => {
                    tracing::debug!(invocation_id = %invocation_id, function_id = %function_id, error_code = %error.code, "Function failed: {}", error.message);
                    tracing::Span::current().record("otel.status_code", "ERROR");

                    // Record metrics
                    metrics.invocations_total.add(
                        1,
                        &[
                            KeyValue::new("function_id", function_id.clone()),
                            KeyValue::new("status", "error"),
                        ],
                    );
                    metrics.invocation_duration.record(
                        duration,
                        &[
                            KeyValue::new("function_id", function_id.clone()),
                            KeyValue::new("status", "error"),
                        ],
                    );
                    metrics.invocation_errors_total.add(
                        1,
                        &[
                            KeyValue::new("function_id", function_id.clone()),
                            KeyValue::new("error_code", error.code.clone()),
                        ],
                    );

                    // Update accumulator for readable metrics
                    let acc = crate::modules::observability::metrics::get_metrics_accumulator();
                    acc.invocations_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.invocations_error.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.increment_function(&function_id);

                    let _ = invocation.sender.send(Err(error));
                }
                FunctionResult::NoResult => {
                    tracing::debug!(invocation_id = %invocation_id, function_id = %function_id, "Function no result");
                    tracing::Span::current().record("otel.status_code", "OK");

                    // Record metrics
                    metrics.invocations_total.add(
                        1,
                        &[
                            KeyValue::new("function_id", function_id.clone()),
                            KeyValue::new("status", "ok"),
                        ],
                    );
                    metrics.invocation_duration.record(
                        duration,
                        &[
                            KeyValue::new("function_id", function_id.clone()),
                            KeyValue::new("status", "ok"),
                        ],
                    );

                    // Update accumulator for readable metrics
                    let acc = crate::modules::observability::metrics::get_metrics_accumulator();
                    acc.invocations_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.invocations_success.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.increment_function(&function_id);

                    let _ = invocation.sender.send(Ok(None));
                }
                FunctionResult::Deferred => {
                    tracing::debug!(invocation_id = %invocation_id, function_id = %function_id, "Function deferred");

                    // Record metrics for deferred invocations
                    metrics.invocations_total.add(
                        1,
                        &[
                            KeyValue::new("function_id", function_id.clone()),
                            KeyValue::new("status", "deferred"),
                        ],
                    );
                    metrics.invocation_duration.record(
                        duration,
                        &[
                            KeyValue::new("function_id", function_id.clone()),
                            KeyValue::new("status", "deferred"),
                        ],
                    );

                    // Update accumulator for readable metrics
                    let acc = crate::modules::observability::metrics::get_metrics_accumulator();
                    acc.invocations_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.invocations_deferred.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.increment_function(&function_id);

                    // Deferred invocations will have their status set when the result comes back
                    // we need to store the invocation because it's a worker invocation
                    self.invocations.insert(invocation_id, invocation);
                }
            }

            let result = receiver.await;
            match &result {
                Ok(Ok(_)) => {
                    tracing::Span::current().record("otel.status_code", "OK");
                }
                Ok(Err(_)) | Err(_) => {
                    tracing::Span::current().record("otel.status_code", "ERROR");
                }
            };
            result
        }
        .instrument(span)
        .await
    }
}
