use std::{
    collections::HashMap,
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
use serde::Deserialize;
use serde_json::Value;
use tokio::net::TcpListener;

use super::{
    core_module::CoreModule, cron::CronCoreModule, event::EventCoreModule,
    observability::LoggerCoreModule, rest_api::RestApiCoreModule,
};
use crate::{engine::Engine, modules::streams::StreamCoreModule};

// =============================================================================
// Constants
// =============================================================================

/// Default modules to load when no config is provided
const DEFAULT_MODULES: &[&str] = &[
    "modules::api::RestApiModule",
    "modules::event::EventModule",
    "modules::observability::LoggingModule",
    "modules::cron::CronModule",
    "modules::streams::StreamModule",
];

/// Default address for the engine server
const DEFAULT_ADDRESS: &str = "127.0.0.1:49134";

// =============================================================================
// EngineConfig (YAML structure)
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct EngineConfig {
    #[serde(default)]
    pub modules: Vec<ModuleEntry>,
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

    pub fn with_defaults() -> Self {
        let registry = Self::new();
        // Register default modules
        registry.register::<StreamCoreModule>("modules::streams::StreamModule");
        registry.register::<RestApiCoreModule>("modules::api::RestApiModule");
        registry.register::<EventCoreModule>("modules::event::EventModule");
        registry.register::<CronCoreModule>("modules::cron::CronModule");
        registry.register::<LoggerCoreModule>("modules::observability::LoggingModule");

        registry
    }
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::with_defaults()
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
}

impl EngineBuilder {
    /// Creates a new EngineBuilder with default registry
    pub fn new() -> Self {
        Self {
            config: None,
            address: DEFAULT_ADDRESS.to_string(),
            engine: Arc::new(Engine::new()),
            registry: Arc::new(ModuleRegistry::with_defaults()),
        }
    }

    /// Sets the server address
    pub fn address(mut self, addr: &str) -> Self {
        self.address = addr.to_string();
        self
    }

    /// Registers a module by type
    /// The module must implement `CoreModule`.
    pub fn register<M: CoreModule + 'static>(self, class: &str) -> Self {
        tracing::info!("Registering module: {}", class);
        self.registry.register::<M>(class);
        self
    }

    /// Loads configuration from a YAML file
    pub fn config_file(mut self, path: &str) -> anyhow::Result<Self> {
        let yaml_content = std::fs::read_to_string(path)?;
        let config: EngineConfig = serde_yaml::from_str(&yaml_content)?;
        tracing::info!("Loaded config from {}", path);
        self.config = Some(config);
        Ok(self)
    }

    /// Uses default modules configuration
    pub fn default_modules(mut self) -> Self {
        let modules = DEFAULT_MODULES
            .iter()
            .map(|class| ModuleEntry {
                class: class.to_string(),
                config: None,
            })
            .collect();

        self.config = Some(EngineConfig { modules });
        self
    }

    /// Loads config from file if exists, otherwise uses defaults
    pub fn config_file_or_default(mut self, path: &str) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(yaml_content) => {
                let config: EngineConfig = serde_yaml::from_str(&yaml_content)?;
                tracing::info!("Loaded config from {}", path);
                self.config = Some(config);
                Ok(self)
            }
            Err(_) => {
                tracing::info!("No {} found, using default modules", path);
                Ok(self.default_modules())
            }
        }
    }

    /// Adds a custom module entry
    pub fn add_module(mut self, class: &str, config: Option<Value>) -> Self {
        if self.config.is_none() {
            self.config = Some(EngineConfig {
                modules: Vec::new(),
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

        tracing::info!("Building engine with {} modules", config.modules.len());

        // Create modules using the registry (with adapter injection)
        for entry in &config.modules {
            tracing::debug!("Creating module: {}", entry.class);
            let module = entry
                .create_module(self.engine.clone(), &self.registry)
                .await?;
            tracing::debug!("Initialing module: {}", entry.class);
            module.initialize().await?;
        }

        Ok(self)
    }

    /// Starts the engine server
    pub async fn serve(self) -> anyhow::Result<()> {
        let engine = self.engine.clone();

        // Start function notification task
        let engine_clone = engine.clone();
        tokio::spawn(async move {
            engine_clone.notify_new_functions(5).await;
        });

        // Setup router
        let app = Router::new().route("/", get(ws_handler)).with_state(engine);

        // Bind and serve
        let listener = TcpListener::bind(&self.address).await?;
        tracing::info!("Engine listening on address: {}", self.address.purple());

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;

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
