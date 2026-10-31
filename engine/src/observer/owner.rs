// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The observed side: one [`FunctionEntry`] per function that has observers or
//! options other than the defaults, kept in step with the function registry.
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
    borrow::Borrow,
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde_json::Value;
use uuid::Uuid;

use super::{
    FnKey, ObserverRegistry,
    lifecycle::{Announcement, Flags, Lock, Observer, lock},
    persistent::{Keyed, PersistentSet},
};
use crate::{
    engine::{Engine, InvocationKind},
    function::{FunctionChange, FunctionKey},
    protocol::{ErrorBody, Message, ObserverView, observe_codes},
    worker_connections::WorkerConnection,
};

/// What a function's registration says about observing it: the
/// `registerfunction` options `args_format`, `private` and `observable`.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionOptions {
    /// JSON Schema observers' `args` are validated against. `None`: the
    /// function takes no args.
    pub args_format: Option<Value>,
    /// Only the connection that hosts the function invokes it.
    pub private: bool,
    /// `false`: every `observe` of the function fails with `NOT_OBSERVABLE`.
    pub observable: bool,
}

impl Default for FunctionOptions {
    fn default() -> Self {
        Self {
            args_format: None,
            private: false,
            observable: true,
        }
    }
}

impl FunctionOptions {
    /// Private, with no args: an event source the engine or a worker hosts.
    pub fn private() -> Self {
        Self {
            private: true,
            ..Self::default()
        }
    }

    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What the engine knows about one observed function, or one registered with
/// options other than the defaults.
pub(crate) struct FunctionEntry {
    /// From the current registration. `None`: the defaults.
    options: Lock<Option<FunctionOptions>>,
    /// Every live observer of the function, in any state but `disposed`.
    observers: Lock<Members>,
    /// What the current owner has been told. Every owner-sync message for the
    /// function is sent while holding it.
    sync: Lock<OwnerSync>,
    /// Mirrors `sync.announced_to.is_some()`, for [`Self::is_unused`], which
    /// runs inside a `by_function` shard lock and must not wait for `sync`.
    announced: AtomicBool,
    /// Set (under `sync`) when the entry is dropped as unused: whoever holds
    /// `sync` on an entry that is not dead may use it, and it stays in the map
    /// until they let go.
    dead: AtomicBool,
    /// The latest time an owner moved on in this function's observer hooks,
    /// with that owner: it answered an observer, or the engine timed one out.
    /// The SDK runs one function's hooks one at a time, so an observer's ack
    /// timeout counts from here, not from its announcement (v2 §4.3).
    owner_progress: Lock<Option<(Uuid, tokio::time::Instant)>>,
}

/// A function's live observers, kept where only [`Members`]' own methods
/// reach them: adding, taking or finding one is `O(log n)`, counting them
/// `O(1)`, and the one scan, [`Members::to_vec`], is what the complexity
/// tests count, so a scan of every observer on each `observe` or
/// `unobserve` shows in them.
mod members {
    use std::{collections::BTreeMap, sync::Arc};

    use super::super::lifecycle::Observer;

    /// A function's live observers in creation order, and how many count
    /// against `observers.max_per_function`.
    #[derive(Default)]
    pub(super) struct Members {
        /// By [`Observer::seq`].
        by_seq: BTreeMap<u64, Arc<Observer>>,
        /// The ones that are not the engine's own.
        limited: usize,
    }

    impl Members {
        pub(super) fn add(&mut self, seq: u64, observer: &Arc<Observer>) {
            observer.set_member(true);
            if self.by_seq.insert(seq, observer.clone()).is_none() && !observer.is_engine_owned() {
                self.limited += 1;
            }
        }

        pub(super) fn take(&mut self, observer: &Arc<Observer>) {
            let Some(seq) = observer.seq() else {
                return;
            };
            if self
                .by_seq
                .get(&seq)
                .is_some_and(|held| Arc::ptr_eq(held, observer))
            {
                self.by_seq.remove(&seq);
                observer.set_member(false);
                if !observer.is_engine_owned() {
                    self.limited -= 1;
                }
            }
        }

        pub(super) fn holds(&self, observer: &Arc<Observer>) -> bool {
            observer.seq().is_some_and(|seq| {
                self.by_seq
                    .get(&seq)
                    .is_some_and(|held| Arc::ptr_eq(held, observer))
            })
        }

        pub(super) fn len(&self) -> usize {
            self.by_seq.len()
        }

        pub(super) fn is_empty(&self) -> bool {
            self.by_seq.is_empty()
        }

        /// The ones that count against `observers.max_per_function`.
        pub(super) fn limited(&self) -> usize {
            self.limited
        }

        /// All of them, in creation order: `O(n)`, and counted by the
        /// complexity tests.
        pub(super) fn to_vec(&self) -> Vec<Arc<Observer>> {
            #[cfg(test)]
            super::super::work::add(self.by_seq.len());
            self.by_seq.values().cloned().collect()
        }
    }
}

use members::Members;

struct OwnerSync {
    /// Increases by one on every `observeradded` / `observerremoved`.
    revision: u64,
    /// The `observe.v1` connection that got this function's
    /// `functionregistered`; `None` until then and once it is gone.
    announced_to: Option<Uuid>,
}

/// `entry`'s `sync`, unless the entry was dropped (between finding it and
/// locking it): a dropped entry is never used.
fn lock_if_live(entry: &FunctionEntry) -> Option<dashmap::RwLockWriteGuard<'_, OwnerSync>> {
    let sync = entry.sync.write();
    (!entry.dead.load(Ordering::Acquire)).then_some(sync)
}

/// One observer joining its function's active set (`true`) or leaving it.
type Change<'a> = (&'a Arc<Observer>, bool);

/// Functions whose last revision is kept after their entry is dropped.
const MAX_KEPT_REVISIONS: usize = 65_536;

/// A change of this many observers' membership, times this, past a
/// function's active observers rebuilds its set from the ones it keeps
/// ([`ObserverRegistry::refresh_observers`]): one observer's change copies
/// up to a node of slots, a rebuild one slot per observer kept.
const REBUILD_AT: usize = 64;

/// The connection that owns a function, from `function_owners`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Owner {
    pub connection: Uuid,
    /// It declared `observe.v1` and hosts the function itself (not an
    /// HTTP-invoked registration): it acks its observers.
    pub speaks_observe: bool,
}

impl FunctionEntry {
    pub fn options(&self) -> Option<FunctionOptions> {
        self.options.write().clone()
    }

    /// The live observers, in creation order.
    pub fn observers(&self) -> Vec<Arc<Observer>> {
        self.observers.write().to_vec()
    }

    /// Live observers.
    pub fn len(&self) -> usize {
        self.observers.write().len()
    }

    /// Observers that count against `observers.max_per_function`: not the
    /// ones the engine owns.
    pub fn limited_observer_count(&self) -> usize {
        self.observers.write().limited()
    }

    /// Whether `observer` is one of the function's live observers.
    fn holds(&self, observer: &Arc<Observer>) -> bool {
        self.observers.write().holds(observer)
    }

    /// A new entry; `revision` continues the function's revisions from an
    /// earlier entry, so they never go back within an engine's lifetime.
    fn starting_at(revision: u64) -> Self {
        Self {
            options: Lock::new(None),
            observers: Lock::new(Members::default()),
            sync: Lock::new(OwnerSync {
                revision,
                announced_to: None,
            }),
            announced: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            owner_progress: Lock::new(None),
        }
    }

    fn is_unused(&self) -> bool {
        self.observers.write().is_empty()
            && self.options.write().is_none()
            && !self.announced.load(Ordering::Acquire)
    }
}

/// The routed hook's view: what it knows of each function it routes (see
/// [`ObserverRegistry::route`]).
pub(crate) type RoutedView = HashMap<RoutedKey, Arc<Routed>>;

/// A function's `(namespace, function_id)` in the routed view, shared, so a
/// copy of the view copies no names. Looked up by `&dyn FunctionKey`, as a
/// `FnKey` is.
#[derive(Clone)]
pub(crate) struct RoutedKey(Arc<FnKey>);

impl FunctionKey for RoutedKey {
    fn parts(&self) -> (&str, &str) {
        (&self.0.0, &self.0.1)
    }
}

impl Hash for RoutedKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.parts().hash(state);
    }
}

impl PartialEq for RoutedKey {
    fn eq(&self, other: &Self) -> bool {
        self.parts() == other.parts()
    }
}

impl Eq for RoutedKey {}

impl<'a> Borrow<dyn FunctionKey + 'a> for RoutedKey {
    fn borrow(&self) -> &(dyn FunctionKey + 'a) {
        self
    }
}

/// `key` as the routed view looks it up.
fn routed_key(key: &FnKey) -> &dyn FunctionKey {
    key
}

/// What the routed hook knows of one function (see
/// [`ObserverRegistry::route`]): its active observers, and who may call it.
/// The active observers change in place: the view is copied only when a
/// function joins or leaves it, or who may call one changes.
pub(crate) struct Routed {
    pub(crate) active: ActiveSlot,
    pub(crate) callers: Callers,
}

impl Routed {
    fn new(active: Option<Arc<ActiveSet>>, callers: Callers) -> Arc<Self> {
        Arc::new(Self {
            active: ActiveSlot(Lock::new(active)),
            callers,
        })
    }
}

/// A function's current active set in the routed view. A reader holds the
/// lock only to clone the `Arc`, and a call keeps the version it cloned. A
/// change replaces the version under the lock and drops the old one after
/// letting it go: one compare-exchange each way, however many threads the
/// engine runs.
pub(crate) struct ActiveSlot(Lock<Option<Arc<ActiveSet>>>);

impl ActiveSlot {
    /// The current version.
    #[inline]
    pub(crate) fn load_full(&self) -> Option<Arc<ActiveSet>> {
        self.0.read().clone()
    }

    /// Whether there is a current version, read without cloning it.
    #[inline]
    pub(crate) fn is_some(&self) -> bool {
        self.0.read().is_some()
    }

    /// Replaces the current version. Called under `rebuild`.
    fn store(&self, active: Option<Arc<ActiveSet>>) {
        let previous = std::mem::replace(&mut *self.0.write(), active);
        drop(previous);
    }
}

/// Who may invoke a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Callers {
    /// Anyone: the function is not private.
    Anyone,
    /// Only this connection, which hosts the private function.
    Host(Uuid),
    /// Nobody: a private function the engine hosts (an engine-hosted
    /// source), whose calls are the engine's emissions. Its handler refuses
    /// every call; while it is observed, the routed hook refuses them before
    /// they are observed.
    Nobody,
}

/// Who may call `key`, for a change of its active observers: as the routed
/// view says, or, when it is not in the view (no connection hosts it
/// privately), nobody for a private function and anyone otherwise.
fn callers_in(view: &RoutedView, key: &FnKey, entry: Option<&FunctionEntry>) -> Callers {
    match view.get(routed_key(key)) {
        Some(routed) => routed.callers,
        None if entry
            .and_then(|entry| entry.options())
            .is_some_and(|options| options.private) =>
        {
            Callers::Nobody
        }
        None => Callers::Anyone,
    }
}

/// `FORBIDDEN`: a call of the private `function_id` from anyone but its host.
pub(crate) fn private_refusal(function_id: &str) -> ErrorBody {
    ErrorBody::new(
        observe_codes::FORBIDDEN,
        format!(
            "`{function_id}` is private: only the worker that hosts it calls it; observe it \
             instead"
        ),
    )
}

/// The active observers of one function, as the routed hook sees them, with
/// the parts of an invocation any of them needs. A call holds the version it
/// started with; an observer that becomes active or leaves meanwhile makes a
/// new version, sharing all but `O(log n)` of its nodes with this one.
pub struct ActiveSet {
    pub(crate) function: Arc<FnKey>,
    /// In creation order, by [`Observer::seq`].
    pub(crate) observers: PersistentSet<Arc<Observer>>,
    /// How many of `observers` need each part: `needs` and `listed_needs`
    /// follow from them as observers come and go.
    counts: NeedCounts,
    /// What a call the engine evaluates needs: what the observers' filters
    /// read, capture or deliver.
    pub(crate) needs: Needs,
    /// What a call whose host lists the observers needs: what they capture
    /// or deliver. The host evaluated their filters.
    pub(crate) listed_needs: Needs,
    /// The function's hash: which evaluator shard takes its completions.
    pub(crate) shard: u64,
}

/// Which parts of an invocation the hook must keep.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Needs {
    pub input: bool,
    pub output: bool,
    pub error: bool,
    pub caller: bool,
    pub metadata: bool,
}

impl Needs {
    /// What `observer` captures or delivers.
    fn delivered(observer: &Observer) -> Self {
        let capture = observer.capture;
        Self {
            input: capture.input || observer.deliver == crate::protocol::Deliver::Input,
            output: capture.output || observer.deliver == crate::protocol::Deliver::Output,
            error: capture.error,
            caller: capture.caller,
            metadata: capture.metadata,
        }
    }

    /// What `observer`'s filters read, captures or delivers.
    fn evaluated(observer: &Observer) -> Self {
        let roots = observer.filter.roots();
        let delivered = Self::delivered(observer);
        Self {
            input: roots.input || delivered.input,
            output: roots.output || delivered.output,
            error: roots.error || delivered.error,
            caller: roots.caller || delivered.caller,
            metadata: roots.metadata || delivered.metadata,
        }
    }

    #[cfg(test)]
    fn union(self, other: Self) -> Self {
        Self {
            input: self.input || other.input,
            output: self.output || other.output,
            error: self.error || other.error,
            caller: self.caller || other.caller,
            metadata: self.metadata || other.metadata,
        }
    }

    fn parts(self) -> [bool; 5] {
        [
            self.input,
            self.output,
            self.error,
            self.caller,
            self.metadata,
        ]
    }

    /// Each part that at least one observer needs.
    fn of_counts(counts: &[usize; 5]) -> Self {
        Self {
            input: counts[0] > 0,
            output: counts[1] > 0,
            error: counts[2] > 0,
            caller: counts[3] > 0,
            metadata: counts[4] > 0,
        }
    }
}

/// How many observers of an [`ActiveSet`] need each part (in
/// [`Needs::parts`] order): what the engine evaluates, and what a host lists.
#[derive(Debug, Clone, Copy, Default)]
struct NeedCounts {
    evaluated: [usize; 5],
    delivered: [usize; 5],
}

impl NeedCounts {
    fn count(&mut self, observer: &Observer, joined: bool) {
        for (counts, needs) in [
            (&mut self.evaluated, Needs::evaluated(observer)),
            (&mut self.delivered, Needs::delivered(observer)),
        ] {
            for (count, needed) in counts.iter_mut().zip(needs.parts()) {
                if needed {
                    if joined {
                        *count += 1;
                    } else {
                        *count -= 1;
                    }
                }
            }
        }
    }
}

/// An active set orders its observers by [`Observer::seq`]: each one in a
/// set joined its function, and its seq never changes after that.
impl Keyed for Arc<Observer> {
    #[inline]
    fn key(&self) -> u64 {
        self.seq().unwrap_or_default()
    }
}

impl ActiveSet {
    /// The set of `observers`, which are in creation order and have joined
    /// their function's observers.
    fn new(key: &FnKey, observers: Vec<Arc<Observer>>) -> Self {
        let observers: Vec<Arc<Observer>> = observers
            .into_iter()
            .filter(|observer| observer.seq().is_some())
            .collect();
        let mut counts = NeedCounts::default();
        for observer in &observers {
            counts.count(observer, true);
        }
        let observers = PersistentSet::from_sorted(observers);
        let shard = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut hasher);
            hasher.finish()
        };
        Self::of(Arc::new(key.clone()), observers, counts, shard)
    }

    fn of(
        function: Arc<FnKey>,
        observers: PersistentSet<Arc<Observer>>,
        counts: NeedCounts,
        shard: u64,
    ) -> Self {
        Self {
            function,
            observers,
            needs: Needs::of_counts(&counts.evaluated),
            listed_needs: Needs::of_counts(&counts.delivered),
            counts,
            shard,
        }
    }

    /// This set with `observer`, filed under its seq.
    fn with(&self, observer: &Arc<Observer>) -> Self {
        let Some(observers) = self.observers.inserted(observer.clone()) else {
            return self.copy();
        };
        let mut counts = self.counts;
        counts.count(observer, true);
        Self::of(self.function.clone(), observers, counts, self.shard)
    }

    /// This set without `observer`, filed under `seq`.
    fn without(&self, observer: &Arc<Observer>, seq: u64) -> Self {
        let Some(observers) = self.observers.removed(seq) else {
            return self.copy();
        };
        let mut counts = self.counts;
        counts.count(observer, false);
        Self::of(self.function.clone(), observers, counts, self.shard)
    }

    fn copy(&self) -> Self {
        Self::of(
            self.function.clone(),
            self.observers.clone(),
            self.counts,
            self.shard,
        )
    }

    /// What a call needs kept: when its host delivers it, only what the
    /// observers it lists capture or deliver.
    pub(crate) fn needs_for(&self, host_delivers: bool) -> Needs {
        if host_delivers {
            self.listed_needs
        } else {
            self.needs
        }
    }
}

/// Checks an observer against the function's options: `NOT_OBSERVABLE` when
/// the function is not observable, `INVALID_ARGS` when `args` fail its
/// `args_format` (an absent `args` is `{}`, so a missing required argument
/// fails too) or when a function without one receives any.
pub(crate) fn check_options(
    options: Option<&FunctionOptions>,
    function: &FnKey,
    args: &Value,
) -> Result<(), ErrorBody> {
    if options.is_some_and(|options| !options.observable) {
        return Err(ErrorBody::new(
            observe_codes::NOT_OBSERVABLE,
            format!("`{}` does not accept observers", function.1),
        ));
    }
    match options.and_then(|options| options.args_format.as_ref()) {
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
            format!("`{}` takes no args: it declares no args_format", function.1),
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

/// Validates a registration's `args_format` (`INVALID_ARGS_FORMAT`): it must
/// compile as a JSON Schema.
pub fn validate_args_format(schema: &Value) -> Result<(), String> {
    jsonschema::Validator::new(schema)
        .map(|_| ())
        .map_err(|err| format!("args_format is not a valid JSON Schema: {err}"))
}

impl ObserverRegistry {
    /// Records the options of the registration about to happen. Called before
    /// the function lands in the registry, so the change it causes already
    /// sees them, and after the registration claimed the function, so a
    /// private function's host is known.
    pub fn set_options(&self, namespace: &str, function_id: &str, options: FunctionOptions) {
        let key = (namespace.to_string(), function_id.to_string());
        let private = options.private;
        if options.is_default() {
            if let Some(entry) = self.by_function.get(&key) {
                *entry.options.write() = None;
            }
            self.drop_entry_if_unused(&key);
        } else {
            self.with_live_entry(&key, |entry, _| {
                *entry.options.write() = Some(options);
            });
        }
        self.set_callers(&key, private);
    }

    /// The options `(namespace, function_id)` is registered with now.
    pub fn options(&self, namespace: &str, function_id: &str) -> FunctionOptions {
        let key = (namespace.to_string(), function_id.to_string());
        self.function_entry(&key)
            .and_then(|entry| entry.options())
            .unwrap_or_default()
    }

    /// Records who may invoke `key` under its registration: anyone, or, when
    /// it is private, the connection that claimed it, or nobody when no
    /// connection did (the engine hosts it). A connection's private function
    /// stays in the routed hook's view while it is registered, observed or
    /// not.
    fn set_callers(&self, key: &FnKey, private: bool) {
        let callers = if private {
            match self
                .deps
                .get()
                .and_then(|deps| deps.function_owners.get(key).map(|owner| owner.0))
            {
                Some(host) => Callers::Host(host),
                None => Callers::Nobody,
            }
        } else {
            Callers::Anyone
        };
        let _serialized = self.rebuild.write();
        let current = self.routed.load();
        let routed = current.get(routed_key(key));
        if routed.map(|routed| routed.callers) == Some(callers) {
            return;
        }
        let active = routed.and_then(|routed| routed.active.load_full());
        self.store_routed(&current, key, active, callers);
    }

    /// Stores `key`'s view for the routed hook: kept while the function has
    /// active observers or a host only it may be called by, dropped
    /// otherwise. A function that stays in the view with the same callers
    /// gets its new active set in place; the view itself is copied only when
    /// a function joins or leaves it, or its callers change. Called under
    /// `rebuild`.
    fn store_routed(
        &self,
        current: &RoutedView,
        key: &FnKey,
        active: Option<Arc<ActiveSet>>,
        callers: Callers,
    ) {
        let kept = active.is_some() || matches!(callers, Callers::Host(_));
        self.watching_observers_changed(key, active.is_some());
        let held = current.get(routed_key(key));
        if let Some(held) = held
            && kept
            && held.callers == callers
        {
            held.active.store(active);
            return;
        }
        if !kept && held.is_none() {
            return;
        }
        let mut next: RoutedView = HashMap::clone(current);
        if kept {
            let routed = Routed::new(active, callers);
            match next.get_mut(routed_key(key)) {
                Some(held) => *held = routed,
                None => {
                    next.insert(RoutedKey(Arc::new(key.clone())), routed);
                }
            }
        } else {
            next.remove(routed_key(key));
        }
        self.routed.store(Arc::new(next));
    }

    /// The active set the routed view holds for `key` now. Called under
    /// `rebuild`.
    fn active_in(current: &RoutedView, key: &FnKey) -> Option<Arc<ActiveSet>> {
        current
            .get(routed_key(key))
            .and_then(|routed| routed.active.load_full())
    }

    /// The routed hook's one read (protocol.md §3.2; v2 §6): the active
    /// observers of `(namespace, function_id)` when it has any, or
    /// `FORBIDDEN` when it is private and `caller` (the calling connection,
    /// `None` for the engine itself) does not host it. A private function's
    /// calls are what its observers observe, so a call from anyone else, the
    /// engine included, would be one it never made. One atomic load while no
    /// function is observed or hosted privately by a connection; otherwise
    /// one hash lookup more, whatever the function, and the function's set
    /// cloned under its read lock when the view holds the function.
    #[inline]
    pub fn route(
        &self,
        namespace: &str,
        function_id: &str,
        caller: Option<Uuid>,
    ) -> Result<Option<Arc<ActiveSet>>, ErrorBody> {
        let view = self.routed.load();
        if view.is_empty() {
            return Ok(None);
        }
        let Some(routed) = view.get(&(namespace, function_id) as &dyn crate::function::FunctionKey)
        else {
            return Ok(None);
        };
        match routed.callers {
            Callers::Anyone => {}
            Callers::Host(host) if Some(host) == caller => {}
            Callers::Host(_) | Callers::Nobody => return Err(private_refusal(function_id)),
        }
        Ok(routed.active.load_full())
    }

    /// Whether `connection` hosts `(namespace, function_id)` as a private
    /// function: the one connection whose calls of it are its events. One
    /// atomic load while no function is hosted privately or observed;
    /// otherwise one hash lookup more.
    #[inline]
    pub fn hosts_privately(&self, namespace: &str, function_id: &str, connection: Uuid) -> bool {
        let view = self.routed.load();
        !view.is_empty()
            && view
                .get(&(namespace, function_id) as &dyn crate::function::FunctionKey)
                .is_some_and(|routed| routed.callers == Callers::Host(connection))
    }

    /// `FORBIDDEN` when `target` is private: only its host invokes it, so
    /// every activation would be refused.
    pub(crate) fn check_target_invocable(&self, target: &FnKey) -> Result<(), ErrorBody> {
        let private = self
            .function_entry(target)
            .and_then(|entry| entry.options())
            .is_some_and(|options| options.private);
        if private {
            return Err(ErrorBody::new(
                observe_codes::FORBIDDEN,
                format!(
                    "the target `{}` is private: only the worker that hosts it calls it",
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
        if let Some(entry) = self.by_function.get(key) {
            return entry.clone();
        }
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
            let Some(sync) = entry.sync.try_write() else {
                return false;
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

    /// Adds `observer` to its function, last in its creation order. An owner
    /// that already got the function's `functionregistered` hears about it at
    /// once (`observeradded`), and the observer waits for its ack.
    ///
    /// An observer disposed since it was published joins no function: its
    /// disposal found it in none, so nothing would take it out again. Its
    /// disposal marks it disposed before it takes `sync` (to tell the owner)
    /// and the function's members (to leave them), and the check here holds
    /// both: either it sees the observer disposed, or the disposal finds the
    /// observer among the members, and tells the owner if it was announced.
    pub(crate) fn attach_to_function(self: &Arc<Self>, observer: &Arc<Observer>) {
        let joined = self.with_owner_sync(&observer.function, |entry, sync, owner| {
            {
                let mut members = entry.observers.write();
                if observer.is_disposed() {
                    return false;
                }
                // Under `sync`: a function's observers join in the order of
                // their `seq`.
                let seq = self.created.fetch_add(1, Ordering::Relaxed) + 1;
                observer.set_seq(seq);
                members.add(seq, observer);
                observer.set_joined_entry(entry);
            }
            let Some(announced_to) = sync.announced_to else {
                // Not announced (yet): acked by default unless the owner
                // speaks observe, whose `functionregistered` will announce it.
                // Derived here, under `sync`, rather than when the observer
                // was built: an owner may have claimed the function since.
                let acked = !owner.is_some_and(|owner| owner.speaks_observe);
                self.update(observer, |flags| flags.owner_acked = acked);
                return true;
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
            true
        });
        if !joined {
            // The entry may have been made for it alone.
            self.drop_entry_if_unused(&observer.function);
        }
    }

    pub(crate) fn detach_from_function(&self, observer: &Arc<Observer>) {
        let unused = self.entry_of(observer).is_none_or(|entry| {
            entry.observers.write().take(observer);
            entry.is_unused()
        });
        // An entry its other observers still use stays: only the last one's
        // leaving looks it up to drop it.
        if unused {
            self.drop_entry_if_unused(&observer.function);
        }
        self.refresh_observer(observer);
    }

    /// `observer`'s function entry: the one it joined, which stays live while
    /// the observer is one of its members, or else the function's entry now.
    fn entry_of(&self, observer: &Observer) -> Option<Arc<FunctionEntry>> {
        observer
            .joined_entry()
            .filter(|entry| !entry.dead.load(Ordering::Acquire))
            .or_else(|| self.function_entry(&observer.function))
    }

    /// Puts `observer` in its function's active set, or takes it out, as its
    /// current state and its function's current observers say: it is one of
    /// the function's active observers when it is one of its live observers
    /// and `active`. `O(log n)` in the function's active observers. The
    /// function's set is replaced by a new version (copy-on-write), and the
    /// view only when the function joins or leaves it: the hot path reads
    /// the view with one atomic load, then clones the function's set under
    /// its read lock, and a call keeps the set it started with.
    pub(crate) fn refresh_observer(&self, observer: &Arc<Observer>) {
        // Not one of its function's observers yet: in no active set.
        let Some(seq) = observer.seq() else {
            return;
        };
        let _serialized = self.rebuild.write();
        let key = &observer.function;
        if cfg!(debug_assertions) {
            let held = Self::active_in(&self.routed.load(), key).is_some_and(|set| {
                set.observers
                    .get(seq)
                    .is_some_and(|held| Arc::ptr_eq(held, observer))
            });
            debug_assert_eq!(
                held,
                observer.in_active_set(),
                "`{}` is marked as its function's active set holds it",
                observer.id
            );
        }
        let wanted = observer.is_member() && observer.is_active();
        if wanted == observer.in_active_set() {
            return;
        }
        let current = self.routed.load();
        let routed = current.get(routed_key(key));
        let next = match routed.and_then(|routed| routed.active.load_full()) {
            Some(set) if wanted => set.with(observer),
            Some(set) => set.without(observer, seq),
            None => ActiveSet::new(key, vec![observer.clone()]),
        };
        observer.set_in_active_set(wanted);
        self.store_next(&current, key, routed, next);
    }

    /// [`Self::refresh_observer`] for each of `observers`, with one change of
    /// each function's active set for all of its observers among them: a
    /// worker's observers suspended together when it leaves, the observers
    /// of a target that registers. A function whose set changes in more than
    /// a few of its slots is rebuilt from the observers it keeps (`O(n)`),
    /// otherwise changed one observer at a time (`O(log n)` each).
    pub(crate) fn refresh_observers(&self, observers: &[Arc<Observer>]) {
        if let [observer] = observers {
            return self.refresh_observer(observer);
        }
        if observers.is_empty() {
            return;
        }
        let _serialized = self.rebuild.write();
        // What changes, by function, in the order given.
        let mut changes: Vec<(&FnKey, Vec<Change<'_>>)> = Vec::new();
        for observer in observers {
            if observer.seq().is_none() {
                continue;
            }
            let wanted = observer.is_member() && observer.is_active();
            if wanted == observer.in_active_set() {
                continue;
            }
            match changes
                .iter_mut()
                .find(|(key, _)| **key == observer.function)
            {
                Some((_, changing)) => changing.push((observer, wanted)),
                None => changes.push((&observer.function, vec![(observer, wanted)])),
            }
        }
        for (key, changing) in changes {
            let current = self.routed.load();
            let routed = current.get(routed_key(key));
            let set = routed.and_then(|routed| routed.active.load_full());
            let len = set.as_ref().map_or(0, |set| set.observers.len());
            let next = if changing.len() * REBUILD_AT > len {
                let mut leaving: Vec<u64> = changing
                    .iter()
                    .filter(|(_, wanted)| !wanted)
                    .filter_map(|(observer, _)| observer.seq())
                    .collect();
                leaving.sort_unstable();
                let mut kept: Vec<Arc<Observer>> = set
                    .iter()
                    .flat_map(|set| set.observers.iter())
                    .filter(|observer| {
                        observer
                            .seq()
                            .is_none_or(|seq| leaving.binary_search(&seq).is_err())
                    })
                    .cloned()
                    .collect();
                #[cfg(test)]
                super::work::add(kept.len());
                let joining = changing.iter().filter(|(_, wanted)| *wanted);
                kept.extend(joining.map(|(observer, _)| Arc::clone(observer)));
                kept.sort_unstable_by_key(|observer| observer.seq());
                // An observer given twice joins once.
                kept.dedup_by(|a, b| Arc::ptr_eq(a, b));
                ActiveSet::new(key, kept)
            } else {
                let mut next = match &set {
                    Some(set) => set.copy(),
                    None => ActiveSet::new(key, Vec::new()),
                };
                for (observer, wanted) in &changing {
                    let seq = observer.seq().unwrap_or_default();
                    next = if *wanted {
                        next.with(observer)
                    } else {
                        next.without(observer, seq)
                    };
                }
                next
            };
            for (observer, wanted) in &changing {
                observer.set_in_active_set(*wanted);
            }
            self.store_next(&current, key, routed, next);
        }
    }

    /// Stores `next` as `key`'s active set, none when it is empty, where
    /// `routed` is what `current` holds for `key`. Who may call the function
    /// stays as the view says. Called under `rebuild`.
    fn store_next(
        &self,
        current: &RoutedView,
        key: &FnKey,
        routed: Option<&Arc<Routed>>,
        next: ActiveSet,
    ) {
        let active = (!next.observers.is_empty()).then(|| Arc::new(next));
        match routed {
            // It stays in the view: the set changes in place.
            Some(routed) if active.is_some() || matches!(routed.callers, Callers::Host(_)) => {
                self.watching_observers_changed(key, active.is_some());
                routed.active.store(active);
            }
            Some(routed) => self.store_routed(current, key, None, routed.callers),
            None => {
                let callers = callers_in(current, key, self.function_entry(key).as_deref());
                self.store_routed(current, key, active, callers);
            }
        }
    }

    /// Notes whether `engine::on-observers-changed` has active observers,
    /// when `key` is it (see [`ObserverRegistry::observer_changed`]).
    fn watching_observers_changed(&self, key: &FnKey, active: bool) {
        if key.1 == super::events::ON_OBSERVERS_CHANGED
            && key.0 == crate::protocol::DEFAULT_NAMESPACE
        {
            self.observers_changed_watched
                .store(active, Ordering::Release);
        }
    }

    /// Rebuilds the routed hook's view of `key` from all its active
    /// observers, keeping who may call it: `O(n)`, for a change that touches
    /// every observer of the function (a registration). One observer's
    /// change goes through [`Self::refresh_observer`].
    pub(crate) fn refresh_active(&self, key: &FnKey) {
        let _serialized = self.rebuild.write();
        let entry = self.function_entry(key);
        let members = entry
            .as_ref()
            .map(|entry| entry.observers())
            .unwrap_or_default();
        let current = self.routed.load();
        let held = Self::active_in(&current, key);
        if members.is_empty() && held.is_none() {
            return;
        }
        // Whatever the set held goes, and the members that are active come
        // in: an observer is marked as it is in the stored set.
        for observer in held.iter().flat_map(|set| set.observers.iter()) {
            observer.set_in_active_set(false);
        }
        let active: Vec<Arc<Observer>> = members
            .into_iter()
            .filter(|o| o.is_member() && o.is_active())
            .collect();
        for observer in &active {
            observer.set_in_active_set(true);
        }
        let callers = callers_in(&current, key, entry.as_deref());
        let active = (!active.is_empty()).then(|| Arc::new(ActiveSet::new(key, active)));
        self.store_routed(&current, key, active, callers);
    }

    /// A function was registered or removed, in any namespace and by any path
    /// (worker, HTTP-invoked, in-process). Synchronous and non-blocking: it
    /// only flips flags and queues notifications.
    ///
    /// A registration starts a new owner relationship: until the owner (if it
    /// speaks observe) gets `functionregistered` and acks, its observers stay
    /// `pending`. The engine's standing observers of the function are made in
    /// its namespace.
    pub fn on_function_change(&self, change: FunctionChange, namespace: &str, function_id: &str) {
        let key = (namespace.to_string(), function_id.to_string());
        let registered = change == FunctionChange::Registered;

        if let Some(entry) = self.function_entry(&key) {
            if !registered {
                // The options belong to the registration that just ended.
                *entry.options.write() = None;
                self.set_callers(&key, false);
            }
            // Only a registration reads `function_owners`: a release calls
            // this while holding the function's shard.
            let acked = registered && self.acked_by_default(&key);
            {
                let mut sync = entry.sync.write();
                sync.announced_to = None;
                entry.announced.store(false, Ordering::Release);
            }
            let options = entry.options();
            let mut disposed = Vec::new();
            for observer in entry.observers() {
                observer.cancel_ack_timer();
                if registered
                    && let Err(error) = self.check_on_registration(&observer, options.as_ref())
                {
                    self.fail(&observer, error);
                    continue;
                }
                // The function's set is rebuilt once for all of them below.
                if let Some(transition) = self.transition(&observer, |flags| {
                    flags.function_available = registered;
                    flags.owner_acked = acked;
                    flags.announced = None;
                    flags.rejected_by_owner = false;
                }) && self
                    .settle_deferring_refresh(&observer, transition)
                    .disposed
                {
                    disposed.push(observer);
                }
            }
            // Activeness may have changed with the new registration.
            self.refresh_active(&key);
            for observer in &disposed {
                self.forget(observer);
            }
            self.drop_entry_if_unused(&key);
        }

        let targeting: Vec<Arc<Observer>> = self
            .by_target
            .get(&key)
            .map(|held| held.iter().map(|held| held.0.clone()).collect())
            .unwrap_or_default();
        let mut changed = Vec::new();
        let mut disposed = Vec::new();
        for observer in &targeting {
            // A different worker may now host the target: it gets the
            // observed input, output and caller only if the observing
            // worker may invoke it, and only if anyone but its host may
            // (a private target refuses every activation).
            if registered
                && let Err(error) = self
                    .check_session_allows(observer, &key)
                    .and_then(|()| self.check_target_invocable(&key))
            {
                self.fail(observer, error);
                continue;
            }
            if let Some(transition) =
                self.transition(observer, |flags| flags.target_available = registered)
            {
                let settled = self.settle_deferring_refresh(observer, transition);
                if settled.refresh {
                    changed.push(observer.clone());
                }
                if settled.disposed {
                    disposed.push(observer);
                }
            }
        }
        // Their functions' sets change once each.
        self.refresh_observers(&changed);
        for observer in disposed {
            self.forget(observer);
        }
        if registered {
            self.standing_on_registration(namespace, function_id);
        }
    }

    /// Who owns `key` now, from the engine's `function_owners` map. `None`
    /// for an engine-hosted function or one nobody registered.
    pub(crate) fn owner_of(&self, key: &FnKey) -> Option<Owner> {
        let deps = self.deps.get()?;
        let (connection, kind) = *deps.function_owners.get(key)?;
        Some(self.owner(connection, kind))
    }

    /// Whether `key`'s host delivers its calls itself (v2 §8.2): a connection
    /// that declared `observe.v2` and hosts the function, not an HTTP-invoked
    /// registration (decision 43). The engine evaluates the calls of every
    /// other function.
    pub(crate) fn host_delivers(&self, key: &FnKey) -> bool {
        let Some(deps) = self.deps.get() else {
            return false;
        };
        let Some((connection, kind)) = deps.function_owners.get(key).map(|owner| *owner) else {
            return false;
        };
        kind == InvocationKind::Regular
            && deps
                .workers
                .workers
                .get(&connection)
                .is_some_and(|worker| worker.features.observe_v2())
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
    /// SDK may run the function in-process: the global switch, a connection
    /// allowed to invoke the function itself, and a session without RBAC
    /// middleware (a local call would bypass it) or a function it hosts
    /// privately, whose calls from its host skip the middleware anyway.
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
                (session.config.middleware_function_id.is_none()
                    || self.hosts_privately(namespace, function_id, worker.id))
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
            // An observer whose observing worker is away (suspended, waiting
            // for a `Reattach`) is announced when it returns.
            let observers: Vec<Arc<Observer>> = entry
                .observers()
                .into_iter()
                .filter(|o| {
                    o.with_flags(|flags| {
                        flags.failed.is_none()
                            && flags.disposed.is_none()
                            && flags.observing_connected
                    })
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
            Some((revision, observers.len()))
        });
        let Some((revision, observers)) = announced else {
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
                        flags.failed = Some(Arc::new(error));
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
        self.owner_moved_on(&observer.function, connection, tokio::time::Instant::now());
    }

    /// The observer left the owner's set (error, disposal): the owner hears
    /// `observerremoved` if it was told about it and did not reject it.
    pub(crate) fn withdraw_from_owner(&self, observer: &Arc<Observer>) {
        observer.cancel_ack_timer();
        let Some(entry) = self.entry_of(observer) else {
            return;
        };
        let mut sync = entry.sync.write();
        self.tell_owner_removed(&entry, &mut sync, observer);
    }

    /// The observing worker of `observers` left (they are suspended for
    /// `observers.observer_grace_ms`): each function's owner lets them go
    /// (`observerremoved`, for each it was told about) and holds only
    /// observers that can become active. Each waits for a new ack, and its
    /// owner hears about it again when the worker reattaches
    /// ([`Self::announce_returned`]). An observer that did not come back is
    /// disposed with nothing more sent to the owner. Each function's owner
    /// sync is taken once for all of its observers among them.
    pub(crate) fn withdraw_all_while_away(self: &Arc<Self>, observers: &[Arc<Observer>]) {
        let mut by_function: Vec<(&FnKey, Vec<&Arc<Observer>>)> = Vec::new();
        for observer in observers {
            match by_function
                .iter_mut()
                .find(|(key, _)| **key == observer.function)
            {
                Some((_, withdrawn)) => withdrawn.push(observer),
                None => by_function.push((&observer.function, vec![observer])),
            }
        }
        for (key, withdrawn) in by_function {
            if self.function_entry(key).is_none() {
                continue;
            }
            for observer in &withdrawn {
                observer.cancel_ack_timer();
            }
            self.with_owner_sync(key, |entry, sync, owner| {
                let acked = !owner.is_some_and(|owner| owner.speaks_observe);
                for observer in withdrawn {
                    let away = observer.with_flags(|flags| {
                        !flags.observing_connected
                            && flags.failed.is_none()
                            && flags.disposed.is_none()
                    });
                    if !away {
                        continue;
                    }
                    self.tell_owner_removed(entry, sync, observer);
                    self.update(observer, |flags| {
                        if !flags.observing_connected {
                            flags.owner_acked = acked;
                            flags.announced = None;
                        }
                    });
                }
            });
        }
    }

    /// The observing worker of `observers` reattached: each function's owner
    /// that got its `functionregistered` hears about them again
    /// (`observeradded`), in the order they were created, as that
    /// `functionregistered` would have listed them, and each waits for its
    /// ack. `O(k log k)` for `k` returning observers, however many others
    /// their functions have.
    pub(crate) fn announce_returned(self: &Arc<Self>, observers: &[Arc<Observer>]) {
        let mut returned: Vec<(u64, &Arc<Observer>)> = observers
            .iter()
            .filter_map(|observer| Some((observer.seq()?, observer)))
            .collect();
        returned.sort_unstable_by_key(|(seq, _)| *seq);
        for (_, observer) in returned {
            if self
                .function_entry(&observer.function)
                .is_some_and(|entry| entry.holds(observer))
            {
                self.announce_on_return(observer);
            }
        }
    }

    fn announce_on_return(self: &Arc<Self>, observer: &Arc<Observer>) {
        self.with_owner_sync(&observer.function, |_entry, sync, _owner| {
            let returning = observer.with_flags(|flags| {
                flags.observing_connected
                    && flags.failed.is_none()
                    && flags.disposed.is_none()
                    && flags.announced.is_none()
            });
            if !returning {
                return;
            }
            // No `functionregistered` yet: it will list the observer.
            let Some(announced_to) = sync.announced_to else {
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

    /// Sends the owner `observerremoved` for `observer` if it was told about
    /// it and did not reject it. The caller holds `entry`'s `sync`.
    fn tell_owner_removed(&self, entry: &FunctionEntry, sync: &mut OwnerSync, observer: &Observer) {
        let (announced, rejected, owner_acked, timed_out) = observer.take_announcement();
        let (Some(announced), Some(owner)) = (announced, sync.announced_to) else {
            return;
        };
        if rejected || announced.owner != owner {
            return;
        }
        if !owner_acked && !timed_out {
            // Its hooks may be running: the owner moves on to the next
            // observer once they end, a full timeout from now at the latest.
            let at = tokio::time::Instant::now() + self.ack_timeout();
            Self::record_progress(entry, owner, at);
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

    fn ack_timeout(&self) -> Duration {
        Duration::from_millis(self.config().owner_ack_timeout_ms)
    }

    /// `OWNER_TIMEOUT` unless the owner acks the announcement `generation`
    /// within `observers.owner_ack_timeout_ms` of moving on to it (see
    /// [`Self::ack_timed_out`]). Checked under the status lock: a timer that
    /// fires while the observer is announced again does nothing.
    fn arm_ack_timer(self: &Arc<Self>, observer: &Arc<Observer>, generation: u64) {
        let announced_at = tokio::time::Instant::now();
        self.arm_ack_timer_until(
            observer,
            generation,
            announced_at,
            announced_at + self.ack_timeout(),
        );
    }

    fn arm_ack_timer_until(
        self: &Arc<Self>,
        observer: &Arc<Observer>,
        generation: u64,
        announced_at: tokio::time::Instant,
        deadline: tokio::time::Instant,
    ) {
        let Some(runtime) = self.runtime() else {
            return;
        };
        let registry = Arc::downgrade(self);
        let weak = Arc::downgrade(observer);
        let handle = runtime.spawn(async move {
            tokio::time::sleep_until(deadline).await;
            if let (Some(registry), Some(observer)) = (registry.upgrade(), weak.upgrade()) {
                registry.ack_timed_out(&observer, generation, announced_at);
            }
        });
        observer.set_ack_timer(handle.abort_handle());
    }

    /// The ack timer of announcement `generation` fired. The SDK runs one
    /// function's observer hooks at a time, in the order the engine announced
    /// the observers (v2 §4.3), so the timeout counts from when the owner moved
    /// on to this observer: its announcement, or the owner's latest answer for
    /// the function (or ack timeout) after it. Before that deadline, or while
    /// an observer announced to the same owner before this one still waits,
    /// the timer is armed again. Past it, `OWNER_TIMEOUT`, unless the observer
    /// was acked, failed, disposed or announced again meanwhile (checked under
    /// its status lock, as a re-announcement is).
    fn ack_timed_out(
        self: &Arc<Self>,
        observer: &Arc<Observer>,
        generation: u64,
        announced_at: tokio::time::Instant,
    ) {
        let Some(announced) = observer
            .awaited_announcement()
            .filter(|announced| announced.generation == generation)
        else {
            return;
        };
        let now = tokio::time::Instant::now();
        let timeout = self.ack_timeout();
        let entry = self.function_entry(&observer.function);
        let started = entry
            .as_deref()
            .and_then(|entry| *entry.owner_progress.write())
            .filter(|(owner, _)| *owner == announced.owner)
            .map_or(announced_at, |(_, at)| at.max(announced_at));
        let deadline = if started + timeout > now {
            Some(started + timeout)
        } else if entry
            .as_deref()
            .is_some_and(|entry| waits_behind(entry, observer, announced))
        {
            // Its hooks have not started: the owner is on an earlier one.
            Some(now + timeout)
        } else {
            None
        };
        let waiting = |flags: &Flags| {
            flags
                .announced
                .is_some_and(|announced| announced.generation == generation)
                && !flags.owner_acked
                && flags.failed.is_none()
                && flags.disposed.is_none()
        };
        if let Some(deadline) = deadline {
            self.update(observer, |flags| {
                if waiting(flags) {
                    self.arm_ack_timer_until(observer, generation, announced_at, deadline);
                }
            });
            return;
        }
        let mut failed = false;
        self.update(observer, |flags| {
            if waiting(flags) {
                flags.failed = Some(Arc::new(ErrorBody::new(
                    observe_codes::OWNER_TIMEOUT,
                    "the function's owner did not ack the observer within \
                     observers.owner_ack_timeout_ms",
                )));
                failed = true;
            }
        });
        if failed && let Some(entry) = entry {
            // The SDK gives up on the hook at the same deadline and moves on.
            Self::record_progress(&entry, announced.owner, now);
        }
    }

    /// The owner `owner` answered an observer of `function` at `at`.
    fn owner_moved_on(&self, function: &FnKey, owner: Uuid, at: tokio::time::Instant) {
        if let Some(entry) = self.function_entry(function) {
            Self::record_progress(&entry, owner, at);
        }
    }

    fn record_progress(entry: &FunctionEntry, owner: Uuid, at: tokio::time::Instant) {
        let mut progress = entry.owner_progress.write();
        *progress = Some(match *progress {
            Some((current, latest)) if current == owner => (owner, latest.max(at)),
            _ => (owner, at),
        });
    }

    /// What `observe` could not check because the function was not registered
    /// yet, and what a re-registration may have changed: the options
    /// (`observable`, args) and the filter paths against the formats.
    ///
    /// Every registration is checked afresh: the new registration may come
    /// from another worker, with other formats, under RBAC that sees it
    /// differently (an `expose_functions` rule on its metadata).
    fn check_on_registration(
        &self,
        observer: &Arc<Observer>,
        options: Option<&FunctionOptions>,
    ) -> Result<(), ErrorBody> {
        if observer.with_flags(|flags| flags.failed.is_some()) {
            return Ok(());
        }
        self.check_session_allows(observer, &observer.function)?;
        check_options(options, &observer.function, &observer.args)?;
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

/// Whether an observer of `entry`'s function announced to the same owner
/// before `observer` (an earlier generation) still waits for its answer: the
/// owner has not started on `observer`'s hooks yet. Read only when an ack
/// timer passed its deadline.
fn waits_behind(entry: &FunctionEntry, observer: &Observer, announced: Announcement) -> bool {
    entry.observers().iter().any(|other| {
        other.id != observer.id
            && other.awaited_announcement().is_some_and(|earlier| {
                earlier.owner == announced.owner && earlier.generation < announced.generation
            })
    })
}

/// An observer as its function's owner sees it (CONTRACT D15). `capture`
/// also names the part `deliver` gives the target, so the host's report of
/// a local call carries it.
pub(crate) fn view(observer: &Observer) -> ObserverView {
    ObserverView {
        id: observer.id.clone(),
        args: observer.args.clone(),
        filters: observer.filter.normalized().clone(),
        target: observer.target.1.clone(),
        capture: observer.capture.with_delivered(&observer.deliver).to_list(),
        on: observer.on.clone(),
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
    fn a_function_without_an_args_format_takes_no_args() {
        assert!(check_options(None, &key(), &json!({})).is_ok());
        assert!(check_options(None, &key(), &Value::Null).is_ok());
        let options = FunctionOptions::private();
        assert!(check_options(Some(&options), &key(), &json!({})).is_ok());
        let err = check_options(None, &key(), &json!({ "expression": "* * * * * *" })).unwrap_err();
        assert_eq!(err.code, observe_codes::INVALID_ARGS);
    }

    #[test]
    fn args_are_validated_against_the_args_format_including_required_ones() {
        let options = FunctionOptions {
            args_format: Some(json!({
                "type": "object", "required": ["expression"],
                "properties": { "expression": { "type": "string" } }
            })),
            ..FunctionOptions::default()
        };
        assert!(check_options(Some(&options), &key(), &json!({ "expression": "x" })).is_ok());
        let missing = check_options(Some(&options), &key(), &json!({})).unwrap_err();
        assert_eq!(missing.code, observe_codes::INVALID_ARGS);
        let wrong = check_options(Some(&options), &key(), &json!({ "expression": 5 })).unwrap_err();
        assert_eq!(wrong.code, observe_codes::INVALID_ARGS);
    }

    #[test]
    fn a_function_that_is_not_observable_refuses_observers() {
        let options = FunctionOptions {
            observable: false,
            ..FunctionOptions::default()
        };
        let err = check_options(Some(&options), &key(), &json!({})).unwrap_err();
        assert_eq!(err.code, observe_codes::NOT_OBSERVABLE);
    }

    #[test]
    fn an_args_format_must_compile() {
        assert!(validate_args_format(&json!({ "type": "object" })).is_ok());
        assert!(validate_args_format(&json!({ "type": "not-a-type" })).is_err());
    }

    /// Only the host calls a private function; the engine itself and every
    /// other connection get `FORBIDDEN`, and it is no target. Its options
    /// go with the registration that set them.
    #[test]
    fn a_private_function_is_called_by_its_host_only() {
        let registry = ObserverRegistry::new();
        let owners = Arc::new(dashmap::DashMap::new());
        let host = Uuid::new_v4();
        owners.insert(key(), (host, InvocationKind::Regular));
        let invocations = Arc::new(crate::invocation::InvocationHandler::new());
        let registry = Arc::new(registry);
        registry.attach(
            Arc::new(crate::function::FunctionsRegistry::new()),
            Arc::new(crate::worker_connections::WorkerConnectionRegistry::new()),
            Arc::downgrade(&invocations),
            owners,
        );
        let (namespace, function_id) = key();
        assert!(registry.route(&namespace, &function_id, None).is_ok());

        registry.set_options(&namespace, &function_id, FunctionOptions::private());
        assert!(registry.options(&namespace, &function_id).private);
        assert!(matches!(
            registry.route(&namespace, &function_id, Some(host)),
            Ok(None)
        ));
        for caller in [None, Some(Uuid::new_v4())] {
            let refused = registry
                .route(&namespace, &function_id, caller)
                .err()
                .expect("refused");
            assert_eq!(refused.code, observe_codes::FORBIDDEN);
        }
        assert_eq!(
            registry.check_target_invocable(&key()).unwrap_err().code,
            observe_codes::FORBIDDEN
        );

        registry.on_function_change(FunctionChange::Unregistered, &namespace, &function_id);
        assert!(registry.route(&namespace, &function_id, None).is_ok());
        assert!(registry.routed.load().is_empty(), "out of the routed view");
        assert_eq!(
            registry.options(&namespace, &function_id),
            FunctionOptions::default()
        );
    }

    /// A private function no connection hosts (an engine-hosted source)
    /// stays out of the routed hook's view while unobserved: with every
    /// source an engine boots registered, a call of any other function
    /// costs one atomic load. Observed, the hook refuses every call of it.
    #[tokio::test]
    async fn an_unobserved_engine_hosted_source_costs_other_calls_nothing() {
        use super::super::events::{
            ON_WORKERS_CHANGED, register_configuration_event_source, register_engine_event_sources,
            register_observability_event_sources, register_stream_event_sources, test_support,
        };
        use crate::protocol::DEFAULT_NAMESPACE;

        let engine = Arc::new(Engine::new());
        register_engine_event_sources(&engine);
        register_configuration_event_source(&engine);
        register_observability_event_sources(&engine);
        register_stream_event_sources(&engine);
        assert!(engine.observers.routed.load().is_empty());
        assert!(matches!(
            engine
                .observers
                .route(DEFAULT_NAMESPACE, "orders::create", None),
            Ok(None)
        ));

        let _sink = test_support::capture(&engine, "test::sink");
        let _socket =
            test_support::observe(&engine, "obs", ON_WORKERS_CHANGED, "test::sink", None).await;
        for caller in [None, Some(Uuid::new_v4())] {
            let refused = engine
                .observers
                .route(DEFAULT_NAMESPACE, ON_WORKERS_CHANGED, caller)
                .err()
                .expect("refused");
            assert_eq!(refused.code, observe_codes::FORBIDDEN);
        }
    }

    /// What a call keeps: a call the engine evaluates needs what the
    /// observers' filters read, capture or deliver; a call whose host lists
    /// the observers needs only what they capture or deliver. `capture: []`
    /// with the envelope needs nothing (A6).
    #[tokio::test]
    async fn a_call_keeps_what_its_observers_need_where_it_is_delivered() {
        use super::super::events::test_support;
        use crate::protocol::{Capture, DEFAULT_NAMESPACE, Deliver};
        let engine = Arc::new(crate::engine::Engine::new());
        let _source = test_support::capture(&engine, "test::src");
        let _target = test_support::capture(&engine, "test::dst");
        let (worker, _socket) = test_support::worker(&engine);
        let observe = |id: &str, filters: Option<Value>, capture: Vec<Capture>, deliver| {
            let mut request = test_support::request(id, "test::src", "test::dst", filters);
            request.source.capture = Some(capture);
            request.source.deliver = Some(deliver);
            request
        };
        let needs = |engine: &crate::engine::Engine| {
            let set = engine
                .observers
                .watch(DEFAULT_NAMESPACE, "test::src")
                .unwrap();
            (set.needs_for(false), set.needs_for(true))
        };

        test_support::observe_request(
            &engine,
            &worker,
            observe("count", None, vec![], Deliver::Envelope),
        )
        .await;
        assert_eq!(needs(&engine), (Needs::default(), Needs::default()));

        test_support::observe_request(
            &engine,
            &worker,
            observe(
                "filtered",
                Some(json!({ "input.n": { "$eq": 1 }, "caller.worker_name": { "$ne": "x" } })),
                vec![],
                Deliver::Envelope,
            ),
        )
        .await;
        let read = Needs {
            input: true,
            caller: true,
            ..Needs::default()
        };
        assert_eq!(needs(&engine), (read, Needs::default()));

        test_support::observe_request(
            &engine,
            &worker,
            observe("outputs", None, vec![Capture::Error], Deliver::Output),
        )
        .await;
        let delivered = Needs {
            output: true,
            error: true,
            ..Needs::default()
        };
        assert_eq!(needs(&engine), (read.union(delivered), delivered));
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

        // Announced longer ago than the timeout: due.
        let announced_at = tokio::time::Instant::now()
            .checked_sub(registry.ack_timeout() * 2)
            .expect("an instant two timeouts ago");
        registry.ack_timed_out(&observer, first, announced_at);
        assert_eq!(
            observer.state(),
            crate::protocol::ObserverState::Pending,
            "stale timer"
        );
        registry.ack_timed_out(&observer, second, announced_at);
        assert_eq!(observer.state(), crate::protocol::ObserverState::Error);
        assert_eq!(
            observer.reason().unwrap().code,
            observe_codes::OWNER_TIMEOUT
        );
    }

    /// An engine-owned observer `id` of `test::src` with `target` as its
    /// target.
    fn engine_observer(id: &str, target: &str) -> crate::observer::EngineObserver {
        crate::observer::EngineObserver {
            id: id.to_string(),
            function: (
                crate::protocol::DEFAULT_NAMESPACE.to_string(),
                "test::src".to_string(),
            ),
            args: json!({}),
            filters: None,
            target: (
                crate::protocol::DEFAULT_NAMESPACE.to_string(),
                target.to_string(),
            ),
            deliver: crate::protocol::Deliver::Input,
            metadata: None,
        }
    }

    /// The ids of `test::src`'s active observers, in the order a call
    /// evaluates them.
    fn active_ids(set: &ActiveSet) -> Vec<String> {
        set.observers.iter().map(|o| o.id.clone()).collect()
    }

    /// A disposed observer goes with the last version of its function's
    /// active set that holds it, oldest observer included (it leaves its
    /// leaf without a copy), and the drops it counted reach
    /// `iii.observer.dropped.total` then: at once when no call holds an
    /// older version, when the last call that does lets go otherwise.
    #[tokio::test]
    async fn a_disposed_oldest_observer_goes_with_the_last_set_that_holds_it() {
        use super::super::{events::test_support, test_metrics};
        let recorded = test_metrics::record();
        let engine = Arc::new(Engine::new());
        let _source = test_support::capture(&engine, "test::src");
        let _target = test_support::capture(&engine, "test::dst");
        let registry = &engine.observers;
        for id in ["a", "b", "c", "d"] {
            registry
                .observe_as_engine(engine_observer(id, "test::dst"))
                .unwrap();
        }
        let ns = crate::protocol::DEFAULT_NAMESPACE;
        let dropped = || {
            recorded.sum(
                "iii.observer.dropped.total",
                &[("function_id", "test::src")],
            )
        };
        let count_drops = |id: &str, count: u64| {
            let observer = registry.get(id).unwrap();
            observer
                .drops
                .overflow
                .fetch_add(count, std::sync::atomic::Ordering::Relaxed);
            Arc::downgrade(&observer)
        };

        // No call holds an older version: `a` goes with its disposal.
        let a = count_drops("a", 5);
        assert!(registry.unobserve_as_engine("a"));
        assert_eq!(
            active_ids(&registry.watch(ns, "test::src").unwrap()),
            ["b", "c", "d"]
        );
        assert_eq!(a.strong_count(), 0, "`a` outlived its disposal");
        assert_eq!(dropped(), 5, "the drops `a` counted reach the metric");

        // A call holds the version with `b`: `b` goes when it lets go.
        let b = count_drops("b", 2);
        let call = registry.watch(ns, "test::src").unwrap();
        assert!(registry.unobserve_as_engine("b"));
        assert_eq!(
            active_ids(&registry.watch(ns, "test::src").unwrap()),
            ["c", "d"]
        );
        assert_eq!(
            active_ids(&call),
            ["b", "c", "d"],
            "the call keeps its version"
        );
        assert!(b.upgrade().is_some(), "the call's version still holds `b`");
        drop(call);
        assert_eq!(
            b.strong_count(),
            0,
            "`b` outlived the last version holding it"
        );
        assert_eq!(dropped(), 7, "the drops `b` counted reach the metric");
    }

    /// Adding an observer to a function and removing it cost `O(log n)` in
    /// the function's `n` active observers: a change copies at most the
    /// nodes on one path of the active set (appending the newest observer
    /// and removing the oldest copy no leaf, only the branches above it), a
    /// few hundred slots at most, not the set. A change that rebuilt the set
    /// would copy every observer, 8,000 at 8,000.
    #[tokio::test]
    async fn adding_and_removing_an_observer_costs_log_n() {
        use super::super::{events::test_support, work};
        let per_change = |n: usize| {
            let engine = Arc::new(Engine::new());
            let _source = test_support::capture(&engine, "test::src");
            let _target = test_support::capture(&engine, "test::dst");
            work::take();
            for i in 0..n {
                engine
                    .observers
                    .observe_as_engine(engine_observer(&format!("churn-{i}"), "test::dst"))
                    .expect("observe");
            }
            assert_eq!(
                engine
                    .observers
                    .watch(crate::protocol::DEFAULT_NAMESPACE, "test::src")
                    .map(|set| set.observers.iter().count()),
                Some(n)
            );
            for i in 0..n {
                assert!(engine.observers.unobserve_as_engine(&format!("churn-{i}")));
            }
            assert!(
                engine
                    .observers
                    .watch(crate::protocol::DEFAULT_NAMESPACE, "test::src")
                    .is_none()
            );
            work::take() as f64 / (2 * n) as f64
        };
        let small = per_change(500);
        let large = per_change(8_000);
        assert!(
            small < 256.0 && large < 256.0,
            "slots copied per change: {small:.1} with 500 observers, {large:.1} with 8,000"
        );
    }

    /// A call keeps the active set it started with while observers come and
    /// go; the next call gets the set as it is then, in creation order.
    #[tokio::test]
    async fn a_call_keeps_the_set_it_started_with() {
        use super::super::events::test_support;
        let engine = Arc::new(Engine::new());
        let _source = test_support::capture(&engine, "test::src");
        let _target = test_support::capture(&engine, "test::dst");
        let registry = &engine.observers;
        let watch = || {
            registry
                .watch(crate::protocol::DEFAULT_NAMESPACE, "test::src")
                .expect("observed")
        };
        for id in ["a", "b", "c"] {
            registry
                .observe_as_engine(engine_observer(id, "test::dst"))
                .unwrap();
        }
        let started = watch();

        assert!(registry.unobserve_as_engine("b"));
        registry
            .observe_as_engine(engine_observer("d", "test::dst"))
            .unwrap();
        // Enough more that the set outgrows one node.
        for n in 0..200 {
            registry
                .observe_as_engine(engine_observer(&format!("x{n:03}"), "test::dst"))
                .unwrap();
        }
        let later = watch();

        assert_eq!(active_ids(&started), ["a", "b", "c"]);
        let expected: Vec<String> = ["a", "c", "d"]
            .into_iter()
            .map(String::from)
            .chain((0..200).map(|n| format!("x{n:03}")))
            .collect();
        assert_eq!(active_ids(&later), expected);
        assert_eq!(later.observers.iter().count(), 203);
    }

    /// Observers that become active out of their creation order are still
    /// evaluated in creation order: one created first, whose target
    /// registers later, comes before one that was active from the start.
    #[tokio::test]
    async fn observers_are_evaluated_in_creation_order_whenever_they_became_active() {
        use super::super::events::test_support;
        let engine = Arc::new(Engine::new());
        let _source = test_support::capture(&engine, "test::src");
        let _target = test_support::capture(&engine, "test::dst");
        let registry = &engine.observers;
        registry
            .observe_as_engine(engine_observer("first", "test::late"))
            .unwrap();
        registry
            .observe_as_engine(engine_observer("second", "test::dst"))
            .unwrap();
        let ids = || {
            active_ids(
                &registry
                    .watch(crate::protocol::DEFAULT_NAMESPACE, "test::src")
                    .expect("observed"),
            )
        };
        assert_eq!(ids(), ["second"], "`first` waits for its target");

        let _late = test_support::capture(&engine, "test::late");
        assert_eq!(ids(), ["first", "second"]);
        assert!(registry.unobserve_as_engine("second"));
        assert_eq!(ids(), ["first"]);
    }

    /// One change of a function's set takes many observers out and puts them
    /// back, in creation order, whichever order they come in, and once each
    /// when one comes twice.
    #[tokio::test]
    async fn a_batch_changes_a_set_once_for_each_observer() {
        use super::super::events::test_support;
        let engine = Arc::new(Engine::new());
        let _source = test_support::capture(&engine, "test::src");
        let _target = test_support::capture(&engine, "test::dst");
        let registry = &engine.observers;
        for id in ["a", "b", "c"] {
            registry
                .observe_as_engine(engine_observer(id, "test::dst"))
                .unwrap();
        }
        let ids = || {
            registry
                .watch(crate::protocol::DEFAULT_NAMESPACE, "test::src")
                .map(|set| active_ids(&set))
                .unwrap_or_default()
        };
        let (a, b) = (registry.get("a").unwrap(), registry.get("b").unwrap());
        let connected = |connected: bool| {
            for observer in [&a, &b] {
                let transition = registry
                    .transition(observer, |flags| flags.observing_connected = connected)
                    .expect("a change");
                let settled = registry.settle_deferring_refresh(observer, transition);
                assert!(settled.refresh && !settled.disposed);
            }
        };
        connected(false);
        registry.refresh_observers(&[b.clone(), a.clone(), b.clone()]);
        assert_eq!(ids(), ["c"]);
        connected(true);
        registry.refresh_observers(&[b.clone(), a.clone(), a.clone()]);
        assert_eq!(ids(), ["a", "b", "c"]);
    }

    /// What a call keeps follows the observers that need it as they leave:
    /// once the last observer whose filter reads the input is gone, calls
    /// stop copying it.
    #[tokio::test]
    async fn what_a_call_keeps_goes_with_the_observers_that_need_it() {
        use super::super::events::test_support;
        use crate::protocol::{Capture, DEFAULT_NAMESPACE, Deliver};
        let engine = Arc::new(crate::engine::Engine::new());
        let _source = test_support::capture(&engine, "test::src");
        let _target = test_support::capture(&engine, "test::dst");
        let (worker, _socket) = test_support::worker(&engine);
        let observe = |id: &str, filters: Option<Value>| {
            let mut request = test_support::request(id, "test::src", "test::dst", filters);
            request.source.capture = Some(vec![]);
            request.source.deliver = Some(Deliver::Envelope);
            request
        };
        let needs = || {
            engine
                .observers
                .watch(DEFAULT_NAMESPACE, "test::src")
                .map(|set| (set.needs_for(false), set.needs_for(true)))
        };
        for (id, filters) in [
            ("reads-input", Some(json!({ "input.n": { "$eq": 1 } }))),
            ("reads-input-too", Some(json!({ "input.n": { "$eq": 2 } }))),
            ("plain", None),
        ] {
            test_support::observe_request(&engine, &worker, observe(id, filters)).await;
        }
        let mut errors = test_support::request("errors", "test::src", "test::dst", None);
        errors.source.capture = Some(vec![Capture::Error]);
        errors.source.deliver = Some(Deliver::Envelope);
        test_support::observe_request(&engine, &worker, errors).await;
        let reads = Needs {
            input: true,
            error: true,
            ..Needs::default()
        };
        let delivered = Needs {
            error: true,
            ..Needs::default()
        };
        assert_eq!(needs(), Some((reads, delivered)));

        engine.observers.unobserve(worker.id, "reads-input");
        assert_eq!(
            needs(),
            Some((reads, delivered)),
            "one still reads the input"
        );
        engine.observers.unobserve(worker.id, "reads-input-too");
        assert_eq!(needs(), Some((delivered, delivered)));
        engine.observers.unobserve(worker.id, "errors");
        assert_eq!(needs(), Some((Needs::default(), Needs::default())));
        engine.observers.unobserve(worker.id, "plain");
        assert_eq!(needs(), None);
    }

    /// An observer disposed between its publish (in `by_id`) and its joining
    /// its function joins no function: its disposal found it in none, so it
    /// would stay among the function's live observers for good, counting
    /// against `max_per_function` and keeping the function's entry. Four
    /// threads observe and unobserve four ids of one function at random, so
    /// some unobserves land in that window.
    #[test]
    fn an_observer_disposed_before_it_joins_its_function_joins_none() {
        use super::super::events::test_support;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let _entered = runtime.enter();
        let engine = Arc::new(Engine::new());
        let _source = test_support::capture(&engine, "test::src");
        let _target = test_support::capture(&engine, "test::dst");
        let threads: Vec<_> = (0..4u64)
            .map(|thread| {
                let engine = engine.clone();
                let handle = runtime.handle().clone();
                std::thread::spawn(move || {
                    let _entered = handle.enter();
                    // xorshift: which id, and whether to observe or unobserve.
                    let mut rng = 0x9e37_79b9_7f4a_7c15_u64 ^ (thread + 1);
                    for _ in 0..25_000 {
                        rng ^= rng << 13;
                        rng ^= rng >> 7;
                        rng ^= rng << 17;
                        let id = format!("race-{}", rng % 4);
                        if (rng >> 32).is_multiple_of(2) {
                            let _ = engine
                                .observers
                                .observe_as_engine(engine_observer(&id, "test::dst"));
                        } else {
                            engine.observers.unobserve_as_engine(&id);
                        }
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let registry = &engine.observers;
        let key: FnKey = (
            crate::protocol::DEFAULT_NAMESPACE.to_string(),
            "test::src".to_string(),
        );
        let members = registry
            .function_entry(&key)
            .map(|entry| entry.observers())
            .unwrap_or_default();
        let left_behind: Vec<String> = members
            .iter()
            .filter(|member| {
                !registry
                    .by_id
                    .get(&member.id)
                    .is_some_and(|held| Arc::ptr_eq(&held, member))
            })
            .map(|member| format!("{} {}", member.id, member.state()))
            .collect();
        assert!(
            left_behind.is_empty(),
            "disposed observers among the live ones: {left_behind:?}"
        );
        assert_eq!(
            registry.observer_count(crate::protocol::DEFAULT_NAMESPACE, "test::src"),
            registry.len()
        );
        for id in 0..4 {
            registry.unobserve_as_engine(&format!("race-{id}"));
        }
        assert!(registry.is_empty());
        assert!(
            registry.function_entry(&key).is_none(),
            "the function's entry goes with its last observer"
        );
        assert!(
            registry
                .watch(crate::protocol::DEFAULT_NAMESPACE, "test::src")
                .is_none()
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
            let _held = entry.sync.write();
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
