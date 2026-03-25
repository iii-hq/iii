// Copyright 2025 Motia LLC. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::Message as TungsteniteMessage};
use tracing::{debug, error};

use super::http::ProxyState;

/// WebSocket proxy handler.
///
/// Upgrades the incoming connection and creates a bidirectional pipe
/// to the engine's stream WebSocket at `ws://engine_host:ws_port`.
pub async fn ws_proxy_handler(
    State(state): State<Arc<ProxyState>>,
    ws: WebSocketUpgrade,
) -> Response {
    let target_url = format!("ws://{}:{}", state.config.engine_host, state.config.ws_port);

    debug!("Proxying WebSocket -> {}", target_url);

    ws.on_upgrade(move |client_ws| proxy_websocket(client_ws, target_url))
}

async fn proxy_websocket(client_ws: WebSocket, target_url: String) {
    // Connect to the upstream WebSocket
    let upstream_ws = match connect_async(&target_url).await {
        Ok((ws, _)) => ws,
        Err(e) => {
            error!(
                "Failed to connect to upstream WebSocket {}: {}",
                target_url, e
            );
            return;
        }
    };

    let (mut client_sink, mut client_stream) = client_ws.split();
    let (mut upstream_sink, mut upstream_stream) = upstream_ws.split();

    // Client -> Upstream
    // Note: Axum 0.8 Message uses Utf8Bytes/Bytes, tungstenite uses the same types
    // in 0.28+. Use .into() for seamless conversion between the two.
    let client_to_upstream = tokio::spawn(async move {
        while let Some(Ok(msg)) = client_stream.next().await {
            let upstream_msg = match msg {
                Message::Text(t) => TungsteniteMessage::Text(t.to_string().into()),
                Message::Binary(b) => TungsteniteMessage::Binary(b.to_vec().into()),
                Message::Ping(p) => TungsteniteMessage::Ping(p.into()),
                Message::Pong(p) => TungsteniteMessage::Pong(p.into()),
                Message::Close(frame) => {
                    let close_msg = TungsteniteMessage::Close(frame.map(|f| {
                        tokio_tungstenite::tungstenite::protocol::CloseFrame {
                            code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(f.code),
                            reason: f.reason.to_string().into(),
                        }
                    }));
                    let _ = upstream_sink.send(close_msg).await;
                    break;
                }
            };
            if upstream_sink.send(upstream_msg).await.is_err() {
                break;
            }
        }
    });

    // Upstream -> Client
    let upstream_to_client = tokio::spawn(async move {
        while let Some(Ok(msg)) = upstream_stream.next().await {
            let client_msg = match msg {
                TungsteniteMessage::Text(t) => Message::Text(t.to_string().into()),
                TungsteniteMessage::Binary(b) => Message::Binary(b.to_vec().into()),
                TungsteniteMessage::Ping(p) => Message::Ping(p),
                TungsteniteMessage::Pong(p) => Message::Pong(p),
                TungsteniteMessage::Close(frame) => {
                    let close_msg = Message::Close(frame.map(|f| axum::extract::ws::CloseFrame {
                        code: f.code.into(),
                        reason: f.reason.to_string().into(),
                    }));
                    let _ = client_sink.send(close_msg).await;
                    break;
                }
                _ => continue,
            };
            if client_sink.send(client_msg).await.is_err() {
                break;
            }
        }
    });

    // Wait for both directions to finish gracefully
    let _ = tokio::join!(client_to_upstream, upstream_to_client);
}
