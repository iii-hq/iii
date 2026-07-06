//! Engine function and trigger ids, mirroring the Node SDK constants.
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
    pub const LIST_TRIGGERS: &'static str = "engine::triggers::list";
    pub const INFO_TRIGGERS: &'static str = "engine::triggers::info";
    pub const LIST_REGISTERED_TRIGGERS: &'static str = "engine::registered-triggers::list";
    pub const INFO_REGISTERED_TRIGGERS: &'static str = "engine::registered-triggers::info";
    pub const REGISTER_WORKER: &'static str = "engine::workers::register";
}

/// Engine trigger ids.
pub struct EngineTriggers;

impl EngineTriggers {
    pub const FUNCTIONS_AVAILABLE: &'static str = "engine::functions-available";
    pub const LOG: &'static str = "log";
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
        assert_eq!(
            EngineFunctions::LIST_REGISTERED_TRIGGERS,
            "engine::registered-triggers::list"
        );
    }

    #[test]
    fn engine_trigger_ids_match_node() {
        assert_eq!(
            EngineTriggers::FUNCTIONS_AVAILABLE,
            "engine::functions-available"
        );
        assert_eq!(EngineTriggers::LOG, "log");
    }
}
