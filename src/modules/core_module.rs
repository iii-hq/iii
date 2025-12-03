use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::engine::Engine;

#[async_trait::async_trait]
pub trait CoreModule: Send + Sync {
    /// Creates a module instance
    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>>
    where
        Self: Sized;

    /// Initializes the module
    async fn initialize(&self) -> anyhow::Result<()>;
}

pub type AdapterFactory<A> = Box<
    dyn Fn(
            Arc<Engine>,
            Option<Value>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Arc<A>>> + Send>>
        + Send
        + Sync,
>;

#[async_trait::async_trait]
pub trait ConfigurableModule: CoreModule + Sized + 'static {
    type Config: DeserializeOwned + Default + Send;
    type Adapter: Send + Sync + 'static + ?Sized;

    /// Build the module from parsed config and adapter
    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self;

    /// Default adapter class name
    fn default_adapter_class() -> &'static str;

    /// Extract adapter class from config (optional override)
    fn adapter_class_from_config(_config: &Self::Config) -> Option<String> {
        None
    }

    /// Extract adapter config from module config (optional override)
    fn adapter_config_from_config(_config: &Self::Config) -> Option<Value> {
        None
    }

    /// Create with typed adapters
    async fn create_with_adapters(
        engine: Arc<Engine>,
        config: Option<Value>,
        adapters: HashMap<String, AdapterFactory<Self::Adapter>>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        // 1. Parse config
        let parsed_config: Self::Config = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        // 2. Determine which adapter to use
        let adapter_class = Self::adapter_class_from_config(&parsed_config)
            .unwrap_or_else(|| Self::default_adapter_class().to_string());

        // 3. Get the factory
        let factory = adapters.get(&adapter_class).ok_or_else(|| {
            anyhow::anyhow!(
                "Adapter factory '{}' not found. Available: {:?}",
                adapter_class,
                adapters.keys().collect::<Vec<_>>()
            )
        })?;

        // 4. Create adapter
        let adapter_config = Self::adapter_config_from_config(&parsed_config);
        let adapter = factory(engine.clone(), adapter_config).await?;

        // 5. Build module
        Ok(Box::new(Self::build(engine, parsed_config, adapter)))
    }
}
