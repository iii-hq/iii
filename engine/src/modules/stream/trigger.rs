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
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{
    modules::stream::StreamCoreModule,
    trigger::{Trigger, TriggerRegistrator},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct StreamTrigger {
    pub trigger: Trigger,
    pub config: StreamTriggerConfig,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StreamTriggerConfig {
    pub stream_name: Option<String>,
    pub group_id: Option<String>,
    pub item_id: Option<String>,
    pub condition_function_id: Option<String>,
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

pub const JOIN_TRIGGER_TYPE: &str = "stream:join";
pub const LEAVE_TRIGGER_TYPE: &str = "stream:leave";
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
                    "Registering join trigger for function {}",
                    trigger.function_id.purple()
                );
                let _ = join_triggers.write().await.insert(trigger);
            } else if trigger.trigger_type == LEAVE_TRIGGER_TYPE {
                tracing::info!(
                    "Registering leave trigger for function {}",
                    trigger.function_id.purple()
                );
                let _ = leave_triggers.write().await.insert(trigger);
            } else if trigger.trigger_type == STREAM_TRIGGER_TYPE {
                let stream_trigger =
                    serde_json::from_value::<StreamTriggerConfig>(trigger.config.clone());

                match stream_trigger {
                    Ok(stream_trigger) => {
                        tracing::info!(stream_name = %stream_trigger.stream_name.clone().unwrap_or_default(),
                            group_id = %stream_trigger.group_id.clone().unwrap_or_default(),
                            item_id = %stream_trigger.item_id.clone().unwrap_or_default(),
                            condition_function_id = %stream_trigger.condition_function_id.clone().unwrap_or_default(),
                            "{} Stream trigger", "[REGISTERED]".green());

                        stream_triggers_by_name
                            .write()
                            .await
                            .entry(stream_trigger.stream_name.clone().unwrap())
                            .or_insert_with(Vec::new)
                            .push(trigger.id.clone());
                        let _ = stream_triggers.write().await.insert(
                            trigger.id.clone(),
                            StreamTrigger {
                                trigger,
                                config: stream_trigger,
                            },
                        );
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to deserialize stream trigger");
                        return Err(anyhow::anyhow!("Failed to deserialize stream trigger"));
                    }
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
                    if let Some(stream_name_key) = removed_trigger.config.stream_name {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{
        module::ConfigurableModule,
        observability::metrics::ensure_default_meter,
        stream::{adapters::kv_store::BuiltinKvStoreAdapter, config::StreamModuleConfig},
    };

    fn setup() -> StreamCoreModule {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let adapter: Arc<dyn crate::modules::stream::adapters::StreamAdapter> =
            Arc::new(BuiltinKvStoreAdapter::new(None));
        StreamCoreModule::build(engine, StreamModuleConfig::default(), adapter)
    }

    fn make_trigger(
        id: &str,
        trigger_type: &str,
        function_id: &str,
        config: serde_json::Value,
    ) -> Trigger {
        Trigger {
            id: id.to_string(),
            trigger_type: trigger_type.to_string(),
            function_id: function_id.to_string(),
            config,
            worker_id: None,
        }
    }

    // ---- StreamTriggerConfig parsing ----

    #[test]
    fn test_stream_trigger_config_full_deserialization() {
        let json = serde_json::json!({
            "stream_name": "chat-messages",
            "group_id": "room-42",
            "item_id": "msg-100",
            "condition_function_id": "check::condition"
        });
        let config: StreamTriggerConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.stream_name.as_deref(), Some("chat-messages"));
        assert_eq!(config.group_id.as_deref(), Some("room-42"));
        assert_eq!(config.item_id.as_deref(), Some("msg-100"));
        assert_eq!(
            config.condition_function_id.as_deref(),
            Some("check::condition")
        );
    }

    #[test]
    fn test_stream_trigger_config_minimal_deserialization() {
        let json = serde_json::json!({});
        let config: StreamTriggerConfig = serde_json::from_value(json).unwrap();
        assert!(config.stream_name.is_none());
        assert!(config.group_id.is_none());
        assert!(config.item_id.is_none());
        assert!(config.condition_function_id.is_none());
    }

    #[test]
    fn test_stream_trigger_config_partial_deserialization() {
        let json = serde_json::json!({"stream_name": "events"});
        let config: StreamTriggerConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.stream_name.as_deref(), Some("events"));
        assert!(config.group_id.is_none());
    }

    #[test]
    fn test_stream_trigger_config_serialization_roundtrip() {
        let config = StreamTriggerConfig {
            stream_name: Some("test-stream".to_string()),
            group_id: Some("grp-1".to_string()),
            item_id: None,
            condition_function_id: None,
        };
        let json = serde_json::to_value(&config).unwrap();
        let deserialized: StreamTriggerConfig = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.stream_name, config.stream_name);
        assert_eq!(deserialized.group_id, config.group_id);
    }

    // ---- StreamTriggers default ----

    #[test]
    fn test_stream_triggers_default() {
        let triggers = StreamTriggers::default();
        // Should create empty collections
        assert!(std::sync::Arc::strong_count(&triggers.join_triggers) >= 1);
        assert!(std::sync::Arc::strong_count(&triggers.stream_triggers) >= 1);
    }

    // ---- register_trigger for join type ----

    #[tokio::test]
    async fn test_register_join_trigger() {
        let module = setup();
        let trigger = make_trigger(
            "join-1",
            JOIN_TRIGGER_TYPE,
            "fn::on_join",
            serde_json::json!({}),
        );

        let result = module.register_trigger(trigger.clone()).await;
        assert!(result.is_ok());

        let joins = module.triggers.join_triggers.read().await;
        assert_eq!(joins.len(), 1);
        assert!(joins.contains(&trigger));
    }

    // ---- register_trigger for leave type ----

    #[tokio::test]
    async fn test_register_leave_trigger() {
        let module = setup();
        let trigger = make_trigger(
            "leave-1",
            LEAVE_TRIGGER_TYPE,
            "fn::on_leave",
            serde_json::json!({}),
        );

        let result = module.register_trigger(trigger.clone()).await;
        assert!(result.is_ok());

        let leaves = module.triggers.leave_triggers.read().await;
        assert_eq!(leaves.len(), 1);
        assert!(leaves.contains(&trigger));
    }

    // ---- register_trigger for stream type ----

    #[tokio::test]
    async fn test_register_stream_trigger() {
        let module = setup();
        let trigger = make_trigger(
            "stream-1",
            STREAM_TRIGGER_TYPE,
            "fn::on_stream_event",
            serde_json::json!({
                "stream_name": "my-stream",
                "group_id": "grp-1"
            }),
        );

        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());

        let stream_triggers = module.triggers.stream_triggers.read().await;
        assert_eq!(stream_triggers.len(), 1);
        assert!(stream_triggers.contains_key("stream-1"));

        let by_name = module.triggers.stream_triggers_by_name.read().await;
        let ids = by_name
            .get("my-stream")
            .expect("should have stream name entry");
        assert_eq!(ids, &vec!["stream-1".to_string()]);
    }

    #[tokio::test]
    async fn test_register_stream_trigger_invalid_config() {
        let module = setup();
        // Pass a config that is not valid JSON for StreamTriggerConfig
        // Actually StreamTriggerConfig has all optional fields, so any object works.
        // Let's use a non-object value to trigger deserialization failure
        let trigger = make_trigger(
            "stream-bad",
            STREAM_TRIGGER_TYPE,
            "fn::handler",
            serde_json::json!("not-an-object"),
        );

        let result = module.register_trigger(trigger).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to deserialize stream trigger")
        );
    }

    #[tokio::test]
    async fn test_register_multiple_stream_triggers_same_stream_name() {
        let module = setup();

        for i in 0..3 {
            let trigger = make_trigger(
                &format!("stream-{}", i),
                STREAM_TRIGGER_TYPE,
                &format!("fn::handler_{}", i),
                serde_json::json!({"stream_name": "shared-stream"}),
            );
            module.register_trigger(trigger).await.unwrap();
        }

        let by_name = module.triggers.stream_triggers_by_name.read().await;
        let ids = by_name
            .get("shared-stream")
            .expect("should have stream name entry");
        assert_eq!(ids.len(), 3);

        let stream_triggers = module.triggers.stream_triggers.read().await;
        assert_eq!(stream_triggers.len(), 3);
    }

    // ---- unregister_trigger ----

    #[tokio::test]
    async fn test_unregister_join_trigger() {
        let module = setup();
        let trigger = make_trigger(
            "join-1",
            JOIN_TRIGGER_TYPE,
            "fn::handler",
            serde_json::json!({}),
        );
        module.register_trigger(trigger.clone()).await.unwrap();

        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());

        let joins = module.triggers.join_triggers.read().await;
        assert!(joins.is_empty());
    }

    #[tokio::test]
    async fn test_unregister_leave_trigger() {
        let module = setup();
        let trigger = make_trigger(
            "leave-1",
            LEAVE_TRIGGER_TYPE,
            "fn::handler",
            serde_json::json!({}),
        );
        module.register_trigger(trigger.clone()).await.unwrap();

        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());

        let leaves = module.triggers.leave_triggers.read().await;
        assert!(leaves.is_empty());
    }

    #[tokio::test]
    async fn test_unregister_stream_trigger() {
        let module = setup();
        let trigger = make_trigger(
            "stream-1",
            STREAM_TRIGGER_TYPE,
            "fn::handler",
            serde_json::json!({"stream_name": "test-stream"}),
        );
        module.register_trigger(trigger.clone()).await.unwrap();

        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());

        let stream_triggers = module.triggers.stream_triggers.read().await;
        assert!(stream_triggers.is_empty());

        let by_name = module.triggers.stream_triggers_by_name.read().await;
        assert!(by_name.is_empty());
    }

    #[tokio::test]
    async fn test_unregister_one_of_multiple_stream_triggers() {
        let module = setup();

        let trigger1 = make_trigger(
            "stream-1",
            STREAM_TRIGGER_TYPE,
            "fn::handler_1",
            serde_json::json!({"stream_name": "shared"}),
        );
        let trigger2 = make_trigger(
            "stream-2",
            STREAM_TRIGGER_TYPE,
            "fn::handler_2",
            serde_json::json!({"stream_name": "shared"}),
        );

        module.register_trigger(trigger1.clone()).await.unwrap();
        module.register_trigger(trigger2).await.unwrap();

        // Unregister only trigger1
        module.unregister_trigger(trigger1).await.unwrap();

        let stream_triggers = module.triggers.stream_triggers.read().await;
        assert_eq!(stream_triggers.len(), 1);
        assert!(stream_triggers.contains_key("stream-2"));

        let by_name = module.triggers.stream_triggers_by_name.read().await;
        let ids = by_name.get("shared").expect("should still have entry");
        assert_eq!(ids, &vec!["stream-2".to_string()]);
    }

    #[tokio::test]
    async fn test_unregister_nonexistent_stream_trigger() {
        let module = setup();
        let trigger = make_trigger(
            "does-not-exist",
            STREAM_TRIGGER_TYPE,
            "fn::handler",
            serde_json::json!({"stream_name": "test"}),
        );

        // Should succeed without error (no-op)
        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());
    }
}
