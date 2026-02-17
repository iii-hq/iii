// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{pin::Pin, sync::Arc};

use futures::Future;
use serde_json::Value;

use crate::{
    builtins::kv::BuiltinKvStore,
    engine::Engine,
    function::FunctionsRegistry,
    invocation::{
        auth::resolve_auth_ref,
        http_function::HttpFunctionConfig,
        http_invoker::{HttpEndpointParams, HttpInvoker},
    },
    config::persistence::http_function_key,
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
    pub fn new(
        http_invoker: Arc<HttpInvoker>,
        functions: Arc<FunctionsRegistry>,
        kv_store: Arc<BuiltinKvStore>,
    ) -> Self {
        Self {
            http_invoker,
            functions,
            kv_store,
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
            .get(&trigger.function_id)
            .ok_or_else(|| ErrorBody {
                code: "function_not_found".into(),
                message: format!("Function '{}' not found", trigger.function_id),
            })?;

        let index = http_function_key(&trigger.function_id);
        let value = self
            .kv_store
            .get(index, "config".to_string())
            .await
            .ok_or_else(|| ErrorBody {
                code: "config_not_found".into(),
                message: format!(
                    "HTTP function config not found for '{}'",
                    trigger.function_id
                ),
            })?;

        let config: HttpFunctionConfig =
            serde_json::from_value(value).map_err(|err| ErrorBody {
                code: "config_decode_failed".into(),
                message: err.to_string(),
            })?;

        let auth = config.auth.as_ref().map(resolve_auth_ref).transpose()?;

        let endpoint = HttpEndpointParams {
            url: &config.url,
            method: &config.method,
            timeout_ms: &config.timeout_ms,
            headers: &config.headers,
            auth: &auth,
        };

        self.http_invoker
            .deliver_webhook(
                &trigger.function_id,
                &endpoint,
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

        Box::pin(async move {
            functions
                .get(&trigger.function_id)
                .ok_or_else(|| anyhow::anyhow!("Function '{}' not found", trigger.function_id))?;

            let index = http_function_key(&trigger.function_id);
            kv_store
                .get(index, "config".to_string())
                .await
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "HTTP function config not found for '{}'",
                        trigger.function_id
                    )
                })?;

            tracing::info!(
                trigger_id = %trigger.id,
                trigger_type = %trigger.trigger_type,
                function_path = %trigger.function_id,
                "Registering HTTP trigger"
            );

            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        Box::pin(async move {
            tracing::info!(
                trigger_id = %trigger.id,
                trigger_type = %trigger.trigger_type,
                function_path = %trigger.function_id,
                "Unregistering HTTP trigger"
            );

            Ok(())
        })
    }
}
