use std::{pin::Pin, sync::Arc};

use futures::Future;

use serde_json::Value;
use uuid::Uuid;

use crate::engine::{Engine, EngineTrait, RegisterFunctionRequest};
use crate::function::FunctionHandler;
use crate::protocol::ErrorBody;
use crate::trigger::{Trigger, TriggerRegistrator, TriggerType};

pub trait EventAdapter: Send + Sync + 'static {
    fn emit(&self, topic: &str, event_data: Value);
    fn subscribe(&self, topic: &str, id: &str, function_path: &str);
    fn unsubscribe(&self, topic: &str, id: &str);
}

#[derive(Clone)]
pub struct EventCoreModule<'a> {
    adapter: Arc<dyn EventAdapter>,
    engine: Arc<&'a Engine<'static>>,
}

impl<'a> TriggerRegistrator<'a> for EventCoreModule<'a> {
    fn register_trigger(
        &'a self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + 'a>> {
        Box::pin(async move {
            self.adapter.subscribe(
                &trigger
                    .config
                    .get("topic")
                    .unwrap_or_default()
                    .as_str()
                    .unwrap_or(""), // TODO throw error if topic is not set
                &trigger.id,
                &trigger.function_path,
            );

            Ok(())
        })
    }

    fn unregister_trigger(
        &'a self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + 'a>> {
        Box::pin(async move {
            self.adapter.unsubscribe(
                &trigger
                    .config
                    .get("topic")
                    .unwrap_or_default()
                    .as_str()
                    .unwrap_or(""), // TODO throw error if topic is not set
                &trigger.id,
            );

            Ok(())
        })
    }
}

impl<'a> EventCoreModule<'a> {
    pub fn new(adapter: Arc<dyn EventAdapter>, engine: &'a Engine<'static>) -> Self {
        let module = Self {
            adapter,
            engine: Arc::new(engine),
        };

        module.initialize();

        module
    }

    fn initialize(&'a self) {
        let trigger_type = Box::new(TriggerType {
            id: "event".to_string(),
            _description: "Event core module".to_string(),
            registrator: Box::new(self.clone()),
            worker_id: None,
        });
        let trigger_type_ref = Box::leak(trigger_type);
        self.engine.register_trigger_type(trigger_type_ref);

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
    }
}

impl<'a> FunctionHandler for EventCoreModule<'a> {
    fn handle_function(
        &self,
        _invocation_id: Option<Uuid>,
        _function_path: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, ErrorBody>> + Send + 'a>> {
        let adapter = Arc::clone(&self.adapter);

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

            adapter.emit(topic, event_data);

            Ok(None)
        })
    }
}
