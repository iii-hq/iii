// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::{
    engine::Engine,
    modules::stream::{
        StreamCoreModule, StreamIncomingMessage,
        adapters::{StreamAdapter, StreamConnection},
        connection::SocketStreamConnection,
        structs::{StreamAuthContext, StreamOutbound},
        trigger::StreamTriggers,
    },
};

pub struct StreamSocketManager {
    pub engine: Arc<Engine>,
    pub auth_function: Option<String>,
    adapter: Arc<dyn StreamAdapter>,
    stream_module: Arc<StreamCoreModule>,
    triggers: Arc<StreamTriggers>,
}

impl StreamSocketManager {
    pub fn new(
        engine: Arc<Engine>,
        adapter: Arc<dyn StreamAdapter>,
        stream_module: Arc<StreamCoreModule>,
        auth_function: Option<String>,
        triggers: Arc<StreamTriggers>,
    ) -> Self {
        Self {
            engine,
            adapter,
            stream_module,
            auth_function,
            triggers,
        }
    }

    pub async fn socket_handler(
        &self,
        socket: WebSocket,
        context: Option<StreamAuthContext>,
    ) -> anyhow::Result<()> {
        let (mut ws_tx, mut ws_rx) = socket.split();
        let (tx, mut rx) = mpsc::channel::<StreamOutbound>(64);
        let connection = SocketStreamConnection::new(
            self.stream_module.clone(),
            context,
            tx,
            self.engine.clone(),
            self.triggers.clone(),
        );

        let writer = tokio::spawn(async move {
            while let Some(outbound) = rx.recv().await {
                let send_result = match outbound {
                    StreamOutbound::Stream(msg) => match serde_json::to_string(&msg) {
                        Ok(payload) => ws_tx.send(WsMessage::Text(payload.into())).await,
                        Err(err) => {
                            tracing::error!(error = ?err, "serialize error");
                            continue;
                        }
                    },
                    StreamOutbound::Raw(frame) => ws_tx.send(frame).await,
                };

                if send_result.is_err() {
                    tracing::error!(error = ?send_result.err(), "Failed to send stream message");
                    break;
                }
            }
        });

        let connection_id = connection.id.to_string();
        let connection = Arc::new(connection);

        if let Err(e) = self
            .adapter
            .subscribe(connection_id.clone(), connection.clone())
            .await
        {
            tracing::error!(error = %e, "Failed to subscribe connection");
            return Err(anyhow::anyhow!("Failed to subscribe connection: {}", e));
        }

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
        if let Err(e) = self.adapter.unsubscribe(connection_id).await {
            tracing::error!(error = %e, "Failed to unsubscribe connection");
        }
        connection.cleanup().await;

        Ok(())
    }
}
