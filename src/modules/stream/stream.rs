// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock as SyncRwLock},
};

use axum::{
    Router,
    extract::{ConnectInfo, State, WebSocketUpgrade, ws::WebSocket},
    http::{HeaderMap, Uri},
    response::IntoResponse,
    routing::get,
};
use chrono::Utc;
use colored::Colorize;
use function_macros::{function, service};
use iii_sdk::{
    UpdateResult,
    types::{DeleteResult, SetResult},
};
use once_cell::sync::Lazy;
use serde_json::Value;
use tokio::net::TcpListener;
use tracing::Instrument;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::{
        module::{AdapterFactory, ConfigurableModule, Module},
        stream::{
            StreamOutboundMessage, StreamSocketManager, StreamWrapperMessage,
            adapters::StreamAdapter,
            config::StreamModuleConfig,
            structs::{
                EventData, StreamAuthContext, StreamAuthInput, StreamDeleteInput, StreamGetInput,
                StreamListAllInput, StreamListGroupsInput, StreamListInput, StreamSendInput,
                StreamSetInput, StreamUpdateInput,
            },
            trigger::{
                JOIN_TRIGGER_TYPE, LEAVE_TRIGGER_TYPE, STREAM_TRIGGER_TYPE, StreamTrigger,
                StreamTriggers,
            },
            utils::{headers_to_map, query_to_multi_map},
        },
    },
    protocol::ErrorBody,
    trigger::TriggerType,
};

#[derive(Clone)]
pub struct StreamCoreModule {
    config: StreamModuleConfig,
    #[cfg_attr(not(test), allow(dead_code))]
    pub adapter: Arc<dyn StreamAdapter>,
    engine: Arc<Engine>,

    pub triggers: Arc<StreamTriggers>,
}

async fn ws_handler(
    State(module): State<Arc<StreamSocketManager>>,
    ws: WebSocketUpgrade,
    uri: Uri,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let module = module.clone();

    if let Some(auth_function) = module.auth_function.clone() {
        let engine = module.engine.clone();
        let input = StreamAuthInput {
            headers: headers_to_map(&headers),
            path: uri.path().to_string(),
            query_params: query_to_multi_map(uri.query()),
            addr: addr.to_string(),
        };
        let input = serde_json::to_value(input);

        match input {
            Ok(input) => match engine.call(&auth_function, input).await {
                Ok(Some(result)) => {
                    let context = serde_json::from_value::<StreamAuthContext>(result);

                    match context {
                        Ok(context) => {
                            return ws.on_upgrade(move |socket: WebSocket| async move {
                                    if let Err(err) = module.socket_handler(socket, Some(context)).await {
                                        tracing::error!(addr = %addr, error = ?err, "stream socket error");
                                    }
                                });
                        }
                        Err(err) => {
                            tracing::error!(error = ?err, "Failed to convert result to context");
                        }
                    }
                }
                Ok(None) => {
                    tracing::debug!("No result from auth function");
                }
                Err(err) => {
                    tracing::error!(error = ?err, "Failed to invoke auth function");
                }
            },
            Err(err) => {
                tracing::error!(error = ?err, "Failed to convert input to value");
            }
        }
    }

    ws.on_upgrade(move |socket: WebSocket| async move {
        if let Err(err) = module.socket_handler(socket, None).await {
            tracing::error!(addr = %addr, error = ?err, "stream socket error");
        }
    })
}

#[async_trait::async_trait]
impl Module for StreamCoreModule {
    fn name(&self) -> &'static str {
        "StreamCoreModule"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying StreamCoreModule");
        let _ = self.adapter.destroy().await;
        Ok(())
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing StreamCoreModule");

        let socket_manager = Arc::new(StreamSocketManager::new(
            self.engine.clone(),
            self.adapter.clone(),
            Arc::new(self.clone()),
            self.config.auth_function.clone(),
            self.triggers.clone(),
        ));
        let addr = format!("{}:{}", self.config.host, self.config.port)
            .parse::<SocketAddr>()
            .unwrap();
        tracing::info!("Starting StreamCoreModule on {}", addr.to_string().purple());
        let listener = TcpListener::bind(addr).await.unwrap();
        let app = Router::new()
            .route("/", get(ws_handler))
            .with_state(socket_manager);

        let _ = self
            .engine
            .register_trigger_type(TriggerType {
                id: JOIN_TRIGGER_TYPE.to_string(),
                _description: "Stream join trigger".to_string(),
                registrator: Box::new(self.clone()),
                worker_id: None,
            })
            .await;

        let _ = self
            .engine
            .register_trigger_type(TriggerType {
                id: LEAVE_TRIGGER_TYPE.to_string(),
                _description: "Stream leave trigger".to_string(),
                registrator: Box::new(self.clone()),
                worker_id: None,
            })
            .await;

        let _ = self
            .engine
            .register_trigger_type(TriggerType {
                id: STREAM_TRIGGER_TYPE.to_string(),
                _description: "Stream trigger".to_string(),
                registrator: Box::new(self.clone()),
                worker_id: None,
            })
            .await;

        tokio::spawn(async move {
            tracing::info!(
                "Stream API listening on address: {}",
                addr.to_string().purple()
            );

            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });

        let adapter = self.adapter.clone();
        tokio::spawn(async move {
            if let Err(e) = adapter.watch_events().await {
                tracing::error!(error = %e, "Failed to watch events");
            }
        });

        Ok(())
    }
}

#[async_trait::async_trait]
impl ConfigurableModule for StreamCoreModule {
    type Config = StreamModuleConfig;
    type Adapter = dyn StreamAdapter;
    type AdapterRegistration = super::registry::StreamAdapterRegistration;
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::stream::adapters::KvStore";

    async fn registry() -> &'static SyncRwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<SyncRwLock<HashMap<String, AdapterFactory<dyn StreamAdapter>>>> =
            Lazy::new(|| SyncRwLock::new(StreamCoreModule::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self {
            config,
            adapter,
            engine,
            triggers: Arc::new(StreamTriggers::new()),
        }
    }

    fn adapter_class_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.class.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

impl StreamCoreModule {
    /// Invoke triggers for a given event type with condition checks
    async fn invoke_triggers(&self, event_data: StreamWrapperMessage) {
        let engine = self.engine.clone();
        let event_stream_name = event_data.stream_name.clone();

        // Collect relevant trigger IDs and clone the triggers we need
        // Only triggers with matching stream_name are registered, so we only need to look up by stream_name
        let triggers_to_invoke: Vec<StreamTrigger> = {
            let by_name = self.triggers.stream_triggers_by_name.read().await;
            let triggers_map = self.triggers.stream_triggers.read().await;
            let mut triggers = Vec::new();

            // Get triggers for this specific stream_name
            if let Some(ids_for_stream) = by_name.get(&event_stream_name) {
                for trigger_id in ids_for_stream {
                    if let Some(trigger) = triggers_map.get(trigger_id) {
                        let group_id = trigger.config.group_id.clone().unwrap_or("".to_string());
                        let item_id = trigger.config.item_id.clone().unwrap_or("".to_string());
                        let event_item_id = event_data.id.clone().unwrap_or("".to_string());

                        if (!group_id.is_empty() && group_id != event_data.group_id)
                            || (!item_id.is_empty() && item_id != event_item_id)
                        {
                            continue;
                        }

                        triggers.push(trigger.clone());
                    }
                }
            }

            triggers
        };

        let current_span = tracing::Span::current();

        if let Ok(event_data) = serde_json::to_value(event_data) {
            tokio::spawn(async move {
                let mut has_error = false;

                for stream_trigger in triggers_to_invoke {
                    let trigger = &stream_trigger.trigger;

                    // Check condition if specified (using pre-parsed value)
                    let condition_function_id = stream_trigger.config.condition_function_id.clone();

                    if let Some(condition_function_id) = condition_function_id {
                        tracing::debug!(
                            condition_function_id = %condition_function_id,
                            "Checking trigger conditions"
                        );

                        match engine
                            .call(&condition_function_id, event_data.clone())
                            .await
                        {
                            Ok(Some(result)) => {
                                tracing::debug!(
                                    condition_function_id = %condition_function_id,
                                    result = ?result,
                                    "Condition function result"
                                );

                                if let Some(passed) = result.as_bool()
                                    && !passed
                                {
                                    tracing::debug!(
                                        function_id = %trigger.function_id,
                                        "Condition check failed, skipping handler"
                                    );
                                    continue;
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    condition_function_id = %condition_function_id,
                                    "Condition function returned no result"
                                );
                                continue;
                            }
                            Err(err) => {
                                tracing::error!(
                                    condition_function_id = %condition_function_id,
                                    error = ?err,
                                    "Error invoking condition function"
                                );
                                has_error = true;
                                continue;
                            }
                        }
                    }

                    // Invoke the handler function
                    tracing::debug!(
                        function_id = %trigger.function_id,
                        "Invoking trigger"
                    );

                    let call_result = engine.call(&trigger.function_id, event_data.clone()).await;

                    match call_result {
                        Ok(_) => {
                            tracing::debug!(
                                function_id = %trigger.function_id,
                                "Trigger handler invoked successfully"
                            );
                        }
                        Err(err) => {
                            has_error = true;
                            tracing::error!(
                                function_id = %trigger.function_id,
                                error = ?err,
                                "Error invoking trigger handler"
                            );
                        }
                    }
                }

                if has_error {
                    tracing::Span::current().record("otel.status_code", "ERROR");
                } else {
                    tracing::Span::current().record("otel.status_code", "OK");
                }
            }.instrument(tracing::info_span!(parent: current_span, "stream_triggers", otel.status_code = tracing::field::Empty)));
        } else {
            tracing::error!("Failed to convert event data to value");
        }
    }
}

#[service(name = "stream")]
impl StreamCoreModule {
    #[function(id = "stream::set", description = "Set a value in a stream")]
    pub async fn set(&self, input: StreamSetInput) -> FunctionResult<SetResult, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;
        let data = input.data;

        let function_id = format!("stream::set({})", stream_name);
        let function = self.engine.functions.get(&function_id);
        let adapter = self.adapter.clone();

        let result: anyhow::Result<SetResult> = match function {
            Some(_) => {
                tracing::debug!(function_id = %function_id, "Calling custom stream.set function");

                let input = match serde_json::to_value(cloned_input) {
                    Ok(input) => input,
                    Err(e) => {
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to convert input to value: {}", e),
                            code: "JSON_ERROR".to_string(),
                        });
                    }
                };
                let result = self.engine.call(&function_id, input).await;

                match result {
                    Ok(Some(result)) => match serde_json::from_value::<SetResult>(result) {
                        Ok(result) => Ok(result),
                        Err(e) => {
                            return FunctionResult::Failure(ErrorBody {
                                message: format!("Failed to convert result to value: {}", e),
                                code: "JSON_ERROR".to_string(),
                            });
                        }
                    },
                    Ok(None) => Err(anyhow::anyhow!("Function returned no result")),
                    Err(error) => Err(anyhow::anyhow!("Failed to invoke function: {:?}", error)),
                }
            }
            None => {
                adapter
                    .set(&stream_name, &group_id, &item_id, data.clone())
                    .await
            }
        };

        crate::modules::telemetry::collector::track_stream_set();
        match result {
            Ok(result) => {
                let event = if result.old_value.is_some() {
                    StreamOutboundMessage::Update {
                        data: result.new_value.clone(),
                    }
                } else {
                    StreamOutboundMessage::Create {
                        data: result.new_value.clone(),
                    }
                };

                let message = StreamWrapperMessage {
                    event_type: "stream".to_string(),
                    id: Some(item_id.clone()),
                    timestamp: Utc::now().timestamp_millis(),
                    stream_name: stream_name.clone(),
                    group_id: group_id.clone(),
                    event,
                };

                self.invoke_triggers(message.clone()).await;

                if let Err(e) = adapter.emit_event(message).await {
                    tracing::error!(error = %e, "Failed to emit event");
                }

                FunctionResult::Success(result)
            }
            Err(error) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to set value: {}", error),
                code: "STREAM_SET_ERROR".to_string(),
            }),
        }
    }

    #[function(id = "stream::get", description = "Get a value from a stream")]
    pub async fn get(&self, input: StreamGetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;

        let function_id = format!("stream::get({})", stream_name);
        let function = self.engine.functions.get(&function_id);
        let adapter = self.adapter.clone();

        crate::modules::telemetry::collector::track_stream_get();
        match function {
            Some(_) => {
                tracing::debug!(function_id = %function_id, "Calling custom stream.get function");

                let input = match serde_json::to_value(cloned_input) {
                    Ok(input) => input,
                    Err(e) => {
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to convert input to value: {}", e),
                            code: "JSON_ERROR".to_string(),
                        });
                    }
                };

                let result = self.engine.call(&function_id, input).await;

                match result {
                    Ok(result) => FunctionResult::Success(result),
                    Err(error) => FunctionResult::Failure(error),
                }
            }
            None => match adapter.get(&stream_name, &group_id, &item_id).await {
                Ok(value) => FunctionResult::Success(value),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get value from stream");
                    FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to get value: {}", e),
                        code: "STREAM_GET_ERROR".to_string(),
                    })
                }
            },
        }
    }

    #[function(id = "stream::delete", description = "Delete a value from a stream")]
    pub async fn delete(
        &self,
        input: StreamDeleteInput,
    ) -> FunctionResult<DeleteResult, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;
        let function_id = format!("stream::delete({})", stream_name);
        let function = self.engine.functions.get(&function_id);
        let adapter = self.adapter.clone();

        let result: anyhow::Result<DeleteResult> = match function {
            Some(_) => {
                tracing::debug!(function_id = %function_id, "Calling custom stream.delete function");

                let input = match serde_json::to_value(cloned_input) {
                    Ok(input) => input,
                    Err(e) => {
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to convert input to value: {}", e),
                            code: "JSON_ERROR".to_string(),
                        });
                    }
                };
                let result = self.engine.call(&function_id, input).await;
                match result {
                    Ok(Some(result)) => {
                        let result = match serde_json::from_value::<DeleteResult>(result) {
                            Ok(result) => result,
                            Err(e) => {
                                return FunctionResult::Failure(ErrorBody {
                                    message: format!("Failed to convert result to value: {}", e),
                                    code: "JSON_ERROR".to_string(),
                                });
                            }
                        };
                        Ok(result)
                    }
                    Ok(None) => Err(anyhow::anyhow!("Function returned no result")),
                    Err(error) => Err(anyhow::anyhow!("Failed to invoke function: {:?}", error)),
                }
            }
            None => adapter.delete(&stream_name, &group_id, &item_id).await,
        };

        crate::modules::telemetry::collector::track_stream_delete();
        match result {
            Ok(result) => {
                if let Some(old_value) = result.old_value.clone() {
                    let message = StreamWrapperMessage {
                        event_type: "stream".to_string(),
                        id: Some(item_id.clone()),
                        timestamp: Utc::now().timestamp_millis(),
                        stream_name: stream_name.clone(),
                        group_id: group_id.clone(),
                        event: StreamOutboundMessage::Delete { data: old_value },
                    };

                    self.invoke_triggers(message.clone()).await;

                    if let Err(e) = adapter.emit_event(message).await {
                        tracing::error!(error = %e, "Failed to emit delete event");
                    }
                }

                FunctionResult::Success(result)
            }
            Err(error) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to delete value: {}", error),
                code: "STREAM_DELETE_ERROR".to_string(),
            }),
        }
    }

    #[function(id = "stream::list", description = "List all items in a stream group")]
    pub async fn list(&self, input: StreamListInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;

        let function_id = format!("stream::list({})", stream_name);
        let function = self.engine.functions.get(&function_id);
        let adapter = self.adapter.clone();

        crate::modules::telemetry::collector::track_stream_list();
        match function {
            Some(_) => {
                tracing::debug!(function_id = %function_id, "Calling custom stream.getGroup function");

                let input = match serde_json::to_value(cloned_input) {
                    Ok(input) => input,
                    Err(e) => {
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to convert input to value: {}", e),
                            code: "JSON_ERROR".to_string(),
                        });
                    }
                };

                let result = self.engine.call(&function_id, input).await;

                match result {
                    Ok(result) => FunctionResult::Success(result),
                    Err(error) => FunctionResult::Failure(error),
                }
            }
            None => match adapter.get_group(&stream_name, &group_id).await {
                Ok(values) => FunctionResult::Success(serde_json::to_value(values).ok()),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get group from stream");
                    FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to get group: {}", e),
                        code: "STREAM_GET_GROUP_ERROR".to_string(),
                    })
                }
            },
        }
    }

    #[function(
        id = "stream::list_groups",
        description = "List all groups in a stream"
    )]
    pub async fn list_groups(
        &self,
        input: StreamListGroupsInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;

        let function_id = format!("stream::list_groups({})", stream_name);
        let function = self.engine.functions.get(&function_id);
        let adapter = self.adapter.clone();

        match function {
            Some(_) => {
                tracing::debug!(function_id = %function_id, "Calling custom stream.list_groups function");

                let input = match serde_json::to_value(cloned_input) {
                    Ok(input) => input,
                    Err(e) => {
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to convert input to value: {}", e),
                            code: "JSON_ERROR".to_string(),
                        });
                    }
                };
                let result = self.engine.call(&function_id, input).await;

                match result {
                    Ok(result) => FunctionResult::Success(result),
                    Err(error) => FunctionResult::Failure(error),
                }
            }
            None => match adapter.list_groups(&stream_name).await {
                Ok(groups) => FunctionResult::Success(serde_json::to_value(groups).ok()),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to list groups from stream");
                    FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to list groups: {}", e),
                        code: "STREAM_LIST_GROUPS_ERROR".to_string(),
                    })
                }
            },
        }
    }

    #[function(
        id = "stream::list_all",
        description = "List all available stream with metadata"
    )]
    pub async fn list_all(
        &self,
        _input: StreamListAllInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let adapter = self.adapter.clone();

        match adapter.list_all_stream().await {
            Ok(stream) => {
                let stream_json: Vec<Value> = stream
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "id": s.id,
                            "groups": s.groups,
                        })
                    })
                    .collect();

                FunctionResult::Success(Some(serde_json::json!({
                    "stream": stream_json,
                    "count": stream_json.len()
                })))
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to list all stream");
                FunctionResult::Failure(ErrorBody {
                    message: format!("Failed to list stream: {}", e),
                    code: "STREAM_LIST_ALL_ERROR".to_string(),
                })
            }
        }
    }

    #[function(
        id = "stream::send",
        description = "Send a custom event to stream subscribers"
    )]
    pub async fn send(&self, input: StreamSendInput) -> FunctionResult<(), ErrorBody> {
        let message = StreamWrapperMessage {
            event_type: "stream".to_string(),
            timestamp: Utc::now().timestamp_millis(),
            stream_name: input.stream_name.clone(),
            group_id: input.group_id.clone(),
            id: input.id.clone(),
            event: StreamOutboundMessage::Event {
                event: EventData {
                    event_type: input.event_type,
                    data: input.data,
                },
            },
        };

        self.invoke_triggers(message.clone()).await;

        if let Err(e) = self.adapter.emit_event(message).await {
            tracing::error!(error = %e, "Failed to emit stream send event");
            return FunctionResult::Failure(ErrorBody {
                message: format!("Failed to send event: {}", e),
                code: "STREAM_SEND_ERROR".to_string(),
            });
        }

        FunctionResult::Success(())
    }

    #[function(
        id = "stream::update",
        description = "Atomically update a stream value with multiple operations"
    )]
    pub async fn update(
        &self,
        input: StreamUpdateInput,
    ) -> FunctionResult<UpdateResult, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;
        let ops = input.ops;

        tracing::debug!(stream_name = %stream_name, group_id = %group_id, item_id = %item_id, ops_count = ops.len(), "Executing atomic stream update");

        let function_id = format!("stream::update({})", stream_name);
        let function = self.engine.functions.get(&function_id);
        let adapter = self.adapter.clone();

        let result: anyhow::Result<UpdateResult> = match function {
            Some(_) => {
                tracing::debug!(function_id = %function_id, "Calling custom stream.set function");

                let input = match serde_json::to_value(cloned_input) {
                    Ok(input) => input,
                    Err(e) => {
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to convert input to value: {}", e),
                            code: "JSON_ERROR".to_string(),
                        });
                    }
                };
                let result = self.engine.call(&function_id, input).await;

                match result {
                    Ok(Some(result)) => match serde_json::from_value::<UpdateResult>(result) {
                        Ok(result) => Ok(result),
                        Err(e) => {
                            return FunctionResult::Failure(ErrorBody {
                                message: format!("Failed to convert result to value: {}", e),
                                code: "JSON_ERROR".to_string(),
                            });
                        }
                    },
                    Ok(None) => Err(anyhow::anyhow!("Function returned no result")),
                    Err(error) => Err(anyhow::anyhow!("Failed to invoke function: {:?}", error)),
                }
            }
            None => adapter.update(&stream_name, &group_id, &item_id, ops).await,
        };

        crate::modules::telemetry::collector::track_stream_update();
        match result {
            Ok(result) => {
                let event = if result.old_value.is_some() {
                    StreamOutboundMessage::Update {
                        data: result.new_value.clone(),
                    }
                } else {
                    StreamOutboundMessage::Create {
                        data: result.new_value.clone(),
                    }
                };

                let message = StreamWrapperMessage {
                    event_type: "stream".to_string(),
                    id: Some(item_id.clone()),
                    timestamp: Utc::now().timestamp_millis(),
                    stream_name: stream_name.clone(),
                    group_id: group_id.clone(),
                    event,
                };

                self.invoke_triggers(message.clone()).await;

                if let Err(e) = adapter.emit_event(message).await {
                    tracing::error!(error = %e, "Failed to emit event");
                }

                FunctionResult::Success(result)
            }
            Err(error) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to update value: {}", error),
                code: "STREAM_SET_ERROR".to_string(),
            }),
        }
    }
}

crate::register_module!(
    "modules::stream::StreamModule",
    StreamCoreModule,
    enabled_by_default = true
);

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use iii_sdk::{
        UpdateOp, UpdateResult,
        types::{DeleteResult, SetResult},
    };
    use serde_json::Value;
    use tokio::sync::mpsc;

    use crate::{
        builtins::pubsub_lite::Subscriber,
        engine::{Engine, EngineTrait},
        modules::stream::{
            StreamMetadata,
            adapters::{StreamAdapter, StreamConnection},
            config::StreamModuleConfig,
            trigger::StreamTriggerConfig,
        },
        protocol::ErrorBody,
        trigger::Trigger,
    };

    use super::*;

    struct RecordingConnection {
        tx: mpsc::UnboundedSender<StreamWrapperMessage>,
    }

    #[async_trait]
    impl StreamConnection for RecordingConnection {
        async fn cleanup(&self) {}

        async fn handle_stream_message(&self, msg: &StreamWrapperMessage) -> anyhow::Result<()> {
            let _ = self.tx.send(msg.clone());
            Ok(())
        }
    }

    #[async_trait]
    impl Subscriber for RecordingConnection {
        async fn handle_message(&self, message: Arc<Value>) -> anyhow::Result<()> {
            let msg = match serde_json::from_value::<StreamWrapperMessage>((*message).clone()) {
                Ok(msg) => msg,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to deserialize stream message");
                    return Err(anyhow::anyhow!("Failed to deserialize stream message"));
                }
            };
            let _ = self.tx.send(msg);
            Ok(())
        }
    }

    fn create_test_module() -> StreamCoreModule {
        crate::modules::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let config = StreamModuleConfig {
            port: 0, // Use 0 for testing (OS will assign port)
            host: "127.0.0.1".to_string(),
            auth_function: None,
            adapter: Some(crate::modules::module::AdapterEntry {
                class: "modules::stream::adapters::KvStore".to_string(),
                config: None,
            }),
        };

        // Create adapter directly using kv_store adapter
        let adapter: Arc<dyn StreamAdapter> =
            Arc::new(crate::modules::stream::adapters::kv_store::BuiltinKvStoreAdapter::new(None));

        StreamCoreModule::build(engine, config, adapter)
    }

    struct FakeStreamAdapter {
        set_result: Mutex<Result<SetResult, String>>,
        get_result: Mutex<Result<Option<Value>, String>>,
        delete_result: Mutex<Result<DeleteResult, String>>,
        get_group_result: Mutex<Result<Vec<Value>, String>>,
        list_groups_result: Mutex<Result<Vec<String>, String>>,
        list_all_result: Mutex<Result<Vec<StreamMetadata>, String>>,
        emit_event_result: Mutex<Result<(), String>>,
        update_result: Mutex<Result<UpdateResult, String>>,
        emitted_messages: Mutex<Vec<StreamWrapperMessage>>,
        destroy_called: AtomicBool,
        watch_events_called: AtomicBool,
    }

    impl Default for FakeStreamAdapter {
        fn default() -> Self {
            Self {
                set_result: Mutex::new(Ok(SetResult {
                    old_value: None,
                    new_value: serde_json::json!({}),
                })),
                get_result: Mutex::new(Ok(None)),
                delete_result: Mutex::new(Ok(DeleteResult { old_value: None })),
                get_group_result: Mutex::new(Ok(Vec::new())),
                list_groups_result: Mutex::new(Ok(Vec::new())),
                list_all_result: Mutex::new(Ok(Vec::new())),
                emit_event_result: Mutex::new(Ok(())),
                update_result: Mutex::new(Ok(UpdateResult {
                    old_value: None,
                    new_value: serde_json::json!({}),
                })),
                emitted_messages: Mutex::new(Vec::new()),
                destroy_called: AtomicBool::new(false),
                watch_events_called: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl StreamAdapter for FakeStreamAdapter {
        async fn set(
            &self,
            _stream_name: &str,
            _group_id: &str,
            _item_id: &str,
            _data: Value,
        ) -> anyhow::Result<SetResult> {
            self.set_result
                .lock()
                .expect("lock set_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn get(
            &self,
            _stream_name: &str,
            _group_id: &str,
            _item_id: &str,
        ) -> anyhow::Result<Option<Value>> {
            self.get_result
                .lock()
                .expect("lock get_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn delete(
            &self,
            _stream_name: &str,
            _group_id: &str,
            _item_id: &str,
        ) -> anyhow::Result<DeleteResult> {
            self.delete_result
                .lock()
                .expect("lock delete_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn get_group(
            &self,
            _stream_name: &str,
            _group_id: &str,
        ) -> anyhow::Result<Vec<Value>> {
            self.get_group_result
                .lock()
                .expect("lock get_group_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn list_groups(&self, _stream_name: &str) -> anyhow::Result<Vec<String>> {
            self.list_groups_result
                .lock()
                .expect("lock list_groups_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn list_all_stream(&self) -> anyhow::Result<Vec<StreamMetadata>> {
            self.list_all_result
                .lock()
                .expect("lock list_all_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn emit_event(&self, message: StreamWrapperMessage) -> anyhow::Result<()> {
            self.emitted_messages
                .lock()
                .expect("lock emitted_messages")
                .push(message);
            self.emit_event_result
                .lock()
                .expect("lock emit_event_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }

        async fn subscribe(
            &self,
            _id: String,
            _connection: Arc<dyn StreamConnection>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn unsubscribe(&self, _id: String) -> anyhow::Result<()> {
            Ok(())
        }

        async fn watch_events(&self) -> anyhow::Result<()> {
            self.watch_events_called.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn destroy(&self) -> anyhow::Result<()> {
            self.destroy_called.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn update(
            &self,
            _stream_name: &str,
            _group_id: &str,
            _item_id: &str,
            _ops: Vec<UpdateOp>,
        ) -> anyhow::Result<UpdateResult> {
            self.update_result
                .lock()
                .expect("lock update_result")
                .clone()
                .map_err(anyhow::Error::msg)
        }
    }

    fn create_module_with_adapter(adapter: Arc<dyn StreamAdapter>) -> StreamCoreModule {
        crate::modules::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let config = StreamModuleConfig {
            port: 0,
            host: "127.0.0.1".to_string(),
            auth_function: None,
            adapter: Some(crate::modules::module::AdapterEntry {
                class: "test-adapter".to_string(),
                config: None,
            }),
        };

        StreamCoreModule::build(engine, config, adapter)
    }

    #[tokio::test]
    async fn test_stream_module_set_get_delete() {
        let module = create_test_module();
        let stream_name = "test_stream";
        let group_id = "test_group";
        let item_id = "item1";
        let data1 = serde_json::json!({"key": "value1"});
        let data2 = serde_json::json!({"key": "value2"});

        // Subscribe to events
        let (tx, mut rx) = mpsc::unbounded_channel();
        module
            .adapter
            .subscribe(
                "test-subscriber".to_string(),
                Arc::new(RecordingConnection { tx }),
            )
            .await
            .expect("Should subscribe successfully");

        // Start event watcher
        let watcher_adapter = module.adapter.clone();
        let watcher = tokio::spawn(async move {
            let _ = watcher_adapter.watch_events().await;
        });
        tokio::task::yield_now().await;

        // Test set (create)
        let set_result = module
            .set(StreamSetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                data: data1.clone(),
            })
            .await;

        assert!(matches!(set_result, FunctionResult::Success(_)));

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for create event")
            .expect("Should receive create event");

        assert!(matches!(msg.event, StreamOutboundMessage::Create { .. }));

        // Test get
        let get_result = module
            .get(StreamGetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
            })
            .await;

        match get_result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, data1);
            }
            _ => panic!("Expected successful get with value"),
        }

        // Test set (update)
        let set_result = module
            .set(StreamSetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                data: data2.clone(),
            })
            .await;

        assert!(matches!(set_result, FunctionResult::Success(_)));

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for update event")
            .expect("Should receive update event");

        assert!(matches!(msg.event, StreamOutboundMessage::Update { .. }));

        // Verify updated value
        let get_result = module
            .get(StreamGetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
            })
            .await;

        match get_result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, data2);
            }
            _ => panic!("Expected successful get with updated value"),
        }

        // Test delete
        let delete_result = module
            .delete(StreamDeleteInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
            })
            .await;

        assert!(matches!(delete_result, FunctionResult::Success(_)));

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for delete event")
            .expect("Should receive delete event");

        assert!(matches!(msg.event, StreamOutboundMessage::Delete { .. }));

        // Verify deleted
        let get_result = module
            .get(StreamGetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
            })
            .await;

        match get_result {
            FunctionResult::Success(None) => {
                // Expected - item was deleted
            }
            _ => panic!("Expected None after delete"),
        }

        watcher.abort();
    }

    #[tokio::test]
    async fn test_stream_module_update_existing_record() {
        let module = create_test_module();
        let stream_name = "test_stream";
        let group_id = "test_group";
        let item_id = "item1";
        let initial_data = serde_json::json!({"key": "value1", "count": 5});
        let updated_data = serde_json::json!({"key": "value2", "count": 10});

        // Subscribe to events
        let (tx, mut rx) = mpsc::unbounded_channel();
        module
            .adapter
            .subscribe(
                "test-subscriber".to_string(),
                Arc::new(RecordingConnection { tx }),
            )
            .await
            .expect("Should subscribe successfully");

        // Start event watcher
        let watcher_adapter = module.adapter.clone();
        let watcher = tokio::spawn(async move {
            let _ = watcher_adapter.watch_events().await;
        });
        tokio::task::yield_now().await;

        // Create initial record using set
        module
            .set(StreamSetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                data: initial_data.clone(),
            })
            .await;

        // Consume the create event
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for create event");

        // Update existing record
        let update_result = module
            .update(StreamUpdateInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                ops: vec![iii_sdk::UpdateOp::set("", updated_data.clone())],
            })
            .await;

        assert!(matches!(update_result, FunctionResult::Success(_)));

        // Verify Update event was emitted
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for update event")
            .expect("Should receive update event");

        assert!(matches!(msg.event, StreamOutboundMessage::Update { .. }));

        // Verify the value was updated
        let get_result = module
            .get(StreamGetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
            })
            .await;

        match get_result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, updated_data);
            }
            _ => panic!("Expected successful get with updated value"),
        }

        watcher.abort();
    }

    #[tokio::test]
    async fn test_stream_module_update_new_record() {
        let module = create_test_module();
        let stream_name = "test_stream";
        let group_id = "test_group";
        let item_id = "new_item";
        let new_data = serde_json::json!({"key": "new_value", "count": 1});

        // Subscribe to events
        let (tx, mut rx) = mpsc::unbounded_channel();
        module
            .adapter
            .subscribe(
                "test-subscriber".to_string(),
                Arc::new(RecordingConnection { tx }),
            )
            .await
            .expect("Should subscribe successfully");

        // Start event watcher
        let watcher_adapter = module.adapter.clone();
        let watcher = tokio::spawn(async move {
            let _ = watcher_adapter.watch_events().await;
        });
        tokio::task::yield_now().await;

        // Update non-existent record (should create it)
        let update_result = module
            .update(StreamUpdateInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
                ops: vec![iii_sdk::UpdateOp::set("", new_data.clone())],
            })
            .await;

        assert!(matches!(update_result, FunctionResult::Success(_)));

        // Verify Create event was emitted (not Update)
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for create event")
            .expect("Should receive create event");

        assert!(matches!(msg.event, StreamOutboundMessage::Create { .. }));

        // Verify the value was created
        let get_result = module
            .get(StreamGetInput {
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                item_id: item_id.to_string(),
            })
            .await;

        match get_result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, new_data);
            }
            _ => panic!("Expected successful get with new value"),
        }

        watcher.abort();
    }

    #[tokio::test]
    async fn test_stream_list_groups_list_all_and_send_with_adapter() {
        let adapter = Arc::new(FakeStreamAdapter::default());
        *adapter
            .get_group_result
            .lock()
            .expect("lock get_group_result") = Ok(vec![
            serde_json::json!({ "item": 1 }),
            serde_json::json!({ "item": 2 }),
        ]);
        *adapter
            .list_groups_result
            .lock()
            .expect("lock list_groups_result") =
            Ok(vec!["group-a".to_string(), "group-b".to_string()]);
        *adapter
            .list_all_result
            .lock()
            .expect("lock list_all_result") = Ok(vec![
            StreamMetadata {
                id: "stream-a".to_string(),
                groups: vec!["group-a".to_string()],
            },
            StreamMetadata {
                id: "stream-b".to_string(),
                groups: vec!["group-b".to_string(), "group-c".to_string()],
            },
        ]);

        let module = create_module_with_adapter(adapter.clone());

        match module
            .list(StreamListInput {
                stream_name: "stream-a".to_string(),
                group_id: "group-a".to_string(),
            })
            .await
        {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, serde_json::json!([{ "item": 1 }, { "item": 2 }]));
            }
            _ => panic!("expected list success"),
        }

        match module
            .list_groups(StreamListGroupsInput {
                stream_name: "stream-a".to_string(),
            })
            .await
        {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, serde_json::json!(["group-a", "group-b"]));
            }
            _ => panic!("expected list_groups success"),
        }

        match module.list_all(StreamListAllInput {}).await {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value["count"], 2);
                assert_eq!(value["stream"][0]["id"], "stream-a");
                assert_eq!(
                    value["stream"][1]["groups"],
                    serde_json::json!(["group-b", "group-c"])
                );
            }
            _ => panic!("expected list_all success"),
        }

        let send_result = module
            .send(StreamSendInput {
                stream_name: "stream-a".to_string(),
                group_id: "group-a".to_string(),
                id: Some("item-1".to_string()),
                event_type: "custom".to_string(),
                data: serde_json::json!({ "message": "hello" }),
            })
            .await;
        assert!(matches!(send_result, FunctionResult::Success(())));

        let messages = adapter
            .emitted_messages
            .lock()
            .expect("lock emitted_messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].stream_name, "stream-a");
        assert_eq!(messages[0].group_id, "group-a");
        assert_eq!(messages[0].id.as_deref(), Some("item-1"));
        assert!(matches!(
            messages[0].event,
            StreamOutboundMessage::Event {
                event: EventData {
                    ref event_type,
                    ..
                }
            } if event_type == "custom"
        ));
    }

    #[tokio::test]
    async fn test_stream_methods_return_adapter_failures() {
        let adapter = Arc::new(FakeStreamAdapter::default());
        *adapter.set_result.lock().expect("lock set_result") = Err("set failed".to_string());
        *adapter.get_result.lock().expect("lock get_result") = Err("get failed".to_string());
        *adapter.delete_result.lock().expect("lock delete_result") =
            Err("delete failed".to_string());
        *adapter
            .get_group_result
            .lock()
            .expect("lock get_group_result") = Err("group failed".to_string());
        *adapter
            .list_groups_result
            .lock()
            .expect("lock list_groups_result") = Err("list groups failed".to_string());
        *adapter
            .list_all_result
            .lock()
            .expect("lock list_all_result") = Err("list all failed".to_string());
        *adapter
            .emit_event_result
            .lock()
            .expect("lock emit_event_result") = Err("emit failed".to_string());
        *adapter.update_result.lock().expect("lock update_result") =
            Err("update failed".to_string());

        let module = create_module_with_adapter(adapter);

        assert!(matches!(
            module
                .set(StreamSetInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    data: serde_json::json!({ "k": "v" }),
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_SET_ERROR"
        ));
        assert!(matches!(
            module
                .get(StreamGetInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_GET_ERROR"
        ));
        assert!(matches!(
            module
                .delete(StreamDeleteInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_DELETE_ERROR"
        ));
        assert!(matches!(
            module
                .list(StreamListInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_GET_GROUP_ERROR"
        ));
        assert!(matches!(
            module
                .list_groups(StreamListGroupsInput {
                    stream_name: "stream".to_string(),
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_LIST_GROUPS_ERROR"
        ));
        assert!(matches!(
            module.list_all(StreamListAllInput {}).await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_LIST_ALL_ERROR"
        ));
        assert!(matches!(
            module
                .send(StreamSendInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                    id: None,
                    event_type: "boom".to_string(),
                    data: serde_json::json!({}),
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_SEND_ERROR"
        ));
        assert!(matches!(
            module
                .update(StreamUpdateInput {
                    stream_name: "stream".to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    ops: vec![UpdateOp::set("", serde_json::json!({ "count": 1 }))],
                })
                .await,
            FunctionResult::Failure(ErrorBody { code, .. }) if code == "STREAM_SET_ERROR"
        ));
    }

    #[tokio::test]
    async fn test_stream_custom_functions_override_adapter() {
        let module = create_test_module();
        let stream_name = "custom_stream";

        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::get({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!({ "from": "custom-get" })))
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::list({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!([{ "from": "custom-list" }])))
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::list_groups({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!(["alpha", "beta"])))
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::set({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!({
                    "old_value": null,
                    "new_value": { "from": "custom-set" }
                })))
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::delete({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!({
                    "old_value": { "from": "custom-delete" }
                })))
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: format!("stream::update({stream_name})"),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!({
                    "old_value": { "count": 1 },
                    "new_value": { "count": 2 }
                })))
            }),
        );

        assert!(matches!(
            module
                .get(StreamGetInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                })
                .await,
            FunctionResult::Success(Some(value)) if value == serde_json::json!({ "from": "custom-get" })
        ));
        assert!(matches!(
            module
                .list(StreamListInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                })
                .await,
            FunctionResult::Success(Some(value)) if value == serde_json::json!([{ "from": "custom-list" }])
        ));
        assert!(matches!(
            module
                .list_groups(StreamListGroupsInput {
                    stream_name: stream_name.to_string(),
                })
                .await,
            FunctionResult::Success(Some(value)) if value == serde_json::json!(["alpha", "beta"])
        ));
        assert!(matches!(
            module
                .set(StreamSetInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    data: serde_json::json!({ "ignored": true }),
                })
                .await,
            FunctionResult::Success(SetResult { new_value, .. }) if new_value == serde_json::json!({ "from": "custom-set" })
        ));
        assert!(matches!(
            module
                .delete(StreamDeleteInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                })
                .await,
            FunctionResult::Success(DeleteResult { old_value: Some(value) }) if value == serde_json::json!({ "from": "custom-delete" })
        ));
        assert!(matches!(
            module
                .update(StreamUpdateInput {
                    stream_name: stream_name.to_string(),
                    group_id: "group".to_string(),
                    item_id: "item".to_string(),
                    ops: vec![UpdateOp::set("", serde_json::json!({ "count": 2 }))],
                })
                .await,
            FunctionResult::Success(UpdateResult { new_value, .. }) if new_value == serde_json::json!({ "count": 2 })
        ));
    }

    #[tokio::test]
    async fn test_stream_invoke_triggers_covers_conditions_and_errors() {
        let module = create_test_module();
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let handler_calls_clone = handler_calls.clone();

        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::stream_handler".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |_input| {
                let handler_calls_clone = handler_calls_clone.clone();
                async move {
                    handler_calls_clone.fetch_add(1, Ordering::SeqCst);
                    FunctionResult::Success(None)
                }
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::stream_handler_fail".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Failure(ErrorBody {
                    code: "HANDLER".to_string(),
                    message: "handler failed".to_string(),
                })
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::cond_false".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Success(Some(serde_json::json!(false)))
            }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::cond_none".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move { FunctionResult::Success(None) }),
        );
        module.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::cond_error".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|_input| async move {
                FunctionResult::Failure(ErrorBody {
                    code: "COND".to_string(),
                    message: "condition failed".to_string(),
                })
            }),
        );

        let mut stream_triggers = module.triggers.stream_triggers.write().await;
        stream_triggers.insert(
            "ok".to_string(),
            StreamTrigger {
                trigger: Trigger {
                    id: "ok".to_string(),
                    trigger_type: STREAM_TRIGGER_TYPE.to_string(),
                    function_id: "test::stream_handler".to_string(),
                    config: serde_json::json!({}),
                    worker_id: None,
                },
                config: StreamTriggerConfig {
                    stream_name: Some("events".to_string()),
                    group_id: None,
                    item_id: None,
                    condition_function_id: None,
                },
            },
        );
        stream_triggers.insert(
            "skip-false".to_string(),
            StreamTrigger {
                trigger: Trigger {
                    id: "skip-false".to_string(),
                    trigger_type: STREAM_TRIGGER_TYPE.to_string(),
                    function_id: "test::stream_handler".to_string(),
                    config: serde_json::json!({}),
                    worker_id: None,
                },
                config: StreamTriggerConfig {
                    stream_name: Some("events".to_string()),
                    group_id: None,
                    item_id: None,
                    condition_function_id: Some("test::cond_false".to_string()),
                },
            },
        );
        stream_triggers.insert(
            "skip-none".to_string(),
            StreamTrigger {
                trigger: Trigger {
                    id: "skip-none".to_string(),
                    trigger_type: STREAM_TRIGGER_TYPE.to_string(),
                    function_id: "test::stream_handler".to_string(),
                    config: serde_json::json!({}),
                    worker_id: None,
                },
                config: StreamTriggerConfig {
                    stream_name: Some("events".to_string()),
                    group_id: None,
                    item_id: None,
                    condition_function_id: Some("test::cond_none".to_string()),
                },
            },
        );
        stream_triggers.insert(
            "skip-error".to_string(),
            StreamTrigger {
                trigger: Trigger {
                    id: "skip-error".to_string(),
                    trigger_type: STREAM_TRIGGER_TYPE.to_string(),
                    function_id: "test::stream_handler".to_string(),
                    config: serde_json::json!({}),
                    worker_id: None,
                },
                config: StreamTriggerConfig {
                    stream_name: Some("events".to_string()),
                    group_id: None,
                    item_id: None,
                    condition_function_id: Some("test::cond_error".to_string()),
                },
            },
        );
        stream_triggers.insert(
            "handler-error".to_string(),
            StreamTrigger {
                trigger: Trigger {
                    id: "handler-error".to_string(),
                    trigger_type: STREAM_TRIGGER_TYPE.to_string(),
                    function_id: "test::stream_handler_fail".to_string(),
                    config: serde_json::json!({}),
                    worker_id: None,
                },
                config: StreamTriggerConfig {
                    stream_name: Some("events".to_string()),
                    group_id: None,
                    item_id: None,
                    condition_function_id: None,
                },
            },
        );
        drop(stream_triggers);

        module
            .triggers
            .stream_triggers_by_name
            .write()
            .await
            .insert(
                "events".to_string(),
                vec![
                    "ok".to_string(),
                    "skip-false".to_string(),
                    "skip-none".to_string(),
                    "skip-error".to_string(),
                    "handler-error".to_string(),
                ],
            );

        module
            .invoke_triggers(StreamWrapperMessage {
                event_type: "stream".to_string(),
                timestamp: Utc::now().timestamp_millis(),
                stream_name: "events".to_string(),
                group_id: "group".to_string(),
                id: Some("item-1".to_string()),
                event: StreamOutboundMessage::Create {
                    data: serde_json::json!({ "x": 1 }),
                },
            })
            .await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(handler_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_stream_initialize_registers_trigger_types_and_destroy_calls_adapter() {
        let adapter = Arc::new(FakeStreamAdapter::default());
        let module = create_module_with_adapter(adapter.clone());

        module
            .initialize()
            .await
            .expect("stream initialize should work");
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(
            module
                .engine
                .trigger_registry
                .trigger_types
                .contains_key(JOIN_TRIGGER_TYPE)
        );
        assert!(
            module
                .engine
                .trigger_registry
                .trigger_types
                .contains_key(LEAVE_TRIGGER_TYPE)
        );
        assert!(
            module
                .engine
                .trigger_registry
                .trigger_types
                .contains_key(STREAM_TRIGGER_TYPE)
        );
        assert!(adapter.watch_events_called.load(Ordering::SeqCst));

        module.destroy().await.expect("stream destroy should work");
        assert!(adapter.destroy_called.load(Ordering::SeqCst));
    }
}
