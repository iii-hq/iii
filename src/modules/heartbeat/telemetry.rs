// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use super::collector::CloudSafeHeartbeatEntry;
use super::config::HeartbeatConfig;
use super::lifecycle::LifecycleEvent;

pub fn check_telemetry_enabled(config: &HeartbeatConfig) -> bool {
    if !config.is_cloud_telemetry_enabled() {
        return false;
    }
    if std::env::var("III_DISABLE_TELEMETRY").is_ok() {
        return false;
    }
    if std::env::var("DO_NOT_TRACK").is_ok() {
        return false;
    }
    if is_ci() {
        return false;
    }
    true
}

fn is_ci() -> bool {
    std::env::var("CI").is_ok()
        || std::env::var("GITHUB_ACTIONS").is_ok()
        || std::env::var("GITLAB_CI").is_ok()
        || std::env::var("CIRCLECI").is_ok()
        || std::env::var("TRAVIS").is_ok()
        || std::env::var("JENKINS_URL").is_ok()
        || std::env::var("BUILDKITE").is_ok()
}

pub fn build_http_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()
}

#[derive(serde::Serialize)]
struct CloudPayload<'a> {
    heartbeat: &'a CloudSafeHeartbeatEntry,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    lifecycle_events: Vec<LifecycleEvent>,
}

pub async fn report_to_cloud(
    client: &reqwest::Client,
    endpoint: &str,
    entry: &CloudSafeHeartbeatEntry,
    lifecycle_events: Vec<LifecycleEvent>,
) -> bool {
    let payload = CloudPayload {
        heartbeat: entry,
        lifecycle_events,
    };

    match client.post(endpoint).json(&payload).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}
