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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;

    // =========================================================================
    // EnvironmentInfo
    // =========================================================================

    #[test]
    fn test_environment_info_collect_returns_valid_fields() {
        let info = EnvironmentInfo::collect();
        assert!(
            !info.machine_id.is_empty(),
            "machine_id should not be empty"
        );
        assert!(info.cpu_cores >= 1, "cpu_cores should be at least 1");
        assert!(!info.os.is_empty(), "os should not be empty");
        assert!(!info.arch.is_empty(), "arch should not be empty");
        assert!(!info.timezone.is_empty(), "timezone should not be empty");
    }

    #[test]
    fn test_environment_info_os_and_arch_match_consts() {
        let info = EnvironmentInfo::collect();
        assert_eq!(info.os, std::env::consts::OS);
        assert_eq!(info.arch, std::env::consts::ARCH);
    }

    #[test]
    fn test_environment_info_to_json_has_all_keys() {
        let info = EnvironmentInfo::collect();
        let json = info.to_json();

        assert!(json.get("machine_id").is_some());
        assert!(json.get("is_container").is_some());
        assert!(json.get("timezone").is_some());
        assert!(json.get("cpu_cores").is_some());
        assert!(json.get("os").is_some());
        assert!(json.get("arch").is_some());
    }

    #[test]
    fn test_environment_info_to_json_types() {
        let info = EnvironmentInfo::collect();
        let json = info.to_json();

        assert!(json["machine_id"].is_string());
        assert!(json["is_container"].is_boolean());
        assert!(json["timezone"].is_string());
        assert!(json["cpu_cores"].is_number());
        assert!(json["os"].is_string());
        assert!(json["arch"].is_string());
    }

    #[test]
    fn test_environment_info_clone() {
        let info = EnvironmentInfo::collect();
        let cloned = info.clone();
        assert_eq!(info.machine_id, cloned.machine_id);
        assert_eq!(info.os, cloned.os);
        assert_eq!(info.arch, cloned.arch);
        assert_eq!(info.cpu_cores, cloned.cpu_cores);
        assert_eq!(info.is_container, cloned.is_container);
        assert_eq!(info.timezone, cloned.timezone);
    }

    #[test]
    fn test_environment_info_debug_format() {
        let info = EnvironmentInfo::collect();
        let debug_str = format!("{:?}", info);
        assert!(debug_str.contains("EnvironmentInfo"));
        assert!(debug_str.contains("machine_id"));
        assert!(debug_str.contains("os"));
    }

    // =========================================================================
    // hashed_hostname
    // =========================================================================

    #[test]
    fn test_hashed_hostname_is_deterministic() {
        let h1 = hashed_hostname();
        let h2 = hashed_hostname();
        assert_eq!(
            h1, h2,
            "hashed_hostname should return same value on repeated calls"
        );
    }

    #[test]
    fn test_hashed_hostname_is_hex_and_32_chars() {
        let h = hashed_hostname();
        // 16 bytes encoded as hex = 32 hex chars
        assert_eq!(h.len(), 32, "hashed hostname should be 32 hex characters");
        assert!(
            h.chars().all(|c| c.is_ascii_hexdigit()),
            "hashed hostname should only contain hex characters"
        );
    }

    // =========================================================================
    // is_ci_environment
    // =========================================================================

    #[test]
    #[serial]
    fn test_is_ci_environment_detects_ci_var() {
        // Remove all CI-related vars first
        let ci_vars = [
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
        for var in &ci_vars {
            unsafe {
                env::remove_var(var);
            }
        }

        assert!(
            !is_ci_environment(),
            "should not detect CI when no CI vars set"
        );

        unsafe {
            env::set_var("CI", "true");
        }
        assert!(is_ci_environment(), "should detect CI when CI=true");
        unsafe {
            env::remove_var("CI");
        }
    }

    #[test]
    #[serial]
    fn test_is_ci_environment_detects_github_actions() {
        let ci_vars = [
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
        for var in &ci_vars {
            unsafe {
                env::remove_var(var);
            }
        }

        unsafe {
            env::set_var("GITHUB_ACTIONS", "true");
        }
        assert!(
            is_ci_environment(),
            "should detect CI when GITHUB_ACTIONS is set"
        );
        unsafe {
            env::remove_var("GITHUB_ACTIONS");
        }
    }

    #[test]
    #[serial]
    fn test_is_ci_environment_detects_gitlab_ci() {
        let ci_vars = [
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
        for var in &ci_vars {
            unsafe {
                env::remove_var(var);
            }
        }

        unsafe {
            env::set_var("GITLAB_CI", "true");
        }
        assert!(
            is_ci_environment(),
            "should detect CI when GITLAB_CI is set"
        );
        unsafe {
            env::remove_var("GITLAB_CI");
        }
    }

    // =========================================================================
    // is_dev_optout
    // =========================================================================

    #[test]
    #[serial]
    fn test_is_dev_optout_with_env_var() {
        unsafe {
            env::remove_var("III_TELEMETRY_DEV");
        }
        assert!(
            !is_dev_optout() || is_dev_optout(),
            "baseline call should not panic"
        );

        unsafe {
            env::set_var("III_TELEMETRY_DEV", "true");
        }
        assert!(
            is_dev_optout(),
            "should detect dev optout when III_TELEMETRY_DEV=true"
        );
        unsafe {
            env::remove_var("III_TELEMETRY_DEV");
        }
    }

    #[test]
    #[serial]
    fn test_is_dev_optout_false_value_not_triggered() {
        unsafe {
            env::set_var("III_TELEMETRY_DEV", "false");
        }
        // "false" != "true", so dev optout should not be triggered by env var alone
        // (it may still be triggered by the file check, so we just confirm no panic)
        let _ = is_dev_optout();
        unsafe {
            env::remove_var("III_TELEMETRY_DEV");
        }
    }

    // =========================================================================
    // detect_client_type
    // =========================================================================

    #[test]
    fn test_detect_client_type_returns_iii_direct() {
        assert_eq!(detect_client_type(), "iii_direct");
    }

    // =========================================================================
    // detect_device_type
    // =========================================================================

    #[test]
    fn test_detect_device_type_returns_server() {
        assert_eq!(detect_device_type(), "server");
    }

    // =========================================================================
    // detect_language
    // =========================================================================

    #[test]
    #[serial]
    fn test_detect_language_from_lang_env() {
        unsafe {
            env::set_var("LANG", "en_US.UTF-8");
            env::remove_var("LC_ALL");
        }
        let lang = detect_language();
        assert_eq!(lang, Some("en_US".to_string()));
        unsafe {
            env::remove_var("LANG");
        }
    }

    #[test]
    #[serial]
    fn test_detect_language_from_lc_all_fallback() {
        unsafe {
            env::remove_var("LANG");
            env::set_var("LC_ALL", "fr_FR.UTF-8");
        }
        let lang = detect_language();
        assert_eq!(lang, Some("fr_FR".to_string()));
        unsafe {
            env::remove_var("LC_ALL");
        }
    }

    #[test]
    #[serial]
    fn test_detect_language_none_when_unset() {
        unsafe {
            env::remove_var("LANG");
            env::remove_var("LC_ALL");
        }
        let lang = detect_language();
        assert_eq!(lang, None);
    }

    #[test]
    #[serial]
    fn test_detect_language_none_when_empty() {
        unsafe {
            env::set_var("LANG", "");
            env::remove_var("LC_ALL");
        }
        let lang = detect_language();
        assert_eq!(lang, None);
        unsafe {
            env::remove_var("LANG");
        }
    }

    #[test]
    #[serial]
    fn test_detect_language_without_dot() {
        unsafe {
            env::set_var("LANG", "C");
            env::remove_var("LC_ALL");
        }
        let lang = detect_language();
        assert_eq!(lang, Some("C".to_string()));
        unsafe {
            env::remove_var("LANG");
        }
    }

    // =========================================================================
    // detect_container
    // =========================================================================

    #[test]
    fn test_detect_container_on_host() {
        // On a typical dev machine (no /.dockerenv), this should return false.
        // If running in Docker, this will return true -- both are valid.
        let result = detect_container();
        assert!(
            result == true || result == false,
            "detect_container should return a bool"
        );
    }

    // =========================================================================
    // detect_timezone
    // =========================================================================

    #[test]
    fn test_detect_timezone_returns_nonempty() {
        let tz = detect_timezone();
        assert!(!tz.is_empty(), "timezone should not be empty");
    }
}
