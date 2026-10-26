// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, ops::Deref, pin::Pin, sync::Arc};

use colored::Colorize;
use dashmap::{
    DashMap,
    mapref::{multiple::RefMulti, one::Ref},
};
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
    /// Shared so a caller can take it out of the registry map before awaiting
    /// it: a map guard must never be held across an `.await`.
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
            registrator: registrator.into(),
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

/// A read-only trigger entry that keeps its registry shard locked.
pub struct TriggerRef<'a>(Ref<'a, String, Arc<Trigger>>);

impl TriggerRef<'_> {
    pub fn key(&self) -> &String {
        self.0.key()
    }

    pub fn value(&self) -> &Trigger {
        self.0.value().as_ref()
    }
}

impl Deref for TriggerRef<'_> {
    type Target = Trigger;

    fn deref(&self) -> &Self::Target {
        self.value()
    }
}

/// A read-only trigger entry returned by registry iteration.
pub struct TriggerIterRef<'a>(RefMulti<'a, String, Arc<Trigger>>);

impl TriggerIterRef<'_> {
    pub fn key(&self) -> &String {
        self.0.key()
    }

    pub fn value(&self) -> &Trigger {
        self.0.value().as_ref()
    }
}

impl Deref for TriggerIterRef<'_> {
    type Target = Trigger;

    fn deref(&self) -> &Self::Target {
        self.value()
    }
}

/// Live triggers with an index for dispatch by trigger type.
///
/// Both maps share each immutable trigger, including its JSON metadata.
/// All writes hold the id's shard lock while updating the type index, so a
/// concurrent replacement or removal cannot leave an old destination indexed.
/// The id map is private and exposes no mutable references. Dispatch reads only
/// the type index and releases its guard before calling a handler.
#[derive(Default)]
pub struct TriggerStore {
    by_id: DashMap<String, Arc<Trigger>>,
    by_type: DashMap<String, HashMap<String, Arc<Trigger>>>,
}

impl TriggerStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn contains_key(&self, id: &str) -> bool {
        self.by_id.contains_key(id)
    }

    pub fn get(&self, id: &str) -> Option<TriggerRef<'_>> {
        // Keep DashMap's original guard so this read handle remains Send.
        self.by_id.get(id).map(TriggerRef)
    }

    pub fn iter(&self) -> impl Iterator<Item = TriggerIterRef<'_>> {
        self.by_id.iter().map(TriggerIterRef)
    }

    pub fn insert(&self, id: String, trigger: Trigger) -> Option<Trigger> {
        use dashmap::mapref::entry::Entry;

        let trigger = Arc::new(trigger);
        match self.by_id.entry(id) {
            Entry::Occupied(mut entry) => {
                if entry.get().trigger_type != trigger.trigger_type {
                    self.remove_target(entry.key(), &entry.get().trigger_type);
                }
                self.insert_target(entry.key(), &trigger);
                Some(Arc::unwrap_or_clone(entry.insert(trigger)))
            }
            Entry::Vacant(entry) => {
                self.insert_target(entry.key(), &trigger);
                entry.insert(trigger);
                None
            }
        }
    }

    pub fn remove(&self, id: &str) -> Option<(String, Trigger)> {
        self.remove_if(id, |_, _| true)
    }

    pub fn remove_if(
        &self,
        id: &str,
        predicate: impl FnOnce(&String, &Trigger) -> bool,
    ) -> Option<(String, Trigger)> {
        self.by_id
            .remove_if(id, |key, trigger| {
                if !predicate(key, trigger) {
                    return false;
                }
                self.remove_target(key, &trigger.trigger_type);
                true
            })
            .map(|(id, trigger)| (id, Arc::unwrap_or_clone(trigger)))
    }

    pub(crate) fn targets_for_type(&self, trigger_type: &str) -> Vec<Arc<Trigger>> {
        self.by_type
            .get(trigger_type)
            .map(|targets| targets.values().map(Arc::clone).collect())
            .unwrap_or_default()
    }

    fn insert_target(&self, id: &str, trigger: &Arc<Trigger>) {
        self.by_type
            .entry(trigger.trigger_type.clone())
            .or_default()
            .insert(id.to_string(), Arc::clone(trigger));
    }

    fn remove_target(&self, id: &str, trigger_type: &str) {
        // Remove an empty bucket under the same lock as its last entry, so an
        // insertion into that type cannot be lost between the two operations.
        self.by_type.remove_if_mut(trigger_type, |_, targets| {
            targets.remove(id);
            targets.is_empty()
        });
    }
}

/// A provider operation that may finish after its binding is cancelled.
struct TriggerTransition {
    token: u64,
    worker_id: Option<Uuid>,
    removing: bool,
    /// The destination is authoritative while a move still exposes its source
    /// in the live map. A late repair must not reactivate that source.
    target: Arc<Trigger>,
}

/// An aborted caller must not leave a token that blocks later provider work.
struct TriggerTransitionGuard<'a> {
    registry: &'a TriggerRegistry,
    target: Trigger,
    token: u64,
    registrator: Option<Arc<dyn TriggerRegistrator>>,
    removing: bool,
    source: Option<(Trigger, Arc<dyn TriggerRegistrator>)>,
    armed: bool,
}

impl Drop for TriggerTransitionGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if self.removing {
            self.registry.recover_abandoned_removal(
                &self.target.id,
                self.token,
                self.registrator.as_ref(),
            );
        } else if let Some(registrator) = &self.registrator {
            self.registry.recover_abandoned_activation(
                &self.target,
                self.token,
                registrator,
                self.source.as_ref(),
            );
        }
    }
}

#[derive(Clone, Default)]
pub struct TriggerRegistry {
    /// Providers, by `(namespace, type id)`.
    pub trigger_types: Arc<DashMap<TypeKey, TriggerType>>,
    pub triggers: Arc<TriggerStore>,
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
    pub park_transition: Arc<std::sync::Mutex<()>>,
    /// Bindings being moved, activated or removed. Tokens prevent a stale
    /// operation from publishing over a replacement. Ownership remains
    /// visible even while a pending intent is outside both binding maps.
    failing_over: Arc<DashMap<String, TriggerTransition>>,
    failover_tokens: Arc<std::sync::atomic::AtomicU64>,
    /// How long a failover waits for a provider to take a binding; 0 means
    /// [`FAILOVER_REPLAY_TIMEOUT`].
    failover_replay_timeout_ms: Arc<std::sync::atomic::AtomicU64>,
}

/// Bindings claimed for a failover by
/// [`TriggerRegistry::take_trigger_type_if_provider`], each with its move's
/// token.
#[must_use = "claimed bindings stay in flight until `fail_over_orphans` moves them"]
pub struct Orphans(Vec<(Trigger, u64)>);

/// How long a failover waits for the provider it moves a binding to: a wedged
/// provider must not hold up the connection whose departure (or
/// `UnregisterTriggerType`) caused the failover.
pub const FAILOVER_REPLAY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

impl TriggerRegistry {
    pub fn new() -> Self {
        Self {
            trigger_types: Arc::new(DashMap::new()),
            triggers: Arc::new(TriggerStore::new()),
            pending_triggers: Arc::new(DashMap::new()),
            park_transition: Arc::new(std::sync::Mutex::new(())),
            failing_over: Arc::new(DashMap::new()),
            failover_tokens: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            failover_replay_timeout_ms: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Overrides [`FAILOVER_REPLAY_TIMEOUT`].
    pub fn set_failover_replay_timeout(&self, timeout: std::time::Duration) {
        self.failover_replay_timeout_ms.store(
            (timeout.as_millis() as u64).max(1),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    fn failover_replay_timeout(&self) -> std::time::Duration {
        match self
            .failover_replay_timeout_ms
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            0 => FAILOVER_REPLAY_TIMEOUT,
            ms => std::time::Duration::from_millis(ms),
        }
    }

    /// Ends any failover of `id` in flight: the binding was replaced,
    /// unregistered or parked by someone else.
    fn interrupt_failover(&self, id: &str) -> bool {
        self.failing_over.remove(id).is_some()
    }

    /// Call with `park_transition` held, together with the binding map changes.
    fn begin_transition(&self, trigger: &Trigger, removing: bool) -> u64 {
        let token = self
            .failover_tokens
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        self.failing_over.insert(
            trigger.id.clone(),
            TriggerTransition {
                token,
                worker_id: trigger.worker_id,
                removing,
                target: Arc::new(trigger.clone()),
            },
        );
        token
    }

    fn finish_transition(&self, id: &str, token: u64) -> bool {
        self.failing_over
            .remove_if(id, |_, current| current.token == token)
            .is_some()
    }

    /// A failed or cancelled removal still owns its binding. Repair any job
    /// another compensation removed, or resume failover if its provider left.
    /// The task owns a shared registry handle so cancelling the caller cannot
    /// cancel this recovery as well.
    fn recover_abandoned_removal(
        &self,
        id: &str,
        token: u64,
        registrator: Option<&Arc<dyn TriggerRegistrator>>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        let (trigger, token, runtime) = {
            let _park = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            let (_, operation) = self
                .failing_over
                .remove_if(id, |_, operation| operation.token == token)?;
            if !operation.removing {
                return None;
            }
            let trigger = self
                .triggers
                .get(id)
                .map(|current| current.value().clone())
                .or_else(|| self.pending_triggers.get(id).map(|pending| pending.clone()))?;
            let current = self.provider_registrator(&trigger.provider_key());
            let same_provider = matches!((registrator, current),
                (Some(before), Some(after)) if Arc::ptr_eq(before, &after));
            let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                self.triggers.remove(id);
                self.pending_triggers.insert(id.to_string(), trigger);
                return None;
            };
            let token = if same_provider {
                None
            } else {
                self.pending_triggers.remove(id);
                Some(self.begin_transition(&trigger, false))
            };
            (trigger, token, runtime)
        };
        let registry = self.clone();
        let registrator = registrator.cloned();
        Some(runtime.spawn(async move {
            if let Some(token) = token {
                registry.fail_over(trigger, token).await;
            } else if let Some(registrator) = registrator {
                registry
                    .settle_superseded_provider(&trigger, &registrator)
                    .await;
            }
        }))
    }

    /// Compensation can deliver a provisional target before its activation
    /// finishes. On cancellation keep the intent tracked and settle both
    /// providers against the surviving maps, even if the caller is gone.
    fn recover_abandoned_activation(
        &self,
        target: &Trigger,
        token: u64,
        registrator: &Arc<dyn TriggerRegistrator>,
        source: Option<&(Trigger, Arc<dyn TriggerRegistrator>)>,
    ) {
        {
            let _park = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            if let Some((_, operation)) = self
                .failing_over
                .remove_if(&target.id, |_, operation| operation.token == token)
            {
                if source.is_none() {
                    self.triggers.remove(&target.id);
                    self.pending_triggers
                        .insert(target.id.clone(), operation.target.as_ref().clone());
                } else if !self.triggers.contains_key(&target.id)
                    && !self.pending_triggers.contains_key(&target.id)
                {
                    self.pending_triggers
                        .insert(target.id.clone(), operation.target.as_ref().clone());
                }
            }
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let registry = self.clone();
        let target = target.clone();
        let registrator = Arc::clone(registrator);
        let source = source.cloned();
        runtime.spawn(async move {
            registry
                .settle_superseded_provider(&target, &registrator)
                .await;
            if let Some((trigger, provider)) = source {
                registry
                    .settle_superseded_provider(&trigger, &provider)
                    .await;
            }
        });
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

    /// Bindings that fell back to [`DEFAULT_NAMESPACE`] and would rather have
    /// the provider that just registered at `namespace`.
    ///
    /// Without this the resolution is order-dependent: a worker that binds
    /// before its project's provider announces itself lands on the engine's
    /// one and stays there for good — so whether a project's own provider is
    /// used comes down to which container started first, which is not
    /// something anyone can see or control.
    fn fallback_bindings_for(&self, namespace: &str, type_id: &str) -> Vec<Trigger> {
        self.triggers
            .iter()
            .filter(|pair| {
                let trigger = pair.value();
                trigger.trigger_type == type_id
                    && trigger.home_namespace == namespace
                    && trigger.is_on_fallback()
                    && !self.failing_over.contains_key(pair.key())
            })
            .map(|pair| pair.value().clone())
            .collect()
    }

    /// Move fallback bindings onto the provider that just registered at home.
    ///
    /// The old provider must let go first. Keep the binding tracked there if
    /// detaching fails. A cancelled move must not publish or park its snapshot.
    async fn rehome_fallback_bindings(
        &self,
        key: &TypeKey,
        registrator: &Arc<dyn TriggerRegistrator>,
    ) {
        let candidates = self.fallback_bindings_for(&key.0, &key.1);
        if candidates.is_empty() {
            return;
        }

        for candidate in candidates {
            let (mut trigger, token) = {
                let _park = self
                    .park_transition
                    .lock()
                    .expect("park_transition lock poisoned");
                let Some(current) = self.triggers.get(&candidate.id) else {
                    continue;
                };
                if !current.is_on_fallback()
                    || current.home_namespace != key.0
                    || current.trigger_type != key.1
                    || self.failing_over.contains_key(&candidate.id)
                {
                    continue;
                }
                let trigger = current.value().clone();
                drop(current);
                let mut target = trigger.clone();
                target.provider_namespace = key.0.clone();
                let token = self.begin_transition(&target, false);
                (trigger, token)
            };
            let previous = trigger.provider_key();
            let old = self.provider_registrator(&previous);
            let mut operation = TriggerTransitionGuard {
                registry: self,
                target: Trigger {
                    provider_namespace: key.0.clone(),
                    ..trigger.clone()
                },
                token,
                registrator: Some(Arc::clone(registrator)),
                removing: false,
                source: old.as_ref().map(|old| (trigger.clone(), Arc::clone(old))),
                armed: true,
            };
            if let Some(old) = &old {
                if let Err(err) = old.unregister_trigger(trigger.clone()).await {
                    {
                        let _park = self
                            .park_transition
                            .lock()
                            .expect("park_transition lock poisoned");
                        self.finish_transition(&trigger.id, token);
                    }
                    tracing::warn!(
                        error = %err,
                        trigger_id = %trigger.id,
                        "Could not detach a fallback binding; keeping its registry entry"
                    );
                    // A concurrent repair may have detached the source after
                    // observing our destination. Restore the surviving binding
                    // now that this move failed, or honour its cancellation.
                    let mut destination = trigger.clone();
                    destination.provider_namespace = key.0.clone();
                    self.settle_superseded_provider(&destination, registrator)
                        .await;
                    self.settle_superseded_provider(&trigger, old).await;
                    operation.armed = false;
                    continue;
                }
                let current = self
                    .failing_over
                    .get(&trigger.id)
                    .is_some_and(|operation| operation.token == token);
                if !current {
                    self.settle_superseded_provider(&operation.target, registrator)
                        .await;
                    self.settle_superseded_provider(&trigger, old).await;
                    operation.armed = false;
                    continue;
                }
            }

            trigger.provider_namespace = key.0.clone();
            let replayed = registrator.replay_trigger(trigger.clone()).await;
            let settled = {
                let _park = self
                    .park_transition
                    .lock()
                    .expect("park_transition lock poisoned");
                if self.finish_transition(&trigger.id, token) {
                    if replayed.is_ok() {
                        self.triggers.insert(trigger.id.clone(), trigger.clone());
                    } else {
                        self.triggers.remove(&trigger.id);
                        self.pending_triggers
                            .insert(trigger.id.clone(), trigger.clone());
                    }
                    true
                } else {
                    false
                }
            };
            match (settled, replayed) {
                (true, Ok(())) => {
                    tracing::info!(
                        "{} Trigger {} (type {}) moved from {} to its own namespace {}",
                        "[RE-HOMED]".green(),
                        trigger.id.purple(),
                        trigger.trigger_type.purple(),
                        previous.0.purple(),
                        key.0.purple(),
                    );
                }
                (true, Err(err)) => {
                    tracing::error!(
                        error = %err,
                        trigger_id = %trigger.id,
                        "Re-homing failed; the binding is parked until the trigger type re-registers"
                    );
                    self.settle_superseded_provider(&trigger, registrator).await;
                }
                (false, Ok(())) => self.settle_superseded_provider(&trigger, registrator).await,
                (false, Err(_)) => self.settle_superseded_provider(&trigger, registrator).await,
            }
            if !settled && let Some(old) = &old {
                trigger.provider_namespace = previous.0;
                self.settle_superseded_provider(&trigger, old).await;
            }
            operation.armed = false;
        }
    }

    /// The registrator of the provider filed under `key`, taken out of the
    /// map so it can be awaited without holding a map guard.
    fn provider_registrator(&self, key: &TypeKey) -> Option<Arc<dyn TriggerRegistrator>> {
        self.trigger_types
            .get(key)
            .map(|provider| provider.registrator.clone())
    }

    /// Repair a late provider call without overwriting a newer registration.
    /// Repeat if the desired binding changed during the repair, within the
    /// same bounded deadline used for failover provider calls.
    async fn settle_superseded_provider(
        &self,
        stale: &Trigger,
        registrator: &Arc<dyn TriggerRegistrator>,
    ) {
        // The binding can already be parked or removed, with its token gone.
        // Cancelling its caller must not also cancel this final provider repair.
        let registry = self.clone();
        let trigger = stale.clone();
        let provider = Arc::clone(registrator);
        if let Err(err) = tokio::spawn(async move {
            registry.reconcile_provider(&trigger, &provider).await;
        })
        .await
        {
            tracing::warn!(error = %err, trigger_id = %stale.id, "Provider repair stopped");
        }
    }

    async fn reconcile_provider(&self, stale: &Trigger, registrator: &Arc<dyn TriggerRegistrator>) {
        let key = stale.provider_key();
        let desired = || {
            let _park = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            let current = match self.failing_over.get(&stale.id) {
                Some(operation) if operation.removing => return None,
                Some(operation) => Arc::clone(&operation.target),
                None => self
                    .triggers
                    .by_id
                    .get(&stale.id)
                    .map(|current| Arc::clone(current.value()))?,
            };
            if current.provider_key() != key {
                return None;
            }
            self.provider_registrator(&key)
                .map(|provider| (current, provider))
        };
        let deadline = tokio::time::Instant::now() + self.failover_replay_timeout();
        let mut touched = Arc::clone(registrator);
        loop {
            if tokio::time::Instant::now() >= deadline {
                tracing::warn!(trigger_id = %stale.id, "Timed out settling a superseded binding's provider");
                return;
            }
            let before = desired();
            let replay = before
                .as_ref()
                .filter(|(_, provider)| Arc::ptr_eq(provider, &touched));
            let result = match replay {
                Some((current, _)) => {
                    tokio::time::timeout_at(
                        deadline,
                        touched.replay_trigger(current.as_ref().clone()),
                    )
                    .await
                }
                None => {
                    tokio::time::timeout_at(deadline, touched.unregister_trigger(stale.clone()))
                        .await
                }
            };
            match result {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    tracing::warn!(error = %err, trigger_id = %stale.id, "Could not settle a superseded binding's provider");
                    return;
                }
                Err(_) => {
                    tracing::warn!(trigger_id = %stale.id, "Timed out settling a superseded binding's provider");
                    return;
                }
            }
            let after = desired();
            if replay.is_some() {
                if let (Some((before, provider_before)), Some((after, provider_after))) =
                    (&before, &after)
                    && Arc::ptr_eq(before, after)
                    && Arc::ptr_eq(provider_before, provider_after)
                {
                    return;
                }
            } else {
                let Some((_, provider)) = after else {
                    return;
                };
                // Re-announcing the same worker creates a new Arc wrapper.
                // Cleaning the old wrapper can also remove the current job,
                // so restore through the current provider before returning.
                touched = provider;
            }
        }
    }

    pub async fn unregister_worker(&self, worker_id: &Uuid) {
        // Cancel claims before awaiting any provider: a pending activation
        // may not appear in either binding map, but it still has an owner.
        {
            let _park = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            self.failing_over
                .retain(|_, operation| operation.worker_id != Some(*worker_id));
            self.pending_triggers
                .retain(|_, trigger| trigger.worker_id != Some(*worker_id));
        }
        let worker_trigger_type_keys: Vec<TypeKey> = self
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
            let removed = {
                let _park = self
                    .park_transition
                    .lock()
                    .expect("park_transition lock poisoned");
                let removed = self
                    .triggers
                    .remove_if(&trigger.id, |_, t| t.worker_id == Some(*worker_id));
                if removed.is_some() {
                    self.interrupt_failover(&trigger.id);
                }
                removed
            };
            let Some((_, trigger)) = removed else {
                tracing::debug!(
                    trigger_id = trigger.id,
                    "Trigger already replaced by another worker; leaving it"
                );
                continue;
            };

            if let Some(registrator) = self.provider_registrator(&trigger.provider_key()) {
                tracing::debug!(trigger_type_id = %trigger.trigger_type, "Unregistering trigger");

                let result: Result<(), anyhow::Error> =
                    registrator.unregister_trigger(trigger.clone()).await;
                if let Err(err) = result {
                    tracing::error!(error = %err, "Error unregistering trigger");
                }
                if self.triggers.contains_key(&trigger.id) {
                    self.settle_superseded_provider(&trigger, &registrator)
                        .await;
                }
            }

            tracing::debug!(trigger_id = trigger.id, "Trigger removed");
        }

        // Connection-owned intents die with their worker, same as live
        // connection-owned bindings above.
        self.pending_triggers
            .retain(|_, trigger| trigger.worker_id != Some(*worker_id));

        for key in worker_trigger_type_keys {
            tracing::debug!(trigger_type_id = %key.1, namespace = %key.0, "Removing trigger type");
            self.remove_trigger_type_if_provider(&key, worker_id).await;
        }
    }

    /// Removes the provider filed under `key` when `worker_id` still serves it,
    /// and re-resolves the bindings it served. Returns whether this call
    /// removed it.
    ///
    /// Shared by the disconnect sweep and `UnregisterTriggerType`: a provider
    /// retiring its type explicitly leaves the same orphans as one that
    /// disconnects, and they go to the same places.
    pub async fn remove_trigger_type_if_provider(&self, key: &TypeKey, worker_id: &Uuid) -> bool {
        let Some(orphans) = self.take_trigger_type_if_provider(key, worker_id) else {
            return false;
        };
        self.fail_over_orphans(orphans).await;
        true
    }

    /// The synchronous half of [`Self::remove_trigger_type_if_provider`]:
    /// removes the provider when `worker_id` still serves it, and claims the
    /// bindings it served for [`Self::fail_over_orphans`] in the same step.
    /// Claimed, they are in flight at once: a provider registering under the
    /// same key before the failover runs (a hot reload) does not replay them,
    /// so each is delivered once. `None` when `worker_id` does not serve it.
    pub fn take_trigger_type_if_provider(
        &self,
        key: &TypeKey,
        worker_id: &Uuid,
    ) -> Option<Orphans> {
        let _park = self
            .park_transition
            .lock()
            .expect("park_transition lock poisoned");
        // The entry may have been replaced by a new provider since the caller
        // looked (worker restart: the replacement can register before the old
        // connection's cleanup runs). Only remove it if it still belongs to
        // this worker — and only then are its bindings orphaned; a replaced
        // type keeps firing.
        let removed = self
            .trigger_types
            .remove_if(key, |_, tt| tt.worker_id == Some(*worker_id))
            .is_some();
        if !removed {
            tracing::debug!(
                trigger_type_id = %key.1,
                "Trigger type not provided by this worker (replaced or never registered); leaving it"
            );
            return None;
        }
        tracing::debug!(trigger_type_id = %key.1, "Trigger type removed");
        let orphans: Vec<(Trigger, u64)> = self
            .triggers
            .iter()
            .filter(|pair| {
                pair.value().provider_key() == *key
                    && !self
                        .failing_over
                        .get(pair.key())
                        .is_some_and(|operation| operation.removing)
            })
            .map(|pair| pair.value().clone())
            .collect::<Vec<_>>()
            .into_iter()
            .map(|orphan| {
                let token = self.begin_transition(&orphan, false);
                (orphan, token)
            })
            .collect();
        Some(Orphans(orphans))
    }

    /// Bindings the provider at `key` served are orphaned: nothing can fire
    /// them. Each resolves again — a binding that only resolved there because
    /// it is at home can fall back to the engine's provider in `default`, the
    /// same step that carried it before its own provider existed. Whatever
    /// still resolves nowhere is parked, so a provider restart never silently
    /// drops everyone else's bindings.
    pub async fn fail_over_orphans(&self, orphans: Orphans) {
        // Concurrently, each within the replay timeout: a stalled provider
        // costs one timeout, not one per binding.
        futures::future::join_all(
            orphans
                .0
                .into_iter()
                .map(|(orphan, token)| self.fail_over(orphan, token)),
        )
        .await;
    }

    /// Moves one binding off the provider at `key`, which just left: to the
    /// provider it resolves to now, or to `pending_triggers` when there is
    /// none or the replay fails or times out.
    ///
    /// The binding stays in `triggers` while the replay is in flight, marked
    /// with this move's token: registering or re-homing a provider skips it,
    /// and replacing, unregistering or parking it drops the token. The move
    /// settles under `park_transition`, and only if its token is still there;
    /// otherwise the binding is left as found, and one unregistered meanwhile
    /// is detached from the provider that just accepted it. A provider that
    /// registered at the binding's home while it was in flight gets it back.
    async fn fail_over(&self, orphan: Trigger, token: u64) {
        let mut moving = orphan;
        let next = {
            let _park = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            let Some(mut operation) = self.failing_over.get_mut(&moving.id) else {
                return;
            };
            if operation.token != token {
                return;
            }
            let next = self.resolve_provider_key(&moving);
            if let Some(next) = &next {
                moving.provider_namespace = next.0.clone();
            }
            operation.target = Arc::new(moving.clone());
            next
        };
        let timeout = self.failover_replay_timeout();
        let registrator = next.as_ref().and_then(|key| self.provider_registrator(key));
        let mut operation = TriggerTransitionGuard {
            registry: self,
            target: moving.clone(),
            token,
            registrator: registrator.clone(),
            removing: false,
            source: None,
            armed: true,
        };
        let replayed = match &next {
            Some(_) => Some(match &registrator {
                Some(registrator) => {
                    match tokio::time::timeout(timeout, registrator.replay_trigger(moving.clone()))
                        .await
                    {
                        Ok(Ok(())) => Ok(()),
                        Ok(Err(err)) => Err(err),
                        Err(_) => Err(anyhow::anyhow!(
                            "the provider did not take the binding within {timeout:?}"
                        )),
                    }
                }
                None => Err(anyhow::anyhow!("provider vanished during failover")),
            }),
            None => None,
        };

        let moved = {
            let _park = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            let ours = self.finish_transition(&moving.id, token);
            if !ours {
                false
            } else {
                match (&next, replayed) {
                    (Some(next), Some(Ok(_))) => {
                        tracing::info!(
                            "{} Trigger {} (type {}) fell back to {} after its provider left",
                            "[RE-HOMED]".green(),
                            moving.id.purple(),
                            moving.trigger_type.purple(),
                            next.0.purple(),
                        );
                        self.triggers.insert(moving.id.clone(), moving.clone());
                        true
                    }
                    (_, Some(Err(err))) => {
                        tracing::error!(
                            error = %err,
                            trigger_id = %moving.id,
                            "Fallback provider refused the binding; parking it"
                        );
                        self.triggers.remove(&moving.id);
                        self.pending_triggers
                            .insert(moving.id.clone(), moving.clone());
                        false
                    }
                    _ => {
                        tracing::warn!(
                            "{} Trigger {} (type {}, function {}) is disabled: its trigger type was unregistered. It will be re-registered automatically when the trigger type returns.",
                            "[DISABLED]".yellow(),
                            moving.id.purple(),
                            moving.trigger_type.purple(),
                            moving.function_id.purple(),
                        );
                        self.triggers.remove(&moving.id);
                        self.pending_triggers
                            .insert(moving.id.clone(), moving.clone());
                        false
                    }
                }
            }
        };
        operation.armed = false;
        // A provisional target may also have been delivered by another
        // operation's compensation. Settle it after failure or supersession.
        if !moved && let Some(registrator) = &registrator {
            self.settle_superseded_provider(&moving, registrator).await;
        }
        // A provider that registered at the binding's home while it was in
        // flight skipped it; it claims it back now.
        if moved && moving.is_on_fallback() {
            let home = type_key(&moving.home_namespace, &moving.trigger_type);
            if let Some(registrator) = self.provider_registrator(&home) {
                self.rehome_fallback_bindings(&home, &registrator).await;
            }
        }
    }

    pub async fn register_trigger_type(
        &self,
        trigger_type: TriggerType,
    ) -> Result<(), anyhow::Error> {
        let key = trigger_type.key();
        let trigger_type_id = trigger_type.id.clone();

        tracing::info!(
            "{} Trigger Type {} in {}",
            "[REGISTERED]".green(),
            trigger_type_id.purple(),
            trigger_type.namespace.purple()
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
        self.trigger_types.insert(key.clone(), trigger_type);

        // Only the bindings this provider actually serves. Filtering by type
        // id alone is what sent one project's bindings into another project's
        // provider, so the resolved key is the filter.
        let matching_triggers: Vec<Trigger> = self
            .triggers
            .iter()
            .filter(|pair| {
                pair.value().provider_key() == key && !self.failing_over.contains_key(pair.key())
            })
            .map(|pair| pair.value().clone())
            .collect();

        // Re-fetch instead of using the inserted value: if an even newer
        // generation replaced the entry in the meantime, replay must route to
        // that one.
        if let Some(registrator) = self.provider_registrator(&key) {
            for trigger in matching_triggers {
                let result = registrator.replay_trigger(trigger.clone()).await;
                if let Err(err) = result {
                    tracing::error!(error = %err, "Error registering trigger");
                }
            }

            // A parked intent resolves again from scratch: it may have been
            // parked when nothing provided the type at all, and this
            // registration is what makes it resolvable.
            let pending: Vec<String> = self
                .pending_triggers
                .iter()
                .filter(|pair| {
                    pair.value().trigger_type == trigger_type_id
                        && self.resolve_provider_key(pair.value()).as_ref() == Some(&key)
                })
                .map(|pair| pair.key().clone())
                .collect();

            for trigger_id in pending {
                // Claiming removes the intent, so a concurrent drain of the
                // same type cannot activate it twice.
                let Some((trigger, token)) = self.claim_parked(&trigger_id, &key) else {
                    continue;
                };
                self.activate_pending_trigger(&registrator, trigger, token)
                    .await;
            }
        }

        // Claim back what settled on the fallback before this provider existed.
        // Re-fetched for the same reason as above, and after the replay so a
        // binding is never in flight to two providers at once.
        if key.0 != crate::protocol::DEFAULT_NAMESPACE
            && let Some(registrator) = self.provider_registrator(&key)
        {
            self.rehome_fallback_bindings(&key, &registrator).await;
        }

        Ok(())
    }

    /// Claims a parked intent for activation: removes it from
    /// `pending_triggers` and marks it in flight with a token, under
    /// `park_transition`. Whatever registers, unregisters or parks the same id
    /// meanwhile drops the token (see `interrupt_failover`).
    fn claim_parked(&self, id: &str, key: &TypeKey) -> Option<(Trigger, u64)> {
        let _park = self
            .park_transition
            .lock()
            .expect("park_transition lock poisoned");
        if self
            .failing_over
            .get(id)
            .is_some_and(|operation| operation.removing)
        {
            return None;
        }
        let (_, mut trigger) = self.pending_triggers.remove(id)?;
        trigger.provider_namespace = key.0.clone();
        let token = self.begin_transition(&trigger, false);
        Some((trigger, token))
    }

    /// Activate a claimed pending intent against its now-available trigger
    /// type. On registrator failure the intent is parked again so
    /// the next type (re)registration retries it.
    ///
    /// The intent was claimed with `token`; it settles (live, or parked again)
    /// only if the token is still there and the id is in neither map: a newer
    /// registration of the same id made during the provider's round trip is
    /// never overwritten by this older one. Superseded, the provider that just
    /// took it lets go of it, or, when the newer binding is with that same
    /// provider, gets the newer one again (the replay may have reached it
    /// after the newer registration).
    async fn activate_pending_trigger(
        &self,
        registrator: &Arc<dyn TriggerRegistrator>,
        trigger: Trigger,
        token: u64,
    ) -> bool {
        let mut operation = TriggerTransitionGuard {
            registry: self,
            target: trigger.clone(),
            token,
            registrator: Some(Arc::clone(registrator)),
            removing: false,
            source: None,
            armed: true,
        };
        let replayed = registrator.replay_trigger(trigger.clone()).await;
        let settled = {
            let _park = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            let ours = self.finish_transition(&trigger.id, token)
                && !self.triggers.contains_key(&trigger.id)
                && !self.pending_triggers.contains_key(&trigger.id);
            if ours {
                match &replayed {
                    Ok(()) => self.triggers.insert(trigger.id.clone(), trigger.clone()),
                    Err(_) => self
                        .pending_triggers
                        .insert(trigger.id.clone(), trigger.clone()),
                };
                Some(replayed.is_ok())
            } else {
                None
            }
        };
        operation.armed = false;
        match (settled, replayed) {
            (Some(true), _) => {
                tracing::info!(
                    "{} Trigger {} (type {}, function {}) recovered from pending",
                    "[REGISTERED]".green(),
                    trigger.id.purple(),
                    trigger.trigger_type.purple(),
                    trigger.function_id.purple(),
                );
                true
            }
            (Some(false), Err(err)) => {
                tracing::error!(
                    error = %err,
                    trigger_id = %trigger.id,
                    trigger_type = %trigger.trigger_type,
                    "Error registering pending trigger; it stays pending"
                );
                self.settle_superseded_provider(&trigger, registrator).await;
                false
            }
            (None, Ok(())) => {
                tracing::debug!(
                    trigger_id = %trigger.id,
                    "A parked binding was superseded while it activated; the newer one stays"
                );
                self.settle_superseded_provider(&trigger, registrator).await;
                false
            }
            (None, Err(_)) => {
                self.settle_superseded_provider(&trigger, registrator).await;
                false
            }
            _ => false,
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
            // iii-stream is an engine worker: it is enabled through engine
            // configuration, not installed with compose::add. It is also
            // deprecated, so point at the migration guide.
            Some(worker_name @ "iii-stream") => format!(
                "{} If this persists, the {} engine worker is not running — enable it in the \
                 engine config.yaml (`workers:` entry `- name: iii-stream`) or under \
                 `engine.workers.iii-stream` in worker-compose.yaml. {}",
                base,
                worker_name.cyan().bold(),
                crate::deprecation::stream_deprecation_message(&format!(
                    "{} trigger type",
                    trigger.trigger_type
                )),
            ),
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

    pub async fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Result<RegisterTriggerOutcome, anyhow::Error> {
        let mut trigger = trigger;
        let resolved = self.resolve_provider_key(&trigger);
        if let Some(key) = &resolved {
            trigger.provider_namespace = key.0.clone();
        }
        let trigger = trigger;

        let Some(registrator) = resolved
            .as_ref()
            .and_then(|key| self.provider_registrator(key))
        else {
            // Park the intent instead of failing: workers may connect in any
            // order, and a binding that arrives before its trigger type's
            // provider must survive until the provider shows up.
            tracing::warn!("{}", Self::pending_trigger_warning(&trigger));
            // It supersedes a live binding with the same id (and any failover
            // of it): the binding ends in exactly one bucket.
            let superseded = {
                let _park = self
                    .park_transition
                    .lock()
                    .expect("park_transition lock poisoned");
                self.interrupt_failover(&trigger.id);
                let superseded = self.triggers.remove(&trigger.id).map(|(_, live)| live);
                self.pending_triggers
                    .insert(trigger.id.clone(), trigger.clone());
                superseded
            };
            if let Some(live) = superseded
                && let Some(provider) = self.provider_registrator(&live.provider_key())
                && let Err(err) = provider.unregister_trigger(live).await
            {
                tracing::warn!(
                    error = %err,
                    trigger_id = %trigger.id,
                    "Could not detach the binding a parked re-registration replaces"
                );
            }

            // Close the park/drain race: if the type registered between the
            // lookup above and the park, its pending drain may have already
            // run. Re-check and activate the intent ourselves — claiming it
            // via `remove` so we never double-activate with a drain.
            // Resolved again rather than reusing the earlier miss: the
            // provider that appeared in the meantime may be either step.
            if let Some(key) = self.resolve_provider_key(&trigger)
                && let Some(registrator) = self.provider_registrator(&key)
                && let Some((parked, token)) = self.claim_parked(&trigger.id, &key)
                && self
                    .activate_pending_trigger(&registrator, parked, token)
                    .await
            {
                return Ok(RegisterTriggerOutcome::Registered);
            }

            return Ok(RegisterTriggerOutcome::Deferred);
        };

        if let Err(err) = registrator.register_trigger(trigger.clone()).await {
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
            {
                let _park = self
                    .park_transition
                    .lock()
                    .expect("park_transition lock poisoned");
                self.interrupt_failover(&trigger.id);
                self.triggers.remove(&trigger.id);
                self.pending_triggers.insert(trigger.id.clone(), trigger);
            }
            return Ok(RegisterTriggerOutcome::Deferred);
        }

        drop(registrator);

        tracing::debug!(trigger = %trigger.id, worker_id = %trigger.worker_id.unwrap_or_default(), "Registering trigger");

        // A live registration supersedes any parked intent with the same id,
        // and any failover of the binding it replaces.
        {
            let _park = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            self.interrupt_failover(&trigger.id);
            self.pending_triggers.remove(&trigger.id);
            self.triggers.insert(trigger.id.clone(), trigger);
        }

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

        let (trigger, token) = {
            let _park = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            let Some(trigger_entry) = self.triggers.get(&id) else {
                // A claimed pending intent is outside both maps. Cancelling
                // its token makes its completion detach the provider instead
                // of publishing a binding after this unregister.
                let cancelled = self.interrupt_failover(&id);
                let removed = self.pending_triggers.remove(&id).is_some();
                return Ok(cancelled || removed);
            };
            let trigger = trigger_entry.value().clone();
            drop(trigger_entry);
            let token = self.begin_transition(&trigger, true);
            (trigger, token)
        };

        let registrator = self.provider_registrator(&trigger.provider_key());
        let _operation = TriggerTransitionGuard {
            registry: self,
            target: trigger.clone(),
            token,
            registrator: registrator.clone(),
            removing: true,
            source: None,
            armed: true,
        };

        if let Some(registrator) = &registrator
            && let Err(err) = registrator.unregister_trigger(trigger.clone()).await
        {
            if let Some(recovery) = self.recover_abandoned_removal(&id, token, Some(registrator))
                && let Err(join_error) = recovery.await
            {
                tracing::warn!(error = %join_error, trigger_id = %id, "Failed removal recovery stopped");
            }
            return Err(err);
        }

        // A late async rejection may re-park this binding while the
        // registrator call above is in flight; an explicit unregister must
        // win over that park or the intent resurrects on the next type
        // registration. The transition lock makes this two-map clear atomic
        // with the re-park's own live-to-pending move.
        let removed = {
            let _park = self
                .park_transition
                .lock()
                .expect("park_transition lock poisoned");
            if self.finish_transition(&id, token) {
                self.triggers.remove(&id);
                self.pending_triggers.remove(&id);
                true
            } else {
                false
            }
        };
        if !removed && let Some(registrator) = &registrator {
            self.settle_superseded_provider(&trigger, registrator).await;
        }

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
            // A rejection may park the entry during an explicit unregister.
            // Keep the removal token so that its completion clears the parked
            // intent; claim_parked will not activate it in the meantime.
            if !self
                .failing_over
                .get(id)
                .is_some_and(|operation| operation.removing)
            {
                self.interrupt_failover(id);
            }
            tracing::warn!(
                trigger_id = %id,
                trigger_type = %failed.trigger_type,
                function_id = %failed.function_id,
                "Trigger registration rejected by provider; parked as pending until the trigger type re-registers"
            );
            let provider = failed.provider_key();
            self.pending_triggers.insert(failed.id.clone(), failed);
            provider
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
        let current = self
            .trigger_types
            .get(&parked_type)
            .map(|provider| (provider.worker_id, provider.registrator.clone()));
        if let Some((provider_worker, registrator)) = current
            && provider_worker != Some(reporter_worker_id)
            && let Some((parked, token)) = self.claim_parked(id, &parked_type)
        {
            self.activate_pending_trigger(&registrator, parked, token)
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::DEFAULT_NAMESPACE;
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
            trigger_namespace: None,
            home_namespace: crate::protocol::default_namespace(),
            provider_namespace: crate::protocol::default_namespace(),
        }
    }

    fn target_fields(targets: Vec<Arc<Trigger>>) -> Vec<(String, String, Option<Value>)> {
        targets
            .iter()
            .map(|trigger| {
                (
                    trigger.function_id.clone(),
                    trigger.namespace.clone(),
                    trigger.metadata.clone(),
                )
            })
            .collect()
    }

    #[test]
    fn trigger_store_replacement_updates_type_namespace_and_metadata() {
        let store = TriggerStore::new();
        store.insert("other".into(), make_trigger("other", "unrelated"));
        store.insert("binding".into(), make_trigger("binding", "first"));

        let mut replacement = make_trigger("binding", "first");
        replacement.namespace = "billing".into();
        replacement.function_id = "orders::changed".into();
        replacement.metadata = Some(serde_json::json!({ "revision": 2 }));
        store.insert(replacement.id.clone(), replacement.clone());
        let expected = vec![(
            replacement.function_id.clone(),
            replacement.namespace.clone(),
            replacement.metadata.clone(),
        )];
        assert_eq!(target_fields(store.targets_for_type("first")), expected);

        replacement.trigger_type = "second".into();
        store.insert(replacement.id.clone(), replacement);
        assert!(store.targets_for_type("first").is_empty());
        assert_eq!(target_fields(store.targets_for_type("second")), expected);
        assert_eq!(store.targets_for_type("unrelated").len(), 1);

        store.remove("binding");
        assert!(store.targets_for_type("second").is_empty());
        assert_eq!(store.targets_for_type("unrelated").len(), 1);
    }

    #[test]
    fn trigger_store_shares_metadata_and_moves_it_out_on_removal() {
        let store = TriggerStore::new();
        let mut trigger = make_trigger("binding", "event");
        trigger.metadata = Some(serde_json::json!({ "payload": "x".repeat(4096) }));
        let payload_pointer = trigger.metadata.as_ref().unwrap()["payload"]
            .as_str()
            .unwrap()
            .as_ptr();
        store.insert(trigger.id.clone(), trigger);

        let weak = {
            let by_id = store.by_id.get("binding").unwrap();
            let by_type = store.by_type.get("event").unwrap();
            assert!(Arc::ptr_eq(by_id.value(), &by_type["binding"]));
            Arc::downgrade(by_id.value())
        };
        {
            let entry = store.iter().next().unwrap();
            assert_eq!(entry.key(), "binding");
            assert_eq!(
                entry.value().metadata.as_ref().unwrap()["payload"]
                    .as_str()
                    .unwrap()
                    .as_ptr(),
                payload_pointer,
            );
        }

        let (_, removed) = store.remove("binding").unwrap();
        assert_eq!(
            removed.metadata.as_ref().unwrap()["payload"]
                .as_str()
                .unwrap()
                .as_ptr(),
            payload_pointer,
            "removal must return the original metadata allocation",
        );
        assert!(weak.upgrade().is_none());
        assert!(store.targets_for_type("event").is_empty());
    }

    #[test]
    fn trigger_store_replacement_releases_the_previous_index_owner() {
        let store = TriggerStore::new();
        store.insert("binding".into(), make_trigger("binding", "event"));

        // Cover replacement both within a type and across types.
        for trigger_type in ["event", "other"] {
            let previous = {
                let entry = store.by_id.get("binding").unwrap();
                Arc::downgrade(entry.value())
            };
            let replacement = make_trigger("binding", trigger_type);
            let removed = store.insert("binding".into(), replacement).unwrap();
            assert_eq!(removed.id, "binding");
            assert!(
                previous.upgrade().is_none(),
                "the type index must release the replaced trigger",
            );
            assert_eq!(store.targets_for_type(trigger_type).len(), 1);
        }
        assert!(store.targets_for_type("event").is_empty());
    }

    #[test]
    fn trigger_dispatch_snapshot_survives_replacement_and_removal() {
        let store = TriggerStore::new();
        let mut trigger = make_trigger("binding", "event");
        trigger.namespace = "orders".into();
        trigger.metadata = Some(serde_json::json!({ "revision": 1 }));
        store.insert(trigger.id.clone(), trigger);

        let snapshot = store.targets_for_type("event");
        assert_eq!(snapshot.len(), 1);
        assert!(Arc::ptr_eq(
            &snapshot[0],
            store.by_id.get("binding").unwrap().value(),
        ));
        let previous = Arc::downgrade(&snapshot[0]);

        let mut replacement = make_trigger("binding", "other");
        replacement.namespace = "billing".into();
        replacement.metadata = Some(serde_json::json!({ "revision": 2 }));
        store.insert(replacement.id.clone(), replacement);
        store.remove("binding");

        assert!(store.is_empty());
        assert!(store.targets_for_type("event").is_empty());
        assert!(store.targets_for_type("other").is_empty());
        assert_eq!(snapshot[0].function_id, "fn_binding");
        assert_eq!(snapshot[0].namespace, "orders");
        assert_eq!(
            snapshot[0].metadata,
            Some(serde_json::json!({ "revision": 1 }))
        );
        drop(snapshot);
        assert!(previous.upgrade().is_none());
    }

    #[test]
    fn trigger_dispatch_snapshot_keeps_members_during_same_type_updates() {
        let store = TriggerStore::new();
        store.insert("first".into(), make_trigger("first", "event"));
        store.insert("second".into(), make_trigger("second", "event"));
        let snapshot = store.targets_for_type("event");
        let previous = Arc::downgrade(
            snapshot
                .iter()
                .find(|trigger| trigger.id == "first")
                .unwrap(),
        );

        store.insert("third".into(), make_trigger("third", "event"));
        store.remove("first");

        let current = store.targets_for_type("event");
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot.iter().any(|trigger| trigger.id == "first"));
        assert!(!snapshot.iter().any(|trigger| trigger.id == "third"));
        assert_eq!(current.len(), 2);
        assert!(!current.iter().any(|trigger| trigger.id == "first"));
        assert!(current.iter().any(|trigger| trigger.id == "third"));
        assert!(Arc::ptr_eq(
            snapshot
                .iter()
                .find(|trigger| trigger.id == "second")
                .unwrap(),
            current
                .iter()
                .find(|trigger| trigger.id == "second")
                .unwrap(),
        ));
        drop(snapshot);
        assert!(previous.upgrade().is_none());
    }

    #[test]
    fn trigger_store_stale_owner_cleanup_keeps_replacement_indexed() {
        let store = TriggerStore::new();
        let previous_owner = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let mut trigger = make_trigger("binding", "event");
        trigger.worker_id = Some(previous_owner);
        store.insert(trigger.id.clone(), trigger.clone());
        trigger.worker_id = Some(owner);
        store.insert(trigger.id.clone(), trigger);

        assert!(
            store
                .remove_if("binding", |_, t| t.worker_id == Some(previous_owner))
                .is_none()
        );
        assert_eq!(store.targets_for_type("event").len(), 1);
        assert!(
            store
                .remove_if("binding", |_, t| t.worker_id == Some(owner))
                .is_some()
        );
        assert!(store.targets_for_type("event").is_empty());
    }

    #[test]
    fn trigger_store_concurrent_replacement_and_removal_leave_no_stale_targets() {
        let store = TriggerStore::new();
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            for offset in 0..2 {
                let store = &store;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    for index in 0..128 {
                        let trigger_type = if (index + offset) % 2 == 0 {
                            "first"
                        } else {
                            "second"
                        };
                        store.insert("binding".into(), make_trigger("binding", trigger_type));
                        if index % 3 == 0 {
                            store.remove("binding");
                        }
                    }
                });
            }
        });
        store.remove("binding");
        assert!(store.is_empty());
        assert!(store.targets_for_type("first").is_empty());
        assert!(store.targets_for_type("second").is_empty());
    }

    #[tokio::test]
    async fn trigger_index_tracks_pending_rehome_failover_and_unregister() {
        let registry = TriggerRegistry::new();
        let fallback_worker = Uuid::new_v4();
        let home_worker = Uuid::new_v4();
        let mut trigger = resolved_trigger("binding", "event", "shop");
        trigger.namespace = "billing".into();
        trigger.metadata = Some(serde_json::json!({ "source": "shop" }));
        let expected = vec![(
            trigger.function_id.clone(),
            trigger.namespace.clone(),
            trigger.metadata.clone(),
        )];

        registry.register_trigger(trigger).await.unwrap();
        assert!(registry.triggers.targets_for_type("event").is_empty());

        let mut fallback = make_trigger_type_ns(DEFAULT_NAMESPACE, "event");
        fallback.worker_id = Some(fallback_worker);
        registry.register_trigger_type(fallback).await.unwrap();
        assert_eq!(
            target_fields(registry.triggers.targets_for_type("event")),
            expected
        );

        let mut home = make_trigger_type_ns("shop", "event");
        home.worker_id = Some(home_worker);
        registry.register_trigger_type(home).await.unwrap();
        assert_eq!(
            target_fields(registry.triggers.targets_for_type("event")),
            expected
        );
        assert_eq!(
            registry.triggers.get("binding").unwrap().provider_namespace,
            "shop"
        );

        registry.unregister_worker(&home_worker).await;
        assert_eq!(
            target_fields(registry.triggers.targets_for_type("event")),
            expected
        );
        assert_eq!(
            registry.triggers.get("binding").unwrap().provider_namespace,
            DEFAULT_NAMESPACE
        );

        registry.unregister_worker(&fallback_worker).await;
        assert!(registry.triggers.targets_for_type("event").is_empty());
        assert!(registry.pending_triggers.contains_key("binding"));

        registry
            .register_trigger_type(make_trigger_type_ns(DEFAULT_NAMESPACE, "event"))
            .await
            .unwrap();
        assert_eq!(
            target_fields(registry.triggers.targets_for_type("event")),
            expected
        );
        registry
            .unregister_trigger("binding".into(), None)
            .await
            .unwrap();
        assert!(registry.triggers.targets_for_type("event").is_empty());
    }

    #[tokio::test]
    async fn trigger_index_excludes_rejected_bindings_until_provider_replacement() {
        let registry = TriggerRegistry::new();
        let worker = Uuid::new_v4();
        let mut provider = make_trigger_type_ns(DEFAULT_NAMESPACE, "event");
        provider.worker_id = Some(worker);
        registry.register_trigger_type(provider).await.unwrap();
        registry
            .register_trigger(make_trigger("binding", "event"))
            .await
            .unwrap();

        registry.park_rejected_trigger("binding", worker).await;
        assert!(registry.triggers.targets_for_type("event").is_empty());
        assert!(registry.pending_triggers.contains_key("binding"));

        registry
            .register_trigger_type(make_trigger_type_ns(DEFAULT_NAMESPACE, "event"))
            .await
            .unwrap();
        assert_eq!(
            target_fields(registry.triggers.targets_for_type("event")),
            vec![("fn_binding".into(), DEFAULT_NAMESPACE.into(), None)]
        );
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

    /// Holds every replay until a permit is added; counts detaches.
    struct GatedRegistrator {
        gate: tokio::sync::Semaphore,
        replay_started: tokio::sync::Notify,
        unregistered: AtomicUsize,
        /// The function of every binding replayed to it, in order.
        replayed: std::sync::Mutex<Vec<String>>,
    }

    impl TriggerRegistrator for Arc<GatedRegistrator> {
        fn register_trigger(
            &self,
            _trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn replay_trigger(
            &self,
            trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            self.replay_started.notify_one();
            Box::pin(async move {
                let _permit = self.gate.acquire().await;
                self.replayed.lock().unwrap().push(trigger.function_id);
                Ok(())
            })
        }

        fn unregister_trigger(
            &self,
            _trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            self.unregistered.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    /// An explicit unregister that lands while a binding is being replayed to
    /// its fallback provider wins: the binding is not resurrected, and the
    /// fallback that just accepted it is told to let go.
    #[tokio::test]
    async fn an_unregister_during_failover_is_not_undone_by_the_replay() {
        let registry = Arc::new(TriggerRegistry::new());
        let worker = Uuid::new_v4();
        let fallback = gated();
        registry
            .register_trigger_type(TriggerType::new(
                "state",
                "the engine's",
                Box::new(fallback.clone()),
                None,
            ))
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

        let failing_over = {
            let registry = registry.clone();
            tokio::spawn(async move {
                registry
                    .remove_trigger_type_if_provider(&type_key("shop", "state"), &worker)
                    .await
            })
        };
        fallback.replay_started.notified().await;
        assert!(
            registry.triggers.contains_key("t1"),
            "the binding stays put while its replay is in flight"
        );

        assert!(
            registry
                .unregister_trigger("t1".into(), None)
                .await
                .unwrap()
        );
        fallback.gate.add_permits(1);
        assert!(failing_over.await.unwrap());

        assert!(!registry.triggers.contains_key("t1"), "not resurrected");
        assert!(!registry.pending_triggers.contains_key("t1"));
        assert_eq!(
            fallback.unregistered.load(Ordering::SeqCst),
            1,
            "detached from the fallback that accepted it"
        );
    }

    fn gated() -> Arc<GatedRegistrator> {
        Arc::new(GatedRegistrator {
            gate: tokio::sync::Semaphore::new(0),
            replay_started: tokio::sync::Notify::new(),
            unregistered: AtomicUsize::new(0),
            replayed: std::sync::Mutex::new(Vec::new()),
        })
    }

    /// The engine's provider (`fallback`) and a project provider at `shop`
    /// served by `worker`, with `t1` bound at home in `shop`.
    async fn shop_binding_with_fallback(
        registry: &TriggerRegistry,
        fallback: &Arc<GatedRegistrator>,
        worker: Uuid,
    ) {
        registry
            .register_trigger_type(TriggerType::new(
                "state",
                "the engine's",
                Box::new(fallback.clone()),
                None,
            ))
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
    }

    /// The project's provider comes back while its binding is in flight to
    /// the fallback: the binding ends on the returning provider alone, never
    /// served by both.
    #[tokio::test]
    async fn a_provider_returning_during_failover_gets_the_binding_back_once() {
        let registry = Arc::new(TriggerRegistry::new());
        let worker = Uuid::new_v4();
        let fallback = gated();
        shop_binding_with_fallback(&registry, &fallback, worker).await;

        let failing_over = {
            let registry = registry.clone();
            tokio::spawn(async move {
                registry
                    .remove_trigger_type_if_provider(&type_key("shop", "state"), &worker)
                    .await
            })
        };
        fallback.replay_started.notified().await;

        let returning = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new_ns(
                "shop",
                "state",
                "the project's own, restarted",
                Box::new(returning.clone()),
                Some(Uuid::new_v4()),
            ))
            .await
            .unwrap();
        assert_eq!(
            returning.register_count.load(Ordering::SeqCst),
            0,
            "a binding in flight is not replayed to a second provider"
        );

        fallback.gate.add_permits(1);
        assert!(failing_over.await.unwrap());

        let stored = registry.triggers.get("t1").expect("still bound");
        assert_eq!(
            stored.provider_namespace, "shop",
            "back with its own provider"
        );
        assert_eq!(returning.register_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            fallback.unregistered.load(Ordering::SeqCst),
            1,
            "the fallback let go of it"
        );
        assert!(registry.pending_triggers.is_empty());
    }

    /// A worker re-registering the same trigger id while its old binding is in
    /// flight keeps its new registration: the failover does not write the old
    /// one back over it.
    #[tokio::test]
    async fn a_re_registration_during_failover_is_not_overwritten() {
        let registry = Arc::new(TriggerRegistry::new());
        let worker = Uuid::new_v4();
        let fallback = gated();
        shop_binding_with_fallback(&registry, &fallback, worker).await;

        let failing_over = {
            let registry = registry.clone();
            tokio::spawn(async move {
                registry
                    .remove_trigger_type_if_provider(&type_key("shop", "state"), &worker)
                    .await
            })
        };
        fallback.replay_started.notified().await;

        let mut fresh = resolved_trigger("t1", "state", "shop");
        fresh.function_id = "fn_fresh".to_string();
        registry.register_trigger(fresh).await.unwrap();

        fallback.gate.add_permits(1);
        assert!(failing_over.await.unwrap());
        let stored = registry.triggers.get("t1").expect("bound");
        assert_eq!(
            stored.function_id, "fn_fresh",
            "the newer registration wins"
        );
        assert!(registry.pending_triggers.is_empty());
        assert_eq!(
            fallback.replayed.lock().unwrap().last().map(String::as_str),
            Some("fn_fresh"),
            "the fallback ends with the newer binding, not the late replay of the old one"
        );
        assert_eq!(fallback.unregistered.load(Ordering::SeqCst), 0);
    }

    /// A parked binding activating when its provider appears does not
    /// overwrite a newer registration of the same id made during the
    /// provider's round trip; the provider ends with the newer one.
    #[tokio::test]
    async fn a_parked_binding_does_not_overwrite_a_newer_registration() {
        let registry = Arc::new(TriggerRegistry::new());
        registry
            .register_trigger(make_trigger("t1", "state"))
            .await
            .unwrap();
        assert!(registry.pending_triggers.contains_key("t1"));

        let provider = gated();
        let registering = {
            let registry = registry.clone();
            let provider = provider.clone();
            tokio::spawn(async move {
                registry
                    .register_trigger_type(TriggerType::new(
                        "state",
                        "arrives late",
                        Box::new(provider),
                        None,
                    ))
                    .await
            })
        };
        provider.replay_started.notified().await;

        let mut newer = make_trigger("t1", "state");
        newer.function_id = "fn_newer".to_string();
        registry.register_trigger(newer).await.unwrap();
        provider.gate.add_permits(1);
        registering.await.unwrap().unwrap();

        assert_eq!(
            registry.triggers.get("t1").unwrap().function_id,
            "fn_newer",
            "the newer registration stays"
        );
        assert!(registry.pending_triggers.is_empty());
        assert_eq!(
            provider.replayed.lock().unwrap().last().map(String::as_str),
            Some("fn_newer"),
            "the provider ends with the newer binding"
        );
    }

    /// A provider retiring its type and registering it again at once (a hot
    /// reload) gets each of its bindings once: the failover claimed them
    /// before the new registration could replay them too.
    #[tokio::test]
    async fn a_hot_reload_delivers_each_binding_once() {
        let registry = TriggerRegistry::new();
        let worker = Uuid::new_v4();
        let first = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new_ns(
                "shop",
                "state",
                "before the reload",
                Box::new(first.clone()),
                Some(worker),
            ))
            .await
            .unwrap();
        registry
            .register_trigger(resolved_trigger("t1", "state", "shop"))
            .await
            .unwrap();

        // `UnregisterTriggerType`, then the same connection's
        // `RegisterTriggerType` before the spawned failover runs.
        let orphans = registry
            .take_trigger_type_if_provider(&type_key("shop", "state"), &worker)
            .expect("its provider");
        let second = Arc::new(ControlledRegistrator::new(false, false));
        registry
            .register_trigger_type(TriggerType::new_ns(
                "shop",
                "state",
                "after the reload",
                Box::new(second.clone()),
                Some(worker),
            ))
            .await
            .unwrap();
        assert_eq!(
            second.register_count.load(Ordering::SeqCst),
            0,
            "in flight: the registration does not replay it"
        );
        registry.fail_over_orphans(orphans).await;
        assert_eq!(second.register_count.load(Ordering::SeqCst), 1, "once");
        assert_eq!(
            registry.triggers.get("t1").unwrap().provider_namespace,
            "shop"
        );
    }

    /// The binding is registered again with another provider (or parked)
    /// while its old one is replayed to the fallback: the fallback lets go,
    /// or the binding would fire from both.
    #[tokio::test]
    async fn a_re_registration_elsewhere_during_failover_detaches_the_fallback() {
        for (fresh, served_by) in [
            (
                explicit_trigger("t1", "state", "shop", "other"),
                Some("other"),
            ),
            // Named at a namespace nothing provides: parked.
            (explicit_trigger("t1", "state", "shop", "nowhere"), None),
        ] {
            let registry = Arc::new(TriggerRegistry::new());
            let worker = Uuid::new_v4();
            let fallback = gated();
            shop_binding_with_fallback(&registry, &fallback, worker).await;
            registry
                .register_trigger_type(make_trigger_type_ns("other", "state"))
                .await
                .unwrap();
            let failing_over = {
                let registry = registry.clone();
                tokio::spawn(async move {
                    registry
                        .remove_trigger_type_if_provider(&type_key("shop", "state"), &worker)
                        .await
                })
            };
            fallback.replay_started.notified().await;
            registry.register_trigger(fresh).await.unwrap();
            fallback.gate.add_permits(1);
            assert!(failing_over.await.unwrap());

            match served_by {
                Some(namespace) => assert_eq!(
                    registry
                        .triggers
                        .get("t1")
                        .expect("bound")
                        .provider_namespace,
                    namespace
                ),
                None => assert!(registry.pending_triggers.contains_key("t1")),
            }
            assert_eq!(
                fallback.unregistered.load(Ordering::SeqCst),
                1,
                "the fallback let go of the copy it took ({served_by:?})"
            );
        }
    }

    /// Many bindings of a provider that leaves fail over concurrently: a
    /// stalled fallback costs one replay timeout, not one per binding.
    #[tokio::test]
    async fn bindings_fail_over_concurrently() {
        let registry = TriggerRegistry::new();
        registry.set_failover_replay_timeout(std::time::Duration::from_millis(200));
        let worker = Uuid::new_v4();
        let fallback = gated(); // never opened
        shop_binding_with_fallback(&registry, &fallback, worker).await;
        for n in 2..=10 {
            registry
                .register_trigger(resolved_trigger(&format!("t{n}"), "state", "shop"))
                .await
                .unwrap();
        }
        let started = std::time::Instant::now();
        assert!(
            registry
                .remove_trigger_type_if_provider(&type_key("shop", "state"), &worker)
                .await
        );
        assert!(
            started.elapsed() < std::time::Duration::from_millis(1500),
            "took {:?} for 10 bindings",
            started.elapsed()
        );
        assert_eq!(registry.pending_triggers.len(), 10, "all parked");
    }

    /// A fallback that never takes the binding cannot hold up the failover
    /// (which runs for the departing provider's connection): it gives up at
    /// the replay timeout and parks the binding.
    #[tokio::test]
    async fn a_stalled_fallback_does_not_hold_up_the_failover() {
        let registry = TriggerRegistry::new();
        registry.set_failover_replay_timeout(std::time::Duration::from_millis(50));
        let worker = Uuid::new_v4();
        let fallback = gated(); // never opened
        shop_binding_with_fallback(&registry, &fallback, worker).await;

        let removed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            registry.remove_trigger_type_if_provider(&type_key("shop", "state"), &worker),
        )
        .await
        .expect("the failover gave up on the stalled provider");
        assert!(removed);
        assert!(!registry.triggers.contains_key("t1"));
        assert!(
            registry.pending_triggers.contains_key("t1"),
            "parked until a provider takes it"
        );
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
    fn pending_warning_for_stream_types_explains_iii_stream_without_compose_add() {
        for trigger_type in ["stream", "stream:join", "stream:leave"] {
            assert_eq!(
                known_trigger_type_provider(trigger_type),
                Some("iii-stream")
            );
            let msg = TriggerRegistry::pending_trigger_warning(&make_trigger("t1", trigger_type));
            assert!(
                !msg.contains("compose::add"),
                "must not suggest compose::add: {msg}"
            );
            assert!(msg.contains("engine.workers.iii-stream"), "{msg}");
            assert!(
                msg.contains(&crate::deprecation::stream_deprecation_message(&format!(
                    "{trigger_type} trigger type"
                ))),
                "{msg}"
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
