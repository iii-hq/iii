// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub topic: String,
    pub data: Value,
    pub attempts_made: u32,
    pub max_attempts: u32,
    pub created_at: u64,
}

impl Job {
    pub fn new(topic: impl Into<String>, data: Value, max_attempts: u32) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            topic: topic.into(),
            data,
            attempts_made: 0,
            max_attempts,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        }
    }

    pub fn increment_attempts(&mut self) {
        self.attempts_made += 1;
    }

    pub fn is_exhausted(&self) -> bool {
        self.attempts_made >= self.max_attempts
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum QueueMode {
    #[default]
    Standard,
    Fifo,
}

impl FromStr for QueueMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "fifo" => Ok(QueueMode::Fifo),
            "standard" => Ok(QueueMode::Standard),
            _ => Ok(QueueMode::default()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RabbitMQConfig {
    pub amqp_url: String,
    pub max_attempts: u32,
    pub prefetch_count: u16,
    pub queue_mode: QueueMode,
}

impl Default for RabbitMQConfig {
    fn default() -> Self {
        Self {
            amqp_url: "amqp://localhost:5672".to_string(),
            max_attempts: 3,
            prefetch_count: 10,
            queue_mode: QueueMode::default(),
        }
    }
}

impl RabbitMQConfig {
    pub fn from_value(config: Option<&Value>) -> Self {
        let mut cfg = Self::default();

        if let Some(config) = config {
            if let Some(url) = config.get("amqp_url").and_then(|v| v.as_str()) {
                cfg.amqp_url = url.to_string();
            }
            if let Some(attempts) = config.get("max_attempts").and_then(|v| v.as_u64()) {
                cfg.max_attempts = attempts as u32;
            }
            if let Some(prefetch) = config.get("prefetch_count").and_then(|v| v.as_u64()) {
                cfg.prefetch_count = prefetch as u16;
            }
            if let Some(mode) = config.get("queue_mode").and_then(|v| v.as_str()) {
                cfg.queue_mode = QueueMode::from_str(mode).unwrap_or_default();
            }
        }

        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_creation() {
        let job = Job::new("test.topic", serde_json::json!({"key": "value"}), 3);
        assert_eq!(job.topic, "test.topic");
        assert_eq!(job.attempts_made, 0);
        assert_eq!(job.max_attempts, 3);
        assert!(!job.is_exhausted());
    }
}
