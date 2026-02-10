// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeartbeatConfig {
    #[serde(default = "default_enabled")]
    pub enabled: Option<bool>,
    #[serde(default = "default_interval")]
    pub interval_seconds: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_non_empty_string")]
    pub cloud_endpoint: Option<String>,
    #[serde(default)]
    pub cloud_telemetry_enabled: Option<bool>,
    #[serde(default = "default_max_history")]
    pub max_history: Option<usize>,
    pub instance_id: Option<String>,
}

fn deserialize_non_empty_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()))
}

fn default_enabled() -> Option<bool> {
    Some(true)
}

fn default_interval() -> Option<u64> {
    Some(60)
}

fn default_max_history() -> Option<usize> {
    Some(1000)
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: Some(true),
            interval_seconds: Some(60),
            cloud_endpoint: None,
            cloud_telemetry_enabled: None,
            max_history: Some(1000),
            instance_id: None,
        }
    }
}

impl HeartbeatConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    pub fn is_cloud_telemetry_enabled(&self) -> bool {
        self.cloud_telemetry_enabled.unwrap_or(false)
    }

    pub fn interval(&self) -> u64 {
        self.interval_seconds.unwrap_or(60)
    }

    pub fn max_history_size(&self) -> usize {
        self.max_history.unwrap_or(1000)
    }

    pub fn get_or_create_instance_id(&self) -> String {
        if let Some(ref id) = self.instance_id {
            return id.clone();
        }

        let path = std::path::Path::new(".iii/instance_id");
        if let Ok(id) = std::fs::read_to_string(path) {
            let id = id.trim().to_string();
            if !id.is_empty() {
                return id;
            }
        }

        let id = uuid::Uuid::new_v4().to_string();
        if let Err(e) = std::fs::create_dir_all(".iii") {
            tracing::warn!(
                "Failed to create .iii directory for instance_id persistence: {}",
                e
            );
        } else if let Err(e) = std::fs::write(path, &id) {
            tracing::warn!("Failed to persist instance_id to {}: {}", path.display(), e);
        }
        id
    }
}
