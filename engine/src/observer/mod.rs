// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Observers: the engine side of `iii.observe(source, target)`
//! (`tech-specs/2026-10-13-observe/protocol.md` §3).
//!
//! - `registry`: `observe` / `unobserve`, validation, replay, the observing
//!   connection's disconnect, grace and `Reattach`.
//! - `owner`: the observed functions, kept in step with the function registry
//!   (descriptor, availability, the routed hook's active sets).
//! - `lifecycle`: observer states, derived from facts, and
//!   `observerstatechanged`.
//! - `dispatch`: the routed hook, policy and activations.
//! - `context`: the caller / depth / activation of the running invocation.
//! - `builtins`: `engine::observers::list | get | dispose`.
//! - `legacy`: the phase 5 shim from `RegisterTrigger` to observers.
//! - `events`: engine-hosted event sources (`engine::on-functions-changed`,
//!   `configuration::on-updated`, ...).

pub mod builtins;
mod config;
mod context;
mod dispatch;
pub mod events;
mod legacy;
mod lifecycle;
mod owner;
mod registry;

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, OnceLock, Weak, atomic::AtomicU64},
};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use uuid::Uuid;

pub use config::ObserversConfig;
pub use context::{ENGINE_CALLER_NAME, InvocationCtx};
pub use dispatch::{Emission, ObservedCall, OwnerCall, ReportHold};
pub use legacy::{LegacyRegistration, ShimBinding, ShimType};
pub use lifecycle::Observer;
pub use owner::{ActiveSet, validate_descriptor};
pub use registry::ObserveRequest;

use crate::{
    function::FunctionsRegistry, invocation::InvocationHandler, protocol::ObserverState,
    worker_connections::WorkerConnectionRegistry, workers::observability::metrics::EngineMetrics,
};

/// `(namespace, function_id)`.
pub type FnKey = (String, String);

/// The engine parts the registry works through. Set once by `Engine::new`;
/// the invocation handler is weak because it holds the registry.
pub(crate) struct Deps {
    pub functions: Arc<FunctionsRegistry>,
    pub workers: Arc<WorkerConnectionRegistry>,
    pub invocations: Weak<InvocationHandler>,
    /// The engine's `(namespace, function_id)` → owning connection map: who
    /// may report for, deliver for and ack observers of a function.
    pub function_owners: Arc<DashMap<FnKey, (Uuid, crate::engine::InvocationKind)>>,
}

/// An observing connection that went away, while its observers wait for a
/// `Reattach`.
struct Departed {
    token: Uuid,
    timer: tokio::task::AbortHandle,
}

/// Every observer, indexed for the routed hook (by function), for the
/// observing worker (by connection) and for target availability.
pub struct ObserverRegistry {
    config: ArcSwap<ObserversConfig>,
    deps: OnceLock<Deps>,
    runtime: OnceLock<tokio::runtime::Handle>,
    by_id: DashMap<String, Arc<Observer>>,
    by_connection: DashMap<Uuid, HashSet<String>>,
    by_function: DashMap<FnKey, Arc<owner::FunctionEntry>>,
    by_target: DashMap<FnKey, HashSet<String>>,
    /// The routed hook's view: auto functions with active observers. Read with
    /// one atomic load per invocation, replaced (copy-on-write) on change.
    observed: ArcSwap<HashMap<FnKey, Arc<ActiveSet>>>,
    /// Serializes rebuilds of `observed`.
    rebuild: Mutex<()>,
    notifiers: DashMap<Uuid, lifecycle::Notifier>,
    departed: DashMap<Uuid, Departed>,
    state_counts: lifecycle::StateCounts,
    payload_copies: AtomicU64,
    /// Times a reporting connection's reader waited for room in a queue its
    /// report overflowed.
    report_waits: AtomicU64,
    /// The routed hook's completions, evaluated off the invocation's task by
    /// one evaluator per shard. All of a function's completions go to the
    /// same shard, which evaluates them in the order they completed.
    evaluators: OnceLock<Vec<tokio::sync::mpsc::Sender<dispatch::Evaluation>>>,
    evaluated: AtomicU64,
    events_built: AtomicU64,
    /// Approximate payload bytes waiting for the evaluator.
    queued_bytes: std::sync::atomic::AtomicUsize,
    /// The observers' queues together.
    queue_budget: Arc<dispatch::QueueBudget>,
    /// Each evaluator shard's queue capacity, read when they start.
    evaluator_capacity: std::sync::atomic::AtomicUsize,
    /// The evaluator queue's byte budget.
    evaluator_bytes: std::sync::atomic::AtomicUsize,
    /// Recently disposed observers, by id: a replay from the same worker
    /// learns the disposal instead of re-creating the observer.
    tombstones: DashMap<String, lifecycle::Tombstone>,
    /// Tombstones by expiry, oldest first.
    tombstone_order: Mutex<std::collections::VecDeque<(std::time::Instant, String)>>,
    /// Observing connections whose grace period ended, by connection: a
    /// late `Reattach` learns their observers are disposed.
    expired: DashMap<Uuid, lifecycle::Expired>,
    /// `expired` by expiry, oldest first.
    expired_order: Mutex<lifecycle::ExpiredOrder>,
    /// Engine-hosted event sources (see [`events`]).
    event_sources: DashMap<FnKey, ()>,
    /// Functions registered with `observe.delivery: owner`: only their owner
    /// invokes them (see [`ObserverRegistry::may_invoke`]). Read on every
    /// invocation with one atomic load; replaced (copy-on-write) on change.
    owner_delivered: ArcSwap<HashSet<FnKey>>,
    /// Owner announcements made so far: each one's generation.
    announcements: AtomicU64,
    /// The last revision of functions whose entry was dropped as unused:
    /// revisions never go back within an engine's lifetime.
    revisions: DashMap<FnKey, u64>,
    /// `revisions` by insertion, oldest first.
    revision_order: Mutex<std::collections::VecDeque<(FnKey, u64)>>,
    /// The task that sends drop counts to the metric is running (see
    /// [`ObserverRegistry::export_drops`]).
    drop_export: std::sync::atomic::AtomicBool,
    /// Invocations the engine did not send because the target worker cannot
    /// receive them, newest last (see [`ObserverRegistry::refused_send`]).
    refused_sends: Mutex<std::collections::VecDeque<dispatch::RefusedSend>>,
}

impl Default for ObserverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ObserverRegistry {
    pub fn new() -> Self {
        Self {
            config: ArcSwap::from_pointee(ObserversConfig::default()),
            deps: OnceLock::new(),
            runtime: OnceLock::new(),
            by_id: DashMap::new(),
            by_connection: DashMap::new(),
            by_function: DashMap::new(),
            by_target: DashMap::new(),
            observed: ArcSwap::from_pointee(HashMap::new()),
            rebuild: Mutex::new(()),
            notifiers: DashMap::new(),
            departed: DashMap::new(),
            state_counts: lifecycle::StateCounts::default(),
            payload_copies: AtomicU64::new(0),
            report_waits: AtomicU64::new(0),
            evaluators: OnceLock::new(),
            evaluated: AtomicU64::new(0),
            events_built: AtomicU64::new(0),
            queued_bytes: std::sync::atomic::AtomicUsize::new(0),
            queue_budget: Arc::new(dispatch::QueueBudget::default()),
            evaluator_capacity: std::sync::atomic::AtomicUsize::new(dispatch::EVALUATOR_QUEUE),
            evaluator_bytes: std::sync::atomic::AtomicUsize::new(dispatch::EVALUATOR_BYTES),
            tombstones: DashMap::new(),
            tombstone_order: Mutex::new(std::collections::VecDeque::new()),
            expired: DashMap::new(),
            expired_order: Mutex::new(lifecycle::ExpiredOrder::default()),
            event_sources: DashMap::new(),
            owner_delivered: ArcSwap::from_pointee(HashSet::new()),
            announcements: AtomicU64::new(0),
            revisions: DashMap::new(),
            revision_order: Mutex::new(std::collections::VecDeque::new()),
            drop_export: std::sync::atomic::AtomicBool::new(false),
            refused_sends: Mutex::new(std::collections::VecDeque::new()),
        }
    }

    pub(crate) fn attach(
        &self,
        functions: Arc<FunctionsRegistry>,
        workers: Arc<WorkerConnectionRegistry>,
        invocations: Weak<InvocationHandler>,
        function_owners: Arc<DashMap<FnKey, (Uuid, crate::engine::InvocationKind)>>,
    ) {
        let _ = self.deps.set(Deps {
            functions,
            workers,
            invocations,
            function_owners,
        });
    }

    pub fn config(&self) -> Arc<ObserversConfig> {
        self.config.load_full()
    }

    /// Applies the `observers:` config section. Limits and timeouts apply to
    /// observers created (and grace periods started) from now on.
    pub fn set_config(&self, config: ObserversConfig) {
        self.config.store(Arc::new(config));
    }

    /// The runtime timers and activations run on: the current one, else the
    /// one the registry first saw.
    pub(crate) fn runtime(&self) -> Option<tokio::runtime::Handle> {
        tokio::runtime::Handle::try_current()
            .ok()
            .or_else(|| self.runtime.get().cloned())
    }

    fn remember_runtime(self: &Arc<Self>) {
        if self.runtime.get().is_none()
            && let Ok(handle) = tokio::runtime::Handle::try_current()
        {
            let _ = self.runtime.set(handle);
        }
        self.start_drop_export();
    }

    /// Starts the task that sends drop counts to `iii.observer.dropped.total`
    /// every [`dispatch::DROP_EXPORT_INTERVAL`], once there is a meter and a
    /// runtime.
    fn start_drop_export(self: &Arc<Self>) {
        use std::sync::atomic::Ordering;
        if metrics().is_none() || self.drop_export.load(Ordering::Acquire) {
            return;
        }
        let Some(runtime) = self.runtime() else {
            return;
        };
        if self.drop_export.swap(true, Ordering::AcqRel) {
            return;
        }
        let registry = Arc::downgrade(self);
        runtime.spawn(async move {
            let mut tick = tokio::time::interval(dispatch::DROP_EXPORT_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let Some(registry) = registry.upgrade() else {
                    return;
                };
                registry.export_drops();
            }
        });
    }

    /// Sends what each live observer dropped since the last export to
    /// `iii.observer.dropped.total`. A drop only counts in the observer
    /// (one atomic add, which matters when an overloaded engine drops by the
    /// million); the metric follows from here, and from the observer's own
    /// drop for what is left when it goes.
    pub fn export_drops(&self) {
        for observer in self.by_id.iter() {
            dispatch::export_drops(observer.value());
        }
    }

    pub fn get(&self, id: &str) -> Option<Arc<Observer>> {
        self.by_id.get(id).map(|o| o.clone())
    }

    pub fn observers(&self) -> Vec<Arc<Observer>> {
        self.by_id.iter().map(|o| o.value().clone()).collect()
    }

    /// Live observers in `state` (the `iii.observers` gauge).
    pub fn count_in_state(&self, state: &ObserverState) -> i64 {
        self.state_counts.get(state)
    }
}

/// The engine metrics, when the meter is initialized (it is on every serve
/// path; unit tests that never call `ensure_default_meter` get `None`).
pub(crate) fn metrics() -> Option<&'static EngineMetrics> {
    crate::workers::observability::metrics::get_meter()
        .map(|_| crate::workers::observability::metrics::get_engine_metrics())
}
