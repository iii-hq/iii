//! Example: Creating a custom Module with its own custom Adapter
//!
//! Run with: `cargo run --example custom_queue_adapter`

use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use futures::Future;
use iii::{
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::{FunctionHandler, FunctionResult},
    modules::{
        config::EngineBuilder,
        module::{AdapterEntry, AdapterFactory, ConfigurableModule, Module},
        registry::{AdapterFuture, AdapterRegistrationEntry},
    },
    protocol::ErrorBody,
};
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock as TokioRwLock;
use uuid::Uuid;

// =============================================================================
// 1. Define your custom Adapter trait
// =============================================================================

#[async_trait]
pub trait CustomQueueAdapter: Send + Sync + 'static {
    async fn enqueue(&self, topic: &str, event_data: Value);
    async fn subscribe(&self, topic: &str, id: &str, function_id: &str);
    async fn unsubscribe(&self, topic: &str, id: &str);
}

type CustomQueueAdapterFuture = AdapterFuture<dyn CustomQueueAdapter>;

pub struct CustomQueueAdapterRegistration {
    pub class: &'static str,
    pub factory: fn(Arc<Engine>, Option<Value>) -> CustomQueueAdapterFuture,
}

impl AdapterRegistrationEntry<dyn CustomQueueAdapter> for CustomQueueAdapterRegistration {
    fn class(&self) -> &'static str {
        self.class
    }

    fn factory(&self) -> fn(Arc<Engine>, Option<Value>) -> CustomQueueAdapterFuture {
        self.factory
    }
}

inventory::collect!(CustomQueueAdapterRegistration);

// =============================================================================
// 2. Implement your custom Adapters
// =============================================================================

type SubscriberMap = HashMap<String, Vec<(String, String)>>;
// Adapter 1: InMemoryQueueAdapter - stores subscribers in memory
pub struct InMemoryQueueAdapter {
    subscribers: Arc<TokioRwLock<SubscriberMap>>,
    engine: Arc<Engine>,
}

impl InMemoryQueueAdapter {
    pub async fn new(_config: Option<Value>, engine: Arc<Engine>) -> anyhow::Result<Self> {
        Ok(Self {
            subscribers: Arc::new(TokioRwLock::new(HashMap::new())),
            engine,
        })
    }
}

#[async_trait]
impl CustomQueueAdapter for InMemoryQueueAdapter {
    async fn enqueue(&self, topic: &str, event_data: Value) {
        let subscribers = self.subscribers.read().await;
        if let Some(subs) = subscribers.get(topic) {
            let mut invokes = vec![];
            for (_id, function_id) in subs {
                let invoke = self.engine.invoke_function(function_id, event_data.clone());
                invokes.push(invoke);
            }
            futures::future::join_all(invokes).await;
        }
    }

    async fn subscribe(&self, topic: &str, id: &str, function_id: &str) {
        self.subscribers
            .write()
            .await
            .entry(topic.to_string())
            .or_default()
            .push((id.to_string(), function_id.to_string()));
    }

    async fn unsubscribe(&self, topic: &str, id: &str) {
        if let Some(subs) = self.subscribers.write().await.get_mut(topic) {
            subs.retain(|(sub_id, _)| sub_id != id);
        }
    }
}

// Adapter 2: LoggingQueueAdapter - logs all events and forwards to another adapter
pub struct LoggingQueueAdapter {
    inner: Arc<dyn CustomQueueAdapter>,
}

impl LoggingQueueAdapter {
    pub async fn new(config: Option<Value>, engine: Arc<Engine>) -> anyhow::Result<Self> {
        // Get the inner adapter class from config, default to InMemoryQueueAdapter
        let inner_adapter_class = config
            .as_ref()
            .and_then(|v| v.get("inner_adapter"))
            .and_then(|v| v.as_str())
            .unwrap_or("my::InMemoryQueueAdapter");

        // Create the inner adapter
        let inner_adapter = match inner_adapter_class {
            "my::InMemoryQueueAdapter" => {
                Arc::new(InMemoryQueueAdapter::new(None, engine.clone()).await?)
                    as Arc<dyn CustomQueueAdapter>
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
impl CustomQueueAdapter for LoggingQueueAdapter {
    async fn enqueue(&self, topic: &str, event_data: Value) {
        tracing::info!(
            topic = %topic,
            event_data = %event_data,
            "LoggingQueueAdapter: Enqueuing message"
        );
        self.inner.enqueue(topic, event_data).await;
    }

    async fn subscribe(&self, topic: &str, id: &str, function_id: &str) {
        tracing::info!(
            topic = %topic,
            id = %id,
            function_id = %function_id,
            "LoggingQueueAdapter: Subscribing"
        );
        self.inner.subscribe(topic, id, function_id).await;
    }

    async fn unsubscribe(&self, topic: &str, id: &str) {
        tracing::info!(
            topic = %topic,
            id = %id,
            "LoggingQueueAdapter: Unsubscribing"
        );
        self.inner.unsubscribe(topic, id).await;
    }
}

fn make_inmemory_adapter(engine: Arc<Engine>, config: Option<Value>) -> CustomQueueAdapterFuture {
    Box::pin(async move {
        Ok(Arc::new(InMemoryQueueAdapter::new(config, engine).await?)
            as Arc<dyn CustomQueueAdapter>)
    })
}

fn make_logging_adapter(engine: Arc<Engine>, config: Option<Value>) -> CustomQueueAdapterFuture {
    Box::pin(async move {
        Ok(Arc::new(LoggingQueueAdapter::new(config, engine).await?)
            as Arc<dyn CustomQueueAdapter>)
    })
}

iii::register_adapter!(<CustomQueueAdapterRegistration> "my::InMemoryQueueAdapter", make_inmemory_adapter);
iii::register_adapter!(<CustomQueueAdapterRegistration> "my::LoggingQueueAdapter", make_logging_adapter);

// =============================================================================
// 3. Define your Module Config
// =============================================================================

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CustomQueueModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}

// =============================================================================
// 4. Create your custom Module
// =============================================================================

#[derive(Clone)]
pub struct CustomQueueModule {
    adapter: Arc<dyn CustomQueueAdapter>,
    engine: Arc<Engine>,
    _config: CustomQueueModuleConfig,
}

#[async_trait]
impl Module for CustomQueueModule {
    fn name(&self) -> &'static str {
        "CustomQueueModule"
    }
    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        Self::create_with_adapters(engine, config).await
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing CustomQueueModule");

        // Register a function to emit to queues
        self.engine.register_function(
            RegisterFunctionRequest {
                function_id: "custom_emit".to_string(),
                description: Some("Emit to custom queue".to_string()),
                request_format: Some(serde_json::json!({
                    "topic": { "type": "string" },
                    "data": { "type": "object" }
                })),
                response_format: None,
                metadata: None,
            },
            Box::new(self.clone()),
        );

        Ok(())
    }
}

#[async_trait]
impl ConfigurableModule for CustomQueueModule {
    type Config = CustomQueueModuleConfig;
    type Adapter = dyn CustomQueueAdapter;
    type AdapterRegistration = CustomQueueAdapterRegistration;
    const DEFAULT_ADAPTER_CLASS: &'static str = "my::InMemoryQueueAdapter";

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn CustomQueueAdapter>>>> =
            Lazy::new(|| RwLock::new(CustomQueueModule::build_registry()));
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

impl FunctionHandler for CustomQueueModule {
    fn handle_function(
        &self,
        _invocation_id: Option<Uuid>,
        _function_id: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'static>>
    {
        let adapter = self.adapter.clone();
        Box::pin(async move {
            let topic = input
                .get("topic")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let data = input.get("data").cloned().unwrap_or(Value::Null);

            if topic.is_empty() {
                return FunctionResult::Failure(ErrorBody {
                    code: "topic_not_set".into(),
                    message: "Topic is not set".into(),
                });
            }

            tracing::debug!(topic = %topic, data = %data, "Emitting to custom queue");
            adapter.enqueue(topic, data).await;

            FunctionResult::Success(None)
        })
    }
}

// =============================================================================
// 5. Register module and run
// =============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    iii::logging::init_log("config.yaml");

    // Register the custom module and add it to the engine using EngineBuilder
    EngineBuilder::new()
        .register_module::<CustomQueueModule>("my::CustomQueueModule")
        .add_module(
            // instead load from config file
            "my::CustomQueueModule",
            Some(serde_json::json!({
                "adapter": {
                    "class": "my::LoggingQueueAdapter",
                    "config": {
                        "inner_adapter": "my::InMemoryQueueAdapter"
                    }
                }
            })),
        )
        .address("127.0.0.1:49134")
        .build()
        .await?;

    tracing::info!("CustomQueueModule initialized successfully!");
    tracing::info!("You can now use the 'custom_emit' function to emit to queues");

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
//   - class: my::CustomQueueModule
//     config:
//       adapter:
//         class: my::LoggingQueueAdapter  # or my::InMemoryQueueAdapter
//         config:
//           inner_adapter: my::InMemoryQueueAdapter
