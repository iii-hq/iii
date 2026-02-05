use std::{pin::Pin, sync::Arc};

use futures::Future;
use serde_json::Value;

use crate::{
    function::FunctionsRegistry,
    invocation::http_invoker::HttpInvoker,
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator, TriggerRegistry},
};

#[derive(Clone)]
pub struct HttpTriggerRegistrator {
    http_invoker: Arc<HttpInvoker>,
    functions: Arc<FunctionsRegistry>,
}

impl HttpTriggerRegistrator {
    pub fn new(http_invoker: Arc<HttpInvoker>, functions: Arc<FunctionsRegistry>) -> Self {
        Self {
            http_invoker,
            functions,
        }
    }

    pub async fn deliver(&self, trigger: &Trigger, payload: Value) -> Result<(), ErrorBody> {
        let function = self.functions.get(&trigger.function_path)
            .ok_or_else(|| ErrorBody {
                code: "function_not_found".into(),
                message: format!("Function '{}' not found", trigger.function_path),
            })?;

        self.http_invoker
            .deliver_webhook(&function, &trigger.trigger_type, &trigger.id, payload)
            .await
    }

    pub async fn deliver_to_matching_triggers<F, P>(
        &self,
        trigger_registry: &TriggerRegistry,
        trigger_type: &str,
        filter: F,
        payload_builder: P,
    )
    where
        F: Fn(&Trigger) -> bool,
        P: Fn(&Trigger) -> Value,
    {
        let triggers: Vec<Trigger> = trigger_registry.triggers
            .iter()
            .filter(|entry| {
                let trigger = entry.value();
                trigger.trigger_type == trigger_type && filter(trigger)
            })
            .map(|entry| entry.value().clone())
            .collect();

        for trigger in triggers {
            let http_invoker = self.http_invoker.clone();
            let functions = self.functions.clone();
            let payload = payload_builder(&trigger);
            
            tokio::spawn(async move {
                if let Some(function) = functions.get(&trigger.function_path) {
                    let _ = http_invoker.deliver_webhook(&function, &trigger.trigger_type, &trigger.id, payload).await;
                }
            });
        }
    }
}

impl TriggerRegistrator for HttpTriggerRegistrator {
    fn register_trigger(
        &self,
        _trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn unregister_trigger(
        &self,
        _trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }
}
