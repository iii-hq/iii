// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    sync::Arc,
};

use colored::Colorize;
use futures::Future;
use tokio::sync::RwLock;

use crate::{
    modules::streams::StreamCoreModule,
    trigger::{Trigger, TriggerRegistrator},
};

#[derive(Clone)]
pub struct StreamTrigger {
    pub trigger: Trigger,
    pub stream_name: Option<String>,
    pub condition_function_path: Option<String>,
}

pub struct StreamTriggers {
    pub join_triggers: Arc<RwLock<HashSet<Trigger>>>,
    pub leave_triggers: Arc<RwLock<HashSet<Trigger>>>,
    // Map from trigger_id to StreamTrigger for unregistration
    pub stream_triggers: Arc<RwLock<HashMap<String, StreamTrigger>>>,
    // Map from stream_name to list of trigger_ids for efficient lookup
    pub stream_triggers_by_name: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl Default for StreamTriggers {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamTriggers {
    pub fn new() -> Self {
        Self {
            join_triggers: Arc::new(RwLock::new(HashSet::new())),
            leave_triggers: Arc::new(RwLock::new(HashSet::new())),
            stream_triggers: Arc::new(RwLock::new(HashMap::new())),
            stream_triggers_by_name: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

pub const JOIN_TRIGGER_TYPE: &str = "streams:join";
pub const LEAVE_TRIGGER_TYPE: &str = "streams:leave";
pub const STREAM_TRIGGER_TYPE: &str = "stream";

#[async_trait::async_trait]
impl TriggerRegistrator for StreamCoreModule {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let join_triggers = &self.triggers.join_triggers;
        let leave_triggers = &self.triggers.leave_triggers;
        let stream_triggers = &self.triggers.stream_triggers;
        let stream_triggers_by_name = &self.triggers.stream_triggers_by_name;

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
            } else if trigger.trigger_type == STREAM_TRIGGER_TYPE {
                // Only register triggers that have stream_name in their config
                if let Some(stream_name_value) = trigger.config.get("stream_name") {
                    if let Some(stream_name_str) = stream_name_value.as_str() {
                        let trigger_id = trigger.id.clone();
                        let stream_name = stream_name_str.to_string();

                        tracing::info!(
                            "Registering stream trigger for function path {} with stream_name {}",
                            trigger.function_path.purple(),
                            stream_name.purple()
                        );

                        let condition_function_path = trigger
                            .config
                            .get("condition_function_path")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        let stream_trigger = StreamTrigger {
                            trigger,
                            stream_name: Some(stream_name.clone()),
                            condition_function_path,
                        };

                        // Store trigger by ID for unregistration
                        let _ = stream_triggers
                            .write()
                            .await
                            .insert(trigger_id.clone(), stream_trigger);

                        // Organize triggers by stream_name for efficient lookup during invocation
                        let mut by_name = stream_triggers_by_name.write().await;
                        by_name
                            .entry(stream_name)
                            .or_insert_with(Vec::new)
                            .push(trigger_id);
                    } else {
                        tracing::warn!(
                            "Stream trigger for function path {} has invalid stream_name (not a string), skipping registration",
                            trigger.function_path.purple()
                        );
                    }
                } else {
                    tracing::warn!(
                        "Stream trigger for function path {} missing stream_name in config, skipping registration",
                        trigger.function_path.purple()
                    );
                }
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
        let stream_triggers = &self.triggers.stream_triggers;
        let stream_triggers_by_name = &self.triggers.stream_triggers_by_name;

        Box::pin(async move {
            if trigger.trigger_type == JOIN_TRIGGER_TYPE {
                let _ = join_triggers.write().await.remove(&trigger);
            }
            if trigger.trigger_type == LEAVE_TRIGGER_TYPE {
                let _ = leave_triggers.write().await.remove(&trigger);
            }
            if trigger.trigger_type == STREAM_TRIGGER_TYPE {
                let trigger_id = trigger.id.clone();

                // Remove from main triggers map
                if let Some(removed_trigger) = stream_triggers.write().await.remove(&trigger_id) {
                    // Remove from stream_name index
                    if let Some(stream_name_key) = removed_trigger.stream_name {
                        let mut by_name = stream_triggers_by_name.write().await;
                        if let Some(trigger_ids) = by_name.get_mut(&stream_name_key) {
                            trigger_ids.retain(|id| id != &trigger_id);
                            if trigger_ids.is_empty() {
                                by_name.remove(&stream_name_key);
                            }
                        }
                    }
                }
            }

            Ok(())
        })
    }
}
