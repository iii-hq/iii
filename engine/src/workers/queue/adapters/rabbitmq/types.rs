// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

#![cfg(feature = "rabbitmq")]

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
    #[serde(default)]
    pub traceparent: Option<String>,
    #[serde(default)]
    pub baggage: Option<String>,
    /// AMQP message priority (0..=queue `x-max-priority`). Carried on the job so
    /// it survives requeue and DLQ-redrive republishes; stamped onto
    /// `BasicProperties` at publish time. `None` means default priority.
    #[serde(default)]
    pub priority: Option<u8>,
}

impl Job {
    pub fn new(
        topic: impl Into<String>,
        data: Value,
        max_attempts: u32,
        traceparent: Option<String>,
        baggage: Option<String>,
    ) -> Self {
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
            traceparent,
            baggage,
            priority: None,
        }
    }

    /// Attach a priority to the job (builder style). `None` leaves it at the
    /// default priority.
    pub fn with_priority(mut self, priority: Option<u8>) -> Self {
        self.priority = priority;
        self
    }

    pub fn increment_attempts(&mut self) {
        self.attempts_made += 1;
    }

    pub fn is_exhausted(&self) -> bool {
        self.attempts_made >= self.max_attempts
    }
}

/// Resolve an AMQP message priority from `data` given the configured field
/// name. Returns `None` when no field is configured, the field is absent, or
/// its value is not a non-negative integer. Values above 255 saturate to 255;
/// the broker further clamps to each queue's `x-max-priority`.
pub fn priority_from_data(data: &Value, priority_field: Option<&str>) -> Option<u8> {
    let field = priority_field?;
    let value = data.get(field)?;
    let n = value.as_u64()?;
    Some(n.min(u8::MAX as u64) as u8)
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
    /// Field in the message data whose integer value sets the priority of
    /// messages published to topics (pub/sub fanout). `None` disables priority
    /// stamping on the topic path.
    pub priority_field: Option<String>,
}

impl Default for RabbitMQConfig {
    fn default() -> Self {
        Self {
            amqp_url: "amqp://localhost:5672".to_string(),
            max_attempts: 3,
            prefetch_count: 10,
            queue_mode: QueueMode::default(),
            priority_field: None,
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
            if let Some(field) = config.get("priority_field").and_then(|v| v.as_str()) {
                cfg.priority_field = Some(field.to_string());
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
        let job = Job::new(
            "test.topic",
            serde_json::json!({"key": "value"}),
            3,
            None,
            None,
        );
        assert_eq!(job.topic, "test.topic");
        assert_eq!(job.attempts_made, 0);
        assert_eq!(job.max_attempts, 3);
        assert!(!job.is_exhausted());
        assert_eq!(job.priority, None);
    }

    #[test]
    fn test_job_with_priority() {
        let job = Job::new("t", serde_json::json!({}), 1, None, None).with_priority(Some(7));
        assert_eq!(job.priority, Some(7));
    }

    #[test]
    fn test_priority_from_data() {
        let data = serde_json::json!({ "priority": 9, "other": "x" });
        // No field configured → None.
        assert_eq!(priority_from_data(&data, None), None);
        // Field present and integer.
        assert_eq!(priority_from_data(&data, Some("priority")), Some(9));
        // Field absent.
        assert_eq!(priority_from_data(&data, Some("missing")), None);
        // Non-integer value → None.
        let str_data = serde_json::json!({ "priority": "high" });
        assert_eq!(priority_from_data(&str_data, Some("priority")), None);
        // Values above u8::MAX saturate.
        let big = serde_json::json!({ "priority": 300 });
        assert_eq!(priority_from_data(&big, Some("priority")), Some(255));
    }
}
