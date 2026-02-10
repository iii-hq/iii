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
                    trigger_type: trigger.trigger_type,
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
            self.invocations
                .write()
                .await
                .insert(invocation_id.unwrap());

            // Inject trace context and baggage from the captured OTel context
            let traceparent = inject_traceparent_from_context(&otel_context);
            let baggage = inject_baggage_from_context(&otel_context);

            let _ = self
                .channel
                .send(Outbound::Protocol(Message::InvokeFunction {
                    invocation_id,
                    function_id,
                    data: input,
                    traceparent,
                    baggage,
                }))
                .await
                .map_err(|err| ErrorBody {
                    code: "channel_send_failed".into(),
                    message: err.to_string(),
                });

            FunctionResult::Deferred
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::Worker;
    use serde_json::json;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_worker_register_trigger() {
        let (tx, mut rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(tx);

        let trigger = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({"schedule": "0 0 * * *"}),
            worker_id: None,
        };

        let result = worker.register_trigger(trigger.clone()).await;
        assert!(result.is_ok());

        let message = rx.recv().await.unwrap();
        match message {
            Outbound::Protocol(Message::RegisterTrigger {
                id,
                trigger_type,
                function_id,
                config: _,
            }) => {
                assert_eq!(id, "trigger-1");
                assert_eq!(trigger_type, "cron");
                assert_eq!(function_id, "test.function");
            }
            _ => panic!("Expected RegisterTrigger message"),
        }
    }

    #[tokio::test]
    async fn test_worker_unregister_trigger() {
        let (tx, mut rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(tx);

        let trigger = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({}),
            worker_id: None,
        };

        let result = worker.unregister_trigger(trigger.clone()).await;
        assert!(result.is_ok());

        let message = rx.recv().await.unwrap();
        match message {
            Outbound::Protocol(Message::UnregisterTrigger { id, trigger_type }) => {
                assert_eq!(id, "trigger-1");
                assert_eq!(trigger_type, "cron");
            }
            _ => panic!("Expected UnregisterTrigger message"),
        }
    }

    #[tokio::test]
    async fn test_worker_register_trigger_channel_error() {
        let (tx, _rx) = mpsc::channel::<Outbound>(1);
        drop(_rx);
        let worker = Worker::new(tx);

        let trigger = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({}),
            worker_id: None,
        };

        let result = worker.register_trigger(trigger).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("failed to send register trigger message"));
    }

    #[tokio::test]
    async fn test_worker_unregister_trigger_channel_error() {
        let (tx, _rx) = mpsc::channel::<Outbound>(1);
        drop(_rx);
        let worker = Worker::new(tx);

        let trigger = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({}),
            worker_id: None,
        };

        let result = worker.unregister_trigger(trigger).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("failed to send unregister trigger message"));
    }

    #[tokio::test]
    async fn test_worker_handle_function() {
        let (tx, mut rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(tx);

        let invocation_id = Uuid::new_v4();
        let function_id = "test.function".to_string();
        let input = json!({"key": "value"});

        let result = worker
            .handle_function(Some(invocation_id), function_id.clone(), input.clone())
            .await;

        match result {
            FunctionResult::Deferred => {}
            _ => panic!("Expected Deferred result"),
        }

        assert_eq!(worker.invocation_count().await, 1);
        assert!(worker
            .invocations
            .read()
            .await
            .contains(&invocation_id));

        let message = rx.recv().await.unwrap();
        match message {
            Outbound::Protocol(Message::InvokeFunction {
                invocation_id: id,
                function_id: fid,
                data,
                traceparent: _,
                baggage: _,
            }) => {
                assert_eq!(id, Some(invocation_id));
                assert_eq!(fid, function_id);
                assert_eq!(data, input);
            }
            _ => panic!("Expected InvokeFunction message"),
        }
    }

    #[tokio::test]
    async fn test_worker_handle_function_without_invocation_id() {
        let (tx, mut rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(tx);

        let function_id = "test.function".to_string();
        let input = json!({"key": "value"});
        let generated_id = Uuid::new_v4();

        let result = worker
            .handle_function(Some(generated_id), function_id.clone(), input.clone())
            .await;

        match result {
            FunctionResult::Deferred => {}
            _ => panic!("Expected Deferred result"),
        }

        let message = rx.recv().await.unwrap();
        match message {
            Outbound::Protocol(Message::InvokeFunction {
                invocation_id: id,
                function_id: fid,
                data: _,
                traceparent: _,
                baggage: _,
            }) => {
                assert_eq!(id, Some(generated_id));
                assert_eq!(fid, function_id);
            }
            _ => panic!("Expected InvokeFunction message"),
        }
    }

    #[tokio::test]
    async fn test_worker_handle_function_channel_error() {
        let (tx, _rx) = mpsc::channel::<Outbound>(1);
        drop(_rx);
        let worker = Worker::new(tx);

        let invocation_id = Uuid::new_v4();
        let function_id = "test.function".to_string();
        let input = json!({"key": "value"});

        let result = worker
            .handle_function(Some(invocation_id), function_id, input)
            .await;

        match result {
            FunctionResult::Deferred => {}
            _ => panic!("Expected Deferred result even on channel error"),
        }
    }
}
