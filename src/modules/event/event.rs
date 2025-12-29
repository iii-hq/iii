use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use colored::Colorize;
use function_macros::{function, service};
use futures::Future;
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde_json::Value;

use super::{EventAdapter, config::EventModuleConfig};
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::{
        core_module::{AdapterFactory, ConfigurableModule, CoreModule},
        registry::ModuleFuture,
    },
    protocol::ErrorBody,
    register_module,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};

#[derive(Clone)]
pub struct EventCoreModule {
    adapter: Arc<dyn EventAdapter>,
    engine: Arc<Engine>,
    _config: EventModuleConfig,
}

#[derive(Deserialize)]
pub struct EventInput {
    topic: String,
    data: Value,
}

#[service(name = "event")]
impl EventCoreModule {
    #[function(name = "emit", description = "Emit an event")]
    pub async fn emit(&self, input: EventInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let adapter = self.adapter.clone();
        let event_data = input.data;
        let topic = input.topic;

        if topic.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "topic_not_set".into(),
                message: "Topic is not set".into(),
            });
        }

        tracing::debug!(topic = %topic, event_data = %event_data, "Emitting event");
        let _ = adapter.emit(&topic, event_data).await;

        FunctionResult::Success(None)
    }
}

impl TriggerRegistrator for EventCoreModule {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let topic = trigger
            .clone()
            .config
            .get("topic")
            .unwrap_or_default()
            .as_str()
            .unwrap_or("")
            .to_string();

        tracing::info!(
            "{} Subscription {} → {}",
            "[REGISTERED]".green(),
            topic.purple(),
            trigger.function_path.cyan()
        );

        // Get adapter reference before async block
        let adapter = self.adapter.clone();

        Box::pin(async move {
            if !topic.is_empty() {
                adapter
                    .subscribe(&topic, &trigger.id, &trigger.function_path)
                    .await;
            } else {
                tracing::warn!(
                    function_path = %trigger.function_path.purple(),
                    "Topic is not set for trigger"
                );
            }

            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        // Get adapter reference before async block
        let adapter = self.adapter.clone();

        Box::pin(async move {
            tracing::debug!(trigger = %trigger.id, "Unregistering trigger");
            adapter
                .unsubscribe(
                    trigger
                        .config
                        .get("topic")
                        .unwrap_or_default()
                        .as_str()
                        .unwrap_or(""),
                    &trigger.id,
                )
                .await;
            Ok(())
        })
    }
}

#[async_trait]
impl CoreModule for EventCoreModule {
    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing EventModule");

        let trigger_type = TriggerType {
            id: "event".to_string(),
            _description: "Event core module".to_string(),
            registrator: Box::new(self.clone()),
            worker_id: None,
        };

        let _ = self.engine.register_trigger_type(trigger_type).await;

        Ok(())
    }
}

#[async_trait]
impl ConfigurableModule for EventCoreModule {
    type Config = EventModuleConfig;
    type Adapter = dyn EventAdapter;
    type AdapterRegistration = super::registry::EventAdapterRegistration;
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::event::RedisAdapter";

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn EventAdapter>>>> =
            Lazy::new(|| RwLock::new(EventCoreModule::build_registry()));
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

fn make_module(engine: Arc<Engine>, config: Option<Value>) -> ModuleFuture {
    Box::pin(async move { EventCoreModule::create(engine, config).await })
}
register_module!("modules::event::EventModule", make_module, true);
