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

fn default_body_limit() -> usize {
    1048576
}

fn default_request_id_header() -> String {
    "x-request-id".to_string()
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

    #[serde(default = "default_body_limit")]
    pub body_limit: usize,

    #[serde(default)]
    pub trust_proxy: bool,

    #[serde(default = "default_request_id_header")]
    pub request_id_header: String,

    #[serde(default)]
    pub ignore_trailing_slash: bool,

    #[serde(default)]
    pub not_found_function: Option<String>,
}

impl Default for RestApiConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            default_timeout: default_timeout(),
            cors: None,
            concurrency_request_limit: default_concurrency_request_limit(),
            body_limit: default_body_limit(),
            trust_proxy: false,
            request_id_header: default_request_id_header(),
            ignore_trailing_slash: false,
            not_found_function: None,
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
    use super::RestApiConfig;

    #[test]
    fn config_defaults() {
        let config = RestApiConfig::default();
        assert_eq!(config.body_limit, 1048576);
        assert!(!config.trust_proxy);
        assert_eq!(config.request_id_header, "x-request-id");
        assert!(!config.ignore_trailing_slash);
        assert!(config.not_found_function.is_none());
    }

    #[test]
    fn config_parses_new_fields() {
        let yaml = r#"
            body_limit: 2048
            trust_proxy: true
            request_id_header: "x-correlation-id"
            ignore_trailing_slash: true
            not_found_function: "my-404-handler"
        "#;
        let config: RestApiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.body_limit, 2048);
        assert!(config.trust_proxy);
        assert_eq!(config.request_id_header, "x-correlation-id");
        assert!(config.ignore_trailing_slash);
        assert_eq!(config.not_found_function.as_deref(), Some("my-404-handler"));
    }
}
