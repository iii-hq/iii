// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    pin::Pin,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use colored::Colorize;
use futures::Future;
use once_cell::sync::Lazy;
use serde_json::{Value, json};

use super::{
    config::CronModuleConfig,
    structs::{CronAdapter, CronSchedulerAdapter},
};
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    trigger::{Trigger, TriggerRegistrator},
    workers::traits::{AdapterFactory, ConfigurableWorker, Worker},
};

/// The live config + scheduler, swapped together under one lock so every reader
/// observes a coherent generation. Combining them mirrors `iii-queue`: a runtime
/// lock-backend hot-swap must not expose a new-config + old-adapter window to a
/// concurrent trigger registration. The inner `Arc`s make a snapshot a cheap
/// clone.
#[derive(Clone)]
struct LiveState {
    config: Arc<CronModuleConfig>,
    adapter: Arc<CronAdapter>,
}

#[derive(Clone)]
pub struct CronWorker {
    engine: Arc<Engine>,
    /// Live config + scheduler behind a single lock. The outer lock is held only
    /// for pointer swaps; readers clone the inner `Arc`(s) once per call via
    /// `config_snapshot()` / `adapter_snapshot()`. Both are hot-swappable from
    /// the configuration worker, and `apply_config` swaps the pair atomically so
    /// no reader sees a half-applied transport change.
    live: Arc<RwLock<LiveState>>,
    /// The config.yaml block passed to `build()`. Seeds the first
    /// `configuration::register`; the configuration entry is the runtime source
    /// of truth afterwards.
    seed: Option<CronModuleConfig>,
    /// Serializes concurrent `apply_config` runs (rapid configuration edits).
    apply_lock: Arc<tokio::sync::Mutex<()>>,
    /// Set by `destroy()` (under `apply_lock`). A late `apply_config` — from an
    /// in-flight trigger event or the detached one-shot retry in
    /// `on_config_change` — checks this after acquiring the lock and bails, so a
    /// torn-down worker can never rebuild a scheduler and re-spawn cron jobs.
    destroyed: Arc<AtomicBool>,
}

impl CronWorker {
    /// Cheap clone of the live config. Take one snapshot per call so all reads
    /// within it are consistent.
    pub fn config_snapshot(&self) -> Arc<CronModuleConfig> {
        self.live.read().expect("live lock poisoned").config.clone()
    }

    fn set_config(&self, config: CronModuleConfig) {
        self.live.write().expect("live lock poisoned").config = Arc::new(config);
    }

    /// Cheap clone of the live scheduler. `pub` so integration tests can confirm
    /// a hot-swap actually rebuilt the adapter instance (via `Arc::ptr_eq`).
    pub fn adapter_snapshot(&self) -> Arc<CronAdapter> {
        self.live
            .read()
            .expect("live lock poisoned")
            .adapter
            .clone()
    }

    /// Atomically swap both the config and the scheduler, so a concurrent reader
    /// sees either the old (config, adapter) pair or the new one — never a mix.
    fn set_live(&self, config: CronModuleConfig, adapter: Arc<CronAdapter>) {
        let mut guard = self.live.write().expect("live lock poisoned");
        guard.config = Arc::new(config);
        guard.adapter = adapter;
    }

    /// Resolve and instantiate the lock backend named by `config` (or the
    /// default). Mirrors steps 3–4 of `ConfigurableWorker::create_with_adapters`,
    /// reused for test construction and for runtime adapter hot-swaps.
    async fn resolve_scheduler(
        engine: &Arc<Engine>,
        config: &CronModuleConfig,
    ) -> anyhow::Result<Arc<dyn CronSchedulerAdapter>> {
        let adapter_name = Self::adapter_name_from_config(config)
            .unwrap_or_else(|| Self::DEFAULT_ADAPTER_NAME.to_string());
        let factory = Self::get_adapter(&adapter_name)
            .await
            .ok_or_else(|| anyhow::anyhow!("cron adapter '{adapter_name}' is not registered"))?;
        let adapter_config = Self::adapter_config_from_config(config);
        factory(engine.clone(), adapter_config).await
    }

    /// Effective `(name, config)` of the adapter selected by `config`, with the
    /// default name filled in, so `None` vs `Some(kv)` is not a false change when
    /// comparing two configs.
    fn effective_adapter(config: &CronModuleConfig) -> (String, Option<Value>) {
        (
            Self::adapter_name_from_config(config)
                .unwrap_or_else(|| Self::DEFAULT_ADAPTER_NAME.to_string()),
            Self::adapter_config_from_config(config),
        )
    }

    /// Construct a worker from a raw config value — mirrors the queue/state
    /// workers so integration tests in `engine/tests/` can drive the concrete
    /// worker without booting the full engine. Async because the scheduler is
    /// resolved from the registry.
    #[doc(hidden)]
    pub async fn for_test(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Self> {
        let parsed: CronModuleConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let scheduler = Self::resolve_scheduler(&engine, &parsed).await?;
        Ok(Self::build(engine, parsed, scheduler))
    }

    /// Register the `iii-cron::on-config-change` handler. Idempotent
    /// (replace-by-id), so it is safe to call from both `register_functions`
    /// (worker scope, for destroy/reload cleanup) and `start_background_tasks`
    /// (which registers the trigger).
    fn register_config_handler(&self, engine: &Arc<Engine>) {
        let worker = self.clone();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: super::configuration::CONFIG_FN_ID.to_string(),
                description: Some(
                    "Internal: re-apply the iii-cron configuration when the \
                     authoritative configuration entry changes."
                        .to_string(),
                ),
                request_format: None,
                response_format: None,
                metadata: Some(serde_json::json!({ "internal": true })),
            },
            Handler::new(move |_payload: Value| {
                let worker = worker.clone();
                async move {
                    super::configuration::on_config_change(&worker).await;
                    FunctionResult::Success(Some(json!({ "ok": true })))
                }
            }),
        );
    }

    /// Re-fetch the authoritative configuration and hot-apply it under
    /// `apply_lock`. Strict gate (fetch, then adapter resolution): any failure
    /// keeps the previous config and scheduler. Cron's module config is
    /// adapter-only, so the only runtime change is a lock-backend hot-swap — the
    /// new transport is built first (fallible), then every live cron job is
    /// re-registered onto it best-effort (a single job's failure is logged while
    /// the rest apply), the pair is swapped in atomically, and the old scheduler
    /// is shut down.
    pub(crate) async fn apply_config(&self) -> anyhow::Result<()> {
        let _guard = self.apply_lock.lock().await;

        // Bail if the worker was torn down. `destroy()` sets this under the same
        // lock, so a late apply (in-flight trigger event, or the detached retry
        // in `on_config_change`) can never rebuild a scheduler post-destroy.
        if self.destroyed.load(Ordering::SeqCst) {
            return Ok(());
        }

        // GATE: fetch under the lock; keep the `Elapsed` error downcastable so
        // `on_config_change` schedules its one-shot retry for timeouts only.
        let new_config = match tokio::time::timeout(
            super::configuration::CONFIG_BUS_TIMEOUT,
            super::configuration::fetch_config(self.engine.as_ref(), &self.config_snapshot()),
        )
        .await
        {
            Ok(result) => result?,
            Err(elapsed) => {
                return Err(anyhow::Error::new(elapsed)
                    .context("configuration::get timed out; keeping previous config"));
            }
        };

        let old = self.config_snapshot();

        // Adapter-only config: if the effective adapter is unchanged there is
        // nothing else to apply. Publish the (byte-identical) value for parity.
        if Self::effective_adapter(&old) == Self::effective_adapter(&new_config) {
            self.set_config(new_config);
            return Ok(());
        }

        // FULL TRANSPORT HOT-SWAP. Build the new lock backend first (fallible); a
        // failure keeps the previous config, scheduler, and jobs.
        let new_scheduler = Self::resolve_scheduler(&self.engine, &new_config).await?;
        let old_adapter = self.adapter_snapshot();

        // Capture the live jobs BEFORE shutting the old scheduler down
        // (`shutdown` drains the job map). Then stop the old jobs and release
        // their locks so the old and new schedulers never both fire a job:
        // shutting down first trades a sub-second scheduling gap for the absence
        // of a double-fire window (the lock backends cannot coordinate with each
        // other mid-swap).
        let specs = old_adapter.job_specs().await;
        let new_adapter = Arc::new(CronAdapter::new(new_scheduler, self.engine.clone()));
        old_adapter.shutdown().await;

        // Re-register every job on the new transport (best-effort per job). The
        // expressions were validated at first registration, so this only fails on
        // an adapter that rejects setup — that job is logged and left unscheduled
        // while the rest apply.
        for spec in &specs {
            if let Err(err) = new_adapter
                .register_with_metadata(
                    &spec.id,
                    &spec.expression,
                    &spec.function_id,
                    spec.condition_function_id.clone(),
                    spec.metadata.clone(),
                    spec.namespace.clone(),
                )
                .await
            {
                tracing::error!(
                    job = %spec.id,
                    error = %err,
                    "iii-cron: failed to re-register job on the new adapter; job left unscheduled"
                );
            }
        }

        // Publish config + scheduler atomically so a concurrent trigger
        // registration sees a coherent generation.
        self.set_live(new_config, new_adapter);
        tracing::info!("iii-cron lock backend hot-swapped after configuration change");
        Ok(())
    }
}

#[async_trait]
impl Worker for CronWorker {
    fn name(&self) -> &'static str {
        "CronModule"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        // The cron worker registers no service functions of its own; the only
        // function is the config-change handler, registered here (inside the
        // worker scope) so destroy/reload track and remove it.
        self.register_config_handler(&engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing CronModule");

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
        // Shut the *current* scheduler down on the shutdown signal. Snapshot at
        // signal time (not now) so a hot-swap that replaced the scheduler shuts
        // down the live one, not a stale handle.
        let worker = self.clone();
        tokio::spawn(async move {
            let _ = shutdown_rx.changed().await;
            tracing::info!("CronModule received shutdown signal, stopping cron jobs");
            worker.adapter_snapshot().shutdown().await;
        });

        // Adopt the configuration worker as the runtime source of truth. Failures
        // degrade to the static config.yaml block so cron stays up. The bus call
        // is time-bounded: worker startup is awaited serially by the boot and
        // reload pipelines, so a hung `configuration::*` provider must not wedge
        // every other worker behind this one.
        let register = tokio::time::timeout(
            super::configuration::CONFIG_BUS_TIMEOUT,
            super::configuration::register_config(self.engine.as_ref(), self.seed.as_ref()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("configuration::register timed out"))
        .and_then(|result| result);
        if let Err(err) = register {
            tracing::warn!(
                error = %err,
                "iii-cron: configuration::register failed; continuing with static config"
            );
        }

        // Initial adoption: re-fetch the authoritative value and apply it
        // unconditionally, so a persisted adapter override (which may differ from
        // the config.yaml seed) takes effect at boot even if the trigger
        // subscription below fails. At this point the live config still equals
        // the build-time seed, so `apply_config` correctly detects the override
        // and hot-swaps. Failures keep the static config.
        if let Err(err) = self.apply_config().await {
            tracing::warn!(
                error = %err,
                "iii-cron: failed to apply persisted configuration at boot; continuing with static config"
            );
        }

        // Register the handler before the trigger so a configuration event can
        // never fan out to a missing function. On reload, `register_functions`
        // runs after this hook and re-registers the handler inside the worker
        // scope; the `get` check keeps the initial-boot path (where it already
        // ran) from logging a spurious overwrite.
        if self
            .engine
            .functions
            .get(
                crate::protocol::DEFAULT_NAMESPACE,
                super::configuration::CONFIG_FN_ID,
            )
            .is_none()
        {
            self.register_config_handler(&self.engine);
        }
        if let Err(err) = super::configuration::register_config_trigger(&self.engine).await {
            tracing::warn!(
                error = %err,
                "iii-cron: failed to watch configuration changes; hot-reload disabled"
            );
        } else {
            // Catch-up pass: replay any `configuration::set` that landed between
            // the adoption apply above and the trigger subscription. Routed
            // through `on_config_change` (not `apply_config` directly) so a
            // timed-out catch-up gets the same one-shot delayed retry.
            super::configuration::on_config_change(self).await;
        }

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying CronModule");
        // The configuration trigger is registered outside the worker scope, so
        // remove it explicitly to keep ReloadManager restarts duplicate-free.
        let _ = self
            .engine
            .trigger_registry
            .unregister_trigger(
                super::configuration::CONFIG_TRIGGER_ID.to_string(),
                Some(super::configuration::CONFIG_TRIGGER_TYPE.to_string()),
            )
            .await;

        // Serialize with any in-flight `apply_config` so a restart can't rebuild
        // a scheduler after we shut down below. Setting `destroyed` under the
        // lock makes every later apply bail before re-registering jobs.
        let _guard = self.apply_lock.lock().await;
        self.destroyed.store(true, Ordering::SeqCst);
        self.adapter_snapshot().shutdown().await;
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
        // Clone so the future owns its state and can serialize with apply_config.
        let worker = self.clone();

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

            // Serialize with `apply_config`, and snapshot the live scheduler only
            // after acquiring the lock: a concurrent lock-backend hot-swap would
            // otherwise shut down the adapter this registration targets, silently
            // stranding the job (its task receives the shutdown signal and exits,
            // yet the trigger registry still believes the job is live). Holding
            // the lock guarantees we register on the current generation — the
            // swap either sees this job in `job_specs()` (registered first) or
            // this registration sees the swapped-in adapter (registered after).
            let _guard = worker.apply_lock.lock().await;
            let adapter = worker.adapter_snapshot();
            adapter
                .register_with_metadata(
                    &trigger.id,
                    &cron_expression,
                    &trigger.function_id,
                    condition_function_id,
                    trigger.metadata.clone(),
                    trigger.namespace.clone(),
                )
                .await
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let worker = self.clone();
        Box::pin(async move {
            tracing::debug!(trigger_id = %trigger.id, "Unregistering cron trigger");
            // Serialize with `apply_config` so the unregister always targets the
            // live scheduler the job was migrated onto by a hot-swap, not a stale
            // (already shut-down) generation.
            let _guard = worker.apply_lock.lock().await;
            let adapter = worker.adapter_snapshot();
            adapter.unregister(&trigger.id).await
        })
    }
}

#[async_trait]
impl ConfigurableWorker for CronWorker {
    type Config = CronModuleConfig;
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
        // The config.yaml block seeds the first `configuration::register`; the
        // configuration entry is the runtime source of truth afterwards.
        let seed = Some(config.clone());
        Self {
            engine,
            live: Arc::new(RwLock::new(LiveState {
                config: Arc::new(config),
                adapter: Arc::new(cron_adapter),
            })),
            seed,
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
            destroyed: Arc::new(AtomicBool::new(false)),
        }
    }

    fn adapter_name_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.name.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

crate::register_worker!(
    "iii-cron",
    CronWorker,
    description = "Schedule functions with cron expressions.",
    enabled_by_default = true
);

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

    fn setup_cron_module() -> (Arc<Engine>, CronWorker) {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let scheduler: Arc<dyn CronSchedulerAdapter> = Arc::new(MockCronSchedulerAdapter);
        let config = super::super::config::CronModuleConfig::default();
        let module = CronWorker::build(engine.clone(), config, scheduler);
        (engine, module)
    }

    // =========================================================================
    // CronWorker::name
    // =========================================================================

    #[test]
    fn cron_module_name() {
        let (_engine, module) = setup_cron_module();
        assert_eq!(Worker::name(&module), "CronModule");
    }

    // =========================================================================
    // Worker::initialize test
    // =========================================================================

    #[tokio::test]
    async fn initialize_registers_cron_trigger_type() {
        let (engine, module) = setup_cron_module();
        let result = module.initialize().await;
        assert!(result.is_ok());
        assert!(engine.trigger_registry.trigger_types.contains_key("cron"));
    }

    // =========================================================================
    // Worker::destroy test
    // =========================================================================

    #[tokio::test]
    async fn destroy_calls_shutdown() {
        let (_engine, module) = setup_cron_module();
        let result = module.destroy().await;
        assert!(result.is_ok());
    }

    // =========================================================================
    // Worker::start_background_tasks test
    // =========================================================================

    #[tokio::test]
    async fn start_background_tasks_spawns_shutdown_listener() {
        let (_engine, module) = setup_cron_module();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let result = module.start_background_tasks(rx, tx.clone()).await;
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
            metadata: None,
            namespace: "default".to_string(),
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
            metadata: None,
            namespace: "default".to_string(),
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
            metadata: None,
            namespace: "default".to_string(),
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn register_trigger_with_condition_function_id() {
        let (_engine, module) = setup_cron_module();
        let trigger = crate::trigger::Trigger {
            id: "cron-trig-cond".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "expression": "0 30 * * * *",
                "condition_function_id": "test::condition_fn"
            }),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
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
            metadata: None,
            namespace: "default".to_string(),
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
            metadata: None,
            namespace: "default".to_string(),
        };
        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_err());
    }

    // =========================================================================
    // ConfigurableWorker trait tests
    // =========================================================================

    #[test]
    fn adapter_name_from_config_none() {
        let config = super::super::config::CronModuleConfig::default();
        assert!(CronWorker::adapter_name_from_config(&config).is_none());
    }

    #[test]
    fn adapter_name_from_config_some() {
        let config = super::super::config::CronModuleConfig {
            adapter: Some(crate::workers::traits::AdapterEntry {
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
        let config = super::super::config::CronModuleConfig::default();
        assert!(CronWorker::adapter_config_from_config(&config).is_none());
    }

    #[test]
    fn adapter_config_from_config_some() {
        let config = super::super::config::CronModuleConfig {
            adapter: Some(crate::workers::traits::AdapterEntry {
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
        let config = super::super::config::CronModuleConfig {
            adapter: Some(crate::workers::traits::AdapterEntry {
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
    fn build_creates_module() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let scheduler: Arc<dyn CronSchedulerAdapter> = Arc::new(MockCronSchedulerAdapter);
        let config = super::super::config::CronModuleConfig::default();
        let module = CronWorker::build(engine.clone(), config, scheduler);
        assert_eq!(Worker::name(&module), "CronModule");
    }

    // =========================================================================
    // Worker::register_functions (noop) test
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
            metadata: None,
            namespace: "default".to_string(),
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
            metadata: None,
            namespace: "default".to_string(),
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

    // =========================================================================
    // Configuration hot-swap / apply_config tests
    // =========================================================================

    fn stub_config_get(engine: &Arc<Engine>, value: Option<serde_json::Value>) {
        use crate::engine::{Handler, RegisterFunctionRequest};
        use crate::function::FunctionResult;
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "configuration::get".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |_input: serde_json::Value| {
                let value = value.clone();
                async move {
                    match value {
                        Some(v) => {
                            FunctionResult::Success(Some(json!({ "id": "iii-cron", "value": v })))
                        }
                        None => FunctionResult::Failure(crate::protocol::ErrorBody {
                            message: "not found".to_string(),
                            code: "NOT_FOUND".to_string(),
                            stacktrace: None,
                        }),
                    }
                }
            }),
        );
    }

    #[test]
    fn effective_adapter_fills_default_name() {
        let config = super::super::config::CronModuleConfig::default();
        assert_eq!(CronWorker::effective_adapter(&config).0, "kv");
    }

    #[tokio::test]
    async fn apply_config_is_a_noop_after_destroy() {
        let (_engine, module) = setup_cron_module();
        module
            .destroyed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        // No `configuration::get` is stubbed; a non-short-circuiting apply would
        // try to fetch and fail. The destroyed gate must return Ok first.
        assert!(module.apply_config().await.is_ok());
    }

    #[tokio::test]
    async fn apply_config_hot_swaps_adapter_and_reregisters_jobs() {
        let (engine, module) = setup_cron_module();
        // Stored value selects the `kv` adapter with a distinguishing config, so
        // the effective adapter differs from the default-None seed and the swap
        // path runs.
        stub_config_get(
            &engine,
            Some(json!({ "adapter": { "name": "kv", "config": { "lock_index": "swap-test" } } })),
        );

        // Multiple live jobs must ALL survive the swap onto the new transport —
        // the best-effort re-registration loop must not abort after one job.
        for id in ["swap-job-a", "swap-job-b"] {
            let trigger = crate::trigger::Trigger {
                id: id.to_string(),
                trigger_type: "cron".to_string(),
                function_id: "test::handler".to_string(),
                config: json!({ "expression": "0 0 * * * *" }),
                worker_id: None,
                metadata: None,
                namespace: "default".to_string(),
            };
            module
                .register_trigger(trigger)
                .await
                .expect("register job");
        }

        let before = module.adapter_snapshot();
        module.apply_config().await.expect("apply_config");
        let after = module.adapter_snapshot();

        assert!(
            !Arc::ptr_eq(&before, &after),
            "a lock-backend change must rebuild the scheduler instance"
        );
        assert_eq!(
            after.job_count().await,
            2,
            "every live cron job must be re-registered onto the new adapter"
        );
        assert_eq!(
            module
                .config_snapshot()
                .adapter
                .as_ref()
                .map(|a| a.name.as_str()),
            Some("kv"),
            "the live config must reflect the applied adapter"
        );
    }

    #[tokio::test]
    async fn apply_config_keeps_scheduler_when_adapter_unresolvable() {
        let (engine, module) = setup_cron_module();
        stub_config_get(
            &engine,
            Some(json!({ "adapter": { "name": "does-not-exist" } })),
        );

        let before = module.adapter_snapshot();
        let result = module.apply_config().await;

        assert!(
            result.is_err(),
            "an unregistered adapter must fail the apply"
        );
        assert!(
            Arc::ptr_eq(&before, &module.adapter_snapshot()),
            "a failed resolve must keep the previous scheduler"
        );
        assert!(
            module.config_snapshot().adapter.is_none(),
            "a failed resolve must keep the previous config"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn apply_config_timeout_surfaces_downcastable_elapsed() {
        let (engine, module) = setup_cron_module();
        // A `configuration::get` that never returns must surface as a
        // downcastable `Elapsed` so `on_config_change` can schedule its retry.
        {
            use crate::engine::{Handler, RegisterFunctionRequest};
            use crate::function::FunctionResult;
            engine.register_function_handler(
                RegisterFunctionRequest {
                    function_id: "configuration::get".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                Handler::new(move |_input: serde_json::Value| async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                    FunctionResult::Success(Some(json!({ "id": "iii-cron", "value": {} })))
                }),
            );
        }

        let err = module
            .apply_config()
            .await
            .expect_err("a hung provider must time out");
        assert!(
            err.downcast_ref::<tokio::time::error::Elapsed>().is_some(),
            "timeout must remain downcastable to Elapsed: {err}"
        );
    }
}
