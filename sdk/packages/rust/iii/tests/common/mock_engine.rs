#![allow(dead_code)]

//! Lightweight in-process WebSocket mock of the III engine, used by SDK
//! integration tests that need to assert on the wire-level frames the SDK
//! emits without depending on a running engine.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::{Notify, broadcast};
use tokio::task::AbortHandle;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

pub struct MockEngine {
    url: String,
    received: Arc<Mutex<Vec<Value>>>,
    new_message: Arc<Notify>,
    close_active: broadcast::Sender<()>,
    accept_task: AbortHandle,
}

impl MockEngine {
    /// Bind to `127.0.0.1:0` and start accepting WebSocket connections.
    /// Every text/binary frame is decoded as JSON and appended to the
    /// internal buffer, observable via [`MockEngine::received_messages`].
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind mock engine");
        let addr = listener.local_addr().expect("local addr");
        let url = format!("ws://{}", addr);

        let received = Arc::new(Mutex::new(Vec::<Value>::new()));
        let new_message = Arc::new(Notify::new());
        let (close_active, _) = broadcast::channel::<()>(16);

        let received_acc = received.clone();
        let new_message_acc = new_message.clone();
        let close_active_acc = close_active.clone();

        let accept_task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let Ok(ws) = accept_async(stream).await else {
                    continue;
                };
                let (mut tx, mut rx) = ws.split();
                let mut close_rx = close_active_acc.subscribe();
                let received = received_acc.clone();
                let new_message = new_message_acc.clone();

                tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            _ = close_rx.recv() => {
                                let _ = tx.close().await;
                                return;
                            }
                            msg = rx.next() => {
                                match msg {
                                    Some(Ok(Message::Text(txt))) => {
                                        if let Ok(value) = serde_json::from_str::<Value>(txt.as_str()) {
                                            received.lock().unwrap().push(value);
                                            new_message.notify_waiters();
                                        }
                                    }
                                    Some(Ok(Message::Binary(bytes))) => {
                                        let s = String::from_utf8_lossy(&bytes).to_string();
                                        if let Ok(value) = serde_json::from_str::<Value>(&s) {
                                            received.lock().unwrap().push(value);
                                            new_message.notify_waiters();
                                        }
                                    }
                                    Some(Ok(Message::Ping(payload))) => {
                                        let _ = tx.send(Message::Pong(payload)).await;
                                    }
                                    Some(Ok(_)) => {}
                                    Some(Err(_)) | None => return,
                                }
                            }
                        }
                    }
                });
            }
        })
        .abort_handle();

        Self {
            url,
            received,
            new_message,
            close_active,
            accept_task,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn received_messages(&self) -> Vec<Value> {
        self.received.lock().unwrap().clone()
    }

    /// Close the currently active WebSocket connection (if any). The
    /// SDK will see the close, transition to `Reconnecting`, and the
    /// mock will accept the next handshake on the same listener.
    pub fn close_active_connection(&self) {
        let _ = self.close_active.send(());
    }

    /// Wait until `predicate` returns true on the current message list,
    /// or until `timeout` elapses. Returns the final snapshot.
    pub async fn wait_for(
        &self,
        predicate: impl Fn(&[Value]) -> bool,
        timeout: Duration,
    ) -> Vec<Value> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.new_message.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            {
                let snap = self.received.lock().unwrap();
                if predicate(&snap) {
                    return snap.clone();
                }
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return self.received.lock().unwrap().clone();
            }
            let _ = tokio::time::timeout(remaining, notified).await;
        }
    }

    /// Convenience: wait until at least `count` total messages have been
    /// received, or `timeout` elapses.
    pub async fn wait_for_count(&self, count: usize, timeout: Duration) -> Vec<Value> {
        self.wait_for(move |msgs| msgs.len() >= count, timeout)
            .await
    }

    /// Reset the recorded messages buffer. Useful between phases of a
    /// test (e.g. after asserting the initial registration set, before
    /// triggering a reconnect).
    pub fn clear(&self) {
        self.received.lock().unwrap().clear();
    }
}

impl Drop for MockEngine {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

/// Returns the message `type` field if present and a string.
pub fn message_type(msg: &Value) -> Option<&str> {
    msg.get("type").and_then(|v| v.as_str())
}

/// Returns the `id` field if present and a string.
pub fn message_id(msg: &Value) -> Option<&str> {
    msg.get("id").and_then(|v| v.as_str())
}

/// Count occurrences of messages with `type == message_type` AND `id == id`.
pub fn count_register(msgs: &[Value], message_type_str: &str, id: &str) -> usize {
    msgs.iter()
        .filter(|m| {
            self::message_type(m) == Some(message_type_str) && self::message_id(m) == Some(id)
        })
        .count()
}

/// Count messages with the given `type` regardless of id.
pub fn count_type(msgs: &[Value], message_type_str: &str) -> usize {
    msgs.iter()
        .filter(|m| self::message_type(m) == Some(message_type_str))
        .count()
}
