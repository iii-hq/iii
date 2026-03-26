pub mod channels;
pub mod error;
pub mod iii;
pub mod logger;
pub mod protocol;
pub mod stream;
#[cfg(feature = "otel")]
pub mod telemetry;
pub mod triggers;
pub mod types;

pub use channels::{
    ChannelDirection, ChannelItem, ChannelReader, ChannelWriter, StreamChannelRef,
    extract_channel_refs, is_channel_ref,
};
pub use error::IIIError;
pub use iii::{
    FunctionInfo, FunctionRef, FunctionsAvailableGuard, III, IIIAsyncFn, IIIConnectionState, IIIFn,
    IntoFunctionHandler, IntoFunctionRegistration, RegisterFunction, TriggerInfo, WorkerInfo,
    WorkerMetadata, iii_async_fn, iii_fn,
};
pub use logger::Logger;
pub use protocol::{
    EnqueueResult, ErrorBody, FunctionMessage, HttpAuthConfig, HttpInvocationConfig, HttpMethod,
    Message, RegisterFunctionMessage, RegisterServiceMessage, RegisterTriggerInput,
    RegisterTriggerMessage, RegisterTriggerTypeMessage, TriggerAction, TriggerRequest,
};
pub use stream::{Streams, UpdateBuilder};
pub use triggers::{Trigger, TriggerConfig, TriggerHandler};
pub use types::{
    ApiRequest, ApiResponse, Channel, FieldPath, StreamUpdateInput, UpdateOp, UpdateResult,
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
    /// OpenTelemetry configuration. Requires the `otel` feature.
    #[cfg(feature = "otel")]
    pub otel: Option<crate::telemetry::types::OtelConfig>,
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
        #[cfg(feature = "otel")]
        otel,
    } = options;

    let iii = if let Some(metadata) = metadata {
        III::with_metadata(address, metadata)
    } else {
        III::new(address)
    };

    #[cfg(feature = "otel")]
    if let Some(cfg) = otel {
        iii.set_otel_config(cfg);
    }

    iii.connect();

    iii
}

// OpenTelemetry re-exports (behind "otel" feature flag)
#[cfg(feature = "otel")]
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
#[cfg(feature = "otel")]
pub use opentelemetry::trace::SpanKind;
#[cfg(feature = "otel")]
pub use opentelemetry::trace::Status as SpanStatus;
