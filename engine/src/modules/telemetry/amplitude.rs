// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::Serialize;

const AMPLITUDE_ENDPOINT: &str = "https://api2.amplitude.com/2/httpapi";
const MAX_RETRIES: u32 = 3;

/// An event to be sent to Amplitude.
#[derive(Debug, Clone, Serialize)]
pub struct AmplitudeEvent {
    pub device_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    pub event_type: String,
    pub event_properties: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_properties: Option<serde_json::Value>,
    pub platform: String,
    pub os_name: String,
    pub app_version: String,
    pub time: i64,
    pub insert_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
}

/// Payload sent to Amplitude HTTP API.
#[derive(Serialize)]
struct AmplitudePayload {
    api_key: String,
    events: Vec<AmplitudeEvent>,
}

/// Client for sending events to Amplitude.
pub struct AmplitudeClient {
    api_key: String,
    client: reqwest::Client,
}

impl AmplitudeClient {
    /// Create a new Amplitude client with the given API key.
    pub fn new(api_key: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Failed to build Amplitude HTTP client with custom config, using defaults");
                reqwest::Client::default()
            });

        Self { api_key, client }
    }

    /// Send a single event to Amplitude.
    pub async fn send_event(&self, event: AmplitudeEvent) -> anyhow::Result<()> {
        self.send_batch(vec![event]).await
    }

    /// Send a batch of events to Amplitude.
    /// If the API key is empty, this silently skips sending (for dev/testing).
    /// Uses exponential backoff (1s, 2s, 4s) with 3 attempts max.
    /// Returns `Ok(())` even when all retries are exhausted — telemetry is fire-and-forget
    /// and must never block or fail the caller.
    pub async fn send_batch(&self, events: Vec<AmplitudeEvent>) -> anyhow::Result<()> {
        if self.api_key.is_empty() {
            return Ok(());
        }

        if events.is_empty() {
            return Ok(());
        }

        let payload = AmplitudePayload {
            api_key: self.api_key.clone(),
            events,
        };

        let mut delay = std::time::Duration::from_secs(1);

        for attempt in 1..=MAX_RETRIES {
            match self
                .client
                .post(AMPLITUDE_ENDPOINT)
                .json(&payload)
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => {
                    return Ok(());
                }
                Ok(_) | Err(_) => {}
            }

            if attempt < MAX_RETRIES {
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
        }

        tracing::debug!("Amplitude: all retry attempts exhausted, dropping events");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> AmplitudeEvent {
        AmplitudeEvent {
            device_id: "device-1".to_string(),
            user_id: Some("user-1".to_string()),
            event_type: "test_event".to_string(),
            event_properties: serde_json::json!({"key": "value"}),
            user_properties: Some(serde_json::json!({"plan": "free"})),
            platform: "test".to_string(),
            os_name: "linux".to_string(),
            app_version: "0.1.0".to_string(),
            time: 1700000000000,
            insert_id: Some("ins-1".to_string()),
            country: Some("US".to_string()),
            language: Some("en".to_string()),
            ip: Some("$remote".to_string()),
        }
    }

    // =========================================================================
    // AmplitudeEvent serialization
    // =========================================================================

    #[test]
    fn test_event_serialization_required_fields() {
        let event = sample_event();
        let json = serde_json::to_value(&event).unwrap();

        assert_eq!(json["device_id"], "device-1");
        assert_eq!(json["user_id"], "user-1");
        assert_eq!(json["event_type"], "test_event");
        assert_eq!(json["event_properties"]["key"], "value");
        assert_eq!(json["platform"], "test");
        assert_eq!(json["os_name"], "linux");
        assert_eq!(json["app_version"], "0.1.0");
        assert_eq!(json["time"], 1700000000000i64);
        assert_eq!(json["insert_id"], "ins-1");
        assert_eq!(json["country"], "US");
        assert_eq!(json["language"], "en");
        assert_eq!(json["ip"], "$remote");
    }

    #[test]
    fn test_event_serialization_skip_none_fields() {
        let event = AmplitudeEvent {
            device_id: "d1".to_string(),
            user_id: None,
            event_type: "evt".to_string(),
            event_properties: serde_json::json!({}),
            user_properties: None,
            platform: "test".to_string(),
            os_name: "macos".to_string(),
            app_version: "1.0.0".to_string(),
            time: 0,
            insert_id: None,
            country: None,
            language: None,
            ip: None,
        };

        let json = serde_json::to_value(&event).unwrap();

        // Fields with skip_serializing_if = "Option::is_none" should be absent
        assert!(
            json.get("user_id").is_none(),
            "user_id=None should be skipped"
        );
        assert!(
            json.get("user_properties").is_none(),
            "user_properties=None should be skipped"
        );
        assert!(
            json.get("country").is_none(),
            "country=None should be skipped"
        );
        assert!(
            json.get("language").is_none(),
            "language=None should be skipped"
        );
        assert!(json.get("ip").is_none(), "ip=None should be skipped");

        // Required fields should still be present
        assert!(json.get("device_id").is_some());
        assert!(json.get("event_type").is_some());
        assert!(json.get("event_properties").is_some());
        assert!(json.get("platform").is_some());
        assert!(json.get("os_name").is_some());
        assert!(json.get("app_version").is_some());
        assert!(json.get("time").is_some());
    }

    #[test]
    fn test_event_clone() {
        let event = sample_event();
        let cloned = event.clone();
        assert_eq!(event.device_id, cloned.device_id);
        assert_eq!(event.event_type, cloned.event_type);
        assert_eq!(event.time, cloned.time);
        assert_eq!(event.insert_id, cloned.insert_id);
    }

    #[test]
    fn test_event_debug_format() {
        let event = sample_event();
        let debug = format!("{:?}", event);
        assert!(debug.contains("AmplitudeEvent"));
        assert!(debug.contains("test_event"));
        assert!(debug.contains("device-1"));
    }

    #[test]
    fn test_event_roundtrip_json() {
        let event = sample_event();
        let json_str = serde_json::to_string(&event).unwrap();
        // Verify it's valid JSON by parsing it back
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.is_object());
        assert_eq!(parsed["event_type"], "test_event");
    }

    // =========================================================================
    // AmplitudePayload serialization
    // =========================================================================

    #[test]
    fn test_payload_serialization() {
        let payload = AmplitudePayload {
            api_key: "test-key".to_string(),
            events: vec![sample_event()],
        };

        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["api_key"], "test-key");
        assert!(json["events"].is_array());
        assert_eq!(json["events"].as_array().unwrap().len(), 1);
        assert_eq!(json["events"][0]["event_type"], "test_event");
    }

    #[test]
    fn test_payload_empty_events() {
        let payload = AmplitudePayload {
            api_key: "k".to_string(),
            events: vec![],
        };

        let json = serde_json::to_value(&payload).unwrap();
        assert!(json["events"].as_array().unwrap().is_empty());
    }

    // =========================================================================
    // AmplitudeClient::send_batch with empty key (no-op)
    // =========================================================================

    #[tokio::test]
    async fn test_send_batch_empty_api_key_is_noop() {
        let client = AmplitudeClient::new(String::new());
        let result = client.send_batch(vec![sample_event()]).await;
        assert!(result.is_ok(), "empty API key should silently succeed");
    }

    #[tokio::test]
    async fn test_send_batch_empty_events_is_noop() {
        let client = AmplitudeClient::new("some-key".to_string());
        let result = client.send_batch(vec![]).await;
        assert!(result.is_ok(), "empty events vec should silently succeed");
    }

    #[tokio::test]
    async fn test_send_event_empty_api_key_is_noop() {
        let client = AmplitudeClient::new(String::new());
        let result = client.send_event(sample_event()).await;
        assert!(
            result.is_ok(),
            "send_event with empty API key should succeed"
        );
    }

    #[tokio::test]
    async fn test_send_batch_retries_and_drops_on_transport_errors() {
        let client = AmplitudeClient {
            api_key: "test-key".to_string(),
            client: reqwest::Client::builder()
                .proxy(reqwest::Proxy::all("http://127.0.0.1:9").expect("build proxy"))
                .timeout(std::time::Duration::from_millis(20))
                .build()
                .expect("build reqwest client"),
        };

        let result = client.send_batch(vec![sample_event()]).await;
        assert!(result.is_ok(), "telemetry failures should be swallowed");
    }

    // =========================================================================
    // Constants
    // =========================================================================

    #[test]
    fn test_amplitude_endpoint_is_https() {
        assert!(
            AMPLITUDE_ENDPOINT.starts_with("https://"),
            "Amplitude endpoint should use HTTPS"
        );
    }

    #[test]
    fn test_max_retries_is_three() {
        assert_eq!(MAX_RETRIES, 3);
    }
}
