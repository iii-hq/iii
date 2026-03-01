// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

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
        self.watcher.shutdown().await;
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

        tokio::spawn(async move {
            if let Err(err) = watcher.run().await {
                tracing::error!("Watcher failed: {:?}", err);
            }
        });

        let watcher = self.watcher.clone();
        tokio::spawn(async move {
            let _ = shutdown.changed().await;
            watcher.shutdown().await;
        });

        Ok(())
    }
}

crate::register_module!(
    "modules::shell::ExecModule",
    ExecCoreModule,
    enabled_by_default = false
);

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn exec_core_module_create_and_lifecycle_work() {
        let engine = Arc::new(Engine::new());
        let created = ExecCoreModule::create(
            engine.clone(),
            Some(json!({
                "watch": null,
                "exec": []
            })),
        )
        .await
        .expect("create exec module");
        assert_eq!(created.name(), "ExecModule");

        let module = ExecCoreModule {
            watcher: Exec::new(ExecConfig {
                watch: None,
                exec: Vec::new(),
            }),
        };
        assert_eq!(module.name(), "ExecModule");
        module.initialize().await.expect("initialize");
        module.register_functions(engine);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        module
            .start_background_tasks(shutdown_rx)
            .await
            .expect("start background tasks");
        let _ = shutdown_tx.send(true);
        tokio::time::sleep(Duration::from_millis(25)).await;

        module.destroy().await.expect("destroy");
    }
}
