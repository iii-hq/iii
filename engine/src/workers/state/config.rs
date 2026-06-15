// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::workers::traits::AdapterEntry;

/// Runtime configuration for the builtin `iii-state` worker. Doc comments on
/// each field flow into the JSON Schema (via `schemars`) that the `iii-state`
/// configuration entry registers, so an agent introspecting the schema sees
/// the same descriptions and bounds documented here. After first boot the
/// configuration worker entry is the runtime source of truth; the config.yaml
/// block is seed-only.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StateModuleConfig {
    /// Storage adapter selection (`kv`, `redis`, `bridge`, ...) and its
    /// adapter-specific config. Restart-tier: changing it at runtime is logged
    /// and takes effect at the next engine start (the persisted entry is read
    /// at boot). The inner `config` is a free-form object so adapters stay
    /// pluggable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<AdapterEntry>,

    /// Globally enable or disable state change-trigger fan-out. Applied live:
    /// flipping this pauses/resumes all `state` trigger delivery without an
    /// engine restart. Defaults to `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triggers_enabled: Option<bool>,

    /// Reject `state::set` writes whose JSON-serialized value exceeds this many
    /// bytes, returning a `VALUE_TOO_LARGE` error before the adapter write.
    /// Applied live. Unset means no limit. (Incremental `state::update` is not
    /// size-guarded in this version.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub max_value_bytes: Option<usize>,

    /// Persistence flush cadence in milliseconds for the file-backed `kv`
    /// adapter. Applied live by respawning the adapter's save loop; has no
    /// effect on in-memory or non-kv adapters. Defaults to 5000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 100, max = 3_600_000))]
    pub save_interval_ms: Option<u64>,
}

impl Default for StateModuleConfig {
    fn default() -> Self {
        Self {
            adapter: None,
            triggers_enabled: Some(true),
            max_value_bytes: None,
            save_interval_ms: None,
        }
    }
}

impl StateModuleConfig {
    /// Normalize a freshly-loaded config. Runs on every load path (static
    /// block, seed, or a value read back from the configuration worker):
    /// out-of-range numeric knobs fall back to `None` (their built-in defaults)
    /// so a stale or hand-edited value neither rejects every write nor
    /// busy-loops the save loop. The JSON Schema rejects out-of-range values at
    /// `configuration::set` time; this re-applies the same bounds to values that
    /// bypass it (yaml / hand-edited persisted files). `save_interval_ms` is
    /// held to the schema's `[100, 3_600_000]` range so a hand-edited `1` can't
    /// drive a 1ms flush loop.
    pub fn normalized(mut self) -> Self {
        self.max_value_bytes = self.max_value_bytes.filter(|&n| n > 0);
        self.save_interval_ms = self
            .save_interval_ms
            .filter(|&n| (100..=3_600_000).contains(&n));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_state_config() {
        let config: StateModuleConfig = serde_json::from_value(json!({})).unwrap();
        assert!(config.adapter.is_none());
    }

    #[test]
    fn state_config_with_adapter() {
        let json = json!({"adapter": {"name": "redis", "config": {"url": "redis://localhost"}}});
        let config: StateModuleConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.adapter.unwrap().name, "redis");
    }

    #[test]
    fn manual_default_enables_triggers() {
        let config = StateModuleConfig::default();
        assert_eq!(config.triggers_enabled, Some(true));
        assert!(config.adapter.is_none());
        assert!(config.max_value_bytes.is_none());
        assert!(config.save_interval_ms.is_none());
    }

    #[test]
    fn deserializing_empty_leaves_knobs_unset() {
        // serde `default` on each Option is None (not the manual struct
        // Default), so the live gates fall back to their unwrap_or defaults.
        let config: StateModuleConfig = serde_json::from_value(json!({})).unwrap();
        assert!(config.triggers_enabled.is_none());
    }

    #[test]
    fn deny_unknown_fields_rejects_typos() {
        let result: Result<StateModuleConfig, _> =
            serde_json::from_value(json!({ "triggers_enabledd": true }));
        assert!(result.is_err(), "unknown top-level field must be rejected");
    }

    #[test]
    fn normalized_zeroes_out_invalid_knobs() {
        let config = StateModuleConfig {
            max_value_bytes: Some(0),
            save_interval_ms: Some(0),
            ..Default::default()
        }
        .normalized();
        assert!(config.max_value_bytes.is_none());
        assert!(config.save_interval_ms.is_none());
    }

    #[test]
    fn schema_has_bounds_descriptions_and_permissive_adapter() {
        let schema =
            serde_json::to_value(schemars::schema_for!(StateModuleConfig)).expect("schema");
        let props = &schema["properties"];
        assert_eq!(props["max_value_bytes"]["minimum"], json!(1.0));
        assert_eq!(props["save_interval_ms"]["minimum"], json!(100.0));
        assert_eq!(props["save_interval_ms"]["maximum"], json!(3_600_000.0));
        assert!(props["triggers_enabled"]["description"].is_string());
        // deny_unknown_fields flows into the schema.
        assert_eq!(schema["additionalProperties"], json!(false));
        // The adapter stays present but loosely-schema'd so adapters remain
        // pluggable (its inner `config` does not constrain shape).
        assert!(props["adapter"].is_object());
    }
}
