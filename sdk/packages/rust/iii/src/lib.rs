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

// No `internal` submodule for Rust: the internal types grouped under
// `iii-sdk/internal` (Node) and `iii.internal` (Python) have no crate-root
// equivalent here. There is no `InternalHttpRequest` (the Rust SDK uses
// `iii_helpers::http::HttpRequest`), and the stream result types
// (`StreamSetResult`, `StreamUpdateResult`, `StreamDeleteResult`) live in `iii_helpers::stream`
// and are consumed inside `stream_provider.rs`, they are not re-exported at
// the crate root. Grouping them here would re-surface clean-break helpers
// types into the SDK, which the `compile_fail` doctests below deliberately
// forbid. Hence the `internal` grouping is a no-op for Rust.

pub use error::{Error, InvocationError};
pub use iii::TelemetryOptions;
pub use iii::{IIIClient, RegisterFunction, RegisterTriggerType};
pub use iii_helpers::queue::EnqueueResult;
pub use protocol::{Message, TriggerAction};
pub use stream_provider::IStream;
pub use structs::MiddlewareFunctionInput;
pub use types::{StreamRequest, StreamResponse};

/// Configuration options passed to [`register_worker`].
///
/// # Examples
/// ```rust,no_run
/// use iii_sdk::{register_worker, InitOptions};
///
/// let worker = register_worker("ws://localhost:49134", InitOptions::default());
/// ```
#[derive(Debug, Clone, Default)]
pub struct InitOptions {
    /// Custom worker metadata. Auto-detected if `None`.
    pub metadata: Option<iii::WorkerMetadata>,
    /// Custom HTTP headers sent during the WebSocket handshake.
    pub headers: Option<std::collections::HashMap<String, String>>,
    /// OpenTelemetry configuration.
    pub otel: Option<iii_helpers::observability::OtelConfig>,
    /// Namespace this worker registers under. Resolution order:
    /// `namespace` > env `III_NAMESPACE` > `None` (the engine then applies its
    /// default namespace). Mirrors the `III_WORKER_NAME` precedence.
    pub namespace: Option<String>,
}

/// Register the worker with a iii instance, returns a connected worker client.
/// The WebSocket connection is established automatically in a dedicated
/// background thread with its own tokio runtime.
///
/// Call [`IIIClient::shutdown`] before the end of `main` to cleanly stop the
/// connection and join the background thread. In Rust the process exits
/// when `main` returns, terminating all threads, so `shutdown()` must be
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
/// let worker = register_worker("ws://localhost:49134", InitOptions::default());
/// // register functions, handle events, etc.
/// worker.shutdown(); // cleanly stops the connection thread
/// ```
pub fn register_worker(address: &str, options: InitOptions) -> IIIClient {
    let InitOptions {
        metadata,
        headers,
        otel,
        namespace,
    } = options;

    let iii = if let Some(metadata) = metadata {
        IIIClient::with_metadata(address, metadata)
    } else {
        IIIClient::new(address)
    };

    // options.namespace > III_NAMESPACE > None (engine applies its default).
    if let Some(ns) = iii::resolve_namespace(namespace) {
        iii.set_namespace(ns);
    }

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

/// ```compile_fail
/// use iii_sdk::IIIConnectionState;
/// ```
#[allow(dead_code)]
fn _ensure_connection_state_not_top_level() {}

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

// ---------------------------------------------------------------------------
// 0.20 clean break: the deprecated crate-root re-exports and renamed aliases
// are removed. The relocated types live under their canonical submodule paths
// (`iii_sdk::{trigger,channel,runtime}`) and the renamed types use their new
// names (`IIIClient`, `Error`, `TelemetryOptions`).
// ---------------------------------------------------------------------------

/// ```compile_fail
/// use iii_sdk::{Channel, ChannelReader, ChannelWriter, StreamChannelRef};
/// ```
#[allow(dead_code)]
fn _ensure_channel_types_not_top_level() {}

/// ```compile_fail
/// use iii_sdk::{IIITrigger, Trigger, TriggerConfig, TriggerHandler};
/// ```
#[allow(dead_code)]
fn _ensure_trigger_types_not_top_level() {}

/// ```compile_fail
/// use iii_sdk::{FunctionInfo, FunctionRef, TriggerInfo, TriggerTypeRef, WorkerInfo, WorkerMetadata};
/// ```
#[allow(dead_code)]
fn _ensure_runtime_types_not_top_level() {}

/// ```compile_fail
/// use iii_sdk::III;
/// ```
#[allow(dead_code)]
fn _ensure_renamed_client_alias_removed() {}

/// ```compile_fail
/// use iii_sdk::IIIError;
/// ```
#[allow(dead_code)]
fn _ensure_renamed_error_alias_removed() {}

/// ```compile_fail
/// use iii_sdk::WorkerTelemetryMeta;
/// ```
#[allow(dead_code)]
fn _ensure_renamed_telemetry_alias_removed() {}

// ---------------------------------------------------------------------------
// Stream types relocated to `iii_helpers::stream`: they are no longer reachable
// at the crate root, and are reachable from the helpers submodule.
// ---------------------------------------------------------------------------

/// ```compile_fail
/// use iii_sdk::{StreamChangeEvent, StreamJoinLeaveEvent};
/// ```
#[allow(dead_code)]
fn _ensure_stream_events_not_top_level() {}

/// ```compile_fail
/// use iii_sdk::{StreamTriggerConfig, StreamJoinLeaveTriggerConfig};
/// ```
#[allow(dead_code)]
fn _ensure_stream_trigger_configs_not_top_level() {}

/// ```compile_fail
/// use iii_sdk::{UpdateOp, StreamGetInput};
/// ```
#[allow(dead_code)]
fn _ensure_stream_io_types_not_top_level() {}

/// ```rust,no_run
/// use iii_helpers::stream::{StreamChangeEvent, StreamJoinLeaveEvent};
/// fn _takes(_a: StreamChangeEvent, _b: StreamJoinLeaveEvent) {}
/// ```
#[allow(dead_code)]
fn _ensure_stream_events_helpers_path() {}

// ---------------------------------------------------------------------------
// engine submodule grouping: engine constants and the remote handler type are
// reachable only at their canonical path `iii_sdk::engine`. Rust folds this
// grouping into the existing `engine` module (the file `engine.rs`) rather than
// a separate `pub mod engine { ... }` block, which would clash with it.
// ---------------------------------------------------------------------------

/// ```rust,no_run
/// use iii_sdk::engine::{EngineFunctions, EngineTriggers, RemoteFunctionHandler};
/// let _ = (EngineFunctions::LIST_FUNCTIONS, EngineTriggers::LOG);
/// fn _takes(_h: RemoteFunctionHandler) {}
/// ```
#[allow(dead_code)]
fn _ensure_engine_submodule_path() {}

/// ```compile_fail
/// use iii_sdk::{EngineFunctions, EngineTriggers};
/// ```
#[allow(dead_code)]
fn _ensure_engine_constants_not_top_level() {}

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

// ---------------------------------------------------------------------------
// protocol submodule grouping: the low-level protocol message and
// register-input types are reachable only at their canonical path
// `iii_sdk::protocol` and are no longer re-exported at the crate root.
// ---------------------------------------------------------------------------

/// ```rust,no_run
/// use iii_sdk::protocol::{
///     ErrorBody, FunctionMessage, RegisterFunctionMessage, RegisterTriggerInput,
///     RegisterTriggerMessage, RegisterTriggerTypeMessage, TriggerRequest,
/// };
/// ```
#[allow(dead_code)]
fn _ensure_protocol_submodule_path() {}

/// ```compile_fail
/// use iii_sdk::{
///     ErrorBody, FunctionMessage, RegisterFunctionMessage, RegisterTriggerInput,
///     RegisterTriggerMessage, RegisterTriggerTypeMessage, TriggerRequest,
/// };
/// ```
#[allow(dead_code)]
fn _ensure_protocol_types_not_top_level() {}

// ---------------------------------------------------------------------------
// EnqueueResult is re-exported at the crate root for convenience alongside
// `TriggerAction`, mirroring its canonical home in `iii_helpers::queue`.
// ---------------------------------------------------------------------------

/// ```rust,no_run
/// use iii_sdk::EnqueueResult;
/// ```
#[allow(dead_code)]
fn _ensure_enqueue_result_at_root() {}
