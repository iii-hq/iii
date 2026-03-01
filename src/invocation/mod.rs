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
    protocol::ErrorBody,
};

pub mod auth;
pub mod http_function;
pub mod http_invoker;
pub mod method;
pub mod signature;
pub mod url_validator;

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
}
impl InvocationHandler {
    pub fn new() -> Self {
        Self {
            invocations: Arc::new(DashMap::new()),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invocation_handler_new() {
        let handler = InvocationHandler::new();
        assert!(handler.invocations.is_empty());
    }

    #[test]
    fn test_invocation_handler_default() {
        let handler = InvocationHandler::default();
        assert!(handler.invocations.is_empty());
    }

    #[test]
    fn test_invocation_handler_remove_nonexistent() {
        let handler = InvocationHandler::new();
        let id = Uuid::new_v4();
        let result = handler.remove(&id);
        assert!(result.is_none());
    }

    #[test]
    fn test_invocation_handler_remove_existing() {
        let handler = InvocationHandler::new();
        let id = Uuid::new_v4();
        let (sender, _receiver) = oneshot::channel();

        let invocation = Invocation {
            id,
            function_id: "test_fn".to_string(),
            worker_id: None,
            sender,
            traceparent: None,
            baggage: None,
        };
        handler.invocations.insert(id, invocation);

        let removed = handler.remove(&id);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, id);
        assert!(handler.invocations.is_empty());
    }

    #[test]
    fn test_invocation_handler_halt_invocation_sends_error() {
        let handler = InvocationHandler::new();
        let id = Uuid::new_v4();
        let (sender, mut receiver) = oneshot::channel();

        let invocation = Invocation {
            id,
            function_id: "test_fn".to_string(),
            worker_id: None,
            sender,
            traceparent: None,
            baggage: None,
        };
        handler.invocations.insert(id, invocation);

        handler.halt_invocation(&id);

        // The invocation should have been removed.
        assert!(handler.invocations.is_empty());

        // The receiver should get an error result.
        let result = (&mut receiver).try_recv();
        assert!(result.is_ok());
        let inner = result.unwrap();
        assert!(inner.is_err());
        let error = inner.unwrap_err();
        assert_eq!(error.code, "invocation_stopped");
        assert_eq!(error.message, "Invocation stopped");
    }

    #[test]
    fn test_invocation_handler_halt_nonexistent_is_noop() {
        let handler = InvocationHandler::new();
        let id = Uuid::new_v4();
        // Should not panic.
        handler.halt_invocation(&id);
    }

    #[test]
    fn test_invocation_handler_multiple_invocations() {
        let handler = InvocationHandler::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let (sender1, _rx1) = oneshot::channel();
        let (sender2, _rx2) = oneshot::channel();

        handler.invocations.insert(
            id1,
            Invocation {
                id: id1,
                function_id: "fn1".to_string(),
                worker_id: None,
                sender: sender1,
                traceparent: None,
                baggage: None,
            },
        );
        handler.invocations.insert(
            id2,
            Invocation {
                id: id2,
                function_id: "fn2".to_string(),
                worker_id: Some(Uuid::new_v4()),
                sender: sender2,
                traceparent: Some("00-trace-id".to_string()),
                baggage: Some("key=value".to_string()),
            },
        );

        assert_eq!(handler.invocations.len(), 2);

        let removed = handler.remove(&id1);
        assert!(removed.is_some());
        assert_eq!(handler.invocations.len(), 1);

        let removed = handler.remove(&id2);
        assert!(removed.is_some());
        assert!(handler.invocations.is_empty());
    }
}
