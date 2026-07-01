// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

use schemars::JsonSchema;
use schemars::r#gen::SchemaGenerator;
use schemars::schema::{InstanceType, Schema, SchemaObject};
use serde::{Deserialize, Serialize};

use crate::workers::traits::AdapterEntry;

/// Name of the always-present built-in queue. The engine provisions this queue
/// with standard defaults during config load, so `TriggerAction::Enqueue {
/// queue: "default" }` works with no `queue_configs` entry — a zero-config
/// durable queue out of the box. An explicit `default` entry in user config
/// takes precedence and is never overwritten.
pub const DEFAULT_QUEUE_NAME: &str = "default";

#[allow(dead_code)] // this is used as default value
fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

fn default_max_retries() -> u32 {
    3
}

fn default_concurrency() -> u32 {
    10
}

fn default_queue_type() -> String {
    "standard".to_string()
}

fn default_backoff_ms() -> u64 {
    1000
}

fn default_poll_interval_ms() -> u64 {
    100
}

/// Per-queue settings. Doc comments on each field flow into the JSON Schema
/// (via `schemars`) that the `iii-queue` configuration entry registers, so an
/// agent introspecting the schema sees the same descriptions and defaults
/// documented here.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FunctionQueueConfig {
    /// Maximum delivery attempts before a message is sent to the dead-letter
    /// queue. Defaults to 3.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Number of messages processed concurrently. For `fifo` queues the
    /// effective prefetch is forced to 1 to preserve ordering. Defaults to 10.
    /// Must be at least 1 — `concurrency: 0` would build a zero-capacity
    /// channel/semaphore and panic the consumer.
    #[serde(default = "default_concurrency")]
    #[schemars(range(min = 1))]
    pub concurrency: u32,

    /// Delivery semantics: "standard" (concurrent, unordered) or "fifo"
    /// (ordered per message group; requires `message_group_field`).
    /// Defaults to "standard".
    #[serde(default = "default_queue_type", rename = "type")]
    pub r#type: String,

    /// For `fifo` queues, the message field whose value defines the ordering
    /// group. Required when `type` is "fifo"; ignored otherwise.
    #[serde(default)]
    pub message_group_field: Option<String>,

    /// Declares the queue as a RabbitMQ priority queue with this many priority
    /// levels (`x-max-priority`, 1–255; RabbitMQ recommends ≤ 10). When unset
    /// the queue is not a priority queue. RabbitMQ-only — other adapters ignore
    /// it. `x-max-priority` is fixed at queue-creation time: changing it for an
    /// already-declared queue has no effect (the queue must be recreated).
    /// Priority ordering is only observable at low `concurrency`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 255))]
    pub max_priority: Option<u8>,

    /// The message field whose integer value sets each message's priority
    /// (0..=`max_priority`; the broker clamps out-of-range values). Analogous to
    /// `message_group_field`. Only meaningful when `max_priority` is set;
    /// RabbitMQ-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority_field: Option<String>,

    /// Base delay in milliseconds for the exponential retry backoff. Defaults
    /// to 1000 (1s).
    #[serde(default = "default_backoff_ms")]
    pub backoff_ms: u64,

    /// Poll interval in milliseconds for adapters that poll for messages.
    /// Defaults to 100.
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
}

impl Default for FunctionQueueConfig {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            concurrency: default_concurrency(),
            r#type: default_queue_type(),
            message_group_field: None,
            max_priority: None,
            priority_field: None,
            backoff_ms: default_backoff_ms(),
            poll_interval_ms: default_poll_interval_ms(),
        }
    }
}

/// Queue worker settings. Doc comments flow into the JSON Schema (via
/// `schemars`) that the `iii-queue` configuration entry registers. After first
/// boot this entry is the runtime source of truth; the `config.yaml` block is
/// seed-only.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct QueueModuleConfig {
    /// Transport backing the queues. When omitted, the built-in in-process
    /// adapter is used. Changing this at runtime re-instantiates the transport
    /// and restarts every consumer. The field keeps the loosely-typed
    /// `AdapterEntry` for deserialization (a hand-edited persisted file is
    /// tolerated at boot), while the schema (via [`queue_adapter_schema`])
    /// advertises a closed per-adapter union so the console renders typed fields.
    ///
    /// `skip_serializing_if` omits the field entirely when unset, so a
    /// no-adapter config is not serialized as `adapter: null` (which the `oneOf`
    /// schema, having no null branch, would reject at `configuration::set`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "queue_adapter_schema")]
    pub adapter: Option<AdapterEntry>,

    /// Named queues keyed by queue name, each with its own retry, concurrency,
    /// and ordering settings. The built-in `default` queue is always present.
    #[serde(default)]
    pub queue_configs: HashMap<String, FunctionQueueConfig>,
}

impl QueueModuleConfig {
    /// Ensure the built-in [`DEFAULT_QUEUE_NAME`] queue exists. Called once
    /// after config load (see `QueueWorker::build`) so callers get a durable
    /// queue without declaring it in `config.yaml`. A user-supplied `default`
    /// entry is preserved — `or_default` only inserts when the key is absent —
    /// so operators can still tune its retries/concurrency/type if they want.
    pub fn ensure_default_queue(&mut self) {
        self.queue_configs
            .entry(DEFAULT_QUEUE_NAME.to_string())
            .or_default();
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, queue_config) in &self.queue_configs {
            if queue_config.r#type != "standard" && queue_config.r#type != "fifo" {
                anyhow::bail!(
                    "Queue '{}' has invalid type '{}'. Must be 'standard' or 'fifo'",
                    name,
                    queue_config.r#type
                );
            }
            if queue_config.r#type == "fifo" && queue_config.message_group_field.is_none() {
                anyhow::bail!(
                    "Queue '{}' is of type 'fifo' but 'message_group_field' is not set",
                    name
                );
            }
            // `concurrency` is the channel/semaphore capacity for the consumer;
            // 0 would panic `mpsc::channel(0)` / wedge `Semaphore::new(0)`. The
            // JSON schema (`range(min = 1)`) rejects this at `configuration::set`,
            // but the `config.yaml` seed bypasses the schema, so guard here too.
            if queue_config.concurrency == 0 {
                anyhow::bail!(
                    "Queue '{}' has 'concurrency' 0; it must be at least 1",
                    name
                );
            }
            // `x-max-priority` must be a positive integer; 0 is rejected by the
            // broker. The JSON schema (`range(min = 1)`) catches this at
            // `configuration::set`, but the `config.yaml` seed bypasses the
            // schema, so guard here too.
            if queue_config.max_priority == Some(0) {
                anyhow::bail!(
                    "Queue '{}' has 'max_priority' 0; it must be between 1 and 255",
                    name
                );
            }
        }
        Ok(())
    }
}

/// Storage backend for the built-in adapter's kv persistence. Variants are
/// intentionally doc-free so schemars emits a flat string `enum` (a single
/// select) rather than a per-variant `oneOf` a schema-driven UI renders as
/// "variant 1"/"variant 2".
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueStoreMethod {
    InMemory,
    FileBased,
}

/// Config for the built-in in-process `builtin` adapter. Per-queue retry,
/// concurrency, and ordering live in `queue_configs`; this only configures the
/// adapter's kv persistence.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct BuiltinAdapterConfig {
    /// Storage backend. `in_memory` (the default) keeps queued messages only for
    /// the process lifetime; `file_based` persists them under `file_path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_method: Option<QueueStoreMethod>,

    /// Directory for file-based storage. Only used when `store_method` is
    /// `file_based`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,

    /// Persistence flush cadence in milliseconds for file-based storage;
    /// in-memory stores ignore it. Defaults to 5000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 100, max = 3_600_000))]
    pub save_interval_ms: Option<u64>,
}

/// Config for the `redis` adapter.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct RedisAdapterConfig {
    /// Redis connection URL. Defaults to `redis://localhost:6379`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis_url: Option<String>,
}

/// Config for the `bridge` adapter.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct BridgeAdapterConfig {
    /// WebSocket bridge URL of the remote queue backend; all queue operations
    /// are forwarded there. Defaults to `ws://localhost:49134`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge_url: Option<String>,
}

/// Config for the `rabbitmq` adapter. Only built (and advertised in the schema)
/// when the `rabbitmq` feature is enabled — i.e. when the transport is actually
/// registered.
#[cfg(feature = "rabbitmq")]
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct RabbitmqAdapterConfig {
    /// AMQP connection URL. Defaults to `amqp://localhost:5672`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amqp_url: Option<String>,

    /// Maximum delivery attempts before a message is dead-lettered. Defaults
    /// to 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub max_attempts: Option<u32>,

    /// Per-consumer prefetch (QoS) window. Defaults to 10.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub prefetch_count: Option<u16>,

    /// Default delivery mode: "standard" (concurrent) or "fifo" (ordered).
    /// Per-queue `type` overrides this. Defaults to "standard".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_mode: Option<String>,

    /// Message field whose integer value sets the priority of messages published
    /// to **topics** (pub/sub fanout). A fanout publish is copied to every bound
    /// subscriber queue, so the priority value is resolved once here at publish
    /// time; each subscriber queue honors it only if declared with a
    /// `maxPriority`. Function queues use their own per-queue `priority_field`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority_field: Option<String>,
}

/// Build the `oneOf` schema for [`QueueModuleConfig::adapter`]: one branch per
/// registered transport, each pinned to its `name` discriminator and carrying
/// that adapter's concrete `config` schema. The set is closed —
/// `configuration::set` rejects any other adapter name — so the console renders
/// per-adapter fields instead of a free-form object. Deserialization stays
/// permissive via the `AdapterEntry` field type, so a hand-edited persisted file
/// is still tolerated at boot.
fn queue_adapter_schema(generator: &mut SchemaGenerator) -> Schema {
    #[allow(unused_mut)]
    let mut branches = vec![
        adapter_branch("builtin", generator.subschema_for::<BuiltinAdapterConfig>()),
        adapter_branch("redis", generator.subschema_for::<RedisAdapterConfig>()),
        adapter_branch("bridge", generator.subschema_for::<BridgeAdapterConfig>()),
    ];

    // The rabbitmq transport is only registered under its feature; advertise it
    // in the schema only when it is available so `configuration::set` never
    // accepts an adapter name the engine cannot build.
    #[cfg(feature = "rabbitmq")]
    branches.push(adapter_branch(
        "rabbitmq",
        generator.subschema_for::<RabbitmqAdapterConfig>(),
    ));

    // The `test-adapters` feature registers an in-process `memory` transport (a
    // second dependency-free backend for the hot-swap e2e); advertise it so
    // `configuration::set` accepts it. Open config — an opaque test carrier.
    #[cfg(feature = "test-adapters")]
    branches.push(adapter_branch(
        "memory",
        Schema::Object(SchemaObject {
            instance_type: Some(vec![InstanceType::Object, InstanceType::Null].into()),
            ..Default::default()
        }),
    ));

    let mut schema = SchemaObject::default();
    schema.metadata().description = Some(
        "Transport backing the queues, a discriminated union keyed on `name` over \
         the registered adapters `builtin` (default, in-process), `redis`, and \
         `bridge`. Hot-swap tier: a runtime edit re-instantiates the transport and \
         restarts every consumer — no engine restart. A value that fails to build \
         the transport is gated and keeps the previous one."
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
    // The console labels each `oneOf` option by its `title`; without it the form
    // shows the bare type ("object") for every adapter branch.
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
    fn default_config() {
        let config = QueueModuleConfig::default();
        assert!(config.adapter.is_none());
        assert!(config.queue_configs.is_empty());
    }

    #[test]
    fn ensure_default_queue_provisions_standard_queue_when_absent() {
        let mut config = QueueModuleConfig::default();
        config.ensure_default_queue();

        let default = config
            .queue_configs
            .get(DEFAULT_QUEUE_NAME)
            .expect("default queue should be provisioned");
        // Standard defaults — a plain, concurrent, retrying queue.
        assert_eq!(default.r#type, "standard");
        assert_eq!(default.concurrency, default_concurrency());
        assert_eq!(default.max_retries, default_max_retries());
        assert!(default.message_group_field.is_none());
        // The provisioned default must satisfy validation.
        assert!(config.validate().is_ok());
    }

    #[test]
    fn ensure_default_queue_preserves_explicit_user_config() {
        let mut queue_configs = HashMap::new();
        queue_configs.insert(
            DEFAULT_QUEUE_NAME.to_string(),
            FunctionQueueConfig {
                r#type: "fifo".to_string(),
                message_group_field: Some("session_id".to_string()),
                max_retries: 9,
                concurrency: 1,
                ..Default::default()
            },
        );
        let mut config = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };

        config.ensure_default_queue();

        // An operator who declares `default` keeps their tuned settings.
        let default = config.queue_configs.get(DEFAULT_QUEUE_NAME).unwrap();
        assert_eq!(default.r#type, "fifo");
        assert_eq!(default.max_retries, 9);
        assert_eq!(default.concurrency, 1);
        assert_eq!(default.message_group_field.as_deref(), Some("session_id"));
        assert_eq!(config.queue_configs.len(), 1);
    }

    #[test]
    fn ensure_default_queue_is_idempotent() {
        let mut config = QueueModuleConfig::default();
        config.ensure_default_queue();
        config.ensure_default_queue();
        assert_eq!(config.queue_configs.len(), 1);
    }

    #[test]
    fn deserialize_empty_json() {
        let config: QueueModuleConfig = serde_json::from_str("{}").unwrap();
        assert!(config.adapter.is_none());
        assert!(config.queue_configs.is_empty());
    }

    #[test]
    fn deserialize_with_adapter() {
        let json =
            r#"{"adapter": {"name": "my::QueueAdapter", "config": {"url": "redis://localhost"}}}"#;
        let config: QueueModuleConfig = serde_json::from_str(json).unwrap();
        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.name, "my::QueueAdapter");
        assert!(adapter.config.is_some());
    }

    #[test]
    fn deserialize_adapter_no_config() {
        let json = r#"{"adapter": {"name": "my::QueueAdapter"}}"#;
        let config: QueueModuleConfig = serde_json::from_str(json).unwrap();
        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.name, "my::QueueAdapter");
        assert!(adapter.config.is_none());
    }

    #[test]
    fn allows_queue_configs_field() {
        let json = r#"{"adapter": null, "queue_configs": {}}"#;
        let result: Result<QueueModuleConfig, _> = serde_json::from_str(json);
        assert!(result.is_ok());
    }

    #[test]
    fn queue_module_config_deny_unknown_fields() {
        let json = r#"{"fake_key": true}"#;
        let result: Result<QueueModuleConfig, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "should reject unknown fields in QueueModuleConfig"
        );
    }

    #[test]
    fn function_queue_config_deny_unknown_fields() {
        let json = r#"{"max_retries": 3, "fake_key": true}"#;
        let result: Result<FunctionQueueConfig, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "should reject unknown fields in FunctionQueueConfig"
        );
    }

    #[test]
    fn default_redis_url_value() {
        assert_eq!(default_redis_url(), "redis://localhost:6379");
    }

    #[test]
    fn deserialize_with_queue_configs() {
        let yaml = r#"
queue_configs:
  default:
    max_retries: 5
    concurrency: 5
    type: standard
  payment:
    max_retries: 10
    concurrency: 2
    type: fifo
    message_group_field: transaction_id
adapter:
  name: builtin
"#;
        let config: QueueModuleConfig = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(config.queue_configs.len(), 2);

        let default_queue = config.queue_configs.get("default").unwrap();
        assert_eq!(default_queue.max_retries, 5);
        assert_eq!(default_queue.concurrency, 5);
        assert_eq!(default_queue.r#type, "standard");
        assert!(default_queue.message_group_field.is_none());
        assert_eq!(default_queue.backoff_ms, 1000);
        assert_eq!(default_queue.poll_interval_ms, 100);

        let payment_queue = config.queue_configs.get("payment").unwrap();
        assert_eq!(payment_queue.max_retries, 10);
        assert_eq!(payment_queue.concurrency, 2);
        assert_eq!(payment_queue.r#type, "fifo");
        assert_eq!(
            payment_queue.message_group_field.as_deref(),
            Some("transaction_id")
        );

        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.name, "builtin");
    }

    #[test]
    fn function_queue_config_defaults() {
        let config = FunctionQueueConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.concurrency, 10);
        assert_eq!(config.r#type, "standard");
        assert!(config.message_group_field.is_none());
        assert_eq!(config.backoff_ms, 1000);
        assert_eq!(config.poll_interval_ms, 100);
    }

    #[test]
    fn validate_fifo_without_group_field_fails() {
        let mut queue_configs = HashMap::new();
        queue_configs.insert(
            "orders".to_string(),
            FunctionQueueConfig {
                r#type: "fifo".to_string(),
                message_group_field: None,
                ..Default::default()
            },
        );
        let config = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("orders"));
        assert!(err.contains("fifo"));
        assert!(err.contains("message_group_field"));
    }

    #[test]
    fn validate_fifo_with_group_field_ok() {
        let mut queue_configs = HashMap::new();
        queue_configs.insert(
            "orders".to_string(),
            FunctionQueueConfig {
                r#type: "fifo".to_string(),
                message_group_field: Some("order_id".to_string()),
                ..Default::default()
            },
        );
        let config = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_invalid_queue_type_fails() {
        let mut queue_configs = HashMap::new();
        queue_configs.insert(
            "orders".to_string(),
            FunctionQueueConfig {
                r#type: "invalid_type".to_string(),
                ..Default::default()
            },
        );
        let config = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid_type"));
    }

    #[test]
    fn validate_zero_max_priority_fails() {
        let mut queue_configs = HashMap::new();
        queue_configs.insert(
            "jobs".to_string(),
            FunctionQueueConfig {
                max_priority: Some(0),
                ..Default::default()
            },
        );
        let config = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("jobs"));
        assert!(err.contains("max_priority"));
    }

    #[test]
    fn validate_priority_queue_ok() {
        let mut queue_configs = HashMap::new();
        queue_configs.insert(
            "jobs".to_string(),
            FunctionQueueConfig {
                max_priority: Some(10),
                priority_field: Some("priority".to_string()),
                concurrency: 1,
                ..Default::default()
            },
        );
        let config = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn deserialize_priority_queue_config() {
        let yaml = r#"
queue_configs:
  jobs:
    type: standard
    concurrency: 1
    max_priority: 10
    priority_field: priority
adapter:
  name: rabbitmq
"#;
        let config: QueueModuleConfig = serde_yaml::from_str(yaml).unwrap();
        let jobs = config.queue_configs.get("jobs").unwrap();
        assert_eq!(jobs.max_priority, Some(10));
        assert_eq!(jobs.priority_field.as_deref(), Some("priority"));
        // Defaults stay backward compatible: omitting both fields means no
        // priority queue.
        let plain: FunctionQueueConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(plain.max_priority, None);
        assert_eq!(plain.priority_field, None);
    }

    #[test]
    fn validate_zero_concurrency_fails() {
        let mut queue_configs = HashMap::new();
        queue_configs.insert(
            "orders".to_string(),
            FunctionQueueConfig {
                concurrency: 0,
                ..Default::default()
            },
        );
        let config = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("orders"));
        assert!(err.contains("concurrency"));
    }

    #[test]
    fn schema_has_per_adapter_oneof() {
        let schema =
            serde_json::to_value(schemars::schema_for!(QueueModuleConfig)).expect("schema");
        // deny_unknown_fields flows into the schema.
        assert_eq!(schema["additionalProperties"], serde_json::json!(false));

        let adapter = &schema["properties"]["adapter"];
        assert!(adapter["description"].is_string());
        let branches = adapter["oneOf"].as_array().expect("adapter oneOf");

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
        let mut want = vec!["bridge", "builtin", "redis"];
        if cfg!(feature = "rabbitmq") {
            want.push("rabbitmq");
        }
        if cfg!(feature = "test-adapters") {
            want.push("memory");
        }
        want.sort_unstable();
        assert_eq!(names, want);

        // Each adapter's concrete config fields land in definitions.
        let defs = &schema["definitions"];
        assert!(defs["BuiltinAdapterConfig"]["properties"]["store_method"].is_object());
        assert_eq!(
            defs["BuiltinAdapterConfig"]["properties"]["save_interval_ms"]["minimum"],
            serde_json::json!(100.0)
        );
        assert!(defs["RedisAdapterConfig"]["properties"]["redis_url"].is_object());
        assert!(defs["BridgeAdapterConfig"]["properties"]["bridge_url"].is_object());

        // `store_method` is a flat string enum (a single select), not a
        // per-variant `oneOf` that renders as "variant 1"/"variant 2".
        let store_method = &defs["QueueStoreMethod"];
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
            serde_json::to_value(schemars::schema_for!(QueueModuleConfig)).expect("schema");
        let validator = jsonschema::Validator::new(&schema).expect("schema compiles");

        // A no-adapter config is valid: the field is omitted, not `null`.
        assert!(validator.is_valid(&serde_json::json!({ "queue_configs": {} })));

        // Accepted: known adapters, with or without config.
        assert!(validator.is_valid(&serde_json::json!({
            "adapter": {"name": "builtin", "config": {"store_method": "file_based", "file_path": "./data/queue.db"}}
        })));
        assert!(validator.is_valid(&serde_json::json!({"adapter": {"name": "builtin"}})));
        assert!(validator.is_valid(&serde_json::json!({
            "adapter": {"name": "redis", "config": {"redis_url": "redis://localhost:6379"}}
        })));
        assert!(validator.is_valid(&serde_json::json!({
            "adapter": {"name": "bridge", "config": {"bridge_url": "ws://localhost:49134"}}
        })));

        // Rejected: unknown adapter, invalid enum value, and an unknown config key.
        assert!(!validator.is_valid(&serde_json::json!({"adapter": {"name": "postgres"}})));
        assert!(!validator.is_valid(&serde_json::json!({
            "adapter": {"name": "builtin", "config": {"store_method": "weird"}}
        })));
        assert!(!validator.is_valid(&serde_json::json!({
            "adapter": {"name": "builtin", "config": {"bogus": 1}}
        })));

        // `rabbitmq` is advertised (and accepted) only when its transport is
        // compiled in, keeping the schema in sync with the adapter registry.
        #[cfg(feature = "rabbitmq")]
        assert!(validator.is_valid(&serde_json::json!({
            "adapter": {"name": "rabbitmq", "config": {"amqp_url": "amqp://localhost:5672"}}
        })));
        #[cfg(not(feature = "rabbitmq"))]
        assert!(!validator.is_valid(&serde_json::json!({"adapter": {"name": "rabbitmq"}})));
    }
}
