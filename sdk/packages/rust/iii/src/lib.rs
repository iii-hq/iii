pub mod builtin_triggers;
pub mod channels;
pub mod engine;
pub mod error;
pub mod helpers;
pub mod iii;
pub mod protocol;
pub mod stream_provider;
pub mod structs;
pub mod triggers;
pub mod types;

/// Public runtime/worker types. (Stage 1 submodule grouping.)
pub mod runtime {
    pub use crate::iii::{
        FunctionInfo, FunctionRef, IIIConnectionState, TriggerInfo, TriggerTypeRef, WorkerInfo,
        WorkerMetadata,
    };
}

/// Public trigger types. (Stage 1 submodule grouping.)
pub mod trigger {
    pub use crate::builtin_triggers::IIITrigger;
    pub use crate::triggers::{Trigger, TriggerConfig, TriggerHandler};
}

/// Public channel types. (Stage 1 submodule grouping.)
pub mod channel {
    pub use crate::channels::{ChannelReader, ChannelWriter, StreamChannelRef};
    pub use crate::types::Channel;
}

/// Public error types. (Stage 1 submodule grouping.)
pub mod errors {
    pub use crate::error::{Error, InvocationError};
}

#[deprecated(since = "0.19.0", note = "import from iii_sdk::trigger")]
pub use builtin_triggers::IIITrigger;
#[deprecated(since = "0.20.0", note = "renamed to StreamChangeEvent")]
pub use builtin_triggers::StreamChangeEvent as StreamCallRequest;
#[deprecated(since = "0.20.0", note = "renamed to StreamJoinLeaveEvent")]
pub use builtin_triggers::StreamJoinLeaveEvent as StreamJoinLeaveCallRequest;
pub use builtin_triggers::{
    StreamChangeEvent, StreamEventDetail, StreamEventType, StreamJoinLeaveEvent,
    StreamJoinLeaveTriggerConfig, StreamTriggerConfig,
};
#[deprecated(since = "0.19.0", note = "import from iii_sdk::channel")]
pub use channels::{ChannelReader, ChannelWriter, StreamChannelRef};
pub use engine::{EngineFunctions, EngineTriggers};
#[deprecated(
    since = "0.19.0",
    note = "renamed to Error; import from iii_sdk::errors"
)]
pub use error::Error as IIIError;
pub use error::{Error, InvocationError};
#[deprecated(since = "0.20.0", note = "renamed to IIIClient")]
pub use iii::IIIClient as III;
pub use iii::TelemetryOptions;
#[deprecated(since = "0.20.0", note = "renamed to TelemetryOptions")]
pub use iii::TelemetryOptions as WorkerTelemetryMeta;
#[deprecated(since = "0.19.0", note = "import from iii_sdk::runtime")]
pub use iii::{
    FunctionInfo, FunctionRef, IIIConnectionState, TriggerInfo, TriggerTypeRef, WorkerInfo,
    WorkerMetadata,
};
pub use iii::{IIIClient, RegisterFunction, RegisterTriggerType};
pub use protocol::{
    EnqueueResult, ErrorBody, FunctionMessage, HttpAuthConfig, HttpInvocationConfig, HttpMethod,
    Message, RegisterFunctionMessage, RegisterTriggerInput, RegisterTriggerMessage,
    RegisterTriggerTypeMessage, TriggerAction, TriggerRequest,
};
pub use stream_provider::IStream;
pub use structs::{
    AuthInput, AuthResult, MiddlewareFunctionInput, OnFunctionRegistrationInput,
    OnFunctionRegistrationResult, OnTriggerRegistrationInput, OnTriggerRegistrationResult,
    OnTriggerTypeRegistrationInput, OnTriggerTypeRegistrationResult,
};
#[deprecated(since = "0.19.0", note = "import from iii_sdk::trigger")]
pub use triggers::{Trigger, TriggerConfig, TriggerHandler};
#[deprecated(since = "0.19.0", note = "import from iii_sdk::channel")]
pub use types::Channel;
pub use types::{
    ApiRequest, ApiResponse, DeleteResult, SetResult, StreamAuthInput, StreamAuthResult,
    StreamDeleteInput, StreamGetInput, StreamJoinResult, StreamListGroupsInput, StreamListInput,
    StreamRequest, StreamResponse, StreamSetInput, StreamUpdateInput, UpdateOp, UpdateOpError,
    UpdateResult,
};

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
    pub metadata: Option<iii::WorkerMetadata>,
    /// Custom HTTP headers sent during the WebSocket handshake.
    pub headers: Option<std::collections::HashMap<String, String>>,
    /// OpenTelemetry configuration.
    pub otel: Option<iii_observability::OtelConfig>,
}

/// Create and return a connected SDK instance. The WebSocket connection is
/// established automatically in a dedicated background thread with its own
/// tokio runtime.
///
/// Call [`IIIClient::shutdown`] before the end of `main` to cleanly stop the
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
pub fn register_worker(address: &str, options: InitOptions) -> IIIClient {
    let InitOptions {
        metadata,
        headers,
        otel,
    } = options;

    let iii = if let Some(metadata) = metadata {
        IIIClient::with_metadata(address, metadata)
    } else {
        IIIClient::new(address)
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

// ---------------------------------------------------------------------------
// Compile-fail doctests: these enforce that the four channel items relocated
// to `helpers` are NOT reachable at the crate root. They live here (not in
// `tests/`) because `cargo test --doc` only picks up doctests inside `src/`.
// ---------------------------------------------------------------------------

/// ```compile_fail
/// use iii_sdk::ChannelDirection;
/// ```
#[allow(dead_code)]
fn _ensure_channel_direction_not_top_level() {}

/// ```compile_fail
/// use iii_sdk::ChannelItem;
/// ```
#[allow(dead_code)]
fn _ensure_channel_item_not_top_level() {}

/// ```compile_fail
/// use iii_sdk::extract_channel_refs;
/// ```
#[allow(dead_code)]
fn _ensure_extract_channel_refs_not_top_level() {}

/// ```compile_fail
/// use iii_sdk::is_channel_ref;
/// ```
#[allow(dead_code)]
fn _ensure_is_channel_ref_not_top_level() {}

// ---------------------------------------------------------------------------
// Compile-fail doctest: enforces that `create_channel` (relocated to
// `helpers`) is no longer callable on `IIIClient`.
// ---------------------------------------------------------------------------

/// ```compile_fail
/// let iii = iii_sdk::IIIClient::new("ws://x");
/// iii.create_channel(None);
/// ```
#[allow(dead_code)]
fn _ensure_create_channel_not_on_instance() {}

// ---------------------------------------------------------------------------
// Stage 1 runtime submodule: runtime/worker types are reachable at their new
// canonical path `iii_sdk::runtime`.
// ---------------------------------------------------------------------------

/// ```rust,no_run
/// use iii_sdk::runtime::{
///     FunctionInfo, FunctionRef, IIIConnectionState, TriggerInfo, TriggerTypeRef, WorkerInfo,
///     WorkerMetadata,
/// };
/// ```
#[allow(dead_code)]
fn _ensure_runtime_submodule_path() {}

// ---------------------------------------------------------------------------
// Stage 1 trigger submodule: trigger types are reachable at their new
// canonical path `iii_sdk::trigger`.
// ---------------------------------------------------------------------------

/// ```rust,no_run
/// use iii_sdk::trigger::{IIITrigger, Trigger, TriggerConfig, TriggerHandler};
/// ```
#[allow(dead_code)]
fn _ensure_trigger_submodule_path() {}

// ---------------------------------------------------------------------------
// Stage 1 channel submodule: channel types are reachable at their new
// canonical path `iii_sdk::channel`.
// ---------------------------------------------------------------------------

/// ```rust,no_run
/// use iii_sdk::channel::{Channel, ChannelReader, ChannelWriter, StreamChannelRef};
/// ```
#[allow(dead_code)]
fn _ensure_channel_submodule_path() {}

// ---------------------------------------------------------------------------
// Stage 1 errors submodule: the renamed error type is reachable at its new
// canonical path `iii_sdk::errors::Error`.
// ---------------------------------------------------------------------------

/// ```rust,no_run
/// use iii_sdk::errors::Error;
/// fn _takes(_e: Error) {}
/// ```
#[allow(dead_code)]
fn _ensure_errors_submodule_path() {}

/// ```rust,no_run
/// use iii_sdk::{StreamChangeEvent, StreamJoinLeaveEvent};
/// fn _takes(_a: StreamChangeEvent, _b: StreamJoinLeaveEvent) {}
/// ```
#[allow(dead_code)]
fn _ensure_stream_event_names() {}

/// ```rust,no_run
/// use iii_sdk::{EngineFunctions, EngineTriggers};
/// let _ = (EngineFunctions::LIST_FUNCTIONS, EngineTriggers::LOG);
/// ```
#[allow(dead_code)]
fn _ensure_engine_constants_path() {}

/// ```rust,no_run
/// use iii_sdk::errors::InvocationError;
/// fn _takes(_e: InvocationError) {}
/// ```
#[allow(dead_code)]
fn _ensure_invocation_error_path() {}

/// ```rust,no_run
/// use iii_sdk::{StreamRequest, StreamResponse};
/// fn _takes(_req: StreamRequest, _res: StreamResponse) {}
/// ```
#[allow(dead_code)]
fn _ensure_stream_request_response_path() {}
