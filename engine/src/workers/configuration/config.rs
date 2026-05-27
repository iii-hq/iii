// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

use crate::workers::traits::AdapterEntry;

/// Worker-level configuration parsed from `engine/config.yaml`'s
/// `- name: configuration` block.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationModuleConfig {
    /// Adapter entry. `fs` is the default; `bridge` delegates to a remote engine.
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,

    /// Per-id TTL in seconds. When set to a non-zero value, an entry whose
    /// last trigger has been unregistered is deleted after this many seconds
    /// of inactivity. `0` (the default) disables the cleanup.
    #[serde(default)]
    pub ttl_seconds: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_config_has_no_adapter_and_zero_ttl() {
        let config = ConfigurationModuleConfig::default();
        assert!(config.adapter.is_none());
        assert_eq!(config.ttl_seconds, 0);
    }

    #[test]
    fn deserialize_with_ttl_and_adapter() {
        let json = json!({
            "adapter": { "name": "fs", "config": { "directory": "/tmp/cfg" } },
            "ttl_seconds": 3600,
        });
        let cfg: ConfigurationModuleConfig = serde_json::from_value(json).unwrap();
        assert_eq!(cfg.ttl_seconds, 3600);
        assert_eq!(cfg.adapter.unwrap().name, "fs");
    }

    #[test]
    fn rejects_unknown_fields() {
        let json = json!({ "frobnicate": true });
        assert!(serde_json::from_value::<ConfigurationModuleConfig>(json).is_err());
    }
}
