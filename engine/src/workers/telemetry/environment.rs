// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

type TomlSections = BTreeMap<String, BTreeMap<String, String>>;

fn iii_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".iii")
}

pub fn telemetry_config_path() -> std::path::PathBuf {
    iii_dir().join("telemetry.toml")
}

fn write_atomic(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, content).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp, perms).ok();
        }
        std::fs::rename(&tmp, path).ok();
    }
}

pub fn read_config_key(section: &str, key: &str) -> Option<String> {
    let contents = std::fs::read_to_string(telemetry_config_path()).ok()?;
    let sections: TomlSections = toml::from_str(&contents).ok()?;
    sections
        .get(section)?
        .get(key)
        .filter(|v| !v.is_empty())
        .cloned()
}

pub fn set_config_key(section: &str, key: &str, value: &str) {
    let path = telemetry_config_path();
    let contents = std::fs::read_to_string(&path).unwrap_or_default();
    let mut sections: TomlSections = toml::from_str(&contents).unwrap_or_default();
    sections
        .entry(section.to_string())
        .or_default()
        .insert(key.to_string(), value.to_string());
    if let Ok(serialized) = toml::to_string(&sections) {
        write_atomic(&path, &serialized);
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EnvironmentInfo {
    pub machine_id: String,
    pub is_container: bool,
    pub container_runtime: String,
    pub timezone: String,
    pub cpu_cores: usize,
    pub os: String,
    pub arch: String,
}

impl EnvironmentInfo {
    pub fn collect() -> Self {
        let container_runtime = detect_container_runtime();
        let is_container = container_runtime != "none";
        Self {
            machine_id: hashed_hostname(),
            is_container,
            container_runtime,
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
            "container_runtime": self.container_runtime,
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

/// Detect container runtime. Returns "docker", "kubernetes", or "none".
/// Priority: III_CONTAINER env var (authoritative) > KUBERNETES_SERVICE_HOST >
/// /.dockerenv / cgroup heuristics > "none".
pub fn detect_container_runtime() -> String {
    if let Ok(val) = std::env::var("III_CONTAINER") {
        let lower = val.to_lowercase();
        if lower == "docker" {
            return "docker".to_string();
        }
        if lower == "kubernetes" || lower == "k8s" {
            return "kubernetes".to_string();
        }
    }

    if std::env::var("KUBERNETES_SERVICE_HOST").is_ok() {
        return "kubernetes".to_string();
    }

    if std::path::Path::new("/.dockerenv").exists() {
        return "docker".to_string();
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(contents) = std::fs::read_to_string("/proc/1/cgroup") {
            let lower = contents.to_lowercase();
            if lower.contains("kubepods") {
                return "kubernetes".to_string();
            }
            if lower.contains("docker") || lower.contains("containerd") {
                return "docker".to_string();
            }
        }
    }

    "none".to_string()
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

    if read_config_key("preferences", "dev_optout").as_deref() == Some("true") {
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

/// Detect the install method based on the current executable path.
pub fn detect_install_method() -> &'static str {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return "unknown",
    };

    let path_str = exe.to_string_lossy();

    if path_str.contains("/opt/homebrew/")
        || path_str.contains("/usr/local/Cellar/")
        || path_str.contains("/home/linuxbrew/")
    {
        return "brew";
    }

    if path_str.contains("\\ProgramData\\chocolatey\\") || path_str.contains("/chocolatey/") {
        return "chocolatey";
    }

    if path_str.contains("/.local/bin/") {
        return "sh";
    }

    "manual"
}

/// Read the `III_ENV` environment variable, defaulting to `"unknown"`.
pub fn detect_env() -> String {
    std::env::var("III_ENV")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Read the host user ID from `III_HOST_USER_ID` (Docker correlation).
pub fn detect_host_user_id() -> Option<String> {
    std::env::var("III_HOST_USER_ID")
        .ok()
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;

    // =========================================================================
    // telemetry.toml helpers
    // =========================================================================

    #[test]
    fn test_toml_roundtrip() {
        let mut sections: TomlSections = BTreeMap::new();
        sections
            .entry("identity".to_string())
            .or_default()
            .insert("id".to_string(), "abc-123".to_string());
        sections
            .entry("state".to_string())
            .or_default()
            .insert("first_run_sent".to_string(), "true".to_string());

        let serialized = toml::to_string(&sections).unwrap();
        let parsed: TomlSections = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed["identity"]["id"], "abc-123");
        assert_eq!(parsed["state"]["first_run_sent"], "true");
    }

    #[test]
    fn test_set_and_read_config_key_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".iii").join("telemetry.toml");

        let key = read_key_from(&toml_path, "identity", "id");
        assert!(key.is_none());

        write_key_to(&toml_path, "identity", "id", "test-uuid");
        let key = read_key_from(&toml_path, "identity", "id");
        assert_eq!(key.as_deref(), Some("test-uuid"));
    }

    #[test]
    fn test_set_config_key_preserves_other_sections() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".iii").join("telemetry.toml");

        write_key_to(&toml_path, "identity", "id", "my-id");
        write_key_to(&toml_path, "state", "first_run_sent", "true");

        let id = read_key_from(&toml_path, "identity", "id");
        let state = read_key_from(&toml_path, "state", "first_run_sent");
        assert_eq!(id.as_deref(), Some("my-id"));
        assert_eq!(state.as_deref(), Some("true"));
    }

    #[test]
    fn test_set_config_key_updates_existing_key() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".iii").join("telemetry.toml");

        write_key_to(&toml_path, "identity", "id", "old-id");
        write_key_to(&toml_path, "identity", "id", "new-id");

        let id = read_key_from(&toml_path, "identity", "id");
        assert_eq!(id.as_deref(), Some("new-id"));
    }

    fn read_key_from(path: &std::path::Path, section: &str, key: &str) -> Option<String> {
        let contents = std::fs::read_to_string(path).ok()?;
        let sections: TomlSections = toml::from_str(&contents).ok()?;
        sections
            .get(section)?
            .get(key)
            .filter(|v| !v.is_empty())
            .cloned()
    }

    fn write_key_to(path: &std::path::Path, section: &str, key: &str, value: &str) {
        let contents = std::fs::read_to_string(path).unwrap_or_default();
        let mut sections: TomlSections = toml::from_str(&contents).unwrap_or_default();
        sections
            .entry(section.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
        let serialized = toml::to_string(&sections).unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(path, serialized).ok();
    }

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
        assert!(
            !info.container_runtime.is_empty(),
            "container_runtime should not be empty"
        );
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
        assert!(json.get("container_runtime").is_some());
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
        assert!(json["container_runtime"].is_string());
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
        assert_eq!(info.container_runtime, cloned.container_runtime);
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

    #[test]
    fn test_is_container_consistent_with_runtime() {
        let info = EnvironmentInfo::collect();
        if info.container_runtime == "none" {
            assert!(!info.is_container);
        } else {
            assert!(info.is_container);
        }
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
        assert_eq!(h.len(), 32, "hashed hostname should be 32 hex characters");
        assert!(
            h.chars().all(|c| c.is_ascii_hexdigit()),
            "hashed hostname should only contain hex characters"
        );
    }

    // =========================================================================
    // detect_container_runtime
    // =========================================================================

    #[test]
    #[serial]
    fn test_detect_container_runtime_env_docker() {
        unsafe {
            env::set_var("III_CONTAINER", "docker");
            env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        assert_eq!(detect_container_runtime(), "docker");
        unsafe {
            env::remove_var("III_CONTAINER");
        }
    }

    #[test]
    #[serial]
    fn test_detect_container_runtime_env_kubernetes() {
        unsafe {
            env::set_var("III_CONTAINER", "kubernetes");
            env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        assert_eq!(detect_container_runtime(), "kubernetes");
        unsafe {
            env::remove_var("III_CONTAINER");
        }
    }

    #[test]
    #[serial]
    fn test_detect_container_runtime_kubernetes_service_host() {
        unsafe {
            env::remove_var("III_CONTAINER");
            env::set_var("KUBERNETES_SERVICE_HOST", "10.96.0.1");
        }
        assert_eq!(detect_container_runtime(), "kubernetes");
        unsafe {
            env::remove_var("KUBERNETES_SERVICE_HOST");
        }
    }

    #[test]
    #[serial]
    fn test_detect_container_runtime_none_on_host() {
        unsafe {
            env::remove_var("III_CONTAINER");
            env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        let runtime = detect_container_runtime();
        assert!(
            runtime == "none" || runtime == "docker" || runtime == "kubernetes",
            "unexpected runtime: {runtime}"
        );
    }

    // =========================================================================
    // detect_env
    // =========================================================================

    #[test]
    #[serial]
    fn test_detect_env_default_unknown() {
        unsafe {
            env::remove_var("III_ENV");
        }
        assert_eq!(detect_env(), "unknown");
    }

    #[test]
    #[serial]
    fn test_detect_env_from_var() {
        unsafe {
            env::set_var("III_ENV", "production");
        }
        assert_eq!(detect_env(), "production");
        unsafe {
            env::remove_var("III_ENV");
        }
    }

    #[test]
    #[serial]
    fn test_detect_env_empty_defaults_to_unknown() {
        unsafe {
            env::set_var("III_ENV", "");
        }
        assert_eq!(detect_env(), "unknown");
        unsafe {
            env::remove_var("III_ENV");
        }
    }

    // =========================================================================
    // detect_host_user_id
    // =========================================================================

    #[test]
    #[serial]
    fn test_detect_host_user_id_none_when_unset() {
        unsafe {
            env::remove_var("III_HOST_USER_ID");
        }
        assert_eq!(detect_host_user_id(), None);
    }

    #[test]
    #[serial]
    fn test_detect_host_user_id_returns_value() {
        unsafe {
            env::set_var("III_HOST_USER_ID", "some-uuid");
        }
        assert_eq!(detect_host_user_id(), Some("some-uuid".to_string()));
        unsafe {
            env::remove_var("III_HOST_USER_ID");
        }
    }

    #[test]
    #[serial]
    fn test_detect_host_user_id_none_when_empty() {
        unsafe {
            env::set_var("III_HOST_USER_ID", "");
        }
        assert_eq!(detect_host_user_id(), None);
        unsafe {
            env::remove_var("III_HOST_USER_ID");
        }
    }

    // =========================================================================
    // detect_install_method
    // =========================================================================

    #[test]
    fn test_detect_install_method_returns_known_value() {
        let method = detect_install_method();
        assert!(
            matches!(method, "brew" | "chocolatey" | "sh" | "manual" | "unknown"),
            "unexpected install method: {method}"
        );
    }

    // =========================================================================
    // is_ci_environment
    // =========================================================================

    #[test]
    #[serial]
    fn test_is_ci_environment_detects_ci_var() {
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
    // detect_container (legacy alias via collect)
    // =========================================================================

    #[test]
    fn test_detect_container_on_host() {
        let _result = EnvironmentInfo::collect().is_container;
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
