// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{pin::Pin, sync::Arc};

use dashmap::DashMap;
use function_macros::{function, service};
use futures::Future;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    builtins::kv::BuiltinKvStore,
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::module::Module,
    protocol::{ErrorBody, WorkerMetrics},
    trigger::{Trigger, TriggerRegistrator, TriggerType},
    workers::{Worker, WorkerMetrics},
};

pub const TRIGGER_FUNCTIONS_AVAILABLE: &str = "engine::functions-available";
pub const TRIGGER_WORKERS_AVAILABLE: &str = "engine::workers-available";
pub const TRIGGER_WORKER_METRICS: &str = "subscribe";
pub const TOPIC_WORKER_METRICS: &str = "iii.worker.metrics";
pub const KV_INDEX_WORKER_METRICS: &str = "worker_metrics";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmptyInput {}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct WorkersListInput {
    pub worker_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionInfo {
    pub function_path: String,
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriggerInfo {
    pub id: String,
    pub trigger_type: String,
    pub function_path: String,
    pub config: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerInfo {
    pub id: String,
    pub name: Option<String>,
    pub runtime: Option<String>,
    pub version: Option<String>,
    pub os: Option<String>,
    pub ip_address: Option<String>,
    pub status: String,
    pub connected_at_ms: u64,
    pub function_count: usize,
    pub functions: Vec<String>,
    pub active_invocations: usize,
    pub latest_metrics: Option<WorkerMetrics>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterWorkerInput {
    /// Worker ID injected by engine from caller context
    #[serde(rename = "_caller_worker_id")]
    pub worker_id: String,
    pub runtime: Option<String>,
    pub version: Option<String>,
    pub name: Option<String>,
    pub os: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReportMetricsInput {
    /// Worker ID injected by engine from caller context
    #[serde(rename = "_caller_worker_id")]
    pub worker_id: String,
    /// The metrics payload
    #[serde(flatten)]
    pub metrics: WorkerMetrics,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetMetricsInput {
    /// Optional worker ID to get metrics for a specific worker
    pub worker_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerMetricsInfo {
    pub worker_id: String,
    pub worker_name: Option<String>,
    pub metrics: WorkerMetrics,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkerMetricsEvent {
    pub worker_id: String,
    pub worker_name: Option<String>,
    pub metrics: WorkerMetrics,
    pub timestamp: Option<u64>,
}

#[derive(Clone)]
pub struct WorkerModule {
    engine: Arc<Engine>,
    triggers: Arc<DashMap<String, Trigger>>,
    kv_store: Arc<BuiltinKvStore>,
}

impl WorkerModule {
    pub fn new(engine: Arc<Engine>) -> Self {
        let kv_store = Arc::new(BuiltinKvStore::new(None));
        Self {
            engine,
            triggers: Arc::new(DashMap::new()),
            kv_store,
        }
    }

    pub async fn fire_triggers(&self, trigger_type: &str, data: Value) {
        let triggers_to_fire: Vec<Trigger> = self
            .triggers
            .iter()
            .filter(|entry| entry.value().trigger_type == trigger_type)
            .map(|entry| entry.value().clone())
            .collect();

        for trigger in triggers_to_fire {
            let engine = self.engine.clone();
            let function_path = trigger.function_path.clone();
            let data = data.clone();
            tokio::spawn(async move {
                let _ = engine.invoke_function(&function_path, data).await;
            });
        }
    }

    fn list_functions(&self) -> Vec<FunctionInfo> {
        self.engine
            .functions
            .iter()
            .map(|entry| {
                let f = entry.value();
                FunctionInfo {
                    function_path: f._function_path.clone(),
                    description: f._description.clone(),
                    request_format: f.request_format.clone(),
                    response_format: f.response_format.clone(),
                    metadata: f.metadata.clone(),
                }
            })
            .collect()
    }

    async fn list_trigger_infos(&self) -> Vec<TriggerInfo> {
        self.engine
            .trigger_registry
            .triggers
            .iter()
            .map(|entry| {
                let t = entry.value();
                TriggerInfo {
                    id: t.id.clone(),
                    trigger_type: t.trigger_type.clone(),
                    function_path: t.function_path.clone(),
                    config: t.config.clone(),
                }
            })
            .collect()
    }

    async fn list_worker_infos(&self, filter_worker_id: Option<&str>) -> Vec<WorkerInfo> {
        use crate::modules::observability::metrics::get_worker_metrics_from_storage;

        let workers = self.engine.worker_registry.list_workers();
        let mut worker_infos = Vec::with_capacity(workers.len());

        for w in workers {
            let worker_id = w.id.to_string();

            // Apply worker_id filter if provided
            if let Some(filter_id) = filter_worker_id
                && worker_id != filter_id
            {
                continue;
            }

            let functions = w.get_function_paths().await;
            let function_count = functions.len();
            let active_invocations = w.invocation_count().await;
            // Query latest metrics from OTEL storage
            let latest_metrics = get_worker_metrics_from_storage(&worker_id);

            worker_infos.push(WorkerInfo {
                id: worker_id,
                name: w.name.clone(),
                runtime: w.runtime.clone(),
                version: w.version.clone(),
                os: w.os.clone(),
                ip_address: w.ip_address.clone(),
                status: w.status.as_str().to_string(),
                connected_at_ms: w.connected_at.timestamp_millis() as u64,
                function_count,
                functions,
                active_invocations,
                latest_metrics,
            });
        }
        worker_infos
    }

    async fn register_worker_metadata(&self, input: RegisterWorkerInput) {
        let worker_id = match uuid::Uuid::parse_str(&input.worker_id) {
            Ok(id) => id,
            Err(_) => {
                tracing::error!(worker_id = %input.worker_id, "Invalid worker_id format");
                return;
            }
        };

        let runtime = input.runtime.unwrap_or_else(|| "unknown".to_string());

        self.engine.worker_registry.update_worker_metadata(
            &worker_id,
            runtime,
            input.version,
            input.name,
            input.os,
        );
    }
}

impl TriggerRegistrator for WorkerModule {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let triggers = self.triggers.clone();
        Box::pin(async move {
            tracing::debug!(
                trigger_id = %trigger.id,
                trigger_type = %trigger.trigger_type,
                function_path = %trigger.function_path,
                "Registering engine trigger"
            );
            triggers.insert(trigger.id.clone(), trigger);
            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let triggers = self.triggers.clone();
        Box::pin(async move {
            tracing::debug!(trigger_id = %trigger.id, "Unregistering engine trigger");
            triggers.remove(&trigger.id);
            Ok(())
        })
    }
}

#[async_trait::async_trait]
impl Module for WorkerModule {
    fn name(&self) -> &'static str {
        "WorkerModule"
    }

    async fn create(
        engine: Arc<Engine>,
        _config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Module>> {
        Ok(Box::new(WorkerModule::new(engine)))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing WorkerModule");

        let functions_trigger = TriggerType {
            id: TRIGGER_FUNCTIONS_AVAILABLE.to_string(),
            _description: "Triggered when functions are registered/unregistered".to_string(),
            registrator: Box::new(self.clone()),
            worker_id: None,
        };
        let _ = self.engine.register_trigger_type(functions_trigger).await;

        let workers_trigger = TriggerType {
            id: TRIGGER_WORKERS_AVAILABLE.to_string(),
            _description: "Triggered when workers connect/disconnect".to_string(),
            registrator: Box::new(self.clone()),
            worker_id: None,
        };
        let _ = self.engine.register_trigger_type(workers_trigger).await;

        let metrics_trigger = Trigger {
            id: "engine.workers.metrics.subscriber".to_string(),
            trigger_type: TRIGGER_WORKER_METRICS.to_string(),
            function_path: "engine.workers.metrics._on_publish".to_string(),
            config: serde_json::json!({
                "topic": TOPIC_WORKER_METRICS
            }),
            worker_id: None,
        };

        if let Err(err) = self
            .engine
            .trigger_registry
            .register_trigger(metrics_trigger)
            .await
        {
            tracing::warn!(error = %err, "Failed to register worker metrics PubSub trigger");
        }

        Ok(())
    }

    async fn start_background_tasks(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let engine = self.engine.clone();
        let triggers = self.triggers.clone();
        let worker_module = self.clone();
        let duration_secs = 5u64;

        tokio::spawn(async move {
            let mut current_functions_hash = engine.functions.functions_hash();

            loop {
                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(duration_secs)) => {
                        let new_functions_hash = engine.functions.functions_hash();
                        if new_functions_hash != current_functions_hash {
                            tracing::info!("New functions detected, firing functions-available trigger");
                            current_functions_hash = new_functions_hash;

                            let functions = worker_module.list_functions();

                            let functions_data = serde_json::json!({
                                "event": "functions_changed",
                                "functions": functions,
                            });

                            // Fire triggers directly from this module
                            let triggers_to_fire: Vec<Trigger> = triggers
                                .iter()
                                .filter(|entry| entry.value().trigger_type == TRIGGER_FUNCTIONS_AVAILABLE)
                                .map(|entry| entry.value().clone())
                                .collect();

                            for trigger in triggers_to_fire {
                                let engine = engine.clone();
                                let function_path = trigger.function_path.clone();
                                let data = functions_data.clone();
                                tokio::spawn(async move {
                                    let _ = engine.invoke_function(&function_path, data).await;
                                });
                            }
                        }
                    }
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            tracing::info!("WorkerModule background tasks shutting down");
                            break;
                        }
                    }
                }
            }
        });

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[service(name = "engine")]
impl WorkerModule {
    #[function(name = "engine.functions.list", description = "List all functions")]
    pub async fn get_functions(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let functions = self.list_functions();
        FunctionResult::Success(Some(serde_json::json!({ "functions": functions })))
    }

    #[function(
        name = "engine.workers.list",
        description = "List all workers with metrics"
    )]
    pub async fn get_workers(
        &self,
        input: WorkersListInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let workers = self.list_worker_infos(input.worker_id.as_deref()).await;
        FunctionResult::Success(Some(serde_json::json!({
            "workers": workers,
            "timestamp": chrono::Utc::now().timestamp_millis(),
        })))
    }

    #[function(name = "engine.triggers.list", description = "List all triggers")]
    pub async fn get_triggers(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let triggers = self.list_trigger_infos().await;
        FunctionResult::Success(Some(serde_json::json!({ "triggers": triggers })))
    }

    #[function(
        name = "engine.workers.register",
        description = "Register worker metadata"
    )]
    pub async fn register_worker(
        &self,
        input: RegisterWorkerInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let worker_id = input.worker_id.clone();
        self.register_worker_metadata(input).await;

        let data = serde_json::json!({
            "event": "worker_metadata_updated",
            "worker_id": worker_id,
        });
        self.engine
            .fire_triggers(TRIGGER_WORKERS_AVAILABLE, data)
            .await;

        FunctionResult::Success(Some(serde_json::json!({"success": true})))
    }

    #[function(
        name = "engine.workers.report_metrics",
        description = "Report worker metrics (CPU, memory, K8s stats, etc.)"
    )]
    pub async fn report_metrics(
        &self,
        input: ReportMetricsInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let worker_id = match uuid::Uuid::parse_str(&input.worker_id) {
            Ok(id) => id,
            Err(_) => {
                return FunctionResult::Failure(ErrorBody {
                    code: "invalid_worker_id".into(),
                    message: format!("Invalid worker_id format: {}", input.worker_id),
                });
            }
        };

        let metrics_value = serde_json::to_value(&input.metrics).unwrap_or(Value::Null);
        self.kv_store
            .set(
                KV_INDEX_WORKER_METRICS.to_string(),
                input.worker_id.clone(),
                metrics_value,
            )
            .await;

        self.engine
            .worker_registry
            .update_worker_metrics(&worker_id, input.metrics);

        tracing::debug!(worker_id = %worker_id, "Worker metrics updated");

        FunctionResult::Success(Some(serde_json::json!({"success": true})))
    }

    #[function(
        name = "engine.workers.metrics._on_publish",
        description = "Internal handler for worker metrics PubSub events"
    )]
    pub async fn on_metrics_publish(
        &self,
        input: WorkerMetricsEvent,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let worker_id = match uuid::Uuid::parse_str(&input.worker_id) {
            Ok(id) => id,
            Err(_) => {
                return FunctionResult::Failure(ErrorBody {
                    code: "invalid_worker_id".into(),
                    message: format!("Invalid worker_id format: {}", input.worker_id),
                });
            }
        };

        let metrics_value = serde_json::to_value(&input.metrics).unwrap_or(Value::Null);
        self.kv_store
            .set(
                KV_INDEX_WORKER_METRICS.to_string(),
                input.worker_id.clone(),
                metrics_value,
            )
            .await;

        self.engine
            .worker_registry
            .update_worker_metrics(&worker_id, input.metrics);

        tracing::debug!(worker_id = %input.worker_id, "Worker metrics saved");

        FunctionResult::Success(None)
    }

    #[function(
        name = "engine.workers.metrics.list",
        description = "List all worker metrics from persistent storage"
    )]
    pub async fn list_worker_metrics_kv(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let all_items = self
            .kv_store
            .list(KV_INDEX_WORKER_METRICS.to_string())
            .await;

        FunctionResult::Success(Some(serde_json::json!({
            "metrics": all_items
        })))
    }

    #[function(
        name = "engine.workers.get_metrics",
        description = "Get worker metrics (optionally for a specific worker)"
    )]
    pub async fn get_metrics(
        &self,
        input: GetMetricsInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        if let Some(worker_id_str) = input.worker_id {
            let worker_id = match uuid::Uuid::parse_str(&worker_id_str) {
                Ok(id) => id,
                Err(_) => {
                    return FunctionResult::Failure(ErrorBody {
                        code: "invalid_worker_id".into(),
                        message: format!("Invalid worker_id format: {}", worker_id_str),
                    });
                }
            };

            let worker: Option<Worker> = self.engine.worker_registry.get_worker(&worker_id);
            let metrics = self.engine.worker_registry.get_worker_metrics(&worker_id);

            match metrics {
                Some(m) => FunctionResult::Success(Some(serde_json::json!({
                    "worker_id": worker_id_str,
                    "worker_name": worker.and_then(|w| w.name),
                    "metrics": m
                }))),
                None => FunctionResult::Failure(ErrorBody {
                    code: "not_found".into(),
                    message: format!("No metrics found for worker: {}", worker_id_str),
                }),
            }
        } else {
            let all_metrics = self.engine.worker_registry.get_all_metrics();
            let workers: Vec<Worker> = self.engine.worker_registry.list_workers();

            let metrics_list: Vec<WorkerMetricsInfo> = all_metrics
                .into_iter()
                .map(|(worker_id, metrics)| {
                    let worker_name = workers
                        .iter()
                        .find(|w| w.id == worker_id)
                        .and_then(|w| w.name.clone());
                    WorkerMetricsInfo {
                        worker_id: worker_id.to_string(),
                        worker_name,
                        metrics,
                    }
                })
                .collect();

            FunctionResult::Success(Some(serde_json::json!({
                "workers": metrics_list
            })))
        }
    }
}

crate::register_module!("modules::worker::WorkerModule", WorkerModule, mandatory);
