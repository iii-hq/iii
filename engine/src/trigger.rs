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

pub const KNOWN_TRIGGER_TYPE_PROVIDERS: &[(&str, &str)] = &[
    ("http", "iii-http"),
    ("cron", "iii-cron"),
    ("subscribe", "iii-pubsub"),
    ("state", "iii-state"),
    ("durable:subscriber", "queue"),
    ("stream", "iii-stream"),
    ("stream:join", "iii-stream"),
    ("stream:leave", "iii-stream"),
    ("log", "iii-observability"),
    ("trace", "iii-observability"),
    ("configuration", "configuration"),
];

const DEPRECATED_PROVIDER_REPLACEMENTS: &[(&str, &str)] = &[
    ("iii-http", "http"),
    ("iii-cron", "cron"),
    ("iii-state", "state"),
    ("iii-pubsub", "pubsub"),
];

/// Maps a known trigger type to the worker package that provides it. Connected
/// workers are attributed by UUID first; this table supplies discovery fallback
/// for in-process workers and identifies the provider when install guidance is
/// needed.
pub fn known_trigger_type_provider(trigger_type_id: &str) -> Option<&'static str> {
    KNOWN_TRIGGER_TYPE_PROVIDERS
        .iter()
        .find(|(id, _)| *id == trigger_type_id)
        .map(|(_, worker)| *worker)
}

fn preferred_install_name(provider: &'static str) -> &'static str {
    DEPRECATED_PROVIDER_REPLACEMENTS
        .iter()
        .find_map(|(deprecated, replacement)| (*deprecated == provider).then_some(*replacement))
        .unwrap_or(provider)
}

/// Outcome of [`TriggerRegistry::register_trigger`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterTriggerOutcome {
    /// The trigger type was available and the binding is live.
    Registered,
    /// The trigger type is not (yet) available, or the bind could not be
    /// delivered to its provider (connection closing). The registration
    /// intent is parked in [`TriggerRegistry::pending_triggers`] and will be
    /// activated automatically when the trigger type (re)registers.
    Deferred,
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

/// Marker error for registrator failures where the bind never reached the
/// provider (its connection channel is closed — the worker is dying or
/// disconnecting). The registry parks such binds for replay on the next type
/// (re)registration. Any other registrator error is a provider rejection
/// (e.g. invalid config): a definitive answer that fails the registration.
#[derive(Debug)]
pub struct RegistratorUnavailable;

impl std::fmt::Display for RegistratorUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("trigger provider connection unavailable")
    }
}

impl std::error::Error for RegistratorUnavailable {}

pub trait TriggerRegistrator: Send + Sync {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>>;
    /// Re-deliver an already-accepted binding (type re-registration replay,
    /// pending-intent recovery). Defaults to `register_trigger`. Registrators
    /// whose register path blocks on an ack from the provider worker
    /// (`WorkerConnection`) must override this to fire-and-forget: replay
    /// runs inline on that provider's own read loop, so awaiting its ack
    /// stalls the connection until the timeout.
    fn replay_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        self.register_trigger(trigger)
    }
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
    /// Namespace the target `function_id` resolves in when this trigger fires.
    /// Taken from the `RegisterTrigger` message's `namespace` (not the registering
    /// connection): the trigger names its target namespace explicitly, and an
    /// absent value means [`crate::protocol::DEFAULT_NAMESPACE`] — see
    /// `Engine::fire_triggers`. Also defaults to `default` for engine-internal /
    /// durable registrations and for wire payloads that predate the field.
    #[serde(default = "crate::protocol::default_namespace")]
    pub namespace: String,
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
    /// Registration intents whose trigger type is not currently available or
    /// whose activation did not complete: the type never registered, its
    /// provider worker disconnected, the bind could not be delivered, or the
    /// provider asynchronously rejected the activation. These
    /// bindings are disabled (they never fire) until the trigger type
    /// (re)registers, at which point they are activated and moved into
    /// `triggers`. Keyed by trigger id, like `triggers`.
    pub pending_triggers: Arc<DashMap<String, Trigger>>,
    /// Serializes two-map transitions between `triggers` and
    /// `pending_triggers` (a late-rejection re-park vs an explicit
    /// unregister). DashMap only makes each map operation atomic; without
    /// this, a re-park's remove+insert can interleave with an unregister's
    /// removes and resurrect an explicitly-unregistered binding. Held only
    /// around the map operations, never across an await.
    pub park_transition: std::sync::Mutex<()>,
}

impl TriggerRegistry {
    pub fn new() -> Self {
        Self {
            trigger_types: Arc::new(DashMap::new()),
            triggers: Arc::new(DashMap::new()),
            pending_triggers: Arc::new(DashMap::new()),
            park_transition: std::sync::Mutex::new(()),
        }
    }

    /// The namespace a fired trigger's target/condition function resolves in,
    /// looked up LIVE by trigger id at fire time.
    ///
    /// Message-broker triggers (queue `durable:subscriber`, `subscribe`) invoke
    /// their target inside an adapter task that holds only the trigger id — the
    /// enqueued/published message carries no namespace. Resolving against the
    /// registry here uses the trigger's CURRENT binding, exactly as the
    /// in-process workers (state/stream/cron/...) read the live `Trigger`. A
    /// message that sits in a durable queue while its trigger is re-registered
    /// therefore resolves against the new namespace, and one whose trigger has
    /// been unregistered falls back to [`DEFAULT_NAMESPACE`] (there is no binding
    /// left to consult). This is the intended semantic for this plan, not an
    /// accident of lookup.
    pub fn namespace_of(&self, trigger_id: &str) -> String {
        self.triggers
            .get(trigger_id)
            .map(|t| t.namespace.clone())
            .unwrap_or_else(|| crate::protocol::DEFAULT_NAMESPACE.to_string())
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
            // The entry may have been replaced between the snapshot above and
            // now (worker reconnect: the new connection replays the same
            // trigger id before the old connection's cleanup runs). Only
            // remove it — and only unregister it with the registrator — if it
            // still belongs to the disconnecting worker; a replaced binding
            // keeps firing. Same guard as trigger types below.
            if self
                .triggers
                .remove_if(&trigger.id, |_, t| t.worker_id == Some(*worker_id))
                .is_none()
            {
                tracing::debug!(
                    trigger_id = trigger.id,
                    "Trigger already replaced by another worker; leaving it"
                );
                continue;
            }

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

        // Connection-owned intents die with their worker, same as live
        // connection-owned bindings above.
        self.pending_triggers
            .retain(|_, trigger| trigger.worker_id != Some(*worker_id));

        for trigger_type_id in worker_trigger_type_ids {
            tracing::debug!(trigger_type_id = %trigger_type_id, "Removing trigger type");
            // The entry may have been replaced by a new provider between the
            // snapshot above and now (worker restart: the replacement can
            // register before the old connection's cleanup runs). Only remove
            // it if it still belongs to the disconnecting worker — and only
            // then are its bindings orphaned; a replaced type keeps firing.
            let removed = self
                .trigger_types
                .remove_if(&trigger_type_id, |_, tt| tt.worker_id == Some(*worker_id))
                .is_some();
            if !removed {
                tracing::debug!(
                    trigger_type_id = %trigger_type_id,
                    "Trigger type already replaced by another worker; leaving it"
                );
                continue;
            }
            tracing::debug!(trigger_type_id = %trigger_type_id, "Trigger type removed");

            // Bindings from other owners that reference the departed type are
            // now orphaned: nothing can fire them. Park them as pending
            // intents so they are re-activated when the type (re)registers —
            // a provider worker restart must not silently drop everyone
            // else's bindings.
            let orphaned: Vec<Trigger> = self
                .triggers
                .iter()
                .filter(|pair| pair.value().trigger_type == trigger_type_id)
                .map(|pair| pair.value().clone())
                .collect();
            for trigger in orphaned {
                // Park only when this call is the one that removed the
                // binding: a concurrent owner-disconnect cleanup may have
                // already reaped it, and parking it anyway would resurrect a
                // dead worker's binding as a pending intent.
                if self.triggers.remove(&trigger.id).is_none() {
                    continue;
                }
                tracing::warn!(
                    "{} Trigger {} (type {}, function {}) is disabled: its trigger type was unregistered. It will be re-registered automatically when the trigger type returns.",
                    "[DISABLED]".yellow(),
                    trigger.id.purple(),
                    trigger.trigger_type.purple(),
                    trigger.function_id.purple(),
                );
                self.pending_triggers.insert(trigger.id.clone(), trigger);
            }
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

        // Publish the type BEFORE any replay so every concurrent path
        // observes the new generation first:
        //  * a concurrent `register_trigger` that misses the pending drain
        //    finds the type on its post-park re-check, so an intent can never
        //    be stranded in pending while the type is available (see
        //    `register_trigger`);
        //  * a stale disconnect cleanup of the previous provider now fails
        //    its ownership check (see `unregister_worker`) instead of parking
        //    bindings this registration is about to replay — which would
        //    deliver them twice.
        self.trigger_types
            .insert(trigger_type_id.clone(), trigger_type);

        let matching_triggers: Vec<Trigger> = self
            .triggers
            .iter()
            .filter(|pair| pair.value().trigger_type == trigger_type_id)
            .map(|pair| pair.value().clone())
            .collect();

        // Re-fetch instead of using the inserted value: if an even newer
        // generation replaced the entry in the meantime, replay must route to
        // that one.
        if let Some(trigger_type) = self.trigger_types.get(&trigger_type_id) {
            for trigger in matching_triggers {
                let result = trigger_type
                    .registrator
                    .replay_trigger(trigger.clone())
                    .await;
                if let Err(err) = result {
                    tracing::error!(error = %err, "Error registering trigger");
                }
            }

            let pending: Vec<String> = self
                .pending_triggers
                .iter()
                .filter(|pair| pair.value().trigger_type == trigger_type_id)
                .map(|pair| pair.key().clone())
                .collect();

            for trigger_id in pending {
                // `remove` claims the intent, so a concurrent drain of the
                // same type cannot activate it twice.
                let Some((_, trigger)) = self.pending_triggers.remove(&trigger_id) else {
                    continue;
                };
                self.activate_pending_trigger(&trigger_type, trigger).await;
            }
        }

        Ok(())
    }

    /// Activate a claimed pending intent against its now-available trigger
    /// type. On registrator failure the intent is parked again so the next
    /// type (re)registration retries it.
    async fn activate_pending_trigger(&self, trigger_type: &TriggerType, trigger: Trigger) -> bool {
        match trigger_type
            .registrator
            .replay_trigger(trigger.clone())
            .await
        {
            Ok(()) => {
                tracing::info!(
                    "{} Trigger {} (type {}, function {}) recovered from pending",
                    "[REGISTERED]".green(),
                    trigger.id.purple(),
                    trigger.trigger_type.purple(),
                    trigger.function_id.purple(),
                );
                self.triggers.insert(trigger.id.clone(), trigger);
                true
            }
            Err(err) => {
                tracing::error!(
                    error = %err,
                    trigger_id = %trigger.id,
                    trigger_type = %trigger.trigger_type,
                    "Error registering pending trigger; it stays pending"
                );
                self.pending_triggers.insert(trigger.id.clone(), trigger);
                false
            }
        }
    }

    /// Human-readable warning for a registration intent that had to be
    /// parked because its trigger type is not available.
    fn pending_trigger_warning(trigger: &Trigger) -> String {
        let base = format!(
            "{} Trigger {} (function {}) was NOT activated: trigger type {} is not registered. It will be registered automatically when the trigger type becomes available.",
            "[PENDING]".yellow(),
            trigger.id.purple(),
            trigger.function_id.purple(),
            trigger.trigger_type.purple().bold(),
        );
        match known_trigger_type_provider(&trigger.trigger_type) {
            Some(worker_name) => {
                let install_name = preferred_install_name(worker_name);
                format!(
                    "{} If this persists, the {} worker is missing — run: {}",
                    base,
                    install_name.cyan().bold(),
                    format!("iii worker add {}", install_name).green().bold()
                )
            }
            None => format!(
                "{} If this persists, search for a worker that provides this trigger type at {}",
                base,
                "https://workers.iii.dev/".cyan().bold()
            ),
        }
    }

    pub async fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Result<RegisterTriggerOutcome, anyhow::Error> {
        let trigger_type_id = trigger.trigger_type.clone();
        let Some(trigger_type) = self.trigger_types.get(&trigger_type_id) else {
            // Park the intent instead of failing: workers may connect in any
            // order, and a binding that arrives before its trigger type's
            // provider must survive until the provider shows up.
            tracing::warn!("{}", Self::pending_trigger_warning(&trigger));
            self.pending_triggers
                .insert(trigger.id.clone(), trigger.clone());

            // Close the park/drain race: if the type registered between the
            // lookup above and the park, its pending drain may have already
            // run. Re-check and activate the intent ourselves — claiming it
            // via `remove` so we never double-activate with a drain.
            if let Some(trigger_type) = self.trigger_types.get(&trigger_type_id)
                && let Some((_, parked)) = self.pending_triggers.remove(&trigger.id)
                && self.activate_pending_trigger(&trigger_type, parked).await
            {
                return Ok(RegisterTriggerOutcome::Registered);
            }

            return Ok(RegisterTriggerOutcome::Deferred);
        };

        if let Err(err) = trigger_type
            .registrator
            .register_trigger(trigger.clone())
            .await
        {
            if err.downcast_ref::<RegistratorUnavailable>().is_none() {
                // The provider processed and rejected the bind (e.g. invalid
                // config): a definitive answer, not a delivery failure. Fail
                // the registration so callers surface the error, and leave any
                // previous live registration of this id tracked — the provider
                // still runs it and it must stay reachable for teardown.
                tracing::error!(error = %err, "Error registering trigger");
                return Err(err);
            }
            tracing::error!(
                error = %err,
                trigger_id = %trigger.id,
                trigger_type = %trigger.trigger_type,
                "Trigger provider unreachable; bind parked as pending until the trigger type re-registers"
            );
            // Park instead of failing; an undeliverable re-registration
            // supersedes a previous live binding with the same id (the dying
            // provider's jobs die with it) — the binding must end in exactly
            // one bucket.
            self.triggers.remove(&trigger.id);
            self.pending_triggers.insert(trigger.id.clone(), trigger);
            return Ok(RegisterTriggerOutcome::Deferred);
        }

        drop(trigger_type);

        tracing::debug!(trigger = %trigger.id, worker_id = %trigger.worker_id.unwrap_or_default(), "Registering trigger");

        // A live registration supersedes any parked intent with the same id.
        self.pending_triggers.remove(&trigger.id);
        self.triggers.insert(trigger.id.clone(), trigger);

        Ok(RegisterTriggerOutcome::Registered)
    }

    /// Unregister a trigger by id. Idempotent: returns `Ok(false)` when no
    /// trigger with this id exists (rather than erroring), so callers can treat
    /// double-unregister as a no-op. A parked pending intent counts as
    /// existing: unregistering it drops the intent and returns `Ok(true)`.
    /// On a registrator error the registry entry is left in place (registry
    /// and registrator stay consistent) and the error is propagated. Returns
    /// `Ok(true)` when a trigger was removed.
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

        let trigger = {
            let _park = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            let Some(trigger_entry) = self.triggers.get(&id) else {
                // A parked intent has no live registration to tear down —
                // dropping it from the pending bucket is the whole unregister.
                return Ok(self.pending_triggers.remove(&id).is_some());
            };
            trigger_entry.value().clone()
        };

        if let Some(tt) = self.trigger_types.get(&trigger.trigger_type) {
            tt.registrator.unregister_trigger(trigger.clone()).await?;
        }

        // A late async rejection may re-park this binding while the
        // registrator call above is in flight; an explicit unregister must
        // win over that park or the intent resurrects on the next type
        // registration. The transition lock makes this two-map clear atomic
        // with the re-park's own live-to-pending move.
        let _park = self
            .park_transition
            .lock()
            .expect("park_transition lock poisoned");
        self.triggers.remove(&id);
        self.pending_triggers.remove(&id);

        Ok(true)
    }

    /// Move a live binding to `pending_triggers` after its provider reported
    /// an async registration error (the caller has already verified the
    /// reporter owned the trigger type). No-op when the binding is already
    /// gone — a concurrent disconnect GC or explicit unregister reaped it,
    /// and parking then would resurrect a dead binding.
    pub async fn park_rejected_trigger(&self, id: &str, reporter_worker_id: Uuid) {
        let parked_type = {
            let _park = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            let Some((_, failed)) = self.triggers.remove(id) else {
                return;
            };
            tracing::warn!(
                trigger_id = %id,
                trigger_type = %failed.trigger_type,
                function_id = %failed.function_id,
                "Trigger registration rejected by provider; parked as pending until the trigger type re-registers"
            );
            let trigger_type_id = failed.trigger_type.clone();
            self.pending_triggers.insert(failed.id.clone(), failed);
            trigger_type_id
        };

        // Close the park/replacement race (mirror of the unknown-type park in
        // `register_trigger`): the reporter's ownership was checked before the
        // park, but a replacement provider may have registered — and drained
        // pending intents — in between, which would strand this park until
        // yet another re-registration. If the type's current owner is no
        // longer the reporter, claim the intent back (via `remove`, so we
        // never double-activate with a drain) and activate it through the
        // current provider. While the reporter still owns the type the park
        // is intended: replaying would just be rejected again.
        if let Some(trigger_type) = self.trigger_types.get(&parked_type)
            && trigger_type.worker_id != Some(reporter_worker_id)
            && let Some((_, parked)) = self.pending_triggers.remove(id)
        {
            self.activate_pending_trigger(&trigger_type, parked).await;
        }
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
            namespace: "default".to_string(),
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

    /// Provider restart where the replacement registers concurrently with the
    /// old connection's cleanup. Whatever the interleave: the replacement
    /// type entry must survive (ownership-checked removal), the binding must
    /// never be lost (it ends live or parked, in exactly one bucket), and the
    /// stale generation must receive nothing after replacement.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stale_cleanup_racing_a_replacement_never_clobbers_or_loses_bindings() {
        for _ in 0..100 {
            let registry = Arc::new(TriggerRegistry::new());
            let old_worker = Uuid::new_v4();

            let a = Arc::new(ControlledRegistrator::new(false, false));
            registry
                .register_trigger_type(TriggerType::new(
                    "evt",
                    "gen A",
                    Box::new(a.clone()),
                    Some(old_worker),
                ))
                .await
                .unwrap();

            let mut binding = make_trigger("t1", "evt");
            binding.worker_id = Some(Uuid::new_v4());
            registry.register_trigger(binding).await.unwrap();
            assert_eq!(a.register_count.load(Ordering::SeqCst), 1);

            let b = Arc::new(ControlledRegistrator::new(false, false));
            let new_worker = Uuid::new_v4();
            let replacement =
                TriggerType::new("evt", "gen B", Box::new(b.clone()), Some(new_worker));

            let cleanup = {
                let registry = registry.clone();
                tokio::spawn(async move { registry.unregister_worker(&old_worker).await })
            };
            let reregister = {
                let registry = registry.clone();
                tokio::spawn(
                    async move { registry.register_trigger_type(replacement).await.unwrap() },
                )
            };
            let (r1, r2) = tokio::join!(cleanup, reregister);
            r1.unwrap();
            r2.unwrap();

            let tt = registry
                .trigger_types
                .get("evt")
                .expect("replacement type survives the stale cleanup");
            assert_eq!(tt.worker_id, Some(new_worker));
            drop(tt);

            let live = registry.triggers.contains_key("t1");
            let parked = registry.pending_triggers.contains_key("t1");
            assert!(
                live ^ parked,
                "binding must end in exactly one bucket (live: {live}, parked: {parked})"
            );
            if live {
                assert!(
                    b.register_count.load(Ordering::SeqCst) >= 1,
                    "a live binding was delivered to the replacement generation"
                );
            }
            assert_eq!(
                a.register_count.load(Ordering::SeqCst),
                1,
                "stale generation receives nothing after replacement"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn replayed_binding_survives_stale_worker_cleanup() {
        // Regression guard for the reconnect kill loop (iii-hq/iii#1975),
        // binding half. When a worker's socket drops and its SDK reconnects,
        // the new connection replays its RegisterTrigger batch while the old
        // connection's `unregister_worker` cleanup runs concurrently. The
        // cleanup used to `triggers.remove(id)` unconditionally from a
        // snapshot, deleting (and provider-unregistering) the binding the new
        // connection had just re-registered — the console watcher would die
        // and never come back. Post-fix, removal is guarded by an ownership
        // CAS, mirroring the trigger-type guard in the same function.
        for _ in 0..100 {
            let registry = Arc::new(TriggerRegistry::new());

            // Trigger type owned by a separate provider worker so the stale
            // worker's cleanup never removes the type (which would orphan the
            // binding into `pending_triggers` and mask the guard we test).
            let provider = Arc::new(ControlledRegistrator::new(false, false));
            let provider_worker = Uuid::new_v4();
            registry
                .register_trigger_type(TriggerType::new(
                    "evt",
                    "provider",
                    Box::new(provider.clone()),
                    Some(provider_worker),
                ))
                .await
                .unwrap();

            let old_worker = Uuid::new_v4();
            let new_worker = Uuid::new_v4();

            // Filler bindings owned by the old worker widen the snapshot→remove
            // window: each removal awaits the provider's unregister, giving the
            // racing re-registration room to land before `t1`'s turn.
            for fid in ["f1", "f2", "f3"] {
                let mut b = make_trigger(fid, "evt");
                b.worker_id = Some(old_worker);
                registry.register_trigger(b).await.unwrap();
            }
            let mut binding = make_trigger("t1", "evt");
            binding.worker_id = Some(old_worker);
            registry.register_trigger(binding).await.unwrap();

            let cleanup = {
                let registry = registry.clone();
                tokio::spawn(async move { registry.unregister_worker(&old_worker).await })
            };
            let reregister = {
                let registry = registry.clone();
                tokio::spawn(async move {
                    let mut replay = make_trigger("t1", "evt");
                    replay.worker_id = Some(new_worker);
                    registry.register_trigger(replay).await.unwrap();
                })
            };
            let (r1, r2) = tokio::join!(cleanup, reregister);
            r1.unwrap();
            r2.unwrap();

            let t1 = registry
                .triggers
                .get("t1")
                .expect("re-registered binding must survive the stale worker's cleanup");
            assert_eq!(
                t1.worker_id,
                Some(new_worker),
                "surviving binding must be owned by the reconnected worker"
            );
            assert!(
                !registry.pending_triggers.contains_key("t1"),
                "binding must be live, not parked"
            );
        }
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
    async fn test_trigger_registry_register_trigger_missing_type_defers() {
        let registry = TriggerRegistry::new();
        // No trigger type registered -- the intent is parked, not dropped.
        let trigger = make_trigger("t1", "nonexistent");
        let result = registry.register_trigger(trigger).await;
        assert!(matches!(result, Ok(RegisterTriggerOutcome::Deferred)));
        assert!(registry.triggers.is_empty());
        assert!(registry.pending_triggers.contains_key("t1"));
    }

    #[test]
    fn pending_warning_uses_new_install_names_without_changing_legacy_owners() {
        let cases = [
            ("http", "iii-http", "http"),
            ("cron", "iii-cron", "cron"),
            ("subscribe", "iii-pubsub", "pubsub"),
            ("state", "iii-state", "state"),
            ("durable:subscriber", "queue", "queue"),
        ];

        for (trigger_type, owner, install_name) in cases {
            assert_eq!(known_trigger_type_provider(trigger_type), Some(owner));

            let msg = TriggerRegistry::pending_trigger_warning(&make_trigger("t1", trigger_type));
            let expected_hint = format!("iii worker add {install_name}");
            assert!(
                msg.contains(&expected_hint),
                "Expected hint with '{expected_hint}', got: {msg}"
            );
        }
    }

    #[test]
    fn pending_warning_for_unknown_type_points_to_workers_directory() {
        let msg = TriggerRegistry::pending_trigger_warning(&make_trigger("t1", "nonexistent"));
        assert!(
            msg.contains("https://workers.iii.dev/"),
            "Expected workers directory recommendation, got: {msg}"
        );
    }

    #[tokio::test]
    async fn deferred_trigger_activates_when_type_registers() {
        let registry = TriggerRegistry::new();

        let result = registry
            .register_trigger(make_trigger("t1", "webhook"))
            .await;
        assert!(matches!(result, Ok(RegisterTriggerOutcome::Deferred)));
        assert!(registry.pending_triggers.contains_key("t1"));
        assert!(!registry.triggers.contains_key("t1"));

        let registrator = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "webhook",
                "Webhook",
                Box::new(Arc::clone(&registrator)),
                None,
            ))
            .await
            .unwrap();

        assert_eq!(
            registrator.register_count.load(Ordering::SeqCst),
            1,
            "parked intent delivered to the registrator when the type arrives"
        );
        assert!(registry.pending_triggers.is_empty());
        assert!(registry.triggers.contains_key("t1"));
    }

    #[tokio::test]
    async fn pending_trigger_that_fails_activation_stays_pending_and_retries() {
        let registry = TriggerRegistry::new();
        registry
            .register_trigger(make_trigger("t1", "webhook"))
            .await
            .unwrap();

        let failing = Arc::new(ControlledRegistrator::new(true, false));
        registry
            .register_trigger_type(TriggerType::new(
                "webhook",
                "gen A",
                Box::new(Arc::clone(&failing)),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(failing.register_count.load(Ordering::SeqCst), 1);
        assert!(
            registry.pending_triggers.contains_key("t1"),
            "failed activation keeps the intent parked"
        );
        assert!(!registry.triggers.contains_key("t1"));

        // The next (re)registration of the type retries the parked intent.
        let healthy = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "webhook",
                "gen B",
                Box::new(Arc::clone(&healthy)),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(healthy.register_count.load(Ordering::SeqCst), 1);
        assert!(registry.pending_triggers.is_empty());
        assert!(registry.triggers.contains_key("t1"));
    }

    #[tokio::test]
    async fn provider_disconnect_parks_other_owners_bindings_until_type_returns() {
        let registry = TriggerRegistry::new();
        let provider = Uuid::new_v4();

        let gen_a = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "evt",
                "gen A",
                Box::new(Arc::clone(&gen_a)),
                Some(provider),
            ))
            .await
            .unwrap();

        // A durable binding (engine::register_trigger path) and another
        // worker's binding, both against the provider's type.
        registry
            .register_trigger(make_trigger("t_durable", "evt"))
            .await
            .unwrap();
        let other_worker = Uuid::new_v4();
        let mut other = make_trigger("t_other", "evt");
        other.worker_id = Some(other_worker);
        registry.register_trigger(other).await.unwrap();

        // Provider restarts: its type goes away, the surviving bindings are
        // parked (disabled), not dropped.
        registry.unregister_worker(&provider).await;
        assert!(registry.trigger_types.is_empty());
        assert!(registry.triggers.is_empty());
        assert!(registry.pending_triggers.contains_key("t_durable"));
        assert!(registry.pending_triggers.contains_key("t_other"));

        // Provider reconnects and re-registers the type: both bindings
        // come back to life.
        let gen_b = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "evt",
                "gen B",
                Box::new(Arc::clone(&gen_b)),
                Some(Uuid::new_v4()),
            ))
            .await
            .unwrap();
        assert_eq!(gen_b.register_count.load(Ordering::SeqCst), 2);
        assert!(registry.pending_triggers.is_empty());
        assert!(registry.triggers.contains_key("t_durable"));
        assert!(registry.triggers.contains_key("t_other"));
    }

    /// Recovery must preserve the namespace. A trigger registered in `orders`
    /// while its type is unavailable is parked in `pending_triggers`; when the
    /// type is (re)registered it is activated into the live `triggers` map. The
    /// recovered binding must still resolve in `orders` — every dispatch path
    /// reads `namespace_of`, so if recovery dropped the namespace the trigger
    /// would silently fire in `default` (BUG 1 via the recovery path).
    #[tokio::test]
    async fn a_recovered_pending_trigger_keeps_its_namespace() {
        let registry = TriggerRegistry::new();

        // Arrives before its type exists → parked pending.
        let mut trig = make_trigger("t_ns", "evt");
        trig.namespace = "orders".to_string();
        registry.register_trigger(trig).await.unwrap();
        assert!(registry.pending_triggers.contains_key("t_ns"));
        assert!(!registry.triggers.contains_key("t_ns"));

        // The type registers: the parked intent is activated (recovered).
        let healthy = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "evt",
                "gen A",
                Box::new(Arc::clone(&healthy)),
                None,
            ))
            .await
            .unwrap();

        assert!(registry.triggers.contains_key("t_ns"), "trigger recovered");
        assert!(registry.pending_triggers.is_empty());
        assert_eq!(
            registry.namespace_of("t_ns"),
            "orders",
            "a recovered trigger must still resolve in the namespace it was registered in, \
             not fall back to default"
        );
    }

    #[tokio::test]
    async fn unregister_worker_drops_its_own_pending_intents() {
        let registry = TriggerRegistry::new();
        let worker = Uuid::new_v4();

        let mut owned = make_trigger("t_owned", "missing-type");
        owned.worker_id = Some(worker);
        registry.register_trigger(owned).await.unwrap();
        registry
            .register_trigger(make_trigger("t_durable", "missing-type"))
            .await
            .unwrap();
        assert_eq!(registry.pending_triggers.len(), 2);

        registry.unregister_worker(&worker).await;

        assert!(
            !registry.pending_triggers.contains_key("t_owned"),
            "connection-owned intent dies with its worker"
        );
        assert!(
            registry.pending_triggers.contains_key("t_durable"),
            "durable intent survives another worker's disconnect"
        );

        // The type arriving later must not resurrect the dropped intent —
        // only the surviving durable one may activate.
        let registrator = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "missing-type",
                "now present",
                Box::new(Arc::clone(&registrator)),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(registrator.register_count.load(Ordering::SeqCst), 1);
        assert!(registry.triggers.contains_key("t_durable"));
        assert!(!registry.triggers.contains_key("t_owned"));
        assert!(registry.pending_triggers.is_empty());
    }

    #[tokio::test]
    async fn owner_disconnect_while_parked_drops_the_disabled_binding() {
        // Full lifecycle chain: a live binding is parked because its type's
        // provider disconnected ([DISABLED]); then the binding's OWNER
        // disconnects while it is parked. The intent must die with its owner
        // and must NOT come back when the provider returns.
        let registry = TriggerRegistry::new();
        let provider = Uuid::new_v4();
        let owner = Uuid::new_v4();

        let gen_a = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "evt",
                "gen A",
                Box::new(Arc::clone(&gen_a)),
                Some(provider),
            ))
            .await
            .unwrap();

        let mut owned = make_trigger("t_owned", "evt");
        owned.worker_id = Some(owner);
        registry.register_trigger(owned).await.unwrap();

        // Provider restarts: the owner's binding is parked, not dropped.
        registry.unregister_worker(&provider).await;
        assert!(registry.pending_triggers.contains_key("t_owned"));

        // The owner disconnects while its binding is still parked.
        registry.unregister_worker(&owner).await;
        assert!(
            registry.pending_triggers.is_empty(),
            "parked intent dies with its owning worker"
        );

        // Provider returns: nothing may be re-delivered.
        let gen_b = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "evt",
                "gen B",
                Box::new(Arc::clone(&gen_b)),
                Some(Uuid::new_v4()),
            ))
            .await
            .unwrap();
        assert_eq!(
            gen_b.register_count.load(Ordering::SeqCst),
            0,
            "dead owner's binding must not be resurrected"
        );
        assert!(registry.triggers.is_empty());
        assert!(registry.pending_triggers.is_empty());
    }

    #[tokio::test]
    async fn unregister_trigger_drops_pending_intent() {
        let registry = TriggerRegistry::new();
        registry
            .register_trigger(make_trigger("t1", "missing-type"))
            .await
            .unwrap();
        assert!(registry.pending_triggers.contains_key("t1"));

        let removed = registry.unregister_trigger("t1".to_string(), None).await;
        assert!(matches!(removed, Ok(true)));
        assert!(registry.pending_triggers.is_empty());

        // Its type arriving later must not resurrect the dropped intent.
        let registrator = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "missing-type",
                "now present",
                Box::new(Arc::clone(&registrator)),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(registrator.register_count.load(Ordering::SeqCst), 0);
        assert!(registry.triggers.is_empty());
    }

    #[tokio::test]
    async fn live_registration_supersedes_parked_intent_with_same_id() {
        let registry = TriggerRegistry::new();
        registry
            .register_trigger(make_trigger("t1", "webhook"))
            .await
            .unwrap();
        assert!(registry.pending_triggers.contains_key("t1"));

        let registrator = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "evt",
                "other type",
                Box::new(Arc::clone(&registrator)),
                None,
            ))
            .await
            .unwrap();

        // Re-registering the same id against an available type replaces the
        // parked intent instead of leaving a stale duplicate behind.
        registry
            .register_trigger(make_trigger("t1", "evt"))
            .await
            .unwrap();
        assert!(registry.pending_triggers.is_empty());
        assert_eq!(registry.triggers.get("t1").unwrap().trigger_type, "evt");
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
            .expect_err("provider rejection should fail the registration");

        assert_eq!(err.to_string(), "register failed");
        assert_eq!(registrator.register_count.load(Ordering::SeqCst), 1);
        assert!(!registry.triggers.contains_key("t-error"));
        assert!(
            !registry.pending_triggers.contains_key("t-error"),
            "a rejected bind must not be parked"
        );
    }

    fn closed_channel_connection() -> crate::worker_connections::WorkerConnection {
        let (tx, rx) = tokio::sync::mpsc::channel::<crate::engine::Outbound>(1);
        drop(rx);
        crate::worker_connections::WorkerConnection::new(tx)
    }

    #[tokio::test]
    async fn register_trigger_transport_failure_parks_and_next_type_registration_activates() {
        let registry = TriggerRegistry::new();
        // Real provider connection whose channel is already closed: the send
        // fails, which must classify as RegistratorUnavailable and park.
        let trigger_type = TriggerType::new(
            "durable:subscriber",
            "Queue",
            Box::new(closed_channel_connection()),
            None,
        );
        registry.register_trigger_type(trigger_type).await.unwrap();

        let outcome = registry
            .register_trigger(make_trigger("t-park", "durable:subscriber"))
            .await
            .unwrap();

        assert_eq!(outcome, RegisterTriggerOutcome::Deferred);
        assert!(!registry.triggers.contains_key("t-park"));
        assert!(
            registry.pending_triggers.contains_key("t-park"),
            "undeliverable bind is parked, not dropped"
        );

        // Provider heals and re-registers the type: the parked intent activates.
        let healthy = Arc::new(ControlledRegistrator::new(false, false));
        let trigger_type = TriggerType::new(
            "durable:subscriber",
            "Queue",
            Box::new(Arc::clone(&healthy)),
            None,
        );
        registry.register_trigger_type(trigger_type).await.unwrap();

        assert_eq!(healthy.register_count.load(Ordering::SeqCst), 1);
        assert!(registry.pending_triggers.is_empty());
        assert!(registry.triggers.contains_key("t-park"));
    }

    #[tokio::test]
    async fn rejected_reregistration_keeps_existing_live_binding_tracked() {
        let registry = TriggerRegistry::new();
        let failing = Arc::new(ControlledRegistrator::new(true, false));
        registry.trigger_types.insert(
            "webhook".to_string(),
            TriggerType::new("webhook", "Webhook", Box::new(Arc::clone(&failing)), None),
        );
        registry
            .triggers
            .insert("t1".to_string(), make_trigger("t1", "webhook"));

        let result = registry
            .register_trigger(make_trigger("t1", "webhook"))
            .await;

        assert!(result.is_err());
        assert!(
            registry.triggers.contains_key("t1"),
            "the previous live registration must stay tracked for teardown"
        );
        assert!(!registry.pending_triggers.contains_key("t1"));
    }

    #[tokio::test]
    async fn undeliverable_reregistration_of_live_binding_ends_parked_not_split() {
        let registry = TriggerRegistry::new();
        registry.trigger_types.insert(
            "webhook".to_string(),
            TriggerType::new(
                "webhook",
                "Webhook",
                Box::new(closed_channel_connection()),
                None,
            ),
        );
        registry
            .triggers
            .insert("t1".to_string(), make_trigger("t1", "webhook"));

        let result = registry
            .register_trigger(make_trigger("t1", "webhook"))
            .await;

        assert!(matches!(result, Ok(RegisterTriggerOutcome::Deferred)));
        assert!(
            !registry.triggers.contains_key("t1"),
            "stale live entry must not survive; the dying provider's jobs die with it"
        );
        assert!(registry.pending_triggers.contains_key("t1"));
    }

    #[tokio::test]
    async fn unregister_live_trigger_clears_stale_pending_intent() {
        // Mid-unregister race state: a late async rejection re-parked the
        // binding while the registrator unregister call was in flight.
        let registry = TriggerRegistry::new();
        let registrator = Arc::new(ControlledRegistrator::new(false, false));
        registry.trigger_types.insert(
            "webhook".to_string(),
            TriggerType::new(
                "webhook",
                "Webhook",
                Box::new(Arc::clone(&registrator)),
                None,
            ),
        );
        registry
            .triggers
            .insert("t1".to_string(), make_trigger("t1", "webhook"));
        registry
            .pending_triggers
            .insert("t1".to_string(), make_trigger("t1", "webhook"));

        let removed = registry
            .unregister_trigger("t1".to_string(), None)
            .await
            .unwrap();

        assert!(removed);
        assert_eq!(registrator.unregister_count.load(Ordering::SeqCst), 1);
        assert!(!registry.triggers.contains_key("t1"));
        assert!(
            !registry.pending_triggers.contains_key("t1"),
            "explicit unregister must clear a raced re-park, or the intent replays on the next type registration"
        );
    }

    #[tokio::test]
    async fn stale_rejection_after_provider_replacement_reactivates_through_current_provider() {
        let registry = TriggerRegistry::new();
        let old_provider = Uuid::new_v4();
        let replacement = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "webhook",
                "gen B",
                Box::new(Arc::clone(&replacement)),
                Some(Uuid::new_v4()),
            ))
            .await
            .unwrap();
        registry
            .triggers
            .insert("t1".to_string(), make_trigger("t1", "webhook"));

        // The rejection reporter is the replaced generation's worker, not the
        // current owner: the park must immediately re-activate through the
        // replacement instead of stranding the binding.
        registry.park_rejected_trigger("t1", old_provider).await;

        assert!(registry.triggers.contains_key("t1"));
        assert!(!registry.pending_triggers.contains_key("t1"));
        assert_eq!(replacement.register_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rejection_from_current_owner_parks_without_replay_pingpong() {
        let registry = TriggerRegistry::new();
        let owner_worker = Uuid::new_v4();
        let owner = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "webhook",
                "Webhook",
                Box::new(Arc::clone(&owner)),
                Some(owner_worker),
            ))
            .await
            .unwrap();
        registry
            .triggers
            .insert("t1".to_string(), make_trigger("t1", "webhook"));

        registry.park_rejected_trigger("t1", owner_worker).await;

        assert!(!registry.triggers.contains_key("t1"));
        assert!(registry.pending_triggers.contains_key("t1"));
        assert_eq!(
            owner.register_count.load(Ordering::SeqCst),
            0,
            "no replay back to the provider that just rejected the bind"
        );
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
            namespace: "default".to_string(),
        };
        let t2 = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "webhook".to_string(),
            function_id: "fn_b".to_string(),
            config: serde_json::json!({"url": "https://example.com"}),
            worker_id: Some(Uuid::new_v4()),
            metadata: None,
            namespace: "default".to_string(),
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
            namespace: "default".to_string(),
        };
        let t2 = Trigger {
            id: "trigger-2".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "fn_a".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
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
            namespace: "default".to_string(),
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
            namespace: "default".to_string(),
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
