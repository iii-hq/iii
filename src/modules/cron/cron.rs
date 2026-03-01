// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
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

    async fn start_background_tasks(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let adapter = Arc::clone(&self.adapter);

        tokio::spawn(async move {
            let _ = shutdown.changed().await;
            tracing::info!("CronModule received shutdown signal, stopping cron jobs");
            adapter.shutdown().await;
        });

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying CronModule");
        self.adapter.shutdown().await;
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

            let condition_function_id = trigger
                .config
                .get("_condition_path")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());

            self.adapter
                .register(
                    &trigger.id,
                    &cron_expression,
                    &trigger.function_id,
                    condition_function_id,
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
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::cron::KvCronAdapter";

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

#[cfg(test)]
mod tests {
    use super::super::structs::CronSchedulerAdapter;
    use super::*;
    use crate::modules::observability::metrics::ensure_default_meter;
    use serde_json::json;

    // =========================================================================
    // ConfigurableModule trait constants
    // =========================================================================

    #[test]
    fn default_adapter_class() {
        assert_eq!(
            CronCoreModule::DEFAULT_ADAPTER_CLASS,
            "modules::cron::KvCronAdapter"
        );
    }

    // =========================================================================
    // Mock scheduler adapter
    // =========================================================================

    struct MockCronSchedulerAdapter;

    #[async_trait]
    impl CronSchedulerAdapter for MockCronSchedulerAdapter {
        async fn try_acquire_lock(&self, _job_id: &str) -> bool {
            true
        }
        async fn release_lock(&self, _job_id: &str) {}
    }

    fn setup_cron_module() -> (Arc<Engine>, CronCoreModule) {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let scheduler: Arc<dyn CronSchedulerAdapter> = Arc::new(MockCronSchedulerAdapter);
        let cron_adapter = super::super::structs::CronAdapter::new(scheduler, engine.clone());
        let config = super::super::config::CronModuleConfig::default();
        let module = CronCoreModule {
            adapter: Arc::new(cron_adapter),
            engine: engine.clone(),
            _config: config,
        };
        (engine, module)
    }

    // =========================================================================
    // CronCoreModule::name
    // =========================================================================

    #[test]
    fn cron_module_name() {
        let (_engine, module) = setup_cron_module();
        assert_eq!(Module::name(&module), "CronModule");
    }

    // =========================================================================
    // Module::initialize test
    // =========================================================================

    #[tokio::test]
    async fn initialize_registers_cron_trigger_type() {
        let (engine, module) = setup_cron_module();
        let result = module.initialize().await;
        assert!(result.is_ok());
        assert!(engine.trigger_registry.trigger_types.contains_key("cron"));
    }

    // =========================================================================
    // Module::destroy test
    // =========================================================================

    #[tokio::test]
    async fn destroy_calls_shutdown() {
        let (_engine, module) = setup_cron_module();
        let result = module.destroy().await;
        assert!(result.is_ok());
    }

    // =========================================================================
    // Module::start_background_tasks test
    // =========================================================================

    #[tokio::test]
    async fn start_background_tasks_spawns_shutdown_listener() {
        let (_engine, module) = setup_cron_module();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let result = module.start_background_tasks(rx).await;
        assert!(result.is_ok());
        // Send shutdown signal
        let _ = tx.send(true);
        // Give the spawned task time to process
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // =========================================================================
    // TriggerRegistrator tests
    // =========================================================================

    #[tokio::test]
    async fn register_trigger_with_valid_cron_expression() {
        let (_engine, module) = setup_cron_module();
        let trigger = crate::trigger::Trigger {
            id: "cron-trig-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "expression": "0 0 * * * *"
            }),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn register_trigger_with_empty_expression_fails() {
        let (_engine, module) = setup_cron_module();
        let trigger = crate::trigger::Trigger {
            id: "cron-trig-empty".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "expression": ""
            }),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Cron expression is required")
        );
    }

    #[tokio::test]
    async fn register_trigger_with_missing_expression_fails() {
        let (_engine, module) = setup_cron_module();
        let trigger = crate::trigger::Trigger {
            id: "cron-trig-missing".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({}),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn register_trigger_with_condition_path() {
        let (_engine, module) = setup_cron_module();
        let trigger = crate::trigger::Trigger {
            id: "cron-trig-cond".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "expression": "0 30 * * * *",
                "_condition_path": "test::condition_fn"
            }),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn unregister_trigger_calls_adapter() {
        let (_engine, module) = setup_cron_module();

        // First register a trigger
        let trigger = crate::trigger::Trigger {
            id: "cron-trig-unreg".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "expression": "0 0 * * * *"
            }),
            worker_id: None,
        };
        let _ = module.register_trigger(trigger.clone()).await;

        // Now unregister it
        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn unregister_trigger_nonexistent_returns_error() {
        let (_engine, module) = setup_cron_module();
        let trigger = crate::trigger::Trigger {
            id: "nonexistent-cron".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({}),
            worker_id: None,
        };
        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_err());
    }

    // =========================================================================
    // ConfigurableModule trait tests
    // =========================================================================

    #[test]
    fn adapter_class_from_config_none() {
        let config = super::super::config::CronModuleConfig::default();
        assert!(CronCoreModule::adapter_class_from_config(&config).is_none());
    }

    #[test]
    fn adapter_class_from_config_some() {
        let config = super::super::config::CronModuleConfig {
            adapter: Some(crate::modules::module::AdapterEntry {
                class: "my::CronAdapter".to_string(),
                config: None,
            }),
        };
        assert_eq!(
            CronCoreModule::adapter_class_from_config(&config),
            Some("my::CronAdapter".to_string())
        );
    }

    #[test]
    fn adapter_config_from_config_none() {
        let config = super::super::config::CronModuleConfig::default();
        assert!(CronCoreModule::adapter_config_from_config(&config).is_none());
    }

    #[test]
    fn adapter_config_from_config_some() {
        let config = super::super::config::CronModuleConfig {
            adapter: Some(crate::modules::module::AdapterEntry {
                class: "my::Adapter".to_string(),
                config: Some(json!({"interval": 60})),
            }),
        };
        assert_eq!(
            CronCoreModule::adapter_config_from_config(&config),
            Some(json!({"interval": 60}))
        );
    }

    #[test]
    fn adapter_config_from_config_adapter_without_config() {
        let config = super::super::config::CronModuleConfig {
            adapter: Some(crate::modules::module::AdapterEntry {
                class: "my::Adapter".to_string(),
                config: None,
            }),
        };
        assert!(CronCoreModule::adapter_config_from_config(&config).is_none());
    }

    // =========================================================================
    // build helper test
    // =========================================================================

    #[test]
    fn build_creates_module() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let scheduler: Arc<dyn CronSchedulerAdapter> = Arc::new(MockCronSchedulerAdapter);
        let config = super::super::config::CronModuleConfig::default();
        let module = CronCoreModule::build(engine.clone(), config, scheduler);
        assert_eq!(Module::name(&module), "CronModule");
    }

    // =========================================================================
    // Module::register_functions (noop) test
    // =========================================================================

    #[test]
    fn register_functions_does_nothing() {
        let (_engine, module) = setup_cron_module();
        let engine = Arc::new(Engine::new());
        // Should not panic
        module.register_functions(engine);
    }

    // =========================================================================
    // Multiple register/unregister cycle
    // =========================================================================

    #[tokio::test]
    async fn register_unregister_cycle() {
        let (_engine, module) = setup_cron_module();

        let trigger = crate::trigger::Trigger {
            id: "cycle-trig".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"expression": "0 0 * * * *"}),
            worker_id: None,
        };

        // Register
        let result = module.register_trigger(trigger.clone()).await;
        assert!(result.is_ok());

        // Unregister
        let result = module.unregister_trigger(trigger.clone()).await;
        assert!(result.is_ok());

        // Unregister again should fail (already removed)
        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_err());
    }

    // =========================================================================
    // Duplicate registration fails
    // =========================================================================

    #[tokio::test]
    async fn register_duplicate_trigger_fails() {
        let (_engine, module) = setup_cron_module();

        let trigger = crate::trigger::Trigger {
            id: "dup-trig".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"expression": "0 0 * * * *"}),
            worker_id: None,
        };

        let result = module.register_trigger(trigger.clone()).await;
        assert!(result.is_ok());

        // Registering again with the same ID should fail
        let result = module.register_trigger(trigger).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("already registered")
        );
    }
}
