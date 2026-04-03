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
    config::CronWorkerConfig,
    structs::{CronAdapter, CronSchedulerAdapter},
};
use crate::{
    engine::{Engine, EngineTrait},
    workers::module::{AdapterFactory, ConfigurableWorker, Worker},
    trigger::{Trigger, TriggerRegistrator},
};

#[derive(Clone)]
pub struct CronWorker {
    adapter: Arc<CronAdapter>,
    engine: Arc<Engine>,
    _config: CronWorkerConfig,
}

#[async_trait]
impl Worker for CronWorker {
    fn name(&self) -> &'static str {
        "CronWorker"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing CronWorker");

        use crate::trigger::TriggerType;

        let trigger_type = TriggerType::new(
            "cron",
            "Cron-based scheduled triggers",
            Box::new(self.clone()),
            None,
        );

        self.engine.register_trigger_type(trigger_type).await;

        tracing::info!("{} Cron trigger type initialized", "[READY]".green());
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
        _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        let adapter = Arc::clone(&self.adapter);

        tokio::spawn(async move {
            let _ = shutdown_rx.changed().await;
            tracing::info!("CronWorker received shutdown signal, stopping cron jobs");
            adapter.shutdown().await;
        });

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying CronWorker");
        self.adapter.shutdown().await;
        Ok(())
    }
}

impl TriggerRegistrator for CronWorker {
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
                .get("condition_function_id")
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
impl ConfigurableWorker for CronWorker {
    type Config = CronWorkerConfig;
    type Adapter = dyn CronSchedulerAdapter;
    type AdapterRegistration = super::registry::CronAdapterRegistration;
    const DEFAULT_ADAPTER_NAME: &'static str = "kv";

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn CronSchedulerAdapter>>>> =
            Lazy::new(|| RwLock::new(CronWorker::build_registry()));
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

    fn adapter_name_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.name.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

crate::register_worker!("iii-cron", CronWorker, enabled_by_default = true);

#[cfg(test)]
mod tests {
    use super::super::structs::CronSchedulerAdapter;
    use super::*;
    use crate::workers::observability::metrics::ensure_default_meter;
    use serde_json::json;

    // =========================================================================
    // ConfigurableWorker trait constants
    // =========================================================================

    #[test]
    fn default_adapter_name() {
        assert_eq!(CronWorker::DEFAULT_ADAPTER_NAME, "kv");
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

    fn setup_cron_worker() -> (Arc<Engine>, CronWorker) {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let scheduler: Arc<dyn CronSchedulerAdapter> = Arc::new(MockCronSchedulerAdapter);
        let cron_adapter = super::super::structs::CronAdapter::new(scheduler, engine.clone());
        let config = super::super::config::CronWorkerConfig::default();
        let worker = CronWorker {
            adapter: Arc::new(cron_adapter),
            engine: engine.clone(),
            _config: config,
        };
        (engine, worker)
    }

    // =========================================================================
    // CronWorker::name
    // =========================================================================

    #[test]
    fn cron_worker_name() {
        let (_engine, worker) = setup_cron_worker();
        assert_eq!(Worker::name(&worker), "CronWorker");
    }

    // =========================================================================
    // Worker::initialize test
    // =========================================================================

    #[tokio::test]
    async fn initialize_registers_cron_trigger_type() {
        let (engine, worker) = setup_cron_worker();
        let result = worker.initialize().await;
        assert!(result.is_ok());
        assert!(engine.trigger_registry.trigger_types.contains_key("cron"));
    }

    // =========================================================================
    // Worker::destroy test
    // =========================================================================

    #[tokio::test]
    async fn destroy_calls_shutdown() {
        let (_engine, worker) = setup_cron_worker();
        let result = worker.destroy().await;
        assert!(result.is_ok());
    }

    // =========================================================================
    // Worker::start_background_tasks test
    // =========================================================================

    #[tokio::test]
    async fn start_background_tasks_spawns_shutdown_listener() {
        let (_engine, worker) = setup_cron_worker();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let result = worker.start_background_tasks(rx, tx.clone()).await;
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
        let (_engine, worker) = setup_cron_worker();
        let trigger = crate::trigger::Trigger {
            id: "cron-trig-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "expression": "0 0 * * * *"
            }),
            worker_id: None,
        };
        let result = worker.register_trigger(trigger).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn register_trigger_with_empty_expression_fails() {
        let (_engine, worker) = setup_cron_worker();
        let trigger = crate::trigger::Trigger {
            id: "cron-trig-empty".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "expression": ""
            }),
            worker_id: None,
        };
        let result = worker.register_trigger(trigger).await;
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
        let (_engine, worker) = setup_cron_worker();
        let trigger = crate::trigger::Trigger {
            id: "cron-trig-missing".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({}),
            worker_id: None,
        };
        let result = worker.register_trigger(trigger).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn register_trigger_with_condition_function_id() {
        let (_engine, worker) = setup_cron_worker();
        let trigger = crate::trigger::Trigger {
            id: "cron-trig-cond".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "expression": "0 30 * * * *",
                "condition_function_id": "test::condition_fn"
            }),
            worker_id: None,
        };
        let result = worker.register_trigger(trigger).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn unregister_trigger_calls_adapter() {
        let (_engine, worker) = setup_cron_worker();

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
        let _ = worker.register_trigger(trigger.clone()).await;

        // Now unregister it
        let result = worker.unregister_trigger(trigger).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn unregister_trigger_nonexistent_returns_error() {
        let (_engine, worker) = setup_cron_worker();
        let trigger = crate::trigger::Trigger {
            id: "nonexistent-cron".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({}),
            worker_id: None,
        };
        let result = worker.unregister_trigger(trigger).await;
        assert!(result.is_err());
    }

    // =========================================================================
    // ConfigurableWorker trait tests
    // =========================================================================

    #[test]
    fn adapter_name_from_config_none() {
        let config = super::super::config::CronWorkerConfig::default();
        assert!(CronWorker::adapter_name_from_config(&config).is_none());
    }

    #[test]
    fn adapter_name_from_config_some() {
        let config = super::super::config::CronWorkerConfig {
            adapter: Some(crate::workers::module::AdapterEntry {
                name: "my::CronAdapter".to_string(),
                config: None,
            }),
        };
        assert_eq!(
            CronWorker::adapter_name_from_config(&config),
            Some("my::CronAdapter".to_string())
        );
    }

    #[test]
    fn adapter_config_from_config_none() {
        let config = super::super::config::CronWorkerConfig::default();
        assert!(CronWorker::adapter_config_from_config(&config).is_none());
    }

    #[test]
    fn adapter_config_from_config_some() {
        let config = super::super::config::CronWorkerConfig {
            adapter: Some(crate::workers::module::AdapterEntry {
                name: "my::Adapter".to_string(),
                config: Some(json!({"interval": 60})),
            }),
        };
        assert_eq!(
            CronWorker::adapter_config_from_config(&config),
            Some(json!({"interval": 60}))
        );
    }

    #[test]
    fn adapter_config_from_config_adapter_without_config() {
        let config = super::super::config::CronWorkerConfig {
            adapter: Some(crate::workers::module::AdapterEntry {
                name: "my::Adapter".to_string(),
                config: None,
            }),
        };
        assert!(CronWorker::adapter_config_from_config(&config).is_none());
    }

    // =========================================================================
    // build helper test
    // =========================================================================

    #[test]
    fn build_creates_worker() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let scheduler: Arc<dyn CronSchedulerAdapter> = Arc::new(MockCronSchedulerAdapter);
        let config = super::super::config::CronWorkerConfig::default();
        let worker = CronWorker::build(engine.clone(), config, scheduler);
        assert_eq!(Worker::name(&worker), "CronWorker");
    }

    // =========================================================================
    // Worker::register_functions (noop) test
    // =========================================================================

    #[test]
    fn register_functions_does_nothing() {
        let (_engine, worker) = setup_cron_worker();
        let engine = Arc::new(Engine::new());
        // Should not panic
        worker.register_functions(engine);
    }

    // =========================================================================
    // Multiple register/unregister cycle
    // =========================================================================

    #[tokio::test]
    async fn register_unregister_cycle() {
        let (_engine, worker) = setup_cron_worker();

        let trigger = crate::trigger::Trigger {
            id: "cycle-trig".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"expression": "0 0 * * * *"}),
            worker_id: None,
        };

        // Register
        let result = worker.register_trigger(trigger.clone()).await;
        assert!(result.is_ok());

        // Unregister
        let result = worker.unregister_trigger(trigger.clone()).await;
        assert!(result.is_ok());

        // Unregister again should fail (already removed)
        let result = worker.unregister_trigger(trigger).await;
        assert!(result.is_err());
    }

    // =========================================================================
    // Duplicate registration fails
    // =========================================================================

    #[tokio::test]
    async fn register_duplicate_trigger_fails() {
        let (_engine, worker) = setup_cron_worker();

        let trigger = crate::trigger::Trigger {
            id: "dup-trig".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"expression": "0 0 * * * *"}),
            worker_id: None,
        };

        let result = worker.register_trigger(trigger.clone()).await;
        assert!(result.is_ok());

        // Registering again with the same ID should fail
        let result = worker.register_trigger(trigger).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("already registered")
        );
    }
}
