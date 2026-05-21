use super::types::{ConnectionState, ReconnectionConfig};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

/// Message to send over the shared WebSocket connection
pub struct OtelMessage {
    pub prefix: &'static [u8],
    pub data: Vec<u8>,
}

/// Shared WebSocket connection for all OTEL exporters
pub struct SharedEngineConnection {
    state: Arc<RwLock<ConnectionState>>,
    tx: mpsc::Sender<OtelMessage>,
    shutdown_tx: Arc<Mutex<Option<mpsc::Sender<()>>>>,
    flush_tx: mpsc::Sender<oneshot::Sender<()>>,
}

impl SharedEngineConnection {
    /// Create a new shared connection and spawn the connection task
    pub fn new(ws_url: String, config: ReconnectionConfig) -> Self {
        Self::with_channel_capacity(ws_url, config, 10_000)
    }

    /// Create a new shared connection with a custom channel capacity
    pub fn with_channel_capacity(
        ws_url: String,
        config: ReconnectionConfig,
        channel_capacity: usize,
    ) -> Self {
        let (tx, rx) = mpsc::channel(channel_capacity);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let (flush_tx, flush_rx) = mpsc::channel(16);
        let state = Arc::new(RwLock::new(ConnectionState::Disconnected));

        let connection = Self {
            state: state.clone(),
            tx,
            shutdown_tx: Arc::new(Mutex::new(Some(shutdown_tx))),
            flush_tx,
        };

        // Spawn the connection management task
        tokio::spawn(connection_loop(
            ws_url,
            config,
            state,
            rx,
            shutdown_rx,
            flush_rx,
        ));

        connection
    }

    /// Send a message with the given prefix
    pub fn send(&self, prefix: &'static [u8], data: Vec<u8>) -> Result<(), String> {
        self.tx
            .try_send(OtelMessage { prefix, data })
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => {
                    tracing::warn!("Telemetry channel full, dropping message");
                    "Channel full".to_string()
                }
                mpsc::error::TrySendError::Closed(_) => "Connection closed".to_string(),
            })
    }

    /// Get current connection state
    pub async fn state(&self) -> ConnectionState {
        *self.state.read().await
    }

    /// Flush all pending messages through the WebSocket connection.
    ///
    /// Sends a flush request to the connection loop, which drains all buffered
    /// messages into the WebSocket. Returns once the drain is complete (or the
    /// connection loop has shut down).
    pub async fn flush(&self) {
        let (done_tx, done_rx) = oneshot::channel();
        if self.flush_tx.try_send(done_tx).is_ok() {
            let _ = done_rx.await;
        }
    }

    /// Shutdown the connection gracefully
    pub async fn shutdown(&self) {
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(()).await;
        }
    }
}

/// Collect pending messages from the channel into the buffer, respecting the max limit.
///
/// When the buffer is full, remaining messages are drained from the channel
/// and discarded to prevent stale data from being delivered on the next reconnect.
fn collect_pending(
    rx: &mut mpsc::Receiver<OtelMessage>,
    pending: &mut Vec<OtelMessage>,
    max_pending: usize,
) {
    let mut dropped = 0u64;
    while let Ok(msg) = rx.try_recv() {
        if pending.len() < max_pending {
            pending.push(msg);
        } else {
            dropped += 1;
        }
    }
    if dropped > 0 {
        warn!(dropped, "Pending message queue full, dropped messages");
    }
}

async fn connection_loop(
    ws_url: String,
    config: ReconnectionConfig,
    state: Arc<RwLock<ConnectionState>>,
    mut rx: mpsc::Receiver<OtelMessage>,
    mut shutdown_rx: mpsc::Receiver<()>,
    mut flush_rx: mpsc::Receiver<oneshot::Sender<()>>,
) {
    let mut retry_count: u64 = 0;
    let max_pending = config.max_pending_messages;
    let mut pending_messages: Vec<OtelMessage> = Vec::new();

    // Seed xorshift64 PRNG for jitter: mix nanoseconds, PID, and a constant.
    // We use a hand-rolled PRNG to avoid pulling in the `rand` crate for
    // non-security-critical reconnection jitter.
    let nanos_seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x517cc1b727220a95);
    let mut rng_state: u64 = nanos_seed ^ (std::process::id() as u64) ^ 0x6a09e667f3bcc908;

    loop {
        // Update state
        let is_reconnect = retry_count > 0;
        *state.write().await = if is_reconnect {
            ConnectionState::Reconnecting
        } else {
            ConnectionState::Connecting
        };

        debug!(
            "Attempting to connect to engine WebSocket: {} (attempt {})",
            ws_url,
            retry_count + 1
        );

        // Try to connect, racing against shutdown signal
        let ws_result = tokio::select! {
            result = connect_async(&ws_url) => result,
            _ = shutdown_rx.recv() => {
                info!("Shutting down OTEL connection during connect");
                *state.write().await = ConnectionState::Disconnected;
                return;
            }
        };

        match ws_result {
            Ok((ws_stream, _)) => {
                info!("Connected to engine WebSocket");
                *state.write().await = ConnectionState::Connected;
                retry_count = 0;

                let (mut write, mut read) = ws_stream.split();

                // Flush pending messages
                debug!("Flushing {} pending messages", pending_messages.len());
                for msg in pending_messages.drain(..) {
                    if let Err(e) = send_message(&mut write, msg).await {
                        error!("Failed to flush pending message: {}", e);
                        break;
                    }
                }

                // Main connection loop
                loop {
                    tokio::select! {
                        // New message to send
                        Some(msg) = rx.recv() => {
                            if let Err(e) = send_message(&mut write, msg).await {
                                error!("Failed to send message: {}", e);
                                break;
                            }
                        }
                        // Flush request: drain all buffered messages then signal completion
                        Some(done_tx) = flush_rx.recv() => {
                            let mut flush_ok = true;
                            while let Ok(msg) = rx.try_recv() {
                                if let Err(e) = send_message(&mut write, msg).await {
                                    error!("Failed to send message during flush: {}", e);
                                    flush_ok = false;
                                    break;
                                }
                            }
                            let _ = done_tx.send(());
                            if !flush_ok {
                                break;
                            }
                        }
                        // Check for incoming messages (ping/pong)
                        result = read.next() => {
                            match result {
                                Some(Ok(Message::Ping(data))) => {
                                    if let Err(e) = write.send(Message::Pong(data)).await {
                                        error!("Failed to send pong: {}", e);
                                        break;
                                    }
                                }
                                Some(Ok(Message::Close(_))) | None => {
                                    info!("WebSocket connection closed");
                                    break;
                                }
                                Some(Err(e)) => {
                                    error!("WebSocket error: {}", e);
                                    break;
                                }
                                _ => {}
                            }
                        }
                        // Shutdown signal
                        _ = shutdown_rx.recv() => {
                            info!("Shutdown signal received");
                            let _ = write.send(Message::Close(None)).await;
                            *state.write().await = ConnectionState::Disconnected;
                            return;
                        }
                    }
                }

                // Connection lost, collect pending messages
                collect_pending(&mut rx, &mut pending_messages, max_pending);
            }
            Err(e) => {
                error!("Failed to connect to engine WebSocket: {}", e);
                retry_count += 1;

                // Collect messages while disconnected
                collect_pending(&mut rx, &mut pending_messages, max_pending);
            }
        }

        // Check max retries
        if let Some(max) = config.max_retries
            && retry_count >= max
        {
            error!("Max retries exceeded, giving up");
            *state.write().await = ConnectionState::Failed;
            break;
        }

        // Calculate backoff delay with jitter (all math in f64, clamped before cast)
        let initial = config.effective_initial_delay_ms() as f64;
        let max_delay = config.max_delay_ms as f64;
        let exponent = retry_count.saturating_sub(1).min(63) as i32;
        let base_delay = (initial * config.backoff_multiplier.powi(exponent))
            .min(max_delay)
            .max(0.0);

        // xorshift64 for jitter randomness
        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 7;
        rng_state ^= rng_state << 17;
        let jitter = base_delay * config.jitter_factor * (rng_state as f64 / u64::MAX as f64);
        let delay = (base_delay + jitter).min(max_delay).max(0.0) as u64;

        debug!("Reconnecting in {}ms", delay);

        // Sleep with shutdown race
        tokio::select! {
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(delay)) => {}
            _ = shutdown_rx.recv() => {
                info!("Shutting down OTEL connection during backoff");
                *state.write().await = ConnectionState::Disconnected;
                return;
            }
        }
    }
}

async fn send_message<S>(
    write: &mut S,
    msg: OtelMessage,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: SinkExt<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut frame = Vec::with_capacity(msg.prefix.len() + msg.data.len());
    frame.extend_from_slice(msg.prefix);
    frame.extend_from_slice(&msg.data);

    write.send(Message::Binary(frame.into())).await?;
    Ok(())
}
