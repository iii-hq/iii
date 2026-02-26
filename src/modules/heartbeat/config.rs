// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeartbeatConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_interval")]
    pub interval_seconds: u64,
    #[serde(default, deserialize_with = "deserialize_non_empty_string")]
    pub cloud_endpoint: Option<String>,
    #[serde(default)]
    pub cloud_telemetry_enabled: bool,
    #[serde(default = "default_max_history")]
    pub max_history: usize,
    pub instance_id: Option<String>,
}

fn deserialize_non_empty_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()))
}

fn default_enabled() -> bool {
    false
}

fn default_interval() -> u64 {
    60
}

fn default_max_history() -> usize {
    1000
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_seconds: 60,
            cloud_endpoint: None,
            cloud_telemetry_enabled: false,
            max_history: 1000,
            instance_id: None,
        }
    }
}

impl HeartbeatConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_cloud_telemetry_enabled(&self) -> bool {
        self.cloud_telemetry_enabled
    }

    pub fn interval(&self) -> u64 {
        self.interval_seconds
    }

    pub fn max_history_size(&self) -> usize {
        self.max_history
    }

    fn instance_id_dir() -> std::path::PathBuf {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(".iii")
    }

    pub fn get_or_create_instance_id(&self) -> String {
        if let Some(ref id) = self.instance_id {
            return id.clone();
        }

        let dir = Self::instance_id_dir();
        let path = dir.join("instance_id");

        if let Ok(id) = std::fs::read_to_string(&path) {
            let id = id.trim().to_string();
            if uuid::Uuid::parse_str(&id).is_ok() {
                return id;
            }
            tracing::warn!(
                "[HEARTBEAT] Instance ID file contains invalid UUID, regenerating: {}",
                path.display()
            );
        }

        let id = uuid::Uuid::new_v4().to_string();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!(
                "Failed to create .iii directory for instance_id persistence: {}",
                e
            );
        } else if let Err(e) = std::fs::write(&path, &id) {
            tracing::warn!("Failed to persist instance_id to {}: {}", path.display(), e);
        }
        id
    }

    pub fn instance_id_short(id: &str) -> &str {
        id.get(..8).unwrap_or(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = HeartbeatConfig::default();
        assert!(!config.is_enabled());
        assert!(!config.is_cloud_telemetry_enabled());
        assert_eq!(config.interval(), 60);
        assert_eq!(config.max_history_size(), 1000);
    }

    #[test]
    fn instance_id_short_handles_short_strings() {
        assert_eq!(HeartbeatConfig::instance_id_short("abc"), "abc");
        assert_eq!(HeartbeatConfig::instance_id_short(""), "");
        assert_eq!(
            HeartbeatConfig::instance_id_short("12345678-rest"),
            "12345678"
        );
    }

    #[test]
    fn deserialize_plain_types() {
        let yaml = r#"
            enabled: false
            interval_seconds: 30
            max_history: 500
            cloud_telemetry_enabled: true
        "#;
        let config: HeartbeatConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!config.is_enabled());
        assert!(config.is_cloud_telemetry_enabled());
        assert_eq!(config.interval(), 30);
        assert_eq!(config.max_history_size(), 500);
    }

    #[test]
    fn empty_cloud_endpoint_becomes_none() {
        let yaml = r#"
            cloud_endpoint: ""
        "#;
        let config: HeartbeatConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.cloud_endpoint.is_none());
    }
}
