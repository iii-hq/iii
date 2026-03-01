// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

fn default_port() -> u16 {
    3111
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_timeout() -> u64 {
    30000
}

fn default_concurrency_request_limit() -> usize {
    1024
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestApiConfig {
    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_timeout")]
    pub default_timeout: u64,

    #[serde(default)]
    pub cors: Option<CorsConfig>,

    #[serde(default = "default_concurrency_request_limit")]
    pub concurrency_request_limit: usize,
}

impl Default for RestApiConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            default_timeout: default_timeout(),
            cors: None,
            concurrency_request_limit: default_concurrency_request_limit(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CorsConfig {
    #[serde(default)]
    pub allowed_origins: Vec<String>,

    #[serde(default)]
    pub allowed_methods: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // RestApiConfig defaults
    // =========================================================================

    #[test]
    fn rest_api_config_default_values() {
        let config = RestApiConfig::default();
        assert_eq!(config.port, 3111);
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.default_timeout, 30000);
        assert!(config.cors.is_none());
        assert_eq!(config.concurrency_request_limit, 1024);
    }

    #[test]
    fn rest_api_config_deserialize_empty_json() {
        let config: RestApiConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.port, 3111);
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.default_timeout, 30000);
        assert!(config.cors.is_none());
        assert_eq!(config.concurrency_request_limit, 1024);
    }

    #[test]
    fn rest_api_config_deserialize_custom_values() {
        let json = r#"{
            "port": 8080,
            "host": "127.0.0.1",
            "default_timeout": 5000,
            "concurrency_request_limit": 512
        }"#;
        let config: RestApiConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.default_timeout, 5000);
        assert_eq!(config.concurrency_request_limit, 512);
        assert!(config.cors.is_none());
    }

    #[test]
    fn rest_api_config_deserialize_with_cors() {
        let json = r#"{
            "cors": {
                "allowed_origins": ["http://localhost:3000", "https://example.com"],
                "allowed_methods": ["GET", "POST"]
            }
        }"#;
        let config: RestApiConfig = serde_json::from_str(json).unwrap();
        let cors = config.cors.unwrap();
        assert_eq!(
            cors.allowed_origins,
            vec!["http://localhost:3000", "https://example.com"]
        );
        assert_eq!(cors.allowed_methods, vec!["GET", "POST"]);
    }

    #[test]
    fn rest_api_config_serialize_roundtrip() {
        let config = RestApiConfig {
            port: 9090,
            host: "localhost".to_string(),
            default_timeout: 10000,
            cors: Some(CorsConfig {
                allowed_origins: vec!["*".to_string()],
                allowed_methods: vec!["GET".to_string()],
            }),
            concurrency_request_limit: 256,
        };
        let json_str = serde_json::to_string(&config).unwrap();
        let deserialized: RestApiConfig = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.port, 9090);
        assert_eq!(deserialized.host, "localhost");
        assert_eq!(deserialized.default_timeout, 10000);
        assert_eq!(deserialized.concurrency_request_limit, 256);
        let cors = deserialized.cors.unwrap();
        assert_eq!(cors.allowed_origins, vec!["*"]);
        assert_eq!(cors.allowed_methods, vec!["GET"]);
    }

    #[test]
    fn rest_api_config_deny_unknown_fields() {
        let json = r#"{"port": 3111, "unknown_field": true}"#;
        let result: Result<RestApiConfig, _> = serde_json::from_str(json);
        assert!(result.is_err(), "should reject unknown fields");
    }

    // =========================================================================
    // CorsConfig
    // =========================================================================

    #[test]
    fn cors_config_default() {
        let cors = CorsConfig::default();
        assert!(cors.allowed_origins.is_empty());
        assert!(cors.allowed_methods.is_empty());
    }

    #[test]
    fn cors_config_deserialize_empty() {
        let cors: CorsConfig = serde_json::from_str("{}").unwrap();
        assert!(cors.allowed_origins.is_empty());
        assert!(cors.allowed_methods.is_empty());
    }

    #[test]
    fn cors_config_deserialize_partial() {
        let json = r#"{"allowed_origins": ["http://example.com"]}"#;
        let cors: CorsConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cors.allowed_origins, vec!["http://example.com"]);
        assert!(cors.allowed_methods.is_empty());
    }

    // =========================================================================
    // YAML deserialization (via serde_yaml)
    // =========================================================================

    #[test]
    fn rest_api_config_from_yaml() {
        let yaml = r#"
port: 4000
host: "192.168.1.1"
default_timeout: 60000
concurrency_request_limit: 2048
cors:
  allowed_origins:
    - "https://app.example.com"
  allowed_methods:
    - "GET"
    - "POST"
    - "PUT"
"#;
        let config: RestApiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.port, 4000);
        assert_eq!(config.host, "192.168.1.1");
        assert_eq!(config.default_timeout, 60000);
        assert_eq!(config.concurrency_request_limit, 2048);
        let cors = config.cors.unwrap();
        assert_eq!(cors.allowed_origins, vec!["https://app.example.com"]);
        assert_eq!(cors.allowed_methods, vec!["GET", "POST", "PUT"]);
    }

    #[test]
    fn rest_api_config_from_yaml_defaults() {
        let yaml = "{}";
        let config: RestApiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.port, 3111);
        assert_eq!(config.host, "0.0.0.0");
    }
}
