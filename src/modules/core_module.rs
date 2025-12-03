use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::engine::Engine;

#[async_trait::async_trait]
pub trait CoreModule: Send + Sync {
    /// Creates a module instance
    /// here is where you can  setup your custom adapter based on the config
    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>>
    where
        Self: Sized;

    /// Initializes the module
    async fn initialize(&self) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
pub trait AdapterConfig: DeserializeOwned + Default {
    type Adapter: Send + Sync + 'static + ?Sized;

    async fn build_adapter(&self, engine: Arc<Engine>) -> anyhow::Result<Arc<Self::Adapter>>;
}
