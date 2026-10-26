// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{sync::Arc, time::Duration};

use axum::{
    body::Bytes,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{CloseFrame, Message as WsMessage, WebSocket, close_code},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::channels::{ChannelItem, ChannelManager, StreamChannel};
use crate::protocol::ChannelDirection;
use crate::workers::worker::AppState;

/// Why a writer socket that ended without a close frame aborted.
const WRITER_LOST: &str = "channel writer aborted: connection lost before close";

/// How long a reader has to answer the engine's close frame.
const READER_CLOSE_REPLY_WAIT: Duration = Duration::from_secs(1);

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

    let Some(channel) = channel_mgr.get_channel(&channel_id, &params.key) else {
        tracing::warn!(
            channel_id = %channel_id,
            dir = %dir_label,
            "Channel not found — returning 404"
        );
        return (StatusCode::NOT_FOUND, "Channel not found").into_response();
    };

    tracing::info!(
        channel_id = %channel_id,
        dir = %dir_label,
        "Channel found, upgrading to WebSocket"
    );

    let direction = params.dir;

    ws.on_upgrade(move |socket| {
        handle_channel_socket(socket, channel_id, channel, direction, channel_mgr)
    })
    .into_response()
}

/// Takes its end from `channel`, the one the upgrade found, not from a second
/// lookup: owner cleanup may have removed it since, with an abort queued in it.
async fn handle_channel_socket(
    mut socket: WebSocket,
    channel_id: String,
    channel: Arc<StreamChannel>,
    direction: ChannelDirection,
    channel_mgr: Arc<ChannelManager>,
) {
    match direction {
        ChannelDirection::Read => {
            let rx = channel.rx.lock().await.take();
            // Holding the channel would keep a sender that never attached alive.
            drop(channel);
            if let Some(mut rx) = rx {
                tracing::info!(channel_id = %channel_id, "Read: receiver acquired, streaming channel → WS");
                let mut total_bytes: u64 = 0;
                let mut msg_count: u64 = 0;
                let mut peer_left = false;
                let mut abort = None;
                loop {
                    let item = tokio::select! {
                        item = rx.recv() => item,
                        // The reader can leave before the writer ever attaches
                        // (the http worker drops its response reader when the
                        // function returns a buffered value). Without watching
                        // the socket, its fd stays held until the sweeper
                        // drops the sender, minutes later.
                        msg = socket.recv() => match msg {
                            Some(Ok(WsMessage::Close(_))) | Some(Err(_)) | None => {
                                peer_left = true;
                                break;
                            }
                            Some(Ok(_)) => continue,
                        },
                    };
                    let Some(item) = item else { break };
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
                        ChannelItem::Abort(reason) => {
                            abort = Some(reason);
                            break;
                        }
                    };
                    if socket.send(ws_msg).await.is_err() {
                        tracing::warn!(channel_id = %channel_id, "Read: WS send failed, aborting");
                        break;
                    }
                }
                // Once the reader is done nothing can consume the channel:
                // drop the entry so an unattached sender goes with it and an
                // attached writer sees its receiver close.
                channel_mgr.remove_channel(&channel_id);
                if peer_left {
                    tracing::info!(channel_id = %channel_id, total_bytes, msg_count, "Read: peer left, releasing channel");
                } else if let Some(reason) = abort {
                    tracing::warn!(channel_id = %channel_id, total_bytes, msg_count, reason = %reason, "Read: writer aborted, sending WS error close");
                    let frame = CloseFrame {
                        code: close_code::ERROR,
                        reason: reason.into(),
                    };
                    close_reader(&mut socket, Some(frame)).await;
                } else {
                    tracing::info!(channel_id = %channel_id, total_bytes, msg_count, "Read: stream complete, sending WS close");
                    close_reader(&mut socket, None).await;
                }
            } else {
                tracing::warn!(channel_id = %channel_id, "Read: channel receiver already taken or missing");
                close_reader(&mut socket, None).await;
            }
        }
        ChannelDirection::Write => {
            let tx = channel.tx.lock().await.take();
            drop(channel);
            if let Some(tx) = tx {
                tracing::info!(channel_id = %channel_id, "Write: sender acquired, streaming WS → channel");
                let mut total_bytes: u64 = 0;
                let mut msg_count: u64 = 0;
                // Set when the writer ends without a clean close, so its
                // reader does not take a truncated stream for a whole one.
                let mut abort = None;
                loop {
                    let msg = tokio::select! {
                        msg = socket.recv() => msg,
                        // The reader is gone, so nothing will consume what this
                        // writer sends: close now rather than on its next frame.
                        _ = tx.closed() => {
                            tracing::info!(channel_id = %channel_id, "Write: reader gone, closing");
                            let _ = socket.send(WsMessage::Close(None)).await;
                            break;
                        }
                    };
                    let msg = match msg {
                        Some(Ok(msg)) => msg,
                        Some(Err(err)) => {
                            tracing::warn!(channel_id = %channel_id, error = ?err, "Write: WS receive error");
                            abort = Some(WRITER_LOST.to_string());
                            break;
                        }
                        None => {
                            abort = Some(WRITER_LOST.to_string());
                            break;
                        }
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
                            // Tungstenite frames share its read buffer (128 KiB
                            // by default). A queued frame can outlive the socket
                            // until a late reader attaches or the channel TTL
                            // expires. Retain only the payload, not that buffer.
                            ChannelItem::Binary(Bytes::copy_from_slice(&data))
                        }
                        WsMessage::Close(frame) => {
                            tracing::info!(channel_id = %channel_id, ?frame, "Write: received WS close frame");
                            // Decided on the frame: a reset or drain error after
                            // a clean close does not make it an abort.
                            abort = frame.filter(|f| f.code != close_code::NORMAL).map(|f| {
                                format!("channel writer aborted: closed with code {}", f.code)
                            });
                            // Defensive: drain any remaining buffered frames.
                            // Protects against clients that send close before all data is flushed.
                            let deadline =
                                tokio::time::Instant::now() + std::time::Duration::from_millis(10);
                            while let Ok(Some(Ok(msg))) =
                                tokio::time::timeout_at(deadline, socket.recv()).await
                            {
                                match msg {
                                    WsMessage::Binary(data) => {
                                        let len = data.len();
                                        total_bytes += len as u64;
                                        msg_count += 1;
                                        tracing::debug!(channel_id = %channel_id, msg = msg_count, bytes = len, "Write: drained binary after close");
                                        if tx
                                            .send(ChannelItem::Binary(Bytes::copy_from_slice(
                                                &data,
                                            )))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                    WsMessage::Text(s) => {
                                        msg_count += 1;
                                        tracing::debug!(channel_id = %channel_id, msg = msg_count, "Write: drained text after close");
                                        if tx.send(ChannelItem::Text(s.to_string())).await.is_err()
                                        {
                                            break;
                                        }
                                    }
                                    _ => break,
                                }
                            }
                            break;
                        }
                        _ => continue,
                    };
                    if tx.send(item).await.is_err() {
                        tracing::warn!(channel_id = %channel_id, "Write: channel send failed (receiver dropped), aborting");
                        break;
                    }
                }
                if let Some(reason) = abort {
                    tracing::warn!(channel_id = %channel_id, reason = %reason, "Write: writer aborted");
                    // Not try_send: a full buffer must not lose the abort. This
                    // fails at once when the reader is gone.
                    let _ = tx.send(ChannelItem::Abort(reason)).await;
                }
                tracing::info!(channel_id = %channel_id, total_bytes, msg_count, "Write: stream complete, dropping sender");
            } else {
                tracing::warn!(channel_id = %channel_id, "Write: channel sender already taken or missing");
                let _ = socket.send(WsMessage::Close(None)).await;
            }
        }
    }
}

/// Sends `frame` to a reader, then waits briefly for its close reply so the
/// reader sees a completed handshake and not a reset.
async fn close_reader(socket: &mut WebSocket, frame: Option<CloseFrame>) {
    if socket.send(WsMessage::Close(frame)).await.is_ok() {
        let _ = tokio::time::timeout(READER_CLOSE_REPLY_WAIT, async {
            while let Some(Ok(_)) = socket.recv().await {}
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use axum::{Router, routing::get};
    use futures_util::{SinkExt, StreamExt};
    use tokio::{
        net::{TcpListener, TcpStream},
        sync::watch,
    };
    use tokio_tungstenite::{
        MaybeTlsStream, WebSocketStream, connect_async,
        tungstenite::{
            Error as WsError, Message,
            protocol::{CloseFrame as WsCloseFrame, frame::coding::CloseCode},
        },
    };

    use super::*;
    use crate::engine::Engine;
    use crate::protocol::StreamChannelRef;
    use crate::workers::worker::WorkerManagerConfig;

    type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

    async fn connect_end(base_url: &str, end: &StreamChannelRef) -> Client {
        let dir = match end.direction {
            ChannelDirection::Read => "read",
            ChannelDirection::Write => "write",
        };
        let url = format!(
            "{base_url}/channels/{}?key={}&dir={dir}",
            end.channel_id, end.access_key
        );
        connect_async(url).await.expect("connect channel socket").0
    }

    /// Reads `reader` up to the engine's close frame and returns the binary
    /// payloads and that frame. It also checks the engine holds the socket
    /// open until the reader answers the close, then lets go of it.
    async fn read_to_close(reader: &mut Client) -> (Vec<Vec<u8>>, Option<WsCloseFrame>) {
        let mut data = Vec::new();
        loop {
            let next = tokio::time::timeout(Duration::from_secs(2), reader.next())
                .await
                .expect("reader frame");
            match next {
                Some(Ok(Message::Binary(bytes))) => data.push(bytes.to_vec()),
                Some(Ok(Message::Close(frame))) => {
                    let MaybeTlsStream::Plain(tcp) = reader.get_ref() else {
                        panic!("plain test socket")
                    };
                    assert!(
                        tokio::time::timeout(Duration::from_millis(200), tcp.peek(&mut [0; 1]))
                            .await
                            .is_err(),
                        "engine must wait for the reader's close reply"
                    );
                    // Polling again sends the close reply.
                    assert!(
                        tokio::time::timeout(Duration::from_millis(500), reader.next())
                            .await
                            .expect("engine drops the socket once the reader answered")
                            .is_none()
                    );
                    return (data, frame);
                }
                other => panic!("unexpected reader frame: {other:?}"),
            }
        }
    }

    fn assert_aborted(frame: Option<WsCloseFrame>, reason: &str) {
        let frame = frame.expect("abort close carries a code");
        assert_eq!(frame.code, CloseCode::Error);
        assert_eq!(frame.reason.as_str(), reason);
    }

    /// Waits until a reader socket took the channel's receiver.
    async fn wait_until_reader_attached(engine: &Engine, end: &StreamChannelRef) {
        let channel = engine
            .channel_manager
            .get_channel(&end.channel_id, &end.access_key)
            .expect("channel exists");
        for _ in 0..40 {
            if channel.rx.lock().await.is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("reader should attach");
    }

    async fn spawn_app() -> (Arc<Engine>, String, watch::Sender<bool>) {
        let engine = Arc::new(Engine::new());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let app = Router::new()
            .route("/channels/{id}", get(channel_ws_upgrade))
            .route("/owner-leaves/{id}", get(upgrade_then_owner_leaves))
            .with_state(AppState {
                engine: Arc::clone(&engine),
                config: Arc::new(WorkerManagerConfig::default()),
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

    /// `channel_ws_upgrade`, then the channel's owner leaves: after the upgrade
    /// found the channel, before the socket takes its end.
    async fn upgrade_then_owner_leaves(
        State(state): State<AppState>,
        Path(channel_id): Path<String>,
        Query(params): Query<ChannelQuery>,
        ws: WebSocketUpgrade,
    ) -> Response {
        let mgr = Arc::clone(&state.engine.channel_manager);
        let owner = mgr
            .get_channel(&channel_id, &params.key)
            .and_then(|channel| channel.owner_worker_id);
        let response = channel_ws_upgrade(State(state), Path(channel_id), Query(params), ws).await;
        if let Some(owner) = owner {
            mgr.remove_channels_by_worker(&owner).await;
        }
        response
    }

    async fn wait_until_removed(engine: &Engine, channel_id: &str, key: &str) -> bool {
        for _ in 0..40 {
            if engine
                .channel_manager
                .get_channel(channel_id, key)
                .is_none()
            {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    async fn wait_until_attached(engine: &Engine, channel_id: &str, key: &str) {
        let channel = engine
            .channel_manager
            .get_channel(channel_id, key)
            .expect("channel exists");
        for _ in 0..40 {
            if channel.tx.lock().await.is_none() && channel.rx.lock().await.is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("both channel ends should attach");
    }

    // The http worker's response channel for a buffered-return function: the
    // reader attaches, the writer never does, and the reader is dropped
    // without a close frame. The engine must notice and release the socket
    // and the channel now, not when the sweeper's TTL expires.
    #[tokio::test]
    async fn read_socket_released_when_reader_leaves_before_writer_attaches() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let (_writer_ref, reader_ref) = engine.channel_manager.create_channel(8, None);

        let url = format!(
            "{base_url}/channels/{}?key={}&dir=read",
            reader_ref.channel_id, reader_ref.access_key
        );
        let (socket, _) = connect_async(url).await.expect("connect read socket");
        drop(socket);

        assert!(
            wait_until_removed(&engine, &reader_ref.channel_id, &reader_ref.access_key).await,
            "channel should be released once its reader leaves"
        );
    }

    // Fully connected channels are never swept. When the reader leaves while
    // the writer is idle, the engine must close the writer socket too.
    #[tokio::test]
    async fn idle_writer_closed_when_reader_leaves() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let (writer_ref, reader_ref) = engine.channel_manager.create_channel(8, None);

        let (mut writer, _) = connect_async(format!(
            "{base_url}/channels/{}?key={}&dir=write",
            writer_ref.channel_id, writer_ref.access_key
        ))
        .await
        .expect("connect write socket");
        let (reader, _) = connect_async(format!(
            "{base_url}/channels/{}?key={}&dir=read",
            reader_ref.channel_id, reader_ref.access_key
        ))
        .await
        .expect("connect read socket");
        wait_until_attached(&engine, &writer_ref.channel_id, &writer_ref.access_key).await;

        drop(reader);

        let next = tokio::time::timeout(Duration::from_secs(2), writer.next())
            .await
            .expect("engine should close the idle writer once the reader leaves");
        assert!(matches!(
            next,
            None | Some(Ok(Message::Close(_))) | Some(Err(_))
        ));
    }

    // A late reader must still receive queued data after the writer disconnects,
    // but a tiny frame must not pin the WebSocket's entire 128 KiB read buffer
    // for the channel's five-minute orphan TTL.
    #[tokio::test]
    async fn queued_binary_does_not_retain_websocket_read_buffer() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let (writer_ref, reader_ref) = engine.channel_manager.create_channel(8, None);
        let (mut writer, _) = connect_async(format!(
            "{base_url}/channels/{}?key={}&dir=write",
            writer_ref.channel_id, writer_ref.access_key
        ))
        .await
        .expect("connect write socket");
        let payload = vec![42; 100];
        writer
            .send(Message::Binary(payload.clone().into()))
            .await
            .unwrap();
        writer.close(None).await.unwrap();
        drop(writer);

        let mut receiver = engine
            .channel_manager
            .take_receiver(&reader_ref.channel_id, &reader_ref.access_key)
            .await
            .unwrap();
        let item = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), receiver.recv())
                .await
                .unwrap()
                .is_none(),
            "writer must release the sender"
        );
        let ChannelItem::Binary(data) = item else {
            panic!("expected binary")
        };
        assert_eq!(data.as_ref(), payload.as_slice());
        // The socket is gone, so the queued frame owns the backing allocation.
        let allocation = data.try_into_mut().expect("frame should be uniquely owned");
        assert!(
            allocation.capacity() <= payload.len(),
            "100-byte frame retained {} bytes",
            allocation.capacity()
        );
        engine
            .channel_manager
            .remove_channel(&reader_ref.channel_id);
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
        // A drained channel must not linger in the manager (the sweeper
        // skips fully connected channels, so nothing else would remove it).
        assert!(wait_until_removed(&engine, &reader_ref.channel_id, &reader_ref.access_key).await);
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

        let (data, frame) = read_to_close(&mut socket).await;
        assert!(data.is_empty() && frame.is_none(), "{frame:?}");
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

    // A writer killed mid stream: its TCP connection ends without a close
    // frame. The reader gets what was sent, then an error close, not an end.
    #[tokio::test]
    async fn writer_lost_mid_stream_aborts_the_reader_after_its_data() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let (writer_ref, reader_ref) = engine.channel_manager.create_channel(8, None);
        let mut reader = connect_end(&base_url, &reader_ref).await;
        let mut writer = connect_end(&base_url, &writer_ref).await;
        wait_until_attached(&engine, &writer_ref.channel_id, &writer_ref.access_key).await;

        writer
            .send(Message::Binary(vec![1, 2, 3].into()))
            .await
            .unwrap();
        drop(writer);

        let (data, frame) = read_to_close(&mut reader).await;
        assert_eq!(data, vec![vec![1, 2, 3]]);
        assert_aborted(
            frame,
            "channel writer aborted: connection lost before close",
        );
    }

    // A close with no code or with 1000 is a clean end, even when the writer
    // drops its socket right after it without reading the reply (the Rust
    // SDK does).
    #[tokio::test]
    async fn writer_clean_close_ends_the_reader_cleanly() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        for close in [
            None,
            Some(WsCloseFrame {
                code: CloseCode::Normal,
                reason: "stream_complete".into(),
            }),
        ] {
            let (writer_ref, reader_ref) = engine.channel_manager.create_channel(8, None);
            let mut reader = connect_end(&base_url, &reader_ref).await;
            let mut writer = connect_end(&base_url, &writer_ref).await;
            writer.send(Message::Binary(vec![7].into())).await.unwrap();
            writer.close(close).await.unwrap();
            drop(writer);

            let (data, frame) = read_to_close(&mut reader).await;
            assert_eq!(data, vec![vec![7]]);
            assert!(
                frame.is_none(),
                "clean end keeps Close(None), got {frame:?}"
            );
        }
    }

    // Any other close code from the writer (a browser can only send 1000 or
    // 3000-4999, and 1001 is a client going away) is an abort.
    #[tokio::test]
    async fn writer_close_with_another_code_aborts_the_reader() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        for code in [1001, 4000] {
            let (writer_ref, reader_ref) = engine.channel_manager.create_channel(8, None);
            let mut reader = connect_end(&base_url, &reader_ref).await;
            let mut writer = connect_end(&base_url, &writer_ref).await;
            writer.send(Message::Binary(vec![9].into())).await.unwrap();
            writer
                .close(Some(WsCloseFrame {
                    code: CloseCode::from(code),
                    reason: "failed".into(),
                }))
                .await
                .unwrap();

            let (data, frame) = read_to_close(&mut reader).await;
            assert_eq!(data, vec![vec![9]]);
            assert_aborted(
                frame,
                &format!("channel writer aborted: closed with code {code}"),
            );
        }
    }

    // The abort is queued after the data, so a reader that attaches after
    // the writer died still learns the stream is incomplete.
    #[tokio::test]
    async fn late_reader_gets_the_queued_data_then_the_abort() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let (writer_ref, reader_ref) = engine.channel_manager.create_channel(8, None);
        let mut writer = connect_end(&base_url, &writer_ref).await;
        writer
            .send(Message::Binary(vec![4, 5].into()))
            .await
            .unwrap();
        drop(writer);

        let channel = engine
            .channel_manager
            .get_channel(&reader_ref.channel_id, &reader_ref.access_key)
            .expect("channel exists");
        let mut queued = 0;
        for _ in 0..40 {
            queued = channel
                .rx
                .lock()
                .await
                .as_ref()
                .expect("no reader yet")
                .len();
            if queued == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(queued, 2, "the data and the abort queue before the reader");
        drop(channel);

        let mut reader = connect_end(&base_url, &reader_ref).await;
        let (data, frame) = read_to_close(&mut reader).await;
        assert_eq!(data, vec![vec![4, 5]]);
        assert_aborted(
            frame,
            "channel writer aborted: connection lost before close",
        );
    }

    // fs::write: a writer killed mid upload while the guest reads slowly. The
    // abort waits for room behind the data, so a full buffer does not lose it.
    #[tokio::test]
    async fn writer_lost_with_a_full_buffer_still_aborts_the_reader() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let (writer_ref, reader_ref) = engine.channel_manager.create_channel(1, None);
        let mut receiver = engine
            .channel_manager
            .take_receiver(&reader_ref.channel_id, &reader_ref.access_key)
            .await
            .unwrap();
        let mut writer = connect_end(&base_url, &writer_ref).await;
        for byte in [1, 2] {
            writer
                .send(Message::Binary(vec![byte].into()))
                .await
                .unwrap();
        }
        drop(writer);

        let mut items = Vec::new();
        loop {
            // Slow: the buffer is full whenever the engine queues the next item.
            tokio::time::sleep(Duration::from_millis(50)).await;
            match tokio::time::timeout(Duration::from_secs(2), receiver.recv())
                .await
                .expect("channel item")
            {
                Some(item) => items.push(item),
                None => break,
            }
        }
        assert!(
            matches!(
                items.as_slice(),
                [ChannelItem::Binary(a), ChannelItem::Binary(b), ChannelItem::Abort(reason)]
                    if a.as_ref() == [1] && b.as_ref() == [2] && reason == WRITER_LOST
            ),
            "{items:?}"
        );
    }

    // A reader that never answers the engine's close is let go after the
    // reply wait, not held until it leaves.
    #[tokio::test]
    async fn reader_that_never_answers_the_close_is_let_go() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let (writer_ref, reader_ref) = engine.channel_manager.create_channel(8, None);
        let mut reader = connect_end(&base_url, &reader_ref).await;
        wait_until_reader_attached(&engine, &reader_ref).await;
        drop(
            engine
                .channel_manager
                .take_sender(&writer_ref.channel_id, &writer_ref.access_key)
                .await,
        );

        assert!(matches!(
            reader.next().await,
            Some(Ok(Message::Close(None)))
        ));
        // Not polled again, so the reply is never sent.
        let MaybeTlsStream::Plain(tcp) = reader.get_ref() else {
            panic!("plain test socket")
        };
        assert!(
            tokio::time::timeout(Duration::from_secs(2), tcp.peek(&mut [0; 1]))
                .await
                .is_ok(),
            "engine must drop the socket once the reply wait is over"
        );
    }

    // fs::write: a client killed after the trigger but before its writer
    // connected. The owner's teardown must not read as an empty, complete
    // stream.
    #[tokio::test]
    async fn owner_cleanup_aborts_a_reader_whose_writer_never_attached() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let owner = uuid::Uuid::new_v4();
        let (_writer_ref, reader_ref) = engine.channel_manager.create_channel(8, Some(owner));
        let mut reader = connect_end(&base_url, &reader_ref).await;
        wait_until_reader_attached(&engine, &reader_ref).await;

        engine
            .channel_manager
            .remove_channels_by_worker(&owner)
            .await;

        let (data, frame) = read_to_close(&mut reader).await;
        assert!(data.is_empty());
        assert_aborted(
            frame,
            "channel owner disconnected before the writer attached",
        );
    }

    // The same, with the owner leaving while the reader's upgrade is in
    // flight: the channel is gone from the manager by the time the socket
    // takes its receiver, and that must still read as an abort.
    #[tokio::test]
    async fn owner_cleanup_during_the_reader_upgrade_still_aborts_it() {
        let (engine, base_url, _shutdown_tx) = spawn_app().await;
        let (_writer_ref, reader_ref) = engine
            .channel_manager
            .create_channel(8, Some(uuid::Uuid::new_v4()));
        let (mut reader, _) = connect_async(format!(
            "{base_url}/owner-leaves/{}?key={}&dir=read",
            reader_ref.channel_id, reader_ref.access_key
        ))
        .await
        .expect("connect read socket");

        let (data, frame) = read_to_close(&mut reader).await;
        assert!(data.is_empty());
        assert_aborted(
            frame,
            "channel owner disconnected before the writer attached",
        );
    }
}
