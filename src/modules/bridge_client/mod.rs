// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use iii_sdk::{Bridge, BridgeError};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::module::Module,
    protocol::ErrorBody,
};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BridgeClientConfig {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub service_id: Option<String>,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub expose: Vec<ExposeFunctionConfig>,
    #[serde(default)]
    pub forward: Vec<ForwardFunctionConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExposeFunctionConfig {
    pub local_function: String,
    #[serde(default)]
    pub remote_function: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ForwardFunctionConfig {
    pub local_function: String,
    pub remote_function: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InvokeInput {
    pub function_path: String,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone)]
pub struct BridgeClientModule {
    engine: Arc<Engine>,
    bridge: Bridge,
    config: BridgeClientConfig,
}

#[async_trait]
impl Module for BridgeClientModule {
    fn name(&self) -> &'static str {
        "Bridge Client"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        let config: BridgeClientConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let url = config
            .url
            .clone()
            .or_else(|| std::env::var("III_BRIDGE_URL").ok())
            .unwrap_or_else(|| "ws://127.0.0.1:49134".to_string());

        let bridge = Bridge::new(&url);

        Ok(Box::new(Self {
            engine,
            bridge,
            config,
        }))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        let bridge = self.bridge.clone();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_path: "bridge.invoke".to_string(),
                description: Some("Invoke a function on the remote III instance".to_string()),
                request_format: None,
                response_format: None,
                metadata: None,
                worker_id: None,
            },
            Handler::new(move |input: Value| {
                let bridge = bridge.clone();
                async move {
                    let parsed: Result<InvokeInput, _> = serde_json::from_value(input);
                    let invoke = match parsed {
                        Ok(v) => v,
                        Err(err) => {
                            return FunctionResult::Failure(ErrorBody {
                                code: "deserialization_error".into(),
                                message: format!("Failed to parse invoke input: {}", err),
                            });
                        }
                    };

                    let timeout = invoke
                        .timeout_ms
                        .map(Duration::from_millis)
                        .unwrap_or_else(|| Duration::from_secs(30));

                    match bridge
                        .invoke_function_with_timeout(&invoke.function_path, invoke.data, timeout)
                        .await
                    {
                        Ok(result) => FunctionResult::Success(Some(result)),
                        Err(err) => {
                            dbg!(&err);
                            FunctionResult::Failure(ErrorBody {
                                code: "bridge_error".into(),
                                message: err.to_string(),
                            })
                        }
                    }
                }
            }),
        );

        let bridge = self.bridge.clone();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_path: "bridge.invoke_async".to_string(),
                description: Some("Fire-and-forget invoke on the remote III instance".to_string()),
                request_format: None,
                response_format: None,
                metadata: None,
                worker_id: None,
            },
            Handler::new(move |input: Value| {
                let bridge = bridge.clone();
                async move {
                    let parsed: Result<InvokeInput, _> = serde_json::from_value(input);
                    let invoke = match parsed {
                        Ok(v) => v,
                        Err(err) => {
                            return FunctionResult::Failure(ErrorBody {
                                code: "deserialization_error".into(),
                                message: format!("Failed to parse invoke input: {}", err),
                            });
                        }
                    };

                    if let Err(err) =
                        bridge.invoke_function_async(&invoke.function_path, invoke.data)
                    {
                        dbg!(&err);
                        return FunctionResult::Failure(ErrorBody {
                            code: "bridge_error".into(),
                            message: err.to_string(),
                        });
                    }

                    FunctionResult::NoResult
                }
            }),
        );

        for forward in &self.config.forward {
            let bridge = self.bridge.clone();
            let local_function = forward.local_function.clone();
            let remote_function = forward.remote_function.clone();
            let timeout = forward.timeout_ms;

            engine.register_function_handler(
                RegisterFunctionRequest {
                    function_path: local_function.clone(),
                    description: Some(format!("Forward to remote function {}", remote_function)),
                    request_format: None,
                    response_format: None,
                    metadata: None,
                    worker_id: None,
                },
                Handler::new(move |input: Value| {
                    let bridge = bridge.clone();
                    let remote_function = remote_function.clone();
                    async move {
                        let timeout = timeout
                            .map(Duration::from_millis)
                            .unwrap_or_else(|| Duration::from_secs(30));

                        match bridge
                            .invoke_function_with_timeout(&remote_function, input, timeout)
                            .await
                        {
                            Ok(result) => FunctionResult::Success(Some(result)),
                            Err(err) => {
                                dbg!(&err);
                                FunctionResult::Failure(ErrorBody {
                                    code: "bridge_error".into(),
                                    message: err.to_string(),
                                })
                            }
                        }
                    }
                }),
            );
        }
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        self.bridge
            .connect()
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;

        if let Some(service_id) = &self.config.service_id {
            let name = self
                .config
                .service_name
                .clone()
                .unwrap_or_else(|| service_id.clone());
            self.bridge
                .register_service_with_name(service_id.clone(), name, None);
        }

        for expose in &self.config.expose {
            let bridge = self.bridge.clone();
            let engine = self.engine.clone();
            let local_function = expose.local_function.clone();
            let remote_function = expose
                .remote_function
                .clone()
                .unwrap_or_else(|| local_function.clone());

            bridge.register_function(remote_function, move |input| {
                let engine = engine.clone();
                let local_function = local_function.clone();
                async move {
                    match engine.invoke_function(&local_function, input).await {
                        Ok(result) => Ok(result.unwrap_or(Value::Null)),
                        Err(err) => Err(BridgeError::Remote {
                            code: err.code,
                            message: err.message,
                        }),
                    }
                }
            });
        }

        Ok(())
    }
}

crate::register_module!(
    "modules::bridge_client::BridgeClientModule",
    BridgeClientModule
);
