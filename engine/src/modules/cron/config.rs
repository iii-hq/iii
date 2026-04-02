// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

use crate::modules::module::AdapterEntry;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CronModuleConfig {
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
        let json = r#"{"adapter": {"class": "my::CronAdapter", "config": {"key": "val"}}}"#;
        let config: CronModuleConfig = serde_json::from_str(json).unwrap();
        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.class, "my::CronAdapter");
        assert!(adapter.config.is_some());
    }

    #[test]
    fn deserialize_adapter_no_config() {
        let json = r#"{"adapter": {"class": "cron::Adapter"}}"#;
        let config: CronModuleConfig = serde_json::from_str(json).unwrap();
        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.class, "cron::Adapter");
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
                class: "test::Adapter".to_string(),
                config: Some(serde_json::json!({"interval": 60})),
            }),
        };
        let json_str = serde_json::to_string(&config).unwrap();
        let deserialized: CronModuleConfig = serde_json::from_str(&json_str).unwrap();
        let adapter = deserialized.adapter.unwrap();
        assert_eq!(adapter.class, "test::Adapter");
        assert_eq!(adapter.config.unwrap()["interval"], 60);
    }
}
