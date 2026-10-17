// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The observed side: one [`FunctionEntry`] per function that has observers or
//! an `observe` descriptor, kept in step with the function registry.
//!
//! Owner sync (protocol.md §2.3, §3.3): an owner that speaks `observe.v1`
//! learns its function's observers from `functionregistered` (the full set,
//! on every registration) and `observeradded` / `observerremoved` (one at a
//! time, with a per-function revision), and an observer is `active` only once
//! the owner acked it. An owner that does not speak observe (an older SDK, an
//! engine-hosted or HTTP-invoked function) has nothing to ack: its observers go
//! `active` as soon as the function and the target are registered.
//!
//! The owner is whoever holds the function in the engine's `function_owners`
//! map. Every message to it for one function is sent under the entry's `sync`
//! lock, so an observer is either in the `functionregistered` snapshot or gets
//! its own `observeradded`, never both and never neither.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde_json::Value;
use uuid::Uuid;

use super::{
    FnKey, ObserverRegistry,
    lifecycle::{Announcement, Flags, Observer, lock},
};
use crate::{
    engine::{Engine, InvocationKind},
    function::FunctionChange,
    protocol::{Delivery, ErrorBody, Message, ObserveDescriptor, ObserverView, observe_codes},
    worker_connections::WorkerConnection,
};

/// What the engine knows about one observed (or observable-with-a-descriptor)
/// function.
pub(crate) struct FunctionEntry {
    /// From the current registration's `RegisterFunction.observe`. `None`:
    /// `delivery: auto`, observable, no args.
    descriptor: Mutex<Option<ObserveDescriptor>>,
    /// Every live observer of the function, in any state but `disposed`.
    observers: Mutex<Vec<Arc<Observer>>>,
    /// What the current owner has been told. Every owner-sync message for the
    /// function is sent while holding it.
    sync: Mutex<OwnerSync>,
    /// Mirrors `sync.announced_to.is_some()`, for [`Self::is_unused`], which
    /// runs inside a `by_function` shard lock and must not wait for `sync`.
    announced: AtomicBool,
    /// Set (under `sync`) when the entry is dropped as unused: whoever holds
    /// `sync` on an entry that is not dead may use it, and it stays in the map
    /// until they let go.
    dead: AtomicBool,
}

struct OwnerSync {
    /// Increases by one on every `observeradded` / `observerremoved`.
    revision: u64,
    /// The `observe.v1` connection that got this function's
    /// `functionregistered`; `None` until then and once it is gone.
    announced_to: Option<Uuid>,
}

/// `entry`'s `sync`, unless the entry was dropped (between finding it and
/// locking it): a dropped entry is never used.
fn lock_if_live(entry: &FunctionEntry) -> Option<std::sync::MutexGuard<'_, OwnerSync>> {
    let sync = lock(&entry.sync);
    (!entry.dead.load(Ordering::Acquire)).then_some(sync)
}

/// Functions whose last revision is kept after their entry is dropped.
const MAX_KEPT_REVISIONS: usize = 65_536;

/// The connection that owns a function, from `function_owners`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Owner {
    pub connection: Uuid,
    /// It declared `observe.v1` and hosts the function itself (not an
    /// HTTP-invoked registration): it acks its observers.
    pub speaks_observe: bool,
}

impl FunctionEntry {
    pub fn descriptor(&self) -> Option<ObserveDescriptor> {
        lock(&self.descriptor).clone()
    }

    pub fn observers(&self) -> Vec<Arc<Observer>> {
        lock(&self.observers).clone()
    }

    /// Observers that count against `observers.max_per_function`: not the
    /// ones the legacy shim made from old triggers.
    pub fn limited_observer_count(&self) -> usize {
        lock(&self.observers)
            .iter()
            .filter(|observer| observer.legacy.is_none())
            .count()
    }

    /// The routed hook evaluates auto functions only; an owner-delivered
    /// function's events are picked and filtered by its owner.
    pub fn is_auto(&self) -> bool {
        lock(&self.descriptor)
            .as_ref()
            .is_none_or(|d| d.delivery() == Delivery::Auto)
    }

    /// A new entry; `revision` continues the function's revisions from an
    /// earlier entry, so they never go back within an engine's lifetime.
    fn starting_at(revision: u64) -> Self {
        Self {
            descriptor: Mutex::new(None),
            observers: Mutex::new(Vec::new()),
            sync: Mutex::new(OwnerSync {
                revision,
                announced_to: None,
            }),
            announced: AtomicBool::new(false),
            dead: AtomicBool::new(false),
        }
    }

    fn is_unused(&self) -> bool {
        lock(&self.observers).is_empty()
            && lock(&self.descriptor).is_none()
            && !self.announced.load(Ordering::Acquire)
    }
}

/// The active observers of one auto function, as the routed hook sees them,
/// with the parts of an invocation any of them needs.
pub struct ActiveSet {
    pub(crate) observers: Vec<Arc<Observer>>,
    pub(crate) needs: Needs,
    /// The function is an engine-hosted event source: its events come from
    /// [`ObserverRegistry::emit`], never from routed calls.
    pub(crate) event_source: bool,
}

/// Which parts of an invocation the hook must keep: the union of what the
/// observers' filters read and what they capture or deliver.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Needs {
    pub input: bool,
    pub output: bool,
    pub error: bool,
    pub caller: bool,
    pub metadata: bool,
}

impl ActiveSet {
    fn new(observers: Vec<Arc<Observer>>, event_source: bool) -> Self {
        let mut needs = Needs::default();
        for observer in &observers {
            let roots = observer.filter.roots();
            let capture = observer.capture;
            needs.input |=
                roots.input || capture.input || observer.deliver == crate::protocol::Deliver::Input;
            needs.output |= roots.output
                || capture.output
                || observer.deliver == crate::protocol::Deliver::Output;
            needs.error |= roots.error || capture.error;
            needs.caller |= roots.caller || capture.caller;
            needs.metadata |= roots.metadata || capture.metadata;
        }
        Self {
            observers,
            needs,
            event_source,
        }
    }
}

/// Checks `args` against the function's descriptor: `NOT_OBSERVABLE` when it
/// opted out, `INVALID_ARGS` when `args` fail its `args_format` (an absent
/// `args` is `{}`, so a missing required argument fails too) or when a
/// function without one receives any.
pub(crate) fn check_descriptor(
    descriptor: Option<&ObserveDescriptor>,
    function: &FnKey,
    args: &Value,
) -> Result<(), ErrorBody> {
    if descriptor.is_some_and(|d| !d.allow()) {
        return Err(ErrorBody::new(
            observe_codes::NOT_OBSERVABLE,
            format!("`{}` does not accept observers", function.1),
        ));
    }
    match descriptor.and_then(|d| d.args_format.as_ref()) {
        Some(schema) => {
            let validator = jsonschema::Validator::new(schema).map_err(|err| {
                ErrorBody::new(
                    observe_codes::INVALID_ARGS,
                    format!("`{}` has an invalid args_format: {err}", function.1),
                )
            })?;
            let errors: Vec<String> = validator.iter_errors(args).map(|e| e.to_string()).collect();
            if errors.is_empty() {
                Ok(())
            } else {
                Err(ErrorBody::new(
                    observe_codes::INVALID_ARGS,
                    format!(
                        "args do not match `{}`'s args_format: {}",
                        function.1,
                        errors.join("; ")
                    ),
                ))
            }
        }
        None if is_empty_args(args) => Ok(()),
        None => Err(ErrorBody::new(
            observe_codes::INVALID_ARGS,
            format!(
                "`{}` takes no args: only owner-delivered functions with an args_format do",
                function.1
            ),
        )),
    }
}

fn is_empty_args(args: &Value) -> bool {
    match args {
        Value::Null => true,
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

/// Validates a `RegisterFunction.observe` descriptor
/// (`INVALID_OBSERVE_DESCRIPTOR`): a known delivery, and an `args_format` only
/// with `delivery: owner`, compiling as a JSON Schema.
pub fn validate_descriptor(descriptor: &ObserveDescriptor) -> Result<(), String> {
    let delivery = descriptor.delivery();
    if !delivery.is_known() {
        return Err(format!("unknown observe.delivery `{delivery}`"));
    }
    if let Some(schema) = &descriptor.args_format {
        if delivery != Delivery::Owner {
            return Err(
                "observe.args_format requires delivery `owner`: an auto function's events are its \
                 invocations, offered to every observer, so there is nothing to route by"
                    .to_string(),
            );
        }
        jsonschema::Validator::new(schema)
            .map_err(|err| format!("observe.args_format is not a valid JSON Schema: {err}"))?;
    }
    Ok(())
}

impl ObserverRegistry {
    /// Records the `observe` descriptor of the registration about to happen.
    /// Called before the function lands in the registry, so the change it
    /// causes already sees it.
    pub fn set_descriptor(
        &self,
        namespace: &str,
        function_id: &str,
        descriptor: Option<ObserveDescriptor>,
    ) {
        let key = (namespace.to_string(), function_id.to_string());
        self.mark_owner_delivered(
            &key,
            descriptor
                .as_ref()
                .is_some_and(|descriptor| descriptor.delivery() == Delivery::Owner),
        );
        match descriptor {
            Some(descriptor) => {
                self.with_live_entry(&key, |entry, _| {
                    *lock(&entry.descriptor) = Some(descriptor);
                });
            }
            None => {
                if let Some(entry) = self.by_function.get(&key) {
                    *lock(&entry.descriptor) = None;
                }
                self.drop_entry_if_unused(&key);
            }
        }
    }

    fn mark_owner_delivered(&self, key: &FnKey, owner_delivered: bool) {
        let current = self.owner_delivered.load();
        if current.contains(key) == owner_delivered {
            return;
        }
        let _serialized = lock(&self.rebuild);
        let current = self.owner_delivered.load();
        let mut next: HashSet<FnKey> = HashSet::clone(&current);
        if owner_delivered {
            next.insert(key.clone());
        } else {
            next.remove(key);
        }
        self.owner_delivered.store(Arc::new(next));
    }

    /// Whether `caller` (the calling connection, `None` for the engine
    /// itself) may invoke `(namespace, function_id)`. An owner-delivered
    /// function only by its owner (calling itself remotely, when it does not
    /// dispatch locally): its events are what the owner reports, stamped with
    /// the owner's identity, so a call from anyone else would let them forge
    /// events that look like the owner's (README §4.2, protocol.md §5). Every
    /// other function: no restriction here. One atomic load when no function
    /// is owner-delivered.
    #[inline]
    pub fn may_invoke(
        &self,
        namespace: &str,
        function_id: &str,
        caller: Option<Uuid>,
    ) -> Result<(), ErrorBody> {
        let owner_delivered = self.owner_delivered.load();
        if owner_delivered.is_empty()
            || !owner_delivered
                .contains(&(namespace, function_id) as &dyn crate::function::FunctionKey)
        {
            return Ok(());
        }
        let key: FnKey = (namespace.to_string(), function_id.to_string());
        let owner = self.owner_of(&key).map(|owner| owner.connection);
        if owner.is_some() && owner == caller {
            return Ok(());
        }
        Err(ErrorBody::new(
            observe_codes::FORBIDDEN,
            format!(
                "`{function_id}` is owner-delivered: only the worker that hosts it invokes it; \
                 observe it instead"
            ),
        ))
    }

    /// `FORBIDDEN` when `target` is owner-delivered: only its owner invokes
    /// it, so every activation would be refused (protocol.md §5).
    pub(crate) fn check_target_invocable(&self, target: &FnKey) -> Result<(), ErrorBody> {
        if self.owner_delivered.load().contains(target) {
            return Err(ErrorBody::new(
                observe_codes::FORBIDDEN,
                format!(
                    "the target `{}` is owner-delivered: only the worker that hosts it invokes it",
                    target.1
                ),
            ));
        }
        Ok(())
    }

    pub(crate) fn function_entry(&self, key: &FnKey) -> Option<Arc<FunctionEntry>> {
        self.by_function.get(key).map(|entry| entry.clone())
    }

    /// `key`'s entry, created if needed (continuing its revisions).
    fn entry_for(&self, key: &FnKey) -> Arc<FunctionEntry> {
        self.by_function
            .entry(key.clone())
            .or_insert_with(|| {
                let revision = self
                    .revisions
                    .remove(key)
                    .map(|(_, revision)| revision)
                    .unwrap_or(0);
                Arc::new(FunctionEntry::starting_at(revision))
            })
            .clone()
    }

    /// Drops `key`'s entry when nothing uses it, keeping its revision. The
    /// entry is marked dead under its `sync`, tried and never waited for (a
    /// shard lock is held here): an entry someone is using stays, and goes
    /// at the next change that finds it unused.
    fn drop_entry_if_unused(&self, key: &FnKey) {
        let mut revision = 0;
        let dropped = self.by_function.remove_if(key, |_, entry| {
            let sync = match entry.sync.try_lock() {
                Ok(sync) => sync,
                Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => return false,
            };
            if !entry.is_unused() {
                return false;
            }
            entry.dead.store(true, Ordering::Release);
            revision = sync.revision;
            true
        });
        if dropped.is_some() && revision > 0 {
            self.keep_revision(key, revision);
        }
    }

    /// Remembers the last revision of a function whose entry was dropped, for
    /// at most [`MAX_KEPT_REVISIONS`] functions (the oldest are forgotten:
    /// an owner replaces its set on `functionregistered` anyway).
    fn keep_revision(&self, key: &FnKey, revision: u64) {
        self.revisions.insert(key.clone(), revision);
        let mut order = lock(&self.revision_order);
        order.push_back((key.clone(), revision));
        while self.revisions.len() > MAX_KEPT_REVISIONS {
            let Some((oldest, kept)) = order.pop_front() else {
                break;
            };
            self.revisions
                .remove_if(&oldest, |_, current| *current == kept);
        }
        if order.len() > 2 * MAX_KEPT_REVISIONS {
            order.retain(|(key, kept)| self.revisions.get(key).is_some_and(|r| *r == *kept));
        }
    }

    /// Runs `f` with `key`'s entry (created if needed), its `sync` held, and
    /// the entry live: one dropped concurrently is never used.
    fn with_live_entry<R>(
        &self,
        key: &FnKey,
        f: impl FnOnce(&Arc<FunctionEntry>, &mut OwnerSync) -> R,
    ) -> R {
        loop {
            let entry = self.entry_for(key);
            if let Some(mut sync) = lock_if_live(&entry) {
                return f(&entry, &mut sync);
            }
        }
    }

    /// Runs `f` with `key`'s live entry, its `sync` held, and the function's
    /// owner read while holding it. `function_owners` is only tried, never
    /// waited on: a release holds its shard while the change it causes takes
    /// `sync`, so waiting here could deadlock; when the shard is busy, `sync`
    /// is let go and taken again. An entry dropped as unused meanwhile is not
    /// the live one, and is not used.
    fn with_owner_sync<R>(
        &self,
        key: &FnKey,
        f: impl FnOnce(&Arc<FunctionEntry>, &mut OwnerSync, Option<Owner>) -> R,
    ) -> R {
        loop {
            let entry = self.entry_for(key);
            let Some(mut sync) = lock_if_live(&entry) else {
                continue;
            };
            match self.try_owner_of(key) {
                Some(owner) => return f(&entry, &mut sync, owner),
                None => {
                    drop(sync);
                    std::thread::yield_now();
                }
            }
        }
    }

    /// Adds `observer` to its function. An owner that already got the
    /// function's `functionregistered` hears about it now (`observeradded`),
    /// and the observer waits for its ack.
    pub(crate) fn attach_to_function(self: &Arc<Self>, observer: &Arc<Observer>) {
        self.with_owner_sync(&observer.function, |entry, sync, owner| {
            lock(&entry.observers).push(observer.clone());
            let Some(announced_to) = sync.announced_to else {
                // Not announced (yet): acked by default unless the owner
                // speaks observe, whose `functionregistered` will announce it.
                // Derived here, under `sync`, rather than when the observer
                // was built: an owner may have claimed the function since.
                let acked = !owner.is_some_and(|owner| owner.speaks_observe);
                self.update(observer, |flags| flags.owner_acked = acked);
                return;
            };
            sync.revision += 1;
            let revision = sync.revision;
            self.announce(observer, announced_to, revision);
            self.notify(
                announced_to,
                Message::ObserverAdded {
                    function_id: observer.function.1.clone(),
                    namespace: observer.function.0.clone(),
                    revision,
                    observer: view(observer),
                },
            );
        });
    }

    pub(crate) fn detach_from_function(&self, observer: &Arc<Observer>) {
        if let Some(entry) = self.function_entry(&observer.function) {
            lock(&entry.observers).retain(|o| !Arc::ptr_eq(o, observer));
        }
        self.drop_entry_if_unused(&observer.function);
        self.refresh_active(&observer.function);
    }

    /// Rebuilds the routed hook's view of `key`: its active observers, when it
    /// is an auto function with any. Copy-on-write, so the hot path reads a
    /// consistent snapshot with one atomic load.
    pub(crate) fn refresh_active(&self, key: &FnKey) {
        let _serialized = lock(&self.rebuild);
        let active: Vec<Arc<Observer>> = match self.function_entry(key) {
            Some(entry) if entry.is_auto() => entry
                .observers()
                .into_iter()
                .filter(|o| o.is_active())
                .collect(),
            _ => Vec::new(),
        };
        let current = self.observed.load();
        if active.is_empty() && !current.contains_key(key) {
            return;
        }
        let mut next: HashMap<FnKey, Arc<ActiveSet>> = HashMap::clone(&current);
        if active.is_empty() {
            next.remove(key);
        } else {
            let event_source = self.event_sources.contains_key(key);
            next.insert(key.clone(), Arc::new(ActiveSet::new(active, event_source)));
        }
        self.observed.store(Arc::new(next));
    }

    /// A function was registered or removed, in any namespace and by any path
    /// (worker, HTTP-invoked, in-process). Synchronous and non-blocking: it
    /// only flips flags and queues notifications.
    ///
    /// A registration starts a new owner relationship: until the owner (if it
    /// speaks observe) gets `functionregistered` and acks, its observers stay
    /// `pending`.
    pub fn on_function_change(&self, change: FunctionChange, namespace: &str, function_id: &str) {
        let key = (namespace.to_string(), function_id.to_string());
        let registered = change == FunctionChange::Registered;

        if let Some(entry) = self.function_entry(&key) {
            if !registered {
                // The descriptor belongs to the registration that just ended.
                *lock(&entry.descriptor) = None;
                self.mark_owner_delivered(&key, false);
            }
            // Only a registration reads `function_owners`: a release calls
            // this while holding the function's shard.
            let acked = registered && self.acked_by_default(&key);
            {
                let mut sync = lock(&entry.sync);
                sync.announced_to = None;
                entry.announced.store(false, Ordering::Release);
            }
            let descriptor = entry.descriptor();
            for observer in entry.observers() {
                observer.cancel_ack_timer();
                if registered
                    && let Err(error) = self.check_on_registration(&observer, descriptor.as_ref())
                {
                    self.fail(&observer, error);
                    continue;
                }
                self.update(&observer, |flags| {
                    flags.function_available = registered;
                    flags.owner_acked = acked;
                    flags.announced = None;
                    flags.rejected_by_owner = false;
                });
            }
            // The delivery may have changed with the new registration.
            self.refresh_active(&key);
            self.drop_entry_if_unused(&key);
        }

        let targeting: Vec<String> = self
            .by_target
            .get(&key)
            .map(|ids| ids.iter().cloned().collect())
            .unwrap_or_default();
        for id in targeting {
            if let Some(observer) = self.by_id.get(&id).map(|o| o.clone()) {
                // A different worker may now host the target: it gets the
                // observed input, output and caller only if the observing
                // worker may invoke it, and only if anyone but its owner may
                // (an owner-delivered target refuses every activation).
                if registered
                    && let Err(error) = self
                        .check_session_allows(&observer, &key)
                        .and_then(|()| self.check_target_invocable(&key))
                {
                    self.fail(&observer, error);
                    continue;
                }
                self.update(&observer, |flags| flags.target_available = registered);
            }
        }
    }

    /// Who owns `key` now, from the engine's `function_owners` map. `None`
    /// for an engine-hosted function or one nobody registered.
    pub(crate) fn owner_of(&self, key: &FnKey) -> Option<Owner> {
        let deps = self.deps.get()?;
        let (connection, kind) = *deps.function_owners.get(key)?;
        Some(self.owner(connection, kind))
    }

    /// As [`Self::owner_of`] without waiting: `None` when `function_owners`'
    /// shard for `key` is locked (a claim or a release in progress).
    fn try_owner_of(&self, key: &FnKey) -> Option<Option<Owner>> {
        let Some(deps) = self.deps.get() else {
            return Some(None);
        };
        match deps.function_owners.try_get(key) {
            dashmap::try_result::TryResult::Present(found) => {
                let (connection, kind) = *found;
                drop(found);
                Some(Some(self.owner(connection, kind)))
            }
            dashmap::try_result::TryResult::Absent => Some(None),
            dashmap::try_result::TryResult::Locked => None,
        }
    }

    fn owner(&self, connection: Uuid, kind: InvocationKind) -> Owner {
        let speaks_observe = kind == InvocationKind::Regular
            && self.deps.get().is_some_and(|deps| {
                deps.workers
                    .get_worker(&connection)
                    .is_some_and(|worker| worker.features.observe_v1())
            });
        Owner {
            connection,
            speaks_observe,
        }
    }

    /// Whether an observer of `key` counts as acked without an `observerack`:
    /// true unless its owner speaks observe.
    pub(crate) fn acked_by_default(&self, key: &FnKey) -> bool {
        !self.owner_of(key).is_some_and(|owner| owner.speaks_observe)
    }

    /// `RegisterFunction` from `worker` was applied: the success ack an
    /// `observe.v1` owner gets (protocol.md §2.3), with the function's full
    /// observer set, each of which it must ack again. Also says whether the
    /// SDK may run the function in-process: the global switch, a session
    /// without RBAC middleware (a local call would bypass it), and a
    /// connection allowed to invoke the function itself.
    ///
    /// Sent once per registration: a second call for the same one (a late
    /// `engine::workers::register` racing the registration it catches up on)
    /// finds it announced already and sends nothing.
    pub fn function_registered(
        self: &Arc<Self>,
        engine: &Engine,
        worker: &WorkerConnection,
        namespace: &str,
        function_id: &str,
    ) {
        if !worker.features.observe_v1() {
            return;
        }
        self.remember_runtime();
        let key: FnKey = (namespace.to_string(), function_id.to_string());
        let local_dispatch = self.config().local_dispatch
            && worker.session.as_ref().is_none_or(|session| {
                session.config.middleware_function_id.is_none()
                    && engine.is_function_allowed(session, Some(namespace), function_id)
            });

        let announced = self.with_owner_sync(&key, |entry, sync, owner| {
            // Under `sync`: a teardown that releases the function clears the
            // announcement under it too, so it cannot land between this check
            // and the announcement and leave one for a dead connection.
            if owner.map(|owner| owner.connection) != Some(worker.id)
                || sync.announced_to == Some(worker.id)
            {
                return None;
            }
            sync.announced_to = Some(worker.id);
            entry.announced.store(true, Ordering::Release);
            // A new revision for every announcement: an ack for an earlier
            // one (the same connection registering again) is stale.
            sync.revision += 1;
            let revision = sync.revision;
            let observers: Vec<Arc<Observer>> = entry
                .observers()
                .into_iter()
                .filter(|o| {
                    let flags = o.flags();
                    flags.failed.is_none() && flags.disposed.is_none()
                })
                .collect();
            for observer in &observers {
                self.announce(observer, worker.id, revision);
            }
            self.notify(
                worker.id,
                Message::FunctionRegistered {
                    id: function_id.to_string(),
                    namespace: namespace.to_string(),
                    revision,
                    local_dispatch,
                    observers: observers.iter().map(|o| view(o)).collect(),
                },
            );
            Some((entry.clone(), revision, observers.len()))
        });
        let Some((entry, revision, observers)) = announced else {
            // Not the owner (or announced already): an entry made just for
            // this call goes again.
            self.drop_entry_if_unused(&key);
            return;
        };
        tracing::debug!(
            worker_id = %worker.id,
            function_id = %function_id,
            namespace = %namespace,
            revision,
            observers,
            local_dispatch,
            "functionregistered"
        );
        // A stand-in for a trigger type: bindings parked for it become its
        // observers now (protocol.md §6).
        if entry
            .descriptor()
            .is_some_and(|descriptor| descriptor.legacy_trigger_type.is_some())
        {
            self.convert_pending_triggers(engine);
        }
    }

    /// Records that `owner` is being told about `observer` at `revision` (the
    /// caller holds the function's `sync` and sends the message): the
    /// observer waits for the ack of this announcement.
    fn announce(self: &Arc<Self>, observer: &Arc<Observer>, owner: Uuid, revision: u64) {
        let generation = self.announcements.fetch_add(1, Ordering::Relaxed) + 1;
        // Nothing to dispose here: clearing the ack only gates `active`.
        self.update(observer, |flags| {
            flags.owner_acked = false;
            flags.rejected_by_owner = false;
            flags.announced = Some(Announcement {
                owner,
                revision,
                generation,
            });
        });
        self.arm_ack_timer(observer, generation);
    }

    /// `observerack` from `connection`: the owner indexed the observer (or
    /// rejected it with `error`, now or later). It counts only for the
    /// announcement it answers (that owner, that revision or a later one),
    /// checked and applied under the observer's status lock, so an owner
    /// change or a re-announcement in between voids it.
    #[allow(clippy::too_many_arguments)]
    pub fn observer_ack(
        self: &Arc<Self>,
        connection: Uuid,
        connection_namespace: &str,
        function_id: &str,
        namespace: Option<&str>,
        id: &str,
        revision: u64,
        error: Option<ErrorBody>,
    ) {
        let key: FnKey = (
            namespace.unwrap_or(connection_namespace).to_string(),
            function_id.to_string(),
        );
        let Some(observer) = self.get(id).filter(|o| o.function == key) else {
            tracing::debug!(observer_id = %id, function_id = %function_id, "observerack for an unknown observer; ignoring");
            return;
        };
        let answers = |flags: &Flags| {
            flags.announced.is_some_and(|announced| {
                announced.owner == connection && revision >= announced.revision
            })
        };
        let mut applied = false;
        let rejected = error.is_some();
        match error {
            None => self.update(&observer, |flags| {
                if answers(flags) && flags.failed.is_none() {
                    flags.owner_acked = true;
                    applied = true;
                }
            }),
            Some(error) => {
                tracing::debug!(observer_id = %id, code = %error.code, "observer rejected by its owner");
                self.update(&observer, |flags| {
                    if answers(flags) && flags.failed.is_none() && flags.disposed.is_none() {
                        // An owner that rejects an observer it never acked did
                        // not add it, so it hears no `observerremoved`; a late
                        // rejection of an acked one does.
                        flags.rejected_by_owner = !flags.owner_acked;
                        flags.failed = Some(error);
                        applied = true;
                    }
                });
            }
        }
        if !applied {
            tracing::debug!(
                observer_id = %id,
                connection = %connection,
                revision,
                "observerack that answers no current announcement; ignoring"
            );
            return;
        }
        observer.cancel_ack_timer();
        if !rejected {
            // The registration is done even while the target is missing:
            // like a trigger, the binding waits for it.
            self.answer_legacy(&observer, None);
        }
    }

    /// The observer left the owner's set (error, disposal): the owner hears
    /// `observerremoved` if it was told about it and did not reject it.
    pub(crate) fn withdraw_from_owner(&self, observer: &Arc<Observer>) {
        observer.cancel_ack_timer();
        let Some(entry) = self.function_entry(&observer.function) else {
            return;
        };
        let mut sync = lock(&entry.sync);
        let (announced, rejected) = observer.take_announcement();
        let (Some(announced), Some(owner)) = (announced, sync.announced_to) else {
            return;
        };
        if rejected || announced.owner != owner {
            return;
        }
        sync.revision += 1;
        self.notify(
            owner,
            Message::ObserverRemoved {
                function_id: observer.function.1.clone(),
                namespace: observer.function.0.clone(),
                revision: sync.revision,
                id: observer.id.clone(),
            },
        );
    }

    /// `OWNER_TIMEOUT` unless the owner acks the announcement `generation`
    /// within `observers.owner_ack_timeout_ms`. Checked under the status lock:
    /// a timer that fires while the observer is announced again does nothing.
    fn arm_ack_timer(self: &Arc<Self>, observer: &Arc<Observer>, generation: u64) {
        let Some(runtime) = self.runtime() else {
            return;
        };
        let timeout = Duration::from_millis(self.config().owner_ack_timeout_ms);
        let registry = Arc::downgrade(self);
        let weak = Arc::downgrade(observer);
        let handle = runtime.spawn(async move {
            tokio::time::sleep(timeout).await;
            if let (Some(registry), Some(observer)) = (registry.upgrade(), weak.upgrade()) {
                registry.ack_timed_out(&observer, generation);
            }
        });
        observer.set_ack_timer(handle.abort_handle());
    }

    /// The ack timeout of announcement `generation` elapsed: `OWNER_TIMEOUT`,
    /// unless the observer was acked, failed, disposed or announced again
    /// meanwhile (checked under its status lock, as a re-announcement is).
    fn ack_timed_out(&self, observer: &Arc<Observer>, generation: u64) {
        self.update(observer, |flags| {
            let waiting = flags
                .announced
                .is_some_and(|announced| announced.generation == generation)
                && !flags.owner_acked
                && flags.failed.is_none()
                && flags.disposed.is_none();
            if waiting {
                flags.failed = Some(ErrorBody::new(
                    observe_codes::OWNER_TIMEOUT,
                    "the function's owner did not ack the observer within \
                     observers.owner_ack_timeout_ms",
                ));
            }
        });
    }

    /// What `observe` could not check because the function was not registered
    /// yet, and what a re-registration may have changed: the descriptor (opt-out,
    /// args) and the filter paths against the formats.
    ///
    /// Every registration is checked afresh: the new registration may come
    /// from another worker, with other formats, under RBAC that sees it
    /// differently (an `expose_functions` rule on its metadata).
    fn check_on_registration(
        &self,
        observer: &Arc<Observer>,
        descriptor: Option<&ObserveDescriptor>,
    ) -> Result<(), ErrorBody> {
        if observer.flags().failed.is_some() {
            return Ok(());
        }
        if observer.legacy.is_some() {
            // The registrant named a trigger type, not this function, and
            // its config is the owner's to validate (protocol.md §6): only
            // an opt-out applies.
            return match descriptor {
                Some(descriptor) if !descriptor.allow() => Err(ErrorBody::new(
                    observe_codes::NOT_OBSERVABLE,
                    format!("`{}` does not accept observers", observer.function.1),
                )),
                _ => Ok(()),
            };
        }
        self.check_session_allows(observer, &observer.function)?;
        check_descriptor(descriptor, &observer.function, &observer.args)?;
        if let Some((request, response)) = self.function_formats(&observer.function) {
            observer
                .filter
                .check_schema_paths(request.as_ref(), response.as_ref())
                .map_err(|err| ErrorBody::new(err.code.as_str(), err.message))?;
        }
        Ok(())
    }

    /// `FORBIDDEN` unless the observing worker's session may invoke `key` as
    /// it is registered now. Creation-time RBAC saw the function (or nothing)
    /// as it was then; whoever registers it later must pass again.
    pub(crate) fn check_session_allows(
        &self,
        observer: &Observer,
        key: &FnKey,
    ) -> Result<(), ErrorBody> {
        let Some(session) = observer.observing().session else {
            return Ok(());
        };
        let Some(deps) = self.deps.get() else {
            return Ok(());
        };
        let allowed = deps.functions.with_function(&key.0, &key.1, |function| {
            crate::workers::worker::rbac_config::is_function_allowed_ref(
                &key.1,
                &key.0,
                session.config.rbac.as_ref(),
                &session.namespaces,
                &session.allowed_functions,
                &session.forbidden_functions,
                function,
            )
        });
        if allowed {
            Ok(())
        } else {
            Err(ErrorBody::new(
                observe_codes::FORBIDDEN,
                format!(
                    "function '{}' not allowed in namespace '{}' for the observing worker",
                    key.1, key.0
                ),
            ))
        }
    }

    /// The registered function's `request_format` / `response_format`, or
    /// `None` when it is not registered.
    pub(crate) fn function_formats(&self, key: &FnKey) -> Option<(Option<Value>, Option<Value>)> {
        let deps = self.deps.get()?;
        deps.functions.with_function(&key.0, &key.1, |function| {
            function.map(|f| (f.request_format.clone(), f.response_format.clone()))
        })
    }

    /// `worker` declared its features only after its registrations had been
    /// applied (the namespace grace timer drained them first), so they were
    /// handled as a legacy owner's. It gets `functionregistered` for each of
    /// them now, and their observers wait for its acks.
    pub async fn features_declared_late(
        self: &Arc<Self>,
        engine: &Engine,
        worker: &WorkerConnection,
        namespace: &str,
    ) {
        if !worker.features.observe_v1() {
            return;
        }
        for function_id in worker.get_regular_function_ids().await {
            self.function_registered(engine, worker, namespace, &function_id);
        }
    }

    pub(crate) fn is_registered(&self, key: &FnKey) -> bool {
        self.deps.get().is_some_and(|deps| {
            deps.functions
                .with_function(&key.0, &key.1, |f| f.is_some())
        })
    }
}

/// An observer as its function's owner sees it (CONTRACT D15).
pub(crate) fn view(observer: &Observer) -> ObserverView {
    ObserverView {
        id: observer.id.clone(),
        args: observer.args.clone(),
        filters: observer.filter.normalized().clone(),
        target: observer.target.1.clone(),
        capture: observer.capture.to_list(),
        on: observer.on.clone(),
        legacy: observer
            .legacy
            .as_ref()
            .map(|binding| binding.trigger.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key() -> FnKey {
        ("default".to_string(), "cron::on-tick".to_string())
    }

    #[test]
    fn auto_functions_take_no_args() {
        assert!(check_descriptor(None, &key(), &json!({})).is_ok());
        assert!(check_descriptor(None, &key(), &Value::Null).is_ok());
        let err =
            check_descriptor(None, &key(), &json!({ "expression": "* * * * * *" })).unwrap_err();
        assert_eq!(err.code, observe_codes::INVALID_ARGS);
    }

    #[test]
    fn args_are_validated_against_the_args_format_including_required_ones() {
        let descriptor = ObserveDescriptor {
            delivery: Some(Delivery::Owner),
            args_format: Some(json!({
                "type": "object", "required": ["expression"],
                "properties": { "expression": { "type": "string" } }
            })),
            ..ObserveDescriptor::default()
        };
        assert!(check_descriptor(Some(&descriptor), &key(), &json!({ "expression": "x" })).is_ok());
        let missing = check_descriptor(Some(&descriptor), &key(), &json!({})).unwrap_err();
        assert_eq!(missing.code, observe_codes::INVALID_ARGS);
        let wrong =
            check_descriptor(Some(&descriptor), &key(), &json!({ "expression": 5 })).unwrap_err();
        assert_eq!(wrong.code, observe_codes::INVALID_ARGS);
    }

    #[test]
    fn an_opted_out_function_is_not_observable() {
        let descriptor = ObserveDescriptor {
            allow: Some(false),
            ..ObserveDescriptor::default()
        };
        let err = check_descriptor(Some(&descriptor), &key(), &json!({})).unwrap_err();
        assert_eq!(err.code, observe_codes::NOT_OBSERVABLE);
    }

    #[test]
    fn descriptors_are_validated_at_registration() {
        assert!(validate_descriptor(&ObserveDescriptor::default()).is_ok());
        assert!(
            validate_descriptor(&ObserveDescriptor {
                delivery: Some(Delivery::Unknown("broadcast".into())),
                ..ObserveDescriptor::default()
            })
            .is_err()
        );
        assert!(
            validate_descriptor(&ObserveDescriptor {
                args_format: Some(json!({ "type": "object" })),
                ..ObserveDescriptor::default()
            })
            .is_err(),
            "args_format on an auto function"
        );
        assert!(
            validate_descriptor(&ObserveDescriptor {
                delivery: Some(Delivery::Owner),
                args_format: Some(json!({ "type": "not-a-type" })),
                ..ObserveDescriptor::default()
            })
            .is_err(),
            "a schema that does not compile"
        );
        assert!(
            validate_descriptor(&ObserveDescriptor {
                delivery: Some(Delivery::Owner),
                args_format: Some(json!({ "type": "object" })),
                ..ObserveDescriptor::default()
            })
            .is_ok()
        );
    }

    /// An ack timeout fires for the announcement it was armed for only: a
    /// re-announcement in between makes the first one's timer a no-op.
    #[tokio::test]
    async fn a_timer_for_an_earlier_announcement_does_nothing() {
        use super::super::events::test_support;
        let engine = std::sync::Arc::new(crate::engine::Engine::new());
        let _source = test_support::capture(&engine, "test::src");
        let _target = test_support::capture(&engine, "test::dst");
        let _socket = test_support::observe(&engine, "obs", "test::src", "test::dst", None).await;
        let registry = engine.observers.clone();
        let observer = registry.get("obs").unwrap();
        let owner = Uuid::new_v4();
        let generation = |o: &Observer| o.flags().announced.unwrap().generation;

        registry.announce(&observer, owner, 1);
        let first = generation(&observer);
        registry.announce(&observer, owner, 2);
        let second = generation(&observer);
        assert_ne!(first, second);

        registry.ack_timed_out(&observer, first);
        assert_eq!(
            observer.state(),
            crate::protocol::ObserverState::Pending,
            "stale timer"
        );
        registry.ack_timed_out(&observer, second);
        assert_eq!(observer.state(), crate::protocol::ObserverState::Error);
        assert_eq!(
            observer.reason().unwrap().code,
            observe_codes::OWNER_TIMEOUT
        );
    }

    /// An entry is dropped only while nobody holds its `sync`, and one that
    /// was dropped is never used again.
    #[test]
    fn a_dropped_entry_is_never_used() {
        let registry = ObserverRegistry::new();
        let key = key();
        let entry = registry.entry_for(&key);
        {
            let _held = lock(&entry.sync);
            registry.drop_entry_if_unused(&key);
            assert!(
                registry.function_entry(&key).is_some(),
                "in use: not dropped"
            );
        }
        registry.drop_entry_if_unused(&key);
        assert!(registry.function_entry(&key).is_none());
        assert!(entry.dead.load(Ordering::Acquire));
        let used = registry.with_live_entry(&key, |live, _| Arc::ptr_eq(live, &entry));
        assert!(!used, "a fresh entry, not the dropped one");
        // Someone still holding the dropped entry (found it just before it
        // went) does not get to use it.
        assert!(lock_if_live(&entry).is_none());
    }

    /// Revisions continue after an entry is dropped, and the kept ones are
    /// bounded.
    #[test]
    fn revisions_survive_a_dropped_entry() {
        let registry = ObserverRegistry::new();
        let key = key();
        registry.with_live_entry(&key, |_, sync| sync.revision = 7);
        registry.drop_entry_if_unused(&key);
        registry.with_live_entry(&key, |_, sync| assert_eq!(sync.revision, 7));
        for n in 0..(MAX_KEPT_REVISIONS + 10) {
            registry.keep_revision(&("ns".to_string(), format!("f{n}")), 1);
        }
        assert!(registry.revisions.len() <= MAX_KEPT_REVISIONS);
    }
}
