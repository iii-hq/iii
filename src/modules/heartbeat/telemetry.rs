// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use super::collector::CloudSafeHeartbeatEntry;
use super::config::HeartbeatConfig;
use super::lifecycle::LifecycleEvent;
use std::net::IpAddr;

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

pub fn validate_cloud_endpoint(endpoint: &str) -> Result<(), String> {
    let url =
        reqwest::Url::parse(endpoint).map_err(|e| format!("invalid cloud endpoint URL: {}", e))?;

    if url.scheme() != "https" {
        return Err("cloud endpoint must use HTTPS".to_string());
    }

    if let Some(host) = url.host_str() {
        if let Ok(ip) = host.parse::<IpAddr>()
            && !ip_is_global(&ip)
        {
            return Err(format!(
                "cloud endpoint must not point to private/link-local address: {}",
                ip
            ));
        }

        if host == "localhost" || host == "127.0.0.1" || host == "::1" {
            return Err("cloud endpoint must not point to localhost".to_string());
        }

        if host == "169.254.169.254" {
            return Err("cloud endpoint must not point to cloud metadata service".to_string());
        }
    }

    Ok(())
}

fn ip_is_global(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !v4.is_private()
                && !v4.is_loopback()
                && !v4.is_link_local()
                && !v4.is_broadcast()
                && !v4.is_unspecified()
        }
        IpAddr::V6(v6) => !v6.is_loopback() && !v6.is_unspecified(),
    }
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
        Err(e) => {
            tracing::debug!("[HEARTBEAT] Cloud telemetry POST failed: {}", e);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_http_endpoint() {
        assert!(validate_cloud_endpoint("http://example.com/api").is_err());
    }

    #[test]
    fn accepts_https_endpoint() {
        assert!(validate_cloud_endpoint("https://api.motia.dev/telemetry").is_ok());
    }

    #[test]
    fn rejects_localhost() {
        assert!(validate_cloud_endpoint("https://localhost/api").is_err());
    }

    #[test]
    fn rejects_private_ip() {
        assert!(validate_cloud_endpoint("https://192.168.1.1/api").is_err());
        assert!(validate_cloud_endpoint("https://10.0.0.1/api").is_err());
    }

    #[test]
    fn rejects_metadata_endpoint() {
        assert!(validate_cloud_endpoint("https://169.254.169.254/latest").is_err());
    }

    #[test]
    fn rejects_invalid_url() {
        assert!(validate_cloud_endpoint("not-a-url").is_err());
    }
}
