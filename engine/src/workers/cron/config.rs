// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

use crate::workers::traits::AdapterEntry;

#[derive(Debug, Clone, Deserialize, Serialize, Default, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CronModuleConfig {
    /// The distributed-lock backend used to coordinate cron job execution across
    /// engine instances (e.g. `kv`, `redis`). Omit to use the default `kv`
    /// adapter. Changing this at runtime hot-swaps the lock backend and
    /// re-schedules every live cron job onto the new transport.
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
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
    fn schema_denies_unknown_fields_and_documents_adapter() {
        let schema = serde_json::to_value(schemars::schema_for!(CronModuleConfig)).unwrap();
        // `deny_unknown_fields` must flow into the schema so `configuration::set`
        // rejects a typo'd top-level key at write time.
        assert_eq!(
            schema["additionalProperties"],
            serde_json::json!(false),
            "schema must deny unknown fields: {schema}"
        );
        // The adapter field must exist and carry the doc comment as a schema
        // description, so an agent introspecting the config gets guidance.
        assert!(
            schema["properties"]["adapter"].is_object(),
            "adapter property must be present: {schema}"
        );
        assert!(
            schema["properties"]["adapter"]["description"].is_string(),
            "adapter field must carry a schema description: {schema}"
        );
    }
}
