// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use axum::{
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message as WsMessage, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::{ChannelItem, ChannelManager};
use crate::modules::config::AppState;
use crate::protocol::ChannelDirection;

#[derive(Deserialize)]
pub struct ChannelQuery {
    pub key: String,
    pub dir: ChannelDirection,
}

pub async fn channel_ws_upgrade(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    Query(params): Query<ChannelQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let dir_label = match params.dir {
        ChannelDirection::Read => "read",
        ChannelDirection::Write => "write",
    };
    tracing::info!(
        channel_id = %channel_id,
        dir = %dir_label,
        "Channel WS upgrade requested"
    );

    let channel_mgr = state.engine.channel_manager.clone();

    if channel_mgr.get_channel(&channel_id, &params.key).is_none() {
        tracing::warn!(
            channel_id = %channel_id,
            dir = %dir_label,
            "Channel not found — returning 404"
        );
        return (StatusCode::NOT_FOUND, "Channel not found").into_response();
    }

    tracing::info!(
        channel_id = %channel_id,
        dir = %dir_label,
        "Channel found, upgrading to WebSocket"
    );

    let direction = params.dir;
    let key = params.key.clone();

    ws.on_upgrade(move |socket| {
        handle_channel_socket(socket, channel_id, key, direction, channel_mgr)
    })
    .into_response()
}

async fn handle_channel_socket(
    mut socket: WebSocket,
    channel_id: String,
    access_key: String,
    direction: ChannelDirection,
    channel_mgr: Arc<ChannelManager>,
) {
    match direction {
        ChannelDirection::Read => {
            let rx = channel_mgr.take_receiver(&channel_id, &access_key).await;
            if let Some(mut rx) = rx {
                tracing::info!(channel_id = %channel_id, "Read: receiver acquired, streaming channel → WS");
                let mut total_bytes: u64 = 0;
                let mut msg_count: u64 = 0;
                while let Some(item) = rx.recv().await {
                    let ws_msg = match item {
                        ChannelItem::Text(s) => {
                            msg_count += 1;
                            tracing::debug!(channel_id = %channel_id, msg = msg_count, "Read: forwarding text message to WS");
                            WsMessage::Text(s.into())
                        }
                        ChannelItem::Binary(b) => {
                            let len = b.len();
                            total_bytes += len as u64;
                            msg_count += 1;
                            tracing::debug!(channel_id = %channel_id, msg = msg_count, bytes = len, "Read: forwarding binary chunk to WS");
                            WsMessage::Binary(b)
                        }
                    };
                    if socket.send(ws_msg).await.is_err() {
                        tracing::warn!(channel_id = %channel_id, "Read: WS send failed, aborting");
                        break;
                    }
                }
                tracing::info!(channel_id = %channel_id, total_bytes, msg_count, "Read: stream complete, sending WS close");
                let _ = socket.send(WsMessage::Close(None)).await;
            } else {
                tracing::warn!(channel_id = %channel_id, "Read: channel receiver already taken or missing");
                let _ = socket.send(WsMessage::Close(None)).await;
            }
        }
        ChannelDirection::Write => {
            let tx = channel_mgr.take_sender(&channel_id, &access_key).await;
            if let Some(tx) = tx {
                tracing::info!(channel_id = %channel_id, "Write: sender acquired, streaming WS → channel");
                let mut total_bytes: u64 = 0;
                let mut msg_count: u64 = 0;
                loop {
                    let msg = match socket.recv().await {
                        Some(Ok(msg)) => msg,
                        Some(Err(err)) => {
                            tracing::warn!(channel_id = %channel_id, error = ?err, "Write: WS receive error");
                            break;
                        }
                        None => break,
                    };
                    let item = match msg {
                        WsMessage::Text(s) => {
                            msg_count += 1;
                            tracing::debug!(channel_id = %channel_id, msg = msg_count, "Write: received text message from WS");
                            ChannelItem::Text(s.to_string())
                        }
                        WsMessage::Binary(data) => {
                            let len = data.len();
                            total_bytes += len as u64;
                            msg_count += 1;
                            tracing::debug!(channel_id = %channel_id, msg = msg_count, bytes = len, "Write: received binary chunk from WS");
                            ChannelItem::Binary(data)
                        }
                        WsMessage::Close(_) => {
                            tracing::info!(channel_id = %channel_id, "Write: received WS close frame");
                            break;
                        }
                        _ => continue,
                    };
                    if tx.send(item).await.is_err() {
                        tracing::warn!(channel_id = %channel_id, "Write: channel send failed (receiver dropped), aborting");
                        break;
                    }
                }
                tracing::info!(channel_id = %channel_id, total_bytes, msg_count, "Write: stream complete, dropping sender");
            } else {
                tracing::warn!(channel_id = %channel_id, "Write: channel sender already taken or missing");
                let _ = socket.send(WsMessage::Close(None)).await;
            }
        }
    }
}
