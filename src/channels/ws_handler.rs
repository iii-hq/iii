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

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use axum::{Router, routing::get};
    use futures_util::{SinkExt, StreamExt};
    use tokio::{net::TcpListener, sync::watch};
    use tokio_tungstenite::{
        connect_async,
        tungstenite::{Error as WsError, Message},
    };

    use super::*;
    use crate::engine::Engine;

    async fn spawn_app() -> (Arc<Engine>, String, watch::Sender<bool>) {
        let engine = Arc::new(Engine::new());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let app = Router::new()
            .route("/channels/{id}", get(channel_ws_upgrade))
            .with_state(AppState {
                engine: Arc::clone(&engine),
                shutdown_rx,
            });

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind websocket test server");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve websocket app");
        });

        (engine, format!("ws://{addr}"), shutdown_tx)
    }

    #[tokio::test]
    async fn channel_ws_upgrade_returns_404_for_missing_channel() {
        let (_engine, base_url, _shutdown_tx) = spawn_app().await;
        let url = format!("{base_url}/channels/missing?key=bad&dir=read");

        let error = connect_async(url)
            .await
            .expect_err("missing channel should reject upgrade");
        match error {
            WsError::Http(response) => assert_eq!(response.status(), StatusCode::NOT_FOUND),
            other => panic!("expected http error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn read_socket_streams_channel_messages_and_closes() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let (writer_ref, reader_ref) = engine.channel_manager.create_channel(8, None);
        let sender = engine
            .channel_manager
            .take_sender(&writer_ref.channel_id, &writer_ref.access_key)
            .await
            .expect("take sender");

        let url = format!(
            "{base_url}/channels/{}?key={}&dir=read",
            reader_ref.channel_id, reader_ref.access_key
        );
        let (mut socket, _) = connect_async(url).await.expect("connect read socket");

        sender
            .send(ChannelItem::Text("hello".to_string()))
            .await
            .expect("send text");
        sender
            .send(ChannelItem::Binary(axum::body::Bytes::from_static(
                b"\x01\x02",
            )))
            .await
            .expect("send binary");
        drop(sender);

        assert!(matches!(
            socket.next().await.expect("text frame").expect("text message"),
            Message::Text(text) if text == "hello"
        ));
        assert!(matches!(
            socket.next().await.expect("binary frame").expect("binary message"),
            Message::Binary(bytes) if bytes.as_ref() == b"\x01\x02"
        ));
        assert!(matches!(
            socket
                .next()
                .await
                .expect("close frame")
                .expect("close message"),
            Message::Close(_)
        ));
    }

    #[tokio::test]
    async fn read_socket_closes_when_receiver_was_already_taken() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let (_writer_ref, reader_ref) = engine.channel_manager.create_channel(8, None);
        let _receiver = engine
            .channel_manager
            .take_receiver(&reader_ref.channel_id, &reader_ref.access_key)
            .await
            .expect("take receiver first");

        let url = format!(
            "{base_url}/channels/{}?key={}&dir=read",
            reader_ref.channel_id, reader_ref.access_key
        );
        let (mut socket, _) = connect_async(url).await.expect("connect read socket");

        assert!(matches!(
            socket
                .next()
                .await
                .expect("close frame")
                .expect("close message"),
            Message::Close(_)
        ));
    }

    #[tokio::test]
    async fn write_socket_forwards_messages_into_channel() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let (writer_ref, reader_ref) = engine.channel_manager.create_channel(8, None);
        let mut receiver = engine
            .channel_manager
            .take_receiver(&reader_ref.channel_id, &reader_ref.access_key)
            .await
            .expect("take receiver");

        let url = format!(
            "{base_url}/channels/{}?key={}&dir=write",
            writer_ref.channel_id, writer_ref.access_key
        );
        let (mut socket, _) = connect_async(url).await.expect("connect write socket");

        socket
            .send(Message::Text("hello".into()))
            .await
            .expect("send text");
        socket
            .send(Message::Binary(vec![1, 2, 3].into()))
            .await
            .expect("send binary");
        socket.send(Message::Close(None)).await.expect("send close");

        match receiver.recv().await.expect("receive text") {
            ChannelItem::Text(text) => assert_eq!(text, "hello"),
            _ => panic!("expected text item"),
        }
        match receiver.recv().await.expect("receive binary") {
            ChannelItem::Binary(bytes) => assert_eq!(bytes.as_ref(), &[1, 2, 3]),
            _ => panic!("expected binary item"),
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                .await
                .expect("channel close timeout")
                .is_none()
        );
    }

    #[tokio::test]
    async fn write_socket_closes_when_sender_was_already_taken() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let (writer_ref, _reader_ref) = engine.channel_manager.create_channel(8, None);
        let _sender = engine
            .channel_manager
            .take_sender(&writer_ref.channel_id, &writer_ref.access_key)
            .await
            .expect("take sender first");

        let url = format!(
            "{base_url}/channels/{}?key={}&dir=write",
            writer_ref.channel_id, writer_ref.access_key
        );
        let (mut socket, _) = connect_async(url).await.expect("connect write socket");

        assert!(matches!(
            socket
                .next()
                .await
                .expect("close frame")
                .expect("close message"),
            Message::Close(_)
        ));
    }
}
