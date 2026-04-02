use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::error::IIIError;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChannelDirection {
    #[default]
    Read,
    Write,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

type WsWriter = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    WsMessage,
>;

type WsReader = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

fn build_channel_url(
    engine_ws_base: &str,
    channel_id: &str,
    access_key: &str,
    direction: &str,
) -> String {
    let base = engine_ws_base.trim_end_matches('/');
    let encoded_key = urlencoded(access_key);
    format!("{base}/ws/channels/{channel_id}?key={encoded_key}&dir={direction}")
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
    ws: Arc<Mutex<Option<WsWriter>>>,
}

impl ChannelWriter {
    pub fn new(engine_ws_base: &str, channel_ref: &StreamChannelRef) -> Self {
        Self {
            url: build_channel_url(
                engine_ws_base,
                &channel_ref.channel_id,
                &channel_ref.access_key,
                "write",
            ),
            ws: Arc::new(Mutex::new(None)),
        }
    }

    async fn ensure_connected(&self) -> Result<(), IIIError> {
        let mut guard = self.ws.lock().await;
        if guard.is_some() {
            return Ok(());
        }
        let (stream, _) = connect_async(&self.url).await?;
        let (writer, _reader) = stream.split();
        *guard = Some(writer);
        Ok(())
    }

    const MAX_FRAME_SIZE: usize = 64 * 1024;

    pub async fn write(&self, data: &[u8]) -> Result<(), IIIError> {
        self.ensure_connected().await?;
        let mut guard = self.ws.lock().await;
        let ws = guard.as_mut().ok_or(IIIError::NotConnected)?;
        for chunk in data.chunks(Self::MAX_FRAME_SIZE) {
            ws.send(WsMessage::Binary(chunk.to_vec().into())).await?;
        }
        Ok(())
    }

    pub async fn send_message(&self, msg: &str) -> Result<(), IIIError> {
        self.ensure_connected().await?;
        let mut guard = self.ws.lock().await;
        let ws = guard.as_mut().ok_or(IIIError::NotConnected)?;
        ws.send(WsMessage::Text(msg.to_string().into())).await?;
        Ok(())
    }

    pub async fn close(&self) -> Result<(), IIIError> {
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
    ws: Arc<Mutex<Option<WsReader>>>,
    message_callbacks: MessageCallbackList,
}

impl ChannelReader {
    pub fn new(engine_ws_base: &str, channel_ref: &StreamChannelRef) -> Self {
        Self {
            url: build_channel_url(
                engine_ws_base,
                &channel_ref.channel_id,
                &channel_ref.access_key,
                "read",
            ),
            ws: Arc::new(Mutex::new(None)),
            message_callbacks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn ensure_connected(&self) -> Result<(), IIIError> {
        let mut guard = self.ws.lock().await;
        if guard.is_some() {
            return Ok(());
        }
        let (stream, _) = connect_async(&self.url).await?;
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
    /// Returns `None` when the stream is closed.
    pub async fn next_binary(&self) -> Result<Option<Vec<u8>>, IIIError> {
        self.ensure_connected().await?;

        loop {
            let mut guard = self.ws.lock().await;
            let mut reader = guard.take().ok_or(IIIError::NotConnected)?;
            drop(guard);

            let msg = reader.next().await;

            let mut guard = self.ws.lock().await;
            *guard = Some(reader);
            drop(guard);

            match msg {
                Some(Ok(WsMessage::Binary(data))) => return Ok(Some(data.to_vec())),
                Some(Ok(WsMessage::Text(text))) => {
                    let callbacks = self.message_callbacks.lock().await;
                    for cb in callbacks.iter() {
                        cb(text.to_string());
                    }
                }
                Some(Ok(WsMessage::Close(_))) | None => return Ok(None),
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(IIIError::WebSocket(e.to_string())),
            }
        }
    }

    /// Read the entire stream into a single `Vec<u8>`.
    pub async fn read_all(&self) -> Result<Vec<u8>, IIIError> {
        let mut buffer = Vec::new();
        while let Some(chunk) = self.next_binary().await? {
            buffer.extend_from_slice(&chunk);
        }
        Ok(buffer)
    }

    pub async fn close(&self) -> Result<(), IIIError> {
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
