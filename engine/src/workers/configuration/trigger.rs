// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `configuration` trigger type.
//!
//! Subscribers bind a function to changes on a specific configuration id by
//! registering a trigger of type `configuration`. The worker walks every
//! matching trigger after each create / update / delete and invokes the
//! handler asynchronously via `engine.call(...)`.
//!
//! ## TTL
//!
//! Each id maintains a ref-count of currently registered triggers. When the
//! ref-count drops to zero AND the worker config has `ttl_seconds > 0`, a
//! `tokio::spawn`-based countdown is scheduled. The countdown deletes the
//! configuration entry (and emits `configuration:deleted`) when it elapses
//! without being interrupted. A new trigger registration before expiry
//! aborts the pending task; `destroy()` aborts every live countdown.

use std::collections::HashMap;
use std::pin::Pin;
use std::time::Duration;

use futures::Future;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use crate::trigger::{Trigger, TriggerRegistrator};
use crate::workers::configuration::ConfigurationWorker;

pub const TRIGGER_TYPE: &str = "configuration";

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ConfigurationTriggerConfig {
    /// Configuration id to watch (exact match). Omit to match every id.
    #[serde(default)]
    pub configuration_id: Option<String>,
    /// Event type filter (e.g. `["configuration:updated"]`). Omit to match all.
    #[serde(default)]
    pub event_types: Option<Vec<String>>,
    /// Optional condition function evaluated on the event before the handler runs.
    #[serde(default)]
    pub condition_function_id: Option<String>,
}

#[derive(Clone)]
pub struct ConfigurationTrigger {
    pub config: ConfigurationTriggerConfig,
    pub trigger: Trigger,
}

/// Per-configuration-id trigger ref-count + pending TTL handle.
struct TriggerSlot {
    triggers: HashMap<String, ConfigurationTrigger>,
    expiry: Option<JoinHandle<()>>,
}

impl TriggerSlot {
    fn new() -> Self {
        Self {
            triggers: HashMap::new(),
            expiry: None,
        }
    }
}

pub struct ConfigurationTriggers {
    /// Triggers without a configuration_id filter — fan out to every event
    /// regardless of id. Stored separately so they don't keep arbitrary
    /// id-specific slots alive.
    global: RwLock<HashMap<String, ConfigurationTrigger>>,
    /// Triggers scoped to a specific configuration_id. The slot also holds
    /// the TTL countdown JoinHandle for that id.
    per_id: RwLock<HashMap<String, TriggerSlot>>,
}

impl Default for ConfigurationTriggers {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurationTriggers {
    pub fn new() -> Self {
        Self {
            global: RwLock::new(HashMap::new()),
            per_id: RwLock::new(HashMap::new()),
        }
    }

    /// Snapshot every trigger that should be evaluated for a given id +
    /// event type. Filtering by event type happens in the worker; this
    /// method collapses the global + per-id buckets so the caller doesn't
    /// have to know about the split.
    pub async fn matching(&self, id: &str) -> Vec<ConfigurationTrigger> {
        let mut out = Vec::new();
        for trigger in self.global.read().await.values() {
            out.push(trigger.clone());
        }
        if let Some(slot) = self.per_id.read().await.get(id) {
            for trigger in slot.triggers.values() {
                out.push(trigger.clone());
            }
        }
        out
    }

    /// Abort every pending TTL countdown. Called by the worker on shutdown.
    pub async fn abort_all_expiries(&self) {
        let mut per_id = self.per_id.write().await;
        for slot in per_id.values_mut() {
            if let Some(handle) = slot.expiry.take() {
                handle.abort();
            }
        }
    }

    /// Test-only — does the per-id slot currently have a pending expiry task?
    #[cfg(test)]
    pub async fn has_pending_expiry(&self, id: &str) -> bool {
        self.per_id
            .read()
            .await
            .get(id)
            .map(|slot| slot.expiry.is_some())
            .unwrap_or(false)
    }

    /// Test-only — total triggers registered for a given id (global triggers
    /// excluded since they're not bound to any specific id).
    #[cfg(test)]
    pub async fn id_trigger_count(&self, id: &str) -> usize {
        self.per_id
            .read()
            .await
            .get(id)
            .map(|slot| slot.triggers.len())
            .unwrap_or(0)
    }
}

#[async_trait::async_trait]
impl TriggerRegistrator for ConfigurationWorker {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        Box::pin(async move {
            let config: ConfigurationTriggerConfig = serde_json::from_value(trigger.config.clone())
                .map_err(|e| {
                    anyhow::anyhow!("Failed to parse configuration trigger config: {}", e)
                })?;

            let trigger_id = trigger.id.clone();
            let stored = ConfigurationTrigger {
                config: config.clone(),
                trigger,
            };

            tracing::info!(
                trigger_id = %trigger_id,
                configuration_id = ?config.configuration_id,
                event_types = ?config.event_types,
                "Registering configuration trigger"
            );

            match config.configuration_id.as_ref() {
                None => {
                    self.triggers
                        .global
                        .write()
                        .await
                        .insert(trigger_id, stored);
                }
                Some(cfg_id) => {
                    let mut per_id = self.triggers.per_id.write().await;
                    let slot = per_id
                        .entry(cfg_id.clone())
                        .or_insert_with(TriggerSlot::new);
                    if let Some(handle) = slot.expiry.take() {
                        handle.abort();
                        tracing::debug!(
                            configuration_id = %cfg_id,
                            "Cancelled pending TTL expiry due to new trigger registration"
                        );
                    }
                    slot.triggers.insert(trigger_id, stored);
                }
            }
            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        Box::pin(async move {
            let trigger_id = trigger.id.clone();
            let parsed: ConfigurationTriggerConfig =
                serde_json::from_value(trigger.config.clone()).unwrap_or_default();

            match parsed.configuration_id.as_ref() {
                None => {
                    self.triggers.global.write().await.remove(&trigger_id);
                }
                Some(cfg_id) => {
                    let cfg_id = cfg_id.clone();
                    let now_empty = {
                        let mut per_id = self.triggers.per_id.write().await;
                        if let Some(slot) = per_id.get_mut(&cfg_id) {
                            slot.triggers.remove(&trigger_id);
                            slot.triggers.is_empty()
                        } else {
                            false
                        }
                    };

                    if now_empty && self.ttl_seconds > 0 {
                        self.schedule_ttl_expiry(cfg_id).await;
                    }
                }
            }
            Ok(())
        })
    }
}

impl ConfigurationWorker {
    /// Spawn a TTL countdown for `cfg_id`. Stores the JoinHandle on the
    /// per-id slot so a subsequent registration (or `destroy()`) can abort it.
    ///
    /// Re-checks the slot's trigger count under the write lock before
    /// installing the handle: `unregister_trigger` releases its lock between
    /// "slot is empty" and this call, so a concurrent `register_trigger` may
    /// have repopulated the slot in that window. Bail out in that case so the
    /// TTL countdown isn't scheduled against a slot that is no longer empty.
    async fn schedule_ttl_expiry(&self, cfg_id: String) {
        let ttl = Duration::from_secs(self.ttl_seconds);
        let worker = self.clone();
        let cfg_id_for_task = cfg_id.clone();

        let mut per_id = self.triggers.per_id.write().await;
        let slot = per_id.entry(cfg_id).or_insert_with(TriggerSlot::new);
        if !slot.triggers.is_empty() {
            tracing::debug!(
                configuration_id = %cfg_id_for_task,
                "Skipping TTL countdown — a concurrent registration repopulated the slot"
            );
            return;
        }

        let handle = tokio::spawn(async move {
            tokio::time::sleep(ttl).await;
            tracing::info!(
                configuration_id = %cfg_id_for_task,
                ttl_seconds = worker.ttl_seconds,
                "TTL expired with no triggers — cleaning up configuration"
            );
            if let Err(e) = worker.expire_configuration(&cfg_id_for_task).await {
                tracing::error!(
                    configuration_id = %cfg_id_for_task,
                    error = %e,
                    "Failed to expire configuration"
                );
            }
        });

        // Replace any leftover handle (race between rapid unregister-register
        // cycles) — the previous handle was already aborted by register.
        if let Some(prev) = slot.expiry.replace(handle) {
            prev.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn config_defaults_match_optional_filters() {
        let cfg: ConfigurationTriggerConfig = serde_json::from_value(json!({})).unwrap();
        assert!(cfg.configuration_id.is_none());
        assert!(cfg.event_types.is_none());
        assert!(cfg.condition_function_id.is_none());
    }

    #[test]
    fn config_with_explicit_filters() {
        let cfg: ConfigurationTriggerConfig = serde_json::from_value(json!({
            "configuration_id": "iii-stream",
            "event_types": ["configuration:updated"],
        }))
        .unwrap();
        assert_eq!(cfg.configuration_id.as_deref(), Some("iii-stream"));
        assert_eq!(
            cfg.event_types.as_deref(),
            Some(&["configuration:updated".to_string()][..])
        );
    }

    #[tokio::test]
    async fn register_then_unregister_drains_per_id_slot() {
        use crate::workers::configuration::{
            ConfigurationWorker, adapters::ConfigurationAdapter, adapters::fs::FsAdapter,
        };
        crate::workers::observability::metrics::ensure_default_meter();

        let dir = tempfile::tempdir().unwrap();
        let adapter = Arc::new(
            FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
                .await
                .unwrap(),
        ) as Arc<dyn ConfigurationAdapter>;
        let engine = Arc::new(crate::engine::Engine::new());
        let worker = ConfigurationWorker::for_test(engine, adapter, 0);

        let trigger = Trigger {
            id: "t-1".into(),
            trigger_type: TRIGGER_TYPE.into(),
            function_id: "h::handler".into(),
            config: json!({ "configuration_id": "iii-stream" }),
            worker_id: None,
            metadata: None,
        };
        worker.register_trigger(trigger.clone()).await.unwrap();
        assert_eq!(worker.triggers.id_trigger_count("iii-stream").await, 1);

        worker.unregister_trigger(trigger).await.unwrap();
        assert_eq!(worker.triggers.id_trigger_count("iii-stream").await, 0);
        // ttl_seconds=0 means no countdown is scheduled.
        assert!(!worker.triggers.has_pending_expiry("iii-stream").await);
    }

    #[tokio::test]
    async fn ttl_countdown_fires_when_last_trigger_unregistered() {
        use crate::workers::configuration::{
            ConfigurationWorker, adapters::ConfigurationAdapter, adapters::fs::FsAdapter,
            structs::ConfigurationRegisterInput,
        };
        crate::workers::observability::metrics::ensure_default_meter();

        let dir = tempfile::tempdir().unwrap();
        let adapter = Arc::new(
            FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
                .await
                .unwrap(),
        ) as Arc<dyn ConfigurationAdapter>;
        let engine = Arc::new(crate::engine::Engine::new());
        // 1-second TTL keeps the test fast but still exercises the real
        // tokio::time::sleep path the production code uses.
        let worker = ConfigurationWorker::for_test(engine, adapter, 1);

        worker
            .register_fn(ConfigurationRegisterInput {
                id: "iii-stream".into(),
                name: "Stream".into(),
                description: "...".into(),
                schema: json!({ "type": "object" }),
                initial_value: Some(json!({})),
                metadata: None,
            })
            .await;
        assert!(worker.store.get("iii-stream").await.is_some());

        let trigger = Trigger {
            id: "t-ttl".into(),
            trigger_type: TRIGGER_TYPE.into(),
            function_id: "h::handler".into(),
            config: json!({ "configuration_id": "iii-stream" }),
            worker_id: None,
            metadata: None,
        };
        worker.register_trigger(trigger.clone()).await.unwrap();
        worker.unregister_trigger(trigger).await.unwrap();

        // Pending expiry handle exists immediately after the unregister.
        assert!(worker.triggers.has_pending_expiry("iii-stream").await);

        // Wait long enough for the real-time TTL to elapse and the cleanup
        // task to run. Loop with a short interval so we don't oversleep.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if worker.store.get("iii-stream").await.is_none() {
                break;
            }
            if std::time::Instant::now() >= deadline {
                panic!("configuration should be cleaned up after TTL elapses");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    #[tokio::test]
    async fn ttl_countdown_aborts_on_re_register() {
        use crate::workers::configuration::{
            ConfigurationWorker, adapters::ConfigurationAdapter, adapters::fs::FsAdapter,
            structs::ConfigurationRegisterInput,
        };
        crate::workers::observability::metrics::ensure_default_meter();

        let dir = tempfile::tempdir().unwrap();
        let adapter = Arc::new(
            FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
                .await
                .unwrap(),
        ) as Arc<dyn ConfigurationAdapter>;
        let engine = Arc::new(crate::engine::Engine::new());
        let worker = ConfigurationWorker::for_test(engine, adapter, 1);

        worker
            .register_fn(ConfigurationRegisterInput {
                id: "iii-stream".into(),
                name: "Stream".into(),
                description: "...".into(),
                schema: json!({ "type": "object" }),
                initial_value: Some(json!({})),
                metadata: None,
            })
            .await;

        let trigger_a = Trigger {
            id: "t-a".into(),
            trigger_type: TRIGGER_TYPE.into(),
            function_id: "h::a".into(),
            config: json!({ "configuration_id": "iii-stream" }),
            worker_id: None,
            metadata: None,
        };
        worker.register_trigger(trigger_a.clone()).await.unwrap();
        worker.unregister_trigger(trigger_a).await.unwrap();
        assert!(worker.triggers.has_pending_expiry("iii-stream").await);

        // New trigger arrives BEFORE TTL elapses — countdown should abort.
        let trigger_b = Trigger {
            id: "t-b".into(),
            trigger_type: TRIGGER_TYPE.into(),
            function_id: "h::b".into(),
            config: json!({ "configuration_id": "iii-stream" }),
            worker_id: None,
            metadata: None,
        };
        worker.register_trigger(trigger_b).await.unwrap();
        assert!(!worker.triggers.has_pending_expiry("iii-stream").await);

        // Wait > TTL of real time so any leftover countdown would have fired.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        assert!(
            worker.store.get("iii-stream").await.is_some(),
            "configuration should survive when a new trigger lands before TTL elapses"
        );
    }

    #[tokio::test]
    async fn matching_collapses_global_and_per_id_buckets() {
        let triggers = ConfigurationTriggers::new();
        triggers.global.write().await.insert(
            "g-1".into(),
            ConfigurationTrigger {
                config: ConfigurationTriggerConfig::default(),
                trigger: Trigger {
                    id: "g-1".into(),
                    trigger_type: TRIGGER_TYPE.into(),
                    function_id: "f::a".into(),
                    config: json!({}),
                    worker_id: None,
                    metadata: None,
                },
            },
        );

        let mut slot = TriggerSlot::new();
        slot.triggers.insert(
            "p-1".into(),
            ConfigurationTrigger {
                config: ConfigurationTriggerConfig {
                    configuration_id: Some("iii-stream".into()),
                    ..Default::default()
                },
                trigger: Trigger {
                    id: "p-1".into(),
                    trigger_type: TRIGGER_TYPE.into(),
                    function_id: "f::b".into(),
                    config: json!({ "configuration_id": "iii-stream" }),
                    worker_id: None,
                    metadata: None,
                },
            },
        );
        triggers
            .per_id
            .write()
            .await
            .insert("iii-stream".into(), slot);

        let matched = triggers.matching("iii-stream").await;
        assert_eq!(matched.len(), 2);

        let unmatched = triggers.matching("other").await;
        assert_eq!(unmatched.len(), 1);
        assert_eq!(unmatched[0].trigger.id, "g-1");
    }
}
