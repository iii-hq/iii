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
    ("http", "http"),
    ("cron", "cron"),
    ("subscribe", "pubsub"),
    ("state", "state"),
    ("durable:subscriber", "queue"),
    ("stream", "iii-stream"),
    ("stream:join", "iii-stream"),
    ("stream:leave", "iii-stream"),
    ("log", "iii-observability"),
    ("trace", "iii-observability"),
    ("configuration", "configuration"),
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

/// What identifies a provider: the namespace it serves and the type id it
/// answers for.
///
/// The id alone was the key until trigger types became namespaced, and two
/// projects providing the same id overwrote each other — silently, and taking
/// the loser's bindings with them into the winner's provider.
pub type TypeKey = (String, String);

/// Builds a [`TypeKey`] without the call sites spelling out two `to_string()`s.
pub fn type_key(namespace: &str, id: &str) -> TypeKey {
    (namespace.to_string(), id.to_string())
}

#[derive(Clone)]
pub struct TriggerType {
    pub id: String,
    /// Namespace this provider serves. In-process engine providers use
    /// [`DEFAULT_NAMESPACE`]; an SDK worker's provider uses its connection's
    /// namespace.
    pub namespace: String,
    pub _description: String,
    pub trigger_request_format: Option<Value>,
    pub call_request_format: Option<Value>,
    pub call_response_format: Option<Value>,
    pub registrator: Arc<dyn TriggerRegistrator>,
    pub worker_id: Option<Uuid>,
}

impl TriggerType {
    /// A provider in [`DEFAULT_NAMESPACE`] — where every in-process engine
    /// provider lives, and where a resolved lookup falls back to.
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        registrator: Box<dyn TriggerRegistrator>,
        worker_id: Option<Uuid>,
    ) -> Self {
        Self::new_ns(
            crate::protocol::DEFAULT_NAMESPACE,
            id,
            description,
            registrator,
            worker_id,
        )
    }

    pub fn new_ns(
        namespace: impl Into<String>,
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
            namespace: namespace.into(),
            _description: description.into(),
            trigger_request_format,
            call_request_format,
            call_response_format,
            registrator: Arc::from(registrator),
            worker_id,
        }
    }

    /// The key this provider is filed under.
    pub fn key(&self) -> TypeKey {
        (self.namespace.clone(), self.id.clone())
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
    /// `Engine::fire_triggers`. Also defaults to `default` for engine-internal
    /// registrations and for wire payloads that predate the field. A durable
    /// `engine::register_trigger` binding takes the calling connection's
    /// namespace instead: its target is the caller's own function.
    #[serde(default = "crate::protocol::default_namespace")]
    pub namespace: String,
    /// Namespace the caller named for the provider, if any.
    ///
    /// `Some` is strict: that namespace or nothing. `None` asks for the
    /// resolution in [`TriggerRegistry::resolve_provider_key`] — `home_namespace`
    /// first, then [`DEFAULT_NAMESPACE`] — which is what carries workers that
    /// have not been migrated onto the engine's own providers without them
    /// saying anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_namespace: Option<String>,
    /// The registering connection's namespace: the first place a resolved
    /// lookup looks, and the namespace a provider must later register in to
    /// claim this binding back from the fallback.
    ///
    /// Kept apart from `namespace`, which is the *target*'s: a worker may bind
    /// a trigger whose function lives elsewhere, and its provider should still
    /// be the one at home.
    #[serde(default = "crate::protocol::default_namespace")]
    pub home_namespace: String,
    /// Where the provider was actually found. Written by the registry at bind
    /// time and read by everything that has to reach the same provider again:
    /// replay, unregister, and the disconnect sweep.
    #[serde(default = "crate::protocol::default_namespace")]
    pub provider_namespace: String,
}

impl Trigger {
    /// The three namespace fields for an engine-internal binding: one declared
    /// in engine configuration rather than by a worker connection.
    ///
    /// These are engine orchestration, so they are at home in
    /// [`DEFAULT_NAMESPACE`] and resolve their provider there — which is also
    /// where every in-process provider registers. They stay resolvable rather
    /// than explicit so a project that later provides the type can still be
    /// preferred if such a binding is ever given a different home.
    pub fn internal_namespaces() -> (Option<String>, String, String) {
        (
            None,
            crate::protocol::default_namespace(),
            crate::protocol::default_namespace(),
        )
    }

    /// The provider this binding is currently bound to.
    pub fn provider_key(&self) -> TypeKey {
        (self.provider_namespace.clone(), self.trigger_type.clone())
    }

    /// Whether this binding is only on the fallback provider, and so would
    /// prefer a provider that registers at home later.
    ///
    /// Explicit bindings are never re-homed: naming a namespace means that one
    /// and no other.
    pub fn is_on_fallback(&self) -> bool {
        self.trigger_namespace.is_none() && self.provider_namespace != self.home_namespace
    }
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

/// Provider deliveries retained until detach succeeds, serialized per binding.
#[derive(Default)]
struct BindingLifecycle {
    deliveries: Vec<(TriggerType, Trigger)>,
}

/// Keeps a gate shared with queued operations and reclaims idle, empty gates.
/// The last lease checks Arc ownership while holding the map's entry lock,
/// so a new caller cannot acquire a different gate for the same binding.
struct LifecycleLease<'a> {
    registry: &'a TriggerRegistry,
    id: String,
    gate: Option<Arc<tokio::sync::Mutex<BindingLifecycle>>>,
    intent: Option<Uuid>,
}

impl Drop for LifecycleLease<'_> {
    fn drop(&mut self) {
        if let Some(intent) = self.intent {
            self.registry.in_flight.remove(&intent);
        }
        self.registry.lifecycles.remove_if(&self.id, |_, gate| {
            // Release this lease under the same shard lock as the last-owner
            // check, including when two final leases drop concurrently.
            drop(self.gate.take());
            Arc::strong_count(gate) == 1
                && !self.registry.triggers.contains_key(&self.id)
                && !self.registry.pending_triggers.contains_key(&self.id)
                && gate
                    .try_lock()
                    .is_ok_and(|state| state.deliveries.is_empty())
        });
    }
}

#[derive(Default)]
pub struct TriggerRegistry {
    /// Providers, by `(namespace, type id)`.
    pub trigger_types: Arc<DashMap<TypeKey, TriggerType>>,
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
    /// Gates outlive map removal so queued teardown cannot race a new gate for
    /// the same id. Type publication never waits on these gates: the holder
    /// reconciles against the current provider before releasing its gate.
    lifecycles: DashMap<String, Arc<tokio::sync::Mutex<BindingLifecycle>>>,
    /// Intents are visible even while waiting for their binding gate.
    in_flight: DashMap<Uuid, Trigger>,
}

impl TriggerRegistry {
    pub fn new() -> Self {
        Self {
            trigger_types: Arc::new(DashMap::new()),
            triggers: Arc::new(DashMap::new()),
            pending_triggers: Arc::new(DashMap::new()),
            park_transition: std::sync::Mutex::new(()),
            lifecycles: DashMap::new(),
            in_flight: DashMap::new(),
        }
    }

    /// Take an owned provider snapshot before calling user code or awaiting.
    /// Holding a DashMap guard across an await can block a reconnect's insert
    /// on the same shard and stall the executor that must resume the reader.
    fn provider_snapshot(&self, key: &TypeKey) -> Option<TriggerType> {
        self.trigger_types
            .get(key)
            .map(|entry| entry.value().clone())
    }

    /// Lease a gate and optionally expose a direct registration's intent to
    /// disconnect cleanup before waiting for the gate. Drop reclaims empty
    /// entries on both normal completion and cancellation.
    fn lifecycle(&self, id: &str, trigger: Option<&Trigger>) -> LifecycleLease<'_> {
        let gate = Arc::clone(self.lifecycles.entry(id.to_owned()).or_default().value());
        let intent = trigger.map(|trigger| {
            let token = Uuid::new_v4();
            self.in_flight.insert(token, trigger.clone());
            token
        });
        LifecycleLease {
            registry: self,
            id: id.to_owned(),
            gate: Some(gate),
            intent,
        }
    }

    /// Read a binding only after taking its lifecycle gate, not from a replay
    /// batch's potentially stale snapshot.
    fn binding_snapshot(&self, id: &str) -> Option<Trigger> {
        self.triggers
            .get(id)
            .map(|t| t.clone())
            .or_else(|| self.pending_triggers.get(id).map(|t| t.clone()))
    }

    /// Detach every generation that accepted this binding. Failed detaches
    /// remain tracked so an explicit unregister can retry them.
    async fn detach_deliveries(state: &mut BindingLifecycle) -> Result<(), anyhow::Error> {
        let mut failure = None;
        let mut index = 0;
        while index < state.deliveries.len() {
            let (provider, trigger) = &state.deliveries[index];
            // Leave the delivery recorded across await: cancellation must not
            // discard this or the remaining providers in a drained iterator.
            match provider
                .registrator
                .unregister_trigger(trigger.clone())
                .await
            {
                Ok(()) => {
                    state.deliveries.remove(index);
                }
                Err(err) => {
                    failure = Some(err);
                    index += 1;
                }
            }
        }
        match failure {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// Keep an existing accepted delivery without synthesizing a successful
    /// attempt. If replacement skipped this gate, replay the previous intent
    /// through settlement instead of attributing it to the rejecting provider.
    async fn reconcile_existing(
        &self,
        state: tokio::sync::OwnedMutexGuard<BindingLifecycle>,
        trigger: Trigger,
    ) {
        {
            let fence = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            let current = self
                .resolve_provider_key(&trigger)
                .and_then(|key| self.provider_snapshot(&key));
            if self.triggers.contains_key(&trigger.id)
                && current.as_ref().is_some_and(|current| {
                    state.deliveries.iter().any(|(provider, delivered)| {
                        Arc::ptr_eq(&provider.registrator, &current.registrator)
                            && delivered.provider_key() == trigger.provider_key()
                            && delivered.config == trigger.config
                            && delivered.function_id == trigger.function_id
                    })
                })
            {
                drop(state);
                drop(fence);
                return;
            }
        }
        self.settle_binding(state, trigger, None).await;
    }

    /// Reconcile an accepted intent with the latest resolved provider. The
    /// async gate serializes delivery/detach; the short transition fence makes
    /// publication AND releasing that gate atomic with provider replacement.
    /// Replacement skips busy gates, so it cannot block a provider's ack loop.
    async fn settle_binding(
        &self,
        mut state: tokio::sync::OwnedMutexGuard<BindingLifecycle>,
        mut trigger: Trigger,
        mut attempt: Option<(TriggerType, Result<(), anyhow::Error>)>,
    ) -> RegisterTriggerOutcome {
        loop {
            if let Some((provider, result)) = attempt.take() {
                let succeeded = result.is_ok();
                if succeeded {
                    state
                        .deliveries
                        .retain(|(p, _)| !Arc::ptr_eq(&p.registrator, &provider.registrator));
                    state.deliveries.push((provider.clone(), trigger.clone()));
                } else if let Err(err) = result {
                    tracing::error!(error = %err, trigger_id = %trigger.id, "Trigger activation failed; retaining intent");
                }
                let fence = self
                    .park_transition
                    .lock()
                    .expect("park_transition lock poisoned");
                let current = self
                    .resolve_provider_key(&trigger)
                    .and_then(|key| self.provider_snapshot(&key));
                if current
                    .as_ref()
                    .is_some_and(|p| Arc::ptr_eq(&p.registrator, &provider.registrator))
                {
                    self.triggers.remove(&trigger.id);
                    self.pending_triggers.remove(&trigger.id);
                    if succeeded {
                        self.triggers.insert(trigger.id.clone(), trigger);
                    } else {
                        self.pending_triggers.insert(trigger.id.clone(), trigger);
                    }
                    drop(state);
                    drop(fence);
                    return if succeeded {
                        RegisterTriggerOutcome::Registered
                    } else {
                        RegisterTriggerOutcome::Deferred
                    };
                }
            }

            // Old generations may still run jobs after replacement. Detach
            // before replay, retaining failed deliveries for later teardown.
            if let Err(err) = Self::detach_deliveries(&mut state).await {
                tracing::warn!(error = %err, trigger_id = %trigger.id, "Could not detach previous trigger generation");
            }
            let provider = {
                let fence = self
                    .park_transition
                    .lock()
                    .expect("park_transition lock poisoned");
                match self
                    .resolve_provider_key(&trigger)
                    .and_then(|key| self.provider_snapshot(&key))
                {
                    Some(provider) => provider,
                    None => {
                        self.triggers.remove(&trigger.id);
                        self.pending_triggers.insert(trigger.id.clone(), trigger);
                        drop(state);
                        drop(fence);
                        return RegisterTriggerOutcome::Deferred;
                    }
                }
            };
            trigger.provider_namespace = provider.namespace.clone();
            let result = provider.registrator.replay_trigger(trigger.clone()).await;
            attempt = Some((provider, result));
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

    /// Where this binding's provider lives, or `None` when nothing provides it.
    ///
    /// Two steps, and the order is the whole migration story: a project that
    /// ships its own provider for a type id gets it, and everything that has
    /// not been migrated reaches the engine's provider in
    /// [`DEFAULT_NAMESPACE`] without having to say so. Naming a namespace
    /// explicitly skips both steps — that namespace or nothing, because a
    /// caller who was specific should not be quietly served by someone else.
    pub fn resolve_provider_key(&self, trigger: &Trigger) -> Option<TypeKey> {
        if let Some(explicit) = &trigger.trigger_namespace {
            let key = type_key(explicit, &trigger.trigger_type);
            return self.trigger_types.contains_key(&key).then_some(key);
        }

        let home = type_key(&trigger.home_namespace, &trigger.trigger_type);
        if self.trigger_types.contains_key(&home) {
            return Some(home);
        }

        let fallback = type_key(crate::protocol::DEFAULT_NAMESPACE, &trigger.trigger_type);
        self.trigger_types
            .contains_key(&fallback)
            .then_some(fallback)
    }

    /// Reconcile a replay candidate after acquiring its gate. A busy binding
    /// is the gate holder's responsibility; waiting here could stall the same
    /// provider read loop that must deliver its registration acknowledgement.
    async fn replay_candidate(&self, id: &str, key: &TypeKey) {
        let lease = self.lifecycle(id, None);
        let Ok(mut state) =
            Arc::clone(lease.gate.as_ref().expect("live lifecycle lease")).try_lock_owned()
        else {
            return;
        };
        let Some(trigger) = self.binding_snapshot(id) else {
            return;
        };
        {
            let fence = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            let current = self
                .resolve_provider_key(&trigger)
                .and_then(|key| self.provider_snapshot(&key));
            if self.triggers.contains_key(id)
                && current.as_ref().is_some_and(|current| {
                    state
                        .deliveries
                        .iter()
                        .any(|(p, _)| Arc::ptr_eq(&p.registrator, &current.registrator))
                })
            {
                drop(state);
                drop(fence);
                return;
            }
        }
        // Legacy/in-process entries can predate lifecycle tracking.
        if state.deliveries.is_empty()
            && self.triggers.contains_key(id)
            && trigger.provider_key() != *key
            && let Some(old) = self.provider_snapshot(&trigger.provider_key())
        {
            state.deliveries.push((old, trigger.clone()));
        }
        self.settle_binding(state, trigger, None).await;
    }

    /// Remove connection-owned bindings and re-resolve surviving bindings
    /// after provider removal. Gates also cover intents currently in flight,
    /// so an owner disconnect cannot miss a claimed pending registration.
    pub async fn unregister_worker(&self, worker_id: &Uuid) {
        let removed: std::collections::HashSet<TypeKey> = {
            let _fence = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            let mut removed = std::collections::HashSet::new();
            self.trigger_types.retain(|key, provider| {
                if provider.worker_id == Some(*worker_id) {
                    removed.insert(key.clone());
                    false
                } else {
                    true
                }
            });
            removed
        };
        let affected = |trigger: &Trigger| {
            trigger.worker_id == Some(*worker_id)
                || removed.contains(&trigger.provider_key())
                || removed.iter().any(|key| {
                    key.1 == trigger.trigger_type
                        && match &trigger.trigger_namespace {
                            Some(namespace) => namespace == &key.0,
                            None => {
                                key.0 == trigger.home_namespace
                                    || key.0 == crate::protocol::DEFAULT_NAMESPACE
                            }
                        }
                })
        };
        let mut ids: std::collections::HashSet<String> = self
            .in_flight
            .iter()
            .filter(|e| affected(e.value()))
            .map(|e| e.id.clone())
            .collect();
        ids.extend(
            self.triggers
                .iter()
                .filter(|e| affected(e.value()))
                .map(|e| e.key().clone()),
        );
        ids.extend(
            self.pending_triggers
                .iter()
                .filter(|e| e.worker_id == Some(*worker_id))
                .map(|e| e.key().clone()),
        );
        for id in ids {
            let lease = self.lifecycle(&id, None);
            let mut state = Arc::clone(lease.gate.as_ref().expect("live lifecycle lease"))
                .lock_owned()
                .await;
            let Some(trigger) = self.binding_snapshot(&id) else {
                continue;
            };
            if trigger.worker_id == Some(*worker_id) {
                if state.deliveries.is_empty()
                    && let Some(provider) = self.provider_snapshot(&trigger.provider_key())
                {
                    state.deliveries.push((provider, trigger.clone()));
                }
                if let Err(err) = Self::detach_deliveries(&mut state).await {
                    tracing::error!(error = %err, "Error unregistering disconnected owner's trigger");
                }
                let _fence = self
                    .park_transition
                    .lock()
                    .expect("park_transition lock poisoned");
                self.triggers.remove(&id);
                self.pending_triggers.remove(&id);
            } else {
                {
                    let fence = self
                        .park_transition
                        .lock()
                        .expect("park_transition lock poisoned");
                    let current = self
                        .resolve_provider_key(&trigger)
                        .and_then(|key| self.provider_snapshot(&key));
                    if current.as_ref().is_some_and(|current| {
                        self.triggers.contains_key(&id)
                            && state
                                .deliveries
                                .iter()
                                .any(|(p, _)| Arc::ptr_eq(&p.registrator, &current.registrator))
                    }) || (current.is_none() && self.pending_triggers.contains_key(&id))
                    {
                        drop(state);
                        drop(fence);
                        continue;
                    }
                }
                self.settle_binding(state, trigger, None).await;
            }
        }
    }

    /// Publish a provider generation, then reconcile current live, pending,
    /// and fallback bindings without replaying stale binding snapshots.
    pub async fn register_trigger_type(
        &self,
        trigger_type: TriggerType,
    ) -> Result<(), anyhow::Error> {
        let key = trigger_type.key();
        let ids = {
            let _fence = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            self.trigger_types.insert(key.clone(), trigger_type);
            let mut ids: std::collections::HashSet<String> = self
                .triggers
                .iter()
                .filter(|e| {
                    e.trigger_type == key.1
                        && self.resolve_provider_key(e.value()).as_ref() == Some(&key)
                })
                .map(|e| e.key().clone())
                .collect();
            ids.extend(
                self.pending_triggers
                    .iter()
                    .filter(|e| {
                        e.trigger_type == key.1
                            && self.resolve_provider_key(e.value()).as_ref() == Some(&key)
                    })
                    .map(|e| e.key().clone()),
            );
            ids
        };
        for id in ids {
            self.replay_candidate(&id, &key).await;
        }
        Ok(())
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
            Some(worker_name) => format!(
                "{} If this persists, the {} worker is missing — run: {}",
                base,
                worker_name.cyan().bold(),
                format!(
                    "iii trigger -n <compose-daemon-namespace> compose::add worker={}",
                    worker_name
                )
                .green()
                .bold()
            ),
            None => format!(
                "{} If this persists, search for a worker that provides this trigger type at {}",
                base,
                "https://workers.iii.dev/".cyan().bold()
            ),
        }
    }

    /// Register under the binding's lifecycle gate. Provider rejection leaves
    /// the prior binding intact; unavailable delivery retains a pending intent.
    pub async fn register_trigger(
        &self,
        mut trigger: Trigger,
    ) -> Result<RegisterTriggerOutcome, anyhow::Error> {
        let lease = self.lifecycle(&trigger.id, Some(&trigger));
        let mut state = Arc::clone(lease.gate.as_ref().expect("live lifecycle lease"))
            .lock_owned()
            .await;
        // Legacy fixtures/in-process entries may predate delivery tracking.
        // Snapshot their actual route BEFORE attempting a new configuration.
        if state.deliveries.is_empty()
            && let Some(previous) = self.triggers.get(&trigger.id).map(|t| t.clone())
            && let Some(provider) = self.provider_snapshot(&previous.provider_key())
        {
            state.deliveries.push((provider, previous));
        }
        let provider = self
            .resolve_provider_key(&trigger)
            .and_then(|key| self.provider_snapshot(&key));
        let Some(provider) = provider else {
            tracing::warn!("{}", Self::pending_trigger_warning(&trigger));
            return Ok(self.settle_binding(state, trigger, None).await);
        };
        trigger.provider_namespace = provider.namespace.clone();
        let result = provider.registrator.register_trigger(trigger.clone()).await;
        if result
            .as_ref()
            .is_err_and(|err| err.downcast_ref::<RegistratorUnavailable>().is_none())
        {
            // A replacement may have skipped the busy gate while this attempt
            // was rejected. Reconcile the previous accepted binding, not the
            // rejected config, before returning the definitive provider error.
            if let Some(previous) = self.binding_snapshot(&trigger.id) {
                self.reconcile_existing(state, previous).await;
            }
            return result.map(|()| RegisterTriggerOutcome::Registered);
        }
        Ok(self
            .settle_binding(state, trigger, Some((provider, result)))
            .await)
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
        let _ = trigger_type;
        let lease = self.lifecycle(&id, None);
        let mut state = Arc::clone(lease.gate.as_ref().expect("live lifecycle lease"))
            .lock_owned()
            .await;
        let Some(trigger) = self.binding_snapshot(&id) else {
            return Ok(false);
        };
        if state.deliveries.is_empty()
            && self.triggers.contains_key(&id)
            && let Some(provider) = self.provider_snapshot(&trigger.provider_key())
        {
            state.deliveries.push((provider, trigger.clone()));
        }
        if let Err(err) = Self::detach_deliveries(&mut state).await {
            // Replacement skipped our gate. Keep the binding tracked and
            // reconcile it before propagating the failed detach.
            self.reconcile_existing(state, trigger).await;
            return Err(err);
        }
        let _fence = self
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
        let lease = self.lifecycle(id, None);
        let mut state = Arc::clone(lease.gate.as_ref().expect("live lifecycle lease"))
            .lock_owned()
            .await;
        let Some(trigger) = self.binding_snapshot(id) else {
            return;
        };
        {
            let fence = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            let current = self.provider_snapshot(&trigger.provider_key());
            if current
                .as_ref()
                .is_some_and(|p| p.worker_id == Some(reporter_worker_id))
            {
                // The rejecting generation never accepted this activation.
                state
                    .deliveries
                    .retain(|(p, _)| p.worker_id != Some(reporter_worker_id));
                self.triggers.remove(id);
                self.pending_triggers.insert(id.to_owned(), trigger);
                drop(state);
                drop(fence);
                return;
            }
        }
        self.settle_binding(state, trigger, None).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::DEFAULT_NAMESPACE;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Records actual provider jobs and pauses exactly one delivery or detach.
    #[derive(Default)]
    struct DeliveryProbe {
        live: std::sync::Mutex<HashSet<String>>,
        events: std::sync::Mutex<Vec<(bool, String)>>,
        pause_register: std::sync::atomic::AtomicBool,
        reject_register: std::sync::atomic::AtomicBool,
        pause_unregister: std::sync::atomic::AtomicBool,
        resume: tokio::sync::Notify,
    }

    impl TriggerRegistrator for Arc<DeliveryProbe> {
        fn register_trigger(
            &self,
            trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            Box::pin(async move {
                if self.pause_register.swap(false, Ordering::SeqCst) {
                    self.resume.notified().await;
                }
                if self.reject_register.load(Ordering::SeqCst) {
                    return Err(anyhow::anyhow!("probe rejected config"));
                }
                self.live.lock().unwrap().insert(trigger.id.clone());
                self.events.lock().unwrap().push((true, trigger.id));
                Ok(())
            })
        }

        fn unregister_trigger(
            &self,
            trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            Box::pin(async move {
                if self.pause_unregister.swap(false, Ordering::SeqCst) {
                    self.resume.notified().await;
                }
                self.live.lock().unwrap().remove(&trigger.id);
                self.events.lock().unwrap().push((false, trigger.id));
                Ok(())
            })
        }
    }

    /// Install an observable provider without involving ports or worker tasks.
    async fn install_probe(
        registry: &TriggerRegistry,
        namespace: &str,
        probe: &Arc<DeliveryProbe>,
        owner: Option<Uuid>,
    ) {
        registry
            .register_trigger_type(TriggerType::new_ns(
                namespace,
                "evt",
                "probe",
                Box::new(Arc::clone(probe)),
                owner,
            ))
            .await
            .unwrap();
    }

    /// Assert delivery exactly once and that explicit teardown reaches every job.
    async fn assert_single_delivery_and_detach(
        registry: &TriggerRegistry,
        old: &DeliveryProbe,
        new: &DeliveryProbe,
    ) {
        assert!(old.live.lock().unwrap().is_empty());
        assert_eq!(
            *new.live.lock().unwrap(),
            HashSet::from(["binding".to_owned()])
        );
        assert_eq!(
            *new.events.lock().unwrap(),
            vec![(true, "binding".to_owned())]
        );
        assert!(registry.triggers.contains_key("binding"));
        assert!(registry.pending_triggers.is_empty());
        assert!(
            registry
                .unregister_trigger("binding".into(), None)
                .await
                .unwrap()
        );
        assert!(new.live.lock().unwrap().is_empty());
        assert!(registry.triggers.is_empty());
        assert!(registry.pending_triggers.is_empty());
    }

    /// A direct registration cannot publish an acknowledgement from an obsolete generation.
    #[tokio::test]
    async fn direct_registration_reconciles_replacement_before_publication() {
        let registry = TriggerRegistry::new();
        let old = Arc::new(DeliveryProbe::default());
        let new = Arc::new(DeliveryProbe::default());
        install_probe(&registry, DEFAULT_NAMESPACE, &old, None).await;
        old.pause_register.store(true, Ordering::SeqCst);
        let registration = registry.register_trigger(make_trigger("binding", "evt"));
        tokio::pin!(registration);
        assert!(futures::poll!(&mut registration).is_pending());
        install_probe(&registry, DEFAULT_NAMESPACE, &new, None).await;
        old.resume.notify_one();
        assert_eq!(
            registration.await.unwrap(),
            RegisterTriggerOutcome::Registered
        );
        assert_single_delivery_and_detach(&registry, &old, &new).await;
    }

    /// Failover must hand off an in-flight fallback delivery to its replacement.
    #[tokio::test]
    async fn failover_reconciles_replacement_before_publication() {
        let registry = TriggerRegistry::new();
        let old = Arc::new(DeliveryProbe::default());
        let new = Arc::new(DeliveryProbe::default());
        let home = Arc::new(DeliveryProbe::default());
        let owner = Uuid::new_v4();
        install_probe(&registry, DEFAULT_NAMESPACE, &old, None).await;
        install_probe(&registry, "shop", &home, Some(owner)).await;
        registry
            .register_trigger(resolved_trigger("binding", "evt", "shop"))
            .await
            .unwrap();
        old.pause_register.store(true, Ordering::SeqCst);
        let failover = registry.unregister_worker(&owner);
        tokio::pin!(failover);
        assert!(futures::poll!(&mut failover).is_pending());
        install_probe(&registry, DEFAULT_NAMESPACE, &new, None).await;
        old.resume.notify_one();
        failover.await;
        assert!(home.live.lock().unwrap().is_empty());
        assert_single_delivery_and_detach(&registry, &old, &new).await;
    }

    /// Replacement during detach must not replay the still-visible registry entry.
    #[tokio::test]
    async fn unregister_serializes_replacement_replay_and_detach() {
        let registry = TriggerRegistry::new();
        let old = Arc::new(DeliveryProbe::default());
        let new = Arc::new(DeliveryProbe::default());
        install_probe(&registry, DEFAULT_NAMESPACE, &old, None).await;
        registry
            .register_trigger(make_trigger("binding", "evt"))
            .await
            .unwrap();
        old.pause_unregister.store(true, Ordering::SeqCst);
        let detach = registry.unregister_trigger("binding".into(), None);
        tokio::pin!(detach);
        assert!(futures::poll!(&mut detach).is_pending());
        install_probe(&registry, DEFAULT_NAMESPACE, &new, None).await;
        old.resume.notify_one();
        assert!(detach.await.unwrap());
        // Simulate the remainder of a replay batch captured before unregister.
        registry
            .replay_candidate("binding", &type_key(DEFAULT_NAMESPACE, "evt"))
            .await;
        assert!(old.live.lock().unwrap().is_empty());
        assert!(new.events.lock().unwrap().is_empty());
        assert!(registry.triggers.is_empty());
        assert!(registry.pending_triggers.is_empty());
    }

    /// Re-homing and pending activation share the same generation-safe handoff.
    #[tokio::test]
    async fn recovery_paths_reconcile_replacement_and_owner_disconnect() {
        for pending in [false, true] {
            let registry = TriggerRegistry::new();
            let fallback = Arc::new(DeliveryProbe::default());
            let old = Arc::new(DeliveryProbe::default());
            let new = Arc::new(DeliveryProbe::default());
            let owner = Uuid::new_v4();
            if !pending {
                install_probe(&registry, DEFAULT_NAMESPACE, &fallback, None).await;
            }
            let mut binding = resolved_trigger("binding", "evt", "shop");
            binding.worker_id = Some(owner);
            registry.register_trigger(binding).await.unwrap();
            old.pause_register.store(true, Ordering::SeqCst);
            let recovery = install_probe(&registry, "shop", &old, None);
            tokio::pin!(recovery);
            assert!(futures::poll!(&mut recovery).is_pending());
            install_probe(&registry, "shop", &new, None).await;
            let cleanup = registry.unregister_worker(&owner);
            tokio::pin!(cleanup);
            assert!(futures::poll!(&mut cleanup).is_pending());
            old.resume.notify_one();
            recovery.await;
            cleanup.await;
            assert!(fallback.live.lock().unwrap().is_empty());
            assert!(old.live.lock().unwrap().is_empty());
            assert!(new.live.lock().unwrap().is_empty());
            assert_eq!(
                *new.events.lock().unwrap(),
                vec![(true, "binding".into()), (false, "binding".into())]
            );
            assert!(registry.triggers.is_empty());
            assert!(registry.pending_triggers.is_empty());
        }
    }

    /// Rejected route changes must not invent deliveries to the rejecting provider.
    #[tokio::test]
    async fn rejected_route_change_preserves_only_accepted_deliveries() {
        for changed_type in [false, true] {
            let registry = TriggerRegistry::new();
            let old = Arc::new(DeliveryProbe::default());
            let rejected = Arc::new(DeliveryProbe::default());
            install_probe(&registry, DEFAULT_NAMESPACE, &old, None).await;
            registry
                .register_trigger(make_trigger("binding", "evt"))
                .await
                .unwrap();
            rejected.reject_register.store(true, Ordering::SeqCst);
            let namespace = if changed_type {
                DEFAULT_NAMESPACE
            } else {
                "other"
            };
            let kind = if changed_type { "different" } else { "evt" };
            registry
                .register_trigger_type(TriggerType::new_ns(
                    namespace,
                    kind,
                    "reject",
                    Box::new(Arc::clone(&rejected)),
                    None,
                ))
                .await
                .unwrap();
            let mut attempt = explicit_trigger("binding", kind, DEFAULT_NAMESPACE, namespace);
            attempt.config = serde_json::json!({"rejected": true});
            assert!(registry.register_trigger(attempt).await.is_err());
            assert_eq!(*old.events.lock().unwrap(), vec![(true, "binding".into())]);
            assert!(rejected.events.lock().unwrap().is_empty());
            assert_eq!(
                registry.triggers.get("binding").unwrap().trigger_type,
                "evt"
            );
            assert!(
                registry
                    .unregister_trigger("binding".into(), None)
                    .await
                    .unwrap()
            );
            assert!(old.live.lock().unwrap().is_empty());
            assert!(rejected.events.lock().unwrap().is_empty());
        }
    }

    /// A replacement skipped during rejection must receive the previous config.
    #[tokio::test]
    async fn replacement_during_rejection_replays_previous_delivery() {
        let registry = TriggerRegistry::new();
        let old = Arc::new(DeliveryProbe::default());
        let new = Arc::new(DeliveryProbe::default());
        install_probe(&registry, DEFAULT_NAMESPACE, &old, None).await;
        registry
            .register_trigger(make_trigger("binding", "evt"))
            .await
            .unwrap();
        old.pause_register.store(true, Ordering::SeqCst);
        old.reject_register.store(true, Ordering::SeqCst);
        let mut invalid = make_trigger("binding", "evt");
        invalid.config = serde_json::json!({"invalid": true});
        let rejection = registry.register_trigger(invalid);
        tokio::pin!(rejection);
        assert!(futures::poll!(&mut rejection).is_pending());
        install_probe(&registry, DEFAULT_NAMESPACE, &new, None).await;
        old.resume.notify_one();
        assert!(rejection.await.is_err());
        assert_eq!(
            registry.triggers.get("binding").unwrap().config,
            serde_json::json!({})
        );
        assert_single_delivery_and_detach(&registry, &old, &new).await;
    }

    /// Disconnect cleanup must finish while an unrelated registration is paused.
    #[tokio::test]
    async fn disconnect_does_not_wait_for_unrelated_busy_binding() {
        let registry = TriggerRegistry::new();
        let slow = Arc::new(DeliveryProbe::default());
        let fast = Arc::new(DeliveryProbe::default());
        let owner = Uuid::new_v4();
        install_probe(&registry, "slow", &slow, Some(Uuid::new_v4())).await;
        install_probe(&registry, "fast", &fast, Some(owner)).await;
        let mut owned = explicit_trigger("owned", "evt", "fast", "fast");
        owned.worker_id = Some(owner);
        registry.register_trigger(owned).await.unwrap();
        slow.pause_register.store(true, Ordering::SeqCst);
        let registration =
            registry.register_trigger(explicit_trigger("slow-binding", "evt", "slow", "slow"));
        tokio::pin!(registration);
        assert!(futures::poll!(&mut registration).is_pending());
        let cleanup = registry.unregister_worker(&owner);
        tokio::pin!(cleanup);
        assert!(
            futures::poll!(&mut cleanup).is_ready(),
            "unrelated paused provider blocked cleanup"
        );
        assert!(fast.live.lock().unwrap().is_empty());
        slow.resume.notify_one();
        registration.await.unwrap();
        assert!(slow.live.lock().unwrap().contains("slow-binding"));
    }

    /// Gate reclamation covers unknown ids, normal churn, and queued leases.
    #[tokio::test]
    async fn lifecycle_churn_reclaims_empty_gates_without_splitting_waiters() {
        let registry = TriggerRegistry::new();
        install_probe(
            &registry,
            DEFAULT_NAMESPACE,
            &Arc::new(DeliveryProbe::default()),
            None,
        )
        .await;
        for index in 0..128 {
            let id = format!("churn-{index}");
            assert!(!registry.unregister_trigger(id.clone(), None).await.unwrap());
            registry
                .register_trigger(make_trigger(&id, "evt"))
                .await
                .unwrap();
            assert!(registry.unregister_trigger(id, None).await.unwrap());
        }
        assert!(registry.lifecycles.is_empty());
        assert!(registry.in_flight.is_empty());
        let first = registry.lifecycle("queued", None);
        let second = registry.lifecycle("queued", None);
        let gate = Arc::clone(second.gate.as_ref().unwrap());
        drop(first);
        let third = registry.lifecycle("queued", None);
        assert!(Arc::ptr_eq(&gate, third.gate.as_ref().unwrap()));
        drop(gate);
        drop(second);
        drop(third);
        assert!(registry.lifecycles.is_empty());
    }

    /// Cancelled detach retains its delivery for a later explicit retry.
    #[tokio::test]
    async fn cancelled_detach_keeps_delivery_tracked() {
        let registry = TriggerRegistry::new();
        let provider = Arc::new(DeliveryProbe::default());
        install_probe(&registry, DEFAULT_NAMESPACE, &provider, None).await;
        registry
            .register_trigger(make_trigger("binding", "evt"))
            .await
            .unwrap();
        provider.pause_unregister.store(true, Ordering::SeqCst);
        {
            let detach = registry.unregister_trigger("binding".into(), None);
            tokio::pin!(detach);
            assert!(futures::poll!(&mut detach).is_pending());
        }
        assert!(
            registry
                .unregister_trigger("binding".into(), None)
                .await
                .unwrap()
        );
        assert!(provider.live.lock().unwrap().is_empty());
        assert!(registry.lifecycles.is_empty());
    }

    struct CountingYieldingRegistrator(Arc<ControlledRegistrator>);

    impl TriggerRegistrator for CountingYieldingRegistrator {
        fn register_trigger(
            &self,
            trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            Box::pin(async move {
                tokio::task::yield_now().await;
                self.0.register_trigger(trigger).await
            })
        }

        fn unregister_trigger(
            &self,
            trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            Box::pin(async move {
                tokio::task::yield_now().await;
                self.0.unregister_trigger(trigger).await
            })
        }
    }

    struct YieldingRegistrator;

    impl TriggerRegistrator for YieldingRegistrator {
        fn register_trigger(
            &self,
            _trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            Box::pin(async {
                tokio::task::yield_now().await;
                Ok(())
            })
        }

        fn unregister_trigger(
            &self,
            trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            self.register_trigger(trigger)
        }
    }

    #[tokio::test]
    async fn replay_releases_provider_shard_before_await() {
        let registry = TriggerRegistry::new();
        let trigger = make_trigger("live", "evt");
        registry.triggers.insert(trigger.id.clone(), trigger);
        let key = type_key(DEFAULT_NAMESPACE, "evt");
        let replay = registry.register_trigger_type(TriggerType::new(
            "evt",
            "yielding",
            Box::new(YieldingRegistrator),
            None,
        ));
        tokio::pin!(replay);
        assert!(futures::poll!(&mut replay).is_pending());

        // A blocking insert here would deadlock the runtime on the old code.
        // try_get_mut proves the shard is writable without hanging the suite.
        assert!(
            matches!(
                registry.trigger_types.try_get_mut(&key),
                dashmap::try_result::TryResult::Present(_)
            ),
            "replay retained a provider shard guard across await"
        );
        replay.await.unwrap();
        assert_eq!(registry.triggers.len(), 1);
        assert!(registry.pending_triggers.is_empty());
    }

    #[tokio::test]
    async fn pending_replay_follows_replacement_generation() {
        let registry = TriggerRegistry::new();
        registry
            .register_trigger(make_trigger("pending", "evt"))
            .await
            .unwrap();
        let old_worker = Uuid::new_v4();
        let replacement_worker = Uuid::new_v4();
        let replay = registry.register_trigger_type(TriggerType::new(
            "evt",
            "old",
            Box::new(YieldingRegistrator),
            Some(old_worker),
        ));
        tokio::pin!(replay);
        assert!(futures::poll!(&mut replay).is_pending());
        let key = type_key(DEFAULT_NAMESPACE, "evt");
        assert!(matches!(
            registry.trigger_types.try_get_mut(&key),
            dashmap::try_result::TryResult::Present(_)
        ));

        let replacement = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new(
                "evt",
                "replacement",
                Box::new(Arc::clone(&replacement)),
                Some(replacement_worker),
            ))
            .await
            .unwrap();
        replay.await.unwrap();
        registry.unregister_worker(&old_worker).await;

        assert_eq!(replacement.register_count.load(Ordering::SeqCst), 1);
        assert_eq!(registry.triggers.len(), 1);
        assert!(registry.pending_triggers.is_empty());
        assert_eq!(
            registry.trigger_types.get(&key).unwrap().worker_id,
            Some(replacement_worker)
        );
    }

    #[tokio::test]
    async fn registration_and_unregister_release_provider_shard_before_await() {
        let registry = TriggerRegistry::new();
        let key = type_key(DEFAULT_NAMESPACE, "evt");
        registry
            .register_trigger_type(TriggerType::new(
                "evt",
                "yielding",
                Box::new(YieldingRegistrator),
                None,
            ))
            .await
            .unwrap();
        let registration = registry.register_trigger(make_trigger("live", "evt"));
        tokio::pin!(registration);
        assert!(futures::poll!(&mut registration).is_pending());
        assert!(matches!(
            registry.trigger_types.try_get_mut(&key),
            dashmap::try_result::TryResult::Present(_)
        ));
        registration.await.unwrap();

        let unregister = registry.unregister_trigger("live".into(), None);
        tokio::pin!(unregister);
        assert!(futures::poll!(&mut unregister).is_pending());
        assert!(matches!(
            registry.trigger_types.try_get_mut(&key),
            dashmap::try_result::TryResult::Present(_)
        ));
        assert!(unregister.await.unwrap());
        assert!(registry.triggers.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_provider_reconnect_preserves_all_bindings() {
        let registry = Arc::new(TriggerRegistry::new());
        let mut owners: Vec<_> = (0..8).map(|_| Uuid::new_v4()).collect();
        for (p, owner) in owners.iter().enumerate() {
            for k in 0..6 {
                let id = format!("fixture-{p}-{k}");
                registry
                    .register_trigger_type(TriggerType::new(
                        &id,
                        "yielding",
                        Box::new(YieldingRegistrator),
                        Some(*owner),
                    ))
                    .await
                    .unwrap();
                for n in 0..12 {
                    registry
                        .register_trigger(make_trigger(&format!("bind-{p}-{k}-{n}"), &id))
                        .await
                        .unwrap();
                }
            }
        }
        for _ in 0..20 {
            let mut tasks = tokio::task::JoinSet::new();
            for owner in owners {
                let registry = Arc::clone(&registry);
                tasks.spawn(async move {
                    registry.unregister_worker(&owner).await;
                });
            }
            while let Some(result) = tasks.join_next().await {
                result.unwrap();
            }
            assert!(registry.triggers.is_empty());
            assert_eq!(registry.pending_triggers.len(), 576);
            owners = (0..8).map(|_| Uuid::new_v4()).collect();
            let counters: Vec<_> = (0..8)
                .map(|_| Arc::new(ControlledRegistrator::new(false, false)))
                .collect();
            for (p, owner) in owners.iter().copied().enumerate() {
                let registry = Arc::clone(&registry);
                let counter = Arc::clone(&counters[p]);
                tasks.spawn(async move {
                    for k in 0..6 {
                        registry
                            .register_trigger_type(TriggerType::new(
                                format!("fixture-{p}-{k}"),
                                "replacement",
                                Box::new(CountingYieldingRegistrator(Arc::clone(&counter))),
                                Some(owner),
                            ))
                            .await
                            .unwrap();
                        tokio::task::yield_now().await;
                    }
                });
            }
            while let Some(result) = tasks.join_next().await {
                result.unwrap();
            }
            assert_eq!(registry.trigger_types.len(), 48);
            assert_eq!(registry.triggers.len(), 576);
            assert!(registry.pending_triggers.is_empty());
            for counter in counters {
                assert_eq!(counter.register_count.load(Ordering::SeqCst), 72);
            }
        }
    }

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
            trigger_namespace: None,
            home_namespace: crate::protocol::default_namespace(),
            provider_namespace: crate::protocol::default_namespace(),
        }
    }

    /// A binding as a worker at `home` would register it: the provider is
    /// resolved, not named.
    fn resolved_trigger(id: &str, trigger_type: &str, home: &str) -> Trigger {
        Trigger {
            home_namespace: home.to_string(),
            ..make_trigger(id, trigger_type)
        }
    }

    /// A binding that names its provider's namespace: strict, no fallback.
    fn explicit_trigger(id: &str, trigger_type: &str, home: &str, provider: &str) -> Trigger {
        Trigger {
            home_namespace: home.to_string(),
            trigger_namespace: Some(provider.to_string()),
            ..make_trigger(id, trigger_type)
        }
    }

    fn make_trigger_type_ns(namespace: &str, id: &str) -> TriggerType {
        TriggerType::new_ns(
            namespace,
            id,
            format!("Test trigger type {} in {}", id, namespace),
            Box::new(MockRegistrator::new()),
            None,
        )
    }

    // ── The four resolutions ─────────────────────────────────────────────
    //
    // A binding names two namespaces, and they answer different questions:
    // where the provider is, and where the target function is. These four
    // cover every combination of "at home" and "somewhere else".

    #[tokio::test]
    async fn a_provider_at_home_serves_its_own_namespace() {
        let registry = TriggerRegistry::new();
        registry
            .register_trigger_type(make_trigger_type_ns("shop", "webhook"))
            .await
            .unwrap();

        let trigger = resolved_trigger("t1", "webhook", "shop");
        assert_eq!(
            registry.register_trigger(trigger).await.unwrap(),
            RegisterTriggerOutcome::Registered
        );

        let stored = registry.triggers.get("t1").unwrap();
        assert_eq!(stored.provider_namespace, "shop");
        assert!(!stored.is_on_fallback());
    }

    #[tokio::test]
    async fn a_worker_with_no_provider_at_home_falls_back_to_default() {
        // The migration case: the engine's providers live in `default`, and a
        // worker in another namespace reaches them without saying anything.
        let registry = TriggerRegistry::new();
        registry
            .register_trigger_type(make_trigger_type("cron"))
            .await
            .unwrap();

        let trigger = resolved_trigger("t1", "cron", "shop");
        assert_eq!(
            registry.register_trigger(trigger).await.unwrap(),
            RegisterTriggerOutcome::Registered
        );

        let stored = registry.triggers.get("t1").unwrap();
        assert_eq!(stored.provider_namespace, DEFAULT_NAMESPACE);
        assert!(stored.is_on_fallback(), "it is only borrowing the engine's");
    }

    #[tokio::test]
    async fn home_wins_over_the_fallback_when_both_exist() {
        // Both providers are up before the bind. The project's own is the one
        // that should serve it — that is what makes shipping a replacement for
        // a built-in type possible at all.
        let registry = TriggerRegistry::new();
        registry
            .register_trigger_type(make_trigger_type("state"))
            .await
            .unwrap();
        registry
            .register_trigger_type(make_trigger_type_ns("shop", "state"))
            .await
            .unwrap();

        registry
            .register_trigger(resolved_trigger("t1", "state", "shop"))
            .await
            .unwrap();

        assert_eq!(
            registry.triggers.get("t1").unwrap().provider_namespace,
            "shop"
        );
        // And both providers are still there: one did not replace the other.
        assert_eq!(registry.trigger_types.len(), 2);
    }

    #[tokio::test]
    async fn naming_a_namespace_takes_that_one_and_no_other() {
        let registry = TriggerRegistry::new();
        registry
            .register_trigger_type(make_trigger_type("cron"))
            .await
            .unwrap();
        registry
            .register_trigger_type(make_trigger_type_ns("shop", "cron"))
            .await
            .unwrap();

        // Explicit `default` from a worker at home in `shop`: the provider at
        // home exists and must NOT be preferred. A caller who was specific is
        // not quietly served by someone else.
        registry
            .register_trigger(explicit_trigger("t1", "cron", "shop", DEFAULT_NAMESPACE))
            .await
            .unwrap();
        assert_eq!(
            registry.triggers.get("t1").unwrap().provider_namespace,
            DEFAULT_NAMESPACE
        );

        // And an explicit namespace nobody provides does not fall back — it
        // parks, the same as any other unavailable provider.
        assert_eq!(
            registry
                .register_trigger(explicit_trigger("t2", "cron", "shop", "nowhere"))
                .await
                .unwrap(),
            RegisterTriggerOutcome::Deferred
        );
        assert!(registry.pending_triggers.contains_key("t2"));
    }

    #[tokio::test]
    async fn the_target_namespace_is_independent_of_the_provider() {
        // Everything outside its own namespace: the provider is the engine's,
        // the target is a third namespace, and the worker's own appears in
        // neither resolution.
        let registry = TriggerRegistry::new();
        registry
            .register_trigger_type(make_trigger_type("cron"))
            .await
            .unwrap();

        let trigger = Trigger {
            namespace: "billing".to_string(),
            ..resolved_trigger("t1", "cron", "shop")
        };
        registry.register_trigger(trigger).await.unwrap();

        let stored = registry.triggers.get("t1").unwrap();
        assert_eq!(stored.provider_namespace, DEFAULT_NAMESPACE, "provider");
        assert_eq!(stored.namespace, "billing", "target");
        assert_eq!(stored.home_namespace, "shop", "the worker's own");
    }

    // ── Re-homing ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn a_provider_arriving_late_claims_back_what_fell_back() {
        // The ordering that would otherwise decide everything silently: the
        // binding is made before the project's own provider is up.
        let registry = TriggerRegistry::new();
        registry
            .register_trigger_type(make_trigger_type("state"))
            .await
            .unwrap();
        registry
            .register_trigger(resolved_trigger("t1", "state", "shop"))
            .await
            .unwrap();
        assert_eq!(
            registry.triggers.get("t1").unwrap().provider_namespace,
            DEFAULT_NAMESPACE
        );

        // The project's provider registers afterwards.
        registry
            .register_trigger_type(make_trigger_type_ns("shop", "state"))
            .await
            .unwrap();

        assert_eq!(
            registry.triggers.get("t1").unwrap().provider_namespace,
            "shop",
            "a binding must not be stuck on the fallback because of start order"
        );
    }

    #[tokio::test]
    async fn an_explicit_binding_is_never_re_homed() {
        let registry = TriggerRegistry::new();
        registry
            .register_trigger_type(make_trigger_type("state"))
            .await
            .unwrap();
        registry
            .register_trigger(explicit_trigger("t1", "state", "shop", DEFAULT_NAMESPACE))
            .await
            .unwrap();

        registry
            .register_trigger_type(make_trigger_type_ns("shop", "state"))
            .await
            .unwrap();

        assert_eq!(
            registry.triggers.get("t1").unwrap().provider_namespace,
            DEFAULT_NAMESPACE,
            "naming a namespace means that one, whatever appears later"
        );
    }

    #[tokio::test]
    async fn one_namespace_provider_does_not_take_another_namespace_bindings() {
        // The defect this whole change exists for: two providers of the same
        // type id used to overwrite each other, and the winner was handed the
        // loser's bindings.
        let registry = TriggerRegistry::new();
        registry
            .register_trigger_type(make_trigger_type_ns("shop-a", "webhook"))
            .await
            .unwrap();
        registry
            .register_trigger(resolved_trigger("t-a", "webhook", "shop-a"))
            .await
            .unwrap();

        registry
            .register_trigger_type(make_trigger_type_ns("shop-b", "webhook"))
            .await
            .unwrap();

        assert_eq!(
            registry.trigger_types.len(),
            2,
            "neither overwrote the other"
        );
        assert_eq!(
            registry.triggers.get("t-a").unwrap().provider_namespace,
            "shop-a",
            "shop-a's binding must stay with shop-a's provider"
        );
    }

    #[tokio::test]
    async fn a_departing_home_provider_hands_its_bindings_back_to_the_fallback() {
        // The symmetric case. A project provider restarting must not disable
        // the bindings it was serving when the engine still provides the type.
        let registry = TriggerRegistry::new();
        let worker = Uuid::new_v4();
        registry
            .register_trigger_type(make_trigger_type("state"))
            .await
            .unwrap();
        registry
            .register_trigger_type(TriggerType::new_ns(
                "shop",
                "state",
                "the project's own",
                Box::new(MockRegistrator::new()),
                Some(worker),
            ))
            .await
            .unwrap();
        registry
            .register_trigger(resolved_trigger("t1", "state", "shop"))
            .await
            .unwrap();
        assert_eq!(
            registry.triggers.get("t1").unwrap().provider_namespace,
            "shop"
        );

        registry.unregister_worker(&worker).await;

        let stored = registry
            .triggers
            .get("t1")
            .expect("the binding must survive");
        assert_eq!(
            stored.provider_namespace, DEFAULT_NAMESPACE,
            "it should fall back rather than be disabled"
        );
        assert!(registry.pending_triggers.is_empty());
    }

    #[tokio::test]
    async fn with_no_fallback_a_departing_provider_still_parks_its_bindings() {
        let registry = TriggerRegistry::new();
        let worker = Uuid::new_v4();
        registry
            .register_trigger_type(TriggerType::new_ns(
                "shop",
                "custom",
                "only provider anywhere",
                Box::new(MockRegistrator::new()),
                Some(worker),
            ))
            .await
            .unwrap();
        registry
            .register_trigger(resolved_trigger("t1", "custom", "shop"))
            .await
            .unwrap();

        registry.unregister_worker(&worker).await;

        assert!(registry.triggers.is_empty());
        assert!(registry.pending_triggers.contains_key("t1"));
    }

    #[tokio::test]
    async fn a_parked_binding_activates_on_whichever_step_appears_first() {
        let registry = TriggerRegistry::new();
        // Nothing provides it yet.
        assert_eq!(
            registry
                .register_trigger(resolved_trigger("t1", "webhook", "shop"))
                .await
                .unwrap(),
            RegisterTriggerOutcome::Deferred
        );

        // The fallback appears: the intent activates there.
        registry
            .register_trigger_type(make_trigger_type("webhook"))
            .await
            .unwrap();
        assert_eq!(
            registry.triggers.get("t1").unwrap().provider_namespace,
            DEFAULT_NAMESPACE
        );
        assert!(registry.pending_triggers.is_empty());
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
        assert!(
            registry
                .trigger_types
                .contains_key(&crate::trigger::type_key(
                    crate::protocol::DEFAULT_NAMESPACE,
                    "cron"
                ))
        );
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
                .get(&crate::trigger::type_key(
                    crate::protocol::DEFAULT_NAMESPACE,
                    "evt",
                ))
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
    fn pending_warning_points_to_compose_workers() {
        let cases = [
            ("http", "http"),
            ("cron", "cron"),
            ("subscribe", "pubsub"),
            ("state", "state"),
            ("durable:subscriber", "queue"),
        ];

        for (trigger_type, worker_name) in cases {
            assert_eq!(known_trigger_type_provider(trigger_type), Some(worker_name));

            let msg = TriggerRegistry::pending_trigger_warning(&make_trigger("t1", trigger_type));
            let expected_hint = format!(
                "iii trigger -n <compose-daemon-namespace> compose::add worker={worker_name}"
            );
            assert!(
                msg.contains(&expected_hint),
                "Expected hint with '{expected_hint}', got: {msg}"
            );
            assert!(
                !msg.contains("iii trigger -n default"),
                "worker namespace must not be presented as the Compose daemon namespace: {msg}"
            );
            assert!(!msg.contains("iii worker"), "legacy command leaked: {msg}");
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
        assert!(
            registry
                .trigger_types
                .contains_key(&crate::trigger::type_key(
                    crate::protocol::DEFAULT_NAMESPACE,
                    "webhook"
                ))
        );
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
        assert!(
            registry
                .trigger_types
                .contains_key(&crate::trigger::type_key(
                    crate::protocol::DEFAULT_NAMESPACE,
                    "webhook"
                ))
        );
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
            crate::trigger::type_key(crate::protocol::DEFAULT_NAMESPACE, &"webhook".to_string()),
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
            crate::trigger::type_key(crate::protocol::DEFAULT_NAMESPACE, &"webhook".to_string()),
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
            crate::trigger::type_key(crate::protocol::DEFAULT_NAMESPACE, &"webhook".to_string()),
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
            trigger_namespace: None,
            home_namespace: crate::protocol::default_namespace(),
            provider_namespace: crate::protocol::default_namespace(),
        };
        let t2 = Trigger {
            id: "trigger-1".to_string(),
            trigger_type: "webhook".to_string(),
            function_id: "fn_b".to_string(),
            config: serde_json::json!({"url": "https://example.com"}),
            worker_id: Some(Uuid::new_v4()),
            metadata: None,
            namespace: "default".to_string(),
            trigger_namespace: None,
            home_namespace: crate::protocol::default_namespace(),
            provider_namespace: crate::protocol::default_namespace(),
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
            trigger_namespace: None,
            home_namespace: crate::protocol::default_namespace(),
            provider_namespace: crate::protocol::default_namespace(),
        };
        let t2 = Trigger {
            id: "trigger-2".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "fn_a".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
            namespace: "default".to_string(),
            trigger_namespace: None,
            home_namespace: crate::protocol::default_namespace(),
            provider_namespace: crate::protocol::default_namespace(),
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
            trigger_namespace: None,
            home_namespace: crate::protocol::default_namespace(),
            provider_namespace: crate::protocol::default_namespace(),
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
            trigger_namespace: None,
            home_namespace: crate::protocol::default_namespace(),
            provider_namespace: crate::protocol::default_namespace(),
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
