// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, serde::Serialize)]
pub struct EnvironmentInfo {
    pub machine_id: String,
    pub is_container: bool,
    pub timezone: String,
    pub cpu_cores: usize,
    pub os: String,
    pub arch: String,
}

impl EnvironmentInfo {
    pub fn collect() -> Self {
        Self {
            machine_id: hashed_hostname(),
            is_container: detect_container(),
            timezone: detect_timezone(),
            cpu_cores: std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(1),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "machine_id": self.machine_id,
            "is_container": self.is_container,
            "timezone": self.timezone,
            "cpu_cores": self.cpu_cores,
            "os": self.os,
            "arch": self.arch,
        })
    }
}

fn hashed_hostname() -> String {
    let raw = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());

    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..16])
}

fn detect_container() -> bool {
    if std::path::Path::new("/.dockerenv").exists() {
        return true;
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(contents) = std::fs::read_to_string("/proc/1/cgroup") {
            let lower = contents.to_lowercase();
            if lower.contains("docker")
                || lower.contains("containerd")
                || lower.contains("kubepods")
            {
                return true;
            }
        }
    }

    false
}

fn detect_timezone() -> String {
    if let Ok(tz) = iana_time_zone::get_timezone()
        && !tz.is_empty()
    {
        return tz;
    }

    std::env::var("TZ").unwrap_or_else(|_| "Unknown".to_string())
}

pub fn is_ci_environment() -> bool {
    const CI_ENV_VARS: &[&str] = &[
        "CI",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "CIRCLECI",
        "JENKINS_URL",
        "TRAVIS",
        "BUILDKITE",
        "TF_BUILD",
        "CODEBUILD_BUILD_ID",
        "BITBUCKET_BUILD_NUMBER",
        "DRONE",
        "TEAMCITY_VERSION",
    ];

    CI_ENV_VARS.iter().any(|var| std::env::var(var).is_ok())
}

pub fn is_dev_optout() -> bool {
    if std::env::var("III_TELEMETRY_DEV").ok().as_deref() == Some("true") {
        return true;
    }

    let base_dir = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
    base_dir.join(".iii").join("telemetry_dev_optout").exists()
}

pub fn detect_client_type() -> &'static str {
    "iii_direct"
}

pub fn detect_client_type_from_workers(
    _worker_names: &std::collections::HashSet<String>,
) -> &'static str {
    detect_client_type()
}

pub fn detect_language() -> Option<String> {
    std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| s.split('.').next().unwrap_or(&s).to_string())
}

pub fn detect_device_type() -> &'static str {
    "server"
}
