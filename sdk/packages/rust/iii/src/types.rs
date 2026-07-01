use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde_json::Value;

use crate::{
    channels::{ChannelReader, ChannelWriter, StreamChannelRef},
    error::Error,
    protocol::{RegisterFunctionMessage, RegisterTriggerTypeMessage},
    triggers::TriggerHandler,
};

/// A dispatchable function handler. Receives the invocation payload.
///
/// Handlers that also want the optional per-invocation `metadata` sidecar use
/// [`RemoteFunctionHandlerWithMetadata`]; this single-argument shape is kept
/// for backward compatibility.
pub type RemoteFunctionHandler =
    Arc<dyn Fn(Value) -> BoxFuture<'static, Result<Value, Error>> + Send + Sync>;

/// A dispatchable function handler that also receives the optional
/// per-invocation `metadata` sidecar (delivered as a distinct argument rather
/// than folded into the payload; `None` when the caller attached none).
///
/// This is the SDK's internal dispatch shape: handlers built from
/// metadata-unaware functions ignore the second argument.
pub type RemoteFunctionHandlerWithMetadata =
    Arc<dyn Fn(Value, Option<Value>) -> BoxFuture<'static, Result<Value, Error>> + Send + Sync>;

#[derive(Clone)]
pub struct RemoteFunctionData {
    pub message: RegisterFunctionMessage,
    pub handler: Option<RemoteFunctionHandlerWithMetadata>,
}

#[derive(Clone)]
pub struct RemoteTriggerTypeData {
    pub message: RegisterTriggerTypeMessage,
    pub handler: Arc<dyn TriggerHandler>,
}

/// Streaming request type, mirroring the Node and Python `StreamRequest`.
///
/// Alias of [`iii_helpers::http::HttpRequest`]; added for cross-language parity.
pub type StreamRequest<T = Value> = iii_helpers::http::HttpRequest<T>;

/// Streaming response type, mirroring the Node and Python `StreamResponse`.
///
/// Alias of [`iii_helpers::http::HttpResponse`]; added for cross-language parity.
pub type StreamResponse<T = Value> = iii_helpers::http::HttpResponse<T>;

/// A streaming channel pair for worker-to-worker data transfer.
pub struct Channel {
    pub writer: ChannelWriter,
    pub reader: ChannelReader,
    pub writer_ref: StreamChannelRef,
    pub reader_ref: StreamChannelRef,
}

#[cfg(test)]
mod tests {
    #[test]
    fn http_request_defaults_when_missing_fields() {
        let request: iii_helpers::http::HttpRequest = serde_json::from_str("{}").unwrap();

        assert!(request.query_params.is_empty());
        assert!(request.path_params.is_empty());
        assert!(request.headers.is_empty());
        assert_eq!(request.path, "");
        assert_eq!(request.method, "");
        assert!(request.body.is_null());
    }
}
