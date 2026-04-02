// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
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
    trigger::{Trigger, TriggerRegistrator},
    workers::Worker,
};

impl TriggerRegistrator for Worker {
    fn register_trigger(
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

impl FunctionHandler for Worker {
    fn handle_function<'a>(
        &'a self,
        invocation_id: Option<Uuid>,
        function_id: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'a>> {
        // Capture OTel context from current tracing span BEFORE async move
        // This ensures we get the trace context from the #[tracing::instrument] span
        let current_span = Span::current();
        let otel_context = current_span.context();

        Box::pin(async move {
            let traceparent = inject_traceparent_from_context(&otel_context);
            let baggage = inject_baggage_from_context(&otel_context);

            let send_result = self
                .channel
                .send(Outbound::Protocol(Message::InvokeFunction {
                    invocation_id,
                    function_id,
                    data: input,
                    traceparent,
                    baggage,
                    action: None,
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

    fn make_worker() -> (Worker, mpsc::Receiver<Outbound>) {
        let (tx, rx) = mpsc::channel(16);
        (Worker::new(tx), rx)
    }

    #[tokio::test]
    async fn register_trigger_sends_message() {
        let (worker, mut rx) = make_worker();
        let trigger = Trigger {
            id: "t1".into(),
            trigger_type: "http".into(),
            function_id: "fn1".into(),
            config: json!({}),
            worker_id: None,
        };
        worker.register_trigger(trigger).await.unwrap();
        let msg = rx.recv().await.unwrap();
        match msg {
            Outbound::Protocol(Message::RegisterTrigger {
                id,
                trigger_type,
                function_id,
                ..
            }) => {
                assert_eq!(id, "t1");
                assert_eq!(trigger_type, "http");
                assert_eq!(function_id, "fn1");
            }
            _ => panic!("Expected RegisterTrigger message"),
        }
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
            .handle_function(Some(inv_id), "fn1".into(), json!({"key": "val"}))
            .await;
        assert!(matches!(result, FunctionResult::Deferred));

        let msg = rx.recv().await.unwrap();
        match msg {
            Outbound::Protocol(Message::InvokeFunction {
                invocation_id,
                function_id,
                data,
                ..
            }) => {
                assert_eq!(invocation_id, Some(inv_id));
                assert_eq!(function_id, "fn1");
                assert_eq!(data, json!({"key": "val"}));
            }
            _ => panic!("Expected InvokeFunction message"),
        }

        // Verify invocation was tracked
        assert!(worker.invocations.read().await.contains(&inv_id));
    }
}
