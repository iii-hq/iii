pub mod builtin_triggers;
pub mod channels;
pub mod error;
pub mod helpers;
pub mod iii;
pub mod protocol;
pub mod stream;
pub mod stream_provider;
pub mod structs;
pub mod triggers;
pub mod types;

pub use builtin_triggers::{
    IIITrigger, StreamCallRequest, StreamEventDetail, StreamEventType, StreamJoinLeaveCallRequest,
    StreamJoinLeaveTriggerConfig, StreamTriggerConfig,
};
pub use channels::{ChannelReader, ChannelWriter, StreamChannelRef};
pub use error::IIIError;
pub use iii::{
    FunctionInfo, FunctionRef, III, IIIConnectionState, RegisterFunction, RegisterTriggerType,
    TriggerInfo, TriggerTypeInfo, TriggerTypeRef, WorkerInfo, WorkerMetadata,
};
pub use protocol::{
    EnqueueResult, ErrorBody, FunctionMessage, HttpAuthConfig, HttpInvocationConfig, HttpMethod,
    Message, RegisterFunctionMessage, RegisterTriggerInput, RegisterTriggerMessage,
    RegisterTriggerTypeMessage, TriggerAction, TriggerRequest,
};
pub use stream::UpdateBuilder;
pub use stream_provider::IStream;
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
// `helpers`) is no longer callable on `III`.
// ---------------------------------------------------------------------------

/// ```compile_fail
/// let iii = iii_sdk::III::new("ws://x");
/// iii.create_channel(None);
/// ```
#[allow(dead_code)]
fn _ensure_create_channel_not_on_instance() {}
