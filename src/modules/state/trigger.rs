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
                    return Err(anyhow::anyhow!("Failed to parse trigger config: {}", e));
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
