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
