// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock as SyncRwLock},
};

use function_macros::{function, service};
use once_cell::sync::Lazy;
use serde_json::Value;
use tracing::Instrument;

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
                StateGetInput, StateListGroupsInput, StateSetInput, StateUpdateInput,
            },
            trigger::{StateTrigger, StateTriggers, TRIGGER_TYPE},
        },
    },
    protocol::ErrorBody,
    trigger::TriggerType,
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
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::state::adapters::KvStore";

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
        let triggers: Vec<StateTrigger> = {
            let triggers_guard = self.triggers.list.read().await;
            triggers_guard.values().cloned().collect()
        };
        let engine = self.engine.clone();
        let event_type = event_data.event_type.clone();
        let event_key = event_data.key.clone();
        let event_scope = event_data.scope.clone();

        let current_span = tracing::Span::current();

        if let Ok(event_data) = serde_json::to_value(event_data) {
            tokio::spawn(
                async move {
                    tracing::debug!("Invoking triggers for event type {:?}", event_type);
                    let mut has_error = false;

                    for trigger in triggers {
                        let trigger = trigger.clone();

                        if let Some(scope) = &trigger.config.scope {
                            tracing::info!(
                                scope = %scope,
                                event_scope = %event_scope,
                                "Checking trigger scope"
                            );
                            if scope != &event_scope {
                                tracing::info!(
                                    scope = %scope,
                                    event_scope = %event_scope,
                                    "Trigger scope does not match event scope, skipping trigger"
                                );
                                continue;
                            }
                        }

                        if let Some(key) = &trigger.config.key {
                            tracing::info!(
                                key = %key,
                                event_key = %event_key,
                                "Checking trigger key"
                            );
                            if key != &event_key {
                                tracing::info!(
                                    key = %key,
                                    event_key = %event_key,
                                    "Trigger key does not match event key, skipping trigger"
                                );
                                continue;
                            }
                        }

                        if let Some(condition_function_id) = trigger.config.condition_function_id {
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
                                            function_id = %trigger.trigger.function_id,
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
                                    has_error = true;
                                    tracing::error!(
                                        condition_function_id = %condition_function_id,
                                        error = ?err,
                                        "Error invoking condition function"
                                    );
                                    continue;
                                }
                            }
                        }

                        // Invoke the handler function
                        tracing::info!(
                            function_id = %trigger.trigger.function_id,
                            "Invoking trigger"
                        );

                        let call_result = engine
                            .call(&trigger.trigger.function_id, event_data.clone())
                            .await;

                        match call_result {
                            Ok(_) => {
                                tracing::debug!(
                                    function_id = %trigger.trigger.function_id,
                                    "Trigger handler invoked successfully"
                                );
                            }
                            Err(err) => {
                                has_error = true;
                                tracing::error!(
                                    function_id = %trigger.trigger.function_id,
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
                }
                .instrument(tracing::info_span!(parent: current_span, "state_triggers", otel.status_code = tracing::field::Empty))
            );
        } else {
            tracing::error!("Failed to convert event data to value");
        }
    }
}

#[service(name = "state")]
impl StateCoreModule {
    #[function(id = "state::set", description = "Set a value in state")]
    pub async fn set(&self, input: StateSetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        match self
            .adapter
            .set(&input.scope, &input.key, input.data.clone())
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
                    scope: input.scope,
                    key: input.key,
                    old_value,
                    new_value: new_value.clone(),
                };

                self.invoke_triggers(event_data).await;

                match serde_json::to_value(value) {
                    Ok(value) => FunctionResult::Success(Some(value)),
                    Err(e) => FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to convert value to JSON: {}", e),
                        code: "JSON_ERROR".to_string(),
                    }),
                }
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to set value: {}", e),
                code: "SET_ERROR".to_string(),
            }),
        }
    }

    #[function(id = "state::get", description = "Get a value from state")]
    pub async fn get(&self, input: StateGetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.adapter.get(&input.scope, &input.key).await {
            Ok(value) => FunctionResult::Success(value),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to get value: {}", e),
                code: "GET_ERROR".to_string(),
            }),
        }
    }

    #[function(id = "state::delete", description = "Delete a value from state")]
    pub async fn delete(
        &self,
        input: StateDeleteInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let value = match self.adapter.get(&input.scope, &input.key).await {
            Ok(v) => v,
            Err(e) => {
                return FunctionResult::Failure(ErrorBody {
                    message: format!("Failed to get value before delete: {}", e),
                    code: "GET_ERROR".to_string(),
                });
            }
        };

        match self.adapter.delete(&input.scope, &input.key).await {
            Ok(_) => {
                // Invoke triggers after successful delete
                let event_data = StateEventData {
                    message_type: "state".to_string(),
                    event_type: StateEventType::Deleted,
                    scope: input.scope,
                    key: input.key,
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

    #[function(id = "state::update", description = "Update a value in state")]
    pub async fn update(
        &self,
        input: StateUpdateInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match self
            .adapter
            .update(&input.scope, &input.key, input.ops)
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
                    scope: input.scope,
                    key: input.key,
                    old_value,
                    new_value,
                };

                self.invoke_triggers(event_data).await;

                match serde_json::to_value(value) {
                    Ok(value) => FunctionResult::Success(Some(value)),
                    Err(e) => FunctionResult::Failure(ErrorBody {
                        message: format!("Failed to convert value to JSON: {}", e),
                        code: "JSON_ERROR".to_string(),
                    }),
                }
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to update value: {}", e),
                code: "UPDATE_ERROR".to_string(),
            }),
        }
    }

    #[function(id = "state::list", description = "Get a group from state")]
    pub async fn list(
        &self,
        input: StateGetGroupInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.adapter.list(&input.scope).await {
            Ok(values) => FunctionResult::Success(serde_json::to_value(values).ok()),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to list values: {}", e),
                code: "LIST_ERROR".to_string(),
            }),
        }
    }

    #[function(id = "state::list_groups", description = "List all state groups")]
    pub async fn list_groups(
        &self,
        _input: StateListGroupsInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.adapter.list_groups().await {
            Ok(groups) => {
                // Normalize: deduplicate and sort
                let mut normalized_groups: Vec<String> = groups
                    .into_iter()
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect();
                normalized_groups.sort();

                let result = serde_json::json!({
                    "groups": normalized_groups
                });
                FunctionResult::Success(Some(result))
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to list groups: {}", e),
                code: "LIST_GROUPS_ERROR".to_string(),
            }),
        }
    }
}

crate::register_module!(
    "modules::state::StateModule",
    StateCoreModule,
    enabled_by_default = true
);
