// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod config;

use std::{pin::Pin, sync::Arc};

use dashmap::DashMap;
use futures::Future;
use serde_json::Value;

use crate::{
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::FunctionResult,
    invocation::{
        auth::resolve_auth_ref,
        http_function::HttpFunctionConfig,
        http_invoker::{HttpEndpointParams, HttpInvoker, HttpInvokerConfig},
        method::HttpAuth,
    },
    modules::module::Module,
    protocol::ErrorBody,
};

use config::HttpFunctionsConfig;

type HandlerFuture = Pin<Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send>>;

#[derive(Clone)]
pub struct HttpFunctionsModule {
    engine: Arc<Engine>,
    http_invoker: Arc<HttpInvoker>,
    http_functions: Arc<DashMap<String, HttpFunctionConfig>>,
    #[allow(dead_code)]
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
                function_id: config.function_path.clone(),
                description: config.description.clone(),
                request_format: config.request_format.clone(),
                response_format: config.response_format.clone(),
                metadata: config.metadata.clone(),
            },
            handler,
        );

        self.http_functions
            .insert(config.function_path.clone(), config);

        Ok(())
    }

    pub async fn unregister_http_function(&self, function_path: &str) -> Result<(), ErrorBody> {
        self.http_functions.remove(function_path);
        self.engine.functions.remove(function_path);
        Ok(())
    }

    pub fn http_invoker(&self) -> &Arc<HttpInvoker> {
        &self.http_invoker
    }

    pub fn http_functions(&self) -> &Arc<DashMap<String, HttpFunctionConfig>> {
        &self.http_functions
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

        let http_invoker = Arc::new(HttpInvoker::new(HttpInvokerConfig {
            url_validator: config.security.clone().into(),
            ..HttpInvokerConfig::default()
        })?);

        let http_functions = Arc::new(DashMap::new());

        Ok(Box::new(Self {
            engine,
            http_invoker,
            http_functions,
            config,
        }))
    }

    fn register_functions(&self, _engine: Arc<Engine>) {}

    async fn initialize(&self) -> anyhow::Result<()> {
        self.engine
            .service_registry
            .register_service("http_functions", Arc::new(self.clone()));

        Ok(())
    }
}

crate::register_module!(
    "modules::http_functions::HttpFunctionsModule",
    HttpFunctionsModule
);
