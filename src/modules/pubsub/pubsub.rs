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
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{PubSubAdapter, config::PubSubModuleConfig};
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::core_module::{AdapterFactory, ConfigurableModule, CoreModule},
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};

#[derive(Clone)]
pub struct PubSubCoreModule {
    adapter: Arc<dyn PubSubAdapter>,
    engine: Arc<Engine>,
    _config: PubSubModuleConfig,
}

#[derive(Serialize, Deserialize)]
pub struct PubSubInput {
    pub topic: String,
    pub data: Value,
}

#[service(name = "pubsub")]
impl PubSubCoreModule {
    #[function(name = "publish", description = "Publishes an event")]
    pub async fn publish(&self, input: PubSubInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let adapter = self.adapter.clone();
        let event_data = input.data;
        let topic = input.topic;

        if topic.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "topic_not_set".into(),
                message: "Topic is not set".into(),
            });
        }

        tracing::debug!(topic = %topic, event_data = %event_data, "Publishing event");
        let _ = adapter.publish(&topic, event_data).await;

        FunctionResult::Success(None)
    }
}

impl TriggerRegistrator for PubSubCoreModule {
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
            "{} PubSub subscription {} → {}",
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
impl CoreModule for PubSubCoreModule {
    fn name(&self) -> &'static str {
        "PubSubModule"
    }
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
        tracing::info!("Initializing PubSubModule");

        let trigger_type = TriggerType {
            id: "subscribe".to_string(),
            _description: "Subscribe to a topic".to_string(),
            registrator: Box::new(self.clone()),
            worker_id: None,
        };

        let _ = self.engine.register_trigger_type(trigger_type).await;

        Ok(())
    }
}

#[async_trait]
impl ConfigurableModule for PubSubCoreModule {
    type Config = PubSubModuleConfig;
    type Adapter = dyn PubSubAdapter;
    type AdapterRegistration = super::registry::PubSubAdapterRegistration;
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::pubsub::LocalAdapter";

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn PubSubAdapter>>>> =
            Lazy::new(|| RwLock::new(PubSubCoreModule::build_registry()));
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

crate::register_module!(
    "modules::pubsub::PubSubModule",
    PubSubCoreModule,
    enabled_by_default = true
);
