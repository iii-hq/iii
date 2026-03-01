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
        crate::modules::telemetry::collector::track_state_set();
        match self
            .adapter
            .set(&input.scope, &input.key, input.value.clone())
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
        crate::modules::telemetry::collector::track_state_get();
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

        crate::modules::telemetry::collector::track_state_delete();
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
        crate::modules::telemetry::collector::track_state_update();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{
        observability::metrics::ensure_default_meter,
        state::adapters::kv_store::BuiltinKvStoreAdapter,
    };
    use iii_sdk::{UpdateResult, types::SetResult};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FakeStateAdapter {
        set_error: Option<&'static str>,
        get_error: Option<&'static str>,
        delete_error: Option<&'static str>,
        update_error: Option<&'static str>,
        list_error: Option<&'static str>,
        list_groups_error: Option<&'static str>,
        destroy_error: Option<&'static str>,
        set_result: SetResult,
        get_result: Option<Value>,
        update_result: UpdateResult,
        list_values: Vec<Value>,
        groups: Vec<String>,
        destroy_called: AtomicBool,
    }

    impl Default for FakeStateAdapter {
        fn default() -> Self {
            Self {
                set_error: None,
                get_error: None,
                delete_error: None,
                update_error: None,
                list_error: None,
                list_groups_error: None,
                destroy_error: None,
                set_result: SetResult {
                    old_value: None,
                    new_value: serde_json::json!({"ok": true}),
                },
                get_result: None,
                update_result: UpdateResult {
                    old_value: None,
                    new_value: serde_json::json!({"ok": true}),
                },
                list_values: Vec::new(),
                groups: Vec::new(),
                destroy_called: AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl StateAdapter for FakeStateAdapter {
        async fn set(&self, _scope: &str, _key: &str, _value: Value) -> anyhow::Result<SetResult> {
            match self.set_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(self.set_result.clone()),
            }
        }

        async fn get(&self, _scope: &str, _key: &str) -> anyhow::Result<Option<Value>> {
            match self.get_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(self.get_result.clone()),
            }
        }

        async fn delete(&self, _scope: &str, _key: &str) -> anyhow::Result<()> {
            match self.delete_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(()),
            }
        }

        async fn update(
            &self,
            _scope: &str,
            _key: &str,
            _ops: Vec<iii_sdk::UpdateOp>,
        ) -> anyhow::Result<UpdateResult> {
            match self.update_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(self.update_result.clone()),
            }
        }

        async fn list(&self, _scope: &str) -> anyhow::Result<Vec<Value>> {
            match self.list_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(self.list_values.clone()),
            }
        }

        async fn list_groups(&self) -> anyhow::Result<Vec<String>> {
            match self.list_groups_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(self.groups.clone()),
            }
        }

        async fn destroy(&self) -> anyhow::Result<()> {
            self.destroy_called.store(true, Ordering::Relaxed);
            match self.destroy_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(()),
            }
        }
    }

    fn setup() -> (Arc<Engine>, StateCoreModule) {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let adapter: Arc<dyn StateAdapter> = Arc::new(BuiltinKvStoreAdapter::new(None));
        let module = StateCoreModule {
            adapter,
            engine: engine.clone(),
            triggers: Arc::new(StateTriggers::new()),
        };
        (engine, module)
    }

    fn setup_with_adapter(adapter: Arc<dyn StateAdapter>) -> (Arc<Engine>, StateCoreModule) {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let module = StateCoreModule {
            adapter,
            engine: engine.clone(),
            triggers: Arc::new(StateTriggers::new()),
        };
        (engine, module)
    }

    // ---- set/get/delete/list operations ----

    #[tokio::test]
    async fn test_set_and_get() {
        let (_engine, module) = setup();

        let set_input = StateSetInput {
            scope: "test-scope".to_string(),
            key: "key1".to_string(),
            value: serde_json::json!({"name": "Alice"}),
        };
        let result = module.set(set_input).await;
        assert!(matches!(result, FunctionResult::Success(_)));

        let get_input = StateGetInput {
            scope: "test-scope".to_string(),
            key: "key1".to_string(),
        };
        let result = module.get(get_input).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, serde_json::json!({"name": "Alice"}));
            }
            _ => panic!("Expected Success with Some value"),
        }
    }

    #[tokio::test]
    async fn test_get_nonexistent_key() {
        let (_engine, module) = setup();

        let get_input = StateGetInput {
            scope: "test-scope".to_string(),
            key: "nonexistent".to_string(),
        };
        let result = module.get(get_input).await;
        assert!(matches!(result, FunctionResult::Success(None)));
    }

    #[tokio::test]
    async fn test_set_overwrites_existing() {
        let (_engine, module) = setup();

        let set_input = StateSetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
            value: serde_json::json!(1),
        };
        module.set(set_input).await;

        let set_input = StateSetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
            value: serde_json::json!(2),
        };
        let result = module.set(set_input).await;

        // Verify the old value is in the set result
        if let FunctionResult::Success(Some(val)) = &result {
            assert_eq!(val["old_value"], serde_json::json!(1));
            assert_eq!(val["new_value"], serde_json::json!(2));
        } else {
            panic!("Expected Success with Some value");
        }

        // Verify get returns updated value
        let get_input = StateGetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
        };
        let result = module.get(get_input).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, serde_json::json!(2));
            }
            _ => panic!("Expected Success with Some value"),
        }
    }

    #[tokio::test]
    async fn test_delete_existing_key() {
        let (_engine, module) = setup();

        let set_input = StateSetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
            value: serde_json::json!("hello"),
        };
        module.set(set_input).await;

        let delete_input = StateDeleteInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
        };
        let result = module.delete(delete_input).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, serde_json::json!("hello"));
            }
            _ => panic!("Expected Success with deleted value"),
        }

        // Verify it's gone
        let get_input = StateGetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
        };
        let result = module.get(get_input).await;
        assert!(matches!(result, FunctionResult::Success(None)));
    }

    #[tokio::test]
    async fn test_delete_nonexistent_key() {
        let (_engine, module) = setup();

        let delete_input = StateDeleteInput {
            scope: "scope".to_string(),
            key: "nonexistent".to_string(),
        };
        let result = module.delete(delete_input).await;
        // Should succeed with None (nothing was there to delete)
        assert!(matches!(result, FunctionResult::Success(None)));
    }

    #[tokio::test]
    async fn test_list_scope() {
        let (_engine, module) = setup();

        // Set multiple items in the same scope
        for i in 0..3 {
            let set_input = StateSetInput {
                scope: "my-scope".to_string(),
                key: format!("item-{}", i),
                value: serde_json::json!({"index": i}),
            };
            module.set(set_input).await;
        }

        let list_input = StateGetGroupInput {
            scope: "my-scope".to_string(),
        };
        let result = module.list(list_input).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                let arr = value.as_array().expect("list should return array");
                assert_eq!(arr.len(), 3);
            }
            _ => panic!("Expected Success with Some value"),
        }
    }

    #[tokio::test]
    async fn test_list_empty_scope() {
        let (_engine, module) = setup();

        let list_input = StateGetGroupInput {
            scope: "empty-scope".to_string(),
        };
        let result = module.list(list_input).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                let arr = value.as_array().expect("list should return array");
                assert!(arr.is_empty());
            }
            _ => panic!("Expected Success with Some value"),
        }
    }

    #[tokio::test]
    async fn test_list_groups() {
        let (_engine, module) = setup();

        // Set items in different scopes
        for scope in &["alpha", "beta", "gamma"] {
            let set_input = StateSetInput {
                scope: scope.to_string(),
                key: "k".to_string(),
                value: serde_json::json!(1),
            };
            module.set(set_input).await;
        }

        let result = module.list_groups(StateListGroupsInput {}).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                let groups = value["groups"].as_array().expect("groups should be array");
                let group_names: Vec<&str> = groups.iter().filter_map(|v| v.as_str()).collect();
                assert_eq!(group_names.len(), 3);
                // They should be sorted
                assert_eq!(group_names, vec!["alpha", "beta", "gamma"]);
            }
            _ => panic!("Expected Success with Some value"),
        }
    }

    // ---- invoke_triggers tests ----

    #[tokio::test]
    async fn test_invoke_triggers_scope_filter() {
        let (_engine, module) = setup();

        // Register a trigger that only matches scope "orders"
        let trigger = crate::trigger::Trigger {
            id: "state-trig-1".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::handler".to_string(),
            config: serde_json::json!({
                "scope": "orders"
            }),
            worker_id: None,
        };

        {
            let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
                trigger.config.clone(),
            )
            .unwrap();
            module.triggers.list.write().await.insert(
                trigger.id.clone(),
                super::super::trigger::StateTrigger {
                    config,
                    trigger: trigger.clone(),
                },
            );
        }

        // Event with different scope should be filtered out
        let event_data = super::super::structs::StateEventData {
            message_type: "state".to_string(),
            event_type: super::super::structs::StateEventType::Created,
            scope: "users".to_string(),
            key: "user-1".to_string(),
            old_value: None,
            new_value: serde_json::json!({"name": "Alice"}),
        };

        // This should not panic even though the function doesn't exist -
        // the scope filter should skip it
        module.invoke_triggers(event_data).await;

        // Give the spawned task time to complete
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn test_invoke_triggers_key_filter() {
        let (_engine, module) = setup();

        // Register a trigger that only matches key "special-key"
        let trigger = crate::trigger::Trigger {
            id: "state-trig-2".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::handler".to_string(),
            config: serde_json::json!({
                "key": "special-key"
            }),
            worker_id: None,
        };

        {
            let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
                trigger.config.clone(),
            )
            .unwrap();
            module.triggers.list.write().await.insert(
                trigger.id.clone(),
                super::super::trigger::StateTrigger {
                    config,
                    trigger: trigger.clone(),
                },
            );
        }

        // Event with different key should be filtered out
        let event_data = super::super::structs::StateEventData {
            message_type: "state".to_string(),
            event_type: super::super::structs::StateEventType::Updated,
            scope: "scope".to_string(),
            key: "other-key".to_string(),
            old_value: Some(serde_json::json!(1)),
            new_value: serde_json::json!(2),
        };

        module.invoke_triggers(event_data).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn test_invoke_triggers_matching_trigger_calls_function() {
        let (engine, module) = setup();

        // Register a function that the trigger will call
        let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
        let tx_clone = tx.clone();

        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::on_state_change".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(move |input: Value| {
                let tx = tx_clone.clone();
                async move {
                    if let Some(sender) = tx.lock().unwrap().take() {
                        let _ = sender.send(input);
                    }
                    FunctionResult::Success(None)
                }
            }),
        );

        // Register a trigger with no scope/key filter (matches everything)
        let trigger = crate::trigger::Trigger {
            id: "state-trig-all".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::on_state_change".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
        };

        {
            let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
                trigger.config.clone(),
            )
            .unwrap();
            module.triggers.list.write().await.insert(
                trigger.id.clone(),
                super::super::trigger::StateTrigger {
                    config,
                    trigger: trigger.clone(),
                },
            );
        }

        let event_data = super::super::structs::StateEventData {
            message_type: "state".to_string(),
            event_type: super::super::structs::StateEventType::Created,
            scope: "test-scope".to_string(),
            key: "test-key".to_string(),
            old_value: None,
            new_value: serde_json::json!({"data": true}),
        };

        module.invoke_triggers(event_data).await;

        let received = tokio::time::timeout(std::time::Duration::from_secs(2), rx)
            .await
            .expect("timed out waiting for trigger")
            .expect("channel closed");

        assert_eq!(received["scope"], "test-scope");
        assert_eq!(received["key"], "test-key");
    }

    #[tokio::test]
    async fn test_update_operation() {
        let (_engine, module) = setup();

        // First set a value
        let set_input = StateSetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
            value: serde_json::json!({"count": 0, "name": "test"}),
        };
        module.set(set_input).await;

        // Update using ops
        let update_input = StateUpdateInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
            ops: vec![iii_sdk::UpdateOp::Set {
                path: iii_sdk::FieldPath("count".to_string()),
                value: Some(serde_json::json!(42)),
            }],
        };
        let result = module.update(update_input).await;
        assert!(matches!(result, FunctionResult::Success(_)));

        // Verify the update
        let get_input = StateGetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
        };
        let result = module.get(get_input).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value["count"], 42);
                assert_eq!(value["name"], "test");
            }
            _ => panic!("Expected Success with Some value"),
        }
    }

    #[tokio::test]
    async fn test_initialize_registers_trigger_type_and_destroy_calls_adapter() {
        let adapter = Arc::new(FakeStateAdapter::default());
        let (engine, module) = setup_with_adapter(adapter.clone());

        module.initialize().await.unwrap();
        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key(TRIGGER_TYPE)
        );

        module.destroy().await.unwrap();
        assert!(adapter.destroy_called.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn test_set_returns_failure_when_adapter_errors() {
        let adapter = Arc::new(FakeStateAdapter {
            set_error: Some("set exploded"),
            ..Default::default()
        });
        let (_engine, module) = setup_with_adapter(adapter);

        let result = module
            .set(StateSetInput {
                scope: "scope".to_string(),
                key: "key".to_string(),
                value: serde_json::json!(1),
            })
            .await;

        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "SET_ERROR"),
            _ => panic!("expected set failure"),
        }
    }

    #[tokio::test]
    async fn test_delete_returns_get_error_when_lookup_fails() {
        let adapter = Arc::new(FakeStateAdapter {
            get_error: Some("missing backend"),
            ..Default::default()
        });
        let (_engine, module) = setup_with_adapter(adapter);

        let result = module
            .delete(StateDeleteInput {
                scope: "scope".to_string(),
                key: "key".to_string(),
            })
            .await;

        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "GET_ERROR"),
            _ => panic!("expected delete lookup failure"),
        }
    }

    #[tokio::test]
    async fn test_update_returns_failure_when_adapter_errors() {
        let adapter = Arc::new(FakeStateAdapter {
            update_error: Some("update exploded"),
            ..Default::default()
        });
        let (_engine, module) = setup_with_adapter(adapter);

        let result = module
            .update(StateUpdateInput {
                scope: "scope".to_string(),
                key: "key".to_string(),
                ops: vec![],
            })
            .await;

        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "UPDATE_ERROR"),
            _ => panic!("expected update failure"),
        }
    }

    #[tokio::test]
    async fn test_list_returns_failure_when_adapter_errors() {
        let adapter = Arc::new(FakeStateAdapter {
            list_error: Some("list exploded"),
            ..Default::default()
        });
        let (_engine, module) = setup_with_adapter(adapter);

        let result = module
            .list(StateGetGroupInput {
                scope: "scope".to_string(),
            })
            .await;

        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "LIST_ERROR"),
            _ => panic!("expected list failure"),
        }
    }

    #[tokio::test]
    async fn test_list_groups_deduplicates_and_sorts_adapter_values() {
        let adapter = Arc::new(FakeStateAdapter {
            groups: vec![
                "beta".to_string(),
                "alpha".to_string(),
                "beta".to_string(),
                "gamma".to_string(),
            ],
            ..Default::default()
        });
        let (_engine, module) = setup_with_adapter(adapter);

        let result = module.list_groups(StateListGroupsInput {}).await;

        match result {
            FunctionResult::Success(Some(value)) => {
                let groups = value["groups"].as_array().unwrap();
                assert_eq!(
                    groups,
                    &vec![
                        serde_json::json!("alpha"),
                        serde_json::json!("beta"),
                        serde_json::json!("gamma"),
                    ]
                );
            }
            _ => panic!("expected list_groups success"),
        }
    }

    #[tokio::test]
    async fn test_invoke_triggers_condition_none_skips_handler() {
        let (engine, module) = setup();

        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::condition_none".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(
                |_input: Value| async move { FunctionResult::Success(None) },
            ),
        );

        let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
        let tx_clone = tx.clone();
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::state_handler".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(move |input: Value| {
                let tx = tx_clone.clone();
                async move {
                    if let Some(sender) = tx.lock().unwrap().take() {
                        let _ = sender.send(input);
                    }
                    FunctionResult::Success(None)
                }
            }),
        );

        let trigger = crate::trigger::Trigger {
            id: "state-trig-condition-none".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::state_handler".to_string(),
            config: serde_json::json!({
                "condition_function_id": "test::condition_none"
            }),
            worker_id: None,
        };

        let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
            trigger.config.clone(),
        )
        .unwrap();
        module.triggers.list.write().await.insert(
            trigger.id.clone(),
            super::super::trigger::StateTrigger { config, trigger },
        );

        module
            .invoke_triggers(super::super::structs::StateEventData {
                message_type: "state".to_string(),
                event_type: super::super::structs::StateEventType::Updated,
                scope: "scope".to_string(),
                key: "key".to_string(),
                old_value: Some(serde_json::json!(1)),
                new_value: serde_json::json!(2),
            })
            .await;

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), rx)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_invoke_triggers_condition_error_skips_handler() {
        let (engine, module) = setup();

        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::condition_error".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(|_input: Value| async move {
                FunctionResult::Failure(ErrorBody {
                    code: "COND".to_string(),
                    message: "bad".to_string(),
                })
            }),
        );

        let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
        let tx_clone = tx.clone();
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::state_handler_error_skip".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(move |input: Value| {
                let tx = tx_clone.clone();
                async move {
                    if let Some(sender) = tx.lock().unwrap().take() {
                        let _ = sender.send(input);
                    }
                    FunctionResult::Success(None)
                }
            }),
        );

        let trigger = crate::trigger::Trigger {
            id: "state-trig-condition-error".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::state_handler_error_skip".to_string(),
            config: serde_json::json!({
                "condition_function_id": "test::condition_error"
            }),
            worker_id: None,
        };

        let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
            trigger.config.clone(),
        )
        .unwrap();
        module.triggers.list.write().await.insert(
            trigger.id.clone(),
            super::super::trigger::StateTrigger { config, trigger },
        );

        module
            .invoke_triggers(super::super::structs::StateEventData {
                message_type: "state".to_string(),
                event_type: super::super::structs::StateEventType::Updated,
                scope: "scope".to_string(),
                key: "key".to_string(),
                old_value: Some(serde_json::json!(1)),
                new_value: serde_json::json!(2),
            })
            .await;

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), rx)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_invoke_triggers_handler_error_is_tolerated() {
        let (engine, module) = setup();

        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::failing_state_handler".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(|_input: Value| async move {
                FunctionResult::Failure(ErrorBody {
                    code: "HANDLER".to_string(),
                    message: "failed".to_string(),
                })
            }),
        );

        let trigger = crate::trigger::Trigger {
            id: "state-trig-handler-error".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::failing_state_handler".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
        };

        let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
            trigger.config.clone(),
        )
        .unwrap();
        module.triggers.list.write().await.insert(
            trigger.id.clone(),
            super::super::trigger::StateTrigger { config, trigger },
        );

        module
            .invoke_triggers(super::super::structs::StateEventData {
                message_type: "state".to_string(),
                event_type: super::super::structs::StateEventType::Created,
                scope: "scope".to_string(),
                key: "key".to_string(),
                old_value: None,
                new_value: serde_json::json!(true),
            })
            .await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
