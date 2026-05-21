pub mod builtin_triggers;
pub mod channels;
pub mod error;
pub mod iii;
pub mod protocol;
pub mod stream;
pub mod structs;
pub mod triggers;
pub mod types;

pub use builtin_triggers::{
    IIITrigger, StreamCallRequest, StreamEventDetail, StreamEventType, StreamJoinLeaveCallRequest,
    StreamJoinLeaveTriggerConfig, StreamTriggerConfig,
};
pub use channels::{
    ChannelDirection, ChannelItem, ChannelReader, ChannelWriter, StreamChannelRef,
    extract_channel_refs, is_channel_ref,
};
pub use error::IIIError;
pub use iii::{
    FunctionRef, III, IIIAsyncFn, IIIConnectionState, IIIFn, IntoFunctionHandler,
    IntoFunctionRegistration, RegisterFunction, RegisterTriggerType, TriggerTypeRef,
    WorkerMetadata, iii_async_fn, iii_fn,
};
pub use protocol::{
    EnqueueResult, ErrorBody, FunctionMessage, HttpAuthConfig, HttpInvocationConfig, HttpMethod,
    Message, RegisterFunctionMessage, RegisterTriggerInput, RegisterTriggerMessage,
    RegisterTriggerTypeMessage, TriggerAction, TriggerRequest,
};
pub use stream::UpdateBuilder;
pub use structs::{
    AuthInput, AuthResult, MiddlewareFunctionInput, OnFunctionRegistrationInput,
    OnFunctionRegistrationResult, OnTriggerRegistrationInput, OnTriggerRegistrationResult,
    OnTriggerTypeRegistrationInput, OnTriggerTypeRegistrationResult,
};
pub use triggers::{Trigger, TriggerConfig, TriggerHandler};
pub use types::{
    ApiRequest, ApiResponse, Channel, DeleteResult, FieldPath, MergePath, SetResult,
    StreamAuthInput, StreamAuthResult, StreamDeleteInput, StreamGetInput, StreamJoinResult,
    StreamListGroupsInput, StreamListInput, StreamSetInput, StreamUpdateInput, UpdateOp,
    UpdateOpError, UpdateResult,
};

pub use serde_json::Value;

/// Configuration options passed to [`register_worker`].
///
/// # Examples
/// ```rust,no_run
/// use iii_sdk::{register_worker, InitOptions};
///
/// let iii = register_worker("ws://localhost:49134", InitOptions::default());
/// ```
#[derive(Debug, Clone, Default)]
pub struct InitOptions {
    /// Custom worker metadata. Auto-detected if `None`.
    pub metadata: Option<WorkerMetadata>,
    /// Custom HTTP headers sent during the WebSocket handshake.
    pub headers: Option<std::collections::HashMap<String, String>>,
    /// OpenTelemetry configuration.
    pub otel: Option<iii_observability::OtelConfig>,
}

/// Create and return a connected SDK instance. The WebSocket connection is
/// established automatically in a dedicated background thread with its own
/// tokio runtime.
///
/// Call [`III::shutdown`] before the end of `main` to cleanly stop the
/// connection and join the background thread. In Rust the process exits
/// when `main` returns, terminating all threads — so `shutdown()` must be
/// called while `main` is still running.
///
/// # Arguments
/// * `address` - WebSocket URL of the III engine (e.g. `ws://localhost:49134`).
/// * `options` - Configuration for worker metadata and OTel.
///
/// # Examples
/// ```rust,no_run
/// use iii_sdk::{register_worker, InitOptions};
///
/// let iii = register_worker("ws://localhost:49134", InitOptions::default());
/// // register functions, handle events, etc.
/// iii.shutdown(); // cleanly stops the connection thread
/// ```
pub fn register_worker(address: &str, options: InitOptions) -> III {
    let InitOptions {
        metadata,
        headers,
        otel,
    } = options;

    let iii = if let Some(metadata) = metadata {
        III::with_metadata(address, metadata)
    } else {
        III::new(address)
    };

    if let Some(h) = headers {
        iii.set_headers(h);
    }

    if let Some(cfg) = otel {
        iii.set_otel_config(cfg);
    }

    iii.connect();

    iii
}

// Re-exports from iii-observability for back-compat.
// The next SDK major will remove these — import from iii-observability directly.
pub use iii_observability::{
    BaggageSpanProcessor, CapturedContext, DEFAULT_ALLOWLIST, Logger, OtelConfig,
    REDACTED_PLACEHOLDER, ReconnectionConfig, capture_otel_context, current_span_id,
    current_span_is_recording, current_trace_id, execute_traced_request, extract_baggage,
    extract_context, extract_traceparent, flush_otel, get_all_baggage, get_baggage_entry,
    init_otel, inject_baggage, inject_traceparent, record_span_event, redact,
    redact_and_truncate, remove_baggage_entry, resolve_max_bytes_from_env, run_in_span,
    run_with_baggage, set_baggage_entry, set_current_span_attribute, set_current_span_error,
    shutdown_otel, with_span,
};
