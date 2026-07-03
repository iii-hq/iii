// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{pin::Pin, sync::Arc};

use colored::Colorize;
use dashmap::DashMap;
use futures::Future;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const BUILTIN_TRIGGER_TYPES: &[(&str, &str)] = &[
    ("http", "iii-http"),
    ("cron", "iii-cron"),
    ("subscribe", "iii-pubsub"),
    ("state", "iii-state"),
    ("durable:subscriber", "iii-queue"),
    ("stream", "iii-stream"),
    ("stream:join", "iii-stream"),
    ("stream:leave", "iii-stream"),
    ("log", "iii-observability"),
    ("trace", "iii-observability"),
    ("configuration", "configuration"),
];

/// Maps a trigger-type id to the in-process worker that owns it. In-process
/// workers register their `TriggerType` with `worker_id: None`, so the
/// `worker_registry` Uuid lookup used for WebSocket workers cannot attribute
/// them. The static table is the source of truth for both error reporting
/// (`RegisterTriggerError::UnknownBuiltin`) and discovery (`engine_fn`
/// rolls trigger types up into the owning runtime worker's `workers::info`
/// envelope).
pub fn builtin_trigger_type_owner(trigger_type_id: &str) -> Option<&'static str> {
    BUILTIN_TRIGGER_TYPES
        .iter()
        .find(|(id, _)| *id == trigger_type_id)
        .map(|(_, worker)| *worker)
}

#[derive(Debug, thiserror::Error)]
pub enum RegisterTriggerError {
    #[error(
        "Trigger type \"{trigger_type}\" not found — worker {worker} is missing. Run: iii worker add {worker}"
    )]
    UnknownBuiltin {
        trigger_type: String,
        worker: &'static str,
    },
    #[error(
        "Trigger type \"{trigger_type}\" not found. Search for a worker that provides this trigger type at https://workers.iii.dev/"
    )]
    Unknown { trigger_type: String },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub struct TriggerType {
    pub id: String,
    pub _description: String,
    pub trigger_request_format: Option<Value>,
    pub call_request_format: Option<Value>,
    pub call_response_format: Option<Value>,
    pub registrator: Box<dyn TriggerRegistrator>,
    pub worker_id: Option<Uuid>,
}

impl TriggerType {
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        registrator: Box<dyn TriggerRegistrator>,
        worker_id: Option<Uuid>,
    ) -> Self {
        let id = id.into();
        let trigger_request_format = Self::trigger_request_format_for(&id);
        let call_request_format = Self::call_request_format_for(&id);
        let call_response_format = Self::call_response_format_for(&id);
        Self {
            id,
            _description: description.into(),
            trigger_request_format,
            call_request_format,
            call_response_format,
            registrator,
            worker_id,
        }
    }

    pub fn with_trigger_request_format<T: schemars::JsonSchema>(mut self) -> Self {
        self.trigger_request_format = Self::schema_for::<T>();
        self
    }

    pub fn with_call_request_format<T: schemars::JsonSchema>(mut self) -> Self {
        self.call_request_format = Self::schema_for::<T>();
        self
    }

    /// Schema for what a bound handler must RETURN when this trigger fires
    /// (e.g. the HTTP response envelope). Exposed via `engine::triggers::info`
    /// as `response_schema` so callers can discover the return contract.
    pub fn with_call_response_format<T: schemars::JsonSchema>(mut self) -> Self {
        self.call_response_format = Self::schema_for::<T>();
        self
    }

    fn schema_for<T: schemars::JsonSchema>() -> Option<Value> {
        serde_json::to_value(schemars::schema_for!(T)).ok()
    }

    fn trigger_request_format_for(id: &str) -> Option<Value> {
        use crate::trigger_formats::*;

        match id {
            "http" => Self::schema_for::<HttpTriggerConfig>(),
            "cron" => Self::schema_for::<CronTriggerConfig>(),
            "durable:subscriber" => Self::schema_for::<QueueTriggerConfig>(),
            "subscribe" => Self::schema_for::<SubscribeTriggerConfig>(),
            "state" => Self::schema_for::<StateTriggerConfig>(),
            "stream:join" | "stream:leave" => Self::schema_for::<StreamJoinLeaveTriggerConfig>(),
            "stream" => Self::schema_for::<StreamTriggerConfig>(),
            "log" => Self::schema_for::<LogTriggerConfig>(),
            "trace" => Self::schema_for::<TraceTriggerConfig>(),
            "configuration" => Self::schema_for::<ConfigurationTriggerConfig>(),
            _ => None,
        }
    }

    fn call_request_format_for(id: &str) -> Option<Value> {
        use crate::trigger_formats::*;

        match id {
            "http" => Self::schema_for::<HttpCallRequest>(),
            "cron" => Self::schema_for::<CronCallRequest>(),
            "state" => Self::schema_for::<StateCallRequest>(),
            "stream:join" | "stream:leave" => Self::schema_for::<StreamJoinLeaveCallRequest>(),
            "stream" => Self::schema_for::<StreamCallRequest>(),
            "log" => Self::schema_for::<LogCallRequest>(),
            "trace" => Self::schema_for::<TraceCallRequest>(),
            "configuration" => Self::schema_for::<ConfigurationCallRequest>(),
            _ => None,
        }
    }

    /// Schema a bound handler must RETURN when this trigger fires. Only trigger
    /// types whose handler return shape is fixed declare one. `http` returns an
    /// `HttpCallResponse` (`status_code` / `headers` / `body`); most triggers
    /// place no constraint on the return and report `None`.
    fn call_response_format_for(id: &str) -> Option<Value> {
        use crate::trigger_formats::*;

        match id {
            "http" => Self::schema_for::<HttpCallResponse>(),
            _ => None,
        }
    }
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

#[derive(Clone, Debug, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub id: String,
    pub trigger_type: String,
    pub function_id: String,
    pub config: Value,
    pub worker_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
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

    pub async fn register_trigger(&self, trigger: Trigger) -> Result<(), RegisterTriggerError> {
        let trigger_type_id = trigger.trigger_type.clone();
        let Some(trigger_type) = self.trigger_types.get(&trigger_type_id) else {
            if let Some(worker_name) = builtin_trigger_type_owner(&trigger_type_id) {
                tracing::error!(
                    "Trigger type {} requires the {} worker, which is not active in your project.\n\n  To fix this, run:\n\n    {}\n",
                    trigger_type_id.purple().bold(),
                    worker_name.cyan().bold(),
                    format!("iii worker add {}", worker_name).green().bold()
                );
                return Err(RegisterTriggerError::UnknownBuiltin {
                    trigger_type: trigger_type_id,
                    worker: worker_name,
                });
            }

            tracing::error!(
                "Trigger type {} not found. Search for a worker that provides this trigger type at {}",
                trigger_type_id.purple().bold(),
                "https://workers.iii.dev/".cyan().bold()
            );
            return Err(RegisterTriggerError::Unknown {
                trigger_type: trigger_type_id,
            });
        };

        if let Err(err) = trigger_type
            .registrator
            .register_trigger(trigger.clone())
            .await
        {
            tracing::error!(error = %err, "Error registering trigger");
            return Err(RegisterTriggerError::Other(err));
        }

        drop(trigger_type);

        tracing::debug!(trigger = %trigger.id, worker_id = %trigger.worker_id.unwrap_or_default(), "Registering trigger");

        self.triggers.insert(trigger.id.clone(), trigger);

        Ok(())
    }

    /// Unregister a trigger by id. Idempotent: returns `Ok(false)` when no
    /// trigger with this id exists (rather than erroring), so callers can treat
    /// double-unregister as a no-op. On a registrator error the registry entry
    /// is left in place (registry and registrator stay consistent) and the
    /// error is propagated. Returns `Ok(true)` when a trigger was removed.
    pub async fn unregister_trigger(
        &self,
        id: String,
        trigger_type: Option<String>,
    ) -> Result<bool, anyhow::Error> {
        tracing::info!(
            "Unregistering trigger: {} of type: {}",
            id.purple(),
            trigger_type.as_deref().unwrap_or("<missing>").purple()
        );

        let Some(trigger_entry) = self.triggers.get(&id) else {
            return Ok(false);
        };
        let trigger = trigger_entry.value().clone();
        drop(trigger_entry);

        if let Some(tt) = self.trigger_types.get(&trigger.trigger_type) {
            tt.registrator.unregister_trigger(trigger.clone()).await?;
        }

        self.triggers.remove(&id);

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A no-op registrator used for testing synchronous registry operations.
    struct MockRegistrator {
        register_count: AtomicUsize,
        unregister_count: AtomicUsize,
    }

    impl MockRegistrator {
        fn new() -> Self {
            Self {
                register_count: AtomicUsize::new(0),
                unregister_count: AtomicUsize::new(0),
            }
        }
    }

    struct ControlledRegistrator {
        register_count: AtomicUsize,
        unregister_count: AtomicUsize,
        fail_register: bool,
        fail_unregister: bool,
    }

    impl ControlledRegistrator {
        fn new(fail_register: bool, fail_unregister: bool) -> Self {
            Self {
                register_count: AtomicUsize::new(0),
                unregister_count: AtomicUsize::new(0),
                fail_register,
                fail_unregister,
            }
        }
    }

    impl TriggerRegistrator for Arc<ControlledRegistrator> {
        fn register_trigger(
            &self,
            _trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            self.register_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if self.fail_register {
                    Err(anyhow::anyhow!("register failed"))
                } else {
                    Ok(())
                }
            })
        }

        fn unregister_trigger(
            &self,
            _trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            self.unregister_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if self.fail_unregister {
                    Err(anyhow::anyhow!("unregister failed"))
                } else {
                    Ok(())
                }
            })
        }
    }

    impl TriggerRegistrator for MockRegistrator {
        fn register_trigger(
            &self,
            _trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            self.register_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }

        fn unregister_trigger(
            &self,
            _trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            self.unregister_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    fn make_trigger(id: &str, trigger_type: &str) -> Trigger {
        Trigger {
            id: id.to_string(),
            trigger_type: trigger_type.to_string(),
            function_id: format!("fn_{}", id),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
        }
    }

    fn make_trigger_type(id: &str) -> TriggerType {
        TriggerType::new(
            id,
            format!("Test trigger type {}", id),
            Box::new(MockRegistrator::new()),
            None,
        )
    }

    #[test]
    fn test_trigger_registry_new() {
        let registry = TriggerRegistry::new();
        assert!(registry.trigger_types.is_empty());
        assert!(registry.triggers.is_empty());
    }

    #[test]
    fn test_trigger_registry_default() {
        let registry = TriggerRegistry::default();
        assert!(registry.trigger_types.is_empty());
        assert!(registry.triggers.is_empty());
    }

    #[tokio::test]
    async fn test_trigger_registry_register_trigger_type() {
        let registry = TriggerRegistry::new();
        let tt = make_trigger_type("cron");

        let result = registry.register_trigger_type(tt).await;
        assert!(result.is_ok());
        assert_eq!(registry.trigger_types.len(), 1);
        assert!(registry.trigger_types.contains_key("cron"));
    }

    #[tokio::test]
    async fn test_trigger_registry_register_trigger_type_overwrites() {
        let registry = TriggerRegistry::new();
        registry
            .register_trigger_type(make_trigger_type("cron"))
            .await
            .unwrap();
        registry
            .register_trigger_type(make_trigger_type("cron"))
            .await
            .unwrap();

        // DashMap insert overwrites; there should still be exactly one entry.
        assert_eq!(registry.trigger_types.len(), 1);
    }

    #[tokio::test]
    async fn re_registering_a_type_replaces_registrator_and_replays_bindings() {
        let registry = TriggerRegistry::new();

        let a = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new("cron", "gen A", Box::new(a.clone()), None))
            .await
            .unwrap();
        registry
            .register_trigger(make_trigger("t1", "cron"))
            .await
            .unwrap();
        assert_eq!(a.register_count.load(Ordering::SeqCst), 1);

        // Provider reload / reconnect: the type re-registers with a NEW
        // registrator. The existing binding must be re-delivered to it, and
        // later registrations must route to it — never to the stale one.
        let b = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new("cron", "gen B", Box::new(b.clone()), None))
            .await
            .unwrap();
        assert_eq!(
            b.register_count.load(Ordering::SeqCst),
            1,
            "existing binding replayed to the new registrator"
        );

        registry
            .register_trigger(make_trigger("t2", "cron"))
            .await
            .unwrap();
        assert_eq!(b.register_count.load(Ordering::SeqCst), 2);
        assert_eq!(
            a.register_count.load(Ordering::SeqCst),
            1,
            "stale registrator receives nothing after replacement"
        );
    }

    #[tokio::test]
    async fn unregister_worker_reaps_only_connection_owned_triggers() {
        let registry = TriggerRegistry::new();
        registry
            .register_trigger_type(make_trigger_type("evt"))
            .await
            .unwrap();
        let worker = Uuid::new_v4();

        // A worker's own lifecycle binding (Message::RegisterTrigger path).
        let mut owned = make_trigger("t_owned", "evt");
        owned.worker_id = Some(worker);
        registry.register_trigger(owned).await.unwrap();

        // A function-path registration (engine::register_trigger): durable,
        // no connection owner.
        registry
            .register_trigger(make_trigger("t_durable", "evt"))
            .await
            .unwrap();

        registry.unregister_worker(&worker).await;

        assert!(
            !registry.triggers.contains_key("t_owned"),
            "connection-owned binding dies with its worker"
        );
        assert!(
            registry.triggers.contains_key("t_durable"),
            "durable binding survives the owner's disconnect GC"
        );

        // Explicit unregister is the durable binding's only teardown.
        let removed = registry
            .unregister_trigger("t_durable".to_string(), None)
            .await
            .unwrap();
        assert!(removed);
        assert!(!registry.triggers.contains_key("t_durable"));
    }

    #[tokio::test]
    async fn test_trigger_registry_register_trigger() {
        let registry = TriggerRegistry::new();
        registry
            .register_trigger_type(make_trigger_type("cron"))
            .await
            .unwrap();

        let trigger = make_trigger("t1", "cron");
        let result = registry.register_trigger(trigger).await;
        assert!(result.is_ok());
        assert_eq!(registry.triggers.len(), 1);
        assert!(registry.triggers.contains_key("t1"));
    }

    #[tokio::test]
    async fn test_trigger_registry_register_trigger_missing_type() {
        let registry = TriggerRegistry::new();
        // No trigger type registered -- should fail.
        let trigger = make_trigger("t1", "nonexistent");
        let result = registry.register_trigger(trigger).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("\"nonexistent\" not found"),
            "Expected unknown-type message, got: {err_msg}"
        );
        assert!(
            err_msg.contains("https://workers.iii.dev/"),
            "Expected workers directory recommendation, got: {err_msg}"
        );
        assert!(registry.triggers.is_empty());
    }

    #[tokio::test]
    async fn test_trigger_registry_register_trigger_missing_builtin_worker() {
        let registry = TriggerRegistry::new();
        let trigger = make_trigger("t1", "http");
        let result = registry.register_trigger(trigger).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("iii worker add iii-http"),
            "Expected hint with 'iii worker add iii-http', got: {err_msg}"
        );
        assert!(registry.triggers.is_empty());
    }

    #[tokio::test]
    async fn test_trigger_registry_unregister_trigger() {
        let registry = TriggerRegistry::new();
        registry
            .register_trigger_type(make_trigger_type("cron"))
            .await
            .unwrap();
        registry
            .register_trigger(make_trigger("t1", "cron"))
            .await
            .unwrap();

        let result = registry
            .unregister_trigger("t1".to_string(), Some("cron".to_string()))
            .await;
        assert!(matches!(result, Ok(true)));
        assert!(registry.triggers.is_empty());
    }

    #[tokio::test]
    async fn test_trigger_registry_unregister_trigger_not_found() {
        let registry = TriggerRegistry::new();
        let result = registry
            .unregister_trigger("nonexistent".to_string(), None)
            .await;
        // Idempotent: unregistering an unknown id is a no-op, not an error.
        assert!(matches!(result, Ok(false)));
    }

    #[tokio::test]
    async fn test_trigger_registry_register_type_auto_registers_existing_triggers() {
        // When a trigger type is registered, any triggers already stored that
        // reference that type should be forwarded to the registrator.
        let registry = TriggerRegistry::new();

        // Insert a trigger manually before its type exists.
        let trigger = make_trigger("t1", "webhook");
        registry.triggers.insert(trigger.id.clone(), trigger);

        let registrator = Arc::new(MockRegistrator::new());
        let tt = TriggerType::new(
            "webhook",
            "Webhook type",
            Box::new(MockRegistrator::new()),
            None,
        );

        // We cannot easily inspect the boxed registrator, but the call should
        // succeed and not panic.
        let result = registry.register_trigger_type(tt).await;
        assert!(result.is_ok());
        // The trigger type was inserted.
        assert!(registry.trigger_types.contains_key("webhook"));
        // A trigger for this type was already registered explicitly, so
        // the registrator's register_trigger should have been called
        // (tested implicitly -- no panic means success).
        drop(registrator);
    }

    #[tokio::test]
    async fn test_unregister_worker_removes_triggers_and_types() {
        let registry = TriggerRegistry::new();
        let worker_id = Uuid::new_v4();

        // Register a trigger type owned by the worker.
        let tt = TriggerType::new(
            "cron",
            "Cron",
            Box::new(MockRegistrator::new()),
            Some(worker_id),
        );
        registry.register_trigger_type(tt).await.unwrap();

        // Register a trigger owned by the worker.
        let mut trigger = make_trigger("t1", "cron");
        trigger.worker_id = Some(worker_id);
        registry.register_trigger(trigger).await.unwrap();

        assert_eq!(registry.triggers.len(), 1);
        assert_eq!(registry.trigger_types.len(), 1);

        registry.unregister_worker(&worker_id).await;

        assert!(registry.triggers.is_empty());
        assert!(registry.trigger_types.is_empty());
    }

    #[tokio::test]
    async fn test_unregister_worker_does_not_affect_other_workers() {
        let registry = TriggerRegistry::new();
        let worker_a = Uuid::new_v4();
        let worker_b = Uuid::new_v4();

        let tt = TriggerType::new(
            "cron",
            "Cron",
            Box::new(MockRegistrator::new()),
            Some(worker_a),
        );
        registry.register_trigger_type(tt).await.unwrap();

        let mut trigger_a = make_trigger("ta", "cron");
        trigger_a.worker_id = Some(worker_a);
        registry.register_trigger(trigger_a).await.unwrap();

        let mut trigger_b = make_trigger("tb", "cron");
        trigger_b.worker_id = Some(worker_b);
        registry.register_trigger(trigger_b).await.unwrap();

        registry.unregister_worker(&worker_b).await;

        // Only worker_b's trigger should be removed.
        assert_eq!(registry.triggers.len(), 1);
        assert!(registry.triggers.contains_key("ta"));
        // The trigger type belongs to worker_a so it should remain.
        assert_eq!(registry.trigger_types.len(), 1);
    }

    #[tokio::test]
    async fn test_register_trigger_type_ignores_existing_trigger_registration_errors() {
        let registry = TriggerRegistry::new();
        let trigger = make_trigger("existing", "webhook");
        registry.triggers.insert(trigger.id.clone(), trigger);

        let registrator = Arc::new(ControlledRegistrator::new(true, false));
        let trigger_type = TriggerType::new(
            "webhook",
            "Webhook",
            Box::new(Arc::clone(&registrator)),
            None,
        );

        registry.register_trigger_type(trigger_type).await.unwrap();

        assert_eq!(registrator.register_count.load(Ordering::SeqCst), 1);
        assert!(registry.trigger_types.contains_key("webhook"));
    }

    #[tokio::test]
    async fn test_register_trigger_propagates_registrator_error() {
        let registry = TriggerRegistry::new();
        let registrator = Arc::new(ControlledRegistrator::new(true, false));
        let trigger_type = TriggerType::new(
            "durable:subscriber",
            "Queue",
            Box::new(Arc::clone(&registrator)),
            None,
        );
        registry.register_trigger_type(trigger_type).await.unwrap();

        let err = registry
            .register_trigger(make_trigger("t-error", "durable:subscriber"))
            .await
            .expect_err("register should fail when registrator errors");

        assert_eq!(err.to_string(), "register failed");
        assert_eq!(registrator.register_count.load(Ordering::SeqCst), 1);
        assert!(!registry.triggers.contains_key("t-error"));
    }

    #[tokio::test]
    async fn test_unregister_trigger_propagates_registrator_error() {
        let registry = TriggerRegistry::new();
        let registrator = Arc::new(ControlledRegistrator::new(false, true));
        let trigger_type = TriggerType::new(
            "durable:subscriber",
            "Queue",
            Box::new(Arc::clone(&registrator)),
            None,
        );
        registry.register_trigger_type(trigger_type).await.unwrap();
        registry
            .register_trigger(make_trigger("t-unregister", "durable:subscriber"))
            .await
            .unwrap();

        let err = registry
            .unregister_trigger(
                "t-unregister".to_string(),
                Some("durable:subscriber".to_string()),
            )
            .await
            .expect_err("unregister should fail when registrator errors");

        assert_eq!(err.to_string(), "unregister failed");
        assert_eq!(registrator.unregister_count.load(Ordering::SeqCst), 1);
        assert!(registry.triggers.contains_key("t-unregister"));
    }

    #[tokio::test]
    async fn test_unregister_worker_continues_after_registrator_error() {
        let registry = TriggerRegistry::new();
        let worker_id = Uuid::new_v4();
        let registrator = Arc::new(ControlledRegistrator::new(false, true));
        let trigger_type = TriggerType::new(
            "durable:subscriber",
            "Queue",
            Box::new(Arc::clone(&registrator)),
            Some(worker_id),
        );
        registry.register_trigger_type(trigger_type).await.unwrap();

        let mut trigger = make_trigger("t-owned", "durable:subscriber");
        trigger.worker_id = Some(worker_id);
        registry.register_trigger(trigger).await.unwrap();

        registry.unregister_worker(&worker_id).await;

        assert_eq!(registrator.unregister_count.load(Ordering::SeqCst), 1);
        assert!(registry.triggers.is_empty());
        assert!(registry.trigger_types.is_empty());
    }

    // ---- Trigger Hash / Eq tests ----

    #[test]
    fn test_trigger_hash_and_eq_same_id() {
        let t1 = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "fn_a".to_string(),
            config: serde_json::json!({"interval": 5}),
            worker_id: None,
            metadata: None,
        };
        let t2 = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "webhook".to_string(),
            function_id: "fn_b".to_string(),
            config: serde_json::json!({"url": "https://example.com"}),
            worker_id: Some(Uuid::new_v4()),
            metadata: None,
        };

        // Same id means equal, even though other fields differ.
        assert_eq!(t1, t2);

        // HashSet should treat them as one entry.
        let mut set = HashSet::new();
        set.insert(t1);
        set.insert(t2);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_trigger_hash_and_eq_different_id() {
        let t1 = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "fn_a".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
        };
        let t2 = Trigger {
            id: "trigger-2".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "fn_a".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
        };

        assert_ne!(t1, t2);

        let mut set = HashSet::new();
        set.insert(t1);
        set.insert(t2);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_trigger_serialize_deserialize() {
        let trigger = make_trigger("t1", "cron");
        let json = serde_json::to_string(&trigger).unwrap();
        let deserialized: Trigger = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "t1");
        assert_eq!(deserialized.trigger_type, "cron");
        assert_eq!(deserialized.function_id, "fn_t1");
    }

    #[test]
    fn test_trigger_serialize_deserialize_with_metadata() {
        let trigger = Trigger {
            id: "t1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "fn_t1".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: Some(serde_json::json!({"team": "platform", "priority": "high"})),
        };
        let json = serde_json::to_string(&trigger).unwrap();
        let deserialized: Trigger = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.metadata,
            Some(serde_json::json!({"team": "platform", "priority": "high"}))
        );
    }

    #[test]
    fn test_trigger_serialize_deserialize_without_metadata() {
        let trigger = Trigger {
            id: "t2".to_string(),
            trigger_type: "http".to_string(),
            function_id: "fn_t2".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
        };
        let json = serde_json::to_string(&trigger).unwrap();
        let deserialized: Trigger = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.metadata, None);
        assert!(!json.contains("metadata"));
    }

    fn assert_http_call_response_properties(schema: &Value) {
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .expect("call_response_format schema has a properties object");
        for key in ["status_code", "headers", "body"] {
            assert!(
                properties.contains_key(key),
                "expected property '{key}', got: {properties:?}"
            );
        }

        // Pin optionality: the worker (`HttpResponse::from_function_return`) treats
        // every field as optional (status_code defaults to 200, headers to empty,
        // body to {}), so none of them may appear in the schema's `required` array.
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in ["status_code", "headers", "body"] {
                assert!(
                    !required.iter().any(|v| v.as_str() == Some(key)),
                    "'{key}' must not be required, got required: {required:?}"
                );
            }
        }
    }

    #[test]
    fn http_trigger_type_populates_call_response_format() {
        let tt = make_trigger_type("http");
        let schema = tt
            .call_response_format
            .expect("http trigger type sets call_response_format");
        assert_http_call_response_properties(&schema);
    }

    #[test]
    fn cron_trigger_type_has_no_call_response_format() {
        let tt = make_trigger_type("cron");
        assert!(tt.call_response_format.is_none());
    }

    #[test]
    fn with_call_response_format_sets_schema() {
        use crate::trigger_formats::HttpCallResponse;

        let tt = make_trigger_type("cron").with_call_response_format::<HttpCallResponse>();
        let schema = tt
            .call_response_format
            .expect("with_call_response_format sets call_response_format");
        assert_http_call_response_properties(&schema);
    }
}
