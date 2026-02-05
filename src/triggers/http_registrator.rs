use std::{pin::Pin, sync::Arc};

use futures::Future;
use serde_json::Value;

use crate::{
    builtins::kv::BuiltinKvStore,
    function::FunctionsRegistry,
    invocation::{auth::resolve_auth_ref, http_function::HttpFunctionConfig, http_invoker::HttpInvoker},
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator, TriggerRegistry},
};

#[derive(Clone)]
pub struct HttpTriggerRegistrator {
    http_invoker: Arc<HttpInvoker>,
    functions: Arc<FunctionsRegistry>,
    kv_store: Arc<BuiltinKvStore>,
}

impl HttpTriggerRegistrator {
    pub fn new(http_invoker: Arc<HttpInvoker>, functions: Arc<FunctionsRegistry>, kv_store: Arc<BuiltinKvStore>) -> Self {
        Self {
            http_invoker,
            functions,
            kv_store,
        }
    }

    pub async fn deliver(&self, trigger: &Trigger, payload: Value) -> Result<(), ErrorBody> {
        self.functions.get(&trigger.function_path)
            .ok_or_else(|| ErrorBody {
                code: "function_not_found".into(),
                message: format!("Function '{}' not found", trigger.function_path),
            })?;

        let index = format!("http_function:{}", trigger.function_path);
        let value = self.kv_store
            .get(index, "config".to_string())
            .await
            .ok_or_else(|| ErrorBody {
                code: "config_not_found".into(),
                message: format!("HTTP function config not found for '{}'", trigger.function_path),
            })?;

        let config: HttpFunctionConfig = serde_json::from_value(value).map_err(|err| ErrorBody {
            code: "config_decode_failed".into(),
            message: err.to_string(),
        })?;

        let auth = config.auth.as_ref().map(resolve_auth_ref).transpose()?;

        self.http_invoker
            .deliver_webhook(
                &trigger.function_path,
                &config.url,
                &config.method,
                &config.timeout_ms,
                &config.headers,
                &auth,
                &trigger.trigger_type,
                &trigger.id,
                payload,
            )
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
            let registrator = self.clone();
            let payload = payload_builder(&trigger);
            
            tokio::spawn(async move {
                let _ = registrator.deliver(&trigger, payload).await;
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
