use std::sync::Arc;

use function_macros::{function, service};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::core_module::CoreModule,
    protocol::ErrorBody,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmptyInput {}

#[derive(Clone)]
pub struct CoreEngineModule {
    engine: Arc<Engine>,
}

impl CoreEngineModule {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self { engine }
    }
}

#[async_trait::async_trait]
impl CoreModule for CoreEngineModule {
    fn name(&self) -> &'static str {
        "CoreEngineModule"
    }

    async fn create(
        engine: Arc<Engine>,
        _config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        Ok(Box::new(CoreEngineModule::new(engine)))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing CoreEngineModule");
        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[service(name = "engine")]
impl CoreEngineModule {
    #[function(name = "engine.functions.list", description = "List all functions")]
    pub async fn list_functions(&self, _input: EmptyInput) -> FunctionResult<Option<Value>, ErrorBody> {
        FunctionResult::Success(Some(self.engine.list_functions_as_json()))
    }

    #[function(name = "engine.workers.list", description = "List all workers")]
    pub async fn list_workers(&self, _input: EmptyInput) -> FunctionResult<Option<Value>, ErrorBody> {
        FunctionResult::Success(Some(self.engine.list_workers_as_json().await))
    }

    #[function(name = "engine.triggers.list", description = "List all triggers")]
    pub async fn list_triggers(&self, _input: EmptyInput) -> FunctionResult<Option<Value>, ErrorBody> {
        FunctionResult::Success(Some(self.engine.list_triggers_as_json().await))
    }
}

crate::register_module!(
    "modules::core_engine::CoreEngineModule",
    CoreEngineModule,
    enabled_by_default = true
);
