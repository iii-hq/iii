use std::{future::Future, net::SocketAddr, sync::Arc};

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
    adapter_registry::AdapterRegistry,
    configurable::Configurable,
    core_module::CoreModule,
    cron::{CronCoreModule, CronScheduler},
    event::{EventAdapter, EventCoreModule},
    observability::LoggerCoreModule,
    rest_api::RestApiCoreModule,
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

impl ModuleEntry {
    /// Creates a module instance from this entry
    pub fn create_module(
        &self,
        engine: Arc<Engine>,
        registry: Arc<AdapterRegistry>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        match self.class.as_str() {
            "modules::streams::StreamModule" => {
                let streams_module = StreamCoreModule::new(engine.clone());
                // dumb adapter initialization for example purposes
                Ok(Box::new(streams_module))
            }

            "modules::api::RestApiModule" => {
                let module = RestApiCoreModule::from_value(engine, self.config.clone());
                Ok(Box::new(module))
            }

            "modules::event::EventModule" => {
                let mut module = EventCoreModule::from_value(engine, self.config.clone());
                module.set_adapter_registry(registry);
                Ok(Box::new(module))
            }

            "modules::cron::CronModule" => {
                let mut module = CronCoreModule::from_value(engine, self.config.clone());
                module.set_adapter_registry(registry);
                Ok(Box::new(module))
            }

            "modules::observability::LoggingModule" | "modules::observability::TracingModule" => {
                let module = LoggerCoreModule::from_value(engine, self.config.clone());
                Ok(Box::new(module))
            }

            unknown => Err(anyhow::anyhow!("Unknown module class: {}", unknown)),
        }
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
/// Only default modules:
/// ```ignore
/// EngineBuilder::new()
///     .default_modules()
///     .build().await?
///     .serve().await?;
/// ```
///
/// Register custom adapters:
/// ```ignore
/// EngineBuilder::new()
///     .register_event_adapter("my::CustomEventAdapter", |config, engine| async move {
///         let adapter = MyCustomAdapter::new(config, engine).await?;
///         Ok(Arc::new(adapter) as Arc<dyn EventAdapter>)
///     })
///     .register_cron_scheduler("my::CustomCronScheduler", |config| async move {
///         let scheduler = MyCustomScheduler::new(config).await?;
///         Ok(Arc::new(scheduler) as Arc<dyn CronScheduler>)
///     })
///     .config_file_or_default("config.yaml")?
///     .build().await?
///     .serve().await?;
/// ```
///
/// Custom modules:
/// ```ignore
/// EngineBuilder::new()
///     .add_module("modules::api::RestApiModule", Some(json!({"port": 8080})))
///     .add_module("modules::observability::LoggingModule", None)
///     .build().await?
///     .serve().await?;
/// ```
pub struct EngineBuilder {
    config: Option<EngineConfig>,
    address: String,
    engine: Arc<Engine>,
    modules: Vec<Box<dyn CoreModule>>,
    adapter_registry: Arc<AdapterRegistry>,
}

impl EngineBuilder {
    /// Creates a new EngineBuilder with default adapter registry
    pub fn new() -> Self {
        Self {
            config: None,
            address: DEFAULT_ADDRESS.to_string(),
            engine: Arc::new(Engine::new()),
            modules: Vec::new(),
            adapter_registry: Arc::new(AdapterRegistry::with_defaults()),
        }
    }

    /// Sets the server address
    pub fn address(mut self, addr: &str) -> Self {
        self.address = addr.to_string();
        self
    }

    /// Registers a custom event adapter factory
    ///
    /// # Example
    /// ```ignore
    /// builder.register_event_adapter("my::CustomAdapter", |config, engine| async move {
    ///     let adapter = MyAdapter::new(config, engine).await?;
    ///     Ok(Arc::new(adapter) as Arc<dyn EventAdapter>)
    /// })
    /// ```
    pub fn register_event_adapter<F, Fut>(self, class: &str, factory: F) -> Self
    where
        F: Fn(Option<Value>, Arc<Engine>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<Arc<dyn EventAdapter>>> + Send + 'static,
    {
        tracing::info!("Registering event adapter: {}", class);
        self.adapter_registry.register_event_adapter(class, factory);
        self
    }

    /// Registers a custom cron scheduler factory
    ///
    /// # Example
    /// ```ignore
    /// builder.register_cron_scheduler("my::CustomScheduler", |config| async move {
    ///     let scheduler = MyScheduler::new(config).await?;
    ///     Ok(Arc::new(scheduler) as Arc<dyn CronScheduler>)
    /// })
    /// ```
    pub fn register_cron_scheduler<F, Fut>(self, class: &str, factory: F) -> Self
    where
        F: Fn(Option<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<Arc<dyn CronScheduler>>> + Send + 'static,
    {
        self.adapter_registry
            .register_cron_scheduler(class, factory);
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
        let config = self.config.take().unwrap_or_else(|| {
            tracing::info!("No config provided, using default modules");
            EngineConfig {
                modules: DEFAULT_MODULES
                    .iter()
                    .map(|class| ModuleEntry {
                        class: class.to_string(),
                        config: None,
                    })
                    .collect(),
            }
        });

        tracing::info!("Building engine with {} modules", config.modules.len());

        // Create modules
        for entry in &config.modules {
            tracing::debug!("Creating module: {}", entry.class);
            let module = entry.create_module(self.engine.clone(), self.adapter_registry.clone())?;
            self.modules.push(module);
        }

        // Initialize modules
        for module in &self.modules {
            module.initialize().await?;
        }

        tracing::info!(
            "All {} modules initialized successfully",
            self.modules.len()
        );

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
