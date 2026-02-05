use std::{pin::Pin, sync::Arc};

use dashmap::DashMap;
use futures::Future;
use serde_json::Value;

use crate::{
    builtins::kv::BuiltinKvStore,
    engine::Engine,
    function::FunctionsRegistry,
    invocation::{
        auth::resolve_auth_ref, http_function::HttpFunctionConfig, http_invoker::HttpInvoker,
    },
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator, TriggerRegistry},
};

#[derive(Clone)]
pub struct HttpTriggerRegistrator {
    http_invoker: Arc<HttpInvoker>,
    functions: Arc<FunctionsRegistry>,
    kv_store: Arc<BuiltinKvStore>,
    triggers: Arc<DashMap<String, Trigger>>,
}

impl HttpTriggerRegistrator {
    pub fn new(
        http_invoker: Arc<HttpInvoker>,
        functions: Arc<FunctionsRegistry>,
        kv_store: Arc<BuiltinKvStore>,
    ) -> Self {
        Self {
            http_invoker,
            functions,
            kv_store,
            triggers: Arc::new(DashMap::new()),
        }
    }

    pub async fn dispatch_http_triggers_from_engine<F, P>(
        engine: &Arc<Engine>,
        trigger_type: &str,
        filter: F,
        payload_builder: P,
    ) where
        F: Fn(&Trigger) -> bool,
        P: Fn(&Trigger) -> Value,
    {
        if let Some(http_module) = engine
            .service_registry
            .get_service::<crate::modules::http_functions::HttpFunctionsModule>(
            "http_functions",
        ) {
            let registrator = Self::new(
                http_module.http_invoker().clone(),
                engine.functions.clone(),
                http_module.kv_store().clone(),
            );
            registrator
                .deliver_to_matching_triggers(
                    &engine.trigger_registry,
                    trigger_type,
                    filter,
                    payload_builder,
                )
                .await;
        }
    }

    pub async fn deliver(&self, trigger: &Trigger, payload: Value) -> Result<(), ErrorBody> {
        self.functions
            .get(&trigger.function_path)
            .ok_or_else(|| ErrorBody {
                code: "function_not_found".into(),
                message: format!("Function '{}' not found", trigger.function_path),
            })?;

        let index = format!("http_function:{}", trigger.function_path);
        let value = self
            .kv_store
            .get(index, "config".to_string())
            .await
            .ok_or_else(|| ErrorBody {
                code: "config_not_found".into(),
                message: format!(
                    "HTTP function config not found for '{}'",
                    trigger.function_path
                ),
            })?;

        let config: HttpFunctionConfig =
            serde_json::from_value(value).map_err(|err| ErrorBody {
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
    ) where
        F: Fn(&Trigger) -> bool,
        P: Fn(&Trigger) -> Value,
    {
        let triggers: Vec<Trigger> = trigger_registry
            .triggers
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
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let functions = self.functions.clone();
        let kv_store = self.kv_store.clone();
        let triggers = self.triggers.clone();

        Box::pin(async move {
            functions
                .get(&trigger.function_path)
                .ok_or_else(|| anyhow::anyhow!("Function '{}' not found", trigger.function_path))?;

            let index = format!("http_function:{}", trigger.function_path);
            kv_store
                .get(index, "config".to_string())
                .await
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "HTTP function config not found for '{}'",
                        trigger.function_path
                    )
                })?;

            tracing::info!(
                trigger_id = %trigger.id,
                trigger_type = %trigger.trigger_type,
                function_path = %trigger.function_path,
                "Registering HTTP trigger"
            );

            triggers.insert(trigger.id.clone(), trigger);
            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let triggers = self.triggers.clone();

        Box::pin(async move {
            tracing::info!(
                trigger_id = %trigger.id,
                trigger_type = %trigger.trigger_type,
                function_path = %trigger.function_path,
                "Unregistering HTTP trigger"
            );

            triggers.remove(&trigger.id);
            Ok(())
        })
    }
}
