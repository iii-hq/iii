use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors returned by the III SDK.
#[derive(Debug, Error, Clone, Serialize, JsonSchema)]
pub enum Error {
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
    /// The engine rejected this worker's registration because another live
    /// worker already holds the name (or an exported function id) in the same
    /// namespace. Fatal: the SDK stops and does not reconnect.
    #[error(
        "registration rejected ({code}): worker '{worker_name}' in namespace '{namespace}' \
         is already owned by worker '{owner_worker_id}'"
    )]
    RegistrationRejected {
        code: String,
        namespace: String,
        worker_name: String,
        owner_worker_id: String,
    },
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Serde(err.to_string())
    }
}

impl From<String> for Error {
    fn from(msg: String) -> Self {
        Error::Handler(msg)
    }
}

impl From<&str> for Error {
    fn from(msg: &str) -> Self {
        Error::Handler(msg.to_string())
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        Error::WebSocket(err.to_string())
    }
}

/// Structured invocation failure, mirroring the Node and Python `InvocationError`.
///
/// Produced from the [`Error::Remote`] variant via [`Error::invocation_error`].
/// `function_id` is `None` from that accessor because the wire `Remote` payload
/// does not carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InvocationError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stacktrace: Option<String>,
}

impl std::fmt::Display for InvocationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for InvocationError {}

impl Error {
    /// If this is a remote invocation failure (`Error::Remote`), return its
    /// structured form. Returns `None` for transport/serde/handler errors.
    pub fn invocation_error(&self) -> Option<InvocationError> {
        match self {
            Error::Remote {
                code,
                message,
                stacktrace,
            } => Some(InvocationError {
                code: code.clone(),
                message: message.clone(),
                function_id: None,
                stacktrace: stacktrace.clone(),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod invocation_error_tests {
    use super::*;

    #[test]
    fn remote_error_yields_invocation_error() {
        let err = Error::Remote {
            code: "FORBIDDEN".into(),
            message: "nope".into(),
            stacktrace: Some("trace".into()),
        };
        let inv = err.invocation_error().expect("remote -> invocation");
        assert_eq!(inv.code, "FORBIDDEN");
        assert_eq!(inv.message, "nope");
        assert_eq!(inv.stacktrace.as_deref(), Some("trace"));
        assert_eq!(inv.to_string(), "FORBIDDEN: nope");
    }

    #[test]
    fn non_remote_error_yields_none() {
        assert!(Error::Timeout.invocation_error().is_none());
    }
}
