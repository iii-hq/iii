use std::collections::BTreeMap;

use serde::Serialize;
use sha2::{Digest, Sha256};

const AMPLITUDE_ENDPOINT: &str = "https://api2.amplitude.com/2/httpapi";

// ---------------------------------------------------------------------------
// ~/.iii/telemetry.toml helpers (shared format with engine)
// ---------------------------------------------------------------------------

type TomlSections = BTreeMap<String, BTreeMap<String, String>>;

fn telemetry_toml_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".iii")
        .join("telemetry.toml")
}

fn read_toml_key(section: &str, key: &str) -> Option<String> {
    let contents = std::fs::read_to_string(telemetry_toml_path()).ok()?;
    let sections: TomlSections = toml::from_str(&contents).ok()?;
    sections
        .get(section)?
        .get(key)
        .filter(|v| !v.is_empty())
        .cloned()
}

fn set_toml_key(section: &str, key: &str, value: &str) {
    let path = telemetry_toml_path();
    let contents = std::fs::read_to_string(&path).unwrap_or_default();
    let mut sections: TomlSections = toml::from_str(&contents).unwrap_or_default();
    sections
        .entry(section.to_string())
        .or_default()
        .insert(key.to_string(), value.to_string());
    let serialized = match toml::to_string(&sections) {
        Ok(s) => s,
        Err(_) => return,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, &serialized).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp, perms).ok();
        }
        std::fs::rename(&tmp, &path).ok();
    }
}

const API_KEY: &str = "a7182ac460dde671c8f2e1318b517228";

#[derive(Serialize)]
struct AmplitudeEvent {
    device_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_id: Option<String>,
    event_type: String,
    event_properties: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_properties: Option<serde_json::Value>,
    platform: String,
    os_name: String,
    app_version: String,
    time: i64,
    insert_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ip: Option<String>,
}

#[derive(Serialize)]
struct AmplitudePayload<'a> {
    api_key: &'a str,
    events: Vec<AmplitudeEvent>,
}

fn detect_machine_id() -> String {
    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string());
    let mut hasher = Sha256::new();
    hasher.update(hostname.as_bytes());
    let result = hasher.finalize();
    result[..8].iter().map(|b| format!("{:02x}", b)).collect()
}

fn detect_is_container() -> bool {
    if std::env::var("III_CONTAINER").is_ok() {
        return true;
    }
    if std::env::var("KUBERNETES_SERVICE_HOST").is_ok() {
        return true;
    }
    std::path::Path::new("/.dockerenv").exists()
}

fn detect_install_method() -> &'static str {
    if let Ok(exe) = std::env::current_exe() {
        let path = exe.to_string_lossy();
        if path.contains("homebrew") || path.contains("Cellar") || path.contains("linuxbrew") {
            return "brew";
        }
        if path.contains("chocolatey") || path.contains("choco") {
            return "chocolatey";
        }
        if path.contains(".local/bin") {
            return "sh";
        }
    }
    "manual"
}

fn build_user_properties() -> serde_json::Value {
    serde_json::json!({
        "environment.os": std::env::consts::OS,
        "environment.arch": std::env::consts::ARCH,
        "environment.cpu_cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        "environment.timezone": std::env::var("TZ").unwrap_or_else(|_| "Unknown".to_string()),
        "environment.machine_id": detect_machine_id(),
        "environment.is_container": detect_is_container(),
        "env": std::env::var("III_ENV").unwrap_or_else(|_| "unknown".to_string()),
        "install_method": detect_install_method(),
        "cli_version": env!("CARGO_PKG_VERSION"),
        "host_user_id": std::env::var("III_HOST_USER_ID").ok(),
    })
}

fn get_or_create_telemetry_id() -> String {
    if let Some(id) = read_toml_key("identity", "id") {
        return id;
    }

    let legacy_path = dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".iii")
        .join("telemetry_id");
    if let Ok(id) = std::fs::read_to_string(&legacy_path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            set_toml_key("identity", "id", &id);
            return id;
        }
    }

    let id = format!("auto-{}", uuid::Uuid::new_v4());
    set_toml_key("identity", "id", &id);
    id
}

fn is_telemetry_disabled() -> bool {
    if let Ok(val) = std::env::var("III_TELEMETRY_ENABLED") {
        if val == "false" || val == "0" {
            return true;
        }
    }

    if std::env::var("III_TELEMETRY_DEV").ok().as_deref() == Some("true") {
        return true;
    }

    const CI_VARS: &[&str] = &[
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
    if CI_VARS.iter().any(|v| std::env::var(v).is_ok()) {
        return true;
    }

    false
}

fn build_event(event_type: &str, properties: serde_json::Value) -> Option<AmplitudeEvent> {
    if is_telemetry_disabled() {
        return None;
    }

    let telemetry_id = get_or_create_telemetry_id();
    Some(AmplitudeEvent {
        device_id: telemetry_id.clone(),
        // user_id: currently telemetry_id, will become iii cloud user ID when accounts ship
        user_id: Some(telemetry_id),
        event_type: event_type.to_string(),
        event_properties: properties,
        user_properties: Some(build_user_properties()),
        platform: "iii-cli".to_string(),
        os_name: std::env::consts::OS.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        time: chrono::Utc::now().timestamp_millis(),
        insert_id: uuid::Uuid::new_v4().to_string(),
        ip: Some("$remote".to_string()),
    })
}

fn send_fire_and_forget(event: AmplitudeEvent) {
    tokio::spawn(async move {
        let payload = AmplitudePayload {
            api_key: API_KEY,
            events: vec![event],
        };
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build();

        if let Ok(client) = client {
            let _ = client.post(AMPLITUDE_ENDPOINT).json(&payload).send().await;
        }
    });
}

pub fn send_cli_update_started(target_binary: &str, from_version: &str) {
    if let Some(event) = build_event(
        "cli_update_started",
        serde_json::json!({
            "target_binary": target_binary,
            "from_version": from_version,
            "install_method": detect_install_method(),
        }),
    ) {
        send_fire_and_forget(event);
    }
}

pub fn send_cli_update_succeeded(target_binary: &str, from_version: &str, to_version: &str) {
    if let Some(event) = build_event(
        "cli_update_succeeded",
        serde_json::json!({
            "target_binary": target_binary,
            "from_version": from_version,
            "to_version": to_version,
            "install_method": detect_install_method(),
        }),
    ) {
        send_fire_and_forget(event);
    }
}

pub fn send_cli_update_failed(target_binary: &str, from_version: &str, error: &str) {
    if let Some(event) = build_event(
        "cli_update_failed",
        serde_json::json!({
            "target_binary": target_binary,
            "from_version": from_version,
            "error": error,
            "install_method": detect_install_method(),
        }),
    ) {
        send_fire_and_forget(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;

    fn clear_opt_out_vars() {
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
            for v in &[
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
            ] {
                env::remove_var(v);
            }
        }
    }

    #[test]
    #[serial]
    fn test_is_telemetry_disabled_when_env_false() {
        clear_opt_out_vars();
        unsafe { env::set_var("III_TELEMETRY_ENABLED", "false") };
        assert!(is_telemetry_disabled());
        unsafe { env::remove_var("III_TELEMETRY_ENABLED") };
    }

    #[test]
    #[serial]
    fn test_is_telemetry_disabled_when_env_zero() {
        clear_opt_out_vars();
        unsafe { env::set_var("III_TELEMETRY_ENABLED", "0") };
        assert!(is_telemetry_disabled());
        unsafe { env::remove_var("III_TELEMETRY_ENABLED") };
    }

    #[test]
    #[serial]
    fn test_is_telemetry_disabled_dev_optout() {
        clear_opt_out_vars();
        unsafe { env::set_var("III_TELEMETRY_DEV", "true") };
        assert!(is_telemetry_disabled());
        unsafe { env::remove_var("III_TELEMETRY_DEV") };
    }

    #[test]
    #[serial]
    fn test_is_telemetry_disabled_ci_detection() {
        clear_opt_out_vars();
        unsafe { env::set_var("CI", "true") };
        assert!(is_telemetry_disabled());
        unsafe { env::remove_var("CI") };
    }

    #[test]
    #[serial]
    fn test_is_telemetry_not_disabled_when_unset() {
        clear_opt_out_vars();
        assert!(!is_telemetry_disabled());
    }

    #[test]
    #[serial]
    fn test_build_event_returns_none_when_disabled() {
        clear_opt_out_vars();
        unsafe { env::set_var("III_TELEMETRY_ENABLED", "false") };
        let result = build_event("cli_update_started", serde_json::json!({}));
        assert!(result.is_none());
        unsafe { env::remove_var("III_TELEMETRY_ENABLED") };
    }

    #[test]
    #[serial]
    fn test_build_event_returns_some_when_enabled() {
        clear_opt_out_vars();
        let result = build_event(
            "cli_update_started",
            serde_json::json!({"target_binary": "iii"}),
        );
        assert!(result.is_some());
        let event = result.unwrap();
        assert_eq!(event.event_type, "cli_update_started");
        assert_eq!(event.platform, "iii-cli");
        assert_eq!(event.app_version, env!("CARGO_PKG_VERSION"));
        assert!(!event.device_id.is_empty());
        assert!(!event.insert_id.is_empty());
        assert_eq!(event.event_properties["target_binary"], "iii");
        let user_props = event
            .user_properties
            .as_ref()
            .expect("user_properties should be set");
        assert!(user_props.get("cli_version").is_some());
        assert!(user_props.get("environment.os").is_some());
        assert!(user_props.get("install_method").is_some());
    }

    #[test]
    #[serial]
    fn test_build_event_insert_ids_are_unique() {
        clear_opt_out_vars();
        let e1 = build_event("evt", serde_json::json!({})).unwrap();
        let e2 = build_event("evt", serde_json::json!({})).unwrap();
        assert_ne!(e1.insert_id, e2.insert_id);
    }

    #[test]
    fn test_get_or_create_telemetry_id_is_stable() {
        let id1 = get_or_create_telemetry_id();
        let id2 = get_or_create_telemetry_id();
        assert!(!id1.is_empty());
        assert_eq!(id1, id2);
    }

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
}
