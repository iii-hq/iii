use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

/// Errors returned by the III SDK.
#[derive(Debug, Error, Clone, Serialize, JsonSchema)]
pub enum IIIError {
    #[error("iii is not connected")]
    NotConnected,
    #[error("invocation timed out")]
    Timeout,
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("remote error ({code}): {message}")]
    Remote {
        code: String,
        message: String,
        stacktrace: Option<String>,
    },
    #[error("handler error: {0}")]
    Handler(String),
    #[error("serialization error: {0}")]
    Serde(String),
    #[error("websocket error: {0}")]
    WebSocket(String),
}

impl From<serde_json::Error> for IIIError {
    fn from(err: serde_json::Error) -> Self {
        IIIError::Serde(err.to_string())
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for IIIError {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        IIIError::WebSocket(err.to_string())
    }
}
