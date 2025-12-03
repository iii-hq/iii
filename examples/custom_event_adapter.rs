//! Example: Registering a custom EventAdapter
//!
//! Run with: `cargo run --example custom_event_adapter`

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use engine::{
    EngineBuilder,
    engine::{Engine, EngineTrait},
    modules::{
        core_module::ConfigurableModule,
        event::{EventAdapter, EventCoreModule},
    },
};
use serde_json::Value;
use tokio::sync::RwLock;

// =============================================================================
// 1. Define your custom EventAdapter
// =============================================================================

pub struct InMemoryEventAdapter {
    subscribers: Arc<RwLock<HashMap<String, Vec<(String, String)>>>>,
    engine: Arc<Engine>,
}

impl InMemoryEventAdapter {
    pub async fn new(_config: Option<Value>, engine: Arc<Engine>) -> anyhow::Result<Self> {
        Ok(Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            engine,
        })
    }
}

// =============================================================================
// 2. Implement the EventAdapter trait
// =============================================================================

#[async_trait]
impl EventAdapter for InMemoryEventAdapter {
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

// =============================================================================
// 3. Register with EngineBuilder and run
// =============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    engine::logging::init_tracing();

    // Register the custom adapter with EventCoreModule
    EventCoreModule::add_adapter("my::InMemoryEventAdapter", |engine, config| async move {
        let adapter = InMemoryEventAdapter::new(config, engine).await?;
        Ok(Arc::new(adapter) as Arc<dyn EventAdapter>)
    })
    .await?;

    EngineBuilder::new()
        .config_file_or_default("config.yaml")?
        .address("127.0.0.1:49134")
        .build()
        .await?
        .serve()
        .await
}

// =============================================================================
// To use this adapter, set in config.yaml:
// =============================================================================
//
// modules:
//   - class: modules::event::EventModule
//     config:
//       adapter:
//         class: my::InMemoryEventAdapter
