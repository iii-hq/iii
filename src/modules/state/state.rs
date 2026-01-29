use std::{
    collections::HashMap,
    sync::{Arc, RwLock as SyncRwLock},
};

use function_macros::{function, service};
use once_cell::sync::Lazy;
use serde_json::Value;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::{
        module::{AdapterFactory, ConfigurableModule, Module},
        state::{
            adapters::StateAdapter,
            config::StateModuleConfig,
            structs::{
                StateDeleteInput, StateEventData, StateEventType, StateGetGroupInput,
                StateGetInput, StateSetInput, StateUpdateInput,
            },
            trigger::{StateTriggers, TRIGGER_TYPE},
        },
    },
    protocol::ErrorBody,
    trigger::{Trigger, TriggerType},
};

#[derive(Clone)]
pub struct StateCoreModule {
    adapter: Arc<dyn StateAdapter>,
    engine: Arc<Engine>,
    pub triggers: Arc<StateTriggers>,
}

#[async_trait::async_trait]
impl Module for StateCoreModule {
    fn name(&self) -> &'static str {
        "StateCoreModule"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying StateCoreModule");
        self.adapter.destroy().await?;
        Ok(())
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing StateCoreModule");

        let _ = self
            .engine
            .register_trigger_type(TriggerType {
                id: TRIGGER_TYPE.to_string(),
                _description: "State trigger".to_string(),
                registrator: Box::new(self.clone()),
                worker_id: None,
            })
            .await;

        Ok(())
    }
}

#[async_trait::async_trait]
impl ConfigurableModule for StateCoreModule {
    type Config = StateModuleConfig;
    type Adapter = dyn StateAdapter;
    type AdapterRegistration = super::registry::StateAdapterRegistration;
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::state::adapters::RedisAdapter";

    async fn registry() -> &'static SyncRwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<SyncRwLock<HashMap<String, AdapterFactory<dyn StateAdapter>>>> =
            Lazy::new(|| SyncRwLock::new(StateCoreModule::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, _config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self {
            adapter,
            engine,
            triggers: Arc::new(StateTriggers::new()),
        }
    }

    fn adapter_class_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.class.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

impl StateCoreModule {
    /// Invoke triggers for a given event type with condition checks
    async fn invoke_triggers(&self, event_data: StateEventData) {
        // Collect triggers into Vec to release the lock before spawning
        let triggers: Vec<Trigger> = {
            let triggers_guard = self.triggers.list.read().await;
            triggers_guard.values().cloned().collect()
        };
        let engine = self.engine.clone();
        let event_type = event_data.event_type.clone();

        if let Ok(event_data) = serde_json::to_value(event_data) {
            tokio::spawn(async move {
                tracing::info!("Invoking triggers for event type {:?}", event_type);

                for trigger in triggers {
                    let trigger = trigger.clone();

                    // Check condition if specified
                    let condition_function_path = trigger
                        .config
                        .get("condition_function_path")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

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
                                if let Some(passed) = result.as_bool() {
                                    if !passed {
                                        tracing::debug!(
                                            function_path = %trigger.function_path,
                                            "Condition check failed, skipping handler"
                                        );
                                        continue;
                                    }
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

#[service(name = "state")]
impl StateCoreModule {
    #[function(name = "state.set", description = "Set a value in state")]
    pub async fn set(&self, input: StateSetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        match self
            .adapter
            .set(&input.group_id, &input.item_id, input.data.clone())
            .await
        {
            Ok(value) => {
                let old_value = value.old_value.clone();
                let new_value = value.new_value.clone();
                let is_create = old_value.is_none();
                // Invoke triggers after successful set
                let event_data = StateEventData {
                    message_type: "state".to_string(),
                    event_type: if is_create {
                        StateEventType::Created
                    } else {
                        StateEventType::Updated
                    },
                    group_id: input.group_id,
                    item_id: input.item_id,
                    old_value: old_value,
                    new_value: new_value.clone(),
                };

                self.invoke_triggers(event_data).await;

                FunctionResult::Success(Some(serde_json::to_value(value).unwrap()))
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to set value: {}", e),
                code: "SET_ERROR".to_string(),
            }),
        }
    }

    #[function(name = "state.get", description = "Get a value from state")]
    pub async fn get(&self, input: StateGetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.adapter.get(&input.group_id, &input.item_id).await {
            Ok(value) => FunctionResult::Success(value),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to get value: {}", e),
                code: "GET_ERROR".to_string(),
            }),
        }
    }

    #[function(name = "state.delete", description = "Delete a value from state")]
    pub async fn delete(
        &self,
        input: StateDeleteInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let value = match self.adapter.get(&input.group_id, &input.item_id).await {
            Ok(v) => v,
            Err(e) => {
                return FunctionResult::Failure(ErrorBody {
                    message: format!("Failed to get value before delete: {}", e),
                    code: "GET_ERROR".to_string(),
                });
            }
        };

        match self.adapter.delete(&input.group_id, &input.item_id).await {
            Ok(_) => {
                // Invoke triggers after successful delete
                let event_data = StateEventData {
                    message_type: "state".to_string(),
                    event_type: StateEventType::Deleted,
                    group_id: input.group_id,
                    item_id: input.item_id,
                    old_value: value.clone(),
                    new_value: Value::Null,
                };

                self.invoke_triggers(event_data).await;

                FunctionResult::Success(value)
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to delete value: {}", e),
                code: "DELETE_ERROR".to_string(),
            }),
        }
    }

    #[function(name = "state.update", description = "Update a value in state")]
    pub async fn update(
        &self,
        input: StateUpdateInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match self
            .adapter
            .update(&input.group_id, &input.item_id, input.ops)
            .await
        {
            Ok(value) => {
                let old_value = value.old_value.clone();
                let new_value = value.new_value.clone();
                let is_create = old_value.is_none();
                let event_data = StateEventData {
                    message_type: "state".to_string(),
                    event_type: if is_create {
                        StateEventType::Created
                    } else {
                        StateEventType::Updated
                    },
                    group_id: input.group_id,
                    item_id: input.item_id,
                    old_value: old_value,
                    new_value: new_value,
                };

                self.invoke_triggers(event_data).await;

                FunctionResult::Success(Some(serde_json::to_value(value).unwrap()))
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to update value: {}", e),
                code: "UPDATE_ERROR".to_string(),
            }),
        }
    }

    #[function(name = "state.list", description = "Get a group from state")]
    pub async fn list(
        &self,
        input: StateGetGroupInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.adapter.list(&input.group_id).await {
            Ok(values) => FunctionResult::Success(serde_json::to_value(values).ok()),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to list values: {}", e),
                code: "LIST_ERROR".to_string(),
            }),
        }
    }
}

crate::register_module!(
    "modules::state::StateModule",
    StateCoreModule,
    enabled_by_default = true
);
