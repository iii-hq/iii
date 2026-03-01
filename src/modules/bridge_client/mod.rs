// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use iii_sdk::{III, IIIError};
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
    pub function_id: String,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone)]
pub struct BridgeClientModule {
    engine: Arc<Engine>,
    bridge: III,
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
            .unwrap_or_else(|| "ws://0.0.0.0:49134".to_string());

        let bridge = III::new(&url);

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
                function_id: "bridge.invoke".to_string(),
                description: Some("Invoke a function on the remote III instance".to_string()),
                request_format: None,
                response_format: None,
                metadata: None,
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
                        .call_with_timeout(&invoke.function_id, invoke.data, timeout)
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
                function_id: "bridge.invoke_async".to_string(),
                description: Some("Fire-and-forget invoke on the remote III instance".to_string()),
                request_format: None,
                response_format: None,
                metadata: None,
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

                    if let Err(err) = bridge.call_void(&invoke.function_id, invoke.data) {
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
                    function_id: local_function.clone(),
                    description: Some(format!("Forward to remote function {}", remote_function)),
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                Handler::new(move |input: Value| {
                    let bridge = bridge.clone();
                    let remote_function = remote_function.clone();
                    async move {
                        let timeout = timeout
                            .map(Duration::from_millis)
                            .unwrap_or_else(|| Duration::from_secs(30));

                        match bridge
                            .call_with_timeout(&remote_function, input, timeout)
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
                    match engine.call(&local_function, input).await {
                        Ok(result) => Ok(result.unwrap_or(Value::Null)),
                        Err(err) => Err(IIIError::Remote {
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use futures_util::{SinkExt, StreamExt};
    use iii_sdk::Message;
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};
    use uuid::Uuid;

    use super::*;

    fn build_module(config: BridgeClientConfig) -> BridgeClientModule {
        BridgeClientModule {
            engine: Arc::new(Engine::new()),
            bridge: III::new("ws://127.0.0.1:9"),
            config,
        }
    }

    async fn spawn_bridge_server(
        service_names: Arc<Mutex<Vec<(String, String)>>>,
        registered_functions: Arc<Mutex<Vec<String>>>,
        exposed_result_tx: tokio::sync::oneshot::Sender<(String, serde_json::Value)>,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind websocket listener");
        let addr = listener.local_addr().expect("listener address");

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept websocket");
            let mut websocket = accept_async(stream).await.expect("upgrade websocket");
            let mut exposed_result_tx = Some(exposed_result_tx);
            let mut sent_exposed_invocation = false;
            let exposed_invocation_id = Uuid::new_v4();

            while let Some(message) = websocket.next().await {
                let message = message.expect("websocket frame");
                let WsMessage::Text(text) = message else {
                    continue;
                };
                let protocol =
                    serde_json::from_str::<Message>(&text).expect("decode sdk protocol frame");

                match protocol {
                    Message::RegisterService { id, name, .. } => {
                        service_names
                            .lock()
                            .expect("lock service names")
                            .push((id, name));
                    }
                    Message::RegisterFunction { id, .. } => {
                        registered_functions
                            .lock()
                            .expect("lock registered functions")
                            .push(id.clone());

                        if id == "local.echo" && !sent_exposed_invocation {
                            sent_exposed_invocation = true;
                            let invoke = Message::InvokeFunction {
                                invocation_id: Some(exposed_invocation_id),
                                function_id: id,
                                data: json!({ "source": "server" }),
                                traceparent: None,
                                baggage: None,
                            };
                            websocket
                                .send(WsMessage::Text(
                                    serde_json::to_string(&invoke)
                                        .expect("serialize invoke function")
                                        .into(),
                                ))
                                .await
                                .expect("send invoke function");
                        }
                    }
                    Message::InvokeFunction {
                        invocation_id: Some(invocation_id),
                        function_id,
                        data,
                        ..
                    } => {
                        let response = Message::InvocationResult {
                            invocation_id,
                            function_id: function_id.clone(),
                            result: Some(json!({
                                "function_id": function_id,
                                "data": data,
                            })),
                            error: None,
                            traceparent: None,
                            baggage: None,
                        };
                        websocket
                            .send(WsMessage::Text(
                                serde_json::to_string(&response)
                                    .expect("serialize invocation result")
                                    .into(),
                            ))
                            .await
                            .expect("send invocation result");
                    }
                    Message::InvocationResult {
                        invocation_id,
                        function_id,
                        result,
                        ..
                    } if invocation_id == exposed_invocation_id => {
                        if let Some(tx) = exposed_result_tx.take() {
                            tx.send((function_id, result.unwrap_or(serde_json::Value::Null)))
                                .expect("send exposed invocation result");
                        }
                    }
                    _ => {}
                }
            }
        });

        format!("ws://{}", addr)
    }

    #[tokio::test]
    async fn bridge_client_create_register_initialize_and_handlers_work() {
        unsafe {
            std::env::remove_var("III_BRIDGE_URL");
        }

        let created = BridgeClientModule::create(Arc::new(Engine::new()), None)
            .await
            .expect("create bridge client");
        assert_eq!(created.name(), "Bridge Client");

        let config = BridgeClientConfig {
            url: Some("ws://127.0.0.1:9".to_string()),
            service_id: Some("svc-1".to_string()),
            service_name: Some("Bridge Service".to_string()),
            expose: vec![ExposeFunctionConfig {
                local_function: "local.echo".to_string(),
                remote_function: Some("remote.echo".to_string()),
            }],
            forward: vec![ForwardFunctionConfig {
                local_function: "forward.echo".to_string(),
                remote_function: "remote.echo".to_string(),
                timeout_ms: Some(1),
            }],
        };
        let module = build_module(config);
        let engine = module.engine.clone();

        module.register_functions(engine.clone());
        assert!(engine.functions.get("bridge.invoke").is_some());
        assert!(engine.functions.get("bridge.invoke_async").is_some());
        assert!(engine.functions.get("forward.echo").is_some());

        module.initialize().await.expect("initialize bridge client");

        let invoke = engine
            .functions
            .get("bridge.invoke")
            .expect("bridge.invoke handler");
        match invoke
            .clone()
            .call_handler(None, json!({ "bad": true }))
            .await
        {
            FunctionResult::Failure(err) => assert_eq!(err.code, "deserialization_error"),
            _ => panic!("invalid invoke input should fail"),
        }
        match invoke
            .call_handler(
                None,
                json!({
                    "function_id": "remote.echo",
                    "data": { "hello": "world" },
                    "timeout_ms": 1
                }),
            )
            .await
        {
            FunctionResult::Failure(err) => {
                assert_eq!(err.code, "bridge_error");
                assert!(!err.message.is_empty());
            }
            _ => panic!("timed out invoke should fail"),
        }

        let invoke_async = engine
            .functions
            .get("bridge.invoke_async")
            .expect("bridge.invoke_async handler");
        match invoke_async
            .call_handler(
                None,
                json!({
                    "function_id": "remote.echo",
                    "data": { "hello": "world" }
                }),
            )
            .await
        {
            FunctionResult::NoResult => {}
            _ => panic!("fire-and-forget invoke should return no result"),
        }

        let forward = engine
            .functions
            .get("forward.echo")
            .expect("forward handler");
        match forward.call_handler(None, json!({ "value": 1 })).await {
            FunctionResult::Failure(err) => {
                assert_eq!(err.code, "bridge_error");
                assert!(!err.message.is_empty());
            }
            _ => panic!("forward call should fail with timeout"),
        }
    }

    #[tokio::test]
    async fn bridge_client_initialize_covers_fallbacks_and_async_deserialization_errors() {
        let service_names = Arc::new(Mutex::new(Vec::new()));
        let registered_functions = Arc::new(Mutex::new(Vec::new()));
        let (exposed_result_tx, _exposed_result_rx) = tokio::sync::oneshot::channel();
        let url = spawn_bridge_server(
            service_names.clone(),
            registered_functions.clone(),
            exposed_result_tx,
        )
        .await;

        let config = BridgeClientConfig {
            url: Some(url),
            service_id: Some("svc-fallback".to_string()),
            service_name: None,
            expose: vec![ExposeFunctionConfig {
                local_function: "local.echo".to_string(),
                remote_function: None,
            }],
            forward: vec![],
        };
        let module = build_module(config);
        let engine = module.engine.clone();

        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "local.echo".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(
                |input| async move { FunctionResult::Success(Some(json!({ "echo": input }))) },
            ),
        );

        module.register_functions(engine.clone());
        module.initialize().await.expect("initialize bridge client");

        let invoke_async = engine
            .functions
            .get("bridge.invoke_async")
            .expect("bridge.invoke_async handler");
        match invoke_async
            .call_handler(None, json!({ "bad": true }))
            .await
        {
            FunctionResult::Failure(err) => assert_eq!(err.code, "deserialization_error"),
            _ => panic!("invalid invoke_async input should fail"),
        }
    }
}
