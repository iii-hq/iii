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
    use serde_json::json;

    #[test]
    fn test_rest_api_config_default() {
        let config = RestApiConfig::default();
        assert_eq!(config.port, 3111);
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.default_timeout, 30000);
        assert_eq!(config.concurrency_request_limit, 1024);
        assert!(config.cors.is_none());
    }

    #[test]
    fn test_rest_api_config_deserialize_full() {
        let json = json!({
            "port": 8080,
            "host": "127.0.0.1",
            "default_timeout": 60000,
            "concurrency_request_limit": 2048,
            "cors": {
                "allowed_origins": ["http://localhost:3000"],
                "allowed_methods": ["GET", "POST"]
            }
        });
        let config: RestApiConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.default_timeout, 60000);
        assert_eq!(config.concurrency_request_limit, 2048);
        assert!(config.cors.is_some());
        let cors = config.cors.unwrap();
        assert_eq!(cors.allowed_origins.len(), 1);
        assert_eq!(cors.allowed_methods.len(), 2);
    }

    #[test]
    fn test_rest_api_config_deserialize_partial() {
        let json = json!({
            "port": 8080
        });
        let config: RestApiConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.default_timeout, 30000);
        assert_eq!(config.concurrency_request_limit, 1024);
        assert!(config.cors.is_none());
    }

    #[test]
    fn test_rest_api_config_deserialize_empty() {
        let json = json!({});
        let config: RestApiConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.port, 3111);
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.default_timeout, 30000);
        assert_eq!(config.concurrency_request_limit, 1024);
        assert!(config.cors.is_none());
    }

    #[test]
    fn test_cors_config_default() {
        let config = CorsConfig::default();
        assert_eq!(config.allowed_origins.len(), 0);
        assert_eq!(config.allowed_methods.len(), 0);
    }

    #[test]
    fn test_cors_config_deserialize_empty() {
        let json = json!({});
        let config: CorsConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.allowed_origins.len(), 0);
        assert_eq!(config.allowed_methods.len(), 0);
    }

    #[test]
    fn test_cors_config_deserialize_with_values() {
        let json = json!({
            "allowed_origins": ["http://localhost:3000", "https://example.com"],
            "allowed_methods": ["GET", "POST", "PUT"]
        });
        let config: CorsConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.allowed_origins.len(), 2);
        assert_eq!(config.allowed_origins[0], "http://localhost:3000");
        assert_eq!(config.allowed_origins[1], "https://example.com");
        assert_eq!(config.allowed_methods.len(), 3);
        assert_eq!(config.allowed_methods[0], "GET");
        assert_eq!(config.allowed_methods[1], "POST");
        assert_eq!(config.allowed_methods[2], "PUT");
    }

    #[test]
    fn test_rest_api_config_serialize_round_trip() {
        let config = RestApiConfig {
            port: 8080,
            host: "127.0.0.1".to_string(),
            default_timeout: 60000,
            cors: Some(CorsConfig {
                allowed_origins: vec!["http://localhost:3000".to_string()],
                allowed_methods: vec!["GET".to_string()],
            }),
            concurrency_request_limit: 2048,
        };
        let json = serde_json::to_value(&config).unwrap();
        let deserialized: RestApiConfig = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.port, 8080);
        assert_eq!(deserialized.host, "127.0.0.1");
        assert_eq!(deserialized.default_timeout, 60000);
        assert_eq!(deserialized.concurrency_request_limit, 2048);
        assert!(deserialized.cors.is_some());
    }
}
