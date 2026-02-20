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

        #[cfg(feature = "heartbeat")]
        crate::modules::heartbeat::lifecycle::get_lifecycle_tracker().record_registration(
            crate::modules::heartbeat::lifecycle::EventKind::RegisterTrigger,
            Some(
                serde_json::json!({"trigger_id": trigger.id, "trigger_type": trigger.trigger_type}),
            ),
        );

        self.triggers.insert(trigger.id.clone(), trigger);

        Ok(())
    }

    pub async fn unregister_trigger(
        &self,
        id: String,
        trigger_type: Option<String>,
    ) -> Result<(), anyhow::Error> {
        tracing::info!(
            "Unregistering trigger: {} of type: {}",
            id.purple(),
            trigger_type.as_deref().unwrap_or("<missing>").purple()
        );

        let Some(trigger_entry) = self.triggers.get(&id) else {
            return Err(anyhow::anyhow!("Trigger not found"));
        };
        let trigger = trigger_entry.value().clone();
        drop(trigger_entry);

        if let Some(tt) = self.trigger_types.get(&trigger.trigger_type) {
            let result: Result<(), anyhow::Error> =
                tt.registrator.unregister_trigger(trigger.clone()).await;

            result?
        }

        self.triggers.remove(&id);

        Ok(())
    }
}
