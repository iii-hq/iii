// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::Deserialize;

use crate::workers::module::AdapterEntry;

#[allow(dead_code)] // this is used as default value
fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PubSubWorkerConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_pubsub_config() {
        let config: PubSubWorkerConfig = serde_json::from_value(json!({})).unwrap();
        assert!(config.adapter.is_none());
    }

    #[test]
    fn pubsub_config_deny_unknown_fields() {
        let result = serde_json::from_value::<PubSubWorkerConfig>(json!({"unknown": true}));
        assert!(result.is_err());
    }
}
