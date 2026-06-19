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

/// Runtime configuration for the builtin `iii-pubsub` worker. The doc comment on
/// each field flows into the JSON Schema (via `schemars`) that the `iii-pubsub`
/// configuration entry registers, so an agent introspecting the schema sees the
/// same descriptions documented here. After first boot the configuration worker
/// entry is the runtime source of truth; the config.yaml block is seed-only.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PubSubModuleConfig {
    /// Pub/sub backend selection and its adapter-specific config, advertised as a
    /// discriminated union keyed on `name` over the built-in adapters `local`
    /// (default, in-process broadcast) and `redis` (cross-instance Redis Pub/Sub).
    /// Hot-swap tier: a runtime edit rebuilds the backend, re-subscribes the live
    /// subscriptions onto it, and tears down the previous one — no engine restart.
    /// A value that fails to build the backend is gated and keeps the previous
    /// one. The set is closed: `configuration::set` rejects an unknown adapter
    /// name. The field keeps the loosely-typed `AdapterEntry` for deserialization
    /// so a hand-edited persisted file is tolerated at boot, while the schema
    /// validates against the concrete per-adapter shape.
    #[serde(default)]
    #[schemars(schema_with = "pubsub_adapter_schema")]
    pub adapter: Option<AdapterEntry>,
}

/// Schema for an adapter `config`: an object (or `null`/absent — the seed
/// serializes an absent `AdapterEntry.config` as `null`). `properties` names the
/// adapter's typed keys; `strict` adds `additionalProperties = false`. Passing
/// `&[]` with `strict = true` yields a closed empty object (for adapters like
/// `local` that take no config — any key is rejected).
fn adapter_config_schema(properties: &[(&str, &str)], strict: bool) -> Schema {
    let mut object = SchemaObject {
        // Accept both an object and `null` so a seed with no config (serialized
        // as `config: null`) still validates at `configuration::register`.
        instance_type: Some(vec![InstanceType::Object, InstanceType::Null].into()),
        ..Default::default()
    };
    {
        let obj = object.object();
        for (key, description) in properties {
            let mut field = SchemaObject {
                instance_type: Some(InstanceType::String.into()),
                ..Default::default()
            };
            field.metadata().description = Some((*description).to_string());
            obj.properties
                .insert((*key).to_string(), Schema::Object(field));
        }
        if strict {
            obj.additional_properties = Some(Box::new(Schema::Bool(false)));
        }
    }
    Schema::Object(object)
}

/// Build the `oneOf` schema for [`PubSubModuleConfig::adapter`]: one branch per
/// built-in adapter, each pinned to its `name` discriminator and carrying that
/// adapter's `config` schema. The name set is closed — `configuration::set`
/// rejects any other adapter name — so the console renders per-adapter fields
/// instead of a free-form (and unrenderable) object. Deserialization stays
/// permissive via the `AdapterEntry` field type, so a hand-edited persisted file
/// is still tolerated at boot.
fn pubsub_adapter_schema(_generator: &mut SchemaGenerator) -> Schema {
    #[allow(unused_mut)]
    let mut branches = vec![
        // `local` ignores its config entirely (the factory takes `_config`), so
        // its branch carries a closed empty object: the console shows no config
        // fields and `configuration::set` rejects any junk key (the `null`/absent
        // seed still validates — the config schema allows `null`).
        adapter_branch("local", adapter_config_schema(&[], true)),
        // `redis` carries a typed `redis_url`.
        adapter_branch(
            "redis",
            adapter_config_schema(
                &[(
                    "redis_url",
                    "Redis connection URL. Defaults to `redis://localhost:6379`.",
                )],
                true,
            ),
        ),
    ];

    // The `test-adapters` feature registers an in-process `memory` backend for
    // the pubsub config e2e suite; advertise it in the schema so
    // `configuration::set` accepts it. Open config — it's an opaque test carrier.
    #[cfg(feature = "test-adapters")]
    branches.push(adapter_branch("memory", adapter_config_schema(&[], false)));

    let mut schema = SchemaObject::default();
    schema.metadata().description = Some(
        "Pub/sub backend selection and its adapter-specific config, a discriminated \
         union keyed on `name` over the built-in adapters `local` (default, \
         in-process broadcast) and `redis` (cross-instance Redis Pub/Sub). Hot-swap \
         tier: a runtime edit rebuilds the backend, re-subscribes the live \
         subscriptions onto it, and tears down the previous one — no engine restart. \
         A value that fails to build the backend is gated and keeps the previous one."
            .to_string(),
    );
    schema.subschemas().one_of = Some(branches);
    Schema::Object(schema)
}

/// One `oneOf` branch: an object pinned to `name` and carrying the adapter's
/// `config` sub-schema. `config` is optional (both built-in adapters have working
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
    use serde_json::json;

    #[test]
    fn default_pubsub_config() {
        let config: PubSubModuleConfig = serde_json::from_value(json!({})).unwrap();
        assert!(config.adapter.is_none());
    }

    #[test]
    fn pubsub_config_deny_unknown_fields() {
        let result = serde_json::from_value::<PubSubModuleConfig>(json!({"unknown": true}));
        assert!(result.is_err());
    }

    #[test]
    fn schema_denies_unknown_fields_and_documents_adapter() {
        let schema = serde_json::to_value(schemars::schema_for!(PubSubModuleConfig))
            .expect("schema serializes");
        // deny_unknown_fields flows into the schema so the configuration worker
        // rejects typo'd top-level keys at `configuration::set` time.
        assert_eq!(schema["additionalProperties"], json!(false));
        // The adapter field's doc comment must reach the schema so an agent
        // introspecting the config gets a description, not just a `$ref`.
        let adapter = &schema["properties"]["adapter"];
        assert!(
            adapter.is_object(),
            "adapter property must be present: {schema}"
        );
        assert!(
            adapter["description"].is_string(),
            "adapter field must carry a schema description: {schema}"
        );
    }

    #[test]
    fn adapter_schema_is_a_closed_local_redis_union_with_typed_redis_config() {
        let schema = serde_json::to_value(schemars::schema_for!(PubSubModuleConfig))
            .expect("schema serializes");
        let branches = schema["properties"]["adapter"]["oneOf"]
            .as_array()
            .expect("adapter is a oneOf union");
        let names: Vec<&str> = branches
            .iter()
            .filter_map(|b| b["properties"]["name"]["enum"][0].as_str())
            .collect();
        // The `test-adapters` feature appends a `memory` branch (see
        // `pubsub_adapter_schema`); this crate's own test build enables it via the
        // self dev-dependency, so the closed set is exactly local/redis (+ memory).
        #[cfg(not(feature = "test-adapters"))]
        assert_eq!(names, vec!["local", "redis"], "closed set: {schema}");
        #[cfg(feature = "test-adapters")]
        assert_eq!(
            names,
            vec!["local", "redis", "memory"],
            "closed set incl. test adapter: {schema}"
        );

        // The redis branch carries a typed `redis_url` (so the console renders a
        // field instead of a free-form object).
        let redis = branches
            .iter()
            .find(|b| b["properties"]["name"]["enum"][0] == "redis")
            .expect("redis branch present");
        assert!(
            redis["properties"]["config"]["properties"]["redis_url"].is_object(),
            "redis config must expose a typed redis_url: {schema}"
        );

        // `local` takes no config, so its branch is a closed empty object: the
        // console renders no fields and junk keys are rejected at set time.
        let local = branches
            .iter()
            .find(|b| b["properties"]["name"]["enum"][0] == "local")
            .expect("local branch present");
        assert_eq!(
            local["properties"]["config"]["additionalProperties"],
            json!(false),
            "local config must be a closed empty object: {schema}"
        );
    }
}
