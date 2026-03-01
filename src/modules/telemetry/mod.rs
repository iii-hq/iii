// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod amplitude;
pub mod collector;
pub mod environment;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::engine::Engine;
use crate::modules::module::Module;
use crate::workers::WorkerTelemetryMeta;

use self::amplitude::{AmplitudeClient, AmplitudeEvent};
use self::collector::collector;
use self::environment::EnvironmentInfo;

#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub sdk_api_key: Option<String>,
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,
}

fn default_enabled() -> bool {
    true
}

fn default_heartbeat_interval() -> u64 {
    6 * 60 * 60
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            api_key: String::new(),
            sdk_api_key: None,
            heartbeat_interval_secs: 6 * 60 * 60,
        }
    }
}

fn resolve_project_id() -> Option<String> {
    std::env::var("III_PROJECT_ID")
        .ok()
        .filter(|s| !s.is_empty())
}

fn get_or_create_install_id() -> String {
    let base_dir = dirs::home_dir().unwrap_or_else(|| {
        tracing::warn!(
            "Failed to resolve home directory, falling back to temp dir for telemetry_id"
        );
        std::env::temp_dir()
    });
    let path = base_dir.join(".iii").join("telemetry_id");

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

    let tmp_path = path.with_extension("tmp");
    if std::fs::write(&tmp_path, &id).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp_path, perms).ok();
        }
        std::fs::rename(&tmp_path, &path).ok();
    }

    id
}

enum DisableReason {
    UserOptOut,
    CiDetected,
    DevOptOut,
    Config,
}

fn check_disabled(config: &TelemetryConfig) -> Option<DisableReason> {
    if !config.enabled {
        return Some(DisableReason::Config);
    }

    if let Ok(env_val) = std::env::var("III_TELEMETRY_ENABLED")
        && (env_val == "false" || env_val == "0")
    {
        return Some(DisableReason::UserOptOut);
    }

    if environment::is_ci_environment() {
        return Some(DisableReason::CiDetected);
    }

    if environment::is_dev_optout() {
        return Some(DisableReason::DevOptOut);
    }

    None
}

fn collect_functions_and_triggers(engine: &Engine) -> serde_json::Value {
    let functions: Vec<String> = engine
        .functions
        .iter()
        .map(|entry| entry.key().clone())
        .filter(|id| !id.starts_with("engine::"))
        .collect();

    let function_count = functions.len();

    let mut triggers_list: Vec<serde_json::Value> = Vec::new();
    let mut trigger_types_used: HashSet<String> = HashSet::new();

    for entry in engine.trigger_registry.triggers.iter() {
        let trigger = entry.value();
        trigger_types_used.insert(trigger.trigger_type.clone());
        triggers_list.push(serde_json::json!({
            "id": trigger.id,
            "type": trigger.trigger_type,
            "function_id": trigger.function_id,
        }));
    }

    let trigger_count = triggers_list.len();

    let services: Vec<String> = engine
        .service_registry
        .services
        .iter()
        .map(|entry| entry.key().clone())
        .collect();

    serde_json::json!({
        "functions": functions,
        "function_count": function_count,
        "triggers": triggers_list,
        "trigger_count": trigger_count,
        "trigger_types_used": trigger_types_used.into_iter().collect::<Vec<_>>(),
        "services": services,
    })
}

fn collect_worker_data(engine: &Engine) -> (HashMap<String, u64>, Option<WorkerTelemetryMeta>) {
    let mut runtime_counts: HashMap<String, u64> = HashMap::new();
    let mut best_telemetry: Option<(uuid::Uuid, WorkerTelemetryMeta)> = None;

    for entry in engine.worker_registry.workers.iter() {
        let worker = entry.value();
        let runtime = worker
            .runtime
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        *runtime_counts.entry(runtime).or_insert(0) += 1;

        if let Some(telemetry) = worker.telemetry.as_ref()
            && (telemetry.language.is_some()
                || telemetry.project_name.is_some()
                || telemetry.framework.is_some())
            && best_telemetry
                .as_ref()
                .is_none_or(|(id, _)| worker.id < *id)
        {
            best_telemetry = Some((worker.id, telemetry.clone()));
        }
    }

    (runtime_counts, best_telemetry.map(|(_, t)| t))
}

fn build_client_context(
    runtime_counts: &HashMap<String, u64>,
    sdk_telemetry: Option<&WorkerTelemetryMeta>,
) -> serde_json::Value {
    let client_type = sdk_telemetry
        .and_then(|t| t.framework.clone())
        .unwrap_or_else(|| environment::detect_client_type().to_string());

    let sdk_detected: Vec<String> = runtime_counts
        .keys()
        .map(|r| match r.as_str() {
            "node" => "iii-js".to_string(),
            "python" => "iii-py".to_string(),
            other => other.to_string(),
        })
        .collect();

    let worker_runtimes: Vec<&String> = runtime_counts.keys().collect();

    serde_json::json!({
        "type": client_type,
        "sdk_detected": sdk_detected,
        "worker_runtimes": worker_runtimes,
    })
}

/// Cloneable context for building telemetry events inside spawned tasks.
#[derive(Clone)]
struct TelemetryContext {
    install_id: String,
    env_info: EnvironmentInfo,
}

impl TelemetryContext {
    fn build_user_properties(
        &self,
        sdk_telemetry: Option<&WorkerTelemetryMeta>,
    ) -> serde_json::Value {
        let mut props = serde_json::json!({
            "environment": self.env_info.to_json(),
            "device_type": environment::detect_device_type(),
        });
        if let Some(project_id) = resolve_project_id() {
            props["project_id"] = serde_json::Value::String(project_id);
        }
        if let Some(telemetry) = sdk_telemetry {
            if let Some(project_name) = &telemetry.project_name {
                props["project_name"] = serde_json::Value::String(project_name.clone());
            }
            if let Some(framework) = &telemetry.framework {
                props["framework"] = serde_json::Value::String(framework.clone());
            }
        }
        props
    }

    fn build_event(
        &self,
        event_type: &str,
        properties: serde_json::Value,
        sdk_telemetry: Option<&WorkerTelemetryMeta>,
    ) -> AmplitudeEvent {
        let language = sdk_telemetry
            .and_then(|t| t.language.clone())
            .or_else(environment::detect_language);
        AmplitudeEvent {
            device_id: self.install_id.clone(),
            user_id: Some(self.install_id.clone()),
            event_type: event_type.to_string(),
            event_properties: properties,
            user_properties: Some(self.build_user_properties(sdk_telemetry)),
            platform: "III Engine".to_string(),
            os_name: std::env::consts::OS.to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            time: chrono::Utc::now().timestamp_millis(),
            insert_id: Some(uuid::Uuid::new_v4().to_string()),
            country: None,
            language,
            ip: Some("$remote".to_string()),
        }
    }
}

pub struct TelemetryModule {
    engine: Arc<Engine>,
    config: TelemetryConfig,
    client: Arc<AmplitudeClient>,
    sdk_client: Option<Arc<AmplitudeClient>>,
    ctx: TelemetryContext,
    start_time: Instant,
}

impl TelemetryModule {
    fn active_client(&self) -> &Arc<AmplitudeClient> {
        self.sdk_client.as_ref().unwrap_or(&self.client)
    }
}

struct DisabledTelemetryModule;

#[async_trait]
impl Module for DisabledTelemetryModule {
    fn name(&self) -> &'static str {
        "Telemetry"
    }

    async fn create(
        _engine: Arc<Engine>,
        _config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Module>> {
        Ok(Box::new(DisabledTelemetryModule))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        _shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Module for TelemetryModule {
    fn name(&self) -> &'static str {
        "Telemetry"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        let mut telemetry_config: TelemetryConfig = match config {
            Some(cfg) => serde_json::from_value(cfg)?,
            None => TelemetryConfig::default(),
        };

        if let Some(reason) = check_disabled(&telemetry_config) {
            match reason {
                DisableReason::Config => {
                    tracing::info!("Anonymous telemetry disabled (config).");
                }
                DisableReason::UserOptOut => {
                    tracing::info!("Anonymous telemetry disabled (user opt-out).");
                }
                DisableReason::CiDetected => {
                    tracing::info!("Anonymous telemetry disabled (CI detected).");
                }
                DisableReason::DevOptOut => {
                    tracing::info!("Anonymous telemetry disabled (dev opt-out).");
                }
            }
            return Ok(Box::new(DisabledTelemetryModule));
        }

        if telemetry_config.api_key.is_empty() {
            telemetry_config.api_key = "e8fb1f8d290a72dbb2d9b264926be4bf".to_string();
        }

        let install_id = get_or_create_install_id();
        let env_info = EnvironmentInfo::collect();

        tracing::info!("Anonymous telemetry enabled. Set III_TELEMETRY_ENABLED=false to disable.");

        let client = Arc::new(AmplitudeClient::new(telemetry_config.api_key.clone()));

        let sdk_client = telemetry_config
            .sdk_api_key
            .as_deref()
            .filter(|k| !k.is_empty())
            .map(|key| Arc::new(AmplitudeClient::new(key.to_owned())));

        let ctx = TelemetryContext {
            install_id: install_id.clone(),
            env_info,
        };

        Ok(Box::new(TelemetryModule {
            engine,
            config: telemetry_config,
            client,
            sdk_client,
            ctx,
            start_time: Instant::now(),
        }))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let interval_secs = self.config.heartbeat_interval_secs;
        let client = Arc::clone(self.active_client());
        let engine = Arc::clone(&self.engine);
        let ctx = self.ctx.clone();
        let start_time = self.start_time;
        let mut shutdown_rx = shutdown;

        let engine_for_started = Arc::clone(&self.engine);
        let client_for_started = Arc::clone(self.active_client());
        let ctx_for_started = self.ctx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            let active_modules: Vec<String> = engine_for_started
                .functions
                .iter()
                .filter_map(|entry| entry.key().split('.').next().map(String::from))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            let registry_data = collect_functions_and_triggers(&engine_for_started);
            let (runtime_counts, sdk_telemetry) = collect_worker_data(&engine_for_started);
            let client_context = build_client_context(&runtime_counts, sdk_telemetry.as_ref());

            let event = ctx_for_started.build_event(
                "engine_started",
                serde_json::json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "os": std::env::consts::OS,
                    "arch": std::env::consts::ARCH,
                    "active_modules": active_modules,
                    "registry": registry_data,
                    "client_context": client_context,
                }),
                sdk_telemetry.as_ref(),
            );

            let _ = client_for_started.send_event(event).await;
        });

        let engine_for_registry = Arc::clone(&self.engine);
        let client_for_registry = Arc::clone(self.active_client());
        let ctx_for_registry = self.ctx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;

            let active_modules: Vec<String> = engine_for_registry
                .functions
                .iter()
                .filter_map(|entry| entry.key().split('.').next().map(String::from))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            let registry_data = collect_functions_and_triggers(&engine_for_registry);
            let (runtime_counts, sdk_telemetry) = collect_worker_data(&engine_for_registry);
            let client_context = build_client_context(&runtime_counts, sdk_telemetry.as_ref());

            let event = ctx_for_registry.build_event(
                "engine_registry_snapshot",
                serde_json::json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "os": std::env::consts::OS,
                    "arch": std::env::consts::ARCH,
                    "active_modules": active_modules,
                    "registry": registry_data,
                    "client_context": client_context,
                }),
                sdk_telemetry.as_ref(),
            );

            let _ = client_for_registry.send_event(event).await;
        });

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));

            interval.tick().await;

            let mut prev_invocations: u64 = 0;
            let mut prev_queue_emits: u64 = 0;
            let mut prev_api_requests: u64 = 0;

            loop {
                tokio::select! {
                    result = shutdown_rx.changed() => {
                        if result.is_err() {
                            break;
                        }
                        if *shutdown_rx.borrow() {
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        let telemetry_snapshot = collector().snapshot();

                        let accumulator = crate::modules::observability::metrics::get_metrics_accumulator();
                        let invocations_total = accumulator.invocations_total.load(std::sync::atomic::Ordering::Relaxed);
                        let invocations_success = accumulator.invocations_success.load(std::sync::atomic::Ordering::Relaxed);
                        let invocations_error = accumulator.invocations_error.load(std::sync::atomic::Ordering::Relaxed);
                        let workers_spawns = accumulator.workers_spawns.load(std::sync::atomic::Ordering::Relaxed);
                        let workers_deaths = accumulator.workers_deaths.load(std::sync::atomic::Ordering::Relaxed);

                        let queue_emits_now = collector().queue_emits.load(std::sync::atomic::Ordering::Relaxed);
                        let api_requests_now = collector().api_requests.load(std::sync::atomic::Ordering::Relaxed);

                        let invocation_delta = invocations_total.saturating_sub(prev_invocations);
                        let queue_emit_delta = queue_emits_now.saturating_sub(prev_queue_emits);
                        let api_request_delta = api_requests_now.saturating_sub(prev_api_requests);

                        let rate_invocations = if interval_secs > 0 { invocation_delta as f64 / interval_secs as f64 } else { 0.0 };
                        let rate_queue_emits = if interval_secs > 0 { queue_emit_delta as f64 / interval_secs as f64 } else { 0.0 };
                        let rate_api_requests = if interval_secs > 0 { api_request_delta as f64 / interval_secs as f64 } else { 0.0 };

                        prev_invocations = invocations_total;
                        prev_queue_emits = queue_emits_now;
                        prev_api_requests = api_requests_now;

                        let (runtime_counts, sdk_telemetry) = collect_worker_data(&engine);
                        let client_context = build_client_context(&runtime_counts, sdk_telemetry.as_ref());
                        let registry_data = collect_functions_and_triggers(&engine);
                        let uptime_secs = start_time.elapsed().as_secs();

                        let properties = serde_json::json!({
                            "uptime_secs": uptime_secs,
                            "invocations": {
                                "total": invocations_total,
                                "success": invocations_success,
                                "error": invocations_error,
                            },
                            "rates": {
                                "invocations_per_sec": rate_invocations,
                                "queue_emits_per_sec": rate_queue_emits,
                                "api_requests_per_sec": rate_api_requests,
                            },
                            "workers": {
                                "spawns": workers_spawns,
                                "deaths": workers_deaths,
                                "active": workers_spawns.saturating_sub(workers_deaths),
                                "runtimes": runtime_counts,
                            },
                            "modules": telemetry_snapshot,
                            "registry": registry_data,
                            "client_context": client_context,
                        });

                        let event = ctx.build_event(
                            "engine_heartbeat",
                            properties,
                            sdk_telemetry.as_ref(),
                        );

                        let _ = client.send_event(event).await;
                    }
                }
            }
        });

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        let uptime_secs = self.start_time.elapsed().as_secs();
        let telemetry_snapshot = collector().snapshot();
        let registry_data = collect_functions_and_triggers(&self.engine);
        let (runtime_counts, sdk_telemetry) = collect_worker_data(&self.engine);
        let client_context = build_client_context(&runtime_counts, sdk_telemetry.as_ref());

        let event = self.ctx.build_event(
            "engine_stopped",
            serde_json::json!({
                "uptime_secs": uptime_secs,
                "counters": telemetry_snapshot,
                "registry": registry_data,
                "client_context": client_context,
            }),
            sdk_telemetry.as_ref(),
        );

        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.active_client().send_event(event),
        )
        .await;

        Ok(())
    }
}

crate::register_module!(
    "modules::telemetry::TelemetryModule",
    TelemetryModule,
    mandatory
);

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::{env, future::Future, pin::Pin, sync::atomic::Ordering, time::Duration};
    use tokio::sync::mpsc;

    use crate::{
        function::{Function, FunctionResult, HandlerFn},
        modules::{
            observability::metrics::get_metrics_accumulator, telemetry::collector::collector,
        },
        services::Service,
        trigger::{Trigger, TriggerRegistrator, TriggerType},
        workers::Worker,
    };

    // Helper to clear all CI-related env vars so tests are isolated.
    fn clear_ci_env_vars() {
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
    }

    fn reset_telemetry_globals() {
        let acc = get_metrics_accumulator();
        acc.invocations_total.store(0, Ordering::Relaxed);
        acc.invocations_success.store(0, Ordering::Relaxed);
        acc.invocations_error.store(0, Ordering::Relaxed);
        acc.invocations_deferred.store(0, Ordering::Relaxed);
        acc.workers_spawns.store(0, Ordering::Relaxed);
        acc.workers_deaths.store(0, Ordering::Relaxed);
        acc.invocations_by_function.clear();

        let telemetry = collector();
        telemetry.cron_executions.store(0, Ordering::Relaxed);
        telemetry.queue_emits.store(0, Ordering::Relaxed);
        telemetry.queue_consumes.store(0, Ordering::Relaxed);
        telemetry.state_sets.store(0, Ordering::Relaxed);
        telemetry.state_gets.store(0, Ordering::Relaxed);
        telemetry.state_deletes.store(0, Ordering::Relaxed);
        telemetry.state_updates.store(0, Ordering::Relaxed);
        telemetry.stream_sets.store(0, Ordering::Relaxed);
        telemetry.stream_gets.store(0, Ordering::Relaxed);
        telemetry.stream_deletes.store(0, Ordering::Relaxed);
        telemetry.stream_lists.store(0, Ordering::Relaxed);
        telemetry.stream_updates.store(0, Ordering::Relaxed);
        telemetry.pubsub_publishes.store(0, Ordering::Relaxed);
        telemetry.pubsub_subscribes.store(0, Ordering::Relaxed);
        telemetry.kv_sets.store(0, Ordering::Relaxed);
        telemetry.kv_gets.store(0, Ordering::Relaxed);
        telemetry.kv_deletes.store(0, Ordering::Relaxed);
        telemetry.api_requests.store(0, Ordering::Relaxed);
        telemetry.function_registrations.store(0, Ordering::Relaxed);
        telemetry.trigger_registrations.store(0, Ordering::Relaxed);
        telemetry.peak_active_workers.store(0, Ordering::Relaxed);
    }

    fn register_test_function(engine: &Arc<Engine>, function_id: &str) {
        let handler: Arc<HandlerFn> =
            Arc::new(|_invocation_id, _input| Box::pin(async { FunctionResult::NoResult }));
        engine.functions.register_function(
            function_id.to_string(),
            Function {
                handler,
                _function_id: function_id.to_string(),
                _description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
        );
        engine
            .service_registry
            .insert_service(Service::new("svc".to_string(), "svc-1".to_string()));
        engine
            .service_registry
            .insert_function_to_service(&"svc".to_string(), "worker");
    }

    struct NoopRegistrator;

    impl TriggerRegistrator for NoopRegistrator {
        fn register_trigger(
            &self,
            _trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn unregister_trigger(
            &self,
            _trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }
    }

    fn build_manual_module(
        engine: Arc<Engine>,
        sdk_client: bool,
        heartbeat_interval_secs: u64,
    ) -> TelemetryModule {
        TelemetryModule {
            engine,
            config: TelemetryConfig {
                enabled: true,
                api_key: String::new(),
                sdk_api_key: sdk_client.then(String::new),
                heartbeat_interval_secs,
            },
            client: Arc::new(AmplitudeClient::new(String::new())),
            sdk_client: sdk_client.then(|| Arc::new(AmplitudeClient::new(String::new()))),
            ctx: TelemetryContext {
                install_id: "test-install-id".to_string(),
                env_info: EnvironmentInfo {
                    machine_id: "machine-1".to_string(),
                    is_container: false,
                    timezone: "UTC".to_string(),
                    cpu_cores: 4,
                    os: "linux".to_string(),
                    arch: "x86_64".to_string(),
                },
            },
            start_time: Instant::now(),
        }
    }

    // =========================================================================
    // TelemetryConfig defaults
    // =========================================================================

    #[test]
    fn test_default_enabled_returns_true() {
        assert!(default_enabled());
    }

    #[test]
    fn test_default_heartbeat_interval_is_six_hours() {
        assert_eq!(default_heartbeat_interval(), 6 * 60 * 60);
    }

    #[test]
    fn test_telemetry_config_default() {
        let config = TelemetryConfig::default();
        assert!(config.enabled);
        assert!(config.api_key.is_empty());
        assert!(config.sdk_api_key.is_none());
        assert_eq!(config.heartbeat_interval_secs, 6 * 60 * 60);
    }

    #[test]
    fn test_telemetry_config_deserialize_defaults() {
        let json = serde_json::json!({});
        let config: TelemetryConfig = serde_json::from_value(json).unwrap();
        assert!(config.enabled);
        assert!(config.api_key.is_empty());
        assert!(config.sdk_api_key.is_none());
        assert_eq!(config.heartbeat_interval_secs, 6 * 60 * 60);
    }

    #[test]
    fn test_telemetry_config_deserialize_overrides() {
        let json = serde_json::json!({
            "enabled": false,
            "api_key": "my-key",
            "sdk_api_key": "sdk-key",
            "heartbeat_interval_secs": 3600
        });
        let config: TelemetryConfig = serde_json::from_value(json).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.api_key, "my-key");
        assert_eq!(config.sdk_api_key, Some("sdk-key".to_string()));
        assert_eq!(config.heartbeat_interval_secs, 3600);
    }

    #[test]
    fn test_telemetry_config_debug_and_clone() {
        let config = TelemetryConfig::default();
        let debug = format!("{:?}", config);
        assert!(debug.contains("TelemetryConfig"));

        let cloned = config.clone();
        assert_eq!(cloned.enabled, config.enabled);
        assert_eq!(cloned.api_key, config.api_key);
    }

    // =========================================================================
    // resolve_project_id
    // =========================================================================

    #[test]
    #[serial]
    fn test_resolve_project_id_when_set() {
        unsafe {
            env::set_var("III_PROJECT_ID", "proj-123");
        }
        let result = resolve_project_id();
        assert_eq!(result, Some("proj-123".to_string()));
        unsafe {
            env::remove_var("III_PROJECT_ID");
        }
    }

    #[test]
    #[serial]
    fn test_resolve_project_id_when_unset() {
        unsafe {
            env::remove_var("III_PROJECT_ID");
        }
        let result = resolve_project_id();
        assert_eq!(result, None);
    }

    #[test]
    #[serial]
    fn test_resolve_project_id_when_empty() {
        unsafe {
            env::set_var("III_PROJECT_ID", "");
        }
        let result = resolve_project_id();
        assert_eq!(result, None);
        unsafe {
            env::remove_var("III_PROJECT_ID");
        }
    }

    // =========================================================================
    // check_disabled
    // =========================================================================

    #[test]
    #[serial]
    fn test_check_disabled_returns_config_when_disabled() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let config = TelemetryConfig {
            enabled: false,
            ..TelemetryConfig::default()
        };
        let reason = check_disabled(&config);
        assert!(reason.is_some());
        assert!(matches!(reason.unwrap(), DisableReason::Config));
    }

    #[test]
    #[serial]
    fn test_check_disabled_returns_user_optout_for_env_false() {
        clear_ci_env_vars();
        unsafe {
            env::set_var("III_TELEMETRY_ENABLED", "false");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let config = TelemetryConfig::default();
        let reason = check_disabled(&config);
        assert!(reason.is_some());
        assert!(matches!(reason.unwrap(), DisableReason::UserOptOut));

        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
        }
    }

    #[test]
    #[serial]
    fn test_check_disabled_returns_user_optout_for_env_zero() {
        clear_ci_env_vars();
        unsafe {
            env::set_var("III_TELEMETRY_ENABLED", "0");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let config = TelemetryConfig::default();
        let reason = check_disabled(&config);
        assert!(reason.is_some());
        assert!(matches!(reason.unwrap(), DisableReason::UserOptOut));

        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
        }
    }

    #[test]
    #[serial]
    fn test_check_disabled_does_not_optout_for_env_true() {
        clear_ci_env_vars();
        unsafe {
            env::set_var("III_TELEMETRY_ENABLED", "true");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let config = TelemetryConfig::default();
        let reason = check_disabled(&config);
        // Should not be UserOptOut -- could be None or CiDetected depending on env
        if let Some(r) = &reason {
            assert!(!matches!(r, DisableReason::UserOptOut));
        }

        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
        }
    }

    #[test]
    #[serial]
    fn test_check_disabled_returns_ci_detected_when_ci_set() {
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }
        clear_ci_env_vars();
        unsafe {
            env::set_var("CI", "true");
        }

        let config = TelemetryConfig::default();
        let reason = check_disabled(&config);
        assert!(reason.is_some());
        assert!(matches!(reason.unwrap(), DisableReason::CiDetected));

        unsafe {
            env::remove_var("CI");
        }
    }

    #[test]
    #[serial]
    fn test_check_disabled_returns_dev_optout_when_dev_env_set() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::set_var("III_TELEMETRY_DEV", "true");
        }

        let config = TelemetryConfig::default();
        let reason = check_disabled(&config);
        assert!(reason.is_some());
        assert!(matches!(reason.unwrap(), DisableReason::DevOptOut));

        unsafe {
            env::remove_var("III_TELEMETRY_DEV");
        }
    }

    #[test]
    #[serial]
    fn test_check_disabled_returns_none_when_all_enabled() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let config = TelemetryConfig::default();
        let reason = check_disabled(&config);
        assert!(
            reason.is_none(),
            "should return None when telemetry is fully enabled"
        );
    }

    #[test]
    #[serial]
    fn test_check_disabled_config_takes_priority_over_env() {
        // Even if env says enabled, config.enabled=false should win (checked first)
        clear_ci_env_vars();
        unsafe {
            env::set_var("III_TELEMETRY_ENABLED", "true");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let config = TelemetryConfig {
            enabled: false,
            ..TelemetryConfig::default()
        };
        let reason = check_disabled(&config);
        assert!(matches!(reason.unwrap(), DisableReason::Config));

        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
        }
    }

    // =========================================================================
    // build_client_context
    // =========================================================================

    #[test]
    fn test_build_client_context_no_sdk_telemetry() {
        let mut runtime_counts = HashMap::new();
        runtime_counts.insert("node".to_string(), 3);
        runtime_counts.insert("python".to_string(), 1);

        let ctx = build_client_context(&runtime_counts, None);

        assert_eq!(ctx["type"], "iii_direct");

        let sdk_detected = ctx["sdk_detected"].as_array().unwrap();
        let sdk_names: Vec<&str> = sdk_detected.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(
            sdk_names.contains(&"iii-js"),
            "node runtime should map to iii-js"
        );
        assert!(
            sdk_names.contains(&"iii-py"),
            "python runtime should map to iii-py"
        );
    }

    #[test]
    fn test_build_client_context_with_sdk_telemetry_framework() {
        let runtime_counts = HashMap::new();
        let telemetry = WorkerTelemetryMeta {
            language: Some("typescript".to_string()),
            project_name: Some("my-project".to_string()),
            framework: Some("next".to_string()),
        };

        let ctx = build_client_context(&runtime_counts, Some(&telemetry));
        assert_eq!(
            ctx["type"], "next",
            "should use framework from SDK telemetry"
        );
    }

    #[test]
    fn test_build_client_context_with_sdk_telemetry_no_framework() {
        let runtime_counts = HashMap::new();
        let telemetry = WorkerTelemetryMeta {
            language: Some("typescript".to_string()),
            project_name: Some("my-project".to_string()),
            framework: None,
        };

        let ctx = build_client_context(&runtime_counts, Some(&telemetry));
        assert_eq!(
            ctx["type"], "iii_direct",
            "should fall back to detect_client_type"
        );
    }

    #[test]
    fn test_build_client_context_empty_runtime_counts() {
        let runtime_counts: HashMap<String, u64> = HashMap::new();
        let ctx = build_client_context(&runtime_counts, None);

        let sdk_detected = ctx["sdk_detected"].as_array().unwrap();
        assert!(
            sdk_detected.is_empty(),
            "no runtimes should produce empty sdk_detected"
        );

        let worker_runtimes = ctx["worker_runtimes"].as_array().unwrap();
        assert!(worker_runtimes.is_empty());
    }

    #[test]
    fn test_build_client_context_unknown_runtime() {
        let mut runtime_counts = HashMap::new();
        runtime_counts.insert("ruby".to_string(), 2);

        let ctx = build_client_context(&runtime_counts, None);
        let sdk_detected = ctx["sdk_detected"].as_array().unwrap();
        let sdk_names: Vec<&str> = sdk_detected.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(
            sdk_names.contains(&"ruby"),
            "unknown runtime should pass through as-is"
        );
    }

    #[test]
    fn test_build_client_context_has_required_keys() {
        let runtime_counts = HashMap::new();
        let ctx = build_client_context(&runtime_counts, None);

        assert!(ctx.get("type").is_some());
        assert!(ctx.get("sdk_detected").is_some());
        assert!(ctx.get("worker_runtimes").is_some());
    }

    // =========================================================================
    // DisableReason enum
    // =========================================================================

    #[test]
    fn test_disable_reason_variants_exist() {
        // Just ensure all variants can be constructed
        let _config = DisableReason::Config;
        let _user = DisableReason::UserOptOut;
        let _ci = DisableReason::CiDetected;
        let _dev = DisableReason::DevOptOut;
    }

    // =========================================================================
    // AmplitudeEvent serialization (via TelemetryContext::build_event)
    // =========================================================================

    #[test]
    fn test_build_event_basic_fields() {
        let ctx = TelemetryContext {
            install_id: "test-install-id".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "abc123".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let event = ctx.build_event("test_event", serde_json::json!({"key": "value"}), None);

        assert_eq!(event.device_id, "test-install-id");
        assert_eq!(event.user_id, Some("test-install-id".to_string()));
        assert_eq!(event.event_type, "test_event");
        assert_eq!(event.event_properties["key"], "value");
        assert_eq!(event.platform, "III Engine");
        assert_eq!(event.os_name, std::env::consts::OS);
        assert!(event.insert_id.is_some());
        assert_eq!(event.ip, Some("$remote".to_string()));
        assert!(event.time > 0);
    }

    #[test]
    fn test_build_event_with_sdk_telemetry_language() {
        let ctx = TelemetryContext {
            install_id: "id-1".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 2,
                os: "macos".to_string(),
                arch: "aarch64".to_string(),
            },
        };

        let telemetry = WorkerTelemetryMeta {
            language: Some("typescript".to_string()),
            project_name: None,
            framework: None,
        };

        let event = ctx.build_event("evt", serde_json::json!({}), Some(&telemetry));
        assert_eq!(event.language, Some("typescript".to_string()));
    }

    #[test]
    #[serial]
    fn test_build_user_properties_includes_environment() {
        unsafe {
            env::remove_var("III_PROJECT_ID");
        }

        let ctx = TelemetryContext {
            install_id: "id-1".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "America/New_York".to_string(),
                cpu_cores: 8,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let props = ctx.build_user_properties(None);
        assert!(props.get("environment").is_some());
        assert!(props.get("device_type").is_some());
        assert_eq!(props["device_type"], "server");
        assert!(props.get("project_id").is_none());
    }

    #[test]
    #[serial]
    fn test_build_user_properties_with_project_id() {
        unsafe {
            env::set_var("III_PROJECT_ID", "proj-abc");
        }

        let ctx = TelemetryContext {
            install_id: "id-1".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let props = ctx.build_user_properties(None);
        assert_eq!(props["project_id"], "proj-abc");

        unsafe {
            env::remove_var("III_PROJECT_ID");
        }
    }

    #[test]
    #[serial]
    fn test_build_user_properties_with_sdk_telemetry() {
        unsafe {
            env::remove_var("III_PROJECT_ID");
        }

        let ctx = TelemetryContext {
            install_id: "id-1".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let telemetry = WorkerTelemetryMeta {
            language: Some("python".to_string()),
            project_name: Some("my-project".to_string()),
            framework: Some("fastapi".to_string()),
        };

        let props = ctx.build_user_properties(Some(&telemetry));
        assert_eq!(props["project_name"], "my-project");
        assert_eq!(props["framework"], "fastapi");
    }

    #[test]
    fn test_build_event_insert_id_is_unique() {
        let ctx = TelemetryContext {
            install_id: "id-1".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let event1 = ctx.build_event("evt", serde_json::json!({}), None);
        let event2 = ctx.build_event("evt", serde_json::json!({}), None);
        assert_ne!(
            event1.insert_id, event2.insert_id,
            "each event should have a unique insert_id"
        );
    }

    // =========================================================================
    // collect_functions_and_triggers
    // =========================================================================

    fn make_test_engine() -> Arc<Engine> {
        Arc::new(Engine::new())
    }

    #[test]
    fn test_collect_functions_and_triggers_empty_engine() {
        let engine = make_test_engine();
        let result = collect_functions_and_triggers(&engine);

        assert_eq!(result["function_count"], 0);
        assert_eq!(result["trigger_count"], 0);
        assert!(result["functions"].as_array().unwrap().is_empty());
        assert!(result["triggers"].as_array().unwrap().is_empty());
        assert!(result["trigger_types_used"].as_array().unwrap().is_empty());
        assert!(result["services"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_collect_functions_and_triggers_filters_engine_prefix() {
        let engine = make_test_engine();

        // Register a function that starts with "engine::" -- should be filtered out
        let handler: Arc<crate::function::HandlerFn> = Arc::new(|_inv_id, _input| {
            Box::pin(async { crate::function::FunctionResult::NoResult })
        });
        engine.functions.register_function(
            "engine::internal_fn".to_string(),
            crate::function::Function {
                handler: handler.clone(),
                _function_id: "engine::internal_fn".to_string(),
                _description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
        );

        // Register a normal function -- should be included
        engine.functions.register_function(
            "user.my_function".to_string(),
            crate::function::Function {
                handler,
                _function_id: "user.my_function".to_string(),
                _description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
        );

        let result = collect_functions_and_triggers(&engine);
        assert_eq!(result["function_count"], 1);
        let functions = result["functions"].as_array().unwrap();
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0], "user.my_function");
    }

    #[test]
    fn test_collect_functions_and_triggers_with_triggers() {
        let engine = make_test_engine();

        engine.trigger_registry.triggers.insert(
            "trigger-1".to_string(),
            crate::trigger::Trigger {
                id: "trigger-1".to_string(),
                trigger_type: "cron".to_string(),
                function_id: "my_fn".to_string(),
                config: serde_json::json!({}),
                worker_id: None,
            },
        );

        engine.trigger_registry.triggers.insert(
            "trigger-2".to_string(),
            crate::trigger::Trigger {
                id: "trigger-2".to_string(),
                trigger_type: "http".to_string(),
                function_id: "other_fn".to_string(),
                config: serde_json::json!({}),
                worker_id: None,
            },
        );

        let result = collect_functions_and_triggers(&engine);
        assert_eq!(result["trigger_count"], 2);

        let trigger_types_used = result["trigger_types_used"].as_array().unwrap();
        let types: Vec<&str> = trigger_types_used
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(types.contains(&"cron"));
        assert!(types.contains(&"http"));
    }

    #[test]
    fn test_collect_functions_and_triggers_with_services() {
        let engine = make_test_engine();

        engine.service_registry.services.insert(
            "my_service".to_string(),
            crate::services::Service::new("My Service".to_string(), "my_service".to_string()),
        );

        let result = collect_functions_and_triggers(&engine);
        let services = result["services"].as_array().unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0], "my_service");
    }

    // =========================================================================
    // collect_worker_data
    // =========================================================================

    #[test]
    fn test_collect_worker_data_empty_engine() {
        let engine = make_test_engine();
        let (runtime_counts, sdk_telemetry) = collect_worker_data(&engine);

        assert!(runtime_counts.is_empty());
        assert!(sdk_telemetry.is_none());
    }

    #[test]
    fn test_collect_worker_data_with_workers() {
        let engine = make_test_engine();

        let (tx1, _rx1) = tokio::sync::mpsc::channel(1);
        let mut worker1 = crate::workers::Worker::new(tx1);
        worker1.runtime = Some("node".to_string());
        worker1.telemetry = Some(WorkerTelemetryMeta {
            language: Some("typescript".to_string()),
            project_name: Some("proj-a".to_string()),
            framework: Some("next".to_string()),
        });
        let w1_id = worker1.id;
        engine.worker_registry.workers.insert(w1_id, worker1);

        let (tx2, _rx2) = tokio::sync::mpsc::channel(1);
        let mut worker2 = crate::workers::Worker::new(tx2);
        worker2.runtime = Some("python".to_string());
        worker2.telemetry = None;
        let w2_id = worker2.id;
        engine.worker_registry.workers.insert(w2_id, worker2);

        let (tx3, _rx3) = tokio::sync::mpsc::channel(1);
        let mut worker3 = crate::workers::Worker::new(tx3);
        worker3.runtime = Some("node".to_string());
        worker3.telemetry = None;
        let w3_id = worker3.id;
        engine.worker_registry.workers.insert(w3_id, worker3);

        let (runtime_counts, sdk_telemetry) = collect_worker_data(&engine);

        assert_eq!(*runtime_counts.get("node").unwrap(), 2);
        assert_eq!(*runtime_counts.get("python").unwrap(), 1);

        // Should have picked up the telemetry from worker1
        assert!(sdk_telemetry.is_some());
        let telem = sdk_telemetry.unwrap();
        assert_eq!(telem.language, Some("typescript".to_string()));
        assert_eq!(telem.project_name, Some("proj-a".to_string()));
        assert_eq!(telem.framework, Some("next".to_string()));
    }

    #[test]
    fn test_collect_worker_data_unknown_runtime_defaults() {
        let engine = make_test_engine();

        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut worker = crate::workers::Worker::new(tx);
        worker.runtime = None; // no runtime set
        worker.telemetry = None;
        let wid = worker.id;
        engine.worker_registry.workers.insert(wid, worker);

        let (runtime_counts, _) = collect_worker_data(&engine);
        assert_eq!(*runtime_counts.get("unknown").unwrap(), 1);
    }

    #[test]
    fn test_collect_worker_data_picks_smallest_uuid_telemetry() {
        let engine = make_test_engine();

        // Create two workers with telemetry; the one with smaller UUID should be picked
        let (tx1, _rx1) = tokio::sync::mpsc::channel(1);
        let mut worker1 = crate::workers::Worker::new(tx1);
        worker1.id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        worker1.runtime = Some("node".to_string());
        worker1.telemetry = Some(WorkerTelemetryMeta {
            language: Some("ts".to_string()),
            project_name: Some("proj-smallest".to_string()),
            framework: None,
        });
        engine.worker_registry.workers.insert(worker1.id, worker1);

        let (tx2, _rx2) = tokio::sync::mpsc::channel(1);
        let mut worker2 = crate::workers::Worker::new(tx2);
        worker2.id = uuid::Uuid::parse_str("ffffffff-ffff-ffff-ffff-ffffffffffff").unwrap();
        worker2.runtime = Some("node".to_string());
        worker2.telemetry = Some(WorkerTelemetryMeta {
            language: Some("py".to_string()),
            project_name: Some("proj-largest".to_string()),
            framework: None,
        });
        engine.worker_registry.workers.insert(worker2.id, worker2);

        let (_, sdk_telemetry) = collect_worker_data(&engine);
        let telem = sdk_telemetry.unwrap();
        assert_eq!(telem.project_name, Some("proj-smallest".to_string()));
    }

    // =========================================================================
    // TelemetryContext::build_user_properties edge cases
    // =========================================================================

    #[test]
    #[serial]
    fn test_build_user_properties_sdk_telemetry_partial_fields() {
        unsafe {
            env::remove_var("III_PROJECT_ID");
        }

        let ctx = TelemetryContext {
            install_id: "id-1".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        // Only project_name, no framework
        let telemetry = WorkerTelemetryMeta {
            language: None,
            project_name: Some("only-project".to_string()),
            framework: None,
        };
        let props = ctx.build_user_properties(Some(&telemetry));
        assert_eq!(props["project_name"], "only-project");
        assert!(props.get("framework").is_none());
    }

    #[test]
    #[serial]
    fn test_build_user_properties_sdk_telemetry_only_framework() {
        unsafe {
            env::remove_var("III_PROJECT_ID");
        }

        let ctx = TelemetryContext {
            install_id: "id-1".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let telemetry = WorkerTelemetryMeta {
            language: None,
            project_name: None,
            framework: Some("express".to_string()),
        };
        let props = ctx.build_user_properties(Some(&telemetry));
        assert!(props.get("project_name").is_none());
        assert_eq!(props["framework"], "express");
    }

    // =========================================================================
    // TelemetryContext::build_event edge cases
    // =========================================================================

    #[test]
    #[serial]
    fn test_build_event_without_sdk_telemetry_language_falls_back() {
        // When sdk_telemetry is None, language is detected from environment
        unsafe {
            env::remove_var("LANG");
            env::remove_var("LC_ALL");
        }

        let ctx = TelemetryContext {
            install_id: "id-test".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let event = ctx.build_event("evt", serde_json::json!({}), None);
        // With LANG/LC_ALL unset, detect_language returns None
        assert_eq!(event.language, None);
    }

    #[test]
    #[serial]
    fn test_build_event_with_lang_env_and_no_sdk() {
        unsafe {
            env::set_var("LANG", "en_US.UTF-8");
            env::remove_var("LC_ALL");
        }

        let ctx = TelemetryContext {
            install_id: "id-test".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let event = ctx.build_event("evt", serde_json::json!({}), None);
        assert_eq!(event.language, Some("en_US".to_string()));

        unsafe {
            env::remove_var("LANG");
        }
    }

    #[test]
    fn test_build_event_app_version_matches_cargo_pkg() {
        let ctx = TelemetryContext {
            install_id: "id-test".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let event = ctx.build_event("evt", serde_json::json!({}), None);
        assert_eq!(event.app_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_build_event_country_is_none() {
        let ctx = TelemetryContext {
            install_id: "id-test".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let event = ctx.build_event("evt", serde_json::json!({}), None);
        assert!(event.country.is_none());
    }

    #[test]
    fn test_build_event_user_properties_is_some() {
        let ctx = TelemetryContext {
            install_id: "id-test".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let event = ctx.build_event("evt", serde_json::json!({}), None);
        assert!(event.user_properties.is_some());
    }

    // =========================================================================
    // TelemetryContext clone
    // =========================================================================

    #[test]
    fn test_telemetry_context_clone() {
        let ctx = TelemetryContext {
            install_id: "clone-test-id".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: true,
                timezone: "America/Chicago".to_string(),
                cpu_cores: 16,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let cloned = ctx.clone();
        assert_eq!(cloned.install_id, ctx.install_id);
        assert_eq!(cloned.env_info.machine_id, ctx.env_info.machine_id);
        assert_eq!(cloned.env_info.is_container, ctx.env_info.is_container);
        assert_eq!(cloned.env_info.timezone, ctx.env_info.timezone);
        assert_eq!(cloned.env_info.cpu_cores, ctx.env_info.cpu_cores);
    }

    // =========================================================================
    // DisabledTelemetryModule
    // =========================================================================

    #[tokio::test]
    async fn test_disabled_telemetry_module_name() {
        let module = DisabledTelemetryModule;
        assert_eq!(module.name(), "Telemetry");
    }

    #[tokio::test]
    async fn test_disabled_telemetry_module_initialize() {
        let module = DisabledTelemetryModule;
        let result = module.initialize().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_disabled_telemetry_module_start_background_tasks() {
        let module = DisabledTelemetryModule;
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let result = module.start_background_tasks(rx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_disabled_telemetry_module_destroy() {
        let module = DisabledTelemetryModule;
        let result = module.destroy().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_disabled_telemetry_module_create() {
        let engine = make_test_engine();
        let result = DisabledTelemetryModule::create(engine, None).await;
        assert!(result.is_ok());
        let module = result.unwrap();
        assert_eq!(module.name(), "Telemetry");
    }

    // =========================================================================
    // TelemetryModule::create with disabled config
    // =========================================================================

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_create_disabled_by_config() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let engine = make_test_engine();
        let config = serde_json::json!({ "enabled": false });
        let module = TelemetryModule::create(engine, Some(config)).await.unwrap();
        // Should return a DisabledTelemetryModule
        assert_eq!(module.name(), "Telemetry");
        // Verify it is the disabled variant by testing initialize is Ok
        assert!(module.initialize().await.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_create_disabled_by_env_optout() {
        clear_ci_env_vars();
        unsafe {
            env::set_var("III_TELEMETRY_ENABLED", "false");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let engine = make_test_engine();
        let module = TelemetryModule::create(engine, None).await.unwrap();
        assert_eq!(module.name(), "Telemetry");

        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_create_disabled_by_ci() {
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }
        clear_ci_env_vars();
        unsafe {
            env::set_var("CI", "true");
        }

        let engine = make_test_engine();
        let module = TelemetryModule::create(engine, None).await.unwrap();
        assert_eq!(module.name(), "Telemetry");

        unsafe {
            env::remove_var("CI");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_create_disabled_by_dev_optout() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::set_var("III_TELEMETRY_DEV", "true");
        }

        let engine = make_test_engine();
        let module = TelemetryModule::create(engine, None).await.unwrap();
        assert_eq!(module.name(), "Telemetry");

        unsafe {
            env::remove_var("III_TELEMETRY_DEV");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_create_enabled_defaults_api_key() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let engine = make_test_engine();
        // Empty config should have api_key defaulted
        let module = TelemetryModule::create(engine, None).await.unwrap();
        assert_eq!(module.name(), "Telemetry");
    }

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_create_with_custom_api_key() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let engine = make_test_engine();
        let config = serde_json::json!({
            "api_key": "custom-key-123",
        });
        let module = TelemetryModule::create(engine, Some(config)).await.unwrap();
        assert_eq!(module.name(), "Telemetry");
    }

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_create_with_sdk_api_key() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let engine = make_test_engine();
        let config = serde_json::json!({
            "api_key": "primary-key",
            "sdk_api_key": "sdk-key-456",
        });
        let module = TelemetryModule::create(engine, Some(config)).await.unwrap();
        assert_eq!(module.name(), "Telemetry");
    }

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_create_with_empty_sdk_api_key() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let engine = make_test_engine();
        let config = serde_json::json!({
            "api_key": "primary-key",
            "sdk_api_key": "",
        });
        let module = TelemetryModule::create(engine, Some(config)).await.unwrap();
        assert_eq!(module.name(), "Telemetry");
    }

    // =========================================================================
    // TelemetryConfig deserialization edge cases
    // =========================================================================

    #[test]
    fn test_telemetry_config_deserialize_partial_fields() {
        let json = serde_json::json!({
            "heartbeat_interval_secs": 120
        });
        let config: TelemetryConfig = serde_json::from_value(json).unwrap();
        assert!(config.enabled); // default
        assert!(config.api_key.is_empty()); // default
        assert!(config.sdk_api_key.is_none()); // default
        assert_eq!(config.heartbeat_interval_secs, 120);
    }

    #[test]
    fn test_telemetry_config_deserialize_null_sdk_api_key() {
        let json = serde_json::json!({
            "sdk_api_key": null
        });
        let config: TelemetryConfig = serde_json::from_value(json).unwrap();
        assert!(config.sdk_api_key.is_none());
    }

    // =========================================================================
    // get_or_create_install_id
    // =========================================================================

    #[test]
    fn test_get_or_create_install_id_returns_nonempty_string() {
        let id = get_or_create_install_id();
        assert!(!id.is_empty());
    }

    #[test]
    fn test_get_or_create_install_id_is_stable() {
        let id1 = get_or_create_install_id();
        let id2 = get_or_create_install_id();
        assert_eq!(id1, id2, "install_id should be stable across calls");
    }

    // =========================================================================
    // build_client_context edge cases
    // =========================================================================

    #[test]
    fn test_build_client_context_multiple_runtimes_counts() {
        let mut runtime_counts = HashMap::new();
        runtime_counts.insert("node".to_string(), 5);
        runtime_counts.insert("python".to_string(), 3);
        runtime_counts.insert("ruby".to_string(), 1);

        let ctx = build_client_context(&runtime_counts, None);

        let sdk_detected = ctx["sdk_detected"].as_array().unwrap();
        assert_eq!(sdk_detected.len(), 3);

        let worker_runtimes = ctx["worker_runtimes"].as_array().unwrap();
        assert_eq!(worker_runtimes.len(), 3);
    }

    // =========================================================================
    // collect_functions_and_triggers has triggers with data
    // =========================================================================

    #[test]
    fn test_collect_functions_and_triggers_trigger_data_shape() {
        let engine = make_test_engine();

        engine.trigger_registry.triggers.insert(
            "t-1".to_string(),
            crate::trigger::Trigger {
                id: "t-1".to_string(),
                trigger_type: "cron".to_string(),
                function_id: "fn-1".to_string(),
                config: serde_json::json!({"schedule": "* * * * *"}),
                worker_id: None,
            },
        );

        let result = collect_functions_and_triggers(&engine);
        let triggers = result["triggers"].as_array().unwrap();
        assert_eq!(triggers.len(), 1);

        let t = &triggers[0];
        assert_eq!(t["id"], "t-1");
        assert_eq!(t["type"], "cron");
        assert_eq!(t["function_id"], "fn-1");
    }

    // =========================================================================
    // collect_worker_data: telemetry with all None fields is skipped
    // =========================================================================

    #[test]
    fn test_collect_worker_data_skips_telemetry_with_all_none() {
        let engine = make_test_engine();

        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut worker = crate::workers::Worker::new(tx);
        worker.runtime = Some("node".to_string());
        // Telemetry with all None fields should be skipped
        worker.telemetry = Some(WorkerTelemetryMeta {
            language: None,
            project_name: None,
            framework: None,
        });
        let wid = worker.id;
        engine.worker_registry.workers.insert(wid, worker);

        let (_, sdk_telemetry) = collect_worker_data(&engine);
        // All fields are None, so the filter in collect_worker_data should skip it
        assert!(sdk_telemetry.is_none());
    }

    // =========================================================================
    // TelemetryModule::active_client tests (via create)
    // =========================================================================

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_initialize_is_ok() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let engine = make_test_engine();
        let config = serde_json::json!({
            "api_key": "test-key",
        });
        let module = TelemetryModule::create(engine, Some(config)).await.unwrap();
        assert!(module.initialize().await.is_ok());
    }

    // =========================================================================
    // TelemetryModule::name
    // =========================================================================

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_name_is_telemetry() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let engine = make_test_engine();
        let config = serde_json::json!({ "api_key": "key" });
        let module = TelemetryModule::create(engine, Some(config)).await.unwrap();
        assert_eq!(module.name(), "Telemetry");
    }

    #[test]
    fn test_active_client_prefers_sdk_client_when_available() {
        let engine = make_test_engine();
        let without_sdk = build_manual_module(engine.clone(), false, 1);
        assert!(Arc::ptr_eq(
            without_sdk.active_client(),
            &without_sdk.client
        ));

        let with_sdk = build_manual_module(engine, true, 1);
        let sdk_client = with_sdk
            .sdk_client
            .as_ref()
            .expect("sdk client should exist");
        assert!(Arc::ptr_eq(with_sdk.active_client(), sdk_client));
    }

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_background_tasks_and_destroy_run_without_network() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }
        reset_telemetry_globals();
        crate::modules::observability::metrics::ensure_default_meter();

        let engine = make_test_engine();
        register_test_function(&engine, "svc.worker");

        engine
            .trigger_registry
            .register_trigger_type(TriggerType {
                id: "queue".to_string(),
                _description: "Queue".to_string(),
                registrator: Box::new(NoopRegistrator),
                worker_id: None,
            })
            .await
            .expect("register trigger type");
        engine
            .trigger_registry
            .register_trigger(Trigger {
                id: "queue-trigger-1".to_string(),
                trigger_type: "queue".to_string(),
                function_id: "svc.worker".to_string(),
                config: serde_json::json!({ "topic": "orders" }),
                worker_id: None,
            })
            .await
            .expect("register trigger");

        let (worker_tx, _worker_rx) = mpsc::channel(1);
        let mut worker = Worker::new(worker_tx);
        worker.runtime = Some("node".to_string());
        worker.telemetry = Some(WorkerTelemetryMeta {
            language: Some("typescript".to_string()),
            project_name: Some("telemetry-spec".to_string()),
            framework: Some("iii-js".to_string()),
        });
        engine.worker_registry.register_worker(worker);

        let acc = get_metrics_accumulator();
        acc.invocations_total.store(12, Ordering::Relaxed);
        acc.invocations_success.store(9, Ordering::Relaxed);
        acc.invocations_error.store(3, Ordering::Relaxed);
        acc.workers_spawns.store(4, Ordering::Relaxed);
        acc.workers_deaths.store(1, Ordering::Relaxed);
        acc.invocations_by_function
            .insert("svc.worker".to_string(), 12);

        let telemetry = collector();
        telemetry.queue_emits.store(7, Ordering::Relaxed);
        telemetry.api_requests.store(5, Ordering::Relaxed);
        telemetry.function_registrations.store(1, Ordering::Relaxed);
        telemetry.trigger_registrations.store(1, Ordering::Relaxed);

        let module = build_manual_module(engine, true, 1);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        module
            .start_background_tasks(shutdown_rx)
            .await
            .expect("start background tasks");

        tokio::time::sleep(Duration::from_millis(2200)).await;
        shutdown_tx.send(true).expect("signal shutdown");
        tokio::time::sleep(Duration::from_millis(100)).await;
        module.destroy().await.expect("destroy telemetry module");

        reset_telemetry_globals();
    }

    // =========================================================================
    // build_event timestamp is recent
    // =========================================================================

    #[test]
    fn test_build_event_timestamp_is_recent() {
        let ctx = TelemetryContext {
            install_id: "id-test".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                is_container: false,
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let event = ctx.build_event("evt", serde_json::json!({}), None);
        // Timestamp should be within 5 seconds of now
        assert!((event.time - now_ms).abs() < 5000);
    }

    // =========================================================================
    // build_user_properties environment json shape
    // =========================================================================

    #[test]
    #[serial]
    fn test_build_user_properties_environment_shape() {
        unsafe {
            env::remove_var("III_PROJECT_ID");
        }

        let ctx = TelemetryContext {
            install_id: "id-1".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "test-machine".to_string(),
                is_container: true,
                timezone: "Europe/Berlin".to_string(),
                cpu_cores: 32,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
        };

        let props = ctx.build_user_properties(None);
        let env_val = &props["environment"];
        assert_eq!(env_val["machine_id"], "test-machine");
        assert_eq!(env_val["is_container"], true);
        assert_eq!(env_val["timezone"], "Europe/Berlin");
        assert_eq!(env_val["cpu_cores"], 32);
        assert_eq!(env_val["os"], "linux");
        assert_eq!(env_val["arch"], "x86_64");
    }
}
