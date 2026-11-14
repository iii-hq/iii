//! The wire side of streams: the ref a payload carries for one end of an
//! engine pipe, and the WebSocket that end is read or written through.
//!
//! The public stream types ([`InputStream`](crate::InputStream),
//! [`OutputStream`](crate::OutputStream)) are built on the sockets here. The
//! ref types are public under [`protocol`](crate::protocol).

use std::collections::HashMap;
use std::pin::Pin;

use bytes::Bytes;
use futures_util::{FutureExt, SinkExt, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue, Request};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use crate::error::Error;
use crate::transport::{self, WsConnectResult, WsStream};

/// Which end of a pipe a [`StreamChannelRef`] opens: `read` is received as an
/// [`InputStream`](crate::InputStream), `write` as an
/// [`OutputStream`](crate::OutputStream).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum ChannelDirection {
    #[default]
    Read,
    Write,
}

impl ChannelDirection {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ChannelDirection::Read => "read",
            ChannelDirection::Write => "write",
        }
    }
}

/// One end of an engine pipe as a payload carries it:
/// `{ channel_id, access_key, direction }`. The JSON is the same in every SDK
/// and release, so new and old SDKs read each other's refs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct StreamChannelRef {
    pub channel_id: String,
    pub access_key: String,
    pub direction: ChannelDirection,
}

/// One frame of a stream: a text frame carries a message, a binary frame
/// bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelItem {
    Text(String),
    Binary(Vec<u8>),
}

/// A frame as the SDK reads it: binary frames keep the socket's buffer.
#[derive(Debug, Clone)]
pub(crate) enum Frame {
    Binary(Bytes),
    Text(String),
}

impl From<Frame> for ChannelItem {
    fn from(frame: Frame) -> Self {
        match frame {
            Frame::Binary(bytes) => ChannelItem::Binary(Vec::from(bytes)),
            Frame::Text(text) => ChannelItem::Text(text),
        }
    }
}

/// The frames of a stream, in order. An `Err` is the last item.
pub(crate) type FrameStream = Pin<Box<dyn Stream<Item = Result<Frame, Error>> + Send>>;

type WsWriter = futures_util::stream::SplitSink<WsStream, WsMessage>;

type WsReader = futures_util::stream::SplitStream<WsStream>;

/// The largest binary frame a writer sends; a larger write is split.
pub(crate) const MAX_FRAME_SIZE: usize = 64 * 1024;

/// The longest close reason a WebSocket close frame carries, in bytes.
const MAX_CLOSE_REASON: usize = 123;

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

/// The URL of one end of a channel on the engine at `engine_ws_base`.
pub(crate) fn channel_url(engine_ws_base: &str, channel_ref: &StreamChannelRef) -> String {
    build_channel_url(
        engine_ws_base,
        &channel_ref.channel_id,
        &channel_ref.access_key,
        channel_ref.direction.as_str(),
    )
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

/// `reason` cut to what a close frame carries, on a character boundary.
pub(crate) fn close_reason(reason: &str) -> String {
    if reason.len() <= MAX_CLOSE_REASON {
        return reason.to_string();
    }
    let mut end = MAX_CLOSE_REASON;
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    reason[..end].to_string()
}

/// Why a write fails once the engine closed the writer's socket: it does
/// when the stream's reader leaves.
const READER_LEFT: &str = "the stream is closed: its reader left";

/// The write end of a channel: dialed on first use, then frames until it is
/// closed or aborted. Dropping it with the socket open ends the socket with
/// no close frame, which the engine relays to the reader as an abort.
///
/// A write or send that fails, the socket closed by the engine because the
/// reader left among it, is [`Error::StreamClosed`], and so is every later one.
pub(crate) struct WriterEnd {
    url: String,
    headers: HashMap<String, String>,
    ws: Option<WsWriter>,
    /// The socket's read half: a writer reads nothing, but the engine's close
    /// (sent when the reader leaves) arrives on it.
    incoming: Option<WsReader>,
    /// Closed or aborted: the socket is gone and is not dialed again.
    ended: bool,
    /// Why a write failed: every later one fails with it (`STREAM_CLOSED`).
    failure: Option<String>,
}

impl WriterEnd {
    pub(crate) fn new(url: String, headers: HashMap<String, String>) -> Self {
        Self {
            url,
            headers,
            ws: None,
            incoming: None,
            ended: false,
            failure: None,
        }
    }

    async fn socket(&mut self) -> Result<&mut WsWriter, Error> {
        if let Some(failure) = &self.failure {
            return Err(Error::StreamClosed(failure.clone()));
        }
        if self.ended {
            return Err(Error::StreamClosed(
                "the stream is closed: it was closed or aborted".into(),
            ));
        }
        if self.ws.is_none() {
            // Through connect_channel: it negotiates TCP_NODELAY (MOT-5377).
            let (stream, _) = match connect_channel(&self.url, &self.headers).await {
                Ok(connected) => connected,
                Err(error) => {
                    return Err(Error::StreamClosed(format!(
                        "could not open the stream: {error}"
                    )));
                }
            };
            let (writer, reader) = stream.split();
            self.ws = Some(writer);
            self.incoming = Some(reader);
        } else if self.reader_left() {
            return Err(self.fail(READER_LEFT.into()));
        }
        Ok(self.ws.as_mut().expect("dialed above"))
    }

    /// Whether the engine closed the socket (its reader left): reads what
    /// it sent so far, without waiting.
    fn reader_left(&mut self) -> bool {
        let Some(incoming) = self.incoming.as_mut() else {
            return false;
        };
        loop {
            match incoming.next().now_or_never() {
                // Nothing yet: the socket is open.
                None => return false,
                // A ping (answered with the next frame sent), or a frame the
                // engine never sends a writer.
                Some(Some(Ok(WsMessage::Close(_)))) | Some(Some(Err(_))) | Some(None) => {
                    return true;
                }
                Some(Some(Ok(_))) => {}
            }
        }
    }

    /// Ends the socket after a failed write: nothing sent from now on is read.
    fn fail(&mut self, reason: String) -> Error {
        self.ws = None;
        self.incoming = None;
        self.failure = Some(reason.clone());
        Error::StreamClosed(reason)
    }

    /// The error of a send that failed: the reader left, when the engine's
    /// close is what ended the socket.
    fn send_failed(&mut self, error: tokio_tungstenite::tungstenite::Error) -> Error {
        use tokio_tungstenite::tungstenite::Error as WsError;
        let left = matches!(error, WsError::ConnectionClosed | WsError::AlreadyClosed)
            || self.reader_left();
        self.fail(if left {
            READER_LEFT.to_string()
        } else {
            format!("the stream's connection failed: {error}")
        })
    }

    /// Sends `data` in binary frames of at most [`MAX_FRAME_SIZE`] bytes.
    /// Resolves once the socket took them.
    pub(crate) async fn write(&mut self, data: &[u8]) -> Result<(), Error> {
        let ws = self.socket().await?;
        let mut sent = Ok(());
        for chunk in data.chunks(MAX_FRAME_SIZE) {
            sent = ws
                .send(WsMessage::Binary(Bytes::copy_from_slice(chunk)))
                .await;
            if sent.is_err() {
                break;
            }
        }
        sent.map_err(|error| self.send_failed(error))
    }

    /// Sends one text frame.
    pub(crate) async fn send_text(&mut self, text: &str) -> Result<(), Error> {
        let ws = self.socket().await?;
        let sent = ws.send(WsMessage::Text(text.into())).await;
        sent.map_err(|error| self.send_failed(error))
    }

    /// Ends the stream cleanly: a close frame with no code. A socket never
    /// dialed is dialed first, so the reader sees an empty stream.
    pub(crate) async fn close(&mut self) -> Result<(), Error> {
        // Delay the close frame slightly to allow the TCP stack to flush
        // all buffered send() data. Without this, the close frame can arrive
        // at the engine before all data frames, causing data truncation.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let result = match self.socket().await {
            Ok(ws) => ws.send(WsMessage::Close(None)).await.map_err(Error::from),
            Err(error) => Err(error),
        };
        self.ws = None;
        self.incoming = None;
        self.ended = true;
        result
    }

    /// Ends the stream with an error: a close frame with code 1011 and
    /// `reason`, which the engine relays to the reader as an abort. A socket
    /// never dialed is dialed first, so the reader always learns of it.
    pub(crate) async fn abort(&mut self, reason: &str) -> Result<(), Error> {
        let frame = CloseFrame {
            code: CloseCode::Error,
            reason: close_reason(reason).into(),
        };
        let result = match self.socket().await {
            Ok(ws) => ws
                .send(WsMessage::Close(Some(frame)))
                .await
                .map_err(Error::from),
            Err(error) => Err(error),
        };
        self.ws = None;
        self.incoming = None;
        self.ended = true;
        result
    }

    /// The open socket, taken: for an abort sent from `Drop`.
    pub(crate) fn take_socket(&mut self) -> Option<WsWriter> {
        self.ended = true;
        self.incoming = None;
        self.ws.take()
    }
}

/// Sends a 1011 close frame with `reason` on `ws`, from a task of its own.
pub(crate) fn abort_detached(mut ws: WsWriter, reason: &'static str) {
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(async move {
            let frame = CloseFrame {
                code: CloseCode::Error,
                reason: reason.into(),
            };
            let _ = ws.send(WsMessage::Close(Some(frame))).await;
        });
    }
    // Without a runtime the socket just drops: no close frame, which the
    // engine also relays to the reader as an abort.
}

/// The frames the read end at `url` receives, dialed on the first poll.
///
/// A close with no code or code 1000 ends the stream. Any other close code,
/// or the connection ending without a close, is an
/// [`Error::StreamAborted`] carrying the reason: the data read so far is
/// incomplete. A dial that fails (the engine is unreachable, or the channel
/// is gone and the engine answers 404) is an error too, not an empty stream.
pub(crate) fn read_frames(url: String, headers: HashMap<String, String>) -> FrameStream {
    enum State {
        Dial(String, HashMap<String, String>),
        Open(WsReader),
        Done,
    }
    Box::pin(futures_util::stream::unfold(
        State::Dial(url, headers),
        |state| async move {
            let mut reader = match state {
                State::Dial(url, headers) => match connect_channel(&url, &headers).await {
                    // Through connect_channel: it negotiates TCP_NODELAY (MOT-5377).
                    Ok((stream, _)) => stream.split().1,
                    Err(error) => {
                        return Some((Err(Error::WebSocket(error.to_string())), State::Done));
                    }
                },
                State::Open(reader) => reader,
                State::Done => return None,
            };
            loop {
                let error = match reader.next().await {
                    Some(Ok(WsMessage::Binary(data))) => {
                        return Some((Ok(Frame::Binary(data)), State::Open(reader)));
                    }
                    Some(Ok(WsMessage::Text(text))) => {
                        return Some((Ok(Frame::Text(text.to_string())), State::Open(reader)));
                    }
                    Some(Ok(WsMessage::Close(Some(frame)))) if frame.code != CloseCode::Normal => {
                        let reason = if frame.reason.is_empty() {
                            format!("channel closed with code {}", frame.code)
                        } else {
                            frame.reason.to_string()
                        };
                        Error::StreamAborted { reason }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => return None,
                    Some(Ok(_)) => continue,
                    Some(Err(error)) => Error::StreamAborted {
                        reason: format!("connection lost: {error}"),
                    },
                };
                return Some((Err(error), State::Done));
            }
        },
    ))
}

/// Check if a JSON value looks like a [`StreamChannelRef`]: an object with a
/// string `channel_id`, a string `access_key` and `direction` `"read"` or
/// `"write"`. Extra keys are tolerated.
pub fn is_channel_ref(value: &Value) -> bool {
    value.is_object()
        && value.get("channel_id").is_some_and(|v| v.is_string())
        && value.get("access_key").is_some_and(|v| v.is_string())
        && value
            .get("direction")
            .and_then(Value::as_str)
            .is_some_and(|d| d == "read" || d == "write")
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
    use super::*;

    // ---------------------------------------------------------------------------
    // WriterEnd::close() – timing and state tests
    // ---------------------------------------------------------------------------

    /// close() must sleep at least 10 ms before the close frame, even when the
    /// dial then fails.
    #[tokio::test]
    async fn close_sleeps_before_the_close_frame() {
        tokio::time::pause();
        let mut writer = WriterEnd::new("ws://127.0.0.1:1".to_string(), HashMap::new());

        let start = tokio::time::Instant::now();
        let _ = writer.close().await;
        assert!(
            start.elapsed() >= std::time::Duration::from_millis(10),
            "expected at least 10 ms elapsed, got {:?}",
            start.elapsed()
        );
    }

    /// After close(), the end is gone: writes fail without dialing again.
    #[tokio::test]
    async fn writes_after_close_fail() {
        let mut writer = WriterEnd::new("ws://127.0.0.1:1".to_string(), HashMap::new());
        let _ = writer.close().await;
        assert!(writer.ws.is_none());
        let error = writer.write(b"x").await.unwrap_err();
        assert!(error.to_string().contains("closed"), "{error}");
    }

    #[test]
    fn close_reason_is_cut_on_a_char_boundary() {
        assert_eq!(close_reason("short"), "short");
        let long = "é".repeat(100);
        let cut = close_reason(&long);
        assert!(cut.len() <= MAX_CLOSE_REASON);
        assert!(long.starts_with(&cut));
    }

    #[test]
    fn channel_refs_need_a_read_or_write_direction() {
        let ok =
            serde_json::json!({"channel_id": "c", "access_key": "k", "direction": "read", "x": 1});
        assert!(is_channel_ref(&ok));
        let other = serde_json::json!({"channel_id": "c", "access_key": "k", "direction": "up"});
        assert!(!is_channel_ref(&other));
        let missing = serde_json::json!({"channel_id": "c", "direction": "read"});
        assert!(!is_channel_ref(&missing));
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

    use super::*;
    use crate::transport::test_engine;

    fn url(address: std::net::SocketAddr) -> String {
        format!("ws://{address}/ws/channels/test?key=key&dir=read")
    }

    async fn collect(mut frames: FrameStream) -> (Vec<u8>, Option<Error>) {
        let mut bytes = Vec::new();
        while let Some(frame) = frames.next().await {
            match frame {
                Ok(Frame::Binary(data)) => bytes.extend_from_slice(&data),
                Ok(Frame::Text(_)) => {}
                Err(error) => {
                    // The error is the last item.
                    assert!(frames.next().await.is_none());
                    return (bytes, Some(error));
                }
            }
        }
        (bytes, None)
    }

    /// Reads a stream whose server sends `[1, 2]`, then closes with `close`,
    /// or drops the connection without a close frame when it is `None`.
    async fn read_stream(close: Option<Option<CloseFrame>>) -> (Vec<u8>, Option<Error>) {
        let (address, server) = test_engine::spawn(false).await;
        let server = tokio::spawn(async move {
            let mut ws = server.await.unwrap();
            ws.send(WsMessage::Binary(vec![1, 2].into())).await.unwrap();
            if let Some(frame) = close {
                ws.close(frame).await.unwrap();
            }
        });
        let result = collect(read_frames(url(address), HashMap::new())).await;
        server.await.unwrap();
        result
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
            let (bytes, error) = read_stream(Some(close)).await;
            assert_eq!(bytes, vec![1, 2]);
            assert!(error.is_none(), "{error:?}");
        }
    }

    // The engine sends 1011 when the writer aborted: what was read is not the
    // whole stream, and it ends with an abort that carries the reason.
    #[tokio::test]
    async fn close_with_another_code_is_an_abort() {
        let (bytes, error) = read_stream(Some(Some(CloseFrame {
            code: CloseCode::Error,
            reason: "channel writer aborted: closed with code 1011".into(),
        })))
        .await;
        assert_eq!(bytes, vec![1, 2]);
        match error {
            Some(Error::StreamAborted { reason }) => {
                assert_eq!(reason, "channel writer aborted: closed with code 1011")
            }
            other => panic!("expected an abort, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn connection_lost_without_close_is_an_abort() {
        let (bytes, error) = read_stream(None).await;
        assert_eq!(bytes, vec![1, 2]);
        assert!(
            matches!(error, Some(Error::StreamAborted { .. })),
            "{error:?}"
        );
    }

    // A dial that fails (engine unreachable, or the channel is gone and the
    // engine answers 404) is an error, not an end of stream.
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
            while let Ok((socket, _)) = listener.accept().await {
                let _ = tokio_tungstenite::accept_hdr_async(socket, |_: &Request, _: Response| {
                    Err(Response::builder().status(404).body(None).unwrap())
                })
                .await;
            }
        });

        for address in [unreachable, not_found] {
            let (_, error) = collect(read_frames(url(address), HashMap::new())).await;
            assert!(matches!(error, Some(Error::WebSocket(_))), "{error:?}");
        }
    }

    // The engine reads a writer's end from its close frame: close() must put
    // one with no code on the wire, abort() one with 1011 and the reason, and
    // a writer dropped without either must leave none, so the reader is told
    // the stream was cut.
    #[tokio::test]
    async fn writer_close_sends_a_clean_close_frame() {
        let (address, server) = test_engine::spawn(false).await;
        let mut writer = WriterEnd::new(format!("ws://{address}"), HashMap::new());
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
    async fn writer_close_never_dialed_still_closes_cleanly() {
        let (address, server) = test_engine::spawn(false).await;
        let mut writer = WriterEnd::new(format!("ws://{address}"), HashMap::new());
        writer.close().await.unwrap();
        let mut ws = server.await.unwrap();
        assert_eq!(ws.next().await.unwrap().unwrap(), WsMessage::Close(None));
    }

    #[tokio::test]
    async fn writer_abort_sends_1011_with_the_reason() {
        let (address, server) = test_engine::spawn(false).await;
        let mut writer = WriterEnd::new(format!("ws://{address}"), HashMap::new());
        writer.abort("disk full").await.unwrap();
        let mut ws = server.await.unwrap();
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            WsMessage::Close(Some(CloseFrame {
                code: CloseCode::Error,
                reason: "disk full".into()
            }))
        );
    }

    #[tokio::test]
    async fn writer_dropped_without_close_sends_no_close_frame() {
        let (address, server) = test_engine::spawn(false).await;
        let mut writer = WriterEnd::new(format!("ws://{address}"), HashMap::new());
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

    // The engine closes a writer's socket when its reader leaves: the next
    // write or send fails with STREAM_CLOSED, and so does every later one.
    #[tokio::test]
    async fn a_write_after_the_reader_left_is_stream_closed() {
        for send in [false, true] {
            let (address, server) = test_engine::spawn(false).await;
            let mut writer = WriterEnd::new(format!("ws://{address}"), HashMap::new());
            writer.write(&[1]).await.unwrap();
            let mut ws = server.await.unwrap();
            ws.next().await.unwrap().unwrap();
            ws.close(None).await.unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let error = loop {
                let attempt = if send {
                    writer.send_text("more").await
                } else {
                    writer.write(&[2]).await
                };
                if let Err(error) = attempt {
                    break error;
                }
                assert!(std::time::Instant::now() < deadline, "writes went on");
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            };
            for error in [error, writer.write(&[3]).await.unwrap_err()] {
                assert_eq!(
                    error.code(),
                    Some(crate::protocol::STREAM_CLOSED),
                    "{error:?}"
                );
                assert_eq!(error.to_string(), READER_LEFT);
            }
        }
    }

    #[tokio::test]
    async fn writes_are_split_into_64_kib_frames() {
        let (address, server) = test_engine::spawn(false).await;
        let mut writer = WriterEnd::new(format!("ws://{address}"), HashMap::new());
        writer.write(&vec![7; MAX_FRAME_SIZE + 1]).await.unwrap();
        writer.close().await.unwrap();
        let mut ws = server.await.unwrap();
        let sizes: Vec<usize> = [ws.next().await, ws.next().await]
            .into_iter()
            .map(|m| match m.unwrap().unwrap() {
                WsMessage::Binary(b) => b.len(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(sizes, vec![MAX_FRAME_SIZE, 1]);
    }
}
