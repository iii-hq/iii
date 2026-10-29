//! Engine function and source ids, mirroring the Node SDK constants.
//!
//! This module also re-exports [`RemoteFunctionHandler`] (and its
//! metadata-aware counterpart [`RemoteFunctionHandlerWithMetadata`]) so the
//! engine grouping (`iii_sdk::engine`) exposes the same symbols as the Node
//! `iii-sdk/engine` and Python `iii.engine` submodules. Rust cannot use a
//! separate `pub mod engine { ... }` grouping block here because the real
//! engine module is the file `engine.rs` (`pub mod engine;` in `lib.rs`),
//! so the grouping is folded into this module instead.

pub use crate::types::{RemoteFunctionHandler, RemoteFunctionHandlerWithMetadata};

/// Engine function ids for internal operations.
pub struct EngineFunctions;

impl EngineFunctions {
    pub const LIST_FUNCTIONS: &'static str = "engine::functions::list";
    pub const INFO_FUNCTIONS: &'static str = "engine::functions::info";
    pub const LIST_WORKERS: &'static str = "engine::workers::list";
    pub const INFO_WORKERS: &'static str = "engine::workers::info";
    pub const REGISTER_WORKER: &'static str = "engine::workers::register";
}

/// The ids of the engine-hosted sources: private functions the engine calls
/// once per event, whose handler returns the event. Observe them with filters
/// on their `input`:
///
/// ```rust,no_run
/// # use iii_sdk::{IIIClient, Source};
/// # use iii_sdk::engine::EngineSources;
/// # use serde_json::json;
/// # fn demo(iii: &IIIClient) {
/// iii.observe(
///     Source::function(EngineSources::FUNCTIONS_CHANGED),
///     "catalog::refresh",
/// );
/// iii.observe(
///     Source::function(EngineSources::LOG).filters(json!({ "input.level": { "$eq": "error" } })),
///     "alerts::on-error-log",
/// );
/// # }
/// ```
pub struct EngineSources;

impl EngineSources {
    /// A function was registered or unregistered.
    pub const FUNCTIONS_CHANGED: &'static str = "engine::on-functions-changed";
    /// A worker connected or disconnected.
    pub const WORKERS_CHANGED: &'static str = "engine::on-workers-changed";
    /// An observer changed state: `{ observer_id, function_id, target, state, reason? }`.
    pub const OBSERVERS_CHANGED: &'static str = "engine::on-observers-changed";
    /// A log record.
    pub const LOG: &'static str = "observability::on-log";
    /// A finished span.
    pub const SPAN: &'static str = "observability::on-span";
    /// The engine's configuration was reloaded.
    pub const CONFIGURATION_UPDATED: &'static str = "configuration::on-updated";
    /// A stream item changed; filter on `input.stream_name`, `input.group_id`.
    pub const STREAM_CHANGED: &'static str = "stream::on-changed";
    /// A client joined a stream.
    pub const STREAM_JOIN: &'static str = "stream::on-join";
    /// A client left a stream.
    pub const STREAM_LEAVE: &'static str = "stream::on-leave";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_function_ids_match_node() {
        assert_eq!(EngineFunctions::LIST_FUNCTIONS, "engine::functions::list");
        assert_eq!(
            EngineFunctions::REGISTER_WORKER,
            "engine::workers::register"
        );
    }

    #[test]
    fn engine_source_ids_match_node() {
        assert_eq!(
            [
                EngineSources::FUNCTIONS_CHANGED,
                EngineSources::WORKERS_CHANGED,
                EngineSources::OBSERVERS_CHANGED,
                EngineSources::LOG,
                EngineSources::SPAN,
                EngineSources::CONFIGURATION_UPDATED,
                EngineSources::STREAM_CHANGED,
                EngineSources::STREAM_JOIN,
                EngineSources::STREAM_LEAVE,
            ],
            [
                "engine::on-functions-changed",
                "engine::on-workers-changed",
                "engine::on-observers-changed",
                "observability::on-log",
                "observability::on-span",
                "configuration::on-updated",
                "stream::on-changed",
                "stream::on-join",
                "stream::on-leave",
            ]
        );
    }
}
