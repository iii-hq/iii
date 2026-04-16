pub mod builtin_triggers;
pub mod channels;
pub mod error;
pub mod iii;
pub mod logger;
pub mod protocol;
pub mod stream;
pub mod structs;
pub mod telemetry;
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
    FunctionInfo, FunctionRef, FunctionsAvailableGuard, III, IIIAsyncFn, IIIConnectionState, IIIFn,
    IntoFunctionHandler, IntoFunctionRegistration, RegisterFunction, RegisterTriggerType,
    TriggerInfo, TriggerTypeInfo, TriggerTypeRef, WorkerInfo, WorkerMetadata, iii_async_fn, iii_fn,
};
pub use logger::Logger;
pub use protocol::{
    EnqueueResult, ErrorBody, FunctionMessage, HttpAuthConfig, HttpInvocationConfig, HttpMethod,
    Message, RegisterFunctionMessage, RegisterServiceMessage, RegisterTriggerInput,
    RegisterTriggerMessage, RegisterTriggerTypeMessage, TriggerAction, TriggerRequest,
};
pub use stream::UpdateBuilder;
pub use structs::{
    AuthInput, AuthResult, MiddlewareFunctionInput, OnFunctionRegistrationInput,
    OnFunctionRegistrationResult, OnTriggerRegistrationInput, OnTriggerRegistrationResult,
    OnTriggerTypeRegistrationInput, OnTriggerTypeRegistrationResult,
};
pub use triggers::{Trigger, TriggerConfig, TriggerHandler};
pub use types::{
    ApiRequest, ApiResponse, Channel, DeleteResult, FieldPath, SetResult, StreamAuthInput,
    StreamAuthResult, StreamDeleteInput, StreamGetInput, StreamJoinResult, StreamListGroupsInput,
    StreamListInput, StreamSetInput, StreamUpdateInput, UpdateOp, UpdateResult,
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
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// Custom worker metadata. Auto-detected if `None`.
    pub metadata: Option<WorkerMetadata>,
    /// Custom HTTP headers sent during the WebSocket handshake.
    pub headers: Option<std::collections::HashMap<String, String>>,
    /// OpenTelemetry configuration.
    pub otel: Option<crate::telemetry::types::OtelConfig>,
    /// Auto-install `SIGTERM` and `SIGINT` handlers that send a clean
    /// WebSocket Close frame to the engine, then `process::exit(128 +
    /// signal_number)`. Default `true`.
    ///
    /// Without this, `SIGTERM` takes the default disposition (immediate
    /// process termination) and the WebSocket connection drops without
    /// a Close frame. The engine then relies on TCP-level disconnect
    /// detection, which on a fast restart can race the next worker's
    /// registration — producing spurious `Function ... is already
    /// registered. Overwriting.` warnings and (worse) deleting the new
    /// worker's fresh registrations when the late cleanup finally
    /// fires.
    ///
    /// Set `false` to own signal handling yourself (common when
    /// embedding the SDK inside a larger service that already traps
    /// signals).
    pub auto_shutdown: bool,
}

impl Default for InitOptions {
    fn default() -> Self {
        Self {
            metadata: None,
            headers: None,
            otel: None,
            auto_shutdown: true,
        }
    }
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
/// fn main() {
///     let iii = register_worker("ws://localhost:49134", InitOptions::default());
///     // register functions, handle events, etc.
///     iii.shutdown(); // cleanly stops the connection thread
/// }
/// ```
pub fn register_worker(address: &str, options: InitOptions) -> III {
    let InitOptions {
        metadata,
        headers,
        otel,
        auto_shutdown,
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

    iii.set_auto_shutdown(auto_shutdown);
    iii.connect();

    iii
}

// OpenTelemetry re-exports
pub use telemetry::{
    context::{
        current_span_id, current_trace_id, extract_baggage, extract_context, extract_traceparent,
        get_all_baggage, get_baggage_entry, inject_baggage, inject_traceparent,
        remove_baggage_entry, set_baggage_entry,
    },
    flush_otel, get_meter, get_tracer,
    http_instrumentation::execute_traced_request,
    init_otel, is_initialized, shutdown_otel,
    types::OtelConfig,
    types::ReconnectionConfig,
    with_span,
};

// Re-export commonly used OpenTelemetry types for convenience
pub use opentelemetry::trace::SpanKind;
pub use opentelemetry::trace::Status as SpanStatus;
