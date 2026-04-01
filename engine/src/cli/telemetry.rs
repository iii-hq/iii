// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::Serialize;

use iii::modules::telemetry::environment;

const AMPLITUDE_ENDPOINT: &str = "https://api2.amplitude.com/2/httpapi";
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

fn is_telemetry_disabled() -> bool {
    if let Ok(val) = std::env::var("III_TELEMETRY_ENABLED")
        && (val == "false" || val == "0")
    {
        return true;
    }

    environment::is_ci_environment() || environment::is_dev_optout()
}

fn build_user_properties() -> serde_json::Value {
    let env_info = environment::EnvironmentInfo::collect();
    serde_json::json!({
        "environment.os": env_info.os,
        "environment.arch": env_info.arch,
        "environment.cpu_cores": env_info.cpu_cores,
        "environment.timezone": env_info.timezone,
        "environment.machine_id": env_info.machine_id,
        "iii_execution_context": env_info.iii_execution_context,
        "env": environment::detect_env(),
        "install_method": environment::detect_install_method(),
        "cli_version": env!("CARGO_PKG_VERSION"),
    })
}

fn build_event(event_type: &str, properties: serde_json::Value) -> Option<AmplitudeEvent> {
    if is_telemetry_disabled() {
        return None;
    }

    let device_id = environment::get_or_create_device_id();
    Some(AmplitudeEvent {
        device_id,
        user_id: None,
        event_type: event_type.to_string(),
        event_properties: properties,
        user_properties: Some(build_user_properties()),
        platform: "iii".to_string(),
        os_name: std::env::consts::OS.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        time: chrono::Utc::now().timestamp_millis(),
        insert_id: uuid::Uuid::new_v4().to_string(),
        ip: Some("$remote".to_string()),
    })
}

async fn send_direct(event: AmplitudeEvent) {
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
}

fn send_fire_and_forget(event: AmplitudeEvent) {
    tokio::spawn(async move {
        send_direct(event).await;
    });
}

pub async fn send_install_lifecycle_event(event_type: &str, properties: serde_json::Value) {
    if let Some(mut event) = build_event(event_type, properties) {
        event.platform = "install-script".to_string();
        send_direct(event).await;
    }
}

pub fn send_cli_update_started(target_binary: &str, from_version: &str) {
    if let Some(event) = build_event(
        "cli_update_started",
        serde_json::json!({
            "target_binary": target_binary,
            "from_version": from_version,
            "install_method": environment::detect_install_method(),
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
            "install_method": environment::detect_install_method(),
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
            "install_method": environment::detect_install_method(),
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
        unsafe {
            env::set_var("III_TELEMETRY_ENABLED", "false");
        }
        assert!(is_telemetry_disabled());
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
        }
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
        unsafe {
            env::set_var("III_TELEMETRY_ENABLED", "false");
        }
        let result = build_event("cli_update_started", serde_json::json!({}));
        assert!(result.is_none());
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
        }
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
        let event = result.expect("event should be built");
        assert_eq!(event.event_type, "cli_update_started");
        assert_eq!(event.platform, "iii");
        assert_eq!(event.app_version, env!("CARGO_PKG_VERSION"));
        assert!(!event.device_id.is_empty());
        assert_eq!(event.user_id, None);
        assert!(!event.insert_id.is_empty());
        assert_eq!(event.event_properties["target_binary"], "iii");
        let user_props = event
            .user_properties
            .as_ref()
            .expect("user_properties should be set");
        assert!(user_props.get("cli_version").is_some());
        assert!(user_props.get("environment.os").is_some());
        assert!(user_props.get("iii_execution_context").is_some());
        assert!(user_props.get("install_method").is_some());
    }

    #[test]
    #[serial]
    fn test_build_event_insert_ids_are_unique() {
        clear_opt_out_vars();
        let e1 = build_event("evt", serde_json::json!({})).expect("event");
        let e2 = build_event("evt", serde_json::json!({})).expect("event");
        assert_ne!(e1.insert_id, e2.insert_id);
    }
}
