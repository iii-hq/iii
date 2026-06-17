// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::workers::traits::AdapterEntry;

#[allow(dead_code)] // this is used as default value
fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

/// Runtime configuration for the builtin `iii-pubsub` worker. The doc comment on
/// each field flows into the JSON Schema (via `schemars`) that the `iii-pubsub`
/// configuration entry registers, so an agent introspecting the schema sees the
/// same descriptions documented here. After first boot the configuration worker
/// entry is the runtime source of truth; the config.yaml block is seed-only.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PubSubModuleConfig {
    /// Pub/sub backend selection and its adapter-specific config, keyed on
    /// `name` over the built-in adapters `local` (default, in-process broadcast)
    /// and `redis` (cross-instance Redis Pub/Sub). Hot-swap tier: a runtime edit
    /// rebuilds the backend, re-subscribes every live `subscribe` trigger onto
    /// it, and tears down the previous one — no engine restart. A value that
    /// fails to build the backend is gated and keeps the previous one. The field
    /// keeps the loosely-typed `AdapterEntry` so a custom adapter registered in
    /// the registry is still selectable; `configuration::set` validates the value
    /// shape and the registry validates the adapter name at apply time.
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
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
}
