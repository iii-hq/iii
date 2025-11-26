use std::{pin::Pin, sync::Arc};

use async_trait::async_trait;
use colored::Colorize;
use futures::Future;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::FunctionHandler,
    modules::logger::{LogLevel, log},
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
    adapter: Arc<dyn EventAdapter>,
    engine: Arc<Engine>,
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

        log(
            LogLevel::Info,
            "core::EventCoreModule",
            &format!(
                "{} Subscription {} → {}",
                "[REGISTERED]".green(),
                topic.purple(),
                trigger.function_path.cyan()
            ),
            None,
            None,
        );

        Box::pin(async move {
            if !topic.is_empty() {
                self.adapter
                    .subscribe(&topic, &trigger.id, &trigger.function_path)
                    .await;
            } else {
                log(
                    LogLevel::Warn,
                    "core::EventCoreModule",
                    &format!(
                        "Topic is not set for trigger {}",
                        trigger.function_path.purple()
                    ),
                    None,
                    None,
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
                .unsubscribe(
                    &trigger
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

impl EventCoreModule {
    pub fn new(adapter: Arc<dyn EventAdapter>, engine: Arc<Engine>) -> Self {
        Self { adapter, engine }
    }

    pub async fn initialize(&self) {
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
                // functions
            },
            Box::new(self.clone()),
        );
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
            let _ = adapter.emit(topic, event_data).await;

            Ok(Some(Value::Null))
        })
    }
}
