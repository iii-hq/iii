// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::{HashMap, HashSet},
    env,
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
};

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use super::{module::Module, registry::ModuleRegistration};
use crate::engine::Engine;

// =============================================================================
// EngineConfig (YAML structure)
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct EngineConfig {
    #[serde(default)]
    pub modules: Vec<ModuleEntry>,
    #[serde(default)]
    pub workers: Vec<ModuleEntry>,
}

impl EngineConfig {
    pub fn default_modules(self) -> Self {
        let modules = default_module_entries();

        Self {
            modules,
            workers: Vec::new(),
        }
    }

    pub(crate) fn expand_env_vars(yaml_content: &str) -> String {
        let re = Regex::new(r"\$\{([^}:]+)(?::([^}]*))?\}").unwrap();

        re.replace_all(yaml_content, |caps: &regex::Captures| {
            let var_name = &caps[1];
            let default_value = caps.get(2).map(|m| m.as_str());

            match env::var(var_name) {
                Ok(value) => value,
                Err(_) => match default_value {
                    Some(default) => default.to_string(),
                    None => {
                        tracing::error!(
                            "Environment variable '{}' not set and no
    default provided",
                            var_name
                        );
                        panic!(
                            "Environment variable '{}' not set and no default provided",
                            var_name
                        );
                    }
                },
            }
        })
        .to_string()
    }

    /// Loads config strictly from the given file path.
    /// Returns a clear error if the file does not exist or cannot be parsed.
    pub fn config_file(path: &str) -> anyhow::Result<Self> {
        let yaml_content = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "Config file not found: '{}'.\n\
                     Hint: create a config.yaml or pass --use-default-config to run with defaults.",
                    path
                )
            } else {
                anyhow::anyhow!("Failed to read config file '{}': {}", path, e)
            }
        })?;
        let yaml_content = Self::expand_env_vars(&yaml_content);
        serde_yaml::from_str(&yaml_content)
            .map_err(|e| anyhow::anyhow!("Failed to parse config file '{}': {}", path, e))
    }

    /// Returns a config with default port and default modules (from inventory).
    /// Use this when explicitly opting in to run without a config file.
    pub fn default_config() -> Self {
        tracing::info!("Using default config (no config file)");
        Self {
            modules: default_module_entries(),
            workers: Vec::new(),
        }
    }
}

fn default_module_entries() -> Vec<ModuleEntry> {
    inventory::iter::<ModuleRegistration>
        .into_iter()
        .filter(|registration| registration.is_default)
        .map(|registration| ModuleEntry {
            class: registration.class.to_string(),
            config: None,
        })
        .collect()
}

#[derive(Debug, Deserialize)]
pub struct ModuleEntry {
    pub class: String,
    #[serde(default)]
    pub config: Option<Value>,
}

// =============================================================================
// Type Aliases for Factories
// =============================================================================

/// Factory function type for creating Modules (async)
type ModuleFactory = Arc<
    dyn Fn(
            Arc<Engine>,
            Option<Value>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Box<dyn Module>>> + Send>>
        + Send
        + Sync,
>;

/// Info about a registered module
struct ModuleInfo {
    factory: ModuleFactory,
}

// =============================================================================
// ModuleRegistry (unified registry for modules and adapters)
// =============================================================================

pub struct ModuleRegistry {
    module_factories: RwLock<HashMap<String, ModuleInfo>>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self {
            module_factories: RwLock::new(HashMap::new()),
        }
    }

    fn register_from_inventory(&self) {
        for registration in inventory::iter::<ModuleRegistration> {
            let factory = registration.factory;
            let info = ModuleInfo {
                factory: Arc::new(move |engine, config| (factory)(engine, config)),
            };
            self.module_factories
                .write()
                .expect("RwLock poisoned")
                .insert(registration.class.to_string(), info);
        }
    }

    // =========================================================================
    // Module Registration
    // =========================================================================

    /// Registers a module by type
    ///
    /// The module must implement `Module`. The registry uses `M::create()` to create instances.
    pub fn register<M: Module + 'static>(&self, class: &str) {
        let info = ModuleInfo {
            factory: Arc::new(|engine, config| Box::pin(M::create(engine, config))),
        };

        self.module_factories
            .write()
            .expect("RwLock poisoned")
            .insert(class.to_string(), info);
    }

    /// Creates a module instance.
    ///
    /// First checks the built-in registry. If the class is not found, falls back
    /// to external module resolution: checks `iii.toml` for installed modules and
    /// spawns the corresponding binary from `iii_modules/`.
    pub async fn create_module(
        self: &Arc<Self>,
        class: &str,
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Module>> {
        // Try built-in registry first
        let factory = {
            let factories = self.module_factories.read().expect("RwLock poisoned");
            factories.get(class).map(|info| info.factory.clone())
        };

        if let Some(factory) = factory {
            return factory(engine, config).await;
        }

        // Fallback: try external module from iii_modules/
        if let Some(info) = super::external::resolve_external_module(class) {
            tracing::info!(
                "Resolved '{}' as external module '{}' ({})",
                class,
                info.name,
                info.binary_path.display()
            );
            let module = super::external::ExternalModule::new(info, config);
            return Ok(Box::new(module));
        }

        Err(anyhow::anyhow!("Unknown module class: {}", class))
    }

    // =========================================================================
    // Default Registration
    // =========================================================================

    pub fn with_inventory() -> Self {
        let registry = Self::new();
        registry.register_from_inventory();
        registry
    }
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::with_inventory()
    }
}

impl ModuleEntry {
    /// Creates a module instance from this entry
    pub async fn create_module(
        &self,
        engine: Arc<Engine>,
        registry: &Arc<ModuleRegistry>,
    ) -> anyhow::Result<Box<dyn Module>> {
        registry
            .create_module(&self.class, engine, self.config.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create {}: {}", self.class, e))
    }
}

// =============================================================================
// EngineBuilder
// =============================================================================

/// Builder pattern for configuring and starting the Engine.
///
/// # Examples
///
/// Load from a config file (fails if missing):
/// ```ignore
/// EngineBuilder::new()
///     .config_file("config.yaml")?
///     .build().await?
///     .serve().await?;
/// ```
///
/// Run with built-in defaults (no config file):
/// ```ignore
/// EngineBuilder::new()
///     .default_config()
///     .build().await?
///     .serve().await?;
/// ```
///
/// Register custom module:
/// ```ignore
/// EngineBuilder::new()
///     .register_module::<MyCustomModule>("my::CustomModule")
///     .add_module("my::CustomModule", Some(json!({"key": "value"})))
///     .build().await?
///     .serve().await?;
/// ```
pub struct EngineBuilder {
    config: Option<EngineConfig>,
    engine: Arc<Engine>,
    registry: Arc<ModuleRegistry>,
    modules: Vec<Arc<dyn Module>>,
}

impl EngineBuilder {
    /// Creates a new EngineBuilder with default registry
    pub fn new() -> Self {
        Self {
            config: None,
            engine: Arc::new(Engine::new()),
            registry: Arc::new(ModuleRegistry::with_inventory()),
            modules: Vec::new(),
        }
    }

    /// Loads config strictly from file. Fails if file is missing or unparseable.
    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Registers a custom module type in the registry
    ///
    /// This allows you to register a module implementation that can then be used
    /// via `add_module` or in the config file.
    pub fn register_module<M: Module + 'static>(self, class: &str) -> Self {
        self.registry.register::<M>(class);
        self
    }

    /// Adds a custom module entry
    pub fn add_module(mut self, class: &str, config: Option<Value>) -> Self {
        if self.config.is_none() {
            self.config = Some(EngineConfig {
                modules: Vec::new(),
                workers: Vec::new(),
            });
        }

        if let Some(ref mut cfg) = self.config {
            cfg.modules.push(ModuleEntry {
                class: class.to_string(),
                config,
            });
        }
        self
    }

    /// Builds and initializes all modules
    pub async fn build(mut self) -> anyhow::Result<Self> {
        let config = self.config.take().expect("No module configs founded");

        // Ensure metrics are always available, even if OtelModule is not configured.
        // This prevents panics in workers/invocation code that unconditionally calls get_engine_metrics().
        crate::modules::observability::metrics::ensure_default_meter();

        // Merge workers into the modules processing pipeline
        let mut modules = config.modules;
        modules.extend(config.workers);

        tracing::info!("Building engine with {} modules", modules.len());
        let module_classes = modules
            .iter()
            .map(|entry| entry.class.clone())
            .collect::<HashSet<String>>();

        for registration in inventory::iter::<ModuleRegistration> {
            if registration.mandatory && !module_classes.contains(registration.class) {
                modules.push(ModuleEntry {
                    class: registration.class.to_string(),
                    config: None,
                });
            }
        }

        // Create modules using the registry
        for entry in &modules {
            tracing::debug!("Creating module: {}", entry.class);
            let module = entry
                .create_module(self.engine.clone(), &self.registry)
                .await
                .map_err(|err| {
                    anyhow::anyhow!("failed to create module '{}': {}", entry.class, err)
                })?;
            tracing::debug!("Initializing module: {}", entry.class);
            module.initialize().await.map_err(|err| {
                anyhow::anyhow!("failed to initialize module '{}': {}", entry.class, err)
            })?;
            module.register_functions(self.engine.clone());
            self.modules.push(Arc::from(module));
        }

        Ok(self)
    }

    pub async fn destroy(self) -> anyhow::Result<()> {
        tracing::warn!("Shutting down engine and destroying modules");
        for module in self.modules.iter() {
            tracing::debug!("Destroying module: {}", module.name());
            module.destroy().await?;
        }
        tracing::warn!("Engine shutdown complete");
        Ok(())
    }

    /// Starts the engine server
    pub async fn serve(self) -> anyhow::Result<()> {
        let engine = self.engine.clone();
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

        // Start background tasks for all modules
        for module in self.modules.iter() {
            let shutdown_rx = shutdown_rx.clone();
            let shutdown_tx = shutdown_tx.clone();
            if let Err(e) = module
                .start_background_tasks(shutdown_rx, shutdown_tx)
                .await
            {
                tracing::warn!(
                    module = module.name(),
                    error = %e,
                    "Failed to start background tasks for module"
                );
            }
        }

        // Start channel TTL sweep task
        engine.channel_manager.start_sweep_task(shutdown_rx.clone());

        shutdown_rx.changed().await?;

        self.destroy().await?;
        Ok(())
    }
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    fn test_env_var_expansion() {
        unsafe {
            env::set_var("TEST_VAR", "value1");
        }
        let input = "This is a ${TEST_VAR} and ${UNSET_VAR:default_value}";
        let expected = "This is a value1 and default_value";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_with_default_when_var_missing() {
        unsafe {
            env::remove_var("MISSING_VAR");
        }
        let input = "Value is ${MISSING_VAR:default}";
        let expected = "Value is default";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_existing_var_ignores_default() {
        // When var exists, default should be ignored
        unsafe {
            env::set_var("TEST_VAR_WITH_DEFAULT", "real_value");
        }
        let input = "url: ${TEST_VAR_WITH_DEFAULT:ignored_default}";
        let expected = "url: real_value";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_no_variables_unchanged() {
        // Text without variables should remain unchanged
        let input = "plain text without any variables";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_expand_env_vars_empty_default() {
        // Explicit empty default ${VAR:} should return empty string
        unsafe {
            env::remove_var("TEST_EMPTY_DEFAULT");
        }
        let input = "value: ${TEST_EMPTY_DEFAULT:}";
        let expected = "value: ";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_default_with_special_chars() {
        // Default containing special chars like URLs with colons
        unsafe {
            env::remove_var("TEST_REDIS_URL");
        }
        let input = "redis: ${TEST_REDIS_URL:redis://localhost:6379/0}";
        let expected = "redis: redis://localhost:6379/0";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_multiple_same_var() {
        // Same variable used multiple times
        unsafe {
            env::set_var("TEST_REPEATED", "abc");
        }
        let input = "${TEST_REPEATED}-${TEST_REPEATED}-${TEST_REPEATED}";
        let expected = "abc-abc-abc";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_adjacent_variables() {
        // Variables directly adjacent to each other
        unsafe {
            env::set_var("TEST_FIRST", "hello");
            env::set_var("TEST_SECOND", "world");
        }
        let input = "${TEST_FIRST}${TEST_SECOND}";
        let expected = "helloworld";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    #[should_panic(expected = "not set and no default provided")]
    fn test_expand_env_vars_missing_var_no_default_panics() {
        // Missing var without default should panic
        unsafe {
            env::remove_var("TEST_MUST_PANIC");
        }
        let input = "key: ${TEST_MUST_PANIC}";
        EngineConfig::expand_env_vars(input);
    }

    #[test]
    fn test_expand_env_vars_var_with_underscore_and_numbers() {
        // Variable names with underscores and numbers
        unsafe {
            env::set_var("MY_VAR_123", "test_value");
        }
        let input = "value: ${MY_VAR_123}";
        let expected = "value: test_value";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_multiline_yaml() {
        // Realistic YAML config with multiple lines
        unsafe {
            env::set_var("TEST_HOST", "localhost");
            env::set_var("TEST_PORT", "8080");
        }
        let input = r#"server:
  host: ${TEST_HOST}
  port: ${TEST_PORT}
  timeout: ${TEST_TIMEOUT:30}"#;
        let expected = r#"server:
  host: localhost
  port: 8080
  timeout: 30"#;
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_config_file_returns_error_when_file_missing() {
        let result = EngineConfig::config_file("/tmp/iii_nonexistent_config_12345.yaml");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Config file not found"),
            "Error should mention 'Config file not found', got: {}",
            err_msg
        );
    }

    #[test]
    fn test_config_file_loads_valid_yaml() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_config.yaml");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "modules: []").unwrap();

        let config = EngineConfig::config_file(path.to_str().unwrap()).unwrap();
        assert!(config.modules.is_empty());
    }

    #[test]
    fn test_config_file_error_message_includes_path() {
        let path = "/tmp/iii_this_does_not_exist_67890.yaml";
        let result = EngineConfig::config_file(path);
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains(path),
            "Error should include the path '{}', got: {}",
            path,
            err_msg
        );
    }

    // =========================================================================
    // 1. expand_env_vars tests
    // =========================================================================

    #[test]
    fn test_expand_env_vars_simple() {
        // Expand a simple env var like ${HOME}
        unsafe {
            env::set_var("TEST_SIMPLE_HOME", "/home/user");
        }
        let input = "path: ${TEST_SIMPLE_HOME}";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, "path: /home/user");
    }

    #[test]
    fn test_expand_env_vars_with_default() {
        // Expand ${NONEXISTENT:-default_value} should use default
        // The regex uses `:` as separator, so `:-default_value` means default = `-default_value`
        // Actually, re-examining the regex: r"\$\{([^}:]+)(?::([^}]*))?\}"
        // Group 1 = var name (everything up to : or })
        // Group 2 = everything after : up to }
        // So ${NONEXISTENT:-default_value} => var_name="NONEXISTENT", default="-default_value"
        unsafe {
            env::remove_var("TEST_EXPAND_NONEXISTENT_DEFAULT");
        }
        let input = "value: ${TEST_EXPAND_NONEXISTENT_DEFAULT:default_value}";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, "value: default_value");
    }

    #[test]
    #[should_panic(expected = "not set and no default provided")]
    fn test_expand_env_vars_missing_no_default() {
        // Expand ${NONEXISTENT} without default panics
        unsafe {
            env::remove_var("TEST_EXPAND_MISSING_NODEF");
        }
        let input = "key: ${TEST_EXPAND_MISSING_NODEF}";
        EngineConfig::expand_env_vars(input);
    }

    #[test]
    fn test_expand_env_vars_multiple() {
        // Expand multiple different vars in one string
        unsafe {
            env::set_var("TEST_MULTI_A", "alpha");
            env::set_var("TEST_MULTI_B", "beta");
            env::set_var("TEST_MULTI_C", "gamma");
        }
        let input = "${TEST_MULTI_A}/${TEST_MULTI_B}/${TEST_MULTI_C}";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, "alpha/beta/gamma");
    }

    #[test]
    fn test_expand_env_vars_no_vars() {
        // String without vars returns unchanged
        let input = "just a plain string with no variables at all";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_expand_env_vars_nested_in_yaml() {
        // Expand env vars in a YAML value string
        unsafe {
            env::set_var("TEST_YAML_DB_HOST", "db.example.com");
            env::set_var("TEST_YAML_DB_PORT", "5432");
        }
        let yaml_input = r#"database:
  host: ${TEST_YAML_DB_HOST}
  port: ${TEST_YAML_DB_PORT}
  name: ${TEST_YAML_DB_NAME:mydb}
  pool_size: 10"#;
        let output = EngineConfig::expand_env_vars(yaml_input);
        let expected = r#"database:
  host: db.example.com
  port: 5432
  name: mydb
  pool_size: 10"#;
        assert_eq!(output, expected);

        // Also verify the expanded YAML is actually parseable
        let parsed: serde_yaml::Value = serde_yaml::from_str(&output).unwrap();
        let db = &parsed["database"];
        assert_eq!(db["host"].as_str().unwrap(), "db.example.com");
        assert_eq!(db["port"].as_u64().unwrap(), 5432);
        assert_eq!(db["name"].as_str().unwrap(), "mydb");
        assert_eq!(db["pool_size"].as_u64().unwrap(), 10);
    }

    // =========================================================================
    // 2. default_modules tests
    // =========================================================================

    #[test]
    fn test_default_modules_returns_entries() {
        // Verify default_module_entries returns a Vec of ModuleEntry
        let entries = default_module_entries();
        // Each entry should have a non-empty class name
        for entry in &entries {
            assert!(
                !entry.class.is_empty(),
                "Module entry class should not be empty"
            );
            // Default entries have no config
            assert!(
                entry.config.is_none(),
                "Default module entries should have no config"
            );
        }
    }

    #[test]
    fn test_default_modules_keys() {
        // Verify the module type keys are present (collected from inventory)
        let entries = default_module_entries();
        let class_names: Vec<&str> = entries.iter().map(|e| e.class.as_str()).collect();

        // We cannot know exact modules at compile time since they come from inventory,
        // but we can verify the structure is sound: no duplicates in class names
        let unique_names: HashSet<&str> = class_names.iter().copied().collect();
        assert_eq!(
            class_names.len(),
            unique_names.len(),
            "Default module entries should have unique class names"
        );
    }

    // =========================================================================
    // 3. Config parsing tests
    // =========================================================================

    #[test]
    fn test_config_yaml_parsing() {
        // Parse a minimal valid YAML config string
        let yaml = r#"
modules: []
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.modules.is_empty());
    }

    #[test]
    fn test_config_yaml_with_modules() {
        // Parse config with modules section
        let yaml = r#"
modules:
  - class: "my::TestModule"
    config:
      key: "value"
      count: 42
  - class: "my::OtherModule"
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.modules.len(), 2);

        // First module has class and config
        assert_eq!(config.modules[0].class, "my::TestModule");
        let cfg = config.modules[0].config.as_ref().unwrap();
        assert_eq!(cfg["key"], "value");
        assert_eq!(cfg["count"], 42);

        // Second module has class but no config
        assert_eq!(config.modules[1].class, "my::OtherModule");
        assert!(config.modules[1].config.is_none());
    }

    #[test]
    fn test_config_yaml_empty() {
        // Parse empty/minimal YAML -- should use defaults
        let yaml = "{}";
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.modules.is_empty());
    }

    #[test]
    fn test_config_yaml_only_modules() {
        // Parse YAML with only modules
        let yaml = r#"
modules:
  - class: "test::Module"
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.modules.len(), 1);
        assert_eq!(config.modules[0].class, "test::Module");
    }

    // =========================================================================
    // 4. ModuleRegistry tests
    // =========================================================================

    #[test]
    fn test_module_registry_new_is_empty() {
        // A freshly created registry (without inventory) should be empty
        let registry = ModuleRegistry::new();
        let factories = registry.module_factories.read().expect("RwLock poisoned");
        assert!(
            factories.is_empty(),
            "New ModuleRegistry should have no registered modules"
        );
    }

    #[test]
    fn test_module_registry_register() {
        // Register a module type and verify it exists in the registry
        use async_trait::async_trait;

        struct DummyModule;

        #[async_trait]
        impl Module for DummyModule {
            fn name(&self) -> &'static str {
                "dummy"
            }

            async fn create(
                _engine: Arc<Engine>,
                _config: Option<Value>,
            ) -> anyhow::Result<Box<dyn Module>> {
                Ok(Box::new(DummyModule))
            }

            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let registry = ModuleRegistry::new();
        registry.register::<DummyModule>("test::DummyModule");

        let factories = registry.module_factories.read().expect("RwLock poisoned");
        assert!(
            factories.contains_key("test::DummyModule"),
            "Registry should contain the registered module"
        );
    }

    #[test]
    fn test_module_registry_contains() {
        // Check if a registered type exists and an unregistered one does not
        use async_trait::async_trait;

        struct AnotherDummy;

        #[async_trait]
        impl Module for AnotherDummy {
            fn name(&self) -> &'static str {
                "another_dummy"
            }

            async fn create(
                _engine: Arc<Engine>,
                _config: Option<Value>,
            ) -> anyhow::Result<Box<dyn Module>> {
                Ok(Box::new(AnotherDummy))
            }

            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let registry = ModuleRegistry::new();
        registry.register::<AnotherDummy>("test::AnotherDummy");

        let factories = registry.module_factories.read().expect("RwLock poisoned");
        assert!(
            factories.contains_key("test::AnotherDummy"),
            "Registry should contain 'test::AnotherDummy'"
        );
        assert!(
            !factories.contains_key("test::NonExistent"),
            "Registry should not contain unregistered module"
        );
    }

    #[test]
    fn test_module_registry_register_multiple() {
        // Register multiple modules and verify all are present
        use async_trait::async_trait;

        struct ModA;
        struct ModB;

        #[async_trait]
        impl Module for ModA {
            fn name(&self) -> &'static str {
                "mod_a"
            }
            async fn create(
                _engine: Arc<Engine>,
                _config: Option<Value>,
            ) -> anyhow::Result<Box<dyn Module>> {
                Ok(Box::new(ModA))
            }
            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        #[async_trait]
        impl Module for ModB {
            fn name(&self) -> &'static str {
                "mod_b"
            }
            async fn create(
                _engine: Arc<Engine>,
                _config: Option<Value>,
            ) -> anyhow::Result<Box<dyn Module>> {
                Ok(Box::new(ModB))
            }
            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let registry = ModuleRegistry::new();
        registry.register::<ModA>("test::ModA");
        registry.register::<ModB>("test::ModB");

        let factories = registry.module_factories.read().expect("RwLock poisoned");
        assert_eq!(factories.len(), 2);
        assert!(factories.contains_key("test::ModA"));
        assert!(factories.contains_key("test::ModB"));
    }

    // =========================================================================
    // ModuleEntry
    // =========================================================================

    #[test]
    fn test_module_entry_deserialize() {
        let yaml = r#"
class: "my::Module"
config:
  key: "value"
"#;
        let entry: ModuleEntry = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(entry.class, "my::Module");
        assert!(entry.config.is_some());
    }

    #[test]
    fn test_module_entry_deserialize_no_config() {
        let yaml = r#"class: "my::Module""#;
        let entry: ModuleEntry = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(entry.class, "my::Module");
        assert!(entry.config.is_none());
    }

    // =========================================================================
    // EngineBuilder
    // =========================================================================

    #[test]
    fn test_engine_builder_default() {
        let builder = EngineBuilder::default();
        assert!(builder.config.is_none());
        assert!(builder.modules.is_empty());
    }

    #[test]
    fn test_engine_builder_add_module_without_config() {
        let builder = EngineBuilder::new().add_module("test::Module", None);
        assert!(builder.config.is_some());
        let config = builder.config.unwrap();
        assert_eq!(config.modules.len(), 1);
        assert_eq!(config.modules[0].class, "test::Module");
        assert!(config.modules[0].config.is_none());
    }

    #[test]
    fn test_engine_builder_add_module_with_config() {
        let builder = EngineBuilder::new()
            .add_module("test::Module", Some(serde_json::json!({"key": "value"})));
        let config = builder.config.unwrap();
        assert_eq!(config.modules[0].config.as_ref().unwrap()["key"], "value");
    }

    #[test]
    fn test_engine_builder_add_multiple_modules() {
        let builder = EngineBuilder::new()
            .add_module("test::ModA", None)
            .add_module("test::ModB", Some(serde_json::json!({"port": 3000})));
        let config = builder.config.unwrap();
        assert_eq!(config.modules.len(), 2);
        assert_eq!(config.modules[0].class, "test::ModA");
        assert_eq!(config.modules[1].class, "test::ModB");
    }

    // =========================================================================
    // create_module with unknown class
    // =========================================================================

    #[tokio::test]
    async fn test_create_module_unknown_class_fails() {
        let registry = Arc::new(ModuleRegistry::new());
        let engine = Arc::new(Engine::new());
        let result = registry
            .create_module("nonexistent::Module", engine, None)
            .await;
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("Unknown module class"));
    }

    #[tokio::test]
    async fn test_create_module_registered_class() {
        use async_trait::async_trait;

        struct TestMod;

        #[async_trait]
        impl Module for TestMod {
            fn name(&self) -> &'static str {
                "test_mod"
            }
            async fn create(
                _engine: Arc<Engine>,
                _config: Option<Value>,
            ) -> anyhow::Result<Box<dyn Module>> {
                Ok(Box::new(TestMod))
            }
            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let registry = Arc::new(ModuleRegistry::new());
        registry.register::<TestMod>("test::TestMod");

        let engine = Arc::new(Engine::new());
        let result = registry.create_module("test::TestMod", engine, None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name(), "test_mod");
    }

    // =========================================================================
    // ModuleEntry::create_module
    // =========================================================================

    #[tokio::test]
    async fn test_module_entry_create_unknown_fails() {
        let entry = ModuleEntry {
            class: "unknown::Module".to_string(),
            config: None,
        };
        let registry = Arc::new(ModuleRegistry::new());
        let engine = Arc::new(Engine::new());
        let result = entry.create_module(engine, &registry).await;
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("Failed to create unknown::Module"));
    }

    // =========================================================================
    // EngineConfig YAML parsing edge cases
    // =========================================================================

    #[test]
    fn test_config_yaml_module_with_complex_config() {
        let yaml = r#"
modules:
  - class: "my::Module"
    config:
      nested:
        deep: true
        items:
          - "a"
          - "b"
      number: 42
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.modules.len(), 1);
        let cfg = config.modules[0].config.as_ref().unwrap();
        assert_eq!(cfg["nested"]["deep"], true);
        assert_eq!(cfg["nested"]["items"][0], "a");
        assert_eq!(cfg["number"], 42);
    }

    // =========================================================================
    // expand_env_vars edge cases
    // =========================================================================

    #[test]
    fn test_expand_env_vars_empty_string() {
        let output = EngineConfig::expand_env_vars("");
        assert_eq!(output, "");
    }

    #[test]
    fn test_expand_env_vars_dollar_sign_without_brace() {
        let input = "price is $100";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, "price is $100");
    }

    #[test]
    fn test_expand_env_vars_incomplete_syntax() {
        // ${unclosed should not match the regex
        let input = "value: ${UNCLOSED";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, "value: ${UNCLOSED");
    }

    #[test]
    fn test_expand_env_vars_special_characters_in_value() {
        unsafe {
            env::set_var("TEST_SPECIAL_CHARS_VAL", "hello world!@#$%^&*()");
        }
        let input = "val: ${TEST_SPECIAL_CHARS_VAL}";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, "val: hello world!@#$%^&*()");
    }

    // =========================================================================
    // ModuleRegistry register overwrites
    // =========================================================================

    #[test]
    fn test_module_registry_register_overwrite() {
        use async_trait::async_trait;

        struct ModV1;
        struct ModV2;

        #[async_trait]
        impl Module for ModV1 {
            fn name(&self) -> &'static str {
                "v1"
            }
            async fn create(_: Arc<Engine>, _: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
                Ok(Box::new(ModV1))
            }
            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        #[async_trait]
        impl Module for ModV2 {
            fn name(&self) -> &'static str {
                "v2"
            }
            async fn create(_: Arc<Engine>, _: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
                Ok(Box::new(ModV2))
            }
            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let registry = ModuleRegistry::new();
        registry.register::<ModV1>("test::Overwrite");
        registry.register::<ModV2>("test::Overwrite");

        let factories = registry.module_factories.read().expect("RwLock poisoned");
        assert_eq!(factories.len(), 1);
        assert!(factories.contains_key("test::Overwrite"));
    }

    #[tokio::test]
    async fn test_engine_builder_build_and_destroy_run_module_lifecycle() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        use async_trait::async_trait;

        static INITIALIZED: AtomicUsize = AtomicUsize::new(0);
        static REGISTERED: AtomicUsize = AtomicUsize::new(0);
        static DESTROYED: AtomicUsize = AtomicUsize::new(0);

        struct LifecycleModule;

        #[async_trait]
        impl Module for LifecycleModule {
            fn name(&self) -> &'static str {
                "LifecycleModule"
            }

            async fn create(
                _engine: Arc<Engine>,
                _config: Option<Value>,
            ) -> anyhow::Result<Box<dyn Module>> {
                Ok(Box::new(LifecycleModule))
            }

            async fn initialize(&self) -> anyhow::Result<()> {
                INITIALIZED.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }

            async fn destroy(&self) -> anyhow::Result<()> {
                DESTROYED.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }

            fn register_functions(&self, _engine: Arc<Engine>) {
                REGISTERED.fetch_add(1, Ordering::SeqCst);
            }
        }

        INITIALIZED.store(0, Ordering::SeqCst);
        REGISTERED.store(0, Ordering::SeqCst);
        DESTROYED.store(0, Ordering::SeqCst);

        let builder = EngineBuilder::new()
            .register_module::<LifecycleModule>("test::Lifecycle")
            .add_module(
                "test::Lifecycle",
                Some(serde_json::json!({"enabled": true})),
            )
            .build()
            .await
            .expect("build engine");

        assert_eq!(INITIALIZED.load(Ordering::SeqCst), 1);
        assert_eq!(REGISTERED.load(Ordering::SeqCst), 1);
        assert!(!builder.modules.is_empty());

        builder.destroy().await.expect("destroy engine");
        assert_eq!(DESTROYED.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn engine_builder_reports_module_class_on_stream_bind_failure() {
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let port = occupied.local_addr().expect("local addr").port();

        let err = EngineBuilder::new()
            .add_module(
                "modules::stream::StreamModule",
                Some(serde_json::json!({
                    "host": "127.0.0.1",
                    "port": port,
                    "adapter": {
                        "class": "modules::stream::adapters::KvStore"
                    }
                })),
            )
            .build()
            .await
            .err()
            .expect("build should fail when the stream port is occupied");

        let message = err.to_string();
        assert!(
            message.contains("modules::stream::StreamModule"),
            "unexpected error message: {message}"
        );
        assert!(
            message.contains(&format!("127.0.0.1:{port}")),
            "unexpected error message: {message}"
        );
        assert!(
            message.contains("already in use"),
            "unexpected error message: {message}"
        );
    }
}
