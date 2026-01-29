// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod config;

use std::{pin::Pin, sync::Arc};

use futures::Future;
use serde_json::Value;

use crate::{
    builtins::kv::BuiltinKvStore,
    config::persistence::{
        delete_http_function_from_kv, load_http_functions_from_kv, store_http_function_in_kv,
    },
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::FunctionResult,
    invocation::{
        auth::resolve_auth_ref,
        http_function::HttpFunctionConfig,
        http_invoker::{HttpEndpointParams, HttpInvoker, HttpInvokerConfig},
        method::HttpAuth,
        url_validator::UrlValidatorConfig,
    },
    modules::module::Module,
    protocol::ErrorBody,
    trigger::TriggerType,
    triggers::http_registrator::HttpTriggerRegistrator,
    triggers::http_trigger::HttpTriggerConfig,
};

use config::HttpFunctionsConfig;

type HandlerFuture = Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send>>;

#[derive(Clone)]
pub struct HttpFunctionsModule {
    engine: Arc<Engine>,
    http_invoker: Arc<HttpInvoker>,
    kv_store: Arc<BuiltinKvStore>,
    http_trigger_registrator: Arc<HttpTriggerRegistrator>,
    config: HttpFunctionsConfig,
}

impl HttpFunctionsModule {
    fn create_handler_wrapper(
        &self,
        config: HttpFunctionConfig,
        auth: Option<HttpAuth>,
    ) -> crate::engine::Handler<impl Fn(Value) -> HandlerFuture + Send + Sync + 'static> {
        let invoker = self.http_invoker.clone();
        let function_path = config.function_path.clone();
        let url = config.url.clone();
        let method = config.method.clone();
        let timeout_ms = config.timeout_ms;
        let headers = config.headers.clone();

        crate::engine::Handler::new(move |input: Value| -> HandlerFuture {
            let invoker = invoker.clone();
            let function_path = function_path.clone();
            let url = url.clone();
            let method = method.clone();
            let headers = headers.clone();
            let auth = auth.clone();

            Box::pin(async move {
                let endpoint = HttpEndpointParams {
                    url: &url,
                    method: &method,
                    timeout_ms: &timeout_ms,
                    headers: &headers,
                    auth: &auth,
                };

                match invoker
                    .invoke_http(
                        &function_path,
                        &endpoint,
                        uuid::Uuid::new_v4(),
                        input,
                        None,
                        None,
                    )
                    .await
                {
                    Ok(result) => FunctionResult::Success(result),
                    Err(e) => FunctionResult::Failure(e),
                }
            })
        })
    }

    pub async fn register_http_function(
        &self,
        config: HttpFunctionConfig,
    ) -> Result<(), ErrorBody> {
        self.http_invoker
            .url_validator()
            .validate(&config.url)
            .await
            .map_err(|e| ErrorBody {
                code: "url_validation_failed".into(),
                message: e.to_string(),
            })?;

        let auth = config.auth.as_ref().map(resolve_auth_ref).transpose()?;

        let handler = self.create_handler_wrapper(config.clone(), auth);

        self.engine.register_function_handler(
            RegisterFunctionRequest {
                function_path: config.function_path.clone(),
                description: config.description,
                request_format: config.request_format,
                response_format: config.response_format,
                metadata: config.metadata,
            },
            handler,
        );

        Ok(())
    }

    pub async fn persist_and_register(&self, config: HttpFunctionConfig) -> Result<(), ErrorBody> {
        store_http_function_in_kv(&self.kv_store, &config).await?;
        self.register_http_function(config).await
    }

    pub async fn unregister_http_function(&self, function_path: &str) -> Result<(), ErrorBody> {
        self.engine.functions.remove(function_path);
        delete_http_function_from_kv(&self.kv_store, function_path).await
    }

    pub async fn register_http_trigger(&self, config: HttpTriggerConfig) -> Result<(), ErrorBody> {
        let _function = self
            .engine
            .functions
            .get(&config.function_path)
            .ok_or_else(|| ErrorBody {
                code: "function_not_found".into(),
                message: format!(
                    "http_trigger '{}' references '{}' but no function with that path exists",
                    config.trigger_id, config.function_path
                ),
            })?;

        let trigger = crate::trigger::Trigger {
            id: config.trigger_id,
            trigger_type: format!("http_{}", config.trigger_type),
            function_path: config.function_path,
            config: config.config,
            worker_id: None,
        };

        self.engine
            .trigger_registry
            .register_trigger(trigger)
            .await
            .map_err(|e| ErrorBody {
                code: "trigger_registration_failed".into(),
                message: e.to_string(),
            })?;

        Ok(())
    }

    pub fn kv_store(&self) -> &Arc<BuiltinKvStore> {
        &self.kv_store
    }

    pub fn http_invoker(&self) -> &Arc<HttpInvoker> {
        &self.http_invoker
    }
}

#[async_trait::async_trait]
impl Module for HttpFunctionsModule {
    fn name(&self) -> &'static str {
        "HttpFunctionsModule"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        let config: HttpFunctionsConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let allowlist = if config.security.url_allowlist.is_empty() {
            vec!["*".to_string()]
        } else {
            config.security.url_allowlist.clone()
        };

        let url_validator = UrlValidatorConfig {
            allowlist,
            block_private_ips: config.security.block_private_ips,
            require_https: config.security.require_https,
        };

        let http_invoker = Arc::new(HttpInvoker::new(HttpInvokerConfig {
            url_validator,
            ..HttpInvokerConfig::default()
        })?);

        let kv_store = engine
            .service_registry
            .get_service::<BuiltinKvStore>("kv_store")
            .unwrap_or_else(|| Arc::new(BuiltinKvStore::new(None)));

        let http_trigger_registrator = Arc::new(HttpTriggerRegistrator::new(
            http_invoker.clone(),
            engine.functions.clone(),
            kv_store.clone(),
        ));

        Ok(Box::new(Self {
            engine,
            http_invoker,
            kv_store,
            http_trigger_registrator,
            config,
        }))
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> anyhow::Result<()> {
        self.engine
            .service_registry
            .register_service("http_functions", Arc::new(self.clone()));

        let registrator_cron = (*self.http_trigger_registrator).clone();
        let registrator_event = (*self.http_trigger_registrator).clone();

        self.engine
            .trigger_registry
            .register_trigger_type(TriggerType {
                id: "http_cron".to_string(),
                _description: "HTTP Cron Trigger".to_string(),
                registrator: Box::new(registrator_cron),
                worker_id: None,
            })
            .await?;

        self.engine
            .trigger_registry
            .register_trigger_type(TriggerType {
                id: "http_event".to_string(),
                _description: "HTTP Event Trigger".to_string(),
                registrator: Box::new(registrator_event),
                worker_id: None,
            })
            .await?;

        for func_config in &self.config.functions {
            self.persist_and_register(func_config.clone())
                .await
                .map_err(|e| anyhow::anyhow!(e.message))?;
        }

        load_http_functions_from_kv(&self.kv_store, &self.engine, self)
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;

        for trigger_config in &self.config.triggers {
            self.register_http_trigger(trigger_config.clone())
                .await
                .map_err(|e| anyhow::anyhow!(e.message))?;
        }

        Ok(())
    }
}

crate::register_module!(
    "modules::http_functions::HttpFunctionsModule",
    HttpFunctionsModule
);
