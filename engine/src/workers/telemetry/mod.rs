// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod amplitude;
pub mod collector;
pub mod environment;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::engine::Engine;
use crate::worker_connections::WorkerConnectionTelemetryMeta;
use crate::workers::traits::Worker;

use self::amplitude::{AmplitudeClient, AmplitudeEvent};
use self::environment::EnvironmentInfo;

const API_KEY: &str = "a7182ac460dde671c8f2e1318b517228";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
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
            sdk_api_key: None,
            heartbeat_interval_secs: 6 * 60 * 60,
        }
    }
}

struct ProjectContext {
    project_id: Option<String>,
    project_name: Option<String>,
    source: Option<String>,
}

fn find_project_root() -> Option<PathBuf> {
    if let Ok(root) = std::env::var("III_PROJECT_ROOT")
        && !root.is_empty()
    {
        return Some(PathBuf::from(root));
    }

    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".iii").join("project.ini").exists() {
            return Some(dir.clone());
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

struct ProjectIniData {
    project_id: Option<String>,
    project_name: Option<String>,
    source: Option<String>,
}

fn read_project_ini(root: &std::path::Path) -> Option<ProjectIniData> {
    let ini_path = root.join(".iii").join("project.ini");
    let contents = std::fs::read_to_string(&ini_path).ok()?;

    let mut project_id: Option<String> = None;
    let mut project_name: Option<String> = None;
    let mut source: Option<String> = None;

    for line in contents.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("project_id=") {
            let val = val.trim();
            if !val.is_empty() {
                project_id = Some(val.to_string());
            }
        } else if let Some(val) = line.strip_prefix("project_name=") {
            let val = val.trim();
            if !val.is_empty() {
                project_name = Some(val.to_string());
            }
        } else if let Some(val) = line.strip_prefix("source=") {
            let val = val.trim();
            if !val.is_empty() {
                source = Some(val.to_string());
            }
        }
    }

    if project_id.is_some() || project_name.is_some() || source.is_some() {
        Some(ProjectIniData {
            project_id,
            project_name,
            source,
        })
    } else {
        None
    }
}

fn resolve_project_context(
    sdk_telemetry: Option<&WorkerConnectionTelemetryMeta>,
) -> ProjectContext {
    let ini_data = find_project_root().and_then(|root| read_project_ini(&root));

    let project_id = ini_data
        .as_ref()
        .and_then(|d| d.project_id.clone())
        .or_else(|| {
            std::env::var("III_PROJECT_ID")
                .ok()
                .filter(|s| !s.is_empty())
        });

    let project_name = ini_data
        .as_ref()
        .and_then(|d| d.project_name.clone())
        .or_else(|| sdk_telemetry.and_then(|t| t.project_name.clone()));

    let source = ini_data.as_ref().and_then(|d| d.source.clone());

    ProjectContext {
        project_id,
        project_name,
        source,
    }
}

fn get_or_create_device_id() -> String {
    environment::get_or_create_device_id()
}

fn check_and_mark_first_run() -> bool {
    if environment::read_config_key("state", "first_run_sent").as_deref() == Some("true") {
        return false;
    }

    environment::set_config_key("state", "first_run_sent", "true");
    true
}

enum DisableReason {
    UserOptOut,
    CiDetected,
    DevOptOut,
    Config,
}

pub fn is_iii_builtin_function_id(id: &str) -> bool {
    id.starts_with("engine::")
        || id.starts_with("state::")
        || id.starts_with("stream::")
        || id.starts_with("iii::")
        || id.starts_with("bridge.")
        || id.starts_with("motia::")
        || id == "publish"
        || id == "motia_step_get"
        || id.starts_with("steps::")
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

struct FunctionTriggerData {
    function_count: usize,
    functions: Vec<String>,
    trigger_count: usize,
    trigger_types: Vec<String>,
}

struct EngineSnapshot {
    ft: FunctionTriggerData,
    wd: WorkerData,
    project: ProjectContext,
}

fn collect_engine_snapshot(engine: &Engine) -> EngineSnapshot {
    let ft = collect_functions_and_triggers(engine);
    let wd = collect_worker_data(engine);
    let project = resolve_project_context(wd.sdk_telemetry.as_ref());
    EngineSnapshot { ft, wd, project }
}

fn build_base_properties(snap: &EngineSnapshot) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    m.insert(
        "project_id".into(),
        serde_json::json!(snap.project.project_id),
    );
    m.insert(
        "project_name".into(),
        serde_json::json!(snap.project.project_name),
    );
    if let Some(source) = &snap.project.source {
        m.insert("source".into(), serde_json::json!(source));
    }
    m.insert(
        "version".into(),
        serde_json::json!(env!("CARGO_PKG_VERSION")),
    );
    m.insert(
        "function_count".into(),
        serde_json::json!(snap.ft.function_count),
    );
    m.insert(
        "trigger_count".into(),
        serde_json::json!(snap.ft.trigger_count),
    );
    m.insert("functions".into(), serde_json::json!(snap.ft.functions));
    m.insert(
        "trigger_types".into(),
        serde_json::json!(snap.ft.trigger_types),
    );
    m.insert("client_type".into(), serde_json::json!(snap.wd.client_type));
    m.insert(
        "sdk_languages".into(),
        serde_json::json!(snap.wd.sdk_languages),
    );
    m.insert(
        "worker_count_total".into(),
        serde_json::json!(snap.wd.worker_count_total),
    );
    for (fw, count) in &snap.wd.worker_count_by_framework {
        m.insert(format!("worker_count_{fw}"), serde_json::json!(count));
    }
    m.insert("workers".into(), serde_json::json!(snap.wd.workers));
    m
}

// TODO: Re-enable delta metrics reporting once more important dashboards are ready.
//
// struct DeltaAccumulator {
//     invocations_total: u64,
//     invocations_success: u64,
//     invocations_error: u64,
//     api_requests: u64,
//     queue_emits: u64,
//     queue_consumes: u64,
//     pubsub_publishes: u64,
//     pubsub_subscribes: u64,
//     cron_executions: u64,
// }
//
// impl DeltaAccumulator {
//     fn new() -> Self {
//         Self {
//             invocations_total: 0,
//             invocations_success: 0,
//             invocations_error: 0,
//             api_requests: 0,
//             queue_emits: 0,
//             queue_consumes: 0,
//             pubsub_publishes: 0,
//             pubsub_subscribes: 0,
//             cron_executions: 0,
//         }
//     }
//
//     fn snapshot(&mut self) -> DeltaSnapshot {
//         use std::sync::atomic::Ordering;
//         let acc = crate::workers::observability::metrics::get_metrics_accumulator();
//         let col = collector();
//
//         let cur = DeltaAccumulator {
//             invocations_total: acc.invocations_total.load(Ordering::Relaxed),
//             invocations_success: acc.invocations_success.load(Ordering::Relaxed),
//             invocations_error: acc.invocations_error.load(Ordering::Relaxed),
//             api_requests: col.api_requests.load(Ordering::Relaxed),
//             queue_emits: col.queue_emits.load(Ordering::Relaxed),
//             queue_consumes: col.queue_consumes.load(Ordering::Relaxed),
//             pubsub_publishes: col.pubsub_publishes.load(Ordering::Relaxed),
//             pubsub_subscribes: col.pubsub_subscribes.load(Ordering::Relaxed),
//             cron_executions: col.cron_executions.load(Ordering::Relaxed),
//         };
//
//         let deltas = DeltaSnapshot {
//             invocations_total: cur.invocations_total.saturating_sub(self.invocations_total),
//             invocations_success: cur
//                 .invocations_success
//                 .saturating_sub(self.invocations_success),
//             invocations_error: cur.invocations_error.saturating_sub(self.invocations_error),
//             api_requests: cur.api_requests.saturating_sub(self.api_requests),
//             queue_emits: cur.queue_emits.saturating_sub(self.queue_emits),
//             queue_consumes: cur.queue_consumes.saturating_sub(self.queue_consumes),
//             pubsub_publishes: cur.pubsub_publishes.saturating_sub(self.pubsub_publishes),
//             pubsub_subscribes: cur.pubsub_subscribes.saturating_sub(self.pubsub_subscribes),
//             cron_executions: cur.cron_executions.saturating_sub(self.cron_executions),
//         };
//
//         *self = cur;
//         deltas
//     }
// }
//
// struct DeltaSnapshot {
//     invocations_total: u64,
//     invocations_success: u64,
//     invocations_error: u64,
//     api_requests: u64,
//     queue_emits: u64,
//     queue_consumes: u64,
//     pubsub_publishes: u64,
//     pubsub_subscribes: u64,
//     cron_executions: u64,
// }
//
// impl DeltaSnapshot {
//     fn insert_into(&self, m: &mut serde_json::Map<String, serde_json::Value>) {
//         m.insert(
//             "delta_invocations_total".into(),
//             serde_json::json!(self.invocations_total),
//         );
//         m.insert(
//             "delta_invocations_success".into(),
//             serde_json::json!(self.invocations_success),
//         );
//         m.insert(
//             "delta_invocations_error".into(),
//             serde_json::json!(self.invocations_error),
//         );
//         m.insert(
//             "delta_api_requests".into(),
//             serde_json::json!(self.api_requests),
//         );
//         m.insert(
//             "delta_queue_emits".into(),
//             serde_json::json!(self.queue_emits),
//         );
//         m.insert(
//             "delta_queue_consumes".into(),
//             serde_json::json!(self.queue_consumes),
//         );
//         m.insert(
//             "delta_pubsub_publishes".into(),
//             serde_json::json!(self.pubsub_publishes),
//         );
//         m.insert(
//             "delta_pubsub_subscribes".into(),
//             serde_json::json!(self.pubsub_subscribes),
//         );
//         m.insert(
//             "delta_cron_executions".into(),
//             serde_json::json!(self.cron_executions),
//         );
//     }
// }

fn collect_functions_and_triggers(engine: &Engine) -> FunctionTriggerData {
    let functions: Vec<String> = engine
        .functions
        .iter()
        .map(|entry| entry.key().clone())
        .filter(|id| !is_iii_builtin_function_id(id))
        .collect();

    let function_count = functions.len();

    let mut trigger_types_used: HashSet<String> = HashSet::new();
    let mut trigger_count = 0usize;

    for entry in engine.trigger_registry.triggers.iter() {
        let trigger = entry.value();
        trigger_types_used.insert(trigger.trigger_type.clone());
        trigger_count += 1;
    }

    FunctionTriggerData {
        function_count,
        functions,
        trigger_count,
        trigger_types: trigger_types_used.into_iter().collect(),
    }
}

struct WorkerData {
    worker_count_total: usize,
    worker_count_by_framework: HashMap<String, u64>,
    worker_count_by_language: HashMap<String, u64>,
    workers: Vec<String>,
    sdk_languages: Vec<String>,
    client_type: String,
    sdk_telemetry: Option<WorkerConnectionTelemetryMeta>,
}

fn collect_worker_data(engine: &Engine) -> WorkerData {
    let mut runtime_counts: HashMap<String, u64> = HashMap::new();
    let mut framework_counts: HashMap<String, u64> = HashMap::new();
    let mut best_telemetry: Option<(uuid::Uuid, WorkerConnectionTelemetryMeta)> = None;
    let mut worker_count_total = 0usize;
    let mut workers: Vec<String> = Vec::new();

    for entry in engine.worker_registry.workers.iter() {
        let worker = entry.value();

        let Some(runtime) = worker.runtime.clone() else {
            continue;
        };

        worker_count_total += 1;
        *runtime_counts.entry(runtime.clone()).or_insert(0) += 1;

        let framework = worker
            .telemetry
            .as_ref()
            .and_then(|t| t.framework.clone())
            .unwrap_or_default();

        if !framework.is_empty() {
            *framework_counts.entry(framework.clone()).or_insert(0) += 1;
            workers.push(format!("{}:{}", runtime, framework));
        } else {
            workers.push(runtime);
        }

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

    let sdk_telemetry = best_telemetry.map(|(_, t)| t);

    let client_type = sdk_telemetry
        .as_ref()
        .and_then(|t| t.framework.clone())
        .unwrap_or_else(|| environment::detect_client_type().to_string());

    let sdk_languages: Vec<String> = runtime_counts
        .keys()
        .map(|r| match r.as_str() {
            "node" => "iii-node".to_string(),
            "python" => "iii-py".to_string(),
            "rust" => "iii-rust".to_string(),
            other => other.to_string(),
        })
        .collect();

    WorkerData {
        worker_count_total,
        worker_count_by_framework: framework_counts,
        worker_count_by_language: runtime_counts,
        workers,
        sdk_languages,
        client_type,
        sdk_telemetry,
    }
}

/// Cloneable context for building telemetry events inside spawned tasks.
#[derive(Clone)]
struct TelemetryContext {
    device_id: String,
    env_info: EnvironmentInfo,
}

impl TelemetryContext {
    fn build_user_properties(
        &self,
        sdk_telemetry: Option<&WorkerConnectionTelemetryMeta>,
    ) -> serde_json::Value {
        let env = &self.env_info;
        let project = resolve_project_context(sdk_telemetry);

        let mut props = serde_json::json!({
            "environment.os": env.os,
            "environment.arch": env.arch,
            "environment.cpu_cores": env.cpu_cores,
            "environment.timezone": env.timezone,
            "environment.machine_id": env.machine_id,
            "iii_execution_context": env.iii_execution_context,
            "env": environment::detect_env(),
            "install_method": environment::detect_install_method(),
            "iii_version": env!("CARGO_PKG_VERSION"),
        });

        let host_user_id = std::env::var("III_HOST_USER_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(environment::find_project_ini_device_id);

        if let Some(id) = host_user_id {
            props["host_user_id"] = serde_json::Value::String(id);
        }

        if let Some(project_id) = project.project_id {
            props["project_id"] = serde_json::Value::String(project_id);
        }
        if let Some(project_name) = project.project_name {
            props["project_name"] = serde_json::Value::String(project_name);
        }

        props
    }

    fn build_event(
        &self,
        event_type: &str,
        properties: serde_json::Value,
        sdk_telemetry: Option<&WorkerConnectionTelemetryMeta>,
    ) -> AmplitudeEvent {
        let language = sdk_telemetry
            .and_then(|t| t.language.clone())
            .or_else(environment::detect_language);
        AmplitudeEvent {
            device_id: self.device_id.clone(),
            user_id: None,
            event_type: event_type.to_string(),
            event_properties: properties,
            user_properties: Some(self.build_user_properties(sdk_telemetry)),
            platform: "iii-engine".to_string(),
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

const TEMPLATE_POLL_INTERVAL_SECS: u64 = 3;
const TEMPLATE_POLL_TIMEOUT_SECS: u64 = 60 * 60;

fn build_template_lifecycle_properties(
    event_type: &str,
    function_id: &str,
    source: &str,
    project: &ProjectContext,
) -> (String, serde_json::Value) {
    let mut props = serde_json::Map::new();
    props.insert("function_id".into(), serde_json::json!(function_id));
    props.insert("source".into(), serde_json::json!(source));
    if let Some(pid) = &project.project_id {
        props.insert("project_id".into(), serde_json::json!(pid));
    }
    if let Some(pname) = &project.project_name {
        props.insert("project_name".into(), serde_json::json!(pname));
    }
    (event_type.to_string(), serde_json::Value::Object(props))
}

pub struct TelemetryWorker {
    engine: Arc<Engine>,
    config: TelemetryConfig,
    client: Arc<AmplitudeClient>,
    sdk_client: Option<Arc<AmplitudeClient>>,
    ctx: TelemetryContext,
    start_time: Instant,
}

impl TelemetryWorker {
    fn active_client(&self) -> &Arc<AmplitudeClient> {
        self.sdk_client.as_ref().unwrap_or(&self.client)
    }
}

struct DisabledTelemetryWorker;

#[async_trait]
impl Worker for DisabledTelemetryWorker {
    fn name(&self) -> &'static str {
        "Telemetry"
    }

    async fn create(
        _engine: Arc<Engine>,
        _config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Worker>> {
        Ok(Box::new(DisabledTelemetryWorker))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        _shutdown_rx: tokio::sync::watch::Receiver<bool>,
        _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Worker for TelemetryWorker {
    fn name(&self) -> &'static str {
        "Telemetry"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        let telemetry_config: TelemetryConfig = match config {
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
            return Ok(Box::new(DisabledTelemetryWorker));
        }

        let device_id = get_or_create_device_id();
        let env_info = EnvironmentInfo::collect();

        tracing::info!("Anonymous telemetry enabled. Set III_TELEMETRY_ENABLED=false to disable.");

        let client = Arc::new(AmplitudeClient::new(API_KEY.to_string()));

        let sdk_client = telemetry_config
            .sdk_api_key
            .as_deref()
            .filter(|k| !k.is_empty())
            .map(|key| Arc::new(AmplitudeClient::new(key.to_owned())));

        let ctx = TelemetryContext {
            device_id: device_id.clone(),
            env_info,
        };

        Ok(Box::new(TelemetryWorker {
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
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
        _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        let interval_secs = self.config.heartbeat_interval_secs;
        let client = Arc::clone(self.active_client());
        let engine = Arc::clone(&self.engine);
        let ctx = self.ctx.clone();
        let start_time = self.start_time;

        let engine_for_started = Arc::clone(&self.engine);
        let client_for_started = Arc::clone(self.active_client());
        let ctx_for_started = self.ctx.clone();
        tokio::spawn(async move {
            let user_invocation = collector::first_user_invocation_notify().notified();
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => {},
                _ = user_invocation => {},
            }

            let snap = collect_engine_snapshot(&engine_for_started);

            if check_and_mark_first_run() {
                let first_run_event = ctx_for_started.build_event(
                    "first_run",
                    serde_json::json!({
                        "version": env!("CARGO_PKG_VERSION"),
                        "os": std::env::consts::OS,
                        "arch": std::env::consts::ARCH,
                        "install_method": environment::detect_install_method(),
                    }),
                    snap.wd.sdk_telemetry.as_ref(),
                );
                let _ = client_for_started.send_event(first_run_event).await;
            }

            let mut props = build_base_properties(&snap);
            props.insert("session_start".into(), serde_json::json!(true));
            props.insert(
                "worker_count_by_language".into(),
                serde_json::json!(snap.wd.worker_count_by_language),
            );
            props.insert("period_secs".into(), serde_json::json!(interval_secs));
            props.insert(
                "uptime_secs".into(),
                serde_json::json!(start_time.elapsed().as_secs()),
            );
            // TODO: Re-enable delta metrics once more important dashboards are ready.
            // let d = DeltaAccumulator::new().snapshot();
            // props.insert("is_active".into(), serde_json::json!(d.invocations_total > 0));
            // d.insert_into(&mut props);

            let boot_heartbeat = ctx_for_started.build_event(
                "heartbeat",
                serde_json::Value::Object(props),
                snap.wd.sdk_telemetry.as_ref(),
            );
            let _ = client_for_started.send_event(boot_heartbeat).await;
        });

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));

            interval.tick().await;

            // TODO: Re-enable delta metrics once downstream dashboards are ready.
            // let mut deltas = DeltaAccumulator::new();

            loop {
                tokio::select! {
                    result = shutdown_rx.changed() => {
                        if result.is_err() || *shutdown_rx.borrow() {

                            let snap = collect_engine_snapshot(&engine);

                            let mut props = build_base_properties(&snap);
                            props.insert("uptime_secs".into(), serde_json::json!(start_time.elapsed().as_secs()));

                            let event = ctx.build_event(
                                "engine_stopped",
                                serde_json::Value::Object(props),
                                snap.wd.sdk_telemetry.as_ref(),
                            );

                            let _ = tokio::time::timeout(
                                std::time::Duration::from_secs(5),
                                client.send_event(event),
                            )
                            .await;

                            break;
                        }
                    }
                    _ = interval.tick() => {
                        // let d = deltas.snapshot();
                        let snap = collect_engine_snapshot(&engine);

                        let mut props = build_base_properties(&snap);
                        props.insert("session_start".into(), serde_json::json!(false));
                        props.insert("worker_count_by_language".into(), serde_json::json!(snap.wd.worker_count_by_language));
                        props.insert("period_secs".into(), serde_json::json!(interval_secs));
                        props.insert("uptime_secs".into(), serde_json::json!(start_time.elapsed().as_secs()));
                        // props.insert("is_active".into(), serde_json::json!(d.invocations_total > 0));
                        // d.insert_into(&mut props);

                        let event = ctx.build_event(
                            "heartbeat",
                            serde_json::Value::Object(props),
                            snap.wd.sdk_telemetry.as_ref(),
                        );

                        let _ = client.send_event(event).await;
                    }
                }
            }
        });

        // Template lifecycle polling: fires template_success / template_failure
        // once each when the first user function succeeds or fails.
        let project_ctx = resolve_project_context(None);
        if let Some(source) = project_ctx.source {
            let client_for_template = Arc::clone(self.active_client());
            let ctx_for_template = self.ctx.clone();
            let project_for_template = resolve_project_context(None);
            tokio::spawn(async move {
                let mut success_sent = false;
                let mut failure_sent = false;
                let timeout = std::time::Duration::from_secs(TEMPLATE_POLL_TIMEOUT_SECS);

                loop {
                    if start_time.elapsed() > timeout || (success_sent && failure_sent) {
                        break;
                    }

                    tokio::time::sleep(std::time::Duration::from_secs(TEMPLATE_POLL_INTERVAL_SECS))
                        .await;

                    let acc = crate::workers::observability::metrics::get_metrics_accumulator();

                    if !success_sent {
                        if let Some(fn_id) = acc.first_user_success_fn.get() {
                            let (event_type, props) = build_template_lifecycle_properties(
                                "template_success",
                                fn_id,
                                &source,
                                &project_for_template,
                            );
                            let event = ctx_for_template.build_event(&event_type, props, None);
                            let _ = client_for_template.send_event(event).await;
                            success_sent = true;
                        }
                    }

                    if !failure_sent {
                        if let Some(fn_id) = acc.first_user_failure_fn.get() {
                            let (event_type, props) = build_template_lifecycle_properties(
                                "template_failure",
                                fn_id,
                                &source,
                                &project_for_template,
                            );
                            let event = ctx_for_template.build_event(&event_type, props, None);
                            let _ = client_for_template.send_event(event).await;
                            failure_sent = true;
                        }
                    }
                }
            });
        }

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

crate::register_worker!("iii-telemetry", TelemetryWorker, mandatory);

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::{env, future::Future, pin::Pin, sync::atomic::Ordering, time::Duration};
    use tokio::sync::mpsc;

    use crate::{
        function::{Function, FunctionResult, HandlerFn},
        services::Service,
        trigger::{Trigger, TriggerRegistrator, TriggerType},
        worker_connections::WorkerConnection,
        workers::{
            observability::metrics::get_metrics_accumulator, telemetry::collector::collector,
        },
    };

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
        let handler: Arc<HandlerFn> = Arc::new(|_invocation_id, _input, _session| {
            Box::pin(async { FunctionResult::NoResult })
        });
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

    fn make_env_info() -> EnvironmentInfo {
        EnvironmentInfo {
            machine_id: "machine-1".to_string(),
            iii_execution_context: "user".to_string(),
            timezone: "UTC".to_string(),
            cpu_cores: 4,
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            host_user_id: None,
        }
    }

    fn build_manual_module(
        engine: Arc<Engine>,
        sdk_client: bool,
        heartbeat_interval_secs: u64,
    ) -> TelemetryWorker {
        TelemetryWorker {
            engine,
            config: TelemetryConfig {
                enabled: true,
                sdk_api_key: sdk_client.then(|| "sdk-test-key".to_string()),
                heartbeat_interval_secs,
            },
            client: Arc::new(AmplitudeClient::new(String::new())),
            sdk_client: sdk_client.then(|| Arc::new(AmplitudeClient::new(String::new()))),
            ctx: TelemetryContext {
                device_id: "test-install-id".to_string(),
                env_info: make_env_info(),
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
        assert!(config.sdk_api_key.is_none());
        assert_eq!(config.heartbeat_interval_secs, 6 * 60 * 60);
    }

    #[test]
    fn test_telemetry_config_deserialize_defaults() {
        let json = serde_json::json!({});
        let config: TelemetryConfig = serde_json::from_value(json).unwrap();
        assert!(config.enabled);
        assert!(config.sdk_api_key.is_none());
        assert_eq!(config.heartbeat_interval_secs, 6 * 60 * 60);
    }

    #[test]
    fn test_telemetry_config_deserialize_overrides() {
        let json = serde_json::json!({
            "enabled": false,
            "sdk_api_key": "sdk-key",
            "heartbeat_interval_secs": 3600
        });
        let config: TelemetryConfig = serde_json::from_value(json).unwrap();
        assert!(!config.enabled);
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
    }

    // =========================================================================
    // resolve_project_context
    // =========================================================================

    #[test]
    #[serial]
    fn test_resolve_project_context_env_fallback() {
        unsafe {
            env::set_var("III_PROJECT_ID", "proj-123");
            env::remove_var("III_PROJECT_ROOT");
        }
        let ctx = resolve_project_context(None);
        assert_eq!(ctx.project_id, Some("proj-123".to_string()));
        unsafe {
            env::remove_var("III_PROJECT_ID");
        }
    }

    #[test]
    #[serial]
    fn test_resolve_project_context_sdk_telemetry_project_name() {
        unsafe {
            env::remove_var("III_PROJECT_ID");
            env::remove_var("III_PROJECT_ROOT");
        }
        let telemetry = WorkerConnectionTelemetryMeta {
            language: None,
            project_name: Some("my-sdk-project".to_string()),
            framework: None,
        };
        let ctx = resolve_project_context(Some(&telemetry));
        assert_eq!(ctx.project_name, Some("my-sdk-project".to_string()));
    }

    #[test]
    #[serial]
    fn test_resolve_project_context_none_when_unset() {
        unsafe {
            env::remove_var("III_PROJECT_ID");
            env::remove_var("III_PROJECT_ROOT");
        }
        let ctx = resolve_project_context(None);
        assert_eq!(ctx.project_id, None);
        assert_eq!(ctx.project_name, None);
    }

    // =========================================================================
    // read_project_ini
    // =========================================================================

    #[test]
    fn test_read_project_ini_parses_values() {
        let dir = tempfile::tempdir().unwrap();
        let iii_dir = dir.path().join(".iii");
        std::fs::create_dir_all(&iii_dir).unwrap();
        std::fs::write(
            iii_dir.join("project.ini"),
            "project_id=abc-123\nproject_name=my-project\n",
        )
        .unwrap();

        let data = read_project_ini(dir.path()).unwrap();
        assert_eq!(data.project_id, Some("abc-123".to_string()));
        assert_eq!(data.project_name, Some("my-project".to_string()));
        assert_eq!(data.source, None);
    }

    #[test]
    fn test_read_project_ini_parses_source() {
        let dir = tempfile::tempdir().unwrap();
        let iii_dir = dir.path().join(".iii");
        std::fs::create_dir_all(&iii_dir).unwrap();
        std::fs::write(
            iii_dir.join("project.ini"),
            "project_id=abc-123\nproject_name=my-project\nsource=quickstart\n",
        )
        .unwrap();

        let data = read_project_ini(dir.path()).unwrap();
        assert_eq!(data.project_id, Some("abc-123".to_string()));
        assert_eq!(data.project_name, Some("my-project".to_string()));
        assert_eq!(data.source, Some("quickstart".to_string()));
    }

    #[test]
    fn test_read_project_ini_source_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let iii_dir = dir.path().join(".iii");
        std::fs::create_dir_all(&iii_dir).unwrap();
        std::fs::write(
            iii_dir.join("project.ini"),
            "project_id=abc-123\nproject_name=my-project\n",
        )
        .unwrap();

        let data = read_project_ini(dir.path()).unwrap();
        assert_eq!(data.source, None);
    }

    #[test]
    fn test_read_project_ini_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_project_ini(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_read_project_ini_empty_values_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let iii_dir = dir.path().join(".iii");
        std::fs::create_dir_all(&iii_dir).unwrap();
        std::fs::write(iii_dir.join("project.ini"), "[project]\n").unwrap();

        let result = read_project_ini(dir.path());
        assert!(result.is_none());
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
    // build_user_properties (flat schema)
    // =========================================================================

    #[test]
    #[serial]
    fn test_build_user_properties_flat_environment_keys() {
        unsafe {
            env::remove_var("III_PROJECT_ID");
            env::remove_var("III_PROJECT_ROOT");
            env::remove_var("III_ENV");
        }

        let ctx = TelemetryContext {
            device_id: "id-1".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "test-machine".to_string(),
                iii_execution_context: "user".to_string(),
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
                host_user_id: None,
            },
        };

        let props = ctx.build_user_properties(None);

        assert_eq!(props["environment.os"], "linux");
        assert_eq!(props["environment.arch"], "x86_64");
        assert_eq!(props["environment.cpu_cores"], 4);
        assert_eq!(props["environment.timezone"], "UTC");
        assert_eq!(props["environment.machine_id"], "test-machine");
        assert_eq!(props["iii_execution_context"], "user");
        assert_eq!(props["iii_version"], env!("CARGO_PKG_VERSION"));
        assert!(props.get("env").is_some());
        assert!(props.get("install_method").is_some());
        assert!(
            props.get("device_type").is_none(),
            "device_type should be removed"
        );
        assert!(
            props.get("environment").is_none(),
            "nested environment object should be removed"
        );
    }

    #[test]
    #[serial]
    fn test_build_user_properties_no_project_id_when_unset() {
        unsafe {
            env::remove_var("III_PROJECT_ID");
            env::remove_var("III_PROJECT_ROOT");
        }

        let ctx = TelemetryContext {
            device_id: "id-1".to_string(),
            env_info: make_env_info(),
        };

        let props = ctx.build_user_properties(None);
        assert!(props.get("project_id").is_none());
    }

    #[test]
    #[serial]
    fn test_build_user_properties_with_project_id_env() {
        unsafe {
            env::set_var("III_PROJECT_ID", "proj-abc");
            env::remove_var("III_PROJECT_ROOT");
        }

        let ctx = TelemetryContext {
            device_id: "id-1".to_string(),
            env_info: make_env_info(),
        };

        let props = ctx.build_user_properties(None);
        assert_eq!(props["project_id"], "proj-abc");

        unsafe {
            env::remove_var("III_PROJECT_ID");
        }
    }

    #[test]
    #[serial]
    fn test_build_user_properties_with_sdk_telemetry_project_name() {
        unsafe {
            env::remove_var("III_PROJECT_ID");
            env::remove_var("III_PROJECT_ROOT");
        }

        let ctx = TelemetryContext {
            device_id: "id-1".to_string(),
            env_info: make_env_info(),
        };

        let telemetry = WorkerConnectionTelemetryMeta {
            language: Some("python".to_string()),
            project_name: Some("my-project".to_string()),
            framework: Some("fastapi".to_string()),
        };

        let props = ctx.build_user_properties(Some(&telemetry));
        assert_eq!(props["project_name"], "my-project");
    }

    // =========================================================================
    // AmplitudeEvent serialization (via TelemetryContext::build_event)
    // =========================================================================

    #[test]
    fn test_build_event_basic_fields() {
        let ctx = TelemetryContext {
            device_id: "test-install-id".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "abc123".to_string(),
                iii_execution_context: "user".to_string(),
                timezone: "UTC".to_string(),
                cpu_cores: 4,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
                host_user_id: None,
            },
        };

        let event = ctx.build_event("test_event", serde_json::json!({"key": "value"}), None);

        assert_eq!(event.device_id, "test-install-id");
        assert_eq!(event.user_id, None);
        assert_eq!(event.event_type, "test_event");
        assert_eq!(event.event_properties["key"], "value");
        assert_eq!(event.platform, "iii-engine");
        assert_eq!(event.os_name, std::env::consts::OS);
        assert!(event.insert_id.is_some());
        assert_eq!(event.ip, Some("$remote".to_string()));
        assert!(event.time > 0);
    }

    #[test]
    fn test_build_event_with_sdk_telemetry_language() {
        let ctx = TelemetryContext {
            device_id: "id-1".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                iii_execution_context: "user".to_string(),
                timezone: "UTC".to_string(),
                cpu_cores: 2,
                os: "macos".to_string(),
                arch: "aarch64".to_string(),
                host_user_id: None,
            },
        };

        let telemetry = WorkerConnectionTelemetryMeta {
            language: Some("typescript".to_string()),
            project_name: None,
            framework: None,
        };

        let event = ctx.build_event("evt", serde_json::json!({}), Some(&telemetry));
        assert_eq!(event.language, Some("typescript".to_string()));
    }

    #[test]
    fn test_build_event_insert_id_is_unique() {
        let ctx = TelemetryContext {
            device_id: "id-1".to_string(),
            env_info: make_env_info(),
        };

        let event1 = ctx.build_event("evt", serde_json::json!({}), None);
        let event2 = ctx.build_event("evt", serde_json::json!({}), None);
        assert_ne!(
            event1.insert_id, event2.insert_id,
            "each event should have a unique insert_id"
        );
    }

    #[test]
    fn test_build_event_app_version_matches_cargo_pkg() {
        let ctx = TelemetryContext {
            device_id: "id-test".to_string(),
            env_info: make_env_info(),
        };

        let event = ctx.build_event("evt", serde_json::json!({}), None);
        assert_eq!(event.app_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_build_event_country_is_none() {
        let ctx = TelemetryContext {
            device_id: "id-test".to_string(),
            env_info: make_env_info(),
        };

        let event = ctx.build_event("evt", serde_json::json!({}), None);
        assert!(event.country.is_none());
    }

    #[test]
    fn test_build_event_user_properties_is_some() {
        let ctx = TelemetryContext {
            device_id: "id-test".to_string(),
            env_info: make_env_info(),
        };

        let event = ctx.build_event("evt", serde_json::json!({}), None);
        assert!(event.user_properties.is_some());
    }

    #[test]
    #[serial]
    fn test_build_event_without_sdk_telemetry_language_falls_back() {
        unsafe {
            env::remove_var("LANG");
            env::remove_var("LC_ALL");
        }

        let ctx = TelemetryContext {
            device_id: "id-test".to_string(),
            env_info: make_env_info(),
        };

        let event = ctx.build_event("evt", serde_json::json!({}), None);
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
            device_id: "id-test".to_string(),
            env_info: make_env_info(),
        };

        let event = ctx.build_event("evt", serde_json::json!({}), None);
        assert_eq!(event.language, Some("en_US".to_string()));

        unsafe {
            env::remove_var("LANG");
        }
    }

    #[test]
    fn test_build_event_timestamp_is_recent() {
        let ctx = TelemetryContext {
            device_id: "id-test".to_string(),
            env_info: make_env_info(),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let event = ctx.build_event("evt", serde_json::json!({}), None);
        assert!((event.time - now_ms).abs() < 5000);
    }

    // =========================================================================
    // TelemetryContext clone
    // =========================================================================

    #[test]
    fn test_telemetry_context_clone() {
        let ctx = TelemetryContext {
            device_id: "clone-test-id".to_string(),
            env_info: EnvironmentInfo {
                machine_id: "m1".to_string(),
                iii_execution_context: "docker".to_string(),
                timezone: "America/Chicago".to_string(),
                cpu_cores: 16,
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
                host_user_id: None,
            },
        };

        let cloned = ctx.clone();
        assert_eq!(cloned.device_id, ctx.device_id);
        assert_eq!(cloned.env_info.machine_id, ctx.env_info.machine_id);
        assert_eq!(
            cloned.env_info.iii_execution_context,
            ctx.env_info.iii_execution_context
        );
        assert_eq!(cloned.env_info.timezone, ctx.env_info.timezone);
        assert_eq!(cloned.env_info.cpu_cores, ctx.env_info.cpu_cores);
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

        assert_eq!(result.function_count, 0);
        assert_eq!(result.trigger_count, 0);
        assert!(result.functions.is_empty());
        assert!(result.trigger_types.is_empty());
    }

    #[test]
    fn test_collect_functions_and_triggers_filters_engine_and_iii_prefixes() {
        let engine = make_test_engine();

        let handler: Arc<crate::function::HandlerFn> = Arc::new(|_inv_id, _input, _session| {
            Box::pin(async { crate::function::FunctionResult::NoResult })
        });

        for id in &[
            "engine::internal_fn",
            "iii::durable::publish",
            "iii::queue::redrive",
        ] {
            engine.functions.register_function(
                id.to_string(),
                crate::function::Function {
                    handler: handler.clone(),
                    _function_id: id.to_string(),
                    _description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
            );
        }

        engine.functions.register_function(
            "user::my_function".to_string(),
            crate::function::Function {
                handler: handler.clone(),
                _function_id: "user::my_function".to_string(),
                _description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
        );

        engine.functions.register_function(
            "math::add".to_string(),
            crate::function::Function {
                handler,
                _function_id: "math::add".to_string(),
                _description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
        );

        let result = collect_functions_and_triggers(&engine);
        assert_eq!(result.function_count, 2);
        let mut fns = result.functions.clone();
        fns.sort();
        assert_eq!(fns, vec!["math::add", "user::my_function"]);
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
                metadata: None,
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
                metadata: None,
            },
        );

        let result = collect_functions_and_triggers(&engine);
        assert_eq!(result.trigger_count, 2);

        assert!(result.trigger_types.contains(&"cron".to_string()));
        assert!(result.trigger_types.contains(&"http".to_string()));
    }

    // =========================================================================
    // collect_worker_data
    // =========================================================================

    #[test]
    fn test_collect_worker_data_empty_engine() {
        let engine = make_test_engine();
        let wd = collect_worker_data(&engine);

        assert_eq!(wd.worker_count_total, 0);
        assert!(wd.worker_count_by_framework.is_empty());
        assert!(wd.sdk_telemetry.is_none());
        assert!(wd.sdk_languages.is_empty());
    }

    #[test]
    fn test_collect_worker_data_with_workers() {
        let engine = make_test_engine();

        let (tx1, _rx1) = tokio::sync::mpsc::channel(1);
        let mut worker1 = crate::worker_connections::WorkerConnection::new(tx1);
        worker1.runtime = Some("node".to_string());
        worker1.telemetry = Some(WorkerConnectionTelemetryMeta {
            language: Some("typescript".to_string()),
            project_name: Some("proj-a".to_string()),
            framework: Some("iii-node".to_string()),
        });
        let w1_id = worker1.id;
        engine.worker_registry.workers.insert(w1_id, worker1);

        let (tx2, _rx2) = tokio::sync::mpsc::channel(1);
        let mut worker2 = crate::worker_connections::WorkerConnection::new(tx2);
        worker2.runtime = Some("python".to_string());
        worker2.telemetry = None;
        let w2_id = worker2.id;
        engine.worker_registry.workers.insert(w2_id, worker2);

        let wd = collect_worker_data(&engine);

        assert_eq!(wd.worker_count_total, 2);
        assert_eq!(wd.worker_count_by_framework.get("iii-node"), Some(&1));

        assert!(wd.sdk_telemetry.is_some());
        let telem = wd.sdk_telemetry.unwrap();
        assert_eq!(telem.language, Some("typescript".to_string()));
        assert_eq!(telem.project_name, Some("proj-a".to_string()));
        assert_eq!(telem.framework, Some("iii-node".to_string()));
    }

    #[test]
    fn test_collect_worker_data_skips_unregistered_workers() {
        let engine = make_test_engine();

        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut worker = crate::worker_connections::WorkerConnection::new(tx);
        worker.runtime = None;
        worker.telemetry = None;
        let wid = worker.id;
        engine.worker_registry.workers.insert(wid, worker);

        let wd = collect_worker_data(&engine);
        assert_eq!(wd.worker_count_total, 0);
        assert!(wd.sdk_languages.is_empty());
        assert!(wd.workers.is_empty());
    }

    #[test]
    fn test_collect_worker_data_picks_smallest_uuid_telemetry() {
        let engine = make_test_engine();

        let (tx1, _rx1) = tokio::sync::mpsc::channel(1);
        let mut worker1 = crate::worker_connections::WorkerConnection::new(tx1);
        worker1.id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        worker1.runtime = Some("node".to_string());
        worker1.telemetry = Some(WorkerConnectionTelemetryMeta {
            language: Some("ts".to_string()),
            project_name: Some("proj-smallest".to_string()),
            framework: None,
        });
        engine.worker_registry.workers.insert(worker1.id, worker1);

        let (tx2, _rx2) = tokio::sync::mpsc::channel(1);
        let mut worker2 = crate::worker_connections::WorkerConnection::new(tx2);
        worker2.id = uuid::Uuid::parse_str("ffffffff-ffff-ffff-ffff-ffffffffffff").unwrap();
        worker2.runtime = Some("node".to_string());
        worker2.telemetry = Some(WorkerConnectionTelemetryMeta {
            language: Some("py".to_string()),
            project_name: Some("proj-largest".to_string()),
            framework: None,
        });
        engine.worker_registry.workers.insert(worker2.id, worker2);

        let wd = collect_worker_data(&engine);
        let telem = wd.sdk_telemetry.unwrap();
        assert_eq!(telem.project_name, Some("proj-smallest".to_string()));
    }

    #[test]
    fn test_collect_worker_data_skips_telemetry_with_all_none() {
        let engine = make_test_engine();

        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut worker = crate::worker_connections::WorkerConnection::new(tx);
        worker.runtime = Some("node".to_string());
        worker.telemetry = Some(WorkerConnectionTelemetryMeta {
            language: None,
            project_name: None,
            framework: None,
        });
        let wid = worker.id;
        engine.worker_registry.workers.insert(wid, worker);

        let wd = collect_worker_data(&engine);
        assert!(wd.sdk_telemetry.is_none());
    }

    #[test]
    fn test_collect_worker_data_motia_framework_counted() {
        let engine = make_test_engine();

        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut worker = crate::worker_connections::WorkerConnection::new(tx);
        worker.runtime = Some("node".to_string());
        worker.telemetry = Some(WorkerConnectionTelemetryMeta {
            language: Some("typescript".to_string()),
            project_name: None,
            framework: Some("motia".to_string()),
        });
        engine.worker_registry.workers.insert(worker.id, worker);

        let wd = collect_worker_data(&engine);
        assert_eq!(wd.worker_count_total, 1);
        assert_eq!(wd.worker_count_by_framework.get("motia"), Some(&1));
    }

    #[test]
    fn test_is_iii_builtin_function_id() {
        assert!(is_iii_builtin_function_id("engine::x"));
        assert!(is_iii_builtin_function_id("state::get"));
        assert!(is_iii_builtin_function_id("stream::list"));
        assert!(is_iii_builtin_function_id("iii::durable::publish"));
        assert!(is_iii_builtin_function_id("publish"));
        assert!(is_iii_builtin_function_id("bridge.invoke"));
        assert!(is_iii_builtin_function_id("iii::queue::redrive"));
        assert!(!is_iii_builtin_function_id("orders::process"));
        assert!(!is_iii_builtin_function_id("user::my_function"));
        assert!(!is_iii_builtin_function_id("payments::charge"));
    }

    // =========================================================================
    // DisabledTelemetryWorker
    // =========================================================================

    #[tokio::test]
    async fn test_disabled_telemetry_module_name() {
        let module = DisabledTelemetryWorker;
        assert_eq!(module.name(), "Telemetry");
    }

    #[tokio::test]
    async fn test_disabled_telemetry_module_initialize() {
        let module = DisabledTelemetryWorker;
        let result = module.initialize().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_disabled_telemetry_module_start_background_tasks() {
        let module = DisabledTelemetryWorker;
        let (tx, rx) = tokio::sync::watch::channel(false);
        let result = module.start_background_tasks(rx, tx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_disabled_telemetry_module_destroy() {
        let module = DisabledTelemetryWorker;
        let result = module.destroy().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_disabled_telemetry_module_create() {
        let engine = make_test_engine();
        let result = DisabledTelemetryWorker::create(engine, None).await;
        assert!(result.is_ok());
        let module = result.unwrap();
        assert_eq!(module.name(), "Telemetry");
    }

    // =========================================================================
    // TelemetryWorker::create
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
        let module = TelemetryWorker::create(engine, Some(config)).await.unwrap();
        assert_eq!(module.name(), "Telemetry");
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
        let module = TelemetryWorker::create(engine, None).await.unwrap();
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
        let module = TelemetryWorker::create(engine, None).await.unwrap();
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
        let module = TelemetryWorker::create(engine, None).await.unwrap();
        assert_eq!(module.name(), "Telemetry");

        unsafe {
            env::remove_var("III_TELEMETRY_DEV");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_create_enabled_by_default() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let engine = make_test_engine();
        let module = TelemetryWorker::create(engine, None).await.unwrap();
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
            "sdk_api_key": "sdk-key-456",
        });
        let module = TelemetryWorker::create(engine, Some(config)).await.unwrap();
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
            "sdk_api_key": "",
        });
        let module = TelemetryWorker::create(engine, Some(config)).await.unwrap();
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
        assert!(config.enabled);
        assert!(config.sdk_api_key.is_none());
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
    // get_or_create_device_id
    // =========================================================================

    #[test]
    fn test_get_or_create_device_id_returns_nonempty_string() {
        let id = get_or_create_device_id();
        assert!(!id.is_empty());
    }

    #[test]
    fn test_get_or_create_device_id_is_stable() {
        let id1 = get_or_create_device_id();
        let id2 = get_or_create_device_id();
        assert_eq!(id1, id2, "device_id should be stable across calls");
    }

    // =========================================================================
    // DisableReason enum
    // =========================================================================

    #[test]
    fn test_disable_reason_variants_exist() {
        let _config = DisableReason::Config;
        let _user = DisableReason::UserOptOut;
        let _ci = DisableReason::CiDetected;
        let _dev = DisableReason::DevOptOut;
    }

    // =========================================================================
    // TelemetryWorker::active_client
    // =========================================================================

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

    // =========================================================================
    // TelemetryWorker name
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
        let module = TelemetryWorker::create(engine, None).await.unwrap();
        assert_eq!(module.name(), "Telemetry");
    }

    #[tokio::test]
    #[serial]
    async fn test_telemetry_module_initialize_is_ok() {
        clear_ci_env_vars();
        unsafe {
            env::remove_var("III_TELEMETRY_ENABLED");
            env::remove_var("III_TELEMETRY_DEV");
        }

        let engine = make_test_engine();
        let module = TelemetryWorker::create(engine, None).await.unwrap();
        assert!(module.initialize().await.is_ok());
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
        crate::workers::observability::metrics::ensure_default_meter();

        let engine = make_test_engine();
        register_test_function(&engine, "svc::worker");

        engine
            .trigger_registry
            .register_trigger_type(TriggerType::new(
                "durable:subscriber",
                "Queue",
                Box::new(NoopRegistrator),
                None,
            ))
            .await
            .expect("register trigger type");
        engine
            .trigger_registry
            .register_trigger(Trigger {
                id: "queue-trigger-1".to_string(),
                trigger_type: "durable:subscriber".to_string(),
                function_id: "svc::worker".to_string(),
                config: serde_json::json!({ "topic": "orders" }),
                worker_id: None,
                metadata: None,
            })
            .await
            .expect("register trigger");

        let (worker_tx, _worker_rx) = mpsc::channel(1);
        let mut worker = WorkerConnection::new(worker_tx);
        worker.runtime = Some("node".to_string());
        worker.telemetry = Some(WorkerConnectionTelemetryMeta {
            language: Some("typescript".to_string()),
            project_name: Some("telemetry-spec".to_string()),
            framework: Some("iii-node".to_string()),
        });
        engine.worker_registry.register_worker(worker);

        let acc = get_metrics_accumulator();
        acc.invocations_total.store(12, Ordering::Relaxed);
        acc.invocations_success.store(9, Ordering::Relaxed);
        acc.invocations_error.store(3, Ordering::Relaxed);
        acc.workers_spawns.store(4, Ordering::Relaxed);
        acc.workers_deaths.store(1, Ordering::Relaxed);
        acc.invocations_by_function
            .insert("svc::worker".to_string(), 12);

        let telemetry = collector();
        telemetry.queue_emits.store(7, Ordering::Relaxed);
        telemetry.api_requests.store(5, Ordering::Relaxed);
        telemetry.function_registrations.store(1, Ordering::Relaxed);
        telemetry.trigger_registrations.store(1, Ordering::Relaxed);

        let module = build_manual_module(engine, true, 1);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        module
            .start_background_tasks(shutdown_rx, shutdown_tx.clone())
            .await
            .expect("start background tasks");

        tokio::time::sleep(Duration::from_millis(200)).await;
        shutdown_tx.send(true).expect("signal shutdown");
        tokio::time::sleep(Duration::from_millis(200)).await;
        module.destroy().await.expect("destroy telemetry module");

        reset_telemetry_globals();
    }

    // =========================================================================
    // build_template_lifecycle_properties
    // =========================================================================

    #[test]
    fn test_template_success_properties() {
        let project = ProjectContext {
            project_id: Some("proj-123".to_string()),
            project_name: Some("my-project".to_string()),
            source: Some("quickstart".to_string()),
        };

        let (event_type, props) = build_template_lifecycle_properties(
            "template_success",
            "math::add",
            "quickstart",
            &project,
        );

        assert_eq!(event_type, "template_success");
        assert_eq!(props["function_id"], "math::add");
        assert_eq!(props["source"], "quickstart");
        assert_eq!(props["project_id"], "proj-123");
        assert_eq!(props["project_name"], "my-project");
    }

    #[test]
    fn test_template_failure_properties() {
        let project = ProjectContext {
            project_id: Some("proj-456".to_string()),
            project_name: Some("other".to_string()),
            source: Some("quickstart".to_string()),
        };

        let (event_type, props) = build_template_lifecycle_properties(
            "template_failure",
            "math::divide",
            "quickstart",
            &project,
        );

        assert_eq!(event_type, "template_failure");
        assert_eq!(props["function_id"], "math::divide");
        assert_eq!(props["source"], "quickstart");
        assert_eq!(props["project_id"], "proj-456");
    }

    #[test]
    fn test_template_properties_with_custom_source() {
        let project = ProjectContext {
            project_id: Some("proj-789".to_string()),
            project_name: None,
            source: Some("multi-worker-orchestration".to_string()),
        };

        let (_, props) = build_template_lifecycle_properties(
            "template_success",
            "orders::process",
            "multi-worker-orchestration",
            &project,
        );

        assert_eq!(props["source"], "multi-worker-orchestration");
        assert_eq!(props["function_id"], "orders::process");
        assert!(
            props.get("project_name").is_none(),
            "None project_name should be omitted"
        );
    }

    #[test]
    fn test_template_properties_no_project_id() {
        let project = ProjectContext {
            project_id: None,
            project_name: None,
            source: Some("quickstart".to_string()),
        };

        let (_, props) = build_template_lifecycle_properties(
            "template_success",
            "math::add",
            "quickstart",
            &project,
        );

        assert!(props.get("project_id").is_none());
        assert!(props.get("project_name").is_none());
        assert_eq!(props["function_id"], "math::add");
        assert_eq!(props["source"], "quickstart");
    }

    #[test]
    fn test_template_success_event_construction() {
        let ctx = TelemetryContext {
            device_id: "test-device".to_string(),
            env_info: make_env_info(),
        };

        let project = ProjectContext {
            project_id: Some("proj-1".to_string()),
            project_name: Some("test-proj".to_string()),
            source: Some("quickstart".to_string()),
        };

        let (event_type, props) = build_template_lifecycle_properties(
            "template_success",
            "math::add",
            "quickstart",
            &project,
        );

        let event = ctx.build_event(&event_type, props, None);
        assert_eq!(event.event_type, "template_success");
        assert_eq!(event.device_id, "test-device");
        assert_eq!(event.platform, "iii-engine");
        assert_eq!(event.event_properties["function_id"], "math::add");
        assert_eq!(event.event_properties["source"], "quickstart");
    }

    #[test]
    fn test_template_failure_event_construction() {
        let ctx = TelemetryContext {
            device_id: "test-device".to_string(),
            env_info: make_env_info(),
        };

        let project = ProjectContext {
            project_id: Some("proj-1".to_string()),
            project_name: Some("test-proj".to_string()),
            source: Some("quickstart".to_string()),
        };

        let (event_type, props) = build_template_lifecycle_properties(
            "template_failure",
            "math::divide",
            "quickstart",
            &project,
        );

        let event = ctx.build_event(&event_type, props, None);
        assert_eq!(event.event_type, "template_failure");
        assert_eq!(event.event_properties["function_id"], "math::divide");
    }

    #[test]
    fn test_template_constants() {
        assert_eq!(TEMPLATE_POLL_INTERVAL_SECS, 3);
        assert_eq!(TEMPLATE_POLL_TIMEOUT_SECS, 60 * 60);
    }
}
