use std::{
    pin::Pin,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use colored::Colorize;
use futures::Future;
use serde_json::Value;
use uuid::Uuid;

use super::{adapters::RedisAdapter, config::EventModuleConfig};
use crate::{
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::FunctionHandler,
    modules::core_module::CoreModule,
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};

#[async_trait]
pub trait EventAdapter: Send + Sync + 'static {
    async fn emit(&self, topic: &str, event_data: Value);
    async fn subscribe(&self, topic: &str, id: &str, function_path: &str);
    async fn unsubscribe(&self, topic: &str, id: &str);
}

pub struct EventCoreModule {
    adapter: Arc<dyn EventAdapter>,
    engine: Arc<Engine>,
    config: EventModuleConfig,
}

impl Clone for EventCoreModule {
    fn clone(&self) -> Self {
        // Clone the current adapter if it exists
        Self {
            adapter: self.adapter.clone(),
            engine: self.engine.clone(),
            config: self.config.clone(),
        }
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
        let config: super::config::EventModuleConfig = config
            .map(|v| serde_json::from_value(v))
            .transpose()?
            .unwrap_or_default();

        let redis_url = config
            .adapter
            .as_ref()
            .and_then(|a| a.config.as_ref())
            .and_then(|c| c.get("redis_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("redis://localhost:6379");

        // Create the adapter
        let adapter = RedisAdapter::new(redis_url.to_string(), engine.clone()).await?;

        Ok(Box::new(Self {
            engine,
            config,
            adapter: Arc::new(adapter),
        }))
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
        tracing::debug!(input = %input, "Handling event function");

        let adapter = self.adapter.clone();
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
            adapter.emit(topic, event_data).await;

            Ok(Some(Value::Null))
        })
    }
}
