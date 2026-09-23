// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! CLI telemetry helpers.
//!
//! All HTTP transport, retry behavior, and PII sanitization live in
//! `iii::workers::telemetry::posthog`. This module is the CLI-side glue:
//! gating (`is_telemetry_disabled`), event-property construction
//! (`build_user_properties`), and the named event helpers
//! (`send_cli_update_*`, `send_project_init_*`, `send_install_lifecycle_event`).

use iii::workers::telemetry::environment;
use iii::workers::telemetry::posthog::{POSTHOG_PROJECT_API_KEY, PostHogClient, ProductEvent};

pub(crate) fn is_telemetry_disabled() -> bool {
    environment::env_opt_out() || environment::is_ci_environment() || environment::is_dev_optout()
}

fn build_user_properties(install_method_override: Option<&str>) -> serde_json::Value {
    let env_info = environment::EnvironmentInfo::collect();
    let install_method = match install_method_override {
        Some(m) => m,
        None => environment::detect_install_method(),
    };
    serde_json::json!({
        "environment.os": env_info.os,
        "environment.arch": env_info.arch,
        "environment.cpu_cores": env_info.cpu_cores,
        "environment.timezone": env_info.timezone,
        "environment.machine_id": env_info.machine_id,
        "iii_execution_context": env_info.iii_execution_context,
        "env": environment::detect_env(),
        "install_method": install_method,
        "cli_version": env!("CARGO_PKG_VERSION"),
    })
}

fn build_event(
    event_type: &str,
    properties: serde_json::Value,
    install_method_override: Option<&str>,
) -> Option<ProductEvent> {
    if is_telemetry_disabled() {
        return None;
    }

    let device_id = environment::get_or_create_device_id();
    Some(ProductEvent {
        device_id,
        user_id: None,
        event_type: event_type.to_string(),
        event_properties: properties,
        user_properties: Some(build_user_properties(install_method_override)),
        platform: "iii".to_string(),
        os_name: std::env::consts::OS.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        time: chrono::Utc::now().timestamp_millis(),
        insert_id: Some(uuid::Uuid::new_v4().to_string()),
        country: None,
        language: None,
        ip: Some("$remote".to_string()),
    })
}

async fn send_direct(event: ProductEvent) {
    if let Some(posthog_client) = build_posthog_client_from_env() {
        let _ = posthog_client.send_event(event).await;
    }
}

fn build_posthog_client_from_env() -> Option<PostHogClient> {
    let key = std::env::var("POSTHOG_PROJECT_API_KEY")
        .or_else(|_| std::env::var("POSTHOG_API_KEY"))
        .ok()
        .or_else(|| Some(POSTHOG_PROJECT_API_KEY.to_string()))
        .filter(|key| !key.trim().is_empty())?;
    let host =
        std::env::var("POSTHOG_HOST").unwrap_or_else(|_| "https://us.i.posthog.com".to_string());
    Some(PostHogClient::new(key, host))
}

/// Sends a batch of events in one request.
///
/// Compose reports on the way out of a command, so it waits for its own
/// sends: one request for a startup's events, never one for each.
async fn send_posthog_batch(events: Vec<ProductEvent>) {
    if let Some(client) = build_posthog_client_from_env() {
        let _ = client.send_batch(events).await;
    }
}

fn send_fire_and_forget(event: ProductEvent) {
    tokio::spawn(async move {
        send_direct(event).await;
    });
}

/// How long an awaited report may hold the command that is sending it.
const REPORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Lets `iii-compose` report through the same CLI path as every other
/// command.
///
/// The crate owns the events and knows nothing about PostHog; this is the one
/// place the two meet. Compose runs in this process, so a compose event
/// carries the same `device_id` as the rest of the CLI, and it needs neither
/// a running engine nor a queue worker to be reported. An embedding host that
/// installs nothing sends nothing.
///
/// The send is awaited, never spawned: a compose command that fails reports
/// on its way out, and a spawned task would be dropped with the runtime.
pub fn install_compose_reporter() {
    iii_compose::telemetry::set_reporter(std::sync::Arc::new(|reports| {
        Box::pin(report_compose_batch(reports))
    }));
}

/// Builds and sends one compose batch.
///
/// Bounded, because compose waits for this before it prints an error and
/// exits. A report that cannot be sent in time is worth less than the seconds
/// it would cost the operator. Building the events reads the device id and
/// hardware id from disk, so that runs on the blocking pool and under the
/// same clock as the send.
async fn report_compose_batch(reports: Vec<iii_compose::telemetry::Report>) {
    let _ = tokio::time::timeout(REPORT_TIMEOUT, async {
        let Ok(events) = tokio::task::spawn_blocking(move || {
            reports
                .into_iter()
                .filter_map(|(event, properties)| build_event(&event, properties, None))
                .collect::<Vec<ProductEvent>>()
        })
        .await
        else {
            return;
        };
        if events.is_empty() {
            return;
        }
        send_posthog_batch(events).await;
    })
    .await;
}

pub async fn send_install_lifecycle_event(event_type: &str, properties: serde_json::Value) {
    let install_method = properties
        .get("install_method")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if let Some(mut event) = build_event(event_type, properties, install_method.as_deref()) {
        event.platform = "install-script".to_string();
        // The install script exits right after this; the retry ladder in
        // send_direct can otherwise hold it for over a minute.
        let _ = tokio::time::timeout(REPORT_TIMEOUT, send_direct(event)).await;
    }
}

/// Counts a CLI invocation for the next engine heartbeat to report in
/// aggregate, replacing the former per-invocation `cli_command_invoked` event.
/// One heartbeat property now covers what used to be one event per command, and
/// the CLI no longer makes a network call on every invocation.
///
/// The counter is in-process, so this call reaches a heartbeat only for `serve`,
/// where the CLI process *is* the engine. Commands that connect to a running
/// engine are counted engine-side from the name they register with; see
/// [`iii::workers::telemetry::collector::track_cli_command`].
pub fn record_cli_usage(command_path: &str) {
    if is_telemetry_disabled() {
        return;
    }
    iii::workers::telemetry::collector::track_cli_command(command_path);
}

pub fn send_cli_update_started(target_binary: &str, from_version: &str) {
    if let Some(event) = build_event(
        "cli_update_started",
        serde_json::json!({
            "target_binary": target_binary,
            "from_version": from_version,
            "install_method": environment::detect_install_method(),
        }),
        None,
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
        None,
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
        None,
    ) {
        send_fire_and_forget(event);
    }
}

pub fn send_project_init_succeeded(with_docker: bool, project_id: &str) {
    if let Some(event) = build_event(
        "project_init_succeeded",
        serde_json::json!({
            "with_docker": with_docker,
            "project_id": project_id,
            "install_method": environment::detect_install_method(),
        }),
        None,
    ) {
        send_fire_and_forget(event);
    }
}

pub fn send_project_init_failed(stage: &str, error: &str) {
    if let Some(event) = build_event(
        "project_init_failed",
        serde_json::json!({
            "stage": stage,
            "error": error,
            "install_method": environment::detect_install_method(),
        }),
        None,
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

    /// A host that accepts every connection and never answers. Without a
    /// bound, one send against it waits the client's 30s timeout three times
    /// over, with backoff between attempts.
    fn black_hole_host() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a local port");
        let port = listener.local_addr().expect("local address").port();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in listener.incoming().flatten() {
                held.push(stream);
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    fn event(event_type: &str) -> ProductEvent {
        ProductEvent {
            device_id: "test-device".to_string(),
            user_id: None,
            event_type: event_type.to_string(),
            event_properties: serde_json::json!({}),
            user_properties: None,
            platform: "iii".to_string(),
            os_name: "test".to_string(),
            app_version: "0.0.0".to_string(),
            time: 0,
            insert_id: None,
            country: None,
            language: None,
            ip: None,
        }
    }

    /// Generous slack over the bound, for a slow machine.
    const RELEASED_WITHIN: std::time::Duration = std::time::Duration::from_secs(3);

    #[tokio::test]
    #[serial]
    async fn a_compose_batch_to_an_unreachable_host_releases_within_the_report_timeout() {
        clear_opt_out_vars();
        unsafe {
            env::set_var("POSTHOG_HOST", black_hole_host());
        }

        let began = std::time::Instant::now();
        report_compose_batch(vec![(
            "compose_up_finished".to_string(),
            serde_json::json!({"outcome": "failed"}),
        )])
        .await;
        let elapsed = began.elapsed();

        unsafe {
            env::remove_var("POSTHOG_HOST");
        }
        assert!(elapsed < RELEASED_WITHIN, "held for {elapsed:?}");
    }

    #[tokio::test]
    #[serial]
    async fn an_install_event_to_an_unreachable_host_releases_within_the_report_timeout() {
        clear_opt_out_vars();
        unsafe {
            env::set_var("POSTHOG_HOST", black_hole_host());
        }

        let began = std::time::Instant::now();
        send_install_lifecycle_event("install_started", serde_json::json!({})).await;
        let elapsed = began.elapsed();

        unsafe {
            env::remove_var("POSTHOG_HOST");
        }
        assert!(elapsed < RELEASED_WITHIN, "held for {elapsed:?}");
    }

    #[tokio::test]
    #[serial]
    async fn a_direct_send_to_an_unreachable_host_is_only_bounded_by_the_caller() {
        // Documents why the exit paths wrap send_direct: on its own it runs
        // the client's retry ladder. Bounded here so the test itself ends.
        unsafe {
            env::set_var("POSTHOG_HOST", black_hole_host());
        }

        let began = std::time::Instant::now();
        let released =
            tokio::time::timeout(REPORT_TIMEOUT, send_direct(event("cli_update_started"))).await;
        let elapsed = began.elapsed();

        unsafe {
            env::remove_var("POSTHOG_HOST");
        }
        assert!(
            released.is_err(),
            "the send answered on its own in {elapsed:?}"
        );
        assert!(elapsed < RELEASED_WITHIN, "held for {elapsed:?}");
    }

    #[test]
    #[serial]
    fn test_build_event_returns_none_when_disabled() {
        clear_opt_out_vars();
        unsafe {
            env::set_var("III_TELEMETRY_ENABLED", "false");
        }
        let result = build_event("cli_update_started", serde_json::json!({}), None);
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
            None,
        );
        assert!(result.is_some());
        let event = result.expect("event should be built");
        assert_eq!(event.event_type, "cli_update_started");
        assert_eq!(event.platform, "iii");
        assert_eq!(event.app_version, env!("CARGO_PKG_VERSION"));
        assert!(!event.device_id.is_empty());
        assert_eq!(event.user_id, None);
        assert!(event.insert_id.as_deref().map(str::is_empty) == Some(false));
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
        let e1 = build_event("evt", serde_json::json!({}), None).expect("event");
        let e2 = build_event("evt", serde_json::json!({}), None).expect("event");
        assert_ne!(e1.insert_id, e2.insert_id);
    }

    #[test]
    #[serial]
    fn test_record_cli_usage_accumulates_for_the_next_heartbeat() {
        use iii::workers::telemetry::collector::take_cli_commands;
        clear_opt_out_vars();
        take_cli_commands();

        record_cli_usage("project init");
        record_cli_usage("trigger");
        record_cli_usage("trigger");
        record_cli_usage("   ");

        let counts = take_cli_commands();
        assert_eq!(counts.get("project init"), Some(&1));
        assert_eq!(counts.get("trigger"), Some(&2));
        assert_eq!(counts.len(), 2, "an empty command path records nothing");
    }

    #[test]
    #[serial]
    fn test_record_cli_usage_records_nothing_when_opted_out() {
        use iii::workers::telemetry::collector::take_cli_commands;
        clear_opt_out_vars();
        take_cli_commands();
        unsafe {
            std::env::set_var("III_TELEMETRY_ENABLED", "false");
        }

        record_cli_usage("trigger");
        assert!(take_cli_commands().is_empty());

        unsafe {
            std::env::remove_var("III_TELEMETRY_ENABLED");
        }
    }

    #[test]
    #[serial]
    fn test_build_posthog_client_from_env_defaults_to_public_project_key() {
        unsafe {
            env::remove_var("POSTHOG_PROJECT_API_KEY");
            env::remove_var("POSTHOG_API_KEY");
            env::remove_var("POSTHOG_HOST");
        }
        assert!(build_posthog_client_from_env().is_some());
    }

    #[test]
    #[serial]
    fn test_build_posthog_client_from_env_accepts_project_key() {
        unsafe {
            env::set_var("POSTHOG_PROJECT_API_KEY", "phc_test");
            env::set_var("POSTHOG_HOST", "https://eu.i.posthog.com");
        }
        assert!(build_posthog_client_from_env().is_some());
        unsafe {
            env::remove_var("POSTHOG_PROJECT_API_KEY");
            env::remove_var("POSTHOG_HOST");
        }
    }
}
