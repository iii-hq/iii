// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    engine::Engine,
    modules::registry::{AdapterRegistrationEntry, ModuleFuture},
};

// use across modules
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdapterEntry {
    pub class: String,
    #[serde(default)]
    pub config: Option<Value>,
}

#[async_trait::async_trait]
pub trait Module: Send + Sync {
    fn name(&self) -> &'static str;
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>>
    where
        Self: Sized;

    fn make_module(engine: Arc<Engine>, config: Option<Value>) -> ModuleFuture
    where
        Self: Sized + 'static,
    {
        Self::create(engine, config)
    }

    /// Initializes the module
    async fn initialize(&self) -> anyhow::Result<()>;

    async fn start_background_tasks(
        &self,
        _shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying module: {}", self.name());
        Ok(())
    }

    /// Registers functions to the engine
    #[allow(unused_variables)]
    fn register_functions(&self, engine: Arc<Engine>) {
        // blank implementation since it going to be overriden by the macros
    }
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
pub trait ConfigurableModule: Module + Sized + 'static {
    type Config: DeserializeOwned + Default + Send;
    type Adapter: Send + Sync + 'static + ?Sized;
    type AdapterRegistration: AdapterRegistrationEntry<Self::Adapter> + inventory::Collect;
    const DEFAULT_ADAPTER_CLASS: &'static str;

    async fn register_adapter(
        name: impl Into<String> + Send,
        factory: AdapterFactory<Self::Adapter>,
    ) {
        let registry = Self::registry().await;
        let mut reg = registry.write().unwrap();
        reg.insert(name.into(), factory);
    }

    /// Build the registry map with adapter factories from the inventory registry.
    fn build_registry() -> HashMap<String, AdapterFactory<Self::Adapter>> {
        let mut registry = HashMap::new();
        for registration in inventory::iter::<Self::AdapterRegistration> {
            let factory = registration.factory();
            let adapter_factory: AdapterFactory<Self::Adapter> =
                Arc::new(move |engine, config| (factory)(engine, config));
            registry.insert(registration.class().to_string(), adapter_factory);
        }
        registry
    }

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
    /// QueueCoreModule::add_adapter("my_adapter", |engine, config| async move {
    ///     Ok(Arc::new(MyAdapter::new(engine).await?) as Arc<dyn QueueAdapter>)
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
    ) -> anyhow::Result<Box<dyn Module>> {
        // 1. Parse config
        let parsed_config: Self::Config = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        // 2. Determine which adapter to use
        let adapter_class = match Self::adapter_class_from_config(&parsed_config) {
            Some(class) => class,
            None => {
                tracing::debug!(
                    "No adapter class specified in config, using default: '{}'",
                    Self::default_adapter_class()
                );
                Self::default_adapter_class().to_string()
            }
        };
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

        // 4. Create adapter
        let adapter_config = Self::adapter_config_from_config(&parsed_config);
        tracing::debug!(
            "Using adapter class '{}' with config: {:?}",
            adapter_class,
            &adapter_config
        );
        let adapter = factory(engine.clone(), adapter_config).await?;

        // 5. Build module
        Ok(Box::new(Self::build(engine, parsed_config, adapter)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_adapter_entry_serde_round_trip() {
        let entry = AdapterEntry {
            class: "test::Adapter".to_string(),
            config: Some(json!({"key": "value"})),
        };
        let json = serde_json::to_value(&entry).unwrap();
        let deserialized: AdapterEntry = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.class, "test::Adapter");
        assert_eq!(deserialized.config, Some(json!({"key": "value"})));
    }

    #[test]
    fn test_adapter_entry_serde_without_config() {
        let entry = AdapterEntry {
            class: "test::Adapter".to_string(),
            config: None,
        };
        let json = serde_json::to_value(&entry).unwrap();
        let deserialized: AdapterEntry = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.class, "test::Adapter");
        assert_eq!(deserialized.config, None);
    }

    #[test]
    fn test_adapter_entry_serde_from_json() {
        let json = json!({
            "class": "test::Adapter",
            "config": {"key": "value"}
        });
        let entry: AdapterEntry = serde_json::from_value(json).unwrap();
        assert_eq!(entry.class, "test::Adapter");
        assert_eq!(entry.config, Some(json!({"key": "value"})));
    }

    #[test]
    fn test_adapter_entry_serde_from_json_without_config() {
        let json = json!({
            "class": "test::Adapter"
        });
        let entry: AdapterEntry = serde_json::from_value(json).unwrap();
        assert_eq!(entry.class, "test::Adapter");
        assert_eq!(entry.config, None);
    }

    #[tokio::test]
    async fn test_module_trait_start_background_tasks_default() {
        use async_trait::async_trait;

        struct TestModule;
        #[async_trait]
        impl Module for TestModule {
            fn name(&self) -> &'static str {
                "TestModule"
            }
            async fn create(_engine: Arc<Engine>, _config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
                Ok(Box::new(TestModule))
            }
            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let module = TestModule;
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let result = module.start_background_tasks(rx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_module_trait_destroy_default() {
        use async_trait::async_trait;

        struct TestModule;
        #[async_trait]
        impl Module for TestModule {
            fn name(&self) -> &'static str {
                "TestModule"
            }
            async fn create(_engine: Arc<Engine>, _config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
                Ok(Box::new(TestModule))
            }
            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let module = TestModule;
        let result = module.destroy().await;
        assert!(result.is_ok());
    }
}
