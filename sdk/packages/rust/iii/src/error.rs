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
    /// A stream ended with an abort, code `STREAM_ABORTED`
    /// ([`protocol::STREAM_ABORTED`](crate::protocol::STREAM_ABORTED)): its
    /// writer called [`OutputStream::abort`](crate::OutputStream::abort),
    /// failed or went away before it closed. The data read before it is not
    /// the whole stream. A call whose sink received an aborted stream fails
    /// with it too, even when the function succeeded.
    ///
    /// Through an engine pipe the reason is the engine's (`channel writer
    /// aborted: ...`); in-process it is the writer's own.
    #[error("stream aborted: {reason}")]
    StreamAborted { reason: String },
    /// A call that cannot carry a stream, code `STREAM_NOT_ALLOWED`
    /// ([`protocol::STREAM_NOT_ALLOWED`](crate::protocol::STREAM_NOT_ALLOWED)):
    /// a payload holding a stream sent with
    /// [`CallAction::Enqueue`](crate::CallAction::Enqueue). Nothing was sent.
    #[error("{0}")]
    StreamNotAllowed(String),
    /// A write or send on a stream already closed or aborted, code
    /// `STREAM_CLOSED`
    /// ([`protocol::STREAM_CLOSED`](crate::protocol::STREAM_CLOSED)).
    #[error("{0}")]
    StreamClosed(String),
    /// A stream already in use, code `STREAM_IN_USE`
    /// ([`protocol::STREAM_IN_USE`](crate::protocol::STREAM_IN_USE)): a
    /// second reader (a clone included), the same stream twice in one
    /// payload, or a stream sent on after it was read to the end or already
    /// sent.
    #[error("{0}")]
    StreamInUse(String),
    /// Fatal registration rejection: another live worker already holds this
    /// worker name in the namespace, or the engine sent a rejection that
    /// names no function with a code this SDK does not know. The SDK stops
    /// and does not reconnect. A rejection that names a function
    /// (`FUNCTION_NAMESPACE_CONFLICT`, `INVALID_ARGS_FORMAT`, `UNSUPPORTED`)
    /// refuses only that function, is logged, and does not produce this
    /// error.
    #[error(
        "registration rejected ({code}) in namespace '{namespace}' by worker '{owner_worker_id}'"
    )]
    RegistrationRejected {
        code: String,
        namespace: String,
        worker_name: Option<String>,
        function_id: Option<String>,
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
    /// The wire code of this error, when it has one: the `code` of a remote
    /// failure or a registration rejection, `TIMEOUT` for [`Error::Timeout`],
    /// and the stream codes: `STREAM_ABORTED`, `STREAM_NOT_ALLOWED`,
    /// `STREAM_CLOSED`, `STREAM_IN_USE`.
    pub fn code(&self) -> Option<&str> {
        match self {
            Error::Remote { code, .. } | Error::RegistrationRejected { code, .. } => Some(code),
            Error::Timeout => Some("TIMEOUT"),
            Error::StreamAborted { .. } => Some(crate::protocol::STREAM_ABORTED),
            Error::StreamNotAllowed(_) => Some(crate::protocol::STREAM_NOT_ALLOWED),
            Error::StreamClosed(_) => Some(crate::protocol::STREAM_CLOSED),
            Error::StreamInUse(_) => Some(crate::protocol::STREAM_IN_USE),
            _ => None,
        }
    }

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
