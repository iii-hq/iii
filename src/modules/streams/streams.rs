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
use once_cell::sync::Lazy;
use serde_json::Value;
use tokio::net::TcpListener;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::{
        module::{AdapterFactory, ConfigurableModule, Module},
        streams::{
            StreamOutboundMessage, StreamSocketManager, StreamWrapperMessage,
            adapters::StreamAdapter,
            config::StreamModuleConfig,
            structs::{
                StreamAuthContext, StreamAuthInput, StreamDeleteInput, StreamEventData,
                StreamEventType, StreamGetGroupInput, StreamGetInput, StreamListGroupsInput,
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
    adapter: Arc<dyn StreamAdapter>,
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
            Ok(input) => match engine.invoke_function(&auth_function, input).await {
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
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::streams::adapters::RedisAdapter";

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
    async fn invoke_triggers(&self, event_data: StreamEventData) {
        let engine = self.engine.clone();
        let event_type = event_data.event_type.clone();
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
                        triggers.push(trigger.clone());
                    }
                }
            }

            triggers
        };

        if let Ok(event_data) = serde_json::to_value(event_data) {
            tokio::spawn(async move {
                tracing::debug!("Invoking triggers for event type {:?}", event_type);

                for stream_trigger in triggers_to_invoke {
                    let trigger = &stream_trigger.trigger;

                    // Check condition if specified (using pre-parsed value)
                    let condition_function_path = stream_trigger.condition_function_path.clone();

                    if let Some(condition_function_path) = condition_function_path {
                        tracing::debug!(
                            condition_function_path = %condition_function_path,
                            "Checking trigger conditions"
                        );

                        match engine
                            .invoke_function(&condition_function_path, event_data.clone())
                            .await
                        {
                            Ok(Some(result)) => {
                                tracing::debug!(
                                    condition_function_path = %condition_function_path,
                                    result = ?result,
                                    "Condition function result"
                                );

                                if let Some(passed) = result.as_bool()
                                    && !passed
                                {
                                    tracing::debug!(
                                        function_path = %trigger.function_path,
                                        "Condition check failed, skipping handler"
                                    );
                                    continue;
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    condition_function_path = %condition_function_path,
                                    "Condition function returned no result"
                                );
                                continue;
                            }
                            Err(err) => {
                                tracing::error!(
                                    condition_function_path = %condition_function_path,
                                    error = ?err,
                                    "Error invoking condition function"
                                );
                                continue;
                            }
                        }
                    }

                    // Invoke the handler function
                    tracing::debug!(
                        function_path = %trigger.function_path,
                        "Invoking trigger"
                    );

                    let call_result = engine
                        .invoke_function(&trigger.function_path, event_data.clone())
                        .await;

                    match call_result {
                        Ok(_) => {
                            tracing::debug!(
                                function_path = %trigger.function_path,
                                "Trigger handler invoked successfully"
                            );
                        }
                        Err(err) => {
                            tracing::error!(
                                function_path = %trigger.function_path,
                                error = ?err,
                                "Error invoking trigger handler"
                            );
                        }
                    }
                }
            });
        } else {
            tracing::error!("Failed to convert event data to value");
        }
    }
}

#[service(name = "streams")]
impl StreamCoreModule {
    #[function(name = "streams.set", description = "Set a value in a stream")]
    pub async fn set(&self, input: StreamSetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;
        let data = input.data;
        let data_clone = data.clone();

        // Get old value to determine if this is a create or update
        let old_value = self
            .adapter
            .get(&stream_name, &group_id, &item_id)
            .await
            .ok()
            .flatten();

        let function_path = format!("streams.set({})", stream_name);
        let function = self.engine.functions.get(&function_path);
        let adapter = self.adapter.clone();

        match function {
            Some(_) => {
                tracing::debug!(function_path = %function_path, "Calling custom streams.set function");

                let input = serde_json::to_value(cloned_input);

                if let Err(e) = input {
                    return FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to convert input to value: {}", e),
                        code: "JSON_ERROR".to_string(),
                    });
                }

                let input = input.unwrap();
                let result = self.engine.invoke_function(&function_path, input).await;

                match result {
                    Ok(result) => {
                        let existed = result
                            .as_ref()
                            .and_then(|v| v.get("existed"))
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        let event = if existed {
                            StreamOutboundMessage::Update { data }
                        } else {
                            StreamOutboundMessage::Create { data }
                        };

                        if let Err(e) = adapter
                            .emit_event(StreamWrapperMessage {
                                id: Some(item_id.clone()),
                                timestamp: Utc::now().timestamp_millis(),
                                stream_name: stream_name.clone(),
                                group_id: group_id.clone(),
                                event,
                            })
                            .await
                        {
                            tracing::error!(error = %e, "Failed to emit event");
                        }

                        // Invoke triggers after successful set
                        let is_create = old_value.is_none();
                        let event_data = StreamEventData {
                            message_type: "stream".to_string(),
                            event_type: if is_create {
                                StreamEventType::Created
                            } else {
                                StreamEventType::Updated
                            },
                            stream_name: stream_name.clone(),
                            group_id: group_id.clone(),
                            item_id: item_id.clone(),
                            old_value,
                            new_value: data_clone.clone(),
                        };

                        self.invoke_triggers(event_data).await;
                    }
                    Err(error) => {
                        return FunctionResult::Failure(error);
                    }
                }
            }
            None => {
                match adapter
                    .set(&stream_name, &group_id, &item_id, data.clone())
                    .await
                {
                    Ok(_) => {
                        // Invoke triggers after successful set
                        let is_create = old_value.is_none();
                        let event_data = StreamEventData {
                            message_type: "stream".to_string(),
                            event_type: if is_create {
                                StreamEventType::Created
                            } else {
                                StreamEventType::Updated
                            },
                            stream_name: stream_name.clone(),
                            group_id: group_id.clone(),
                            item_id: item_id.clone(),
                            old_value,
                            new_value: data_clone.clone(),
                        };

                        self.invoke_triggers(event_data).await;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to set value in stream");
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to set value: {}", e),
                            code: "STREAM_SET_ERROR".to_string(),
                        });
                    }
                }
            }
        }

        FunctionResult::Success(Some(data_clone))
    }

    #[function(name = "streams.get", description = "Get a value from a stream")]
    pub async fn get(&self, input: StreamGetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;

        let function_path = format!("streams.get({})", stream_name);
        let function = self.engine.functions.get(&function_path);
        let adapter = self.adapter.clone();

        match function {
            Some(_) => {
                tracing::debug!(function_path = %function_path, "Calling custom streams.get function");

                let input = serde_json::to_value(cloned_input);

                if let Err(e) = input {
                    return FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to convert input to value: {}", e),
                        code: "JSON_ERROR".to_string(),
                    });
                }

                let input = input.unwrap();
                let result = self.engine.invoke_function(&function_path, input).await;

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

    #[function(name = "streams.delete", description = "Delete a value from a stream")]
    pub async fn delete(
        &self,
        input: StreamDeleteInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;

        // Get old value before delete
        let old_value = match self.adapter.get(&stream_name, &group_id, &item_id).await {
            Ok(v) => v,
            Err(e) => {
                return FunctionResult::Failure(ErrorBody {
                    message: format!("Failed to get value before delete: {}", e),
                    code: "GET_ERROR".to_string(),
                });
            }
        };

        let data = self
            .get(StreamGetInput {
                stream_name: stream_name.clone(),
                group_id: group_id.clone(),
                item_id: item_id.clone(),
            })
            .await;

        let function_path = format!("streams.delete({})", stream_name);
        let function = self.engine.functions.get(&function_path);
        let adapter = self.adapter.clone();

        match function {
            Some(_) => {
                tracing::debug!(function_path = %function_path, "Calling custom streams.delete function");

                let input = serde_json::to_value(cloned_input);

                if let Err(e) = input {
                    return FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to convert input to value: {}", e),
                        code: "JSON_ERROR".to_string(),
                    });
                }

                let input = input.unwrap();
                let result = self.engine.invoke_function(&function_path, input).await;

                match result {
                    Ok(_) => {
                        if let Err(e) = adapter
                            .emit_event(StreamWrapperMessage {
                                id: Some(item_id.clone()),
                                timestamp: Utc::now().timestamp_millis(),
                                stream_name: stream_name.clone(),
                                group_id: group_id.clone(),
                                event: StreamOutboundMessage::Delete {
                                    data: serde_json::json!({ "id": item_id }),
                                },
                            })
                            .await
                        {
                            tracing::error!(error = %e, "Failed to emit delete event");
                        }

                        // Invoke triggers after successful delete
                        let event_data = StreamEventData {
                            message_type: "stream".to_string(),
                            event_type: StreamEventType::Deleted,
                            stream_name: stream_name.clone(),
                            group_id: group_id.clone(),
                            item_id: item_id.clone(),
                            old_value: old_value.clone(),
                            new_value: Value::Null,
                        };

                        self.invoke_triggers(event_data).await;
                    }
                    Err(error) => {
                        return FunctionResult::Failure(error);
                    }
                }
            }
            None => {
                match adapter.delete(&stream_name, &group_id, &item_id).await {
                    Ok(_) => {
                        // Invoke triggers after successful delete
                        let event_data = StreamEventData {
                            message_type: "stream".to_string(),
                            event_type: StreamEventType::Deleted,
                            stream_name: stream_name.clone(),
                            group_id: group_id.clone(),
                            item_id: item_id.clone(),
                            old_value: old_value.clone(),
                            new_value: Value::Null,
                        };

                        self.invoke_triggers(event_data).await;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to delete value from stream");
                        return FunctionResult::Failure(ErrorBody {
                            message: format!("Failed to delete value: {}", e),
                            code: "STREAM_DELETE_ERROR".to_string(),
                        });
                    }
                }
            }
        }

        data
    }

    #[function(
        name = "streams.list",
        description = "List all items in a stream group"
    )]
    pub async fn list(
        &self,
        input: StreamGetGroupInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        self.get_group(input).await
    }

    #[function(name = "streams.getGroup", description = "Get a group from a stream")]
    pub async fn get_group(
        &self,
        input: StreamGetGroupInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;
        let group_id = input.group_id;

        let function_path = format!("streams.getGroup({})", stream_name);
        let function = self.engine.functions.get(&function_path);
        let adapter = self.adapter.clone();

        match function {
            Some(_) => {
                tracing::debug!(function_path = %function_path, "Calling custom streams.getGroup function");

                let input = serde_json::to_value(cloned_input);

                if let Err(e) = input {
                    return FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to convert input to value: {}", e),
                        code: "JSON_ERROR".to_string(),
                    });
                }

                let input = input.unwrap();
                let result = self.engine.invoke_function(&function_path, input).await;

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
        name = "streams.listGroups",
        description = "List all groups in a stream"
    )]
    pub async fn list_groups(
        &self,
        input: StreamListGroupsInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let cloned_input = input.clone();
        let stream_name = input.stream_name;

        let function_path = format!("streams.listGroups({})", stream_name);
        let function = self.engine.functions.get(&function_path);
        let adapter = self.adapter.clone();

        match function {
            Some(_) => {
                tracing::debug!(function_path = %function_path, "Calling custom streams.listGroups function");

                let input = serde_json::to_value(cloned_input);

                if let Err(e) = input {
                    return FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to convert input to value: {}", e),
                        code: "JSON_ERROR".to_string(),
                    });
                }

                let input = input.unwrap();
                let result = self.engine.invoke_function(&function_path, input).await;

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
        name = "streams.update",
        description = "Atomically update a stream value with multiple operations"
    )]
    pub async fn update(
        &self,
        input: StreamUpdateInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let stream_name = input.stream_name;
        let group_id = input.group_id;
        let item_id = input.item_id;
        let ops = input.ops;
        let adapter = self.adapter.clone();

        tracing::debug!(stream_name = %stream_name, group_id = %group_id, item_id = %item_id, ops_count = ops.len(), "Executing atomic stream update");

        let result = match adapter.update(&stream_name, &group_id, &item_id, ops).await {
            Ok(result) => result,
            Err(e) => {
                tracing::error!(error = %e, "Failed to update value in stream");
                return FunctionResult::Failure(ErrorBody {
                    message: format!("Failed to update value: {}", e),
                    code: "STREAM_UPDATE_ERROR".to_string(),
                });
            }
        };

        if let Err(e) = adapter
            .emit_event(StreamWrapperMessage {
                id: Some(item_id.clone()),
                timestamp: Utc::now().timestamp_millis(),
                stream_name: stream_name.clone(),
                group_id: group_id.clone(),
                event: StreamOutboundMessage::Update {
                    data: result.new_value.clone(),
                },
            })
            .await
        {
            tracing::error!(error = %e, "Failed to emit update event");
        }

        // Invoke triggers after successful update
        let old_value = result.old_value.clone();
        let new_value = result.new_value.clone();
        let is_create = old_value.is_none();
        let event_data = StreamEventData {
            message_type: "stream".to_string(),
            event_type: if is_create {
                StreamEventType::Created
            } else {
                StreamEventType::Updated
            },
            stream_name: stream_name.clone(),
            group_id: group_id.clone(),
            item_id: item_id.clone(),
            old_value,
            new_value,
        };

        self.invoke_triggers(event_data).await;

        FunctionResult::Success(serde_json::to_value(result).ok())
    }
}

crate::register_module!(
    "modules::streams::StreamModule",
    StreamCoreModule,
    enabled_by_default = true
);
