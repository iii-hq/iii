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

/// Runtime configuration for the `iii-stream` WebSocket server.
///
/// Consumed by the builtin `configuration` worker: the config.yaml block seeds
/// this entry on first boot, after which the configuration entry is the runtime
/// source of truth. `host`/`port` and `adapter` hot-apply at runtime (the
/// server rebinds; the pub/sub backend is hot-swapped); `auth_function` applies
/// to new connections.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StreamModuleConfig {
    /// TCP port the WebSocket server binds to. Defaults to `3112`. A change
    /// rebinds the listener at runtime (dropping live connections on the old
    /// address).
    #[serde(default = "default_port")]
    pub port: u16,

    /// Host/interface the WebSocket server binds to. Defaults to `0.0.0.0`
    /// (all interfaces). A change rebinds the listener at runtime.
    #[serde(default = "default_host")]
    pub host: String,

    /// Optional function id invoked at connection upgrade to authenticate a
    /// client. A change applies to new connections only.
    #[serde(default)]
    pub auth_function: Option<String>,

    /// Pub/sub backend for stream distribution. Defaults to the built-in `kv`
    /// adapter; use `redis` (or `bridge`) for multi-instance deployments. A
    /// change hot-swaps the backend: new connections use it, while existing
    /// connections remain bound to the previous backend until they close.
    ///
    /// Advertised as a discriminated union keyed on `name` (see
    /// [`stream_adapter_schema`]) so the console renders per-adapter fields; the
    /// field type stays the loosely-typed `AdapterEntry` so a hand-edited
    /// persisted file is still tolerated at boot.
    ///
    /// `skip_serializing_if` matters: the closed `oneOf` schema has no null
    /// branch, so an absent adapter must be omitted (not serialized as `null`)
    /// or `configuration::register`/`set` would reject the seed value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "stream_adapter_schema")]
    pub adapter: Option<AdapterEntry>,
}

fn default_port() -> u16 {
    3112
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

impl Default for StreamModuleConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            adapter: None,
            auth_function: None,
        }
    }
}

/// Storage backend for the file-backed `kv` adapter: `in_memory` (volatile,
/// process-lifetime storage, lost on shutdown — not for production) or
/// `file_based` (persisted under `file_path`, flushed on the `save_interval_ms`
/// cadence). Variants are intentionally doc-free so schemars emits a flat string
/// `enum` (a single select) rather than a per-variant `oneOf` that a
/// schema-driven UI renders as "variant 1", "variant 2".
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KvStoreMethod {
    InMemory,
    FileBased,
}

/// Configuration for the built-in `kv` stream adapter, which backs both storage
/// and in-process pub/sub delivery.
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
    /// in-memory stores ignore it. Defaults to 5000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 100, max = 3_600_000))]
    pub save_interval_ms: Option<u64>,

    /// Capacity of the in-process broadcast channel backing pub/sub delivery.
    /// Defaults to 256. Raise it if bursts of stream events outrun slow
    /// subscribers (a full channel drops the oldest undelivered events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 1_048_576))]
    pub channel_size: Option<u64>,
}

/// Configuration for the built-in `redis` stream adapter.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct RedisAdapterConfig {
    /// Redis connection URL. Defaults to `redis://localhost:6379`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis_url: Option<String>,
}

/// Configuration for the built-in `bridge` stream adapter.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct BridgeAdapterConfig {
    /// WebSocket bridge URL of the remote stream backend; all stream pub/sub is
    /// forwarded there. Defaults to `ws://localhost:49134`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge_url: Option<String>,
}

/// Build the `oneOf` schema for [`StreamModuleConfig::adapter`]: one branch per
/// built-in adapter, each pinned to its `name` discriminator and carrying that
/// adapter's concrete `config` schema. The set is closed — `configuration::set`
/// rejects any other adapter name — so the console renders per-adapter fields
/// instead of a free-form object. Deserialization stays permissive via the
/// `AdapterEntry` field type, so a hand-edited persisted file is still tolerated
/// at boot.
fn stream_adapter_schema(generator: &mut SchemaGenerator) -> Schema {
    let branches = vec![
        adapter_branch("kv", generator.subschema_for::<KvAdapterConfig>()),
        adapter_branch("redis", generator.subschema_for::<RedisAdapterConfig>()),
        adapter_branch("bridge", generator.subschema_for::<BridgeAdapterConfig>()),
    ];

    let mut schema = SchemaObject::default();
    schema.metadata().description = Some(
        "Pub/sub backend for stream distribution, advertised as a discriminated \
         union keyed on `name` over the built-in adapters `kv` (default), `redis`, \
         and `bridge`. A change hot-swaps the backend: new connections use it, while \
         existing connections remain bound to the previous backend until they close."
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

    #[test]
    fn default_values() {
        let config = StreamModuleConfig::default();
        assert_eq!(config.port, 3112);
        assert_eq!(config.host, "0.0.0.0");
        assert!(config.auth_function.is_none());
        assert!(config.adapter.is_none());
    }

    #[test]
    fn deserialize_empty_json() {
        let config: StreamModuleConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.port, 3112);
        assert_eq!(config.host, "0.0.0.0");
        assert!(config.auth_function.is_none());
        assert!(config.adapter.is_none());
    }

    #[test]
    fn deserialize_custom_values() {
        let json = r#"{
            "port": 4000,
            "host": "127.0.0.1",
            "auth_function": "my_auth_fn"
        }"#;
        let config: StreamModuleConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.port, 4000);
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.auth_function, Some("my_auth_fn".to_string()));
    }

    #[test]
    fn deserialize_with_adapter() {
        let json = r#"{
            "adapter": {
                "name": "my_adapter::StreamAdapter",
                "config": {"key": "value"}
            }
        }"#;
        let config: StreamModuleConfig = serde_json::from_str(json).unwrap();
        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.name, "my_adapter::StreamAdapter");
        assert!(adapter.config.is_some());
    }

    #[test]
    fn deny_unknown_fields() {
        let json = r#"{"port": 3112, "unknown": true}"#;
        let result: Result<StreamModuleConfig, _> = serde_json::from_str(json);
        assert!(result.is_err(), "should deny unknown fields");
    }

    #[test]
    fn serialize_roundtrip() {
        let config = StreamModuleConfig {
            port: 5000,
            host: "localhost".to_string(),
            auth_function: Some("auth".to_string()),
            adapter: None,
        };
        let json_str = serde_json::to_string(&config).unwrap();
        let deserialized: StreamModuleConfig = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.port, 5000);
        assert_eq!(deserialized.host, "localhost");
        assert_eq!(deserialized.auth_function, Some("auth".to_string()));
    }

    #[test]
    fn from_yaml_with_defaults() {
        let yaml = "{}";
        let config: StreamModuleConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.port, 3112);
        assert_eq!(config.host, "0.0.0.0");
    }

    #[test]
    fn from_yaml_custom() {
        let yaml = r#"
port: 7777
host: "10.0.0.1"
auth_function: "check_auth"
"#;
        let config: StreamModuleConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.port, 7777);
        assert_eq!(config.host, "10.0.0.1");
        assert_eq!(config.auth_function, Some("check_auth".to_string()));
    }

    #[test]
    fn schema_denies_unknown_fields_and_documents_fields() {
        let schema = serde_json::to_value(schemars::schema_for!(StreamModuleConfig)).unwrap();

        // `deny_unknown_fields` must flow into the schema so `configuration::set`
        // rejects typo'd keys (e.g. `prt`) at set time rather than silently
        // ignoring them.
        assert_eq!(
            schema["additionalProperties"],
            serde_json::json!(false),
            "schema must deny unknown fields: {schema}"
        );

        // Field doc comments must become schema descriptions so an agent
        // introspecting the config sees intent, not just types.
        assert!(
            schema["properties"]["port"]["description"].is_string(),
            "port field must carry a schema description: {schema}"
        );
        assert!(
            schema["properties"]["adapter"]["description"].is_string(),
            "adapter field must carry a schema description: {schema}"
        );
    }

    #[test]
    fn schema_advertises_per_adapter_oneof() {
        let schema = serde_json::to_value(schemars::schema_for!(StreamModuleConfig)).unwrap();

        // The adapter is a discriminated union over the built-in adapters, keyed
        // on `name`, each branch carrying a concrete `config` schema — so the
        // console renders per-adapter fields instead of "unsupported schema".
        let adapter = &schema["properties"]["adapter"];
        let branches = adapter["oneOf"].as_array().expect("adapter oneOf");
        assert_eq!(branches.len(), 3);
        let mut names: Vec<&str> = branches
            .iter()
            .map(|b| {
                assert_eq!(b["additionalProperties"], serde_json::json!(false));
                assert!(b["properties"]["config"].is_object());
                let name = b["properties"]["name"]["enum"][0]
                    .as_str()
                    .expect("name discriminator");
                // The branch `title` drives the console's adapter dropdown label
                // (otherwise every option shows "object").
                assert_eq!(b["title"].as_str(), Some(name));
                name
            })
            .collect();
        names.sort_unstable();
        assert_eq!(names, vec!["bridge", "kv", "redis"]);

        // Each adapter's concrete config fields land in definitions, including
        // the stream-specific `channel_size` on the kv adapter.
        let defs = &schema["definitions"];
        assert!(defs["KvAdapterConfig"]["properties"]["store_method"].is_object());
        assert!(defs["KvAdapterConfig"]["properties"]["channel_size"].is_object());
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
        let schema = serde_json::to_value(schemars::schema_for!(StreamModuleConfig)).unwrap();
        let validator = jsonschema::Validator::new(&schema).expect("schema compiles");

        // Accepted: known adapters, with or without config.
        assert!(validator.is_valid(&serde_json::json!({ "adapter": { "name": "kv" } })));
        assert!(validator.is_valid(&serde_json::json!({
            "adapter": { "name": "kv", "config": { "store_method": "file_based", "channel_size": 1024 } }
        })));
        assert!(validator.is_valid(&serde_json::json!({
            "adapter": { "name": "redis", "config": { "redis_url": "redis://localhost:6379" } }
        })));
        assert!(validator.is_valid(&serde_json::json!({
            "adapter": { "name": "bridge", "config": { "bridge_url": "ws://localhost:49134" } }
        })));

        // Rejected: unknown adapter, invalid enum, unknown config key,
        // out-of-range channel_size, and the stale `url` key (redis reads
        // `redis_url`).
        assert!(!validator.is_valid(&serde_json::json!({ "adapter": { "name": "postgres" } })));
        assert!(!validator.is_valid(&serde_json::json!({
            "adapter": { "name": "kv", "config": { "store_method": "weird" } }
        })));
        assert!(!validator.is_valid(&serde_json::json!({
            "adapter": { "name": "kv", "config": { "bogus": 1 } }
        })));
        assert!(!validator.is_valid(&serde_json::json!({
            "adapter": { "name": "kv", "config": { "channel_size": 0 } }
        })));
        assert!(!validator.is_valid(&serde_json::json!({
            "adapter": { "name": "redis", "config": { "url": "redis://localhost" } }
        })));
    }
}
