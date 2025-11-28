use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;

use crate::engine::Engine;

use super::{
    configurable::Configurable,
    core_module::CoreModule,
    cron::CronCoreModule,
    event::EventCoreModule,
    observability::LoggerCoreModule,
    rest_api::RestApiCoreModule,
};

// =============================================================================
// Engine Config (root of YAML)
// =============================================================================

/// Default modules to load when no config.yaml is present
const DEFAULT_MODULES: &[&str] = &[
    "modules::api::RestApiModule",
    "modules::event::EventModule",
    "modules::observability::LoggingModule",
    "modules::cron::CronModule",
];

#[derive(Debug, Deserialize)]
pub struct EngineConfig {
    #[serde(default)]
    pub modules: Vec<ModuleEntry>,
}

impl EngineConfig {
    /// Creates an EngineConfig with default modules
    pub fn default_modules() -> Self {
        let modules = DEFAULT_MODULES
            .iter()
            .map(|class| ModuleEntry {
                class: class.to_string(),
                config: None,
            })
            .collect();

        Self { modules }
    }

    /// Loads config from YAML string
    pub fn from_yaml(yaml_content: &str) -> anyhow::Result<Self> {
        let config = serde_yaml::from_str(yaml_content)?;
        Ok(config)
    }

    /// Loads config from file, or returns default if file doesn't exist
    pub fn from_file_or_default(path: &str) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(yaml_content) => {
                tracing::info!("Loading modules from {}", path);
                Self::from_yaml(&yaml_content)
            }
            Err(_) => {
                tracing::info!("No {} found, using default modules", path);
                Ok(Self::default_modules())
            }
        }
    }

    /// Creates all modules from the configuration
    pub fn create_modules(&self, engine: Arc<Engine>) -> anyhow::Result<Vec<Box<dyn CoreModule>>> {
        let mut modules = Vec::new();

        for entry in &self.modules {
            tracing::debug!("Creating module: {}", entry.class);
            let module = entry.create_module(engine.clone())?;
            modules.push(module);
        }

        Ok(modules)
    }

    /// Creates and initializes all modules from the configuration
    pub async fn load_modules(&self, engine: Arc<Engine>) -> anyhow::Result<Vec<Box<dyn CoreModule>>> {
        tracing::info!("Loading {} modules from config", self.modules.len());

        let modules = self.create_modules(engine)?;

        for module in &modules {
            module.initialize().await?;
        }

        tracing::info!("All {} modules initialized successfully", modules.len());

        Ok(modules)
    }
}

#[derive(Debug, Deserialize)]
pub struct ModuleEntry {
    pub class: String,
    #[serde(default)]
    pub config: Option<Value>,
}

impl ModuleEntry {
    /// Creates a module instance from this entry
    pub fn create_module(&self, engine: Arc<Engine>) -> anyhow::Result<Box<dyn CoreModule>> {
        match self.class.as_str() {
            "modules::api::RestApiModule" => {
                let module = RestApiCoreModule::from_value(engine, self.config.clone());
                Ok(Box::new(module))
            }

            "modules::event::EventModule" => {
                let module = EventCoreModule::from_value(engine, self.config.clone());
                Ok(Box::new(module))
            }

            "modules::cron::CronModule" => {
                let module = CronCoreModule::from_value(engine, self.config.clone());
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
