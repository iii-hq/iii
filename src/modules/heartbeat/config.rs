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
    pub cloud_endpoint: Option<String>,
    #[serde(default = "default_max_history")]
    pub max_history: Option<usize>,
    pub instance_id: Option<String>,
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
            max_history: Some(1000),
            instance_id: None,
        }
    }
}

impl HeartbeatConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    pub fn interval(&self) -> u64 {
        self.interval_seconds.unwrap_or(60)
    }

    pub fn max_history_size(&self) -> usize {
        self.max_history.unwrap_or(1000)
    }

    pub fn get_instance_id(&self) -> String {
        self.instance_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    }
}
