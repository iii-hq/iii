// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use super::collector::HeartbeatEntry;
use super::config::HeartbeatConfig;

pub fn is_telemetry_enabled(config: &HeartbeatConfig) -> bool {
    if std::env::var("III_DISABLE_TELEMETRY").is_ok() {
        return false;
    }
    if std::env::var("DO_NOT_TRACK").is_ok() {
        return false;
    }
    if is_ci() {
        return false;
    }
    config.is_enabled()
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

pub async fn report_to_cloud(endpoint: &str, entry: &HeartbeatEntry) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(_) => return,
    };

    let _ = client.post(endpoint).json(entry).send().await;
}
