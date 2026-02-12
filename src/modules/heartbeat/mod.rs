// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

mod collector;
mod config;
pub mod lifecycle;
mod store;
mod telemetry;

use std::sync::Arc;

use async_trait::async_trait;
use colored::Colorize;
use function_macros::{function, service};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::module::Module,
    protocol::ErrorBody,
};

use self::collector::Collector;
use self::config::HeartbeatConfig;
use self::lifecycle::{EventKind, get_lifecycle_tracker};
use self::store::HeartbeatStore;

#[derive(Serialize, Deserialize, Default)]
pub struct HeartbeatStatusInput {}

#[derive(Serialize, Deserialize, Default)]
pub struct HeartbeatHistoryInput {
    pub limit: Option<usize>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct HeartbeatEventsInput {}

#[derive(Serialize, Deserialize)]
pub struct HeartbeatReportEventInput {
    pub kind: String,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Clone)]
pub struct HeartbeatModule {
    config: HeartbeatConfig,
    collector: Arc<Collector>,
    store: Arc<HeartbeatStore>,
    bg_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

#[service(name = "heartbeat")]
impl HeartbeatModule {
    #[function(
        id = "engine.heartbeat.status",
        description = "Get the current heartbeat data snapshot"
    )]
    pub async fn status(
        &self,
        _input: HeartbeatStatusInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let entry = self.collector.collect();
        match serde_json::to_value(&entry) {
            Ok(val) => FunctionResult::Success(Some(val)),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to serialize heartbeat: {}", e),
                code: "heartbeat.serialize_error".to_string(),
            }),
        }
    }

    #[function(
        id = "engine.heartbeat.history",
        description = "Get historical heartbeat entries"
    )]
    pub async fn history(
        &self,
        input: HeartbeatHistoryInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let entries = self.store.history(input.limit).await;
        match serde_json::to_value(&entries) {
            Ok(val) => FunctionResult::Success(Some(serde_json::json!({
                "count": entries.len(),
                "entries": val,
            }))),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to serialize history: {}", e),
                code: "heartbeat.serialize_error".to_string(),
            }),
        }
    }

    #[function(
        id = "engine.heartbeat.events",
        description = "Get lifecycle events tracked by the engine"
    )]
    pub async fn events(
        &self,
        _input: HeartbeatEventsInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let tracker = get_lifecycle_tracker();
        let events = tracker.all_events();
        match serde_json::to_value(&events) {
            Ok(val) => FunctionResult::Success(Some(serde_json::json!({
                "count": events.len(),
                "events": val,
            }))),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to serialize lifecycle events: {}", e),
                code: "heartbeat.serialize_error".to_string(),
            }),
        }
    }

    #[function(
        id = "engine.heartbeat.report_event",
        description = "Report a lifecycle event from an SDK (install, init, etc.)"
    )]
    pub async fn report_event(
        &self,
        input: HeartbeatReportEventInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let tracker = get_lifecycle_tracker();
        let kind: EventKind =
            match serde_json::from_value(serde_json::Value::String(input.kind.clone())) {
                Ok(k) => k,
                Err(_) => {
                    return FunctionResult::Failure(ErrorBody {
                        message: format!("Unknown event kind: {}", input.kind),
                        code: "heartbeat.invalid_event_kind".to_string(),
                    });
                }
            };

        match kind {
            EventKind::InstallFailed | EventKind::InstallSuccess | EventKind::ProjectCreated => {}
            _ => {
                return FunctionResult::Failure(ErrorBody {
                    message: format!("Event kind '{}' cannot be reported via SDK", input.kind),
                    code: "heartbeat.invalid_event_kind".to_string(),
                });
            }
        }

        tracker.record_sdk_event(kind, input.metadata);
        FunctionResult::Success(Some(serde_json::json!({"status": "recorded"})))
    }
}

#[async_trait]
impl Module for HeartbeatModule {
    fn name(&self) -> &'static str {
        "HeartbeatModule"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        let hb_config: HeartbeatConfig = match config {
            Some(cfg) => serde_json::from_value(cfg)?,
            None => HeartbeatConfig::default(),
        };

        if let Some(ref endpoint) = hb_config.cloud_endpoint {
            if let Err(e) = telemetry::validate_cloud_endpoint(endpoint) {
                anyhow::bail!("[HEARTBEAT] {}", e);
            }
        }

        let instance_id = hb_config.get_or_create_instance_id();
        let store = Arc::new(HeartbeatStore::new(hb_config.max_history_size()));
        let collector = Arc::new(Collector::new(
            engine.clone(),
            instance_id.clone(),
            Vec::new(),
        ));

        let tracker = get_lifecycle_tracker();
        tracker.set_instance_id(instance_id.clone());

        tracing::info!(
            "{} HeartbeatModule created (instance_id={}, interval={}s)",
            "[HEARTBEAT]".cyan(),
            HeartbeatConfig::instance_id_short(&instance_id),
            hb_config.interval(),
        );

        Ok(Box::new(HeartbeatModule {
            config: hb_config,
            collector,
            store,
            bg_handle: Arc::new(Mutex::new(None)),
        }))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!(
            "{} Heartbeat module initialized (functions: engine.heartbeat.{{status,history,events,report_event}})",
            "[READY]".green()
        );
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        if !self.config.is_enabled() {
            tracing::info!(
                "{} Heartbeat disabled, skipping background tasks",
                "[HEARTBEAT]".cyan()
            );
            return Ok(());
        }

        let interval_secs = self.config.interval();
        let collector = self.collector.clone();
        let store = self.store.clone();
        let cloud_endpoint = self.config.cloud_endpoint.clone();
        let telemetry_enabled = telemetry::check_telemetry_enabled(&self.config);

        let http_client = if telemetry_enabled && cloud_endpoint.is_some() {
            telemetry::build_http_client().map(Arc::new)
        } else {
            None
        };

        let cloud_semaphore = Arc::new(tokio::sync::Semaphore::new(3));

        let handle = tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));

            tracing::debug!(
                "[HEARTBEAT] Background heartbeat task started (interval={}s)",
                interval_secs
            );

            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            tracing::debug!("[HEARTBEAT] Heartbeat task shutting down");
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        let entry = collector.collect();
                        store.push(entry.clone()).await;

                        tracing::debug!(
                            "{} heartbeat collected (uptime={}s, workers={}, functions={}, invocations={})",
                            "[HEARTBEAT]".cyan(),
                            entry.uptime_seconds,
                            entry.runtime.workers_active,
                            entry.registration.functions.count,
                            entry.runtime.invocations_total,
                        );

                        if let Some(ref endpoint) = cloud_endpoint
                            && let Some(ref client) = http_client
                        {
                            let tracker = get_lifecycle_tracker();
                            let lifecycle_events = tracker.take_pending();
                            let cloud_entry = entry.to_cloud_safe();

                            let permit = cloud_semaphore.clone().try_acquire_owned();
                            if let Ok(permit) = permit {
                                tokio::spawn({
                                    let client = client.clone();
                                    let endpoint = endpoint.clone();
                                    async move {
                                        let ok = telemetry::report_to_cloud(&client, &endpoint, &cloud_entry, lifecycle_events.clone()).await;
                                        if !ok && !lifecycle_events.is_empty() {
                                            get_lifecycle_tracker().requeue_pending(lifecycle_events);
                                        }
                                        drop(permit);
                                    }
                                });
                            } else if !lifecycle_events.is_empty() {
                                tracker.requeue_pending(lifecycle_events);
                                tracing::debug!("[HEARTBEAT] Cloud reporting task limit reached, skipping this tick");
                            }
                        }
                    }
                }
            }

            tracing::debug!("[HEARTBEAT] Heartbeat task stopped");
        });

        *self.bg_handle.lock().await = Some(handle);

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        if let Some(handle) = self.bg_handle.lock().await.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                handle.abort();
                let _ = handle.await;
            })
            .await;
        }

        tracing::info!(
            "{} HeartbeatModule shutting down (history_entries={})",
            "[HEARTBEAT]".cyan(),
            self.store.len().await,
        );
        Ok(())
    }
}

crate::register_module!(
    "modules::heartbeat::HeartbeatModule",
    HeartbeatModule,
    enabled_by_default = false
);
