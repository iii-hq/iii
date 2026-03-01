// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

use crate::config::SecurityConfig;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HttpFunctionsConfig {
    #[serde(default)]
    pub security: SecurityConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = HttpFunctionsConfig::default();
        // SecurityConfig default has url_allowlist = ["*"], block_private_ips = true, require_https = true
        assert!(!config.security.url_allowlist.is_empty());
        assert!(config.security.block_private_ips);
        assert!(config.security.require_https);
    }

    #[test]
    fn deserialize_empty_json() {
        let config: HttpFunctionsConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.security.url_allowlist.is_empty());
    }

    #[test]
    fn deserialize_with_security() {
        let json = r#"{
            "security": {
                "url_allowlist": ["https://example.com"],
                "block_private_ips": false,
                "require_https": false
            }
        }"#;
        let config: HttpFunctionsConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.security.url_allowlist, vec!["https://example.com"]);
        assert!(!config.security.block_private_ips);
        assert!(!config.security.require_https);
    }

    #[test]
    fn serialize_roundtrip() {
        let config = HttpFunctionsConfig::default();
        let json_str = serde_json::to_string(&config).unwrap();
        let deserialized: HttpFunctionsConfig = serde_json::from_str(&json_str).unwrap();
        assert_eq!(
            config.security.url_allowlist,
            deserialized.security.url_allowlist
        );
        assert_eq!(
            config.security.block_private_ips,
            deserialized.security.block_private_ips
        );
        assert_eq!(
            config.security.require_https,
            deserialized.security.require_https
        );
    }
}
