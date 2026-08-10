// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use serde_json::Value;

macro_rules! extract_field {
    ($config:expr, $json_key:expr, $field:expr, $has_any:expr, $extract:ident, $convert:expr) => {
        if let Some(value) = $config.get($json_key).and_then(|v| v.$extract()) {
            $field = Some($convert(value));
            $has_any = true;
        }
    };
}

#[derive(Debug, Clone, Default)]
pub struct SubscriberQueueConfig {
    pub queue_mode: Option<String>,
    pub max_retries: Option<u32>,
    pub concurrency: Option<u32>,
    /// Maximum time in milliseconds a dispatched delivery may remain
    /// uncompleted before the engine abandons the invocation and the
    /// delivery flows into the path's normal failure handling (nack →
    /// retry → DLQ where the adapter supports it). Unset means the
    /// dispatch is unbounded. The timeout does not cancel work already
    /// running on a worker, so a retry can overlap the original attempt —
    /// bounded handlers must be idempotent.
    pub dispatch_timeout_ms: Option<u64>,
    pub delay_seconds: Option<u64>,
    pub backoff_type: Option<String>,
    pub backoff_delay_ms: Option<u64>,
    /// Declares this subscriber's queue as a RabbitMQ priority queue with this
    /// many levels (`x-max-priority`, 1–255). RabbitMQ-only; the priority value
    /// of each message comes from the adapter-level `priority_field`.
    pub max_priority: Option<u8>,
}

impl SubscriberQueueConfig {
    pub fn from_value(config: Option<&Value>) -> Option<Self> {
        let config = config?;

        if !config.is_object() {
            return None;
        }

        let mut subscriber_config = Self::default();
        let mut has_any_value = false;

        extract_field!(
            config,
            "type",
            subscriber_config.queue_mode,
            has_any_value,
            as_str,
            |v: &str| v.to_string()
        );
        extract_field!(
            config,
            "maxRetries",
            subscriber_config.max_retries,
            has_any_value,
            as_u64,
            |v| v as u32
        );
        extract_field!(
            config,
            "concurrency",
            subscriber_config.concurrency,
            has_any_value,
            as_u64,
            |v| v as u32
        );
        extract_field!(
            config,
            "dispatchTimeoutMs",
            subscriber_config.dispatch_timeout_ms,
            has_any_value,
            as_u64,
            |v| v
        );
        extract_field!(
            config,
            "delaySeconds",
            subscriber_config.delay_seconds,
            has_any_value,
            as_u64,
            |v| v
        );
        extract_field!(
            config,
            "backoffType",
            subscriber_config.backoff_type,
            has_any_value,
            as_str,
            |v: &str| v.to_string()
        );
        extract_field!(
            config,
            "backoffDelayMs",
            subscriber_config.backoff_delay_ms,
            has_any_value,
            as_u64,
            |v| v
        );
        extract_field!(
            config,
            "maxPriority",
            subscriber_config.max_priority,
            has_any_value,
            as_u64,
            |v| v as u8
        );

        if has_any_value {
            Some(subscriber_config)
        } else {
            None
        }
    }

    /// Resolves `dispatch_timeout_ms` into a `Duration`, treating `0` as
    /// invalid (unbounded) since a zero-duration dispatch timeout would
    /// abandon every delivery immediately.
    pub fn dispatch_timeout(&self) -> Option<std::time::Duration> {
        match self.dispatch_timeout_ms {
            Some(0) => {
                tracing::warn!(
                    "'dispatchTimeoutMs' 0 is invalid; ignoring (dispatch stays unbounded)"
                );
                None
            }
            ms => ms.map(std::time::Duration::from_millis),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_from_value_with_all_fields() {
        let config = json!({
            "type": "fifo",
            "maxRetries": 5,
            "concurrency": 20,
            "dispatchTimeoutMs": 3000,
            "delaySeconds": 10,
            "backoffType": "exponential",
            "backoffDelayMs": 2000
        });

        let result = SubscriberQueueConfig::from_value(Some(&config));
        assert!(result.is_some());

        let subscriber_config = result.unwrap();
        assert_eq!(subscriber_config.queue_mode, Some("fifo".to_string()));
        assert_eq!(subscriber_config.max_retries, Some(5));
        assert_eq!(subscriber_config.concurrency, Some(20));
        assert_eq!(subscriber_config.dispatch_timeout_ms, Some(3000));
        assert_eq!(subscriber_config.delay_seconds, Some(10));
        assert_eq!(
            subscriber_config.backoff_type,
            Some("exponential".to_string())
        );
        assert_eq!(subscriber_config.backoff_delay_ms, Some(2000));
    }

    #[test]
    fn test_from_value_parses_max_priority() {
        let config = json!({ "maxPriority": 10 });
        let result = SubscriberQueueConfig::from_value(Some(&config)).expect("should parse");
        assert_eq!(result.max_priority, Some(10));

        // Absent maxPriority leaves it unset (not a priority queue).
        let without = json!({ "type": "standard" });
        let parsed = SubscriberQueueConfig::from_value(Some(&without)).expect("should parse");
        assert_eq!(parsed.max_priority, None);
    }

    #[test]
    fn test_from_value_with_partial_fields() {
        let config = json!({
            "type": "standard",
            "maxRetries": 3
        });

        let result = SubscriberQueueConfig::from_value(Some(&config));
        assert!(result.is_some());

        let subscriber_config = result.unwrap();
        assert_eq!(subscriber_config.queue_mode, Some("standard".to_string()));
        assert_eq!(subscriber_config.max_retries, Some(3));
        assert_eq!(subscriber_config.concurrency, None);
    }

    #[test]
    fn test_from_value_with_empty_object() {
        let config = json!({});
        let result = SubscriberQueueConfig::from_value(Some(&config));
        assert!(result.is_none());
    }

    #[test]
    fn test_from_value_with_none() {
        let result = SubscriberQueueConfig::from_value(None);
        assert!(result.is_none());
    }

    #[test]
    fn test_from_value_with_non_object() {
        let config = json!("not an object");
        let result = SubscriberQueueConfig::from_value(Some(&config));
        assert!(result.is_none());
    }

    #[test]
    fn test_dispatch_timeout_filters_zero() {
        let unset = SubscriberQueueConfig::default();
        assert_eq!(unset.dispatch_timeout(), None);

        let zero = SubscriberQueueConfig {
            dispatch_timeout_ms: Some(0),
            ..Default::default()
        };
        assert_eq!(zero.dispatch_timeout(), None);

        let bounded = SubscriberQueueConfig {
            dispatch_timeout_ms: Some(5000),
            ..Default::default()
        };
        assert_eq!(
            bounded.dispatch_timeout(),
            Some(std::time::Duration::from_millis(5000))
        );
    }
}
