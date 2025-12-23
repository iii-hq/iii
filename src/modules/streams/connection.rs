use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use crate::{
    engine::{Engine, EngineTrait},
    modules::streams::{
        StreamIncomingMessage, StreamOutboundMessage, StreamWrapperMessage, Subscription,
        adapters::{StreamAdapter, StreamConnection},
        structs::{
            StreamAuthContext, StreamIncomingMessageData, StreamJoinLeaveEvent, StreamJoinResult,
            StreamOutbound,
        },
        trigger::{JOIN_TRIGGER_TYPE, LEAVE_TRIGGER_TYPE, StreamTriggers},
    },
};

pub struct SocketStreamConnection {
    pub id: String,
    pub sender: mpsc::Sender<StreamOutbound>,
    pub triggers: Arc<StreamTriggers>,
    subscriptions: Arc<RwLock<DashMap<String, Subscription>>>,
    adapter: Arc<dyn StreamAdapter>,
    context: Option<StreamAuthContext>,
    engine: Arc<Engine>,
}

impl SocketStreamConnection {
    pub fn new(
        adapter: Arc<dyn StreamAdapter>,
        context: Option<StreamAuthContext>,
        sender: mpsc::Sender<StreamOutbound>,
        engine: Arc<Engine>,
        triggers: Arc<StreamTriggers>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            subscriptions: Arc::new(RwLock::new(DashMap::new())),
            sender,
            adapter,
            context,
            engine,
            triggers,
        }
    }

    pub async fn handle_join_leave(&self, message: &StreamIncomingMessage) -> StreamJoinResult {
        let (stream_name, group_id, id, subscription_id, event_type, triggers) = match message {
            StreamIncomingMessage::Join { data } => (
                data.stream_name.clone(),
                data.group_id.clone(),
                data.id.clone(),
                data.subscription_id.clone(),
                JOIN_TRIGGER_TYPE,
                self.triggers.join_triggers.read().await,
            ),
            StreamIncomingMessage::Leave { data } => (
                data.stream_name.clone(),
                data.group_id.clone(),
                data.id.clone(),
                data.subscription_id.clone(),
                LEAVE_TRIGGER_TYPE,
                self.triggers.leave_triggers.read().await,
            ),
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

        for trigger in triggers.iter() {
            if trigger.trigger_type == event_type {
                let trigger = trigger.clone();

                let call_result = self
                    .engine
                    .invoke_function(&trigger.function_path, event_value.clone())
                    .await;

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

                self.subscriptions.write().await.insert(
                    subscription_id.clone(),
                    Subscription {
                        subscription_id,
                        stream_name: stream_name.clone(),
                        group_id: group_id.clone(),
                        id: id.clone(),
                    },
                );

                if let Some(id) = id {
                    let data = self.adapter.get(&stream_name, &group_id, &id).await;

                    self.sender
                        .send(StreamOutbound::Stream(StreamWrapperMessage {
                            timestamp,
                            stream_name: stream_name.clone(),
                            group_id: group_id.clone(),
                            id: Some(id.clone()),
                            event: StreamOutboundMessage::Sync {
                                data: data.unwrap_or(Value::Null),
                            },
                        }))
                        .await?;
                } else {
                    let data = self.adapter.get_group(&stream_name, &group_id).await;
                    self.sender
                        .send(StreamOutbound::Stream(StreamWrapperMessage {
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

                Ok(())
            }
            StreamIncomingMessage::Leave { data } => {
                self.handle_join_leave(msg).await;
                self.subscriptions
                    .write()
                    .await
                    .remove(&data.subscription_id);
                Ok(())
            }
        }
    }
}

#[async_trait]
impl StreamConnection for SocketStreamConnection {
    async fn cleanup(&self) {
        let subscriptions = self.subscriptions.read().await;

        for subscription in subscriptions.iter() {
            let subscription = subscription.value();
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
        let subscriptions = self.subscriptions.read().await;
        tracing::debug!(msg = ?msg, "Sending stream message");

        for subscription in subscriptions.iter() {
            let subscription = subscription.value();

            if subscription.stream_name == msg.stream_name
                && subscription.group_id == msg.group_id
                && (subscription.id.is_none() || subscription.id == msg.id)
            {
                match self.sender.send(StreamOutbound::Stream(msg.clone())).await {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!(error = ?e.to_string(), "Failed to send stream message");
                    }
                }
            }
        }

        Ok(())
    }
}
