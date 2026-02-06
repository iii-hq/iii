// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    builtins::pubsub_lite::Subscriber,
    engine::{Engine, EngineTrait},
    function::FunctionResult,
    modules::streams::{
        StreamCoreModule, StreamIncomingMessage, StreamOutboundMessage, StreamWrapperMessage,
        Subscription,
        adapters::StreamConnection,
        structs::{
            StreamAuthContext, StreamListInput, StreamGetInput, StreamIncomingMessageData,
            StreamJoinLeaveEvent, StreamJoinResult, StreamOutbound,
        },
        trigger::{JOIN_TRIGGER_TYPE, LEAVE_TRIGGER_TYPE, StreamTriggers},
    },
};

pub struct SocketStreamConnection {
    pub id: String,
    pub sender: mpsc::Sender<StreamOutbound>,
    pub triggers: Arc<StreamTriggers>,
    subscriptions: Arc<DashMap<String, Subscription>>,
    stream_module: Arc<StreamCoreModule>,
    context: Option<StreamAuthContext>,
    engine: Arc<Engine>,
}

impl SocketStreamConnection {
    pub fn new(
        stream_module: Arc<StreamCoreModule>,
        context: Option<StreamAuthContext>,
        sender: mpsc::Sender<StreamOutbound>,
        engine: Arc<Engine>,
        triggers: Arc<StreamTriggers>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            subscriptions: Arc::new(DashMap::new()),
            sender,
            stream_module,
            context,
            engine,
            triggers,
        }
    }

    pub async fn handle_join_leave(&self, message: &StreamIncomingMessage) -> StreamJoinResult {
        let (stream_name, group_id, id, subscription_id, event_type, triggers) = match message {
            StreamIncomingMessage::Join { data } => {
                let triggers: Vec<_> = self
                    .triggers
                    .join_triggers
                    .read()
                    .await
                    .iter()
                    .cloned()
                    .collect();
                (
                    data.stream_name.clone(),
                    data.group_id.clone(),
                    data.id.clone(),
                    data.subscription_id.clone(),
                    JOIN_TRIGGER_TYPE,
                    triggers,
                )
            }
            StreamIncomingMessage::Leave { data } => {
                let triggers: Vec<_> = self
                    .triggers
                    .leave_triggers
                    .read()
                    .await
                    .iter()
                    .cloned()
                    .collect();
                (
                    data.stream_name.clone(),
                    data.group_id.clone(),
                    data.id.clone(),
                    data.subscription_id.clone(),
                    LEAVE_TRIGGER_TYPE,
                    triggers,
                )
            }
        };

        let event = StreamJoinLeaveEvent {
            subscription_id,
            stream_name,
            group_id,
            id,
            context: self
                .context
                .as_ref()
                .and_then(|context| context.context.clone()),
        };
        let mut result = StreamJoinResult {
            unauthorized: false,
        };

        let event_value = match serde_json::to_value(event) {
            Ok(event_value) => event_value,
            Err(e) => {
                tracing::error!(error = ?e, "Failed to convert event to value");
                return result;
            }
        };

        for trigger in triggers {
            if trigger.trigger_type == event_type {
                tracing::debug!(function_id = %trigger.function_id, event_type = ?event_type, "Invoking trigger");

                let call_result = self
                    .engine
                    .call(&trigger.function_id, event_value.clone())
                    .await;

                tracing::debug!(call_result = ?call_result, "Call result");

                match call_result {
                    Ok(Some(call_result)) => {
                        if event_type == JOIN_TRIGGER_TYPE {
                            let unauthorized =
                                match call_result.get("unauthorized").and_then(|v| v.as_bool()) {
                                    Some(unauthorized) => unauthorized,
                                    None => {
                                        if event_type == JOIN_TRIGGER_TYPE {
                                            tracing::error!(
                                                error = "unauthorized must be a boolean",
                                                "Failed to get unauthorized from result"
                                            );
                                        }
                                        false
                                    }
                                };

                            if unauthorized {
                                result = StreamJoinResult { unauthorized: true };
                            }
                        }
                    }
                    Err(e) => {
                        if event_type == JOIN_TRIGGER_TYPE {
                            tracing::error!(error = ?e, "Failed to invoke trigger function");
                        }
                    }
                    _ => {}
                }
            }
        }

        result
    }

    pub async fn handle_socket_message(&self, msg: &StreamIncomingMessage) -> anyhow::Result<()> {
        match msg {
            StreamIncomingMessage::Join { data } => {
                let stream_name = data.stream_name.clone();
                let group_id = data.group_id.clone();
                let id = data.id.clone();
                let subscription_id = data.subscription_id.clone();
                let result = self.handle_join_leave(msg).await;
                let timestamp = chrono::Utc::now().timestamp_millis();

                if result.unauthorized {
                    let event = StreamOutboundMessage::Unauthorized {};
                    let message = StreamWrapperMessage {
                        event_type: "stream".to_string(),
                        timestamp,
                        stream_name,
                        group_id,
                        id,
                        event,
                    };
                    let outbound = StreamOutbound::Stream(message);
                    self.sender.send(outbound).await?;

                    return Ok(());
                }

                self.subscriptions.insert(
                    subscription_id.clone(),
                    Subscription {
                        subscription_id,
                        stream_name: stream_name.clone(),
                        group_id: group_id.clone(),
                        id: id.clone(),
                    },
                );

                if let Some(id) = id {
                    let data = self
                        .stream_module
                        .get(StreamGetInput {
                            stream_name: stream_name.clone(),
                            group_id: group_id.clone(),
                            item_id: id.clone(),
                        })
                        .await;

                    match data {
                        FunctionResult::Success(data) => {
                            self.sender
                                .send(StreamOutbound::Stream(StreamWrapperMessage {
                                    event_type: "stream".to_string(),
                                    timestamp,
                                    stream_name: stream_name.clone(),
                                    group_id: group_id.clone(),
                                    id: Some(id.clone()),
                                    event: StreamOutboundMessage::Sync {
                                        data: data.unwrap_or(Value::Null),
                                    },
                                }))
                                .await?;
                        }
                        FunctionResult::Failure(error) => {
                            tracing::error!(error = ?error, "Failed to get data");
                        }
                        FunctionResult::Deferred => {
                            tracing::error!(error = "Deferred result", "Failed to get data");
                        }
                        FunctionResult::NoResult => {
                            tracing::error!("No result");
                        }
                    }

                    return Ok(());
                } else {
                    let data = self
                        .stream_module
                        .list(StreamListInput {
                            stream_name: stream_name.clone(),
                            group_id: group_id.clone(),
                        })
                        .await;

                    match data {
                        FunctionResult::Success(data) => {
                            self.sender
                                .send(StreamOutbound::Stream(StreamWrapperMessage {
                                    event_type: "stream".to_string(),
                                    timestamp,
                                    stream_name: stream_name.clone(),
                                    group_id: group_id.clone(),
                                    id: None,
                                    event: StreamOutboundMessage::Sync {
                                        data: serde_json::to_value(data).unwrap_or(Value::Null),
                                    },
                                }))
                                .await?;
                        }
                        FunctionResult::Failure(error) => {
                            tracing::error!(error = ?error, "Failed to get data");
                        }
                        FunctionResult::Deferred => {
                            tracing::error!(error = "Deferred result", "Failed to get data");
                        }
                        FunctionResult::NoResult => {
                            tracing::error!("No result");
                        }
                    }
                }

                Ok(())
            }
            StreamIncomingMessage::Leave { data } => {
                self.handle_join_leave(msg).await;
                self.subscriptions.remove(&data.subscription_id);
                Ok(())
            }
        }
    }
}

#[async_trait]
impl StreamConnection for SocketStreamConnection {
    async fn cleanup(&self) {
        let subscriptions: Vec<Subscription> = self
            .subscriptions
            .iter()
            .map(|entry| {
                let subscription = entry.value();
                Subscription {
                    subscription_id: subscription.subscription_id.clone(),
                    stream_name: subscription.stream_name.clone(),
                    group_id: subscription.group_id.clone(),
                    id: subscription.id.clone(),
                }
            })
            .collect();

        for subscription in subscriptions {
            tracing::debug!(subscription_id = %subscription.subscription_id, "Cleaning up subscription");

            let _ = self
                .handle_join_leave(&StreamIncomingMessage::Leave {
                    data: StreamIncomingMessageData {
                        subscription_id: subscription.subscription_id.clone(),
                        stream_name: subscription.stream_name.clone(),
                        group_id: subscription.group_id.clone(),
                        id: subscription.id.clone(),
                    },
                })
                .await;
        }
    }

    async fn handle_stream_message(&self, msg: &StreamWrapperMessage) -> anyhow::Result<()> {
        tracing::debug!(msg = ?msg, "Sending stream message");

        let matching_senders: Vec<_> = self
            .subscriptions
            .iter()
            .filter(|entry| {
                let subscription = entry.value();
                subscription.stream_name == msg.stream_name
                    && subscription.group_id == msg.group_id
                    && (subscription.id.is_none() || subscription.id == msg.id)
            })
            .map(|_| self.sender.clone())
            .collect();

        for sender in matching_senders {
            if let Err(e) = sender.send(StreamOutbound::Stream(msg.clone())).await {
                tracing::error!(error = ?e.to_string(), "Failed to send stream message");
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Subscriber for SocketStreamConnection {
    async fn handle_message(&self, message: Arc<Value>) -> anyhow::Result<()> {
        let message = match serde_json::from_value::<StreamWrapperMessage>((*message).clone()) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!(error = ?e, "Failed to deserialize stream message");
                return Err(anyhow::anyhow!("Failed to deserialize stream message"));
            }
        };

        self.handle_stream_message(&message).await
    }
}
