use std::{
    collections::HashMap,
    env,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, RwLock},
};

use axum::{
    Router,
    extract::{ConnectInfo, State, ws::WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use colored::Colorize;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use tokio::net::TcpListener;

use super::{core_module::CoreModule, registry::ModuleRegistration};
use crate::engine::Engine;

// =============================================================================
// Constants
// =============================================================================

/// Default address for the engine server
pub const DEFAULT_PORT: u16 = 49134;
const DEFAULT_HOST: &str = "127.0.0.1";

// =============================================================================
// EngineConfig (YAML structure)
// =============================================================================

fn default_port() -> u16 {
    DEFAULT_PORT
}

#[derive(Debug, Deserialize)]
pub struct EngineConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub modules: Vec<ModuleEntry>,
}

impl EngineConfig {
    pub fn default_modules(self) -> Self {
        let modules = default_module_entries();

        Self {
            port: DEFAULT_PORT,
            modules,
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

    pub fn config_file_or_default(path: &str) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(yaml_content) => {
                let yaml_content = Self::expand_env_vars(&yaml_content);
                let config = serde_yaml::from_str(&yaml_content);
                match config {
                    Ok(cfg) => {
                        tracing::info!("Parsed config file: {}", path);
                        Ok(cfg)
                    }
                    Err(err) => Err(anyhow::anyhow!(
                        "Failed to parse config file {}: {}",
                        path,
                        err
                    )),
                }
            }
            Err(_) => {
                tracing::info!("No {} found, using default modules", path);
                Ok(Self {
                    port: DEFAULT_PORT,
                    modules: default_module_entries(),
                })
            }
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

async fn shutdown_signal() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigint = signal(SignalKind::interrupt())?;

        tokio::select! {
            _ = sigterm.recv() => {},
            _ = sigint.recv() => {},
            _ = tokio::signal::ctrl_c() => {},
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
    }

    Ok(())
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
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Box<dyn CoreModule>>> + Send>>
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
    /// The module must implement `CoreModule`. The registry uses `M::create()` to create instances.
    pub fn register<M: CoreModule + 'static>(&self, class: &str) {
        let info = ModuleInfo {
            factory: Arc::new(|engine, config| Box::pin(M::create(engine, config))),
        };

        self.module_factories
            .write()
            .expect("RwLock poisoned")
            .insert(class.to_string(), info);
    }

    /// Creates a module instance
    pub async fn create_module(
        self: &Arc<Self>,
        class: &str,
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        let factory = {
            let factories = self.module_factories.read().expect("RwLock poisoned");
            factories
                .get(class)
                .ok_or_else(|| anyhow::anyhow!("Unknown module class: {}", class))?
                .factory
                .clone()
        };

        factory(engine, config).await
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
    ) -> anyhow::Result<Box<dyn CoreModule>> {
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
/// Load from file or use defaults:
/// ```ignore
/// EngineBuilder::new()
///     .config_file_or_default("config.yaml")?
///     .address("0.0.0.0:3000")
///     .build().await?
///     .serve().await?;
/// ```
///
/// Register custom module:
/// ```ignore
/// EngineBuilder::new()
///     .register::<MyCustomModule>("my::CustomModule")
///     .add_module("my::CustomModule", Some(json!({"key": "value"})))
///     .build().await?
///     .serve().await?;
/// ```
pub struct EngineBuilder {
    config: Option<EngineConfig>,
    address: String,
    engine: Arc<Engine>,
    registry: Arc<ModuleRegistry>,
    modules: Vec<Arc<dyn CoreModule>>,
}

impl EngineBuilder {
    /// Creates a new EngineBuilder with default registry
    pub fn new() -> Self {
        Self {
            config: None,
            address: format!("{}:{}", DEFAULT_HOST, DEFAULT_PORT),
            engine: Arc::new(Engine::new()),
            registry: Arc::new(ModuleRegistry::with_inventory()),
            modules: Vec::new(),
        }
    }

    /// Sets the server address
    pub fn address(mut self, addr: &str) -> Self {
        self.address = addr.to_string();
        self
    }

    /// Loads config from file if exists, otherwise uses defaults
    pub fn config_file_or_default(mut self, path: &str) -> anyhow::Result<Self> {
        let config = EngineConfig::config_file_or_default(path)?;
        self.config = Some(config);
        Ok(self)
    }

    /// Registers a custom module type in the registry
    ///
    /// This allows you to register a module implementation that can then be used
    /// via `add_module` or in the config file.
    pub fn register_module<M: CoreModule + 'static>(self, class: &str) -> Self {
        self.registry.register::<M>(class);
        self
    }

    /// Adds a custom module entry
    pub fn add_module(mut self, class: &str, config: Option<Value>) -> Self {
        if self.config.is_none() {
            self.config = Some(EngineConfig {
                modules: Vec::new(),
                port: DEFAULT_PORT,
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

        // Collect module classes from config
        let config_classes: std::collections::HashSet<String> =
            config.modules.iter().map(|m| m.class.clone()).collect();

        // Get default modules that aren't already in config
        let default_entries: Vec<ModuleEntry> = default_module_entries()
            .into_iter()
            .filter(|entry| !config_classes.contains(&entry.class))
            .collect();

        let total_modules = config.modules.len() + default_entries.len();
        tracing::info!(
            "Building engine with {} modules ({} from config, {} default)",
            total_modules,
            config.modules.len(),
            default_entries.len()
        );

        let all_entries: Vec<&ModuleEntry> = default_entries
            .iter()
            .chain(config.modules.iter())
            .collect();

        for entry in all_entries {
            tracing::debug!("Creating module: {}", entry.class);
            let module = entry
                .create_module(self.engine.clone(), &self.registry)
                .await?;
            tracing::debug!("Initializing module: {}", entry.class);
            module.initialize().await?;
            module.register_functions(self.engine.clone());
            self.modules.push(Arc::from(module));
        }

        Ok(self)
    }

    pub async fn destroy(self) -> anyhow::Result<()> {
        for module in self.modules.iter() {
            tracing::debug!("Destroying module: {}", module.name());
            module.destroy().await?;
        }
        Ok(())
    }

    /// Starts the engine server
    pub async fn serve(self) -> anyhow::Result<()> {
        let engine = self.engine.clone();

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Start background tasks for all modules
        for module in self.modules.iter() {
            let module_shutdown = shutdown_rx.clone();
            if let Err(e) = module.start_background_tasks(module_shutdown).await {
                tracing::warn!(
                    module = module.name(),
                    error = %e,
                    "Failed to start background tasks for module"
                );
            }
        }

        // Setup router
        let app = Router::new().route("/", get(ws_handler)).with_state(engine);

        // Bind and serve
        let listener = TcpListener::bind(&self.address).await?;
        tracing::info!("Engine listening on address: {}", self.address.purple());

        let shutdown = async move {
            let _ = shutdown_signal().await;
            let _ = shutdown_tx.send(true);
        };

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .await?;

        self.destroy().await?;
        Ok(())
    }
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}
// =============================================================================
// WebSocket Handler
// =============================================================================

async fn ws_handler(
    State(engine): State<Arc<Engine>>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let engine = engine.clone();

    ws.on_upgrade(move |socket| async move {
        if let Err(err) = engine.handle_worker(socket, addr).await {
            tracing::error!(addr = %addr, error = ?err, "worker error");
        }
    })
}

#[cfg(test)]
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
}
