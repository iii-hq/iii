// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

use crate::modules::module::AdapterEntry;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct StreamModuleConfig {
    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default)]
    pub auth_function: Option<String>,

    #[serde(default)]
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
                "class": "my_adapter::StreamAdapter",
                "config": {"key": "value"}
            }
        }"#;
        let config: StreamModuleConfig = serde_json::from_str(json).unwrap();
        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.class, "my_adapter::StreamAdapter");
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
}
