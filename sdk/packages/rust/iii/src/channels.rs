use std::collections::HashMap;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, watch};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue, Request};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use crate::error::Error;
use crate::transport::{self, WsConnectResult, WsStream};

#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChannelDirection {
    #[default]
    Read,
    Write,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct StreamChannelRef {
    pub channel_id: String,
    pub access_key: String,
    pub direction: ChannelDirection,
}

#[derive(Debug, Clone)]
pub enum ChannelItem {
    Text(String),
    Binary(Vec<u8>),
}

type WsWriter = futures_util::stream::SplitSink<WsStream, WsMessage>;

type WsReader = futures_util::stream::SplitStream<WsStream>;

/// Opens a channel end with its handshake headers (see [`handshake_headers`]).
/// Channel frames are often small (HTTP bodies, control messages), so
/// `TCP_NODELAY` is negotiated like on the worker connection.
async fn connect_channel(url: &str, headers: &HashMap<String, String>) -> WsConnectResult {
    transport::connect(handshake_request(url, headers)?, None).await
}

/// Handshake headers for an engine connection: the caller's, plus
/// `Authorization: Bearer $III_TOKEN` when the variable is set and not blank.
/// An `Authorization` header the caller passed wins (names match
/// case-insensitively).
pub(crate) fn handshake_headers(
    explicit: Option<HashMap<String, String>>,
) -> HashMap<String, String> {
    let mut headers = explicit.unwrap_or_default();
    let token = std::env::var("III_TOKEN").unwrap_or_default();
    let token = token.trim();
    if !token.is_empty()
        && !headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("authorization"))
    {
        headers.insert("Authorization".to_string(), format!("Bearer {token}"));
    }
    headers
}

/// A client handshake request for `url` carrying `headers`. A header whose name
/// or value is not valid HTTP is skipped.
pub(crate) fn handshake_request(
    url: &str,
    headers: &HashMap<String, String>,
) -> Result<Request<()>, tokio_tungstenite::tungstenite::Error> {
    let mut request = url.into_client_request()?;
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            request.headers_mut().insert(name, value);
        }
    }
    Ok(request)
}

fn build_channel_url(
    engine_ws_base: &str,
    channel_id: &str,
    access_key: &str,
    direction: &str,
) -> String {
    // Keep a query already on the engine URL (e.g. `?token=...`); the channel's
    // own come after it.
    let (base, query) = engine_ws_base
        .split_once('?')
        .unwrap_or((engine_ws_base, ""));
    let base = base.trim_end_matches('/');
    let query = if query.is_empty() {
        String::new()
    } else {
        format!("{query}&")
    };
    let encoded_key = urlencoded(access_key);
    format!("{base}/ws/channels/{channel_id}?{query}key={encoded_key}&dir={direction}")
}

fn urlencoded(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push('%');
                result.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                result.push(char::from(b"0123456789ABCDEF"[(b & 0x0F) as usize]));
            }
        }
    }
    result
}

/// WebSocket-backed writer for streaming binary data and text messages.
pub struct ChannelWriter {
    url: String,
    headers: HashMap<String, String>,
    ws: Arc<Mutex<Option<WsWriter>>>,
}

impl ChannelWriter {
    /// Authenticates with `III_TOKEN` as a bearer token when it is set.
    pub fn new(engine_ws_base: &str, channel_ref: &StreamChannelRef) -> Self {
        Self {
            url: build_channel_url(
                engine_ws_base,
                &channel_ref.channel_id,
                &channel_ref.access_key,
                "write",
            ),
            headers: handshake_headers(None),
            ws: Arc::new(Mutex::new(None)),
        }
    }

    /// Connect with `explicit` handshake headers (plus the `III_TOKEN` bearer,
    /// see [`handshake_headers`]) instead of the token alone.
    pub(crate) fn with_headers(mut self, explicit: Option<HashMap<String, String>>) -> Self {
        self.headers = handshake_headers(explicit);
        self
    }

    async fn ensure_connected(&self) -> Result<(), Error> {
        let mut guard = self.ws.lock().await;
        if guard.is_some() {
            return Ok(());
        }
        // Through connect_channel: it negotiates TCP_NODELAY (MOT-5377).
        let (stream, _) = connect_channel(&self.url, &self.headers).await?;
        let (writer, _reader) = stream.split();
        *guard = Some(writer);
        Ok(())
    }

    const MAX_FRAME_SIZE: usize = 64 * 1024;

    pub async fn write(&self, data: &[u8]) -> Result<(), Error> {
        self.ensure_connected().await?;
        let mut guard = self.ws.lock().await;
        let ws = guard.as_mut().ok_or(Error::NotConnected)?;
        for chunk in data.chunks(Self::MAX_FRAME_SIZE) {
            ws.send(WsMessage::Binary(chunk.to_vec().into())).await?;
        }
        Ok(())
    }

    pub async fn send_message(&self, msg: &str) -> Result<(), Error> {
        self.ensure_connected().await?;
        let mut guard = self.ws.lock().await;
        let ws = guard.as_mut().ok_or(Error::NotConnected)?;
        ws.send(WsMessage::Text(msg.to_string().into())).await?;
        Ok(())
    }

    /// End the stream cleanly. Dropping a writer that has written, without
    /// calling `close()`, ends the socket with no close frame; the reader gets
    /// that as an error.
    pub async fn close(&self) -> Result<(), Error> {
        // Delay the close frame slightly to allow the TCP stack to flush
        // all buffered send() data. Without this, the close frame can arrive
        // at the engine before all data frames, causing data truncation.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let mut guard = self.ws.lock().await;
        if let Some(ws) = guard.as_mut() {
            ws.send(WsMessage::Close(None)).await?;
        }
        *guard = None;
        Ok(())
    }
}

type MessageCallback = Box<dyn Fn(String) + Send + Sync>;
type MessageCallbackList = Arc<Mutex<Vec<MessageCallback>>>;

/// WebSocket-backed reader for streaming binary data and text messages.
pub struct ChannelReader {
    url: String,
    headers: HashMap<String, String>,
    ws: Arc<Mutex<Option<WsReader>>>,
    message_callbacks: MessageCallbackList,
    /// The error that ended the stream. The socket reads as ended after an
    /// error, so later reads return this instead of a clean end.
    failed: Mutex<Option<Error>>,
    /// Set by `close()`: an end the reader made itself is not an error.
    closed: watch::Sender<bool>,
}

impl ChannelReader {
    /// Authenticates with `III_TOKEN` as a bearer token when it is set.
    pub fn new(engine_ws_base: &str, channel_ref: &StreamChannelRef) -> Self {
        Self {
            url: build_channel_url(
                engine_ws_base,
                &channel_ref.channel_id,
                &channel_ref.access_key,
                "read",
            ),
            headers: handshake_headers(None),
            ws: Arc::new(Mutex::new(None)),
            message_callbacks: Arc::new(Mutex::new(Vec::new())),
            failed: Mutex::new(None),
            closed: watch::Sender::new(false),
        }
    }

    /// Connect with `explicit` handshake headers (plus the `III_TOKEN` bearer,
    /// see [`handshake_headers`]) instead of the token alone.
    pub(crate) fn with_headers(mut self, explicit: Option<HashMap<String, String>>) -> Self {
        self.headers = handshake_headers(explicit);
        self
    }

    async fn ensure_connected(&self) -> Result<(), Error> {
        let mut guard = self.ws.lock().await;
        if guard.is_some() {
            return Ok(());
        }
        // Through connect_channel: it negotiates TCP_NODELAY (MOT-5377).
        let (stream, _) = connect_channel(&self.url, &self.headers).await?;
        let (_writer, reader) = stream.split();
        *guard = Some(reader);
        Ok(())
    }

    /// Register a callback for text messages received on this channel.
    pub async fn on_message<F>(&self, callback: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.message_callbacks.lock().await.push(Box::new(callback));
    }

    /// Read the next binary chunk from the channel.
    /// Text messages are dispatched to registered callbacks.
    /// Returns `None` when the writer ended the stream cleanly (a close with no
    /// code or code 1000). Any other close code, or the connection ending
    /// without a close, is an `Err` carrying the code and reason: the data
    /// read so far is incomplete. Once a read failed, later reads fail too.
    /// After `close()`, reads (and one in flight) return `None`.
    pub async fn next_binary(&self) -> Result<Option<Vec<u8>>, Error> {
        if let Some(error) = self.failed.lock().await.clone() {
            return Err(error);
        }
        let mut closed = self.closed.subscribe();
        // Never dial again after close(): the engine removed the channel.
        if *closed.borrow() {
            return Ok(None);
        }
        if let Err(error) = self.ensure_connected().await {
            // close() during the dial: its failure is the reader's own end.
            return if *closed.borrow() {
                Ok(None)
            } else {
                Err(error)
            };
        }

        loop {
            let mut guard = self.ws.lock().await;
            // close() during the first connect drops the socket the dial made.
            let Some(mut reader) = guard.take() else {
                return if *closed.borrow() {
                    Ok(None)
                } else {
                    Err(Error::NotConnected)
                };
            };
            drop(guard);

            let msg = tokio::select! {
                msg = reader.next() => msg,
                _ = closed.wait_for(|closed| *closed) => return Ok(None),
            };

            let mut guard = self.ws.lock().await;
            // close() ran meanwhile: drop the socket instead of keeping it.
            if *closed.borrow() {
                return Ok(None);
            }
            *guard = Some(reader);
            drop(guard);

            let error = match msg {
                Some(Ok(WsMessage::Binary(data))) => return Ok(Some(data.to_vec())),
                Some(Ok(WsMessage::Text(text))) => {
                    let callbacks = self.message_callbacks.lock().await;
                    for cb in callbacks.iter() {
                        cb(text.to_string());
                    }
                    continue;
                }
                Some(Ok(WsMessage::Close(Some(frame)))) if frame.code != CloseCode::Normal => {
                    Error::WebSocket(format!(
                        "channel closed with code {}: {}",
                        frame.code, frame.reason
                    ))
                }
                Some(Ok(WsMessage::Close(_))) | None => return Ok(None),
                Some(Ok(_)) => continue,
                Some(Err(e)) => Error::WebSocket(e.to_string()),
            };
            *self.failed.lock().await = Some(error.clone());
            return Err(error);
        }
    }

    /// Read the entire stream into a single `Vec<u8>`.
    pub async fn read_all(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = Vec::new();
        while let Some(chunk) = self.next_binary().await? {
            buffer.extend_from_slice(&chunk);
        }
        Ok(buffer)
    }

    pub async fn close(&self) -> Result<(), Error> {
        // Flag first: a read that puts the socket back checks it under the lock.
        self.closed.send_replace(true);
        let mut guard = self.ws.lock().await;
        *guard = None;
        Ok(())
    }
}

/// Check if a JSON value looks like a StreamChannelRef.
pub fn is_channel_ref(value: &Value) -> bool {
    value.is_object()
        && value.get("channel_id").is_some_and(|v| v.is_string())
        && value.get("access_key").is_some_and(|v| v.is_string())
        && value.get("direction").is_some_and(|v| v.is_string())
}

/// Extract all channel references from a JSON value's top-level fields,
/// returning the field path and the deserialized ref.
pub fn extract_channel_refs(data: &Value) -> Vec<(String, StreamChannelRef)> {
    let mut refs = Vec::new();
    extract_refs_recursive(data, String::new(), &mut refs);
    refs
}

fn extract_refs_recursive(
    data: &Value,
    prefix: String,
    refs: &mut Vec<(String, StreamChannelRef)>,
) {
    if let Some(obj) = data.as_object() {
        for (key, value) in obj {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };

            if is_channel_ref(value) {
                if let Ok(channel_ref) = serde_json::from_value::<StreamChannelRef>(value.clone()) {
                    refs.push((path, channel_ref));
                }
            } else if value.is_object() {
                extract_refs_recursive(value, path.clone(), refs);
            } else if let Some(arr) = value.as_array() {
                for (idx, item) in arr.iter().enumerate() {
                    extract_refs_recursive(item, format!("{path}[{idx}]"), refs);
                }
            }
        }
    } else if let Some(arr) = data.as_array() {
        for (idx, item) in arr.iter().enumerate() {
            let path = if prefix.is_empty() {
                format!("[{idx}]")
            } else {
                format!("{prefix}[{idx}]")
            };
            extract_refs_recursive(item, path, refs);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tokio::sync::Mutex;

    use super::*;

    // ---------------------------------------------------------------------------
    // ChannelWriter::close() – timing and state tests
    // ---------------------------------------------------------------------------

    /// close() must sleep at least 10 ms before clearing the ws field, even when
    /// there is no live WebSocket connection (ws = None).
    #[tokio::test]
    async fn close_sleeps_before_clearing_ws() {
        tokio::time::pause();

        let writer = ChannelWriter {
            url: "ws://test".to_string(),
            headers: HashMap::new(),
            ws: Arc::new(Mutex::new(None)),
        };

        let start = tokio::time::Instant::now();
        writer.close().await.expect("close() should not fail");
        let elapsed = start.elapsed();

        assert!(
            elapsed >= std::time::Duration::from_millis(10),
            "expected at least 10 ms elapsed, got {:?}",
            elapsed
        );
    }

    /// close() must return Ok(()) when the writer was never connected (ws = None).
    #[tokio::test]
    async fn close_when_not_connected_succeeds() {
        let writer = ChannelWriter {
            url: "ws://test".to_string(),
            headers: HashMap::new(),
            ws: Arc::new(Mutex::new(None)),
        };

        let result = writer.close().await;
        assert!(result.is_ok(), "expected Ok(()), got {:?}", result);
    }

    /// After close() completes, ws must be None regardless of its initial value.
    #[tokio::test]
    async fn close_sets_ws_to_none() {
        let writer = ChannelWriter {
            url: "ws://test".to_string(),
            headers: HashMap::new(),
            ws: Arc::new(Mutex::new(None)),
        };

        writer.close().await.expect("close() should not fail");

        let guard = writer.ws.lock().await;
        assert!(
            guard.is_none(),
            "expected ws to be None after close(), but it was Some"
        );
    }

    // ---------------------------------------------------------------------------
    // build_channel_url helper
    // ---------------------------------------------------------------------------

    /// build_channel_url must produce the correct URL structure including query
    /// parameters for key and direction.
    #[test]
    fn build_channel_url_formats_correctly() {
        let url = build_channel_url("http://engine", "chan-1", "mykey", "write");
        assert_eq!(url, "http://engine/ws/channels/chan-1?key=mykey&dir=write");
    }

    /// build_channel_url must strip a trailing slash from the base URL so the
    /// resulting URL does not contain a double slash.
    #[test]
    fn build_channel_url_strips_trailing_slash_from_base() {
        let url = build_channel_url("http://engine/", "chan-2", "k", "read");
        assert_eq!(url, "http://engine/ws/channels/chan-2?key=k&dir=read");
    }

    /// A query already on the engine URL stays ahead of the channel's own, and
    /// an empty one adds nothing.
    #[test]
    fn build_channel_url_keeps_the_base_query() {
        assert_eq!(
            build_channel_url("ws://engine/?token=abc", "chan-3", "k", "read"),
            "ws://engine/ws/channels/chan-3?token=abc&key=k&dir=read"
        );
        assert_eq!(
            build_channel_url("ws://engine/p/?a=1&b=2", "chan-3", "k", "write"),
            "ws://engine/p/ws/channels/chan-3?a=1&b=2&key=k&dir=write"
        );
        assert_eq!(
            build_channel_url("ws://engine/?", "chan-3", "k", "read"),
            "ws://engine/ws/channels/chan-3?key=k&dir=read"
        );
    }

    // ---------------------------------------------------------------------------
    // urlencoded helper
    // ---------------------------------------------------------------------------

    /// urlencoded must percent-encode characters outside the unreserved set
    /// (letters, digits, -, _, ., ~).
    #[test]
    fn urlencoded_encodes_special_chars() {
        let encoded = urlencoded("hello world+/=");
        assert_eq!(encoded, "hello%20world%2B%2F%3D");
    }

    /// urlencoded must leave unreserved characters (A-Z, a-z, 0-9, -, _, ., ~)
    /// unchanged.
    #[test]
    fn urlencoded_leaves_unreserved_chars_unchanged() {
        let input = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.~";
        assert_eq!(urlencoded(input), input);
    }

    /// urlencoded must produce an empty string for an empty input.
    #[test]
    fn urlencoded_returns_empty_for_empty_input() {
        assert_eq!(urlencoded(""), "");
    }
}

#[cfg(test)]
mod nodelay_tests {
    use super::*;
    use crate::transport::test_engine;

    async fn channel_nodelay(advertise: bool) -> bool {
        let (address, server) = test_engine::spawn(advertise).await;
        let url = format!("ws://{address}/ws/channels/test?direction=read");
        let (stream, _) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            connect_channel(&url, &HashMap::new()),
        )
        .await
        .unwrap()
        .unwrap();
        let nodelay = test_engine::nodelay(&stream);
        drop(server.await.unwrap());
        nodelay
    }

    #[tokio::test]
    async fn channel_socket_has_tcp_nodelay_when_engine_advertises_it() {
        assert!(channel_nodelay(true).await);
    }

    #[tokio::test]
    async fn channel_socket_keeps_nagle_when_engine_does_not_advertise_it() {
        assert!(!channel_nodelay(false).await);
    }
}

#[cfg(test)]
mod end_of_stream_tests {
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;

    use super::*;
    use crate::transport::test_engine;

    fn channel_ref() -> StreamChannelRef {
        StreamChannelRef {
            channel_id: "test".into(),
            access_key: "key".into(),
            direction: ChannelDirection::Read,
        }
    }

    /// Reads a stream whose server sends `[1, 2]`, then closes with `close`,
    /// or drops the connection without a close frame when it is `None`.
    async fn read_stream(
        close: Option<Option<CloseFrame>>,
    ) -> (ChannelReader, Result<Vec<u8>, Error>) {
        let (address, server) = test_engine::spawn(false).await;
        let server = tokio::spawn(async move {
            let mut ws = server.await.unwrap();
            ws.send(WsMessage::Binary(vec![1, 2].into())).await.unwrap();
            if let Some(frame) = close {
                ws.close(frame).await.unwrap();
            }
        });
        let reader = ChannelReader::new(&format!("ws://{address}"), &channel_ref());
        let result = reader.read_all().await;
        server.await.unwrap();
        (reader, result)
    }

    #[tokio::test]
    async fn close_with_no_code_or_1000_is_end_of_stream() {
        for close in [
            None,
            Some(CloseFrame {
                code: CloseCode::Normal,
                reason: "stream_complete".into(),
            }),
        ] {
            let (_, result) = read_stream(Some(close)).await;
            assert_eq!(result.unwrap(), vec![1, 2]);
        }
    }

    // The engine sends 1011 when the writer aborted: what was read is not the
    // whole stream, and reading on must not turn it into a clean end.
    #[tokio::test]
    async fn close_with_another_code_is_an_error_that_stays() {
        let (reader, result) = read_stream(Some(Some(CloseFrame {
            code: CloseCode::Error,
            reason: "channel writer aborted: connection lost before close".into(),
        })))
        .await;
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("1011") && error.contains("channel writer aborted"),
            "{error}"
        );
        assert_eq!(reader.next_binary().await.unwrap_err().to_string(), error);
        reader.close().await.unwrap();
        assert_eq!(reader.next_binary().await.unwrap_err().to_string(), error);
    }

    // A local close is not an error: it ends a read in flight and drops the
    // socket, and later reads end too without dialing again (test_engine
    // takes one connection, so a redial fails).
    #[tokio::test]
    async fn close_ends_the_stream_cleanly() {
        let (address, server) = test_engine::spawn(false).await;
        let server = tokio::spawn(async move {
            let mut ws = server.await.unwrap();
            ws.send(WsMessage::Binary(vec![1, 2].into())).await.unwrap();
            ws.next().await
        });
        let reader = ChannelReader::new(&format!("ws://{address}"), &channel_ref());
        assert_eq!(reader.next_binary().await.unwrap(), Some(vec![1, 2]));

        let wait = std::time::Duration::from_secs(1);
        let (read, closed) =
            tokio::join!(tokio::time::timeout(wait, reader.next_binary()), async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                reader.close().await
            });
        closed.unwrap();
        assert_eq!(read.expect("close() ends a read in flight").unwrap(), None);
        tokio::time::timeout(wait, server)
            .await
            .expect("close() drops the socket")
            .unwrap();

        assert_eq!(reader.next_binary().await.unwrap(), None);
        assert_eq!(reader.read_all().await.unwrap(), Vec::<u8>::new());
    }

    // close() queued on the lock the first dial holds takes the socket before
    // the read does: that read ends cleanly too, and so it does when that
    // dial fails (the engine already removed the channel).
    #[tokio::test]
    async fn close_during_the_first_connect_ends_the_read_cleanly() {
        for handshake in [true, false] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let (accepted_tx, accepted) = tokio::sync::oneshot::channel();
            let (go_tx, go) = tokio::sync::oneshot::channel::<()>();
            let server = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                accepted_tx.send(()).unwrap();
                go.await.unwrap();
                if handshake {
                    let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
                    ws.next().await;
                }
            });
            let reader = ChannelReader::new(&format!("ws://{address}"), &channel_ref());

            let (read, ()) = tokio::join!(reader.next_binary(), async {
                accepted.await.unwrap();
                let mut close = std::pin::pin!(reader.close());
                assert!(futures_util::poll!(close.as_mut()).is_pending());
                go_tx.send(()).unwrap();
                close.await.unwrap();
            });
            assert_eq!(read.unwrap(), None, "handshake: {handshake}");
            tokio::time::timeout(std::time::Duration::from_secs(1), server)
                .await
                .expect("close() drops the socket")
                .unwrap();
        }
    }

    #[tokio::test]
    async fn connection_lost_without_close_is_an_error_that_stays() {
        let (reader, result) = read_stream(None).await;
        let error = result.unwrap_err().to_string();
        // The socket is fused after the error and yields `None` next.
        assert_eq!(reader.next_binary().await.unwrap_err().to_string(), error);
    }

    // A dial that fails with no close() (engine unreachable, or the channel is
    // gone and the engine answers 404) is an error, not an empty stream.
    #[tokio::test]
    #[expect(
        clippy::result_large_err,
        reason = "tungstenite fixes the handshake callback's error type"
    )]
    async fn failed_dial_is_an_error_not_end_of_stream() {
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let unreachable = closed.local_addr().unwrap();
        drop(closed);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let not_found = listener.local_addr().unwrap();
        tokio::spawn(async move {
            // read_all dials again, so every dial gets the 404.
            while let Ok((socket, _)) = listener.accept().await {
                let _ = tokio_tungstenite::accept_hdr_async(socket, |_: &Request, _: Response| {
                    Err(Response::builder().status(404).body(None).unwrap())
                })
                .await;
            }
        });

        for address in [unreachable, not_found] {
            let reader = ChannelReader::new(&format!("ws://{address}"), &channel_ref());
            reader.next_binary().await.unwrap_err();
            reader.read_all().await.unwrap_err();
        }
    }

    // The engine reads a writer's end from its close frame: close() must put
    // one with no code on the wire, and a writer dropped without close() must
    // leave none, so the reader is told the stream was cut.
    #[tokio::test]
    async fn writer_close_sends_a_clean_close_frame() {
        let (address, server) = test_engine::spawn(false).await;
        let writer = ChannelWriter::new(&format!("ws://{address}"), &channel_ref());
        writer.write(&[1]).await.unwrap();
        writer.close().await.unwrap();

        let mut ws = server.await.unwrap();
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            WsMessage::Binary(vec![1].into())
        );
        assert_eq!(ws.next().await.unwrap().unwrap(), WsMessage::Close(None));
    }

    #[tokio::test]
    async fn writer_dropped_without_close_sends_no_close_frame() {
        let (address, server) = test_engine::spawn(false).await;
        let writer = ChannelWriter::new(&format!("ws://{address}"), &channel_ref());
        writer.write(&[1]).await.unwrap();
        drop(writer);

        let mut ws = server.await.unwrap();
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            WsMessage::Binary(vec![1].into())
        );
        let end = ws.next().await;
        assert!(
            matches!(end, Some(Err(_))),
            "expected no close frame, got {end:?}"
        );
    }
}
