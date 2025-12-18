use std::{collections::HashSet, pin::Pin, sync::Arc};

use colored::Colorize;
use futures::Future;
use tokio::sync::RwLock;

use crate::{
    modules::streams::StreamCoreModule,
    trigger::{Trigger, TriggerRegistrator},
};

pub struct StreamTriggers {
    pub join_triggers: Arc<RwLock<HashSet<Trigger>>>,
    pub leave_triggers: Arc<RwLock<HashSet<Trigger>>>,
}

impl StreamTriggers {
    pub fn new() -> Self {
        Self {
            join_triggers: Arc::new(RwLock::new(HashSet::new())),
            leave_triggers: Arc::new(RwLock::new(HashSet::new())),
        }
    }
}

pub const JOIN_TRIGGER_TYPE: &str = "streams:join";
pub const LEAVE_TRIGGER_TYPE: &str = "streams:leave";

#[async_trait::async_trait]
impl TriggerRegistrator for StreamCoreModule {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let join_triggers = &self.triggers.join_triggers;
        let leave_triggers = &self.triggers.leave_triggers;

        Box::pin(async move {
            if trigger.trigger_type == JOIN_TRIGGER_TYPE {
                tracing::info!(
                    "Registering join trigger for function path {}",
                    trigger.function_path.purple()
                );
                let _ = join_triggers.write().await.insert(trigger);
            } else if trigger.trigger_type == LEAVE_TRIGGER_TYPE {
                tracing::info!(
                    "Registering leave trigger for function path {}",
                    trigger.function_path.purple()
                );
                let _ = leave_triggers.write().await.insert(trigger);
            }

            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let join_triggers = &self.triggers.join_triggers;
        let leave_triggers = &self.triggers.leave_triggers;

        Box::pin(async move {
            if trigger.trigger_type == JOIN_TRIGGER_TYPE {
                let _ = join_triggers.write().await.remove(&trigger);
            }
            if trigger.trigger_type == LEAVE_TRIGGER_TYPE {
                let _ = leave_triggers.write().await.remove(&trigger);
            }

            Ok(())
        })
    }
}
