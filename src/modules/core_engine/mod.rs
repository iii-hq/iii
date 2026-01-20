use std::{pin::Pin, sync::Arc};

use dashmap::DashMap;
use function_macros::{function, service};
use futures::Future;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::core_module::CoreModule,
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};

pub const TRIGGER_FUNCTIONS_AVAILABLE: &str = "engine::functions-available";
pub const TRIGGER_WORKERS_AVAILABLE: &str = "engine::workers-available";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmptyInput {}

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
        let triggers_map = self.engine.trigger_registry.triggers.read().await;
        triggers_map
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

    async fn list_worker_infos(&self) -> Vec<WorkerInfo> {
        let workers = self.engine.worker_registry.list_workers().await;
        let mut worker_infos = Vec::with_capacity(workers.len());

        for w in workers {
            let functions = w.get_function_paths().await;
            let function_count = functions.len();
            let active_invocations = w.invocation_count().await;

            worker_infos.push(WorkerInfo {
                id: w.id.to_string(),
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
            });
        }
        worker_infos
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
impl CoreModule for WorkerModule {
    fn name(&self) -> &'static str {
        "WorkerModule"
    }

    async fn create(
        engine: Arc<Engine>,
        _config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
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

    #[function(name = "engine.workers.list", description = "List all workers")]
    pub async fn get_workers(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let workers = self.list_worker_infos().await;
        FunctionResult::Success(Some(serde_json::json!({ "workers": workers })))
    }

    #[function(name = "engine.triggers.list", description = "List all triggers")]
    pub async fn get_triggers(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let triggers = self.list_trigger_infos().await;
        FunctionResult::Success(Some(serde_json::json!({ "triggers": triggers })))
    }
}

crate::register_module!(
    "modules::core_engine::WorkerModule",
    WorkerModule,
    enabled_by_default = true
);
