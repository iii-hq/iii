// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, pin::Pin, sync::Arc};

use futures::Future;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{
    modules::state::StateCoreModule,
    trigger::{Trigger, TriggerRegistrator},
};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct StateTriggerConfig {
    pub scope: Option<String>,
    pub key: Option<String>,
    pub condition_function_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StateTrigger {
    pub config: StateTriggerConfig,
    pub trigger: Trigger,
}

pub struct StateTriggers {
    pub list: Arc<RwLock<HashMap<String, StateTrigger>>>,
}

impl Default for StateTriggers {
    fn default() -> Self {
        Self::new()
    }
}

impl StateTriggers {
    pub fn new() -> Self {
        Self {
            list: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

pub const TRIGGER_TYPE: &str = "state";

#[async_trait::async_trait]
impl TriggerRegistrator for StateCoreModule {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let triggers = &self.triggers.list;

        Box::pin(async move {
            let trigger_id = trigger.id.clone();
            let config = serde_json::from_value::<StateTriggerConfig>(trigger.config.clone());

            match config {
                Ok(config) => {
                    tracing::info!(
                        config = ?config,
                        function_id = %trigger.function_id,
                        "Registering trigger for function {}",
                        trigger.function_id
                    );

                    let _ = triggers
                        .write()
                        .await
                        .insert(trigger_id, StateTrigger { config, trigger });

                    Ok(())
                }
                Err(e) => {
                    tracing::error!("Failed to parse trigger config: {}", e);

                    Err(anyhow::anyhow!("Failed to parse trigger config: {}", e))
                }
            }
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let triggers = &self.triggers.list;

        Box::pin(async move {
            let trigger_id = trigger.id.clone();
            let _ = triggers.write().await.remove(&trigger_id);

            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use iii_sdk::{UpdateOp, UpdateResult, types::SetResult};
    use serde_json::json;

    use super::*;
    use crate::modules::{
        module::ConfigurableModule,
        state::{StateCoreModule, adapters::StateAdapter, config::StateModuleConfig},
    };

    struct NoopStateAdapter;

    #[async_trait::async_trait]
    impl StateAdapter for NoopStateAdapter {
        async fn set(
            &self,
            _scope: &str,
            _key: &str,
            _value: serde_json::Value,
        ) -> anyhow::Result<SetResult> {
            Ok(SetResult {
                old_value: None,
                new_value: serde_json::Value::Null,
            })
        }

        async fn get(&self, _scope: &str, _key: &str) -> anyhow::Result<Option<serde_json::Value>> {
            Ok(None)
        }

        async fn delete(&self, _scope: &str, _key: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn update(
            &self,
            _scope: &str,
            _key: &str,
            _ops: Vec<UpdateOp>,
        ) -> anyhow::Result<UpdateResult> {
            Ok(UpdateResult {
                old_value: None,
                new_value: serde_json::Value::Null,
            })
        }

        async fn list(&self, _scope: &str) -> anyhow::Result<Vec<serde_json::Value>> {
            Ok(Vec::new())
        }

        async fn list_groups(&self) -> anyhow::Result<Vec<String>> {
            Ok(Vec::new())
        }

        async fn destroy(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn test_module() -> StateCoreModule {
        <StateCoreModule as ConfigurableModule>::build(
            Arc::new(crate::engine::Engine::new()),
            StateModuleConfig::default(),
            Arc::new(NoopStateAdapter),
        )
    }

    #[tokio::test]
    async fn register_and_unregister_state_trigger_updates_registry() {
        assert_eq!(TRIGGER_TYPE, "state");
        let empty = StateTriggers::new();
        assert!(empty.list.read().await.is_empty());

        let module = test_module();
        let trigger = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: TRIGGER_TYPE.to_string(),
            function_id: "state.handler".to_string(),
            config: json!({
                "scope": "users",
                "key": "abc",
                "condition_function_id": "state.condition"
            }),
            worker_id: None,
        };

        module
            .register_trigger(trigger.clone())
            .await
            .expect("register trigger");
        {
            let stored = module.triggers.list.read().await;
            let stored = stored.get("trigger-1").expect("stored trigger");
            assert_eq!(stored.config.scope.as_deref(), Some("users"));
            assert_eq!(stored.config.key.as_deref(), Some("abc"));
            assert_eq!(
                stored.config.condition_function_id.as_deref(),
                Some("state.condition")
            );
        }

        module
            .unregister_trigger(trigger)
            .await
            .expect("unregister trigger");
        assert!(module.triggers.list.read().await.is_empty());
    }

    #[tokio::test]
    async fn register_trigger_rejects_invalid_config() {
        let module = test_module();
        let trigger = Trigger {
            id: "trigger-invalid".to_string(),
            trigger_type: TRIGGER_TYPE.to_string(),
            function_id: "state.handler".to_string(),
            config: json!({
                "scope": 123
            }),
            worker_id: None,
        };

        let error = module
            .register_trigger(trigger)
            .await
            .expect_err("invalid config should fail");
        assert!(error.to_string().contains("Failed to parse trigger config"));
    }
}
