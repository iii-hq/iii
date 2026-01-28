use std::{
    collections::HashMap,
    sync::{Arc, RwLock as SyncRwLock},
};

use function_macros::{function, service};
use once_cell::sync::Lazy;
use serde_json::Value;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::{
        module::{AdapterFactory, ConfigurableModule, Module},
        state::{
            adapters::StateAdapter,
            config::StateModuleConfig,
            structs::{StateDeleteInput, StateGetGroupInput, StateGetInput, StateSetInput},
        },
    },
    protocol::ErrorBody,
};

#[derive(Clone)]
pub struct StateCoreModule {
    adapter: Arc<dyn StateAdapter>,
}

#[async_trait::async_trait]
impl Module for StateCoreModule {
    fn name(&self) -> &'static str {
        "StateCoreModule"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying StateCoreModule");
        self.adapter.destroy().await?;
        Ok(())
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing StateCoreModule");
        Ok(())
    }
}

#[async_trait::async_trait]
impl ConfigurableModule for StateCoreModule {
    type Config = StateModuleConfig;
    type Adapter = dyn StateAdapter;
    type AdapterRegistration = super::registry::StateAdapterRegistration;
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::state::adapters::RedisAdapter";

    async fn registry() -> &'static SyncRwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<SyncRwLock<HashMap<String, AdapterFactory<dyn StateAdapter>>>> =
            Lazy::new(|| SyncRwLock::new(StateCoreModule::build_registry()));
        &REGISTRY
    }

    fn build(_engine: Arc<Engine>, _config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self { adapter }
    }

    fn adapter_class_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.class.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

#[service(name = "state")]
impl StateCoreModule {
    #[function(name = "state.set", description = "Set a value in state")]
    pub async fn set(&self, input: StateSetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        match self
            .adapter
            .set(&input.group_id, &input.item_id, input.data.clone())
            .await
        {
            Ok(_) => FunctionResult::Success(Some(input.data)),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to set value: {}", e),
                code: "SET_ERROR".to_string(),
            }),
        }
    }

    #[function(name = "state.get", description = "Get a value from state")]
    pub async fn get(&self, input: StateGetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.adapter.get(&input.group_id, &input.item_id).await {
            Ok(value) => FunctionResult::Success(value),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to get value: {}", e),
                code: "GET_ERROR".to_string(),
            }),
        }
    }

    #[function(name = "state.delete", description = "Delete a value from state")]
    pub async fn delete(
        &self,
        input: StateDeleteInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let value = match self.adapter.get(&input.group_id, &input.item_id).await {
            Ok(v) => v,
            Err(e) => {
                return FunctionResult::Failure(ErrorBody {
                    message: format!("Failed to get value before delete: {}", e),
                    code: "GET_ERROR".to_string(),
                });
            }
        };

        match self.adapter.delete(&input.group_id, &input.item_id).await {
            Ok(_) => FunctionResult::Success(value),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to delete value: {}", e),
                code: "DELETE_ERROR".to_string(),
            }),
        }
    }

    #[function(name = "state.list", description = "Get a group from state")]
    pub async fn list(
        &self,
        input: StateGetGroupInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.adapter.list(&input.group_id).await {
            Ok(values) => FunctionResult::Success(serde_json::to_value(values).ok()),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to list values: {}", e),
                code: "LIST_ERROR".to_string(),
            }),
        }
    }
}

crate::register_module!(
    "modules::state::StateModule",
    StateCoreModule,
    enabled_by_default = true
);
