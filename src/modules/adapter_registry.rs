use std::{collections::HashMap, future::Future, pin::Pin, sync::{Arc, RwLock}};

use serde_json::Value;

use crate::engine::Engine;

use super::{cron::CronScheduler, event::EventAdapter};

// =============================================================================
// Type Aliases for Adapter Factories
// =============================================================================

/// Factory function type for creating EventAdapters
pub type EventAdapterFactory = Box<
    dyn Fn(Option<Value>, Arc<Engine>) -> Pin<Box<dyn Future<Output = anyhow::Result<Arc<dyn EventAdapter>>> + Send>>
        + Send
        + Sync,
>;

/// Factory function type for creating CronSchedulers
pub type CronSchedulerFactory = Box<
    dyn Fn(Option<Value>) -> Pin<Box<dyn Future<Output = anyhow::Result<Arc<dyn CronScheduler>>> + Send>>
        + Send
        + Sync,
>;

// =============================================================================
// AdapterRegistry
// =============================================================================

/// Registry for dynamically registering adapter factories.
///
/// This allows users to register their own adapter implementations
/// that can be referenced by class name in the configuration.
///
/// # Example
///
/// ```ignore
/// let registry = AdapterRegistry::new();
///
/// // Register a custom event adapter
/// registry.register_event_adapter(
///     "my::CustomEventAdapter",
///     |config, engine| Box::pin(async move {
///         let adapter = MyCustomAdapter::new(config, engine).await?;
///         Ok(Arc::new(adapter) as Arc<dyn EventAdapter>)
///     })
/// );
/// ```
pub struct AdapterRegistry {
    event_adapters: RwLock<HashMap<String, EventAdapterFactory>>,
    cron_schedulers: RwLock<HashMap<String, CronSchedulerFactory>>,
}

impl AdapterRegistry {
    /// Creates a new empty AdapterRegistry
    pub fn new() -> Self {
        Self {
            event_adapters: RwLock::new(HashMap::new()),
            cron_schedulers: RwLock::new(HashMap::new()),
        }
    }

    /// Creates a new AdapterRegistry with default adapters registered
    pub fn with_defaults() -> Self {
        let mut event_adapters = HashMap::new();
        event_adapters.insert(
            "modules::event::RedisAdapter".to_string(),
            create_redis_event_adapter_factory(),
        );

        let mut cron_schedulers = HashMap::new();
        cron_schedulers.insert(
            "modules::cron::RedisCronAdapter".to_string(),
            create_redis_cron_scheduler_factory(),
        );

        Self {
            event_adapters: RwLock::new(event_adapters),
            cron_schedulers: RwLock::new(cron_schedulers),
        }
    }

    // =========================================================================
    // Event Adapter Registration
    // =========================================================================

    /// Registers an event adapter factory
    pub fn register_event_adapter<F, Fut>(&self, class: &str, factory: F)
    where
        F: Fn(Option<Value>, Arc<Engine>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<Arc<dyn EventAdapter>>> + Send + 'static,
    {
        let boxed_factory: EventAdapterFactory =
            Box::new(move |config, engine| Box::pin(factory(config, engine)));

        self.event_adapters
            .write()
            .expect("RwLock poisoned")
            .insert(class.to_string(), boxed_factory);
    }

    /// Creates an event adapter by class name
    pub async fn create_event_adapter(
        &self,
        class: &str,
        config: Option<Value>,
        engine: Arc<Engine>,
    ) -> anyhow::Result<Arc<dyn EventAdapter>> {
        let factory = {
            let adapters = self.event_adapters.read().expect("RwLock poisoned");
            adapters
                .get(class)
                .ok_or_else(|| anyhow::anyhow!("Unknown event adapter class: {}", class))?
                as *const EventAdapterFactory
        };

        // Safety: We hold Arc<AdapterRegistry> so the factory won't be dropped
        let factory = unsafe { &*factory };
        factory(config, engine).await
    }

    /// Lists all registered event adapter classes
    pub fn list_event_adapters(&self) -> Vec<String> {
        self.event_adapters
            .read()
            .expect("RwLock poisoned")
            .keys()
            .cloned()
            .collect()
    }

    // =========================================================================
    // Cron Scheduler Registration
    // =========================================================================

    /// Registers a cron scheduler factory
    pub fn register_cron_scheduler<F, Fut>(&self, class: &str, factory: F)
    where
        F: Fn(Option<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<Arc<dyn CronScheduler>>> + Send + 'static,
    {
        let boxed_factory: CronSchedulerFactory = Box::new(move |config| Box::pin(factory(config)));

        self.cron_schedulers
            .write()
            .expect("RwLock poisoned")
            .insert(class.to_string(), boxed_factory);
    }

    /// Creates a cron scheduler by class name
    pub async fn create_cron_scheduler(
        &self,
        class: &str,
        config: Option<Value>,
    ) -> anyhow::Result<Arc<dyn CronScheduler>> {
        let factory = {
            let schedulers = self.cron_schedulers.read().expect("RwLock poisoned");
            schedulers
                .get(class)
                .ok_or_else(|| anyhow::anyhow!("Unknown cron scheduler class: {}", class))?
                as *const CronSchedulerFactory
        };

        // Safety: We hold Arc<AdapterRegistry> so the factory won't be dropped
        let factory = unsafe { &*factory };
        factory(config).await
    }

    /// Lists all registered cron scheduler classes
    pub fn list_cron_schedulers(&self) -> Vec<String> {
        self.cron_schedulers
            .read()
            .expect("RwLock poisoned")
            .keys()
            .cloned()
            .collect()
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// =============================================================================
// Default Adapter Factories
// =============================================================================

fn create_redis_event_adapter_factory() -> EventAdapterFactory {
    use super::event::adapters::RedisAdapter;

    Box::new(|config, engine| {
        Box::pin(async move {
            let redis_config: RedisAdapterConfig = config
                .map(|v| serde_json::from_value(v).unwrap_or_default())
                .unwrap_or_default();

            tracing::info!(
                "Creating RedisAdapter for EventModule at {}",
                redis_config.redis_url
            );

            let adapter = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                RedisAdapter::new(redis_config.redis_url, engine),
            )
            .await
            .map_err(|_| anyhow::anyhow!("Timed out while connecting to Redis"))?
            .map_err(|e| anyhow::anyhow!("Failed to connect to Redis: {}", e))?;

            Ok(Arc::new(adapter) as Arc<dyn EventAdapter>)
        })
    })
}

fn create_redis_cron_scheduler_factory() -> CronSchedulerFactory {
    use super::cron::RedisCronLock;

    Box::new(|config| {
        Box::pin(async move {
            let redis_config: RedisAdapterConfig = config
                .map(|v| serde_json::from_value(v).unwrap_or_default())
                .unwrap_or_default();

            tracing::info!(
                "Creating RedisCronLock for CronModule at {}",
                redis_config.redis_url
            );

            let scheduler = RedisCronLock::new(&redis_config.redis_url).await?;

            Ok(Arc::new(scheduler) as Arc<dyn CronScheduler>)
        })
    })
}

// =============================================================================
// Shared Config Types
// =============================================================================

use serde::Deserialize;

fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisAdapterConfig {
    #[serde(default = "default_redis_url")]
    pub redis_url: String,
}

impl Default for RedisAdapterConfig {
    fn default() -> Self {
        Self {
            redis_url: default_redis_url(),
        }
    }
}
