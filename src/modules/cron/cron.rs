// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use colored::Colorize;
use futures::Future;
use once_cell::sync::Lazy;
use serde_json::Value;

use super::{
    config::CronModuleConfig,
    structs::{CronAdapter, CronSchedulerAdapter},
};
use crate::{
    engine::{Engine, EngineTrait},
    modules::module::{AdapterFactory, ConfigurableModule, Module},
    trigger::{Trigger, TriggerRegistrator},
};

#[derive(Clone)]
pub struct CronCoreModule {
    adapter: Arc<CronAdapter>,
    engine: Arc<Engine>,
    _config: CronModuleConfig,
}

#[async_trait]
impl Module for CronCoreModule {
    fn name(&self) -> &'static str {
        "CronModule"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing CronModule");

        use crate::trigger::TriggerType;

        let trigger_type = TriggerType {
            id: "cron".to_string(),
            _description: "Cron-based scheduled triggers".to_string(),
            registrator: Box::new(self.clone()),
            worker_id: None,
        };

        self.engine.register_trigger_type(trigger_type).await;

        tracing::info!("{} Cron trigger type initialized", "[READY]".green());
        Ok(())
    }
}

impl TriggerRegistrator for CronCoreModule {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let cron_expression = trigger
            .config
            .get("expression")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Box::pin(async move {
            if cron_expression.is_empty() {
                tracing::error!(
                    "Cron expression is not set for trigger {}",
                    trigger.id.purple()
                );
                return Err(anyhow::anyhow!("Cron expression is required"));
            }

            let condition_function_path = trigger
                .config
                .get("_condition_path")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());

            self.adapter
                .register(
                    &trigger.id,
                    &cron_expression,
                    &trigger.function_path,
                    condition_function_path,
                )
                .await
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        Box::pin(async move {
            tracing::debug!(trigger_id = %trigger.id, "Unregistering cron trigger");
            self.adapter.unregister(&trigger.id).await
        })
    }
}

#[async_trait]
impl ConfigurableModule for CronCoreModule {
    type Config = CronModuleConfig;
    type Adapter = dyn CronSchedulerAdapter;
    type AdapterRegistration = super::registry::CronAdapterRegistration;
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::cron::RedisCronAdapter";

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn CronSchedulerAdapter>>>> =
            Lazy::new(|| RwLock::new(CronCoreModule::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        let cron_adapter = CronAdapter::new(adapter, engine.clone());
        Self {
            engine,
            _config: config,
            adapter: Arc::new(cron_adapter),
        }
    }

    fn adapter_class_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.class.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

crate::register_module!(
    "modules::cron::CronModule",
    CronCoreModule,
    enabled_by_default = true
);
