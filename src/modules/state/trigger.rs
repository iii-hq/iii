// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, pin::Pin, sync::Arc};

use colored::Colorize;
use futures::Future;
use tokio::sync::RwLock;

use crate::{
    modules::state::StateCoreModule,
    trigger::{Trigger, TriggerRegistrator},
};

pub struct StateTriggers {
    pub list: Arc<RwLock<HashMap<String, Trigger>>>,
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
            tracing::info!(
                "Registering trigger for function path {}",
                trigger.function_path.purple()
            );
            let _ = triggers.write().await.insert(trigger_id, trigger);

            Ok(())
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
