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
    protocol::{ErrorBody, StreamChannelRef, WorkerMetrics},
    trigger::{Trigger, TriggerRegistrator, TriggerType},
    workers::WorkerTelemetryMeta,
};

pub const TRIGGER_FUNCTIONS_AVAILABLE: &str = "engine::functions-available";
pub const TRIGGER_WORKERS_AVAILABLE: &str = "engine::workers-available";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmptyInput {}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CreateChannelInput {
    #[serde(default)]
    pub buffer_size: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CreateChannelOutput {
    pub writer: StreamChannelRef,
    pub reader: StreamChannelRef,
}

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
    #[function(
        id = "engine::channels::create",
        description = "Create a streaming channel pair"
    )]
    pub async fn create_function(
        &self,
        input: CreateChannelInput,
    ) -> FunctionResult<CreateChannelOutput, ErrorBody> {
        // We need to have the worker who called this function as the owner of the channel
        // For now we can't really know what is the caller of this function, we will work
        // On changing functions to know who is calling them.

        let channel_mgr = self.engine.channel_manager.clone();
        let buffer_size = input.buffer_size.unwrap_or(64).min(1024);
        let (writer_ref, reader_ref) = channel_mgr.create_channel(buffer_size, None);

        let result = CreateChannelOutput {
            writer: writer_ref,
            reader: reader_ref,
        };

        FunctionResult::Success(result)
    }

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
    use crate::modules::observability::metrics::ensure_default_meter;
    use crate::modules::observability::metrics::{
        StoredDataPoint, StoredMetric, StoredMetricType, StoredNumberDataPoint, get_metric_storage,
        init_metric_storage,
    };
    use serde_json;

    #[test]
    fn test_register_worker_input_deserializes_telemetry() {
        let json = serde_json::json!({
            "_caller_worker_id": "550e8400-e29b-41d4-a716-446655440000",
            "runtime": "node",
            "version": "1.0.0",
            "name": "host:123",
            "os": "darwin 25.0",
            "telemetry": {
                "language": "en-US",
                "project_name": "my-project",
                "framework": "express"
            }
        });
        let input: RegisterWorkerInput = serde_json::from_value(json).expect("deserialize");
        assert_eq!(input.worker_id, "550e8400-e29b-41d4-a716-446655440000");
        let telemetry = input.telemetry.expect("telemetry present");
        assert_eq!(telemetry.language.as_deref(), Some("en-US"));
        assert_eq!(telemetry.project_name.as_deref(), Some("my-project"));
        assert_eq!(telemetry.framework.as_deref(), Some("express"));
    }

    fn setup_engine_and_module() -> (Arc<Engine>, WorkerModule) {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let module = WorkerModule::new(engine.clone());
        (engine, module)
    }

    // ---- list_functions tests ----

    #[test]
    fn test_list_functions_empty_engine() {
        let (_engine, module) = setup_engine_and_module();
        let functions = module.list_functions();
        assert!(functions.is_empty());
    }

    #[test]
    fn test_list_functions_returns_registered_functions() {
        let (engine, module) = setup_engine_and_module();

        // Register a function via the engine
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::my_func".to_string(),
                description: Some("A test function".to_string()),
                request_format: Some(serde_json::json!({"type": "object"})),
                response_format: None,
                metadata: Some(serde_json::json!({"version": 1})),
            },
            crate::engine::Handler::new(
                |_input: Value| async move { FunctionResult::Success(None) },
            ),
        );

        let functions = module.list_functions();
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].function_id, "test::my_func");
        assert_eq!(functions[0].description.as_deref(), Some("A test function"));
        assert_eq!(
            functions[0].request_format,
            Some(serde_json::json!({"type": "object"}))
        );
        assert!(functions[0].response_format.is_none());
        assert_eq!(
            functions[0].metadata,
            Some(serde_json::json!({"version": 1}))
        );
    }

    #[test]
    fn test_list_functions_returns_multiple_functions() {
        let (engine, module) = setup_engine_and_module();

        for i in 0..3 {
            engine.register_function_handler(
                crate::engine::RegisterFunctionRequest {
                    function_id: format!("test::func_{}", i),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                crate::engine::Handler::new(|_input: Value| async move {
                    FunctionResult::Success(None)
                }),
            );
        }

        let functions = module.list_functions();
        assert_eq!(functions.len(), 3);
        let mut ids: Vec<String> = functions.iter().map(|f| f.function_id.clone()).collect();
        ids.sort();
        assert_eq!(ids, vec!["test::func_0", "test::func_1", "test::func_2"]);
    }

    // ---- list_trigger_infos tests ----

    #[tokio::test]
    async fn test_list_trigger_infos_empty() {
        let (_engine, module) = setup_engine_and_module();
        let triggers = module.list_trigger_infos().await;
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn test_list_trigger_infos_returns_registered_triggers() {
        let (engine, module) = setup_engine_and_module();

        // Directly insert a trigger into the engine's trigger registry
        let trigger = crate::trigger::Trigger {
            id: "trig-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: serde_json::json!({"interval": 5}),
            worker_id: None,
        };
        engine
            .trigger_registry
            .triggers
            .insert(trigger.id.clone(), trigger);

        let infos = module.list_trigger_infos().await;
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, "trig-1");
        assert_eq!(infos[0].trigger_type, "cron");
        assert_eq!(infos[0].function_id, "test::handler");
        assert_eq!(infos[0].config, serde_json::json!({"interval": 5}));
    }

    // ---- list_worker_infos tests ----

    #[tokio::test]
    async fn test_list_worker_infos_empty() {
        let (_engine, module) = setup_engine_and_module();
        let workers = module.list_worker_infos(None).await;
        assert!(workers.is_empty());
    }

    #[tokio::test]
    async fn test_list_worker_infos_with_filter_no_match() {
        let (_engine, module) = setup_engine_and_module();
        let workers = module
            .list_worker_infos(Some("nonexistent-worker-id"))
            .await;
        assert!(workers.is_empty());
    }

    // ---- register_worker_metadata tests ----

    #[tokio::test]
    async fn test_register_worker_metadata_invalid_uuid() {
        let (_engine, module) = setup_engine_and_module();

        let input = RegisterWorkerInput {
            worker_id: "not-a-valid-uuid".to_string(),
            runtime: Some("node".to_string()),
            version: Some("1.0".to_string()),
            name: Some("test-worker".to_string()),
            os: Some("linux".to_string()),
            telemetry: None,
        };

        // Should not panic, just log an error and return
        module.register_worker_metadata(input).await;
    }

    #[tokio::test]
    async fn test_register_worker_metadata_defaults_runtime_to_unknown() {
        let (engine, module) = setup_engine_and_module();

        // Create a worker in the registry first
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let worker = crate::workers::Worker::new(tx);
        let worker_id = worker.id.to_string();
        engine.worker_registry.register_worker(worker);

        let input = RegisterWorkerInput {
            worker_id: worker_id.clone(),
            runtime: None, // Should default to "unknown"
            version: Some("2.0".to_string()),
            name: Some("my-worker".to_string()),
            os: Some("darwin".to_string()),
            telemetry: None,
        };

        module.register_worker_metadata(input).await;

        // Verify the worker was updated
        let workers = module.list_worker_infos(Some(&worker_id)).await;
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].runtime.as_deref(), Some("unknown"));
        assert_eq!(workers[0].version.as_deref(), Some("2.0"));
        assert_eq!(workers[0].name.as_deref(), Some("my-worker"));
        assert_eq!(workers[0].os.as_deref(), Some("darwin"));
    }

    // ---- TriggerRegistrator implementation tests ----

    #[tokio::test]
    async fn test_trigger_registrator_register_and_unregister() {
        let (_engine, module) = setup_engine_and_module();

        let trigger = crate::trigger::Trigger {
            id: "eng-trig-1".to_string(),
            trigger_type: TRIGGER_FUNCTIONS_AVAILABLE.to_string(),
            function_id: "test::on_functions_changed".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
        };

        // Register
        let result = module.register_trigger(trigger.clone()).await;
        assert!(result.is_ok());
        assert_eq!(module.triggers.len(), 1);
        assert!(module.triggers.contains_key("eng-trig-1"));

        // Unregister
        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());
        assert!(module.triggers.is_empty());
    }

    #[tokio::test]
    async fn test_trigger_registrator_register_multiple() {
        let (_engine, module) = setup_engine_and_module();

        for i in 0..3 {
            let trigger = crate::trigger::Trigger {
                id: format!("eng-trig-{}", i),
                trigger_type: TRIGGER_WORKERS_AVAILABLE.to_string(),
                function_id: format!("test::handler_{}", i),
                config: serde_json::json!({}),
                worker_id: None,
            };
            module.register_trigger(trigger).await.unwrap();
        }

        assert_eq!(module.triggers.len(), 3);
    }

    // ---- fire_triggers tests ----

    #[tokio::test]
    async fn test_fire_triggers_no_matching_triggers() {
        let (_engine, module) = setup_engine_and_module();

        // Register a trigger of a different type
        let trigger = crate::trigger::Trigger {
            id: "trig-1".to_string(),
            trigger_type: TRIGGER_FUNCTIONS_AVAILABLE.to_string(),
            function_id: "test::handler".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
        };
        module.register_trigger(trigger).await.unwrap();

        // Fire triggers for a different type -- should not panic
        module
            .fire_triggers(
                TRIGGER_WORKERS_AVAILABLE,
                serde_json::json!({"event": "test"}),
            )
            .await;
    }

    #[tokio::test]
    async fn test_fire_triggers_with_matching_trigger() {
        let (engine, module) = setup_engine_and_module();

        // Register a function that the trigger will call
        let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
        let tx_clone = tx.clone();

        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::on_workers".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(move |input: Value| {
                let tx = tx_clone.clone();
                async move {
                    if let Some(sender) = tx.lock().unwrap().take() {
                        let _ = sender.send(input);
                    }
                    FunctionResult::Success(None)
                }
            }),
        );

        // Register a trigger that matches the type we will fire
        let trigger = crate::trigger::Trigger {
            id: "trig-workers".to_string(),
            trigger_type: TRIGGER_WORKERS_AVAILABLE.to_string(),
            function_id: "test::on_workers".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
        };
        module.register_trigger(trigger).await.unwrap();

        let data = serde_json::json!({"event": "worker_connected", "worker_id": "w1"});
        module
            .fire_triggers(TRIGGER_WORKERS_AVAILABLE, data.clone())
            .await;

        // Wait for the spawned task to complete
        let received = tokio::time::timeout(std::time::Duration::from_secs(2), rx)
            .await
            .expect("timed out waiting for trigger fire")
            .expect("channel closed");

        assert_eq!(received, data);
    }

    // ---- Serialization tests ----

    #[test]
    fn test_register_worker_input_minimal_deserialization() {
        let json = serde_json::json!({
            "_caller_worker_id": "550e8400-e29b-41d4-a716-446655440000"
        });
        let input: RegisterWorkerInput = serde_json::from_value(json).expect("deserialize");
        assert_eq!(input.worker_id, "550e8400-e29b-41d4-a716-446655440000");
        assert!(input.runtime.is_none());
        assert!(input.version.is_none());
        assert!(input.name.is_none());
        assert!(input.os.is_none());
        assert!(input.telemetry.is_none());
    }

    #[test]
    fn test_empty_input_deserializes() {
        let json = serde_json::json!({});
        let _input: EmptyInput = serde_json::from_value(json).expect("deserialize");
    }

    #[test]
    fn test_functions_list_input_defaults() {
        let json = serde_json::json!({});
        let input: FunctionsListInput = serde_json::from_value(json).expect("deserialize");
        assert!(input.include_internal.is_none());
    }

    #[test]
    fn test_triggers_list_input_defaults() {
        let json = serde_json::json!({});
        let input: TriggersListInput = serde_json::from_value(json).expect("deserialize");
        assert!(input.include_internal.is_none());
    }

    #[test]
    fn test_workers_list_input_defaults() {
        let json = serde_json::json!({});
        let input: WorkersListInput = serde_json::from_value(json).expect("deserialize");
        assert!(input.worker_id.is_none());
    }

    #[test]
    fn test_create_channel_input_defaults() {
        let input = CreateChannelInput::default();
        assert!(input.buffer_size.is_none());
    }

    #[test]
    fn test_function_info_serializes() {
        let info = FunctionInfo {
            function_id: "my::func".to_string(),
            description: Some("desc".to_string()),
            request_format: None,
            response_format: Some(serde_json::json!({"type": "string"})),
            metadata: None,
        };
        let json = serde_json::to_value(&info).expect("serialize");
        assert_eq!(json["function_id"], "my::func");
        assert_eq!(json["description"], "desc");
        assert!(json["request_format"].is_null());
        assert_eq!(
            json["response_format"],
            serde_json::json!({"type": "string"})
        );
    }

    #[test]
    fn test_trigger_info_serializes() {
        let info = TriggerInfo {
            id: "t-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "fn::handler".to_string(),
            config: serde_json::json!({"schedule": "* * * * *"}),
        };
        let json = serde_json::to_value(&info).expect("serialize");
        assert_eq!(json["id"], "t-1");
        assert_eq!(json["trigger_type"], "cron");
        assert_eq!(json["function_id"], "fn::handler");
    }

    #[tokio::test]
    async fn test_initialize_registers_engine_trigger_types() {
        let (engine, module) = setup_engine_and_module();

        module.initialize().await.unwrap();

        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key(TRIGGER_FUNCTIONS_AVAILABLE)
        );
        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key(TRIGGER_WORKERS_AVAILABLE)
        );
    }

    #[tokio::test]
    async fn test_start_background_tasks_shutdown_is_clean() {
        let (_engine, module) = setup_engine_and_module();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        module.start_background_tasks(shutdown_rx).await.unwrap();
        let _ = shutdown_tx.send(true);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn test_create_function_returns_channel_refs() {
        let (engine, module) = setup_engine_and_module();

        let result = module
            .create_function(CreateChannelInput {
                buffer_size: Some(2048),
            })
            .await;

        match result {
            FunctionResult::Success(output) => {
                assert!(
                    engine
                        .channel_manager
                        .get_channel(&output.writer.channel_id, &output.writer.access_key)
                        .is_some()
                );
                assert_eq!(output.writer.channel_id, output.reader.channel_id);
            }
            _ => panic!("expected create_function success"),
        }
    }

    #[tokio::test]
    async fn test_get_functions_filters_internal_by_default() {
        let (engine, module) = setup_engine_and_module();

        for function_id in ["engine::internal", "user::visible"] {
            engine.register_function_handler(
                crate::engine::RegisterFunctionRequest {
                    function_id: function_id.to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                crate::engine::Handler::new(|_input: Value| async move {
                    FunctionResult::Success(None)
                }),
            );
        }

        let filtered = module
            .get_functions(FunctionsListInput {
                include_internal: None,
            })
            .await;
        match filtered {
            FunctionResult::Success(Some(value)) => {
                let functions = value["functions"].as_array().unwrap();
                assert_eq!(functions.len(), 1);
                assert_eq!(functions[0]["function_id"], "user::visible");
            }
            _ => panic!("expected get_functions success"),
        }

        let all = module
            .get_functions(FunctionsListInput {
                include_internal: Some(true),
            })
            .await;
        match all {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value["functions"].as_array().unwrap().len(), 2);
            }
            _ => panic!("expected get_functions success"),
        }
    }

    #[tokio::test]
    async fn test_get_triggers_filters_internal_by_default() {
        let (_engine, module) = setup_engine_and_module();

        module.engine.trigger_registry.triggers.insert(
            "internal".to_string(),
            crate::trigger::Trigger {
                id: "internal".to_string(),
                trigger_type: "cron".to_string(),
                function_id: "engine::internal".to_string(),
                config: serde_json::json!({}),
                worker_id: None,
            },
        );
        module.engine.trigger_registry.triggers.insert(
            "user".to_string(),
            crate::trigger::Trigger {
                id: "user".to_string(),
                trigger_type: "cron".to_string(),
                function_id: "user::visible".to_string(),
                config: serde_json::json!({}),
                worker_id: None,
            },
        );

        let filtered = module
            .get_triggers(TriggersListInput {
                include_internal: None,
            })
            .await;
        match filtered {
            FunctionResult::Success(Some(value)) => {
                let triggers = value["triggers"].as_array().unwrap();
                assert_eq!(triggers.len(), 1);
                assert_eq!(triggers[0]["function_id"], "user::visible");
            }
            _ => panic!("expected get_triggers success"),
        }

        let all = module
            .get_triggers(TriggersListInput {
                include_internal: Some(true),
            })
            .await;
        match all {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value["triggers"].as_array().unwrap().len(), 2);
            }
            _ => panic!("expected get_triggers success"),
        }
    }

    #[tokio::test]
    async fn test_get_workers_returns_registered_worker_and_metrics() {
        let (engine, module) = setup_engine_and_module();

        init_metric_storage(Some(128), Some(3600));
        if let Some(storage) = get_metric_storage() {
            storage.clear();
        }

        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let worker = crate::workers::Worker::new(tx);
        let worker_id = worker.id.to_string();
        worker.include_function_id("user::visible").await;
        engine.worker_registry.register_worker(worker);

        if let Some(storage) = get_metric_storage() {
            storage.add_metrics(vec![StoredMetric {
                name: "iii.worker.cpu.percent".to_string(),
                description: "cpu".to_string(),
                unit: "%".to_string(),
                metric_type: StoredMetricType::Gauge,
                data_points: vec![StoredDataPoint::Number(StoredNumberDataPoint {
                    value: 42.0,
                    attributes: vec![("worker.id".to_string(), worker_id.clone())],
                    timestamp_unix_nano: 2_000_000_000,
                })],
                service_name: "svc".to_string(),
                timestamp_unix_nano: 2_000_000_000,
                instrumentation_scope_name: None,
                instrumentation_scope_version: None,
            }]);
        }

        let result = module
            .get_workers(WorkersListInput {
                worker_id: Some(worker_id.clone()),
            })
            .await;

        match result {
            FunctionResult::Success(Some(value)) => {
                let workers = value["workers"].as_array().unwrap();
                assert_eq!(workers.len(), 1);
                assert_eq!(workers[0]["id"], worker_id);
                assert_eq!(workers[0]["function_count"], 1);
            }
            _ => panic!("expected get_workers success"),
        }
    }

    #[tokio::test]
    async fn test_register_worker_service_fires_worker_trigger() {
        let (engine, module) = setup_engine_and_module();

        let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
        let tx_clone = tx.clone();
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::worker_listener".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(move |input: Value| {
                let tx = tx_clone.clone();
                async move {
                    if let Some(sender) = tx.lock().unwrap().take() {
                        let _ = sender.send(input);
                    }
                    FunctionResult::Success(None)
                }
            }),
        );

        engine.trigger_registry.triggers.insert(
            "worker-trigger".to_string(),
            crate::trigger::Trigger {
                id: "worker-trigger".to_string(),
                trigger_type: TRIGGER_WORKERS_AVAILABLE.to_string(),
                function_id: "test::worker_listener".to_string(),
                config: serde_json::json!({}),
                worker_id: None,
            },
        );

        let worker = crate::workers::Worker::new(tokio::sync::mpsc::channel(1).0);
        let worker_id = worker.id.to_string();
        engine.worker_registry.register_worker(worker);

        let result = module
            .register_worker(RegisterWorkerInput {
                worker_id: worker_id.clone(),
                runtime: Some("node".to_string()),
                version: Some("1.0.0".to_string()),
                name: Some("my-worker".to_string()),
                os: Some("linux".to_string()),
                telemetry: None,
            })
            .await;
        assert!(matches!(result, FunctionResult::Success(Some(_))));

        let payload = tokio::time::timeout(std::time::Duration::from_secs(2), rx)
            .await
            .expect("timed out waiting for worker trigger")
            .expect("worker trigger channel closed");
        assert_eq!(payload["event"], "worker_metadata_updated");
        assert_eq!(payload["worker_id"], worker_id);
    }

    #[tokio::test]
    async fn test_destroy_returns_ok() {
        let (_engine, module) = setup_engine_and_module();
        module.destroy().await.unwrap();
    }
}
