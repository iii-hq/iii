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
    /// Create with typed adapters.
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
    use std::sync::Mutex;

    use once_cell::sync::Lazy;
    use serial_test::serial;

    use super::*;
    use crate::modules::registry::{AdapterFuture, AdapterRegistration};

    trait TestAdapter: Send + Sync {
        fn adapter_name(&self) -> String;
        fn adapter_config(&self) -> Option<Value>;
    }

    type TestAdapterRegistration = AdapterRegistration<dyn TestAdapter>;
    inventory::collect!(TestAdapterRegistration);

    fn inventory_test_factory(
        _engine: Arc<Engine>,
        config: Option<Value>,
    ) -> AdapterFuture<dyn TestAdapter> {
        Box::pin(async move {
            Ok(Arc::new(RecordingAdapter {
                name: "inventory".to_string(),
                config,
            }) as Arc<dyn TestAdapter>)
        })
    }

    inventory::submit! {
        TestAdapterRegistration {
            class: "test::inventory",
            factory: inventory_test_factory,
        }
    }

    #[derive(Debug)]
    struct RecordingAdapter {
        name: String,
        config: Option<Value>,
    }

    impl TestAdapter for RecordingAdapter {
        fn adapter_name(&self) -> String {
            self.name.clone()
        }

        fn adapter_config(&self) -> Option<Value> {
            self.config.clone()
        }
    }

    #[derive(Clone, Debug, Default, Deserialize, Serialize)]
    struct TestModuleConfig {
        #[serde(default)]
        adapter: Option<AdapterEntry>,
    }

    #[derive(Clone)]
    struct TestConfigurableModule {
        _engine: Arc<Engine>,
        _config: TestModuleConfig,
        _adapter: Arc<dyn TestAdapter>,
    }

    static FACTORY_CALLS: Lazy<Mutex<Vec<(String, Option<Value>)>>> =
        Lazy::new(|| Mutex::new(Vec::new()));

    #[async_trait::async_trait]
    impl Module for TestConfigurableModule {
        fn name(&self) -> &'static str {
            "TestConfigurableModule"
        }

        async fn create(
            engine: Arc<Engine>,
            config: Option<Value>,
        ) -> anyhow::Result<Box<dyn Module>> {
            Self::create_with_adapters(engine, config).await
        }

        async fn initialize(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ConfigurableModule for TestConfigurableModule {
        type Config = TestModuleConfig;
        type Adapter = dyn TestAdapter;
        type AdapterRegistration = TestAdapterRegistration;
        const DEFAULT_ADAPTER_CLASS: &'static str = "test::default";

        async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
            static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn TestAdapter>>>> =
                Lazy::new(|| RwLock::new(HashMap::new()));
            &REGISTRY
        }

        fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
            Self {
                _engine: engine,
                _config: config,
                _adapter: adapter,
            }
        }

        fn adapter_class_from_config(config: &Self::Config) -> Option<String> {
            config.adapter.as_ref().map(|entry| entry.class.clone())
        }

        fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
            config
                .adapter
                .as_ref()
                .and_then(|entry| entry.config.clone())
        }
    }

    struct SimpleModule;

    #[async_trait::async_trait]
    impl Module for SimpleModule {
        fn name(&self) -> &'static str {
            "SimpleModule"
        }

        async fn create(
            _engine: Arc<Engine>,
            _config: Option<Value>,
        ) -> anyhow::Result<Box<dyn Module>> {
            Ok(Box::new(SimpleModule))
        }

        async fn initialize(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[derive(Clone, Debug, Default, Deserialize, Serialize)]
    struct DefaultHooksConfig {
        enabled: bool,
    }

    #[derive(Clone)]
    struct DefaultHooksModule {
        _engine: Arc<Engine>,
        _config: DefaultHooksConfig,
        _adapter: Arc<dyn TestAdapter>,
    }

    #[async_trait::async_trait]
    impl Module for DefaultHooksModule {
        fn name(&self) -> &'static str {
            "DefaultHooksModule"
        }

        async fn create(
            engine: Arc<Engine>,
            config: Option<Value>,
        ) -> anyhow::Result<Box<dyn Module>> {
            Self::create_with_adapters(engine, config).await
        }

        async fn initialize(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ConfigurableModule for DefaultHooksModule {
        type Config = DefaultHooksConfig;
        type Adapter = dyn TestAdapter;
        type AdapterRegistration = TestAdapterRegistration;
        const DEFAULT_ADAPTER_CLASS: &'static str = "test::inventory";

        async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
            static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn TestAdapter>>>> =
                Lazy::new(|| RwLock::new(DefaultHooksModule::build_registry()));
            &REGISTRY
        }

        fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
            Self {
                _engine: engine,
                _config: config,
                _adapter: adapter,
            }
        }
    }

    async fn clear_test_registry() {
        TestConfigurableModule::registry()
            .await
            .write()
            .unwrap()
            .clear();
        FACTORY_CALLS.lock().expect("lock factory calls").clear();
    }

    fn recording_factory(name: &'static str) -> AdapterFactory<dyn TestAdapter> {
        TestConfigurableModule::make_adapter_factory(move |_engine, config| async move {
            FACTORY_CALLS
                .lock()
                .expect("lock factory calls")
                .push((name.to_string(), config.clone()));
            Ok(Arc::new(RecordingAdapter {
                name: name.to_string(),
                config,
            }) as Arc<dyn TestAdapter>)
        })
    }

    // =========================================================================
    // AdapterEntry serialization/deserialization
    // =========================================================================

    #[test]
    fn adapter_entry_deserialize_with_config() {
        let json = r#"{"class": "my::Adapter", "config": {"key": "value"}}"#;
        let entry: AdapterEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.class, "my::Adapter");
        assert!(entry.config.is_some());
        assert_eq!(entry.config.unwrap()["key"], "value");
    }

    #[test]
    fn adapter_entry_deserialize_without_config() {
        let json = r#"{"class": "my::Adapter"}"#;
        let entry: AdapterEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.class, "my::Adapter");
        assert!(entry.config.is_none());
    }

    #[test]
    fn adapter_entry_serialize_roundtrip() {
        let entry = AdapterEntry {
            class: "test::Adapter".to_string(),
            config: Some(serde_json::json!({"port": 8080})),
        };
        let json_str = serde_json::to_string(&entry).unwrap();
        let deserialized: AdapterEntry = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.class, "test::Adapter");
        assert_eq!(deserialized.config.unwrap()["port"], 8080);
    }

    #[test]
    fn adapter_entry_serialize_no_config() {
        let entry = AdapterEntry {
            class: "test::Adapter".to_string(),
            config: None,
        };
        let json_str = serde_json::to_string(&entry).unwrap();
        let deserialized: AdapterEntry = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.class, "test::Adapter");
        assert!(deserialized.config.is_none());
    }

    #[test]
    fn adapter_entry_clone() {
        let entry = AdapterEntry {
            class: "test::Adapter".to_string(),
            config: Some(serde_json::json!({"a": 1})),
        };
        let cloned = entry.clone();
        assert_eq!(cloned.class, "test::Adapter");
        assert_eq!(cloned.config, Some(serde_json::json!({"a": 1})));
    }

    #[test]
    fn adapter_entry_debug() {
        let entry = AdapterEntry {
            class: "test::Adapter".to_string(),
            config: None,
        };
        let debug_str = format!("{:?}", entry);
        assert!(debug_str.contains("test::Adapter"));
    }

    #[tokio::test]
    async fn module_default_trait_methods_work() {
        let engine = Arc::new(Engine::new());
        let module = SimpleModule::make_module(engine, None)
            .await
            .expect("make module should succeed");

        assert_eq!(module.name(), "SimpleModule");

        let (_tx, rx) = tokio::sync::watch::channel(false);
        module
            .start_background_tasks(rx)
            .await
            .expect("default background tasks should succeed");
        module
            .destroy()
            .await
            .expect("default destroy should succeed");
    }

    #[tokio::test]
    async fn module_register_functions_default_is_noop() {
        let engine = Arc::new(Engine::new());
        let module = SimpleModule;
        module.register_functions(engine.clone());
        assert!(engine.functions.get("missing").is_none());
    }

    #[tokio::test]
    async fn module_default_trait_methods_on_concrete_type_work() {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let module = SimpleModule;

        module
            .start_background_tasks(rx)
            .await
            .expect("default background tasks should succeed");
        module
            .destroy()
            .await
            .expect("default destroy should succeed");
    }

    #[tokio::test]
    async fn make_adapter_factory_wraps_custom_closure() {
        let engine = Arc::new(Engine::new());
        let factory = TestConfigurableModule::make_adapter_factory(|_engine, config| async move {
            Ok(Arc::new(RecordingAdapter {
                name: "inline".to_string(),
                config,
            }) as Arc<dyn TestAdapter>)
        });

        let adapter = factory(engine, Some(serde_json::json!({ "from": "factory" })))
            .await
            .expect("factory should create adapter");
        assert_eq!(adapter.adapter_name(), "inline");
        assert_eq!(
            adapter.adapter_config(),
            Some(serde_json::json!({ "from": "factory" }))
        );
    }

    #[tokio::test]
    #[serial]
    async fn register_and_get_adapter_work() {
        clear_test_registry().await;

        TestConfigurableModule::register_adapter("test::default", recording_factory("default"))
            .await;

        assert_eq!(
            TestConfigurableModule::default_adapter_class(),
            "test::default"
        );
        assert!(
            TestConfigurableModule::get_adapter("test::default")
                .await
                .is_some()
        );
        assert!(
            TestConfigurableModule::get_adapter("missing")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    #[serial]
    async fn configurable_module_helper_defaults_are_accessible_directly() {
        clear_test_registry().await;

        let inventory_registry = DefaultHooksModule::build_registry();
        assert!(inventory_registry.contains_key("test::inventory"));
        assert_eq!(
            DefaultHooksModule::default_adapter_class(),
            "test::inventory"
        );
        assert!(
            DefaultHooksModule::get_adapter("test::inventory")
                .await
                .is_some()
        );

        TestConfigurableModule::add_adapter("test::default", |_engine, config| async move {
            Ok(Arc::new(RecordingAdapter {
                name: "added-direct".to_string(),
                config,
            }) as Arc<dyn TestAdapter>)
        })
        .await
        .expect("add adapter should succeed");

        let factory = TestConfigurableModule::get_adapter("test::default")
            .await
            .expect("registered adapter factory");
        let adapter = factory(
            Arc::new(Engine::new()),
            Some(serde_json::json!({ "ok": true })),
        )
        .await
        .expect("factory should create adapter");
        assert_eq!(adapter.adapter_name(), "added-direct");
        assert_eq!(
            adapter.adapter_config(),
            Some(serde_json::json!({ "ok": true }))
        );
    }

    #[tokio::test]
    #[serial]
    async fn create_with_adapters_uses_default_and_custom_configured_adapter() {
        clear_test_registry().await;
        let engine = Arc::new(Engine::new());

        TestConfigurableModule::register_adapter("test::default", recording_factory("default"))
            .await;
        TestConfigurableModule::register_adapter("test::custom", recording_factory("custom")).await;

        let default_module = TestConfigurableModule::create_with_adapters(engine.clone(), None)
            .await
            .expect("create with default adapter");
        assert_eq!(default_module.name(), "TestConfigurableModule");

        let configured = TestModuleConfig {
            adapter: Some(AdapterEntry {
                class: "test::custom".to_string(),
                config: Some(serde_json::json!({ "mode": "custom" })),
            }),
        };
        assert_eq!(
            TestConfigurableModule::adapter_class_from_config(&configured).as_deref(),
            Some("test::custom")
        );
        assert_eq!(
            TestConfigurableModule::adapter_config_from_config(&configured),
            Some(serde_json::json!({ "mode": "custom" }))
        );

        let custom_module = TestConfigurableModule::create_with_adapters(
            engine,
            Some(serde_json::to_value(configured).expect("serialize test config")),
        )
        .await
        .expect("create with custom adapter");
        assert_eq!(custom_module.name(), "TestConfigurableModule");

        let calls = FACTORY_CALLS.lock().expect("lock factory calls").clone();
        assert_eq!(
            calls,
            vec![
                ("default".to_string(), None),
                (
                    "custom".to_string(),
                    Some(serde_json::json!({ "mode": "custom" }))
                ),
            ]
        );
    }

    #[tokio::test]
    #[serial]
    async fn add_adapter_and_missing_adapter_error_are_reported() {
        clear_test_registry().await;
        let engine = Arc::new(Engine::new());

        TestConfigurableModule::add_adapter("test::default", |_engine, config| async move {
            FACTORY_CALLS
                .lock()
                .expect("lock factory calls")
                .push(("added".to_string(), config.clone()));
            Ok(Arc::new(RecordingAdapter {
                name: "added".to_string(),
                config,
            }) as Arc<dyn TestAdapter>)
        })
        .await
        .expect("add adapter should succeed");

        let created = TestConfigurableModule::create_with_adapters(engine.clone(), None)
            .await
            .expect("create with added adapter");
        assert_eq!(created.name(), "TestConfigurableModule");

        let missing = TestConfigurableModule::create_with_adapters(
            engine,
            Some(serde_json::json!({
                "adapter": { "class": "test::missing" }
            })),
        )
        .await;
        let error = missing.err().expect("missing adapter should fail");
        assert!(error.to_string().contains("test::missing"));
        assert!(error.to_string().contains("test::default"));
    }

    #[tokio::test]
    async fn build_registry_and_default_hooks_cover_inventory_path() {
        let registry = DefaultHooksModule::build_registry();
        assert!(registry.contains_key("test::inventory"));

        let module = DefaultHooksModule::create_with_adapters(
            Arc::new(Engine::new()),
            Some(serde_json::json!({ "enabled": true })),
        )
        .await
        .expect("create module from inventory-backed registry");
        assert_eq!(module.name(), "DefaultHooksModule");
        assert_eq!(
            DefaultHooksModule::adapter_class_from_config(&DefaultHooksConfig::default()),
            None
        );
        assert_eq!(
            DefaultHooksModule::adapter_config_from_config(&DefaultHooksConfig::default()),
            None
        );
    }

    #[tokio::test]
    #[serial]
    async fn create_with_adapters_reports_parse_and_factory_errors() {
        clear_test_registry().await;

        TestConfigurableModule::add_adapter("test::failing", |_engine, _config| async move {
            Err(anyhow::anyhow!("adapter factory failed"))
        })
        .await
        .expect("register failing adapter");

        let parse_error = match TestConfigurableModule::create_with_adapters(
            Arc::new(Engine::new()),
            Some(serde_json::json!(["not", "an", "object"])),
        )
        .await
        {
            Ok(_) => panic!("invalid config shape should fail"),
            Err(err) => err,
        };
        assert!(parse_error.to_string().contains("invalid type"));

        let factory_error = match TestConfigurableModule::create_with_adapters(
            Arc::new(Engine::new()),
            Some(serde_json::json!({
                "adapter": { "class": "test::failing" }
            })),
        )
        .await
        {
            Ok(_) => panic!("failing adapter should bubble up"),
            Err(err) => err,
        };
        assert!(factory_error.to_string().contains("adapter factory failed"));
    }
}
