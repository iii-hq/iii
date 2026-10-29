// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::pin::Pin;

use futures::Future;
use serde_json::Value;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;

use crate::{
    engine::Outbound,
    function::{FunctionHandler, FunctionResult},
    protocol::{ErrorBody, Message},
    telemetry::{inject_baggage_from_context, inject_traceparent_from_context},
    worker_connections::WorkerConnection,
};

impl FunctionHandler for WorkerConnection {
    fn handle_function<'a>(
        &'a self,
        invocation_id: Option<Uuid>,
        function_id: String,
        input: Value,
        metadata: Option<Value>,
    ) -> Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'a>> {
        // Capture OTel context from current tracing span BEFORE async move
        // This ensures we get the trace context from the #[tracing::instrument] span.
        let current_span = Span::current();
        let span_cx = current_span.context();
        // Fall back to the ambient OTel context when the active tracing span
        // carries no valid span context — e.g. the engine intentionally
        // suppressed its per-invocation `call` span for this worker-routed call
        // (see `invocation/mod.rs`). Without this the worker invocation would
        // inject an empty traceparent and start a new, orphaned trace instead of
        // nesting under the caller. Every existing flow has a valid tracing
        // span here, so this is a no-op for them.
        let otel_context = if inject_traceparent_from_context(&span_cx).is_some() {
            span_cx
        } else {
            opentelemetry::Context::current()
        };

        Box::pin(async move {
            let traceparent = inject_traceparent_from_context(&otel_context);
            let baggage = inject_baggage_from_context(&otel_context);

            let function_id = if let Some(session) = &self.session {
                if let Some(prefix) = &session.function_registration_prefix {
                    let needle = format!("{prefix}::");
                    function_id
                        .strip_prefix(&needle)
                        .map(String::from)
                        .unwrap_or(function_id)
                } else {
                    function_id
                }
            } else {
                function_id
            };

            let stopped = || {
                FunctionResult::Failure(ErrorBody {
                    code: "invocation_stopped".into(),
                    message: "Invocation stopped".into(),
                    stacktrace: None,
                })
            };
            if self
                .cleanup_claimed
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return stopped();
            }

            // Reserve before tracking so cancellation while waiting for capacity
            // cannot leave a worker-local ID behind.
            let mut cleanup_done = self.cleanup_done.subscribe();
            let permit = tokio::select! {
                biased;
                _ = cleanup_done.wait_for(|done| *done) => return stopped(),
                result = self.channel.reserve() => match result {
                    Ok(permit) => permit,
                    Err(err) => return FunctionResult::Failure(ErrorBody {
                        code: "channel_send_failed".into(),
                        message: err.to_string(),
                        stacktrace: None,
                    }),
                },
            };

            let mut invocations = self.invocations.write().await;
            // A reserved permit can outlive channel closure. Serialize registration
            // with cleanup's drain, and reject it once cleanup has started.
            if self
                .cleanup_claimed
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return stopped();
            }
            if self.channel.is_closed() {
                return FunctionResult::Failure(ErrorBody {
                    code: "channel_send_failed".into(),
                    message: "channel closed".into(),
                    stacktrace: None,
                });
            }
            if let Some(id) = invocation_id {
                invocations.insert(id);
            }
            // The caller, depth and activation of this invocation (D9): only a
            // connection that declared `observe.v1` gets the new fields. The
            // engine keeps the depth for what the connection does meanwhile
            // without saying (see `WorkerConnection::inflight_depth`).
            let current = crate::observer::InvocationCtx::current();
            if let (Some(ctx), Some(id)) = (&current, invocation_id)
                && ctx.observe_depth > 0
            {
                self.remember_inflight_depth(id, ctx.observe_depth);
            }
            let (caller, observe_depth, activation) = match current {
                Some(ctx) if self.features.observe_v1() => (
                    ctx.caller().cloned().map(Box::new),
                    (ctx.observe_depth > 0).then_some(ctx.observe_depth),
                    ctx.activation.clone(),
                ),
                _ => (None, None, None),
            };
            permit.send(Outbound::Protocol(Message::InvokeFunction {
                invocation_id,
                function_id,
                data: input,
                traceparent,
                baggage,
                action: None,
                metadata,
                namespace: None,
                caller,
                observe_depth,
                activation,
            }));
            FunctionResult::Deferred
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::sync::mpsc;

    fn make_worker() -> (WorkerConnection, mpsc::Receiver<Outbound>) {
        let (tx, rx) = mpsc::channel(16);
        (WorkerConnection::new(tx), rx)
    }

    #[tokio::test]
    async fn handle_function_returns_deferred() {
        let (worker, mut rx) = make_worker();
        let inv_id = Uuid::new_v4();
        let result = worker
            .handle_function(
                Some(inv_id),
                "fn1".into(),
                json!({"key": "val"}),
                Some(json!({"session_id": "s_1"})),
            )
            .await;
        assert!(matches!(result, FunctionResult::Deferred));

        let msg = rx.recv().await.unwrap();
        match msg {
            Outbound::Protocol(Message::InvokeFunction {
                invocation_id,
                function_id,
                data,
                metadata,
                ..
            }) => {
                assert_eq!(invocation_id, Some(inv_id));
                assert_eq!(function_id, "fn1");
                assert_eq!(data, json!({"key": "val"}));
                // Metadata rides as a distinct field on the wire message, not
                // folded into `data`.
                assert_eq!(metadata, Some(json!({"session_id": "s_1"})));
            }
            _ => panic!("Expected InvokeFunction message"),
        }

        // Verify invocation was tracked
        assert!(worker.invocations.read().await.contains(&inv_id));
    }
}
