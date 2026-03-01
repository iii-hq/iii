// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::Deserialize;

use crate::modules::module::AdapterEntry;
#[allow(dead_code)] // this is used as default value
fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct QueueModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = QueueModuleConfig::default();
        assert!(config.adapter.is_none());
    }

    #[test]
    fn deserialize_empty_json() {
        let config: QueueModuleConfig = serde_json::from_str("{}").unwrap();
        assert!(config.adapter.is_none());
    }

    #[test]
    fn deserialize_with_adapter() {
        let json =
            r#"{"adapter": {"class": "my::QueueAdapter", "config": {"url": "redis://localhost"}}}"#;
        let config: QueueModuleConfig = serde_json::from_str(json).unwrap();
        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.class, "my::QueueAdapter");
        assert!(adapter.config.is_some());
    }

    #[test]
    fn deserialize_adapter_no_config() {
        let json = r#"{"adapter": {"class": "my::QueueAdapter"}}"#;
        let config: QueueModuleConfig = serde_json::from_str(json).unwrap();
        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.class, "my::QueueAdapter");
        assert!(adapter.config.is_none());
    }

    #[test]
    fn deny_unknown_fields() {
        let json = r#"{"adapter": null, "extra_field": true}"#;
        let result: Result<QueueModuleConfig, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn default_redis_url_value() {
        assert_eq!(default_redis_url(), "redis://localhost:6379");
    }
}
