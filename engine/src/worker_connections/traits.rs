// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::pin::Pin;

use futures::Future;
use serde_json::Value;
use tokio::sync::oneshot;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;

use crate::{
    engine::Outbound,
    function::{FunctionHandler, FunctionResult},
    protocol::{ErrorBody, Message},
    telemetry::{inject_baggage_from_context, inject_traceparent_from_context},
    trigger::{RegistratorUnavailable, Trigger, TriggerRegistrator},
    worker_connections::WorkerConnection,
};

/// How long a function-path registration waits for the registrator worker's
/// `TriggerRegistrationResult` before failing open (legacy / stalled workers
/// are still covered by the late-unwind path in `router_msg`).
const REGISTRATION_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

impl TriggerRegistrator for WorkerConnection {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let sender = self.channel.clone();
        let acks = self.pending_trigger_acks.clone();

        Box::pin(async move {
            // Ownerless registrations come from the `engine::register_trigger`
            // FUNCTION, whose invocation runs in a spawned task — awaiting the
            // worker's ack there is safe and makes validation failures
            // synchronous (no id-then-unwind). Connection-owned registrations
            // (`Message::RegisterTrigger`, a worker's own lifecycle bindings)
            // are processed INLINE in the origin connection's read loop; a
            // worker binding its own trigger type would deadlock on its ack,
            // so those keep fire-and-forget + late unwind.
            let await_ack = trigger.worker_id.is_none();
            let trigger_id = trigger.id.clone();
            let rx = if await_ack {
                let (tx, rx) = oneshot::channel();
                acks.insert(trigger_id.clone(), tx);
                Some(rx)
            } else {
                None
            };

            let sent = sender
                .send(Outbound::Protocol(Message::RegisterTrigger {
                    id: trigger.id,
                    trigger_type: trigger.trigger_type,
                    function_id: trigger.function_id,
                    config: trigger.config,
                    metadata: trigger.metadata,
                }))
                .await;
            if let Err(err) = sent {
                acks.remove(&trigger_id);
                return Err(anyhow::Error::new(RegistratorUnavailable).context(format!(
                    "failed to send register trigger message through worker channel: {err}"
                )));
            }

            let Some(rx) = rx else { return Ok(()) };
            match tokio::time::timeout(REGISTRATION_ACK_TIMEOUT, rx).await {
                Ok(Ok(None)) => Ok(()),
                Ok(Ok(Some(err))) => Err(anyhow::anyhow!("{}: {}", err.code, err.message)),
                // Ack channel dropped (connection teardown) — fail open; the
                // disconnect GC / late-unwind own the cleanup.
                Ok(Err(_)) => Ok(()),
                Err(_elapsed) => {
                    acks.remove(&trigger_id);
                    tracing::debug!(
                        trigger_id = %trigger_id,
                        "no TriggerRegistrationResult within timeout; accepting registration (late unwind still applies)"
                    );
                    Ok(())
                }
            }
        })
    }

    /// Fire-and-forget re-delivery. Replay/recovery runs inline on THIS
    /// worker's read loop (`router_msg` handling its `RegisterTriggerType`),
    /// so awaiting the ack like `register_trigger` does for ownerless
    /// triggers would stall the connection until the timeout — the ack can
    /// only be read once the loop is free again. Failures are still handled:
    /// the worker's `TriggerRegistrationResult` takes the late-unwind path in
    /// `router_msg`, which re-parks the trigger as a pending intent.
    fn replay_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let sender = self.channel.clone();

        Box::pin(async move {
            sender
                .send(Outbound::Protocol(Message::RegisterTrigger {
                    id: trigger.id,
                    trigger_type: trigger.trigger_type,
                    function_id: trigger.function_id,
                    config: trigger.config,
                    metadata: trigger.metadata,
                }))
                .await
                .map_err(|err| {
                    anyhow::anyhow!(
                        "failed to send register trigger message through worker channel: {}",
                        err
                    )
                })?;

            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let sender = self.channel.clone();

        Box::pin(async move {
            sender
                .send(Outbound::Protocol(Message::UnregisterTrigger {
                    id: trigger.id,
                    trigger_type: Some(trigger.trigger_type),
                }))
                .await
                .map_err(|err| {
                    anyhow::anyhow!(
                        "failed to send unregister trigger message through worker channel: {}",
                        err
                    )
                })?;

            Ok(())
        })
    }
}

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

            let send_result = self
                .channel
                .send(Outbound::Protocol(Message::InvokeFunction {
                    invocation_id,
                    function_id,
                    data: input,
                    traceparent,
                    baggage,
                    action: None,
                    metadata,
                    namespace: None,
                }))
                .await;

            match send_result {
                Ok(_) => {
                    let id = invocation_id.unwrap_or_else(Uuid::new_v4);
                    self.invocations.write().await.insert(id);
                    FunctionResult::Deferred
                }
                Err(err) => FunctionResult::Failure(ErrorBody {
                    code: "channel_send_failed".into(),
                    message: err.to_string(),
                    stacktrace: None,
                }),
            }
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

    fn trigger(id: &str, worker_id: Option<Uuid>) -> Trigger {
        Trigger {
            id: id.into(),
            trigger_type: "http".into(),
            function_id: "fn1".into(),
            config: json!({}),
            worker_id,
            metadata: None,
            namespace: "default".to_string(),
        }
    }

    /// Resolve the pending ack for `id` once the delivery message lands.
    async fn ack(
        worker: &WorkerConnection,
        rx: &mut mpsc::Receiver<Outbound>,
        id: &str,
        error: Option<ErrorBody>,
    ) {
        let msg = rx.recv().await.expect("RegisterTrigger delivered");
        assert!(matches!(
            msg,
            Outbound::Protocol(Message::RegisterTrigger { .. })
        ));
        let (_, tx) = worker
            .pending_trigger_acks
            .remove(id)
            .expect("pending ack entry");
        tx.send(error).unwrap();
    }

    #[tokio::test]
    async fn register_trigger_sends_message_and_accepts_on_ok_ack() {
        let (worker, mut rx) = make_worker();
        let (res, ()) = tokio::join!(
            worker.register_trigger(trigger("t1", None)),
            ack(&worker, &mut rx, "t1", None),
        );
        res.unwrap();
        assert!(worker.pending_trigger_acks.is_empty());
    }

    #[tokio::test]
    async fn ownerless_registration_rejects_synchronously_on_error_ack() {
        let (worker, mut rx) = make_worker();
        let (res, ()) = tokio::join!(
            worker.register_trigger(trigger("t_bad", None)),
            ack(
                &worker,
                &mut rx,
                "t_bad",
                Some(ErrorBody::new(
                    "trigger_registration_failed",
                    "unknown model"
                )),
            ),
        );
        let err = res.unwrap_err().to_string();
        assert!(err.contains("unknown model"), "got: {err}");
        assert!(worker.pending_trigger_acks.is_empty());
    }

    #[tokio::test]
    async fn connection_owned_registration_stays_fire_and_forget() {
        let (worker, mut rx) = make_worker();
        // A worker's own lifecycle binding: returns as soon as the delivery is
        // queued, never waits on (or creates) a pending ack.
        worker
            .register_trigger(trigger("t_owned", Some(Uuid::new_v4())))
            .await
            .unwrap();
        assert!(worker.pending_trigger_acks.is_empty());
        let msg = rx.recv().await.unwrap();
        assert!(matches!(
            msg,
            Outbound::Protocol(Message::RegisterTrigger { .. })
        ));
    }

    #[tokio::test]
    async fn replay_trigger_is_fire_and_forget_even_for_ownerless_triggers() {
        let (worker, mut rx) = make_worker();
        // Replay runs inline on this worker's read loop, so it must return as
        // soon as the delivery is queued — no pending ack, no timeout — even
        // for ownerless (durable) triggers that DO await an ack on the
        // register_trigger path.
        worker
            .replay_trigger(trigger("t_replayed", None))
            .await
            .unwrap();
        assert!(worker.pending_trigger_acks.is_empty());
        let msg = rx.recv().await.unwrap();
        assert!(matches!(
            msg,
            Outbound::Protocol(Message::RegisterTrigger { .. })
        ));
    }

    #[tokio::test]
    async fn unregister_trigger_sends_message() {
        let (worker, mut rx) = make_worker();
        let trigger = Trigger {
            id: "t2".into(),
            trigger_type: "cron".into(),
            function_id: "fn2".into(),
            config: json!({}),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
        };
        worker.unregister_trigger(trigger).await.unwrap();
        let msg = rx.recv().await.unwrap();
        match msg {
            Outbound::Protocol(Message::UnregisterTrigger { id, trigger_type }) => {
                assert_eq!(id, "t2");
                assert_eq!(trigger_type, Some("cron".into()));
            }
            _ => panic!("Expected UnregisterTrigger message"),
        }
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
