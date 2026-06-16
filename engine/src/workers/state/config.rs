// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use schemars::JsonSchema;
use schemars::r#gen::SchemaGenerator;
use schemars::schema::{InstanceType, Schema, SchemaObject};
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
    /// Storage adapter selection and its adapter-specific config, advertised as
    /// a discriminated union keyed on `name` over the built-in adapters `kv`
    /// (default), `redis`, and `bridge`. Restart-tier: changing it at runtime
    /// is logged and takes effect at the next engine start (the persisted entry
    /// is read at boot). The field keeps the loosely-typed `AdapterEntry` for
    /// deserialization so a hand-edited persisted file is tolerated, while
    /// `configuration::set` validates against the concrete per-adapter schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "state_adapter_schema")]
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

/// Storage backend for the file-backed `kv` adapter: `in_memory` (volatile,
/// process-lifetime storage, lost on shutdown — not for production) or
/// `file_based` (persisted under `file_path`, flushed on the `save_interval_ms`
/// cadence). Variants are intentionally doc-free so schemars emits a flat
/// string `enum` (a single select) rather than a per-variant `oneOf` that a
/// schema-driven UI renders as "variant 1", "variant 2".
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KvStoreMethod {
    InMemory,
    FileBased,
}

/// Configuration for the built-in `kv` storage adapter.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct KvAdapterConfig {
    /// Storage backend. `in_memory` (the default) keeps data only for the
    /// process lifetime; `file_based` persists it under `file_path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_method: Option<KvStoreMethod>,

    /// Directory for file-based storage. Only used when `store_method` is
    /// `file_based`. Defaults to `kv_store_data.db`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,

    /// Persistence flush cadence in milliseconds for file-based storage;
    /// in-memory stores ignore it. Defaults to 5000. The top-level
    /// `save_interval_ms` overrides this at construction time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 100, max = 3_600_000))]
    pub save_interval_ms: Option<u64>,
}

/// Configuration for the built-in `redis` storage adapter.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct RedisAdapterConfig {
    /// Redis connection URL. Defaults to `redis://localhost:6379`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis_url: Option<String>,
}

/// Configuration for the built-in `bridge` storage adapter.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct BridgeAdapterConfig {
    /// WebSocket bridge URL of the remote state backend; all state operations
    /// are forwarded there. Defaults to `ws://localhost:49134`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge_url: Option<String>,
}

/// Build the `oneOf` schema for [`StateModuleConfig::adapter`]: one branch per
/// built-in adapter, each pinned to its `name` discriminator and carrying that
/// adapter's concrete `config` schema. The set is closed — `configuration::set`
/// rejects any other adapter name — so the console renders per-adapter fields
/// instead of a free-form object. Deserialization stays permissive via the
/// `AdapterEntry` field type, so a hand-edited persisted file is still tolerated
/// at boot.
fn state_adapter_schema(generator: &mut SchemaGenerator) -> Schema {
    let branches = vec![
        adapter_branch("kv", generator.subschema_for::<KvAdapterConfig>()),
        adapter_branch("redis", generator.subschema_for::<RedisAdapterConfig>()),
        adapter_branch("bridge", generator.subschema_for::<BridgeAdapterConfig>()),
    ];

    let mut schema = SchemaObject::default();
    schema.metadata().description = Some(
        "Storage adapter selection and its adapter-specific config, advertised as a \
         discriminated union keyed on `name` over the built-in adapters `kv` (default), \
         `redis`, and `bridge`. Restart-tier: changing it at runtime is logged and takes \
         effect at the next engine start (the persisted entry is read at boot)."
            .to_string(),
    );
    schema.subschemas().one_of = Some(branches);
    Schema::Object(schema)
}

/// One `oneOf` branch: an object pinned to `name` and carrying the adapter's
/// `config` sub-schema. `config` is optional (every adapter has working
/// defaults) and no other keys are permitted.
fn adapter_branch(name: &str, config_schema: Schema) -> Schema {
    let name_schema = SchemaObject {
        instance_type: Some(InstanceType::String.into()),
        enum_values: Some(vec![serde_json::Value::String(name.to_string())]),
        ..Default::default()
    };

    let mut branch = SchemaObject {
        instance_type: Some(InstanceType::Object.into()),
        ..Default::default()
    };
    // The console labels each `oneOf` option by its `title`; without it the
    // form shows the bare type ("object") for every adapter branch.
    branch.metadata().title = Some(name.to_string());
    {
        let object = branch.object();
        object
            .properties
            .insert("name".to_string(), Schema::Object(name_schema));
        object
            .properties
            .insert("config".to_string(), config_schema);
        object.required.insert("name".to_string());
        object.additional_properties = Some(Box::new(Schema::Bool(false)));
    }
    Schema::Object(branch)
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
    fn schema_has_bounds_descriptions_and_per_adapter_oneof() {
        let schema =
            serde_json::to_value(schemars::schema_for!(StateModuleConfig)).expect("schema");
        let props = &schema["properties"];
        assert_eq!(props["max_value_bytes"]["minimum"], json!(1.0));
        assert_eq!(props["save_interval_ms"]["minimum"], json!(100.0));
        assert_eq!(props["save_interval_ms"]["maximum"], json!(3_600_000.0));
        assert!(props["triggers_enabled"]["description"].is_string());
        // deny_unknown_fields flows into the schema.
        assert_eq!(schema["additionalProperties"], json!(false));

        // The adapter is a discriminated union over the built-in adapters,
        // keyed on `name`, each branch carrying a concrete `config` schema.
        let adapter = &props["adapter"];
        assert!(adapter["description"].is_string());
        let branches = adapter["oneOf"].as_array().expect("adapter oneOf");
        assert_eq!(branches.len(), 3);
        let mut names: Vec<&str> = branches
            .iter()
            .map(|b| {
                assert_eq!(b["additionalProperties"], json!(false));
                assert!(b["properties"]["config"].is_object());
                let name = b["properties"]["name"]["enum"][0]
                    .as_str()
                    .expect("name discriminator");
                // The branch `title` drives the console's adapter dropdown
                // label (otherwise every option shows "object").
                assert_eq!(b["title"].as_str(), Some(name));
                name
            })
            .collect();
        names.sort_unstable();
        assert_eq!(names, vec!["bridge", "kv", "redis"]);

        // Each adapter's concrete config fields land in definitions.
        let defs = &schema["definitions"];
        assert!(defs["KvAdapterConfig"]["properties"]["store_method"].is_object());
        assert_eq!(
            defs["KvAdapterConfig"]["properties"]["save_interval_ms"]["minimum"],
            json!(100.0)
        );
        assert!(defs["RedisAdapterConfig"]["properties"]["redis_url"].is_object());
        assert!(defs["BridgeAdapterConfig"]["properties"]["bridge_url"].is_object());

        // `store_method` is a flat string enum (a single select), not a
        // per-variant `oneOf` that renders as "variant 1"/"variant 2".
        let store_method = &defs["KvStoreMethod"];
        assert!(
            store_method["oneOf"].is_null(),
            "store_method must be a flat enum, not a oneOf"
        );
        let methods: Vec<&str> = store_method["enum"]
            .as_array()
            .expect("store_method enum values")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(methods.contains(&"in_memory") && methods.contains(&"file_based"));
    }

    // Closed-schema enforcement through the real validator (with `$ref`
    // resolution) — the same `jsonschema` path `configuration::set` uses.
    #[test]
    fn closed_adapter_schema_accepts_known_rejects_unknown() {
        let schema =
            serde_json::to_value(schemars::schema_for!(StateModuleConfig)).expect("schema");
        let validator = jsonschema::Validator::new(&schema).expect("schema compiles");

        // Accepted: known adapters, with or without config.
        assert!(validator.is_valid(&json!({
            "adapter": {"name": "kv", "config": {"store_method": "file_based", "file_path": "./data/state_store.db"}}
        })));
        assert!(validator.is_valid(&json!({"adapter": {"name": "kv"}})));
        assert!(validator.is_valid(&json!({
            "adapter": {"name": "redis", "config": {"redis_url": "redis://localhost:6379"}}
        })));
        assert!(validator.is_valid(&json!({
            "adapter": {"name": "bridge", "config": {"bridge_url": "ws://localhost:49134"}}
        })));

        // Rejected: unknown adapter, invalid enum, unknown config key, and the
        // stale `url` key (redis reads `redis_url`).
        assert!(!validator.is_valid(&json!({"adapter": {"name": "postgres"}})));
        assert!(!validator.is_valid(&json!({
            "adapter": {"name": "kv", "config": {"store_method": "weird"}}
        })));
        assert!(!validator.is_valid(&json!({
            "adapter": {"name": "kv", "config": {"bogus": 1}}
        })));
        assert!(!validator.is_valid(&json!({
            "adapter": {"name": "redis", "config": {"url": "redis://localhost"}}
        })));
    }
}
