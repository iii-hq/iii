//! Example: Creating a custom Module with its own custom Adapter
//!
//! Run with: `cargo run --example custom_event_adapter`

use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use engine::{
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::{FunctionHandler, FunctionResult},
    modules::{
        config::EngineBuilder,
        core_module::{AdapterFactory, ConfigurableModule, CoreModule},
    },
    protocol::ErrorBody,
};
use futures::Future;
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock as TokioRwLock;
use uuid::Uuid;

// =============================================================================
// 1. Define your custom Adapter trait
// =============================================================================

#[async_trait]
pub trait CustomEventAdapter: Send + Sync + 'static {
    async fn emit(&self, topic: &str, event_data: Value);
    async fn subscribe(&self, topic: &str, id: &str, function_path: &str);
    async fn unsubscribe(&self, topic: &str, id: &str);
}

// =============================================================================
// 2. Implement your custom Adapters
// =============================================================================

// Adapter 1: InMemoryEventAdapter - stores subscribers in memory
pub struct InMemoryEventAdapter {
    subscribers: Arc<TokioRwLock<HashMap<String, Vec<(String, String)>>>>,
    engine: Arc<Engine>,
}

impl InMemoryEventAdapter {
    pub async fn new(_config: Option<Value>, engine: Arc<Engine>) -> anyhow::Result<Self> {
        Ok(Self {
            subscribers: Arc::new(TokioRwLock::new(HashMap::new())),
            engine,
        })
    }
}

#[async_trait]
impl CustomEventAdapter for InMemoryEventAdapter {
    async fn emit(&self, topic: &str, event_data: Value) {
        let subscribers = self.subscribers.read().await;
        if let Some(subs) = subscribers.get(topic) {
            for (_id, function_path) in subs {
                self.engine
                    .invoke_function(function_path, event_data.clone());
            }
        }
    }

    async fn subscribe(&self, topic: &str, id: &str, function_path: &str) {
        self.subscribers
            .write()
            .await
            .entry(topic.to_string())
            .or_default()
            .push((id.to_string(), function_path.to_string()));
    }

    async fn unsubscribe(&self, topic: &str, id: &str) {
        if let Some(subs) = self.subscribers.write().await.get_mut(topic) {
            subs.retain(|(sub_id, _)| sub_id != id);
        }
    }
}

// Adapter 2: LoggingEventAdapter - logs all events and forwards to another adapter
pub struct LoggingEventAdapter {
    inner: Arc<dyn CustomEventAdapter>,
}

impl LoggingEventAdapter {
    pub async fn new(config: Option<Value>, engine: Arc<Engine>) -> anyhow::Result<Self> {
        // Get the inner adapter class from config, default to InMemoryEventAdapter
        let inner_adapter_class = config
            .as_ref()
            .and_then(|v| v.get("inner_adapter"))
            .and_then(|v| v.as_str())
            .unwrap_or("my::InMemoryEventAdapter");

        // Create the inner adapter
        let inner_adapter = match inner_adapter_class {
            "my::InMemoryEventAdapter" => {
                Arc::new(InMemoryEventAdapter::new(None, engine.clone()).await?)
                    as Arc<dyn CustomEventAdapter>
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unknown inner adapter: {}",
                    inner_adapter_class
                ));
            }
        };

        Ok(Self {
            inner: inner_adapter,
        })
    }
}

#[async_trait]
impl CustomEventAdapter for LoggingEventAdapter {
    async fn emit(&self, topic: &str, event_data: Value) {
        tracing::info!(
            topic = %topic,
            event_data = %event_data,
            "LoggingEventAdapter: Emitting event"
        );
        self.inner.emit(topic, event_data).await;
    }

    async fn subscribe(&self, topic: &str, id: &str, function_path: &str) {
        tracing::info!(
            topic = %topic,
            id = %id,
            function_path = %function_path,
            "LoggingEventAdapter: Subscribing"
        );
        self.inner.subscribe(topic, id, function_path).await;
    }

    async fn unsubscribe(&self, topic: &str, id: &str) {
        tracing::info!(
            topic = %topic,
            id = %id,
            "LoggingEventAdapter: Unsubscribing"
        );
        self.inner.unsubscribe(topic, id).await;
    }
}

// =============================================================================
// 3. Define your Module Config
// =============================================================================

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CustomEventModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdapterEntry {
    pub class: String,
    #[serde(default)]
    pub config: Option<Value>,
}

// =============================================================================
// 4. Create your custom Module
// =============================================================================

#[derive(Clone)]
pub struct CustomEventModule {
    adapter: Arc<dyn CustomEventAdapter>,
    engine: Arc<Engine>,
    _config: CustomEventModuleConfig,
}

#[async_trait]
impl CoreModule for CustomEventModule {
    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        Self::create_with_adapters(engine, config).await
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing CustomEventModule");

        // Register a function to emit events
        self.engine.register_function(
            RegisterFunctionRequest {
                function_path: "custom_emit".to_string(),
                description: Some("Emit a custom event".to_string()),
                request_format: Some(serde_json::json!({
                    "topic": { "type": "string" },
                    "data": { "type": "object" }
                })),
                response_format: None,
            },
            Box::new(self.clone()),
        );

        Ok(())
    }
}

#[async_trait]
impl ConfigurableModule for CustomEventModule {
    type Config = CustomEventModuleConfig;
    type Adapter = dyn CustomEventAdapter;
    const DEFAULT_ADAPTER_CLASS: &'static str = "my::InMemoryEventAdapter";

    fn build_registry() -> HashMap<String, AdapterFactory<Self::Adapter>> {
        let mut m = HashMap::new();

        // Register InMemoryEventAdapter as the default
        m.insert(
            "my::InMemoryEventAdapter".to_string(),
            Self::make_adapter_factory(|engine: Arc<Engine>, config: Option<Value>| async move {
                Ok(Arc::new(InMemoryEventAdapter::new(config, engine).await?)
                    as Arc<dyn CustomEventAdapter>)
            }),
        );

        // Register LoggingEventAdapter as a custom adapter
        m.insert(
            "my::LoggingEventAdapter".to_string(),
            Self::make_adapter_factory(|engine: Arc<Engine>, config: Option<Value>| async move {
                Ok(Arc::new(LoggingEventAdapter::new(config, engine).await?)
                    as Arc<dyn CustomEventAdapter>)
            }),
        );

        m
    }

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn CustomEventAdapter>>>> =
            Lazy::new(|| RwLock::new(CustomEventModule::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self {
            engine,
            _config: config,
            adapter,
        }
    }

    fn adapter_class_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.class.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

impl FunctionHandler for CustomEventModule {
    fn handle_function(
        &self,
        _invocation_id: Option<Uuid>,
        _function_path: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'static>>
    {
        let adapter = self.adapter.clone();
        Box::pin(async move {
            let topic = input
                .get("topic")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let event_data = input.get("data").cloned().unwrap_or(Value::Null);

            if topic.is_empty() {
                return FunctionResult::Failure(ErrorBody {
                    code: "topic_not_set".into(),
                    message: "Topic is not set".into(),
                });
            }

            tracing::debug!(topic = %topic, event_data = %event_data, "Emitting custom event");
            adapter.emit(topic, event_data).await;

            FunctionResult::Success(None)
        })
    }
}

// =============================================================================
// 5. Register module and run
// =============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    engine::logging::init_tracing();

    // Register custom adapters for CustomEventModule
    CustomEventModule::add_adapter("my::CustomInMemoryAdapter", |engine, config| async move {
        Ok(Arc::new(InMemoryEventAdapter::new(config, engine).await?)
            as Arc<dyn CustomEventAdapter>)
    })
    .await?;

    CustomEventModule::add_adapter("my::CustomLoggingAdapter", |engine, config| async move {
        Ok(Arc::new(LoggingEventAdapter::new(config, engine).await?)
            as Arc<dyn CustomEventAdapter>)
    })
    .await?;

    // Register the custom module and add it to the engine using EngineBuilder
    EngineBuilder::new()
        .register_module::<CustomEventModule>("my::CustomEventModule")
        .add_module(
            // instead load from config file
            "my::CustomEventModule",
            Some(serde_json::json!({
                "adapter": {
                    "class": "my::CustomLoggingAdapter",
                    "config": {
                        "inner_adapter": "my::InMemoryEventAdapter"
                    }
                }
            })),
        )
        .address("127.0.0.1:49134")
        .build()
        .await?;

    tracing::info!("CustomEventModule initialized successfully!");
    tracing::info!("You can now use the 'custom_emit' function to emit events");

    // Keep the process running (in a real application, you'd start a server here)
    // For this example, we'll just wait a bit
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    Ok(())
}

// =============================================================================
// To use this module, you would add to config.yaml:
// =============================================================================
//
// modules:
//   - class: my::CustomEventModule
//     config:
//       adapter:
//         class: my::CustomLoggingAdapter  # or my::CustomInMemoryAdapter
//         config:
//           inner_adapter: my::InMemoryEventAdapter  # only for LoggingAdapter
