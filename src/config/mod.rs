// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

use crate::invocation::url_validator::UrlValidatorConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub url_allowlist: Vec<String>,
    #[serde(default = "default_block_private_ips")]
    pub block_private_ips: bool,
    #[serde(default = "default_require_https")]
    pub require_https: bool,
}

fn default_block_private_ips() -> bool {
    true
}

fn default_require_https() -> bool {
    true
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            url_allowlist: vec!["*".to_string()],
            block_private_ips: true,
            require_https: true,
        }
    }
}

impl From<SecurityConfig> for UrlValidatorConfig {
    fn from(config: SecurityConfig) -> Self {
        let allowlist = if config.url_allowlist.is_empty() {
            vec!["*".to_string()]
        } else {
            config.url_allowlist
        };

        Self {
            allowlist,
            block_private_ips: config.block_private_ips,
            require_https: config.require_https,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_config_default() {
        let config = SecurityConfig::default();
        assert_eq!(config.url_allowlist, vec!["*".to_string()]);
        assert!(config.block_private_ips);
        assert!(config.require_https);
    }

    #[test]
    fn security_config_deserialize_empty() {
        let config: SecurityConfig = serde_json::from_str("{}").unwrap();
        // url_allowlist defaults to empty Vec (serde default), block_private_ips/require_https default to true
        assert!(config.url_allowlist.is_empty());
        assert!(config.block_private_ips);
        assert!(config.require_https);
    }

    #[test]
    fn security_config_deserialize_custom() {
        let json = r#"{
            "url_allowlist": ["https://example.com", "https://api.example.com"],
            "block_private_ips": false,
            "require_https": false
        }"#;
        let config: SecurityConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.url_allowlist.len(), 2);
        assert!(!config.block_private_ips);
        assert!(!config.require_https);
    }

    #[test]
    fn security_config_serialize_roundtrip() {
        let config = SecurityConfig {
            url_allowlist: vec!["https://foo.com".to_string()],
            block_private_ips: false,
            require_https: true,
        };
        let json_str = serde_json::to_string(&config).unwrap();
        let deserialized: SecurityConfig = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.url_allowlist, vec!["https://foo.com"]);
        assert!(!deserialized.block_private_ips);
        assert!(deserialized.require_https);
    }

    #[test]
    fn security_config_into_url_validator_with_allowlist() {
        let config = SecurityConfig {
            url_allowlist: vec!["https://example.com".to_string()],
            block_private_ips: true,
            require_https: true,
        };
        let validator_config: UrlValidatorConfig = config.into();
        assert_eq!(validator_config.allowlist, vec!["https://example.com"]);
        assert!(validator_config.block_private_ips);
        assert!(validator_config.require_https);
    }

    #[test]
    fn security_config_into_url_validator_empty_allowlist_becomes_wildcard() {
        let config = SecurityConfig {
            url_allowlist: vec![],
            block_private_ips: false,
            require_https: false,
        };
        let validator_config: UrlValidatorConfig = config.into();
        assert_eq!(validator_config.allowlist, vec!["*".to_string()]);
        assert!(!validator_config.block_private_ips);
        assert!(!validator_config.require_https);
    }

    #[test]
    fn security_config_into_url_validator_default() {
        let config = SecurityConfig::default();
        let validator_config: UrlValidatorConfig = config.into();
        assert_eq!(validator_config.allowlist, vec!["*".to_string()]);
        assert!(validator_config.block_private_ips);
        assert!(validator_config.require_https);
    }
}
