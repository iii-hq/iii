// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Process-global snapshot of the live `iii-state` configuration.
//!
//! The state worker's hot-applied knobs (`triggers_enabled`, `max_value_bytes`)
//! are read on the request/fan-out path from this snapshot, so a configuration
//! change applied via `apply_config` (which swaps the snapshot with
//! [`update_state_config`]) takes effect on the very next read — no restart,
//! no per-handle invalidation. Mirrors `observability::otel`'s global-config
//! pattern.

use std::sync::{Arc, RwLock};

use super::config::StateModuleConfig;

/// The live snapshot. `None` until the worker publishes one at construction /
/// boot adoption. Const-initialised so it needs no lazy wrapper.
static GLOBAL_STATE_CONFIG: RwLock<Option<Arc<StateModuleConfig>>> = RwLock::new(None);

/// Swap the global state-config snapshot (the LIVE apply tier). Returns the
/// prior value, if any. Idempotent — applying an identical config is a no-op
/// from a reader's perspective.
pub fn update_state_config(config: StateModuleConfig) -> Option<Arc<StateModuleConfig>> {
    let mut slot = GLOBAL_STATE_CONFIG
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    slot.replace(Arc::new(config))
}

/// Read the current global state-config snapshot, if one has been published.
pub fn get_state_config() -> Option<Arc<StateModuleConfig>> {
    GLOBAL_STATE_CONFIG
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Clear the snapshot. Test-only: process-global state would otherwise leak
/// across `#[serial]` tests in the same binary.
#[cfg(test)]
pub fn clear_state_config_for_test() {
    *GLOBAL_STATE_CONFIG
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}
