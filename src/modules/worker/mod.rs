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
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::module::Module,
    protocol::{ErrorBody, WorkerMetrics},
    trigger::{Trigger, TriggerRegistrator, TriggerType},
    workers::WorkerTelemetryMeta,
};

pub const TRIGGER_FUNCTIONS_AVAILABLE: &str = "engine::functions-available";
pub const TRIGGER_WORKERS_AVAILABLE: &str = "engine::workers-available";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmptyInput {}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct FunctionsListInput {
    /// Include internal engine functions (engine.* prefix). Defaults to false.
    #[serde(default)]
    pub include_internal: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TriggersListInput {
    /// Include internal engine triggers (linked to engine.* functions). Defaults to false.
    #[serde(default)]
    pub include_internal: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct WorkersListInput {
    pub worker_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionInfo {
    pub function_id: String,
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriggerInfo {
    pub id: String,
    pub trigger_type: String,
    pub function_id: String,
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
    #[serde(rename = "_caller_worker_id")]
    pub worker_id: String,
    pub runtime: Option<String>,
    pub version: Option<String>,
    pub name: Option<String>,
    pub os: Option<String>,
    pub telemetry: Option<WorkerTelemetryMeta>,
}

#[derive(Clone)]
pub struct WorkerModule {
    engine: Arc<Engine>,
    triggers: Arc<DashMap<String, Trigger>>,
}

impl WorkerModule {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            triggers: Arc::new(DashMap::new()),
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
            let function_id = trigger.function_id.clone();
            let data = data.clone();
            tokio::spawn(async move {
                let _ = engine.call(&function_id, data).await;
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
                    function_id: f._function_id.clone(),
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
                    function_id: t.function_id.clone(),
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

            let functions = w.get_function_ids().await;
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
            input.telemetry,
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
                function_id = %trigger.function_id,
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
                                let function_id = trigger.function_id.clone();
                                let data = functions_data.clone();
                                tokio::spawn(async move {
                                    let _ = engine.call(&function_id, data).await;
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
    #[function(id = "engine::functions::list", description = "List all functions")]
    pub async fn get_functions(
        &self,
        input: FunctionsListInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let mut functions = self.list_functions();

        if !input.include_internal.unwrap_or(false) {
            functions.retain(|f| !f.function_id.starts_with("engine::"));
        }

        FunctionResult::Success(Some(serde_json::json!({ "functions": functions })))
    }

    #[function(
        id = "engine::workers::list",
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

    #[function(id = "engine::triggers::list", description = "List all triggers")]
    pub async fn get_triggers(
        &self,
        input: TriggersListInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let mut triggers = self.list_trigger_infos().await;

        if !input.include_internal.unwrap_or(false) {
            triggers.retain(|t| !t.function_id.starts_with("engine::"));
        }

        FunctionResult::Success(Some(serde_json::json!({ "triggers": triggers })))
    }

    #[function(
        id = "engine::workers::register",
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
}

crate::register_module!("modules::worker::WorkerModule", WorkerModule, mandatory);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_register_worker_input_deserializes_amplitude_api_key() {
        let json = serde_json::json!({
            "_caller_worker_id": "550e8400-e29b-41d4-a716-446655440000",
            "runtime": "node",
            "version": "1.0.0",
            "name": "host:123",
            "os": "darwin 25.0",
            "telemetry": {
                "language": "en-US",
                "project_name": "my-project",
                "framework": "express",
                "amplitude_api_key": "test-key-123"
            }
        });
        let input: RegisterWorkerInput = serde_json::from_value(json).expect("deserialize");
        assert_eq!(input.worker_id, "550e8400-e29b-41d4-a716-446655440000");
        let telemetry = input.telemetry.expect("telemetry present");
        assert_eq!(telemetry.amplitude_api_key.as_deref(), Some("test-key-123"));
    }
}
