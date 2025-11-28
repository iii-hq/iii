use std::sync::Arc;

use crate::{
    engine::Engine,
    modules::{
        config::{EngineConfig, ModuleEntry},
        configurable::Configurable,
        core_module::CoreModule,
        cron::CronCoreModule,
        event::EventCoreModule,
        observability::LoggerCoreModule,
        rest_api::RestApiCoreModule,
    },
};

/// Default modules to load when no config.yaml is present
const DEFAULT_MODULES: &[&str] = &[
    "modules::api::RestApiModule",
    "modules::event::EventModule",
    "modules::observability::LoggingModule",
    "modules::cron::CronModule",
];

/// Loads default modules when no config.yaml is present
pub async fn load_default_modules(engine: Arc<Engine>) -> anyhow::Result<Vec<Box<dyn CoreModule>>> {
    let entries: Vec<ModuleEntry> = DEFAULT_MODULES
        .iter()
        .map(|class| ModuleEntry {
            class: class.to_string(),
            config: None,
        })
        .collect();

    let config = EngineConfig { modules: entries };

    let modules = load_modules(engine, &config)?;
    initialize_modules(&modules).await?;

    Ok(modules)
}

/// Creates a module from a YAML entry
fn create_module(engine: Arc<Engine>, entry: &ModuleEntry) -> anyhow::Result<Box<dyn CoreModule>> {
    match entry.class.as_str() {
        // REST API
        "modules::api::RestApiModule" => {
            let module = RestApiCoreModule::from_value(engine, entry.config.clone());
            Ok(Box::new(module))
        }

        // Event Module
        "modules::event::EventModule" => {
            let module = EventCoreModule::from_value(engine, entry.config.clone());
            Ok(Box::new(module))
        }

        // Cron Module
        "modules::cron::CronModule" => {
            let module = CronCoreModule::from_value(engine, entry.config.clone());
            Ok(Box::new(module))
        }

        // Logger Module
        "modules::observability::LoggingModule" => {
            let module = LoggerCoreModule::from_value(engine, entry.config.clone());
            Ok(Box::new(module))
        }

        // Tracing Module (alias for LoggingModule for now)
        "modules::observability::TracingModule" => {
            let module = LoggerCoreModule::from_value(engine, entry.config.clone());
            Ok(Box::new(module))
        }

        // Unknown module
        unknown => Err(anyhow::anyhow!("Unknown module class: {}", unknown)),
    }
}

/// Loads all modules from the configuration
pub fn load_modules(
    engine: Arc<Engine>,
    config: &EngineConfig,
) -> anyhow::Result<Vec<Box<dyn CoreModule>>> {
    let mut modules = Vec::new();

    for entry in &config.modules {
        tracing::debug!("Creating module: {}", entry.class);
        let module = create_module(engine.clone(), entry)?;
        modules.push(module);
    }

    Ok(modules)
}

/// Initializes all loaded modules
pub async fn initialize_modules(modules: &[Box<dyn CoreModule>]) -> anyhow::Result<()> {
    for module in modules {
        module.initialize().await?;
    }
    Ok(())
}

/// Loads and initializes modules from YAML
pub async fn load_and_initialize(
    engine: Arc<Engine>,
    yaml_content: &str,
) -> anyhow::Result<Vec<Box<dyn CoreModule>>> {
    let config: EngineConfig = serde_yaml::from_str(yaml_content)?;

    tracing::info!("Loading {} modules from config", config.modules.len());

    let modules = load_modules(engine, &config)?;
    initialize_modules(&modules).await?;

    tracing::info!("All {} modules initialized successfully", modules.len());

    Ok(modules)
}
