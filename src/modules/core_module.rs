use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
};

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

    /// Registers functions to the engine
    fn register_functions(&self, engine: Arc<Engine>);
}

pub type AdapterFactory<A> = Arc<
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
    const DEFAULT_ADAPTER_CLASS: &'static str;

    async fn register_adapter(
        name: impl Into<String> + Send,
        factory: AdapterFactory<Self::Adapter>,
    ) {
        let registry = Self::registry().await;
        let mut reg = registry.write().unwrap();
        reg.insert(name.into(), factory);
    }

    /// Build the registry map with adapter factories.
    /// This method should be implemented by the client to provide the adapter factories.
    fn build_registry() -> HashMap<String, AdapterFactory<Self::Adapter>>;

    /// Get the static registry. This method should be implemented by creating a static Lazy
    /// that calls `Self::build_registry()`. Example:
    /// ```ignore
    /// async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
    ///     static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn MyAdapter>>>> =
    ///         Lazy::new(|| RwLock::new(MyModule::build_registry()));
    ///     &REGISTRY
    /// }
    /// ```
    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>>;
    /// Build the module from parsed config and adapter
    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self;

    /// Default adapter class name
    fn default_adapter_class() -> &'static str {
        Self::DEFAULT_ADAPTER_CLASS
    }

    /// Extract adapter class from config (optional override)
    fn adapter_class_from_config(_config: &Self::Config) -> Option<String> {
        None
    }

    /// Extract adapter config from module config (optional override)
    fn adapter_config_from_config(_config: &Self::Config) -> Option<Value> {
        None
    }

    async fn get_adapter(name: &str) -> Option<AdapterFactory<Self::Adapter>> {
        let registry = Self::registry().await;
        let map = registry.read().unwrap();
        map.get(name).cloned()
    }

    /// Helper function to create an adapter factory from a closure
    fn make_adapter_factory<F, Fut>(create_fn: F) -> AdapterFactory<Self::Adapter>
    where
        F: Fn(Arc<Engine>, Option<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<Arc<Self::Adapter>>> + Send + 'static,
    {
        Arc::new(move |engine, config| Box::pin(create_fn(engine, config)))
    }

    /// Helper function to register a new adapter factory from a closure.
    /// This is a convenience method that combines `make_adapter_factory` and `register_adapter`.
    ///
    /// # Example
    /// ```ignore
    /// EventCoreModule::add_adapter("my_adapter", |engine, config| async move {
    ///     Ok(Arc::new(MyAdapter::new(engine).await?) as Arc<dyn EventAdapter>)
    /// }).await;
    /// ```
    async fn add_adapter<F, Fut>(name: impl Into<String> + Send, create_fn: F) -> anyhow::Result<()>
    where
        F: Fn(Arc<Engine>, Option<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<Arc<Self::Adapter>>> + Send + 'static,
    {
        let factory = Self::make_adapter_factory(create_fn);
        Self::register_adapter(name, factory).await;
        Ok(())
    }
    /// Create with typed adapters
    async fn create_with_adapters(
        engine: Arc<Engine>,
        config: Option<Value>,
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
        let factory = match Self::get_adapter(&adapter_class).await {
            Some(factory) => factory,
            None => {
                let registry = Self::registry().await;
                let available: Vec<String> = registry.read().unwrap().keys().cloned().collect();
                return Err(anyhow::anyhow!(
                    "Adapter factory '{}' not found. Available: {:?}",
                    adapter_class,
                    available
                ));
            }
        };

        tracing::info!("Using adapter class '{}'", adapter_class);
        // 4. Create adapter
        let adapter_config = Self::adapter_config_from_config(&parsed_config);
        let adapter = factory(engine.clone(), adapter_config).await?;

        // 5. Build module
        Ok(Box::new(Self::build(engine, parsed_config, adapter)))
    }
}
