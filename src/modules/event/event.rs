use std::{pin::Pin, sync::Arc};

use async_trait::async_trait;
use colored::Colorize;
use futures::Future;
use serde_json::Value;
use tokio::sync::OnceCell;
use uuid::Uuid;

use super::config::EventModuleConfig;
use crate::{
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::FunctionHandler,
    modules::{adapter_registry::AdapterRegistry, configurable::Configurable, core_module::CoreModule},
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};

#[async_trait]
pub trait EventAdapter: Send + Sync + 'static {
    async fn emit(&self, topic: &str, event_data: Value);
    async fn subscribe(&self, topic: &str, id: &str, function_path: &str);
    async fn unsubscribe(&self, topic: &str, id: &str);
}

#[derive(Clone)]
pub struct EventCoreModule {
    adapter: Arc<OnceCell<Arc<dyn EventAdapter>>>,
    engine: Arc<Engine>,
    config: EventModuleConfig,
    adapter_registry: Option<Arc<AdapterRegistry>>,
}

impl Configurable for EventCoreModule {
    type Config = EventModuleConfig;

    fn with_config(engine: Arc<Engine>, config: Self::Config) -> Self {
        Self {
            adapter: Arc::new(OnceCell::new()),
            engine,
            config,
            adapter_registry: None,
        }
    }
}

impl EventCoreModule {
    /// Sets the adapter registry for dynamic adapter creation
    pub fn set_adapter_registry(&mut self, registry: Arc<AdapterRegistry>) {
        self.adapter_registry = Some(registry);
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

        Box::pin(async move {
            if !topic.is_empty() {
                self.adapter
                    .get()
                    .unwrap()
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
        Box::pin(async move {
            tracing::debug!(trigger = %trigger.id, "Unregistering trigger");

            self.adapter
                .get()
                .unwrap()
                .unsubscribe(
                    trigger
                        .config
                        .get("topic")
                        .unwrap_or_default()
                        .as_str()
                        .unwrap_or(""), // TODO throw error if topic is not set
                    &trigger.id,
                )
                .await;
            Ok(())
        })
    }
}

#[async_trait]
impl CoreModule for EventCoreModule {
    async fn initialize(&self) -> Result<(), anyhow::Error> {
        tracing::info!("Initializing EventModule");

        // Get adapter class from config or use default
        let adapter_class = self
            .config
            .adapter
            .as_ref()
            .map(|a| a.class.as_str())
            .unwrap_or("modules::event::RedisAdapter");

        let adapter_config = self.config.adapter.as_ref().and_then(|a| a.config.clone());

        // Create adapter using registry
        let adapter = match &self.adapter_registry {
            Some(registry) => {
                registry
                    .create_event_adapter(adapter_class, adapter_config, self.engine.clone())
                    .await?
            }
            None => {
                // Fallback to config-based creation if no registry
                self.config.create_adapter(self.engine.clone()).await?
            }
        };

        self.adapter
            .set(adapter)
            .map_err(|_| anyhow::anyhow!("Failed to set EventAdapter; adapter already set?"))?;

        let trigger_type = TriggerType {
            id: "event".to_string(),
            _description: "Event core module".to_string(),
            registrator: Box::new(self.clone()),
            worker_id: None,
        };

        let _ = self.engine.register_trigger_type(trigger_type).await;
        self.engine.register_function(
            RegisterFunctionRequest {
                function_path: "emit".to_string(),
                description: Some("Emit an event".to_string()),
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

impl FunctionHandler for EventCoreModule {
    fn handle_function(
        &self,
        _invocation_id: Option<Uuid>,
        _function_path: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, ErrorBody>> + Send + 'static>> {
        let adapter = Arc::clone(&self.adapter);

        tracing::debug!(input = %input, "Handling event function");

        Box::pin(async move {
            let topic = input
                .get("topic")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let event_data = input.get("data").cloned().unwrap_or(Value::Null);

            if topic.is_empty() {
                return Err(ErrorBody {
                    code: "topic_not_set".into(),
                    message: "Topic is not set".into(),
                });
            }

            tracing::debug!(topic = %topic, event_data = %event_data, "Emitting event");
            let _ = adapter.get().unwrap().emit(topic, event_data).await;

            Ok(Some(Value::Null))
        })
    }
}
