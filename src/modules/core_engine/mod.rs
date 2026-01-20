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
    pub async fn list_functions(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        FunctionResult::Success(Some(self.engine.list_functions_as_json()))
    }

    #[function(name = "engine.workers.list", description = "List all workers")]
    pub async fn list_workers(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        FunctionResult::Success(Some(self.engine.list_workers_as_json().await))
    }

    #[function(name = "engine.triggers.list", description = "List all triggers")]
    pub async fn list_triggers(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        FunctionResult::Success(Some(self.engine.list_triggers_as_json().await))
    }
}

crate::register_module!(
    "modules::core_engine::WorkerModule",
    WorkerModule,
    enabled_by_default = true
);
