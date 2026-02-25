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

fn collect_worker_data(
    engine: &Engine,
) -> (HashMap<String, u64>, Option<WorkerTelemetryMeta>) {
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
            let client_context =
                build_client_context(&runtime_counts, sdk_telemetry.as_ref());

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
        let client_context =
            build_client_context(&runtime_counts, sdk_telemetry.as_ref());

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
