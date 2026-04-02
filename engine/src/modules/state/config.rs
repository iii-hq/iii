// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

use crate::modules::module::AdapterEntry;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct StateModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_state_config() {
        let config: StateModuleConfig = serde_json::from_value(json!({})).unwrap();
        assert!(config.adapter.is_none());
    }

    #[test]
    fn state_config_with_adapter() {
        let json = json!({"adapter": {"class": "redis", "config": {"url": "redis://localhost"}}});
        let config: StateModuleConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.adapter.unwrap().class, "redis");
    }
}
