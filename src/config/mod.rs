// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod persistence;

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
