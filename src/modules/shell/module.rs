use std::sync::Arc;

use anyhow::Ok;
use serde_json::Value;

use super::{config::ExecConfig, exec::Exec};
use crate::{engine::Engine, modules::core_module::CoreModule};

#[derive(Clone)]
pub struct ExecCoreModule {
    config: ExecConfig,
}

#[async_trait::async_trait]
impl CoreModule for ExecCoreModule {
    async fn create(
        _engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        let config: ExecConfig = config.map(serde_json::from_value).transpose()?.unwrap();

        Ok(Box::new(Self { config }))
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> anyhow::Result<()> {
        let watcher = Exec::new(self.config.clone());

        tokio::spawn(async move {
            if let Err(err) = watcher.run().await {
                tracing::error!("Watcher failed: {:?}", err);
            }

            Ok(())
        });

        Ok(())
    }
}

crate::register_module!(
    "modules::shell::ExecModule",
    <ExecCoreModule as CoreModule>::make_module,
    enabled_by_default = false
);
