// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Observers: the engine side of `iii.observe(source, target)`
//! (`tech-specs/2026-10-13-observe/protocol.md` §3,
//! `tech-specs/2026-10-29-observe-v2`).
//!
//! - `registry`: `observe` / `unobserve`, validation, replay, the observing
//!   connection's disconnect, grace and `Reattach`.
//! - `owner`: the observed functions, kept in step with the function registry
//!   (options, availability, host sync, the routed hook's active sets).
//! - `lifecycle`: observer states, derived from facts, and
//!   `observerstatechanged`.
//! - `dispatch`: the routed hook, reports, calls through an observer, policy
//!   and activations.
//! - `context`: the caller / depth / activation of the running invocation.
//! - `builtins`: `engine::observers::list | get | dispose`.
//! - `events`: engine-hosted sources (`engine::on-functions-changed`,
//!   `configuration::on-updated`, ...).
//! - `engine_owned`: observers the engine creates for its own modules.

pub mod builtins;
mod config;
mod context;
mod dispatch;
pub mod engine_owned;
pub mod events;
mod lifecycle;
mod owner;
mod persistent;
mod registry;

use std::{
    collections::{HashMap, HashSet},
    hash::{BuildHasherDefault, Hash, Hasher},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicU64, AtomicUsize},
    },
};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use uuid::Uuid;

pub use config::ObserversConfig;
pub use context::{ENGINE_CALLER_NAME, InvocationCtx};
pub use dispatch::{Emission, HostDelivery, ObservedCall, ObserverCall, ReportHold, RoutedAnswers};
pub use engine_owned::{ENGINE_CONNECTION, EngineObserver, StandingObserver};
pub use lifecycle::Observer;
pub use owner::{ActiveSet, FunctionOptions, validate_args_format};
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

/// One observer in an index, by identity: two observers under the same id
/// (one replacing the other, decision 51) are two entries, and taking one out
/// leaves the other. The index holds the observer, so its address names it
/// for as long as the entry is there.
#[derive(Clone)]
pub(crate) struct Held(pub(crate) Arc<Observer>);

impl PartialEq for Held {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Held {}

impl Hash for Held {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(Arc::as_ptr(&self.0) as usize);
    }
}

/// Hashes an observer's address ([`Held`]): addresses are the engine's own,
/// so a multiply and a shift spread them well enough.
#[derive(Default)]
pub(crate) struct AddressHasher(u64);

impl Hasher for AddressHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0.rotate_left(8) ^ u64::from(*byte)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        }
    }

    fn write_usize(&mut self, address: usize) {
        // Allocations are 16-byte aligned: the low bits say nothing.
        self.0 = ((address as u64) >> 4).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        self.0 ^= self.0 >> 29;
    }
}

/// Observers indexed by identity (see [`Held`]).
pub(crate) type HeldSet = HashSet<Held, BuildHasherDefault<AddressHasher>>;

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
    /// The `Arc` this registry lives in, for work started where only `&self`
    /// is at hand (an engine-hosted source's event, a standing observer).
    this: OnceLock<Weak<ObserverRegistry>>,
    runtime: OnceLock<tokio::runtime::Handle>,
    by_id: DashMap<String, Arc<Observer>>,
    /// Each live observer under its observing connection, and under its
    /// target, by identity. An observer is filed in both in the same step
    /// that gives it its id in `by_id`, under its status lock, so whoever
    /// finds it by id (to dispose of it) finds it here too.
    by_connection: DashMap<Uuid, HeldSet>,
    by_function: DashMap<FnKey, Arc<owner::FunctionEntry>>,
    by_target: DashMap<FnKey, HeldSet>,

    /// The routed hook's view (see [`ObserverRegistry::route`]): functions
    /// with active observers and private functions a connection hosts. Read
    /// with one atomic load per invocation, replaced (copy-on-write) on
    /// change.
    routed: ArcSwap<owner::RoutedView>,
    /// Serializes rebuilds of `routed`.
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
    /// Engine-owned observers kept in every namespace that registers their
    /// function (see [`ObserverRegistry::observe_everywhere`]).
    standing: Mutex<Vec<StandingObserver>>,
    /// `standing.len()`, read on every registration with one atomic load.
    standing_count: AtomicUsize,
    /// Owner announcements made so far: each one's generation.
    announcements: AtomicU64,
    /// Observers that joined their function so far: the last one's
    /// [`Observer::seq`].
    created: AtomicU64,
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
            this: OnceLock::new(),
            runtime: OnceLock::new(),
            by_id: DashMap::new(),
            by_connection: DashMap::new(),
            by_function: DashMap::new(),
            by_target: DashMap::new(),
            routed: ArcSwap::from_pointee(HashMap::new()),
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
            standing: Mutex::new(Vec::new()),
            standing_count: AtomicUsize::new(0),
            announcements: AtomicU64::new(0),
            created: AtomicU64::new(0),
            revisions: DashMap::new(),
            revision_order: Mutex::new(std::collections::VecDeque::new()),
            drop_export: std::sync::atomic::AtomicBool::new(false),
            refused_sends: Mutex::new(std::collections::VecDeque::new()),
        }
    }

    pub(crate) fn attach(
        self: &Arc<Self>,
        functions: Arc<FunctionsRegistry>,
        workers: Arc<WorkerConnectionRegistry>,
        invocations: Weak<InvocationHandler>,
        function_owners: Arc<DashMap<FnKey, (Uuid, crate::engine::InvocationKind)>>,
    ) {
        let _ = self.this.set(Arc::downgrade(self));
        let _ = self.deps.set(Deps {
            functions,
            workers,
            invocations,
            function_owners,
        });
    }

    /// The `Arc` this registry lives in, once [`Self::attach`] ran.
    fn this(&self) -> Option<Arc<Self>> {
        self.this.get().and_then(Weak::upgrade)
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

    /// Live observers of `(namespace, function_id)`, in any state but
    /// `disposed`.
    pub fn observer_count(&self, namespace: &str, function_id: &str) -> usize {
        self.function_entry(&(namespace.to_string(), function_id.to_string()))
            .map_or(0, |entry| entry.len())
    }

    /// Ids of the live observers `connection` created, sorted.
    pub fn created_by(&self, connection: Uuid) -> Vec<String> {
        let mut ids: Vec<String> = self
            .by_connection
            .get(&connection)
            .map(|held| held.iter().map(|held| held.0.id.clone()).collect())
            .unwrap_or_default();
        ids.sort();
        ids
    }

    /// The live observers `connection` observes for.
    fn observing_for(&self, connection: Uuid) -> Vec<Arc<Observer>> {
        self.by_connection
            .get(&connection)
            .map(|held| held.iter().map(|held| held.0.clone()).collect())
            .unwrap_or_default()
    }
}

/// The engine metrics, when the meter is initialized (it is on every serve
/// path; unit tests that never call `ensure_default_meter` get `None`).
pub(crate) fn metrics() -> Option<&'static EngineMetrics> {
    #[cfg(test)]
    if let Some(recorded) = test_metrics::RECORDING.with(std::cell::Cell::get) {
        return Some(recorded);
    }
    crate::workers::observability::metrics::get_meter()
        .map(|_| crate::workers::observability::metrics::get_engine_metrics())
}

/// What the complexity tests count: observer slots copied into a new node of
/// an active set, or copied out of a function's observers in bulk, the one way
/// to scan them, on this thread. A change that rebuilt a function's set from
/// its observers, or scanned them, would count them all.
#[cfg(test)]
pub(crate) mod work {
    use std::cell::Cell;

    thread_local! {
        static SLOTS: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn add(slots: usize) {
        SLOTS.with(|counted| counted.set(counted.get() + slots));
    }

    /// What was counted since the last call.
    pub(crate) fn take() -> usize {
        SLOTS.with(|counted| counted.replace(0))
    }
}

/// Points where a test's threads pause at random ([`interleave::point`]), so
/// another thread's work lands in between: only on threads that asked for it
/// ([`interleave::enable`]).
#[cfg(test)]
pub(crate) mod interleave {
    use std::cell::Cell;

    thread_local! {
        /// The thread's xorshift state; `0` while it pauses nowhere.
        static STATE: Cell<u64> = const { Cell::new(0) };
    }

    /// Pauses this thread at random at every point from now on.
    pub(crate) fn enable(seed: u64) {
        STATE.with(|state| state.set(seed | 1));
    }

    /// One point: a yield one time in four, a sleep of up to 63 µs another,
    /// nothing otherwise.
    pub(crate) fn point() {
        STATE.with(|state| {
            let mut x = state.get();
            if x == 0 {
                return;
            }
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            state.set(x);
            match x % 4 {
                0 => std::thread::yield_now(),
                1 => std::thread::sleep(std::time::Duration::from_micros(x >> 58)),
                _ => {}
            }
        });
    }
}

/// Reads back the observer metrics a test emits: [`test_metrics::record`]
/// sends this thread's to a meter of its own, which the test reads.
#[cfg(test)]
pub(crate) mod test_metrics {
    use std::{
        cell::Cell,
        sync::{Arc, Mutex},
    };

    use opentelemetry::{KeyValue, metrics::MeterProvider as _};
    use opentelemetry_sdk::{
        error::OTelSdkResult,
        metrics::{
            PeriodicReader, SdkMeterProvider, Temporality,
            data::{AggregatedMetrics, MetricData, ResourceMetrics},
            exporter::PushMetricExporter,
        },
    };

    use crate::workers::observability::metrics::EngineMetrics;

    thread_local! {
        pub(super) static RECORDING: Cell<Option<&'static EngineMetrics>> =
            const { Cell::new(None) };
    }

    /// One `u64` sum series: `(metric, attributes, value)`.
    type Series = (String, Vec<KeyValue>, u64);

    /// The `u64` sums of the last export.
    #[derive(Clone, Default)]
    struct Exported(Arc<Mutex<Vec<Series>>>);

    impl PushMetricExporter for Exported {
        fn export(
            &self,
            metrics: &ResourceMetrics,
        ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
            let mut sums = Vec::new();
            for scope in metrics.scope_metrics() {
                for metric in scope.metrics() {
                    if let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() {
                        for point in sum.data_points() {
                            sums.push((
                                metric.name().to_string(),
                                point.attributes().cloned().collect(),
                                point.value(),
                            ));
                        }
                    }
                }
            }
            *self.0.lock().unwrap() = sums;
            std::future::ready(Ok(()))
        }

        fn force_flush(&self) -> OTelSdkResult {
            Ok(())
        }

        fn shutdown_with_timeout(&self, _: std::time::Duration) -> OTelSdkResult {
            Ok(())
        }

        fn temporality(&self) -> Temporality {
            Temporality::Cumulative
        }
    }

    /// The metrics this thread emitted since [`record`].
    pub(crate) struct Recorded {
        provider: SdkMeterProvider,
        exported: Exported,
    }

    impl Recorded {
        /// The sum of counter `name` over its series carrying every one of
        /// `attributes`.
        pub(crate) fn sum(&self, name: &str, attributes: &[(&str, &str)]) -> u64 {
            self.provider.force_flush().expect("export the metrics");
            self.exported
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|(metric, series, _)| {
                    metric == name
                        && attributes.iter().all(|(key, value)| {
                            series
                                .iter()
                                .any(|kv| kv.key.as_str() == *key && kv.value.as_str() == *value)
                        })
                })
                .map(|(_, _, value)| value)
                .sum()
        }
    }

    impl Drop for Recorded {
        fn drop(&mut self) {
            RECORDING.with(|slot| slot.set(None));
        }
    }

    /// Sends the observer metrics this thread emits from now on to a meter
    /// the returned [`Recorded`] reads. Async tests use a current-thread
    /// runtime, so the tasks they spawn emit on this thread too.
    pub(crate) fn record() -> Recorded {
        let exported = Exported::default();
        let provider = SdkMeterProvider::builder()
            .with_reader(PeriodicReader::builder(exported.clone()).build())
            .build();
        let metrics: &'static EngineMetrics =
            Box::leak(Box::new(EngineMetrics::with_meter(&provider.meter("iii"))));
        RECORDING.with(|slot| slot.set(Some(metrics)));
        Recorded { provider, exported }
    }
}
