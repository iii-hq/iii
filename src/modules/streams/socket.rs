use std::{net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use axum::extract::ws::{Message as WsMessage, WebSocket};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use crate::modules::streams::{
    StreamIncomingMessage, StreamOutboundMessage, StreamWrapperMessage, Subscription,
    adapters::{StreamAdapter, StreamConnection},
    structs::StreamOutbound,
};

pub struct StreamSocketManager {
    adapter: Arc<dyn StreamAdapter>,
}

impl StreamSocketManager {
    pub fn new(adapter: Arc<dyn StreamAdapter>) -> Self {
        Self { adapter }
    }

    pub async fn socket_handler(&self, socket: WebSocket, _addr: SocketAddr) -> anyhow::Result<()> {
        let (mut ws_tx, mut ws_rx) = socket.split();
        let (tx, mut rx) = mpsc::channel::<StreamOutbound>(64);
        let connection = SocketStreamConnection::new(self.adapter.clone(), tx);

        let writer = tokio::spawn(async move {
            while let Some(outbound) = rx.recv().await {
                let send_result = match outbound {
                    StreamOutbound::Stream(msg) => match serde_json::to_string(&msg) {
                        Ok(payload) => ws_tx.send(WsMessage::Text(payload)).await,
                        Err(err) => {
                            tracing::error!(error = ?err, "serialize error");
                            continue;
                        }
                    },
                    StreamOutbound::Raw(frame) => ws_tx.send(frame).await,
                };

                if send_result.is_err() {
                    break;
                }
            }
        });

        let connection_id = connection.id.clone();
        let connection = Arc::new(connection);

        self.adapter
            .subscribe(connection_id, connection.clone())
            .await;

        while let Some(frame) = ws_rx.next().await {
            match frame {
                Ok(WsMessage::Text(text)) => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<StreamIncomingMessage>(&text) {
                        Ok(msg) => connection.handle_socket_message(&msg).await?,
                        Err(err) => tracing::warn!(error = ?err, "json decode error"),
                    }
                }
                Ok(WsMessage::Binary(bytes)) => {
                    match serde_json::from_slice::<StreamIncomingMessage>(&bytes) {
                        Ok(msg) => connection.handle_socket_message(&msg).await?,
                        Err(err) => {
                            tracing::warn!(error = ?err, "binary decode error")
                        }
                    }
                }
                Ok(WsMessage::Close(_)) => {
                    tracing::debug!("Stream Websocket Connection closed");
                    break;
                }
                Ok(WsMessage::Ping(payload)) => {
                    let _ = connection
                        .sender
                        .send(StreamOutbound::Raw(WsMessage::Pong(payload)))
                        .await;
                }
                Ok(WsMessage::Pong(_)) => {}
                Err(_err) => {
                    break;
                }
            }
        }

        writer.abort();

        Ok(())
    }
}

struct SocketStreamConnection {
    id: String,
    subscriptions: Arc<RwLock<DashMap<String, Subscription>>>,
    sender: mpsc::Sender<StreamOutbound>,
    adapter: Arc<dyn StreamAdapter>,
}

impl SocketStreamConnection {
    pub fn new(adapter: Arc<dyn StreamAdapter>, sender: mpsc::Sender<StreamOutbound>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            subscriptions: Arc::new(RwLock::new(DashMap::new())),
            sender,
            adapter,
        }
    }

    pub async fn handle_socket_message(&self, msg: &StreamIncomingMessage) -> anyhow::Result<()> {
        match msg {
            StreamIncomingMessage::Join { data } => {
                let stream_name = data.stream_name.clone();
                let group_id = data.group_id.clone();
                let id = data.id.clone();
                let subscription_id = data.subscription_id.clone();

                self.subscriptions.write().await.insert(
                    subscription_id,
                    Subscription {
                        stream_name: stream_name.clone(),
                        group_id: group_id.clone(),
                        id: id.clone(),
                    },
                );

                let timestamp = chrono::Utc::now().timestamp_millis();

                if let Some(id) = id {
                    let data = self.adapter.get(&stream_name, &group_id, &id).await;

                    self.sender
                        .send(StreamOutbound::Stream(StreamWrapperMessage {
                            timestamp,
                            stream_name: stream_name.clone(),
                            group_id: group_id.clone(),
                            item_id: Some(id.clone()),
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
                            item_id: None,
                            event: StreamOutboundMessage::Sync {
                                data: serde_json::to_value(data).unwrap_or(Value::Null),
                            },
                        }))
                        .await?;
                }

                Ok(())
            }
            StreamIncomingMessage::Leave { data } => {
                self.subscriptions
                    .write()
                    .await
                    .remove(&data.subscription_id);
                Ok(())
            }
        }
    }
}

impl Drop for SocketStreamConnection {
    fn drop(&mut self) {
        let adapter = Arc::clone(&self.adapter);
        let id = self.id.clone();

        tokio::spawn(async move { adapter.unsubscribe(id).await });
    }
}

#[async_trait]
impl StreamConnection for SocketStreamConnection {
    async fn handle_stream_message(&self, msg: &StreamWrapperMessage) -> anyhow::Result<()> {
        let subscriptions = self.subscriptions.read().await;
        tracing::debug!(msg = ?msg, "Sending stream message");

        for subscription in subscriptions.iter() {
            let subscription = subscription.value();

            if subscription.stream_name == msg.stream_name
                && subscription.group_id == msg.group_id
                && (subscription.id.is_none() || subscription.id == msg.item_id)
            {
                match self.sender.send(StreamOutbound::Stream(msg.clone())).await {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!(error = ?e, "Failed to send stream message");
                    }
                }
            }
        }

        Ok(())
    }
}
