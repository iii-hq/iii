use serde::Serialize;

const AMPLITUDE_ENDPOINT: &str = "https://api2.amplitude.com/2/httpapi";

const COMPILE_TIME_API_KEY: Option<&str> = option_env!("III_CLI_AMPLITUDE_API_KEY");

#[derive(Serialize)]
struct AmplitudeEvent {
    device_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_id: Option<String>,
    event_type: String,
    event_properties: serde_json::Value,
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

fn get_or_create_telemetry_id() -> String {
    let path = dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".iii")
        .join("telemetry_id");

    if let Ok(id) = std::fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, &id).is_ok() {
        std::fs::rename(&tmp, &path).ok();
    }
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

fn resolve_api_key() -> Option<String> {
    if let Ok(key) = std::env::var("III_TELEMETRY_API_KEY") {
        if !key.is_empty() {
            return Some(key);
        }
    }
    COMPILE_TIME_API_KEY
        .filter(|k| !k.is_empty())
        .map(|k| k.to_string())
}

fn build_event(
    event_type: &str,
    properties: serde_json::Value,
) -> Option<(String, AmplitudeEvent)> {
    if is_telemetry_disabled() {
        return None;
    }
    let api_key = resolve_api_key()?;

    let telemetry_id = get_or_create_telemetry_id();
    let event = AmplitudeEvent {
        device_id: telemetry_id.clone(),
        // user_id: currently telemetry_id, will become iii cloud user ID when accounts ship
        user_id: Some(telemetry_id),
        event_type: event_type.to_string(),
        event_properties: properties,
        platform: "iii-cli".to_string(),
        os_name: std::env::consts::OS.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        time: chrono::Utc::now().timestamp_millis(),
        insert_id: uuid::Uuid::new_v4().to_string(),
        ip: Some("$remote".to_string()),
    };
    Some((api_key, event))
}

fn send_fire_and_forget(api_key: String, event: AmplitudeEvent) {
    tokio::spawn(async move {
        let payload = AmplitudePayload {
            api_key: &api_key,
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
    if let Some((key, event)) = build_event(
        "cli_update_started",
        serde_json::json!({
            "target_binary": target_binary,
            "from_version": from_version,
        }),
    ) {
        send_fire_and_forget(key, event);
    }
}

pub fn send_cli_update_succeeded(target_binary: &str, from_version: &str, to_version: &str) {
    if let Some((key, event)) = build_event(
        "cli_update_succeeded",
        serde_json::json!({
            "target_binary": target_binary,
            "from_version": from_version,
            "to_version": to_version,
        }),
    ) {
        send_fire_and_forget(key, event);
    }
}

pub fn send_cli_update_failed(target_binary: &str, from_version: &str, error: &str) {
    if let Some((key, event)) = build_event(
        "cli_update_failed",
        serde_json::json!({
            "target_binary": target_binary,
            "from_version": from_version,
            "error": error,
        }),
    ) {
        send_fire_and_forget(key, event);
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
    fn test_build_event_returns_none_without_api_key() {
        clear_opt_out_vars();
        unsafe { env::remove_var("III_TELEMETRY_API_KEY") };
        if COMPILE_TIME_API_KEY.is_none() || COMPILE_TIME_API_KEY == Some("") {
            let result = build_event("cli_update_started", serde_json::json!({}));
            assert!(result.is_none());
        }
    }

    #[test]
    #[serial]
    fn test_build_event_returns_some_with_runtime_api_key() {
        clear_opt_out_vars();
        unsafe { env::set_var("III_TELEMETRY_API_KEY", "test-key-123") };
        let result = build_event(
            "cli_update_started",
            serde_json::json!({"target_binary": "iii"}),
        );
        assert!(result.is_some());
        let (key, event) = result.unwrap();
        assert_eq!(key, "test-key-123");
        assert_eq!(event.event_type, "cli_update_started");
        assert_eq!(event.platform, "iii-cli");
        assert_eq!(event.app_version, env!("CARGO_PKG_VERSION"));
        assert!(!event.device_id.is_empty());
        assert!(!event.insert_id.is_empty());
        assert_eq!(event.event_properties["target_binary"], "iii");
        unsafe { env::remove_var("III_TELEMETRY_API_KEY") };
    }

    #[test]
    #[serial]
    fn test_build_event_insert_ids_are_unique() {
        clear_opt_out_vars();
        unsafe { env::set_var("III_TELEMETRY_API_KEY", "test-key") };
        let r1 = build_event("evt", serde_json::json!({}));
        let r2 = build_event("evt", serde_json::json!({}));
        assert!(r1.is_some() && r2.is_some());
        let (_, e1) = r1.unwrap();
        let (_, e2) = r2.unwrap();
        assert_ne!(e1.insert_id, e2.insert_id);
        unsafe { env::remove_var("III_TELEMETRY_API_KEY") };
    }

    #[test]
    fn test_get_or_create_telemetry_id_is_stable() {
        let id1 = get_or_create_telemetry_id();
        let id2 = get_or_create_telemetry_id();
        assert!(!id1.is_empty());
        assert_eq!(id1, id2);
    }
}
