// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

use serde::Deserialize;

use crate::workers::traits::AdapterEntry;

#[allow(dead_code)] // this is used as default value
fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

fn default_max_retries() -> u32 {
    3
}

fn default_concurrency() -> u32 {
    10
}

fn default_queue_type() -> String {
    "standard".to_string()
}

fn default_backoff_ms() -> u64 {
    1000
}

fn default_poll_interval_ms() -> u64 {
    100
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FunctionQueueConfig {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    #[serde(default = "default_concurrency")]
    pub concurrency: u32,

    #[serde(default = "default_queue_type", rename = "type")]
    pub r#type: String,

    #[serde(default)]
    pub message_group_field: Option<String>,

    #[serde(default = "default_backoff_ms")]
    pub backoff_ms: u64,

    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
}

impl Default for FunctionQueueConfig {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            concurrency: default_concurrency(),
            r#type: default_queue_type(),
            message_group_field: None,
            backoff_ms: default_backoff_ms(),
            poll_interval_ms: default_poll_interval_ms(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct QueueModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,

    #[serde(default)]
    pub queue_configs: HashMap<String, FunctionQueueConfig>,
}

impl QueueModuleConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, queue_config) in &self.queue_configs {
            if queue_config.r#type != "standard" && queue_config.r#type != "fifo" {
                anyhow::bail!(
                    "Queue '{}' has invalid type '{}'. Must be 'standard' or 'fifo'",
                    name,
                    queue_config.r#type
                );
            }
            if queue_config.r#type == "fifo" && queue_config.message_group_field.is_none() {
                anyhow::bail!(
                    "Queue '{}' is of type 'fifo' but 'message_group_field' is not set",
                    name
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = QueueModuleConfig::default();
        assert!(config.adapter.is_none());
        assert!(config.queue_configs.is_empty());
    }

    #[test]
    fn deserialize_empty_json() {
        let config: QueueModuleConfig = serde_json::from_str("{}").unwrap();
        assert!(config.adapter.is_none());
        assert!(config.queue_configs.is_empty());
    }

    #[test]
    fn deserialize_with_adapter() {
        let json =
            r#"{"adapter": {"name": "my::QueueAdapter", "config": {"url": "redis://localhost"}}}"#;
        let config: QueueModuleConfig = serde_json::from_str(json).unwrap();
        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.name, "my::QueueAdapter");
        assert!(adapter.config.is_some());
    }

    #[test]
    fn deserialize_adapter_no_config() {
        let json = r#"{"adapter": {"name": "my::QueueAdapter"}}"#;
        let config: QueueModuleConfig = serde_json::from_str(json).unwrap();
        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.name, "my::QueueAdapter");
        assert!(adapter.config.is_none());
    }

    #[test]
    fn allows_queue_configs_field() {
        let json = r#"{"adapter": null, "queue_configs": {}}"#;
        let result: Result<QueueModuleConfig, _> = serde_json::from_str(json);
        assert!(result.is_ok());
    }

    #[test]
    fn queue_module_config_deny_unknown_fields() {
        let json = r#"{"fake_key": true}"#;
        let result: Result<QueueModuleConfig, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "should reject unknown fields in QueueModuleConfig"
        );
    }

    #[test]
    fn function_queue_config_deny_unknown_fields() {
        let json = r#"{"max_retries": 3, "fake_key": true}"#;
        let result: Result<FunctionQueueConfig, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "should reject unknown fields in FunctionQueueConfig"
        );
    }

    #[test]
    fn default_redis_url_value() {
        assert_eq!(default_redis_url(), "redis://localhost:6379");
    }

    #[test]
    fn deserialize_with_queue_configs() {
        let yaml = r#"
queue_configs:
  default:
    max_retries: 5
    concurrency: 5
    type: standard
  payment:
    max_retries: 10
    concurrency: 2
    type: fifo
    message_group_field: transaction_id
adapter:
  name: builtin
"#;
        let config: QueueModuleConfig = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(config.queue_configs.len(), 2);

        let default_queue = config.queue_configs.get("default").unwrap();
        assert_eq!(default_queue.max_retries, 5);
        assert_eq!(default_queue.concurrency, 5);
        assert_eq!(default_queue.r#type, "standard");
        assert!(default_queue.message_group_field.is_none());
        assert_eq!(default_queue.backoff_ms, 1000);
        assert_eq!(default_queue.poll_interval_ms, 100);

        let payment_queue = config.queue_configs.get("payment").unwrap();
        assert_eq!(payment_queue.max_retries, 10);
        assert_eq!(payment_queue.concurrency, 2);
        assert_eq!(payment_queue.r#type, "fifo");
        assert_eq!(
            payment_queue.message_group_field.as_deref(),
            Some("transaction_id")
        );

        let adapter = config.adapter.unwrap();
        assert_eq!(adapter.name, "builtin");
    }

    #[test]
    fn function_queue_config_defaults() {
        let config = FunctionQueueConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.concurrency, 10);
        assert_eq!(config.r#type, "standard");
        assert!(config.message_group_field.is_none());
        assert_eq!(config.backoff_ms, 1000);
        assert_eq!(config.poll_interval_ms, 100);
    }

    #[test]
    fn validate_fifo_without_group_field_fails() {
        let mut queue_configs = HashMap::new();
        queue_configs.insert(
            "orders".to_string(),
            FunctionQueueConfig {
                r#type: "fifo".to_string(),
                message_group_field: None,
                ..Default::default()
            },
        );
        let config = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("orders"));
        assert!(err.contains("fifo"));
        assert!(err.contains("message_group_field"));
    }

    #[test]
    fn validate_fifo_with_group_field_ok() {
        let mut queue_configs = HashMap::new();
        queue_configs.insert(
            "orders".to_string(),
            FunctionQueueConfig {
                r#type: "fifo".to_string(),
                message_group_field: Some("order_id".to_string()),
                ..Default::default()
            },
        );
        let config = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_invalid_queue_type_fails() {
        let mut queue_configs = HashMap::new();
        queue_configs.insert(
            "orders".to_string(),
            FunctionQueueConfig {
                r#type: "invalid_type".to_string(),
                ..Default::default()
            },
        );
        let config = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid_type"));
    }
}
