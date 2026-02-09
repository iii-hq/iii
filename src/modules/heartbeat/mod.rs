// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

mod collector;
mod config;
mod store;
mod telemetry;

use std::sync::Arc;

use async_trait::async_trait;
use colored::Colorize;
use function_macros::{function, service};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::module::Module,
    protocol::ErrorBody,
};

use self::collector::Collector;
use self::config::HeartbeatConfig;
use self::store::HeartbeatStore;

#[derive(Serialize, Deserialize, Default)]
pub struct HeartbeatStatusInput {}

#[derive(Serialize, Deserialize, Default)]
pub struct HeartbeatHistoryInput {
    pub limit: Option<usize>,
}

#[derive(Clone)]
pub struct HeartbeatModule {
    config: HeartbeatConfig,
    _engine: Arc<Engine>,
    collector: Arc<Collector>,
    store: Arc<HeartbeatStore>,
}

#[service(name = "heartbeat")]
impl HeartbeatModule {
    #[function(
        id = "engine.heartbeat.status",
        description = "Get the current heartbeat data snapshot"
    )]
    pub async fn status(
        &self,
        _input: HeartbeatStatusInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let entry = self.collector.collect();
        match serde_json::to_value(&entry) {
            Ok(val) => FunctionResult::Success(Some(val)),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to serialize heartbeat: {}", e),
                code: String::new(),
            }),
        }
    }

    #[function(
        id = "engine.heartbeat.history",
        description = "Get historical heartbeat entries"
    )]
    pub async fn history(
        &self,
        input: HeartbeatHistoryInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let entries = self.store.history(input.limit).await;
        match serde_json::to_value(&entries) {
            Ok(val) => FunctionResult::Success(Some(serde_json::json!({
                "count": entries.len(),
                "entries": val,
            }))),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to serialize history: {}", e),
                code: String::new(),
            }),
        }
    }
}

#[async_trait]
impl Module for HeartbeatModule {
    fn name(&self) -> &'static str {
        "HeartbeatModule"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        let hb_config: HeartbeatConfig = match config {
            Some(cfg) => serde_json::from_value(cfg)?,
            None => HeartbeatConfig::default(),
        };

        let instance_id = hb_config.get_instance_id();
        let store = Arc::new(HeartbeatStore::new(hb_config.max_history_size()));
        let collector = Arc::new(Collector::new(
            engine.clone(),
            instance_id.clone(),
            Vec::new(),
        ));

        tracing::info!(
            "{} HeartbeatModule created (instance_id={}, interval={}s)",
            "[HEARTBEAT]".cyan(),
            &instance_id[..8],
            hb_config.interval(),
        );

        Ok(Box::new(HeartbeatModule {
            config: hb_config,
            _engine: engine,
            collector,
            store,
        }))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!(
            "{} Heartbeat module initialized (functions: engine.heartbeat.{{status,history}})",
            "[READY]".green()
        );
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        if !self.config.is_enabled() {
            tracing::info!(
                "{} Heartbeat disabled, skipping background tasks",
                "[HEARTBEAT]".cyan()
            );
            return Ok(());
        }

        let interval_secs = self.config.interval();
        let collector = self.collector.clone();
        let store = self.store.clone();
        let cloud_endpoint = self.config.cloud_endpoint.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));

            tracing::debug!(
                "[HEARTBEAT] Background heartbeat task started (interval={}s)",
                interval_secs
            );

            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            tracing::debug!("[HEARTBEAT] Heartbeat task shutting down");
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        let entry = collector.collect();
                        store.push(entry.clone()).await;

                        tracing::debug!(
                            "{} heartbeat collected (uptime={}s, workers={}, functions={}, invocations={})",
                            "[HEARTBEAT]".cyan(),
                            entry.uptime_seconds,
                            entry.runtime.workers_active,
                            entry.registration.functions.count,
                            entry.runtime.invocations_total,
                        );

                        if let Some(ref endpoint) = cloud_endpoint
                            && telemetry::is_telemetry_enabled(&config)
                        {
                            tokio::spawn({
                                let endpoint = endpoint.clone();
                                let entry = entry.clone();
                                async move {
                                    telemetry::report_to_cloud(&endpoint, &entry).await;
                                }
                            });
                        }
                    }
                }
            }

            tracing::debug!("[HEARTBEAT] Heartbeat task stopped");
        });

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!(
            "{} HeartbeatModule shutting down (history_entries={})",
            "[HEARTBEAT]".cyan(),
            self.store.len().await,
        );
        Ok(())
    }
}

crate::register_module!(
    "modules::heartbeat::HeartbeatModule",
    HeartbeatModule,
    enabled_by_default = false
);
