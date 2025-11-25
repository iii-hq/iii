use std::{pin::Pin, sync::Arc};

use colored::Colorize;
use dashmap::DashMap;
use futures::Future;
use serde_json::Value;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::modules::logger::{LogLevel, log};

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

#[derive(Clone)]
pub struct Trigger {
    pub id: String,
    pub trigger_type: String,
    pub function_path: String,
    pub config: Value,
    pub worker_id: Option<Uuid>,
}

#[derive(Default)]
pub struct TriggerRegistry {
    pub trigger_types: Arc<RwLock<DashMap<String, TriggerType>>>,
    pub triggers: Arc<RwLock<DashMap<String, Trigger>>>,
}

impl TriggerRegistry {
    pub fn new() -> Self {
        Self {
            trigger_types: Arc::new(RwLock::new(DashMap::new())),
            triggers: Arc::new(RwLock::new(DashMap::new())),
        }
    }

    pub async fn unregister_worker(&self, worker_id: &Uuid) {
        {
            let worker_trigger_types = self
                .trigger_types
                .read()
                .await
                .iter()
                .filter(|pair| pair.value().worker_id == Some(*worker_id))
                .map(|pair| pair.key().clone())
                .collect::<Vec<_>>();

            if !worker_trigger_types.is_empty() {
                let write_lock = self.trigger_types.write().await;
                for trigger_type_id in worker_trigger_types {
                    tracing::debug!(trigger_type_id = %trigger_type_id, "Removing trigger type");
                    write_lock.remove(&trigger_type_id.to_string());
                    tracing::debug!(trigger_type_id = %trigger_type_id, "Trigger type removed");
                }
            }
        }

        let worker_triggers = self
            .triggers
            .read()
            .await
            .iter()
            .filter(|pair| pair.value().worker_id == Some(*worker_id))
            .map(|pair| pair.value().clone())
            .collect::<Vec<Trigger>>();

        if !worker_triggers.is_empty() {
            let write_lock = self.triggers.write().await;
            for trigger in worker_triggers {
                tracing::debug!(trigger_id = trigger.id, "Removing trigger");
                write_lock.remove(&trigger.id);

                if let Some(trigger_type) =
                    self.trigger_types.read().await.get(&trigger.trigger_type)
                {
                    tracing::debug!(trigger_type_id = trigger_type.id, "Unregistering trigger");

                    let result: Result<(), anyhow::Error> = trigger_type
                        .registrator
                        .unregister_trigger(trigger.clone())
                        .await;
                    if result.is_err() {
                        tracing::error!(error = %result.err().unwrap(), "Error unregistering trigger");
                    }
                }

                tracing::debug!(trigger_id = trigger.id, "Trigger removed");
            }
        }
    }

    pub async fn register_trigger_type(
        &self,
        trigger_type: TriggerType,
    ) -> Result<(), anyhow::Error> {
        let trigger_type_id = &trigger_type.id;

        log(
            LogLevel::Info,
            "core::TriggerRegistry",
            &format!("Registering trigger type: {}", trigger_type_id.purple()),
            None,
            None,
        );

        for pair in self.triggers.read().await.iter() {
            let trigger = pair.value();

            if &trigger.trigger_type == trigger_type_id {
                let result = trigger_type
                    .registrator
                    .register_trigger(trigger.clone())
                    .await;
                if let Err(err) = result {
                    tracing::error!(error = %err, "Error registering trigger");
                }
            }
        }

        self.trigger_types
            .write()
            .await
            .insert(trigger_type.id.clone(), trigger_type);

        Ok(())
    }

    pub async fn register_trigger(&self, trigger: Trigger) -> Result<(), anyhow::Error> {
        let trigger_type_id = trigger.trigger_type.clone();
        let lock = self.trigger_types.read().await;

        let Some(trigger_type) = lock.get(&trigger_type_id) else {
            log(
                LogLevel::Error,
                "core::TriggerRegistry",
                &format!("Trigger type not found: {}", trigger_type_id.purple()),
                None,
                None,
            );
            return Err(anyhow::anyhow!("Trigger type not found"));
        };

        let _: Result<(), anyhow::Error> = trigger_type
            .registrator
            .register_trigger(trigger.clone())
            .await;

        tracing::debug!(trigger = %trigger.id, worker_id = %trigger.worker_id.unwrap_or_default(), "Registering trigger");

        self.triggers
            .write()
            .await
            .insert(trigger.id.clone(), trigger);

        Ok(())
    }

    pub async fn unregister_trigger(
        &self,
        id: String,
        trigger_type: String,
    ) -> Result<(), anyhow::Error> {
        log(
            LogLevel::Info,
            "core::TriggerRegistry",
            &format!(
                "Unregistering trigger: {} of type: {}",
                id.purple(),
                trigger_type.purple()
            ),
            None,
            None,
        );

        let trigger_lock = self.triggers.read().await;
        let Some(trigger) = trigger_lock.get(&id) else {
            return Err(anyhow::anyhow!("Trigger not found"));
        };
        let trigger_type_lock = self.trigger_types.read().await;
        let trigger_type = trigger_type_lock.get(&trigger_type.clone());

        if trigger_type.is_some() {
            let result: Result<(), anyhow::Error> = trigger_type
                .unwrap()
                .registrator
                .unregister_trigger(trigger.clone())
                .await;

            if result.is_err() {
                return Err(result.err().unwrap());
            }
        }

        self.triggers.write().await.remove(&trigger.id);

        Ok(())
    }
}
