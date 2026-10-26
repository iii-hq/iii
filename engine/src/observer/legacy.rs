// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The legacy shim (protocol.md §6): an old `RegisterTrigger { trigger_type }`
//! whose type has no live old-style provider, but a function that stands in
//! for it (`observe.legacy_trigger_type`), becomes an observer of that
//! function.
//!
//! - The observer is owned by the registering connection, observes the
//!   stand-in with no args and no filters, delivers the stand-in's input to
//!   the trigger's function (what the trigger handler used to receive), and
//!   carries the trigger's metadata as its sidecar.
//! - The owner gets the trigger's type and config as `legacy` in
//!   `observeradded` / `functionregistered`, validates the config as its
//!   trigger handler did, and acks or rejects; the registrant gets one
//!   `TriggerRegistrationResult` with the outcome.
//! - `UnregisterTrigger` disposes it. A new `RegisterTrigger` with the same id
//!   replaces it, as it replaces a trigger.
//! - A live old-style provider always wins: the shim is only a fallback, and
//!   the observers it made stay where they are when such a provider
//!   registers later; only new registrations go to the provider.
//! - Like the triggers they replace, these observers count against no limit
//!   (`max_per_connection`, `max_per_function`) and their ids have no length
//!   bound (only an empty one is refused).
//! - RBAC is at parity with old triggers: the registration is gated by its
//!   trigger type (`allowed_trigger_types`), the trigger registration hook and
//!   the target, never by the stand-in function, which the registrant did not
//!   name.
//! - Only a function whose owner speaks `observe.v1` stands in for a type: the
//!   shim needs the owner's ack to answer the registrant. A
//!   `legacy_trigger_type` declared by any other owner is ignored.

use std::{
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

use iii_filter::Filter;
use serde_json::{Map, Value};

use super::{
    FnKey, ObserverRegistry,
    lifecycle::{CaptureSet, Flags, NewObserver, ObserveSpec, Observer, Observing, Policy},
};
use crate::{
    engine::Engine,
    protocol::{
        DEFAULT_NAMESPACE, Deliver, ErrorBody, LegacyTrigger, Message, ObserverState, On,
        default_capture, observe_codes,
    },
    worker_connections::WorkerConnection,
};

/// A `RegisterTrigger` the shim turns into an observer, after the engine's
/// usual checks (RBAC hook, target RBAC, prefix).
#[derive(Debug, Clone)]
pub struct LegacyRegistration {
    /// The trigger id, which becomes the observer id.
    pub id: String,
    pub trigger_type: String,
    /// The trigger's function, `(namespace, function_id)` as `RegisterTrigger`
    /// resolves it.
    pub target: FnKey,
    pub config: Value,
    pub metadata: Option<Value>,
}

impl ObserverRegistry {
    /// The function standing in for `trigger_type` (it declares
    /// `observe.legacy_trigger_type`), found the way `RegisterTrigger` finds a
    /// provider: in `trigger_namespace` only when the registration names one,
    /// else in the registering connection's namespace and then in `default`.
    /// The function must be registered by an owner that speaks `observe.v1`;
    /// with several candidates in one namespace the smallest id wins.
    pub fn legacy_function(
        &self,
        trigger_type: &str,
        trigger_namespace: Option<&str>,
        home_namespace: &str,
    ) -> Option<FnKey> {
        let namespaces: Vec<&str> = match trigger_namespace {
            Some(namespace) => vec![namespace],
            None if home_namespace == DEFAULT_NAMESPACE => vec![DEFAULT_NAMESPACE],
            None => vec![home_namespace, DEFAULT_NAMESPACE],
        };
        namespaces.into_iter().find_map(|namespace| {
            let mut candidates: Vec<FnKey> = self
                .by_function
                .iter()
                .filter(|entry| {
                    entry.key().0 == namespace
                        && entry.value().descriptor().is_some_and(|descriptor| {
                            descriptor.legacy_trigger_type.as_deref() == Some(trigger_type)
                        })
                })
                .map(|entry| entry.key().clone())
                .collect();
            candidates.retain(|key| {
                self.is_registered(key)
                    && self.owner_of(key).is_some_and(|owner| owner.speaks_observe)
            });
            candidates.sort();
            candidates.into_iter().next()
        })
    }

    /// Turns `registration` from `worker` into an observer of `function`, or
    /// answers a replay. The registrant gets its `TriggerRegistrationResult`
    /// once the owner acked or rejected the observer; an `Err` is a rejection
    /// the caller answers now.
    pub fn register_legacy(
        self: &Arc<Self>,
        worker: &WorkerConnection,
        function: FnKey,
        registration: LegacyRegistration,
    ) -> Result<(), ErrorBody> {
        self.register_legacy_with(worker, function, registration, true)
    }

    /// [`Self::register_legacy`]; with `replace` false, an existing binding
    /// with the id is left alone (it is newer than a parked one being
    /// converted) and the registration refused with `OBSERVER_ID_CONFLICT`.
    fn register_legacy_with(
        self: &Arc<Self>,
        worker: &WorkerConnection,
        function: FnKey,
        registration: LegacyRegistration,
        replace: bool,
    ) -> Result<(), ErrorBody> {
        self.remember_runtime();
        let LegacyRegistration {
            id,
            trigger_type,
            target,
            config,
            metadata,
        } = registration;
        // Old triggers had no id length, no `max_per_connection` and no
        // `max_per_function`: neither do their observers (compatibility).
        if id.is_empty() {
            return Err(ErrorBody::new(
                observe_codes::INVALID_ARGS,
                "a trigger id is a non-empty string",
            ));
        }
        // Everything that can refuse the registration is checked before an
        // existing binding with this id is replaced: a failing one leaves it.
        if function == target {
            return Err(ErrorBody::new(
                observe_codes::SELF_TARGET,
                format!("`{}` cannot be the target of its own observer", function.1),
            ));
        }
        self.check_target_invocable(&target)?;
        if let Some(descriptor) = self
            .function_entry(&function)
            .and_then(|entry| entry.descriptor())
            && !descriptor.allow()
        {
            return Err(ErrorBody::new(
                observe_codes::NOT_OBSERVABLE,
                format!("`{}` does not accept observers", function.1),
            ));
        }
        let settings = self.config();
        let legacy = LegacyTrigger {
            trigger_type,
            config,
        };
        let filter = Filter::match_all();
        let capture = CaptureSet::from_list(&default_capture());
        let spec = ObserveSpec {
            function: function.clone(),
            target: target.clone(),
            args: Value::Object(Map::new()),
            filters: filter.normalized().clone(),
            capture: capture.to_list(),
            on: On::default(),
            deliver: Deliver::Input,
            metadata: metadata.clone(),
            max_fires: None,
            expires_in_ms: None,
            max_in_flight: settings.default_max_in_flight,
            legacy: Some(legacy.clone()),
        };

        if let Some(existing) = self.get(&id) {
            if existing.legacy.is_none() || !replace {
                return Err(ErrorBody::new(
                    observe_codes::OBSERVER_ID_CONFLICT,
                    format!("`{id}` is taken: by an observer, or by a newer registration"),
                ));
            }
            if existing.observing().connection == worker.id && existing.spec == spec {
                // A replay (a reattached worker registering its triggers
                // again): answered with how the registration stands.
                if existing
                    .legacy
                    .as_ref()
                    .is_some_and(|binding| binding.answered.load(Ordering::Acquire))
                {
                    let (state, reason) = existing.snapshot_state();
                    let error = (state == ObserverState::Error).then_some(reason).flatten();
                    self.notify(worker.id, legacy_result(&existing, error));
                }
                return Ok(());
            }
            // Re-registering a trigger id replaces the binding.
            self.dispose(
                &existing,
                observe_codes::UNOBSERVED,
                "replaced by a new RegisterTrigger with the same id",
            );
        }
        // Unregistering a trigger and registering its id again is ordinary:
        // the disposal is not replayed back to the registrant.
        self.tombstones.remove(&id);

        let observer = Arc::new(Observer::new(NewObserver {
            id: id.clone(),
            target: target.clone(),
            spec,
            args: Value::Object(Map::new()),
            filter,
            capture,
            on: On::default(),
            deliver: Deliver::Input,
            metadata,
            policy: Policy {
                max_fires: None,
                expires_in_ms: None,
                expires_at: None::<Instant>,
                max_in_flight: settings.default_max_in_flight.max(1),
                max_queue: settings.max_queue_per_observer,
            },
            warnings: Vec::new(),
            observing: Observing {
                connection: worker.id,
                worker_name: self
                    .deps
                    .get()
                    .and_then(|deps| deps.workers.get_worker_name(&worker.id)),
                session: worker.session.clone(),
            },
            flags: Flags {
                observing_connected: true,
                function_available: self.is_registered(&function),
                target_available: self.is_registered(&target),
                owner_acked: self.acked_by_default(&function),
                announced: None,
                rejected_by_owner: false,
                activated_once: false,
                failed: None,
                disposed: None,
            },
            function,
            legacy: Some(legacy),
        }));
        if !self.publish(&observer, worker.id, Vec::new()) {
            return Err(ErrorBody::new(
                observe_codes::OBSERVER_ID_CONFLICT,
                format!("`{id}` was registered concurrently"),
            ));
        }
        let function_available = self.is_registered(&observer.function);
        let target_available = self.is_registered(&observer.target);
        self.update(&observer, |flags| {
            flags.function_available = function_available;
            flags.target_available = target_available;
        });
        if worker.cleanup_claimed.load(Ordering::SeqCst) {
            self.connection_closed(worker.id, worker.reattach_token);
        }
        tracing::debug!(
            worker_id = %worker.id,
            trigger_id = %id,
            trigger_type = %observer.legacy.as_ref().map(|b| b.trigger.trigger_type.as_str()).unwrap_or_default(),
            function_id = %observer.function.1,
            target = %observer.target.1,
            "RegisterTrigger served by the legacy shim"
        );
        Ok(())
    }

    /// `UnregisterTrigger` for a trigger the shim made into an observer:
    /// disposes it, and its owner hears `observerremoved`. Returns whether
    /// `id` was one.
    pub fn unregister_legacy(&self, id: &str) -> bool {
        match self.get(id) {
            Some(observer) if observer.legacy.is_some() => {
                self.dispose(
                    &observer,
                    observe_codes::UNOBSERVED,
                    "unregistered by UnregisterTrigger",
                );
                true
            }
            _ => false,
        }
    }

    /// Bindings parked because nothing provided their type become observers
    /// of the function that now stands in for it. Only bindings of a
    /// connected worker: an ownerless one (`engine::register_trigger`) stays
    /// parked for an old-style provider.
    pub fn convert_pending_triggers(self: &Arc<Self>, engine: &Engine) {
        if engine.trigger_registry.pending_triggers.is_empty() {
            return;
        }
        let parked: Vec<crate::trigger::Trigger> = engine
            .trigger_registry
            .pending_triggers
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        for trigger in parked {
            let Some(registrant) = trigger
                .worker_id
                .and_then(|id| engine.worker_registry.get_worker(&id))
            else {
                continue;
            };
            if registrant.cleanup_claimed.load(Ordering::SeqCst)
                || engine
                    .trigger_registry
                    .resolve_provider_key(&trigger)
                    .is_some()
            {
                continue;
            }
            let Some(function) = self.legacy_function(
                &trigger.trigger_type,
                trigger.trigger_namespace.as_deref(),
                &trigger.home_namespace,
            ) else {
                continue;
            };
            // Claimed by removal, so a provider's drain cannot also take it;
            // only as it was read: one registered again meanwhile (another
            // config) is not replaced by this older copy.
            if engine
                .trigger_registry
                .pending_triggers
                .remove_if(&trigger.id, |_, parked| same_registration(parked, &trigger))
                .is_none()
            {
                continue;
            }
            let registration = LegacyRegistration {
                id: trigger.id.clone(),
                trigger_type: trigger.trigger_type.clone(),
                target: (trigger.namespace.clone(), trigger.function_id.clone()),
                config: trigger.config.clone(),
                metadata: trigger.metadata.clone(),
            };
            // A binding registered with this id since it was parked is newer:
            // it stays, and this older one is dropped.
            if let Err(error) =
                self.register_legacy_with(&registrant, function, registration, false)
                && error.code != observe_codes::OBSERVER_ID_CONFLICT
            {
                self.notify(
                    registrant.id,
                    Message::TriggerRegistrationResult {
                        id: trigger.id,
                        trigger_type: trigger.trigger_type,
                        function_id: trigger.function_id,
                        error: Some(error),
                    },
                );
            }
        }
    }

    /// The registrant's one `TriggerRegistrationResult`: success once the
    /// owner acked, or the error that ended the registration.
    pub(crate) fn answer_legacy(&self, observer: &Observer, error: Option<ErrorBody>) {
        let Some(binding) = &observer.legacy else {
            return;
        };
        if binding.answered.swap(true, Ordering::AcqRel) {
            return;
        }
        self.notify(
            observer.observing().connection,
            legacy_result(observer, error),
        );
    }
}

/// A binding the shim made, as the trigger builtins show it
/// (`engine::registered-triggers::*`).
#[derive(Debug, Clone)]
pub struct ShimBinding {
    pub id: String,
    pub trigger_type: String,
    /// The trigger's function.
    pub target: FnKey,
    pub config: Value,
    pub metadata: Option<Value>,
    /// The function standing in for the trigger type.
    pub served_by: FnKey,
    /// The stand-in is registered: the binding is live, as an old trigger
    /// with a live provider was, from the moment it was stored (also while
    /// the owner's ack is in flight). Not served (the owner away), it is
    /// parked, as an old trigger whose provider left.
    pub served: bool,
    /// The observer's state (`pending` while the owner's ack is in flight).
    pub state: crate::protocol::ObserverState,
}

/// A trigger type a stand-in function serves (`engine::triggers::*`).
#[derive(Debug, Clone)]
pub struct ShimType {
    pub trigger_type: String,
    pub function: FnKey,
    /// The stand-in's owner connection.
    pub owner: Option<uuid::Uuid>,
    /// The stand-in's `request_format`: what its targets receive.
    pub request_format: Option<Value>,
    /// The stand-in's `args_format`: the old trigger config it reads.
    pub config_format: Option<Value>,
}

impl ObserverRegistry {
    /// The bindings the shim made, except the ones that failed (an owner
    /// rejection: the registration did not happen).
    pub fn shim_bindings(&self) -> Vec<ShimBinding> {
        self.by_id
            .iter()
            .filter_map(|entry| shim_binding_of(entry.value()))
            .collect()
    }

    pub fn shim_binding(&self, id: &str) -> Option<ShimBinding> {
        self.get(id).as_deref().and_then(shim_binding_of)
    }

    /// The trigger types stand-in functions serve now: registered, by an
    /// owner that speaks `observe.v1`.
    pub fn shim_types(&self) -> Vec<ShimType> {
        let declared: Vec<(FnKey, crate::protocol::ObserveDescriptor)> = self
            .by_function
            .iter()
            .filter_map(|entry| {
                let descriptor = entry.value().descriptor()?;
                descriptor
                    .legacy_trigger_type
                    .is_some()
                    .then(|| (entry.key().clone(), descriptor))
            })
            .collect();
        declared
            .into_iter()
            .filter_map(|(key, descriptor)| {
                let owner = self.owner_of(&key)?;
                if !owner.speaks_observe || !self.is_registered(&key) {
                    return None;
                }
                let request_format = self.function_formats(&key).and_then(|(request, _)| request);
                Some(ShimType {
                    trigger_type: descriptor.legacy_trigger_type.clone()?,
                    function: key,
                    owner: Some(owner.connection),
                    request_format,
                    config_format: descriptor.args_format.clone(),
                })
            })
            .collect()
    }
}

fn shim_binding_of(observer: &Observer) -> Option<ShimBinding> {
    let binding = observer.legacy.as_ref()?;
    let flags = observer.flags();
    if flags.failed.is_some() || flags.disposed.is_some() {
        return None;
    }
    Some(ShimBinding {
        id: observer.id.clone(),
        trigger_type: binding.trigger.trigger_type.clone(),
        target: observer.target.clone(),
        config: binding.trigger.config.clone(),
        metadata: observer.metadata.clone(),
        served_by: observer.function.clone(),
        served: flags.function_available,
        state: observer.state(),
    })
}

/// Whether two bindings are the same registration (`Trigger` compares ids
/// only).
fn same_registration(a: &crate::trigger::Trigger, b: &crate::trigger::Trigger) -> bool {
    a.id == b.id
        && a.worker_id == b.worker_id
        && a.trigger_type == b.trigger_type
        && a.function_id == b.function_id
        && a.namespace == b.namespace
        && a.config == b.config
        && a.metadata == b.metadata
        && a.trigger_namespace == b.trigger_namespace
        && a.home_namespace == b.home_namespace
}

fn legacy_result(observer: &Observer, error: Option<ErrorBody>) -> Message {
    Message::TriggerRegistrationResult {
        id: observer.id.clone(),
        trigger_type: observer
            .legacy
            .as_ref()
            .map(|binding| binding.trigger.trigger_type.clone())
            .unwrap_or_default(),
        function_id: observer.target.1.clone(),
        error,
    }
}
