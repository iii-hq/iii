// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

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
        let _ = self.adapter.destroy().await;
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
        let _ = self
            .adapter
            .set(&input.group_id, &input.item_id, input.data.clone())
            .await;

        FunctionResult::Success(Some(input.data))
    }

    #[function(name = "state.get", description = "Get a value from state")]
    pub async fn get(&self, input: StateGetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let value = self.adapter.get(&input.group_id, &input.item_id).await;

        FunctionResult::Success(value)
    }

    #[function(name = "state.delete", description = "Delete a value from state")]
    pub async fn delete(
        &self,
        input: StateDeleteInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let value = self.adapter.get(&input.group_id, &input.item_id).await;
        let _ = self.adapter.delete(&input.group_id, &input.item_id).await;

        FunctionResult::Success(value)
    }

    #[function(name = "state.list", description = "Get a group from state")]
    pub async fn list(
        &self,
        input: StateGetGroupInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let values = self.adapter.list(&input.group_id).await;

        FunctionResult::Success(serde_json::to_value(values).ok())
    }
}

crate::register_module!(
    "modules::state::StateModule",
    StateCoreModule,
    enabled_by_default = true
);
