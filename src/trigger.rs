// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{pin::Pin, sync::Arc};

use colored::Colorize;
use dashmap::DashMap;
use futures::Future;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub struct TriggerType {
    pub id: String,
    pub _description: String,
    // pub config_schema: Schema,
    pub registrator: Box<dyn TriggerRegistrator>,
    pub worker_id: Option<Uuid>,
}

pub trait TriggerRegistrator: Send + Sync {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>>;
    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>>;
}

#[derive(Clone, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub id: String,
    pub trigger_type: String,
    pub function_id: String,
    pub config: Value,
    pub worker_id: Option<Uuid>,
}

// Only `id` is considered for Hash and Eq/PartialEq
impl PartialEq for Trigger {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::hash::Hash for Trigger {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[derive(Default)]
pub struct TriggerRegistry {
    pub trigger_types: Arc<DashMap<String, TriggerType>>,
    pub triggers: Arc<DashMap<String, Trigger>>,
}

impl TriggerRegistry {
    pub fn new() -> Self {
        Self {
            trigger_types: Arc::new(DashMap::new()),
            triggers: Arc::new(DashMap::new()),
        }
    }

    pub async fn unregister_worker(&self, worker_id: &Uuid) {
        let worker_trigger_type_ids: Vec<String> = self
            .trigger_types
            .iter()
            .filter(|pair| pair.value().worker_id == Some(*worker_id))
            .map(|pair| pair.key().clone())
            .collect();

        let worker_triggers: Vec<Trigger> = self
            .triggers
            .iter()
            .filter(|pair| pair.value().worker_id == Some(*worker_id))
            .map(|pair| pair.value().clone())
            .collect();

        for trigger in worker_triggers {
            tracing::debug!(trigger_id = trigger.id, "Removing trigger");
            self.triggers.remove(&trigger.id);

            if let Some(trigger_type) = self.trigger_types.get(&trigger.trigger_type) {
                tracing::debug!(trigger_type_id = trigger_type.id, "Unregistering trigger");

                let result: Result<(), anyhow::Error> = trigger_type
                    .registrator
                    .unregister_trigger(trigger.clone())
                    .await;
                if let Err(err) = result {
                    tracing::error!(error = %err, "Error unregistering trigger");
                }
            }

            tracing::debug!(trigger_id = trigger.id, "Trigger removed");
        }

        for trigger_type_id in worker_trigger_type_ids {
            tracing::debug!(trigger_type_id = %trigger_type_id, "Removing trigger type");
            self.trigger_types.remove(&trigger_type_id);
            tracing::debug!(trigger_type_id = %trigger_type_id, "Trigger type removed");
        }
    }

    pub async fn register_trigger_type(
        &self,
        trigger_type: TriggerType,
    ) -> Result<(), anyhow::Error> {
        let trigger_type_id = trigger_type.id.clone();

        tracing::info!(
            "{} Trigger Type {}",
            "[REGISTERED]".green(),
            trigger_type_id.purple()
        );

        let matching_triggers: Vec<Trigger> = self
            .triggers
            .iter()
            .filter(|pair| pair.value().trigger_type == trigger_type_id)
            .map(|pair| pair.value().clone())
            .collect();

        for trigger in matching_triggers {
            let result = trigger_type
                .registrator
                .register_trigger(trigger.clone())
                .await;
            if let Err(err) = result {
                tracing::error!(error = %err, "Error registering trigger");
            }
        }

        self.trigger_types
            .insert(trigger_type.id.clone(), trigger_type);

        Ok(())
    }

    pub async fn register_trigger(&self, trigger: Trigger) -> Result<(), anyhow::Error> {
        let trigger_type_id = trigger.trigger_type.clone();
        let Some(trigger_type) = self.trigger_types.get(&trigger_type_id) else {
            tracing::error!(
                trigger_type_id = %trigger_type_id.purple(),
                "Trigger type not found"
            );
            return Err(anyhow::anyhow!("Trigger type not found"));
        };

        match trigger_type
            .registrator
            .register_trigger(trigger.clone())
            .await
        {
            Ok(_) => {}
            Err(err) => {
                tracing::error!(error = %err, "Error registering trigger");
                return Err(err);
            }
        }

        drop(trigger_type);

        tracing::debug!(trigger = %trigger.id, worker_id = %trigger.worker_id.unwrap_or_default(), "Registering trigger");

        self.triggers.insert(trigger.id.clone(), trigger);

        Ok(())
    }

    pub async fn unregister_trigger(
        &self,
        id: String,
        trigger_type: String,
    ) -> Result<(), anyhow::Error> {
        tracing::info!(
            "Unregistering trigger: {} of type: {}",
            id.purple(),
            trigger_type.purple()
        );

        let Some(trigger_entry) = self.triggers.get(&id) else {
            return Err(anyhow::anyhow!("Trigger not found"));
        };
        let trigger = trigger_entry.value().clone();
        drop(trigger_entry);

        if let Some(tt) = self.trigger_types.get(&trigger_type) {
            let result: Result<(), anyhow::Error> =
                tt.registrator.unregister_trigger(trigger.clone()).await;

            result?
        }

        self.triggers.remove(&id);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    struct MockTriggerRegistrator {
        register_calls: Arc<Mutex<Vec<Trigger>>>,
        unregister_calls: Arc<Mutex<Vec<Trigger>>>,
        should_fail_register: bool,
        should_fail_unregister: bool,
    }

    impl MockTriggerRegistrator {
        fn new() -> Self {
            Self {
                register_calls: Arc::new(Mutex::new(Vec::new())),
                unregister_calls: Arc::new(Mutex::new(Vec::new())),
                should_fail_register: false,
                should_fail_unregister: false,
            }
        }

        fn with_register_failure() -> Self {
            Self {
                register_calls: Arc::new(Mutex::new(Vec::new())),
                unregister_calls: Arc::new(Mutex::new(Vec::new())),
                should_fail_register: true,
                should_fail_unregister: false,
            }
        }
    }

    impl TriggerRegistrator for MockTriggerRegistrator {
        fn register_trigger(
            &self,
            trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            let calls = self.register_calls.clone();
            let should_fail = self.should_fail_register;
            Box::pin(async move {
                calls.lock().unwrap().push(trigger);
                if should_fail {
                    Err(anyhow::anyhow!("Mock register failure"))
                } else {
                    Ok(())
                }
            })
        }

        fn unregister_trigger(
            &self,
            trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            let calls = self.unregister_calls.clone();
            let should_fail = self.should_fail_unregister;
            Box::pin(async move {
                calls.lock().unwrap().push(trigger);
                if should_fail {
                    Err(anyhow::anyhow!("Mock unregister failure"))
                } else {
                    Ok(())
                }
            })
        }
    }

    #[test]
    fn test_trigger_equality() {
        let trigger1 = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({}),
            worker_id: None,
        };
        let trigger2 = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "different".to_string(),
            function_id: "different.function".to_string(),
            config: json!({"different": true}),
            worker_id: Some(Uuid::new_v4()),
        };
        assert!(trigger1 == trigger2);
    }

    #[test]
    fn test_trigger_inequality() {
        let trigger1 = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({}),
            worker_id: None,
        };
        let trigger2 = Trigger {
            id: "trigger-2".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({}),
            worker_id: None,
        };
        assert!(trigger1 != trigger2);
    }

    #[test]
    fn test_trigger_registry_new() {
        let registry = TriggerRegistry::new();
        assert_eq!(registry.trigger_types.len(), 0);
        assert_eq!(registry.triggers.len(), 0);
    }

    #[tokio::test]
    async fn test_register_trigger_type() {
        let registry = TriggerRegistry::new();
        let mock_registrator = MockTriggerRegistrator::new();
        let trigger_type = TriggerType {
            id: "cron".to_string(),
            _description: "Cron trigger".to_string(),
            registrator: Box::new(mock_registrator),
            worker_id: None,
        };

        let result = registry.register_trigger_type(trigger_type).await;
        assert!(result.is_ok());
        assert_eq!(registry.trigger_types.len(), 1);
        assert!(registry.trigger_types.contains_key("cron"));
    }

    #[tokio::test]
    async fn test_register_trigger_type_auto_registers_pending_triggers() {
        let registry = TriggerRegistry::new();
        let mock_registrator = MockTriggerRegistrator::new();
        let register_calls = mock_registrator.register_calls.clone();

        let trigger = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({"schedule": "0 0 * * *"}),
            worker_id: None,
        };
        registry.triggers.insert("trigger-1".to_string(), trigger.clone());

        let trigger_type = TriggerType {
            id: "cron".to_string(),
            _description: "Cron trigger".to_string(),
            registrator: Box::new(mock_registrator),
            worker_id: None,
        };

        registry.register_trigger_type(trigger_type).await.unwrap();

        let calls = register_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "trigger-1");
    }

    #[tokio::test]
    async fn test_register_trigger_success() {
        let registry = TriggerRegistry::new();
        let mock_registrator = MockTriggerRegistrator::new();
        let register_calls = mock_registrator.register_calls.clone();

        let trigger_type = TriggerType {
            id: "cron".to_string(),
            _description: "Cron trigger".to_string(),
            registrator: Box::new(mock_registrator),
            worker_id: None,
        };
        registry.register_trigger_type(trigger_type).await.unwrap();

        let trigger = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({"schedule": "0 0 * * *"}),
            worker_id: None,
        };

        let result = registry.register_trigger(trigger.clone()).await;
        assert!(result.is_ok());
        assert_eq!(registry.triggers.len(), 1);
        assert!(registry.triggers.contains_key("trigger-1"));

        let calls = register_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "trigger-1");
    }

    #[tokio::test]
    async fn test_register_trigger_type_not_found() {
        let registry = TriggerRegistry::new();
        let trigger = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "nonexistent".to_string(),
            function_id: "test.function".to_string(),
            config: json!({}),
            worker_id: None,
        };

        let result = registry.register_trigger(trigger).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Trigger type not found"));
    }

    #[tokio::test]
    async fn test_register_trigger_registrator_failure() {
        let registry = TriggerRegistry::new();
        let mock_registrator = MockTriggerRegistrator::with_register_failure();

        let trigger_type = TriggerType {
            id: "cron".to_string(),
            _description: "Cron trigger".to_string(),
            registrator: Box::new(mock_registrator),
            worker_id: None,
        };
        registry.register_trigger_type(trigger_type).await.unwrap();

        let trigger = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({}),
            worker_id: None,
        };

        let result = registry.register_trigger(trigger).await;
        assert!(result.is_err());
        assert_eq!(registry.triggers.len(), 0);
    }

    #[tokio::test]
    async fn test_unregister_trigger_success() {
        let registry = TriggerRegistry::new();
        let mock_registrator = MockTriggerRegistrator::new();
        let unregister_calls = mock_registrator.unregister_calls.clone();

        let trigger_type = TriggerType {
            id: "cron".to_string(),
            _description: "Cron trigger".to_string(),
            registrator: Box::new(mock_registrator),
            worker_id: None,
        };
        registry.register_trigger_type(trigger_type).await.unwrap();

        let trigger = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({}),
            worker_id: None,
        };
        registry.register_trigger(trigger.clone()).await.unwrap();

        let result = registry
            .unregister_trigger("trigger-1".to_string(), "cron".to_string())
            .await;
        assert!(result.is_ok());
        assert_eq!(registry.triggers.len(), 0);

        let calls = unregister_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "trigger-1");
    }

    #[tokio::test]
    async fn test_unregister_trigger_not_found() {
        let registry = TriggerRegistry::new();
        let result = registry
            .unregister_trigger("nonexistent".to_string(), "cron".to_string())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Trigger not found"));
    }

    #[tokio::test]
    async fn test_unregister_worker() {
        let registry = TriggerRegistry::new();
        let worker_id = Uuid::new_v4();
        let other_worker_id = Uuid::new_v4();

        let mock_registrator1 = MockTriggerRegistrator::new();
        let unregister_calls1 = mock_registrator1.unregister_calls.clone();
        let trigger_type1 = TriggerType {
            id: "cron".to_string(),
            _description: "Cron trigger".to_string(),
            registrator: Box::new(mock_registrator1),
            worker_id: Some(worker_id),
        };
        registry.register_trigger_type(trigger_type1).await.unwrap();

        let mock_registrator2 = MockTriggerRegistrator::new();
        let trigger_type2 = TriggerType {
            id: "http".to_string(),
            _description: "HTTP trigger".to_string(),
            registrator: Box::new(mock_registrator2),
            worker_id: Some(other_worker_id),
        };
        registry.register_trigger_type(trigger_type2).await.unwrap();

        let trigger1 = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({}),
            worker_id: Some(worker_id),
        };
        registry.register_trigger(trigger1).await.unwrap();

        let trigger2 = Trigger {
            id: "trigger-2".to_string(),
            trigger_type: "http".to_string(),
            function_id: "test.function2".to_string(),
            config: json!({}),
            worker_id: Some(other_worker_id),
        };
        registry.register_trigger(trigger2).await.unwrap();

        registry.unregister_worker(&worker_id).await;

        assert_eq!(registry.triggers.len(), 1);
        assert!(!registry.triggers.contains_key("trigger-1"));
        assert!(registry.triggers.contains_key("trigger-2"));
        assert_eq!(registry.trigger_types.len(), 1);
        assert!(!registry.trigger_types.contains_key("cron"));
        assert!(registry.trigger_types.contains_key("http"));

        let calls = unregister_calls1.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "trigger-1");
    }

    #[tokio::test]
    async fn test_unregister_worker_no_triggers() {
        let registry = TriggerRegistry::new();
        let worker_id = Uuid::new_v4();

        registry.unregister_worker(&worker_id).await;

        assert_eq!(registry.triggers.len(), 0);
        assert_eq!(registry.trigger_types.len(), 0);
    }
}
