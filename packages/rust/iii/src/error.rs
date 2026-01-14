use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum BridgeError {
    #[error("bridge is not connected")]
    NotConnected,
    #[error("invocation timed out")]
    Timeout,
    #[error("remote error ({code}): {message}")]
    Remote { code: String, message: String },
    #[error("handler error: {0}")]
    Handler(String),
    #[error("serialization error: {0}")]
    Serde(String),
    #[error("websocket error: {0}")]
    WebSocket(String),
}

impl From<serde_json::Error> for BridgeError {
    fn from(err: serde_json::Error) -> Self {
        BridgeError::Serde(err.to_string())
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for BridgeError {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        BridgeError::WebSocket(err.to_string())
    }
}
