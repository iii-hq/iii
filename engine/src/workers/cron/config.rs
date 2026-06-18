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

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CronModuleConfig {
    /// The distributed-lock backend used to coordinate cron job execution across
    /// engine instances, advertised as a discriminated union keyed on `name` over
    /// the built-in adapters `kv` (default, process-local) and `redis`
    /// (distributed). Omit to use the default `kv` adapter. Changing this at
    /// runtime hot-swaps the lock backend and re-schedules every live cron job
    /// onto the new transport. The field keeps the loosely-typed `AdapterEntry`
    /// for deserialization so a hand-edited persisted file is tolerated, while
    /// `configuration::set` validates against the concrete per-adapter schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "cron_adapter_schema")]
    pub adapter: Option<AdapterEntry>,
}

/// Build the `oneOf` schema for [`CronModuleConfig::adapter`]: one branch per
/// built-in cron lock backend, each pinned to its `name` discriminator and
/// carrying that adapter's concrete `config` schema. The set is closed —
/// `configuration::set` rejects any other adapter name — so the console renders
/// per-adapter fields instead of a free-form object. Deserialization stays
/// permissive via the `AdapterEntry` field type, so a hand-edited persisted file
/// is still tolerated at boot.
fn cron_adapter_schema(generator: &mut SchemaGenerator) -> Schema {
    let branches = vec![
        adapter_branch("kv", generator.subschema_for::<KvAdapterConfig>()),
        adapter_branch("redis", generator.subschema_for::<RedisAdapterConfig>()),
    ];

    let mut schema = SchemaObject::default();
    schema.metadata().description = Some(
        "Distributed-lock backend used to coordinate cron job execution across \
         engine instances, advertised as a discriminated union keyed on `name` over \
         the built-in adapters `kv` (default; process-local locks, single-instance \
         only) and `redis` (distributed locks for multi-instance deployments). \
         Changing it at runtime hot-swaps the lock backend and re-registers every \
         live cron job onto it."
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

/// Storage backend for the `kv` lock adapter's underlying key-value store:
/// `in_memory` (volatile, process-lifetime, lost on shutdown — not for
/// production) or `file_based` (persisted under `file_path`). Variants are
/// intentionally doc-free so schemars emits a flat string `enum` (a single
/// select) rather than a per-variant `oneOf` that a schema-driven UI renders as
/// "variant 1", "variant 2".
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KvStoreMethod {
    InMemory,
    FileBased,
}

/// Configuration for the built-in `kv` lock adapter. The lock is
/// **process-local** (suitable for single-instance deployments); use `redis`
/// for distributed locking. The underlying key-value store fields
/// (`store_method` / `file_path` / `save_interval_ms`) are honored by the first
/// `kv` adapter built in the process, which constructs the shared store.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct KvAdapterConfig {
    /// Storage backend for the lock store. `in_memory` (the default) keeps locks
    /// only for the process lifetime; `file_based` persists them under
    /// `file_path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_method: Option<KvStoreMethod>,

    /// Directory for file-based storage. Only used when `store_method` is
    /// `file_based`. Defaults to `kv_store_data.db`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,

    /// Persistence flush cadence in milliseconds for file-based storage;
    /// in-memory stores ignore it. Defaults to 5000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 100, max = 3_600_000))]
    pub save_interval_ms: Option<u64>,

    /// Time-to-live of an acquired cron lock, in milliseconds. A held lock is
    /// auto-released after this window so a crashed instance cannot block a job
    /// forever. Defaults to 30000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub lock_ttl_ms: Option<u64>,

    /// Key-value index (namespace) under which cron locks are stored. Defaults
    /// to `cron_locks`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_index: Option<String>,
}

/// Configuration for the built-in `redis` lock adapter — a **distributed** lock
/// safe across multiple engine instances. The lock TTL and key prefix are fixed
/// by the adapter and are not configurable.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct RedisAdapterConfig {
    /// Redis connection URL. Defaults to `redis://localhost:6379`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = CronModuleConfig::default();
        assert!(config.adapter.is_none());
    }

    #[test]
    fn deserialize_empty_json() {
        let config: CronModuleConfig = serde_json::from_str("{}").unwrap();
        assert!(config.adapter.is_none());
    }

    #[test]
    fn deserialize_with_adapter() {
        let json = r#"{"adapter": {"name": "my::CronAdapter", "config": {"key": "val"}}}"#;
        let config: CronModuleConfig = serde_json::from_str(json).unwrap();
        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.name, "my::CronAdapter");
        assert!(adapter.config.is_some());
    }

    #[test]
    fn deserialize_adapter_no_config() {
        let json = r#"{"adapter": {"name": "cron::Adapter"}}"#;
        let config: CronModuleConfig = serde_json::from_str(json).unwrap();
        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.name, "cron::Adapter");
        assert!(adapter.config.is_none());
    }

    #[test]
    fn deny_unknown_fields() {
        let json = r#"{"unknown": true}"#;
        let result: Result<CronModuleConfig, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn serialize_roundtrip() {
        let config = CronModuleConfig {
            adapter: Some(AdapterEntry {
                name: "test::Adapter".to_string(),
                config: Some(serde_json::json!({"interval": 60})),
            }),
        };
        let json_str = serde_json::to_string(&config).unwrap();
        let deserialized: CronModuleConfig = serde_json::from_str(&json_str).unwrap();
        let adapter = deserialized.adapter.unwrap();
        assert_eq!(adapter.name, "test::Adapter");
        assert_eq!(adapter.config.unwrap()["interval"], 60);
    }

    #[test]
    fn schema_advertises_per_adapter_oneof() {
        let schema = serde_json::to_value(schemars::schema_for!(CronModuleConfig)).unwrap();
        // `deny_unknown_fields` flows into the schema so `configuration::set`
        // rejects a typo'd top-level key at write time.
        assert_eq!(
            schema["additionalProperties"],
            serde_json::json!(false),
            "schema must deny unknown fields: {schema}"
        );

        // The adapter is a discriminated union over the built-in cron lock
        // backends, keyed on `name`, each branch carrying a concrete `config`.
        let adapter = &schema["properties"]["adapter"];
        assert!(
            adapter["description"].is_string(),
            "adapter must carry a schema description: {schema}"
        );
        let branches = adapter["oneOf"].as_array().expect("adapter oneOf");
        assert_eq!(
            branches.len(),
            2,
            "one branch per built-in adapter: {schema}"
        );
        let mut names: Vec<&str> = branches
            .iter()
            .map(|b| {
                assert_eq!(b["additionalProperties"], serde_json::json!(false));
                assert!(b["properties"]["config"].is_object());
                let name = b["properties"]["name"]["enum"][0]
                    .as_str()
                    .expect("name discriminator");
                // The branch `title` drives the console's adapter dropdown label.
                assert_eq!(b["title"].as_str(), Some(name));
                name
            })
            .collect();
        names.sort_unstable();
        assert_eq!(names, vec!["kv", "redis"]);

        // Each adapter's concrete config fields land in definitions.
        let defs = &schema["definitions"];
        assert!(defs["KvAdapterConfig"]["properties"]["lock_index"].is_object());
        assert_eq!(
            defs["KvAdapterConfig"]["properties"]["save_interval_ms"]["minimum"],
            serde_json::json!(100.0)
        );
        assert!(defs["RedisAdapterConfig"]["properties"]["redis_url"].is_object());
        // The redis lock TTL is hardcoded, so it must not be a configurable field.
        assert!(
            defs["RedisAdapterConfig"]["properties"]["lock_ttl_ms"].is_null(),
            "redis adapter must not advertise a configurable lock TTL: {schema}"
        );

        // `store_method` is a flat string enum (a single select), not a
        // per-variant `oneOf` that renders as "variant 1"/"variant 2".
        assert!(
            defs["KvStoreMethod"]["oneOf"].is_null(),
            "store_method must be a flat enum, not a oneOf: {schema}"
        );
    }

    // Closed-schema enforcement through the real validator (with `$ref`
    // resolution) — the same `jsonschema` path `configuration::set` uses.
    #[test]
    fn closed_adapter_schema_accepts_known_rejects_unknown() {
        let schema = serde_json::to_value(schemars::schema_for!(CronModuleConfig)).unwrap();
        let validator = jsonschema::Validator::new(&schema).expect("schema compiles");

        // Accepted: known adapters, with or without config, and the default
        // (no adapter) which serializes to an absent key, not `null`.
        assert!(validator.is_valid(&serde_json::json!({})));
        assert!(validator.is_valid(&serde_json::json!({ "adapter": { "name": "kv" } })));
        assert!(validator.is_valid(&serde_json::json!({
            "adapter": { "name": "kv", "config": { "lock_index": "cron_locks", "store_method": "file_based" } }
        })));
        assert!(validator.is_valid(&serde_json::json!({
            "adapter": { "name": "redis", "config": { "redis_url": "redis://localhost:6379" } }
        })));

        // Rejected: unknown adapter name, bad enum value, unknown config key,
        // and a redis key the adapter does not read (its TTL is hardcoded).
        assert!(
            !validator.is_valid(&serde_json::json!({ "adapter": { "name": "does-not-exist" } }))
        );
        assert!(!validator.is_valid(&serde_json::json!({
            "adapter": { "name": "kv", "config": { "store_method": "weird" } }
        })));
        assert!(!validator.is_valid(&serde_json::json!({
            "adapter": { "name": "kv", "config": { "bogus": 1 } }
        })));
        assert!(!validator.is_valid(&serde_json::json!({
            "adapter": { "name": "redis", "config": { "lock_ttl_ms": 5000 } }
        })));
    }
}
