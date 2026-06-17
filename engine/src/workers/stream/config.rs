// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

use crate::workers::traits::AdapterEntry;

/// Runtime configuration for the `iii-stream` WebSocket server.
///
/// Consumed by the builtin `configuration` worker: the config.yaml block seeds
/// this entry on first boot, after which the configuration entry is the runtime
/// source of truth. `host`/`port` and `adapter` hot-apply at runtime (the
/// server rebinds; the pub/sub backend is hot-swapped); `auth_function` applies
/// to new connections.
#[derive(Serialize, Deserialize, Debug, Clone, schemars::JsonSchema)]
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
}
