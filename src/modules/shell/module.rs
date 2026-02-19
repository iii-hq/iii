// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use anyhow::Ok;
use serde_json::Value;

use super::{config::ExecConfig, exec::Exec};
use crate::{engine::Engine, modules::module::Module};

#[derive(Clone)]
pub struct ExecCoreModule {
    watcher: Exec,
}

#[async_trait::async_trait]
impl Module for ExecCoreModule {
    fn name(&self) -> &'static str {
        "ExecModule"
    }
    async fn create(
        _engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Module>> {
        let config: ExecConfig = config.map(serde_json::from_value).transpose()?.unwrap();
        let watcher = Exec::new(config);

        Ok(Box::new(Self { watcher }))
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        self.watcher.stop_process().await;
        Ok(())
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let watcher = self.watcher.clone();

        let watcher_for_shutdown = watcher.clone();
        tokio::spawn(async move {
            tokio::select! {
                result = watcher.run() => {
                    if let Err(err) = result {
                        tracing::error!("Watcher failed: {:?}", err);
                    }
                }
                _ = shutdown.changed() => {
                    tracing::info!("ExecModule received shutdown signal, stopping process");
                    watcher_for_shutdown.stop_process().await;
                }
            }
        });

        Ok(())
    }
}

crate::register_module!(
    "modules::shell::ExecModule",
    ExecCoreModule,
    enabled_by_default = false
);
