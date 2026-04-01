// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use machineid_rs::{Encryption, HWIDComponent, IdBuilder};

const TELEMETRY_SCHEMA_VERSION: u8 = 2;
const DEVICE_ID_SALT: &str = "iii-machine-id";
const EXECUTION_CONTEXT_ENV: &str = "III_EXECUTION_CONTEXT";
const EXECUTION_CONTEXT_YAML_DEFAULT: &str = "${III_EXECUTION_CONTEXT:user}";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct TelemetryYaml {
    version: Option<u8>,
    #[serde(default)]
    identity: IdentitySection,
    #[serde(default)]
    state: StateSection,
    #[serde(default)]
    iii_execution_context: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct IdentitySection {
    #[serde(default)]
    device_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct StateSection {
    #[serde(default)]
    first_run_sent: Option<bool>,
}

fn iii_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".iii")
}

pub fn telemetry_config_path() -> std::path::PathBuf {
    iii_dir().join("telemetry.yaml")
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

fn normalize_execution_context(value: &str) -> String {
    match value.trim().to_lowercase().as_str() {
        "kubernetes" | "k8s" => "kubernetes".to_string(),
        "docker" => "docker".to_string(),
        "container" => "container".to_string(),
        "ci" | "cicd" => "ci".to_string(),
        "user" => "user".to_string(),
        "" => "user".to_string(),
        _ => "unknown".to_string(),
    }
}

fn parse_yaml_env_syntax(raw: &str) -> Option<(String, Option<String>)> {
    if !(raw.starts_with("${") && raw.ends_with('}')) {
        return None;
    }

    let inner = &raw[2..raw.len() - 1];
    let mut parts = inner.splitn(2, ':');
    let var = parts.next()?.trim();
    if var.is_empty() {
        return None;
    }
    let default = parts.next().map(|s| s.to_string());
    Some((var.to_string(), default))
}

fn expand_yaml_env_syntax(raw: &str) -> String {
    if let Some((var, default)) = parse_yaml_env_syntax(raw) {
        match std::env::var(&var) {
            Ok(val) if !val.is_empty() => val,
            _ => default.unwrap_or_default(),
        }
    } else {
        raw.to_string()
    }
}

fn machine_id_from_machineid_rs() -> Option<String> {
    let mut builder = IdBuilder::new(Encryption::SHA256);
    builder.add_component(HWIDComponent::SystemID);
    builder
        .build(DEVICE_ID_SALT)
        .ok()
        .filter(|id| !id.trim().is_empty())
}

fn generate_new_device_id() -> String {
    if let Some(machine_id) = machine_id_from_machineid_rs() {
        return machine_id;
    }
    format!("fallback-{}", uuid::Uuid::new_v4())
}

fn build_fresh_v2_yaml() -> TelemetryYaml {
    TelemetryYaml {
        version: Some(TELEMETRY_SCHEMA_VERSION),
        identity: IdentitySection {
            device_id: Some(generate_new_device_id()),
        },
        state: StateSection::default(),
        iii_execution_context: Some(EXECUTION_CONTEXT_YAML_DEFAULT.to_string()),
    }
}

fn read_telemetry_yaml(path: &std::path::Path) -> Option<TelemetryYaml> {
    let contents = std::fs::read_to_string(path).ok()?;
    serde_yaml::from_str(&contents).ok()
}

fn write_telemetry_yaml(state: &TelemetryYaml) {
    if let Ok(serialized) = serde_yaml::to_string(state) {
        write_atomic(&telemetry_config_path(), &serialized);
    }
}

fn load_or_migrate_v2_state() -> TelemetryYaml {
    let path = telemetry_config_path();
    let parsed = read_telemetry_yaml(&path);

    if let Some(mut state) = parsed
        && state.version == Some(TELEMETRY_SCHEMA_VERSION)
    {
        let mut changed = false;
        if state.identity.device_id.is_none() {
            state.identity.device_id = Some(generate_new_device_id());
            changed = true;
        }
        if state.iii_execution_context.is_none() {
            state.iii_execution_context = Some(EXECUTION_CONTEXT_YAML_DEFAULT.to_string());
            changed = true;
        }
        if changed {
            write_telemetry_yaml(&state);
        }
        return state;
    }

    let fresh = build_fresh_v2_yaml();
    write_telemetry_yaml(&fresh);
    fresh
}

pub fn get_or_create_device_id() -> String {
    let mut state = load_or_migrate_v2_state();
    if let Some(existing) = state.identity.device_id.clone()
        && !existing.is_empty()
    {
        return existing;
    }

    let id = generate_new_device_id();
    state.identity.device_id = Some(id.clone());
    write_telemetry_yaml(&state);
    id
}

pub fn resolve_execution_context() -> String {
    if let Ok(env_ctx) = std::env::var(EXECUTION_CONTEXT_ENV)
        && !env_ctx.is_empty()
    {
        return normalize_execution_context(&env_ctx);
    }

    if is_ci_environment() {
        return "ci".to_string();
    }

    let runtime = detect_container_runtime();
    if runtime != "none" {
        return runtime;
    }

    let mut state = load_or_migrate_v2_state();
    if state.iii_execution_context.is_none() {
        state.iii_execution_context = Some(EXECUTION_CONTEXT_YAML_DEFAULT.to_string());
        write_telemetry_yaml(&state);
    }

    let raw = state
        .iii_execution_context
        .clone()
        .unwrap_or_else(|| EXECUTION_CONTEXT_YAML_DEFAULT.to_string());
    normalize_execution_context(&expand_yaml_env_syntax(&raw))
}

pub fn read_config_key(section: &str, key: &str) -> Option<String> {
    let state = load_or_migrate_v2_state();
    match (section, key) {
        ("identity", "device_id") | ("identity", "id") => {
            state.identity.device_id.filter(|v| !v.is_empty())
        }
        ("state", "first_run_sent") => state.state.first_run_sent.map(|v| v.to_string()),
        ("telemetry", "iii_execution_context") => state.iii_execution_context,
        _ => None,
    }
}

pub fn set_config_key(section: &str, key: &str, value: &str) {
    let mut state = load_or_migrate_v2_state();
    match (section, key) {
        ("identity", "device_id") | ("identity", "id") => {
            state.identity.device_id = Some(value.to_string());
        }
        ("state", "first_run_sent") => {
            state.state.first_run_sent = Some(value.eq_ignore_ascii_case("true") || value == "1");
        }
        ("telemetry", "iii_execution_context") => {
            state.iii_execution_context = Some(value.to_string());
        }
        _ => {}
    }
    state.version = Some(TELEMETRY_SCHEMA_VERSION);
    write_telemetry_yaml(&state);
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EnvironmentInfo {
    pub machine_id: String,
    pub iii_execution_context: String,
    pub timezone: String,
    pub cpu_cores: usize,
    pub os: String,
    pub arch: String,
}

impl EnvironmentInfo {
    pub fn collect() -> Self {
        Self {
            machine_id: get_or_create_device_id(),
            iii_execution_context: resolve_execution_context(),
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
            "iii_execution_context": self.iii_execution_context,
            "timezone": self.timezone,
            "cpu_cores": self.cpu_cores,
            "os": self.os,
            "arch": self.arch,
        })
    }
}

/// Detect whether the process runs inside a container.
/// Returns `"kubernetes"`, `"docker"`, `"container"`, or `"none"`.
pub fn detect_container_runtime() -> String {
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
                return "container".to_string();
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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;

    // =========================================================================
    // telemetry.yaml + migration
    // =========================================================================

    #[test]
    #[serial]
    fn test_telemetry_config_path_is_yaml() {
        assert!(
            telemetry_config_path()
                .to_string_lossy()
                .ends_with("telemetry.yaml")
        );
    }

    #[test]
    #[serial]
    fn test_load_or_migrate_writes_v2_yaml_with_device_id() {
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            env::set_var("HOME", dir.path());
        }
        let state = load_or_migrate_v2_state();
        assert_eq!(state.version, Some(2));
        assert!(state.identity.device_id.is_some());
        assert!(telemetry_config_path().exists());
        unsafe {
            env::remove_var("HOME");
        }
    }

    #[test]
    #[serial]
    fn test_v1_yaml_resets_to_fresh_v2_state() {
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            env::set_var("HOME", dir.path());
        }
        let path = telemetry_config_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"version: 1
identity:
  device_id: "legacy-id"
state:
  first_run_sent: true
"#,
        )
        .unwrap();

        let state = load_or_migrate_v2_state();
        assert_eq!(state.version, Some(2));
        assert_ne!(state.identity.device_id.as_deref(), Some("legacy-id"));
        assert_eq!(state.state.first_run_sent, None);
        unsafe {
            env::remove_var("HOME");
        }
    }

    #[test]
    #[serial]
    fn test_set_and_read_config_key_for_state() {
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            env::set_var("HOME", dir.path());
        }
        set_config_key("state", "first_run_sent", "true");
        assert_eq!(
            read_config_key("state", "first_run_sent").as_deref(),
            Some("true")
        );
        unsafe {
            env::remove_var("HOME");
        }
    }

    #[test]
    #[serial]
    fn test_get_or_create_device_id_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            env::set_var("HOME", dir.path());
        }
        let d1 = get_or_create_device_id();
        let d2 = get_or_create_device_id();
        assert!(!d1.is_empty());
        assert_eq!(d1, d2);
        unsafe {
            env::remove_var("HOME");
        }
    }

    #[test]
    #[serial]
    fn test_resolve_execution_context_prefers_explicit_env() {
        unsafe {
            env::set_var(EXECUTION_CONTEXT_ENV, "docker");
        }
        assert_eq!(resolve_execution_context(), "docker");
        unsafe {
            env::remove_var(EXECUTION_CONTEXT_ENV);
        }
    }

    #[test]
    #[serial]
    fn test_resolve_execution_context_detects_ci() {
        unsafe {
            env::remove_var(EXECUTION_CONTEXT_ENV);
            env::set_var("CI", "true");
        }
        assert_eq!(resolve_execution_context(), "ci");
        unsafe {
            env::remove_var("CI");
        }
    }

    // =========================================================================
    // EnvironmentInfo
    // =========================================================================

    #[test]
    fn test_environment_info_collect_returns_valid_fields() {
        let info = EnvironmentInfo::collect();
        assert!(!info.machine_id.is_empty());
        assert!(!info.iii_execution_context.is_empty());
        assert!(info.cpu_cores >= 1);
        assert!(!info.os.is_empty());
        assert!(!info.arch.is_empty());
        assert!(!info.timezone.is_empty());
    }

    // =========================================================================
    // detect_container_runtime
    // =========================================================================

    #[test]
    #[serial]
    fn test_detect_container_runtime_kubernetes_env() {
        unsafe {
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
            env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        let runtime = detect_container_runtime();
        assert!(
            runtime == "none" || runtime == "docker" || runtime == "container" || runtime == "kubernetes",
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
    // detect_timezone
    // =========================================================================

    #[test]
    fn test_detect_timezone_returns_nonempty() {
        let tz = detect_timezone();
        assert!(!tz.is_empty(), "timezone should not be empty");
    }
}
