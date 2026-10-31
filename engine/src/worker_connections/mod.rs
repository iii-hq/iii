// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod traits;

use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::{
        Arc, RwLock as StdRwLock,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use opentelemetry::KeyValue;
use tokio::sync::{Notify, RwLock, mpsc, watch};
use uuid::Uuid;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    engine::Outbound,
    protocol::{
        DEFAULT_NAMESPACE, FEATURE_LARGE_MESSAGES_V1, FEATURE_LOCAL_DISPATCH_V1,
        FEATURE_OBSERVE_V1, FEATURE_OBSERVE_V2, effective_namespace,
    },
    workers::{observability::metrics::get_engine_metrics, worker::rbac_session::Session},
};

#[derive(Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct WorkerConnectionTelemetryMeta {
    pub language: Option<String>,
    pub project_name: Option<String>,
    pub framework: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RuntimeWorkerInfo {
    pub id: String,
    pub name: String,
    /// One-line summary of what this builtin worker does, sourced from its
    /// `WorkerRegistration`. Surfaces in `engine::workers::list` / `info`.
    pub description: Option<String>,
    pub worker_type: String,
    pub connected_at: DateTime<Utc>,
    pub function_ids: Vec<String>,
    pub internal: bool,
}

impl std::fmt::Debug for WorkerConnectionTelemetryMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerConnectionTelemetryMeta")
            .field("language", &self.language)
            .field("project_name", &self.project_name)
            .field("framework", &self.framework)
            .finish()
    }
}

#[derive(Default)]
pub struct WorkerConnectionRegistry {
    pub workers: Arc<DashMap<Uuid, WorkerConnection>>,
}

impl WorkerConnectionRegistry {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(DashMap::new()),
        }
    }

    pub fn get_worker(&self, id: &Uuid) -> Option<WorkerConnection> {
        self.workers.get(id).map(|w| w.value().clone())
    }

    /// Name-only lookup. Avoids cloning the whole `WorkerConnection`
    /// (function-id sets, telemetry, session) when callers just attribute
    /// ownership by name.
    pub fn get_worker_name(&self, id: &Uuid) -> Option<String> {
        self.workers.get(id).and_then(|w| w.value().name.clone())
    }

    /// Effective namespace of a connection: the namespace it registered with,
    /// or [`DEFAULT_NAMESPACE`] when it sent none (or is unknown).
    pub fn get_namespace(&self, id: &Uuid) -> String {
        self.workers
            .get(id)
            .map(|w| effective_namespace(&w.value().namespace).to_string())
            .unwrap_or_else(|| DEFAULT_NAMESPACE.to_string())
    }

    pub fn register_worker(&self, worker: WorkerConnection) {
        let ip_address = worker.session.as_ref().map(|s| s.ip_address.as_str());
        tracing::info!(
            worker_id = %worker.id,
            ip_address = ?ip_address,
            "Worker registered"
        );
        self.workers.insert(worker.id, worker);
        let count = self.workers.len() as i64;

        // Update metrics
        let metrics = get_engine_metrics();
        metrics.workers_active.record(count, &[]);
        metrics.workers_spawns_total.add(1, &[]);

        // Update accumulator for readable metrics
        let acc = crate::workers::observability::metrics::get_metrics_accumulator();
        acc.workers_spawns
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        crate::workers::telemetry::collector::track_peak_workers(count as u64);
    }

    pub fn unregister_worker(&self, worker_id: &Uuid) {
        let (ip_address, pid) = self
            .workers
            .get(worker_id)
            .map(|w| (w.session.as_ref().map(|s| s.ip_address.clone()), w.pid))
            .unwrap_or((None, None));

        tracing::info!(
            worker_id = %worker_id,
            ip_address = ?ip_address,
            pid = ?pid,
            "Worker unregistered"
        );

        self.workers.remove(worker_id);
        let count = self.workers.len() as i64;

        // Update metrics
        let metrics = get_engine_metrics();
        metrics.workers_active.record(count, &[]);
        metrics.workers_deaths_total.add(1, &[]);

        // Update accumulator for readable metrics
        let acc = crate::workers::observability::metrics::get_metrics_accumulator();
        acc.workers_deaths
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn list_workers(&self) -> Vec<WorkerConnection> {
        self.workers
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Stores the metadata from `engine::workers::register` and sets the
    /// connection's receive limit from it and from the connection's declared
    /// features, which [`Self::set_worker_features`] must store first.
    #[allow(clippy::too_many_arguments)]
    pub fn update_worker_metadata(
        &self,
        worker_id: &Uuid,
        runtime: String,
        version: Option<String>,
        name: Option<String>,
        description: Option<String>,
        os: Option<String>,
        telemetry: Option<WorkerConnectionTelemetryMeta>,
        pid: Option<u32>,
        isolation: Option<String>,
        namespace: Option<String>,
    ) {
        if let Some(mut worker) = self.workers.get_mut(worker_id) {
            let (limit, requires_upgrade) = crate::workers::worker::receive_message_limit(
                Some(&runtime),
                version.as_deref(),
                &worker.features,
            );
            worker.receive_message_limit.store(limit, Ordering::Relaxed);
            worker
                .receive_limit_requires_upgrade
                .store(requires_upgrade, Ordering::Relaxed);
            *worker
                .receive_sdk_version
                .write()
                .unwrap_or_else(|error| error.into_inner()) = version.clone();
            worker.runtime = Some(runtime);
            worker.version = version;
            if name.is_some() {
                worker.name = name;
            }
            if description.is_some() {
                worker.description = description;
            }
            if os.is_some() {
                worker.os = os;
            }
            worker.telemetry = telemetry;
            if pid.is_some() {
                worker.pid = pid;
            }
            if isolation.is_some() {
                worker.isolation = isolation;
            }
            if namespace.is_some() {
                worker.namespace = namespace;
            }
        }
    }

    /// Records the protocol features `worker_id` declared on
    /// `engine::workers::register`. Every clone of the connection sees them.
    /// Returns whether the declaration was kept ([`WorkerFeatures::set`]).
    pub fn set_worker_features(&self, worker_id: &Uuid, features: &[String]) -> bool {
        self.workers
            .get(worker_id)
            .is_some_and(|worker| worker.features.set(features))
    }

    pub fn update_worker_status(&self, worker_id: &Uuid, status: WorkerConnectionStatus) {
        if let Some(mut worker) = self.workers.get_mut(worker_id) {
            worker.status = status;
        }

        // Update metrics - count workers for each status
        let mut status_counts = std::collections::HashMap::new();
        for w in self.workers.iter() {
            *status_counts.entry(w.value().status).or_insert(0i64) += 1;
        }

        let metrics = get_engine_metrics();
        for (st, count) in status_counts {
            metrics
                .workers_by_status
                .record(count, &[KeyValue::new("status", st.as_str())]);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum WorkerConnectionStatus {
    #[default]
    Connected,
    Available,
    Busy,
    Disconnected,
}

impl WorkerConnectionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkerConnectionStatus::Connected => "connected",
            WorkerConnectionStatus::Available => "available",
            WorkerConnectionStatus::Busy => "busy",
            WorkerConnectionStatus::Disconnected => "disconnected",
        }
    }
}

impl FromStr for WorkerConnectionStatus {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "available" => WorkerConnectionStatus::Available,
            "busy" => WorkerConnectionStatus::Busy,
            "disconnected" => WorkerConnectionStatus::Disconnected,
            _ => WorkerConnectionStatus::Connected,
        })
    }
}

/// Protocol features a connection declared on `engine::workers::register`
/// (protocol.md §1). Empty until then, and for SDKs older than 0.25.
///
/// Shared by every clone of the connection: `engine::workers::register` writes
/// through the registry's copy while `router_msg` and `cleanup_worker` hold the
/// snapshot taken at connect time, and both must see the same answer.
///
/// Set once (CONTRACT D17): the first declaration that lists any feature fixes
/// them; later ones are ignored. The list and the flags the engine acts on are
/// written together, and reading a flag is one atomic load.
#[derive(Debug, Default)]
pub struct WorkerFeatures {
    declared: std::sync::OnceLock<DeclaredFeatures>,
}

#[derive(Debug)]
struct DeclaredFeatures {
    list: Vec<String>,
    observe_v1: bool,
    observe_v2: bool,
    local_dispatch_v1: bool,
    large_messages_v1: bool,
}

impl WorkerFeatures {
    /// Features kept per connection.
    pub const MAX_FEATURES: usize = 32;
    /// Longest feature name kept.
    pub const MAX_FEATURE_LEN: usize = 64;

    /// Records the declared features. Returns whether this declaration was
    /// kept: `false` when it lists none, or when an earlier one already fixed
    /// them. Names longer than [`Self::MAX_FEATURE_LEN`] are dropped and at
    /// most [`Self::MAX_FEATURES`] are kept.
    pub fn set(&self, features: &[String]) -> bool {
        let list: Vec<String> = features
            .iter()
            .filter(|feature| !feature.is_empty() && feature.len() <= Self::MAX_FEATURE_LEN)
            .take(Self::MAX_FEATURES)
            .cloned()
            .collect();
        if list.is_empty() {
            return false;
        }
        let has = |name: &str| list.iter().any(|feature| feature == name);
        let observe_v2 = has(FEATURE_OBSERVE_V2);
        let declared = DeclaredFeatures {
            observe_v1: observe_v2 || has(FEATURE_OBSERVE_V1),
            observe_v2,
            local_dispatch_v1: has(FEATURE_LOCAL_DISPATCH_V1),
            large_messages_v1: has(FEATURE_LARGE_MESSAGES_V1),
            list,
        };
        self.declared.set(declared).is_ok()
    }

    /// The connection speaks observe: the engine may push the observe
    /// messages to it (CONTRACT D2). `observe.v2` includes it.
    pub fn observe_v1(&self) -> bool {
        self.declared.get().is_some_and(|d| d.observe_v1)
    }

    /// The connection delivers the calls of the functions it hosts: it lists
    /// the observers its delivery matched on each `invocationresult`, and the
    /// engine evaluates none of those calls itself.
    pub fn observe_v2(&self) -> bool {
        self.declared.get().is_some_and(|d| d.observe_v2)
    }

    pub fn local_dispatch_v1(&self) -> bool {
        self.declared.get().is_some_and(|d| d.local_dispatch_v1)
    }

    /// The connection receives JSON messages up to 64 MiB.
    pub fn large_messages_v1(&self) -> bool {
        self.declared.get().is_some_and(|d| d.large_messages_v1)
    }

    /// The list as kept, unknown features included.
    pub fn declared(&self) -> Vec<String> {
        self.declared
            .get()
            .map(|d| d.list.clone())
            .unwrap_or_default()
    }
}

#[derive(Clone)]
pub struct WorkerConnection {
    pub id: Uuid,
    pub channel: mpsc::Sender<Outbound>,
    pub function_ids: Arc<RwLock<HashSet<String>>>,
    pub external_function_ids: Arc<RwLock<HashSet<String>>>,
    pub invocations: Arc<RwLock<HashSet<Uuid>>>,
    pub runtime: Option<String>,
    pub version: Option<String>,
    pub connected_at: DateTime<Utc>,
    pub name: Option<String>,
    /// One-line summary reported by the worker via `engine::workers::register`.
    pub description: Option<String>,
    pub os: Option<String>,
    pub status: WorkerConnectionStatus,
    pub telemetry: Option<WorkerConnectionTelemetryMeta>,
    pub pid: Option<u32>,
    pub isolation: Option<String>,
    /// Namespace this connection registered with, reported via
    /// `engine::workers::register`. `None` means the worker sent no namespace
    /// and is treated as [`DEFAULT_NAMESPACE`] — read it through
    /// [`WorkerConnectionRegistry::get_namespace`] rather than directly.
    pub namespace: Option<String>,
    /// Cached maximum serialized JSON envelope accepted by this connection.
    pub receive_message_limit: Arc<AtomicUsize>,
    /// Whether a size rejection should recommend upgrading an unknown/old SDK.
    pub receive_limit_requires_upgrade: Arc<AtomicBool>,
    /// Shared with the connection writer for size-rejection diagnostics.
    pub receive_sdk_version: Arc<StdRwLock<Option<String>>>,
    pub session: Option<Arc<Session>>,
    /// Signaled when a reconnecting successor of this worker reattaches
    /// (`Message::Reattach`): the connection's read loop must exit (its exit
    /// path runs the connection's cleanup). Shared across clones so the
    /// registry's copy can wake the socket task.
    pub evict: Arc<Notify>,
    /// True once a `handle_worker` read loop services this connection — i.e.
    /// `evict` has a listener whose exit path will run the cleanup. Reattach
    /// uses this to choose between "evict and await the reader's own
    /// cleanup" and "no reader: clean up directly".
    pub has_reader: Arc<AtomicBool>,
    /// Claimed (swap to true) by the single caller that runs this
    /// connection's teardown; later callers await `cleanup_done` instead of
    /// returning early, so `cleanup_worker` returning always means the
    /// teardown has COMPLETED.
    pub cleanup_claimed: Arc<AtomicBool>,
    /// Flips to true when the connection's teardown has completed.
    /// `subscribe()` + `wait_for(|d| *d)` to await it.
    pub cleanup_done: Arc<watch::Sender<bool>>,
    /// Per-connection secret, sent to the worker only over its own socket
    /// (in `WorkerRegistered`). A reconnecting successor must present it in
    /// `Message::Reattach` to retire this connection: worker ids alone are
    /// public (`engine::workers::list`, `engine::on-workers-changed`) and
    /// must not suffice to evict a live worker.
    pub reattach_token: Uuid,
    /// Protocol features declared on `engine::workers::register`.
    pub features: Arc<WorkerFeatures>,
    /// The removed message types (`REMOVED_MESSAGES` in the engine) this
    /// connection was warned about, one bit each.
    pub removed_messages_warned: Arc<AtomicU8>,
    /// The observe depth of each in-flight invocation forwarded to this
    /// connection at depth > 0: the loop guard's stand-in for the depth of
    /// what the connection does meanwhile without saying (calls from a worker
    /// without `observe.v1`, registrations, telemetry; see
    /// [`WorkerConnection::inflight_depth`]).
    pub inflight_depths: Arc<std::sync::Mutex<HashMap<Uuid, u32>>>,
    /// The routed calls this connection answered last, when it declared
    /// `observe.v2`: its report items for them are refused (decision 38).
    pub routed_answers: Arc<crate::observer::RoutedAnswers>,
}

impl WorkerConnection {
    pub fn new(channel: mpsc::Sender<Outbound>) -> Self {
        let id = Uuid::new_v4();
        Self {
            id,
            channel,
            invocations: Arc::new(RwLock::new(HashSet::new())),
            function_ids: Arc::new(RwLock::new(HashSet::new())),
            external_function_ids: Arc::new(RwLock::new(HashSet::new())),
            runtime: None,
            version: None,
            connected_at: Utc::now(),
            name: None,
            description: None,
            os: None,
            status: WorkerConnectionStatus::Connected,
            telemetry: None,
            pid: None,
            isolation: None,
            namespace: None,
            receive_message_limit: Arc::new(AtomicUsize::new(
                crate::workers::worker::MAX_JSON_FRAME_BYTES,
            )),
            receive_limit_requires_upgrade: Arc::new(AtomicBool::new(true)),
            receive_sdk_version: Arc::new(StdRwLock::new(None)),
            session: None,
            evict: Arc::new(Notify::new()),
            has_reader: Arc::new(AtomicBool::new(false)),
            cleanup_claimed: Arc::new(AtomicBool::new(false)),
            cleanup_done: Arc::new(watch::channel(false).0),
            reattach_token: Uuid::new_v4(),
            features: Arc::new(WorkerFeatures::default()),
            removed_messages_warned: Arc::new(AtomicU8::new(0)),
            inflight_depths: Arc::new(std::sync::Mutex::new(HashMap::new())),
            routed_answers: Arc::new(crate::observer::RoutedAnswers::default()),
        }
    }

    pub fn with_session(channel: mpsc::Sender<Outbound>, session: Session) -> Self {
        let id = Uuid::new_v4();
        Self {
            id,
            channel,
            invocations: Arc::new(RwLock::new(HashSet::new())),
            function_ids: Arc::new(RwLock::new(HashSet::new())),
            external_function_ids: Arc::new(RwLock::new(HashSet::new())),
            runtime: None,
            version: None,
            connected_at: Utc::now(),
            name: None,
            description: None,
            os: None,
            status: WorkerConnectionStatus::Connected,
            telemetry: None,
            pid: None,
            isolation: None,
            namespace: None,
            session: Some(Arc::new(session)),
            evict: Arc::new(Notify::new()),
            has_reader: Arc::new(AtomicBool::new(false)),
            cleanup_claimed: Arc::new(AtomicBool::new(false)),
            cleanup_done: Arc::new(watch::channel(false).0),
            receive_message_limit: Arc::new(AtomicUsize::new(
                crate::workers::worker::MAX_JSON_FRAME_BYTES,
            )),
            receive_limit_requires_upgrade: Arc::new(AtomicBool::new(true)),
            receive_sdk_version: Arc::new(StdRwLock::new(None)),
            reattach_token: Uuid::new_v4(),
            features: Arc::new(WorkerFeatures::default()),
            removed_messages_warned: Arc::new(AtomicU8::new(0)),
            inflight_depths: Arc::new(std::sync::Mutex::new(HashMap::new())),
            routed_answers: Arc::new(crate::observer::RoutedAnswers::default()),
        }
    }

    pub async fn function_count(&self) -> usize {
        let regular = self.function_ids.read().await.len();
        let external = self.external_function_ids.read().await.len();
        regular + external
    }

    pub async fn invocation_count(&self) -> usize {
        self.invocations.read().await.len()
    }

    pub async fn get_function_ids(&self) -> Vec<String> {
        let mut function_ids = self.function_ids.read().await.clone();
        function_ids.extend(self.external_function_ids.read().await.iter().cloned());
        function_ids.into_iter().collect()
    }

    pub async fn get_regular_function_ids(&self) -> Vec<String> {
        self.function_ids.read().await.iter().cloned().collect()
    }

    pub async fn include_function_id(&self, function_id: &str) {
        self.function_ids
            .write()
            .await
            .insert(function_id.to_owned());
    }

    pub async fn remove_function_id(&self, function_id: &str) -> bool {
        self.function_ids.write().await.remove(function_id)
    }

    pub async fn include_external_function_id(&self, function_id: &str) {
        self.external_function_ids
            .write()
            .await
            .insert(function_id.to_owned());
    }

    pub async fn remove_external_function_id(&self, function_id: &str) -> bool {
        self.external_function_ids.write().await.remove(function_id)
    }

    pub async fn has_external_function_id(&self, function_id: &str) -> bool {
        self.external_function_ids
            .read()
            .await
            .contains(function_id)
    }

    pub async fn get_external_function_ids(&self) -> Vec<String> {
        self.external_function_ids
            .read()
            .await
            .iter()
            .cloned()
            .collect()
    }

    pub async fn add_invocation(&self, invocation_id: Uuid) {
        self.invocations.write().await.insert(invocation_id);
    }

    /// Forgets `invocation_id`; `true` when it was this connection's.
    pub async fn remove_invocation(&self, invocation_id: &Uuid) -> bool {
        let removed = self.invocations.write().await.remove(invocation_id);
        self.forget_inflight_depth(invocation_id);
        removed
    }

    /// Records that `invocation_id`, forwarded to this connection, runs at
    /// `depth` (> 0).
    pub fn remember_inflight_depth(&self, invocation_id: Uuid, depth: u32) {
        self.inflight_depths
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(invocation_id, depth);
    }

    pub fn forget_inflight_depth(&self, invocation_id: &Uuid) {
        let mut depths = self
            .inflight_depths
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !depths.is_empty() {
            depths.remove(invocation_id);
        }
    }

    /// The loop guard's heuristic for what this connection does without
    /// carrying `observe_depth` (calls from a worker without `observe.v1`,
    /// function registrations, telemetry): it is taken to be at the deepest
    /// depth among the invocations it is running. A false positive is
    /// possible only when the worker handles a deep activation and,
    /// meanwhile, does something unrelated. An invocation's depth stays
    /// until the worker answers it, the connection closes, or an activation
    /// timeout abandons it; a caller that gives up any other way does not
    /// clear it, so a hung handler pins its depth until then.
    pub fn inflight_depth(&self) -> Option<u32> {
        self.inflight_depths
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .copied()
            .max()
    }

    /// Whether this is the first frame of the removed message type `index`
    /// this connection sent: true once per connection and type.
    pub fn first_removed_message(&self, index: usize) -> bool {
        let bit = 1u8 << index;
        self.removed_messages_warned
            .fetch_or(bit, Ordering::Relaxed)
            & bit
            == 0
    }
}
#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn make_worker() -> WorkerConnection {
        let (tx, _rx) = mpsc::channel(8);
        WorkerConnection::new(tx)
    }

    #[test]
    fn worker_pid_defaults_to_none() {
        let worker = make_worker();
        assert!(worker.pid.is_none());
    }

    #[test]
    fn worker_features_are_fixed_by_the_first_declaration_that_lists_any() {
        let worker = make_worker();
        assert!(!worker.features.observe_v1());
        assert!(!worker.features.local_dispatch_v1());

        assert!(
            !worker.features.set(&[]),
            "an empty declaration fixes nothing"
        );
        assert!(worker.features.set(&["observe.v1".to_string()]));
        assert!(worker.features.observe_v1());
        assert!(!worker.features.local_dispatch_v1());

        assert!(
            !worker.features.set(&["local-dispatch.v1".to_string()]),
            "a later declaration is ignored"
        );
        assert!(worker.features.observe_v1());
        assert!(!worker.features.local_dispatch_v1());
        assert_eq!(worker.features.declared(), vec!["observe.v1"]);
    }

    #[test]
    fn observe_v2_includes_observe_v1() {
        let worker = make_worker();
        assert!(!worker.features.observe_v2());
        assert!(worker.features.set(&["observe.v2".to_string()]));
        assert!(worker.features.observe_v1() && worker.features.observe_v2());

        let v1 = make_worker();
        assert!(v1.features.set(&["observe.v1".to_string()]));
        assert!(v1.features.observe_v1() && !v1.features.observe_v2());
    }

    #[test]
    fn worker_features_are_bounded() {
        let worker = make_worker();
        let mut features: Vec<String> = (0..40).map(|i| format!("feature.{i}")).collect();
        features.insert(0, "x".repeat(WorkerFeatures::MAX_FEATURE_LEN + 1));
        features.push("observe.v1".to_string());
        assert!(worker.features.set(&features));
        let kept = worker.features.declared();
        assert_eq!(kept.len(), WorkerFeatures::MAX_FEATURES);
        assert!(
            kept.iter()
                .all(|f| f.len() <= WorkerFeatures::MAX_FEATURE_LEN)
        );
        assert_eq!(
            kept[0], "feature.0",
            "an over-long name is dropped, not truncated"
        );
        assert!(
            !worker.features.observe_v1(),
            "only what was kept counts: observe.v1 came after the first 32"
        );
    }

    #[test]
    fn set_worker_features_reaches_every_clone() {
        crate::workers::observability::metrics::ensure_default_meter();
        let registry = WorkerConnectionRegistry::new();
        let worker = make_worker();
        let snapshot = worker.clone();
        registry.register_worker(worker);

        registry.set_worker_features(
            &snapshot.id,
            &["observe.v1".to_string(), "observe.v2".to_string()],
        );

        assert!(snapshot.features.observe_v1());
        assert!(snapshot.features.observe_v2());
    }

    #[test]
    fn worker_telemetry_meta_debug_includes_all_fields() {
        let telemetry = WorkerConnectionTelemetryMeta {
            language: Some("rust".to_string()),
            project_name: Some("iii".to_string()),
            framework: Some("axum".to_string()),
        };

        let debug = format!("{telemetry:?}");
        assert!(debug.contains("rust"));
        assert!(debug.contains("iii"));
        assert!(debug.contains("axum"));
    }

    #[test]
    fn worker_status_as_str_and_from_str_cover_all_variants() {
        assert_eq!(WorkerConnectionStatus::Connected.as_str(), "connected");
        assert_eq!(WorkerConnectionStatus::Available.as_str(), "available");
        assert_eq!(WorkerConnectionStatus::Busy.as_str(), "busy");
        assert_eq!(
            WorkerConnectionStatus::Disconnected.as_str(),
            "disconnected"
        );

        assert_eq!(
            WorkerConnectionStatus::from_str("available").unwrap(),
            WorkerConnectionStatus::Available
        );
        assert_eq!(
            WorkerConnectionStatus::from_str("BUSY").unwrap(),
            WorkerConnectionStatus::Busy
        );
        assert_eq!(
            WorkerConnectionStatus::from_str("disconnected").unwrap(),
            WorkerConnectionStatus::Disconnected
        );
        assert_eq!(
            WorkerConnectionStatus::from_str("unknown").unwrap(),
            WorkerConnectionStatus::Connected
        );
    }

    #[tokio::test]
    async fn worker_with_session_tracks_functions_external_ids_and_invocations() {
        use crate::engine::Engine;
        use crate::workers::worker::WorkerManagerConfig;

        let (tx, _rx) = mpsc::channel(8);
        let session = Session {
            engine: Arc::new(Engine::new()),
            config: Arc::new(WorkerManagerConfig::default()),
            ip_address: "127.0.0.1".to_string(),
            session_id: Uuid::new_v4(),
            namespaces: Default::default(),
            allowed_functions: vec![],
            forbidden_functions: vec![],
            allow_function_registration: false,
            context: serde_json::json!({}),
            function_registration_prefix: None,
        };
        let worker = WorkerConnection::with_session(tx, session);
        assert_eq!(
            worker.session.as_ref().unwrap().ip_address.as_str(),
            "127.0.0.1"
        );

        worker.include_function_id("fn.local").await;
        worker.include_external_function_id("fn.remote").await;
        assert!(worker.has_external_function_id("fn.remote").await);
        assert_eq!(worker.function_count().await, 2);

        let mut function_ids = worker.get_function_ids().await;
        function_ids.sort();
        assert_eq!(
            function_ids,
            vec!["fn.local".to_string(), "fn.remote".to_string()]
        );
        assert_eq!(
            worker.get_regular_function_ids().await,
            vec!["fn.local".to_string()]
        );
        assert_eq!(
            worker.get_external_function_ids().await,
            vec!["fn.remote".to_string()]
        );

        assert!(worker.remove_function_id("fn.local").await);
        assert!(worker.remove_external_function_id("fn.remote").await);
        assert_eq!(worker.function_count().await, 0);

        let invocation_id = Uuid::new_v4();
        worker.add_invocation(invocation_id).await;
        assert_eq!(worker.invocation_count().await, 1);
        worker.remove_invocation(&invocation_id).await;
        assert_eq!(worker.invocation_count().await, 0);
    }

    #[test]
    fn unregister_worker_does_not_panic_with_unknown_id() {
        crate::workers::observability::metrics::ensure_default_meter();
        let registry = WorkerConnectionRegistry::new();
        registry.unregister_worker(&Uuid::new_v4());
    }

    #[tokio::test]
    async fn worker_registry_registers_updates_and_unregisters_workers() {
        crate::workers::observability::metrics::ensure_default_meter();

        let registry = WorkerConnectionRegistry::new();
        let worker = make_worker();
        let worker_id = worker.id;
        registry.register_worker(worker);

        let telemetry = WorkerConnectionTelemetryMeta {
            language: Some("rust".to_string()),
            project_name: Some("iii".to_string()),
            framework: Some("tokio".to_string()),
        };

        registry.update_worker_metadata(
            &worker_id,
            "node".to_string(),
            Some("1.0.0".to_string()),
            Some("worker-a".to_string()),
            None,
            Some("linux".to_string()),
            Some(telemetry.clone()),
            None,
            None,
            None,
        );
        registry.update_worker_status(&worker_id, WorkerConnectionStatus::Busy);
        registry.update_worker_status(&Uuid::new_v4(), WorkerConnectionStatus::Available);

        let stored = registry.get_worker(&worker_id).expect("registered worker");
        assert_eq!(stored.runtime.as_deref(), Some("node"));
        assert_eq!(stored.version.as_deref(), Some("1.0.0"));
        assert_eq!(stored.name.as_deref(), Some("worker-a"));
        assert_eq!(stored.os.as_deref(), Some("linux"));
        assert_eq!(stored.status, WorkerConnectionStatus::Busy);
        assert!(stored.pid.is_none());
        assert!(stored.isolation.is_none());
        assert_eq!(
            serde_json::to_value(stored.telemetry).expect("serialize telemetry"),
            json!(telemetry)
        );

        assert_eq!(registry.list_workers().len(), 1);
        registry.unregister_worker(&worker_id);
        assert!(registry.get_worker(&worker_id).is_none());
        assert!(registry.list_workers().is_empty());
    }

    #[test]
    fn update_worker_metadata_stores_pid_and_isolation() {
        crate::workers::observability::metrics::ensure_default_meter();
        let registry = WorkerConnectionRegistry::new();
        let worker = make_worker();
        let worker_id = worker.id;
        registry.register_worker(worker);

        registry.update_worker_metadata(
            &worker_id,
            "node".to_string(),
            Some("18.0.0".to_string()),
            None,
            None,
            None,
            None,
            Some(1234u32),
            Some("libkrun".to_string()),
            None,
        );

        let stored = registry.get_worker(&worker_id).expect("worker exists");
        assert_eq!(stored.pid, Some(1234u32));
        assert_eq!(stored.isolation.as_deref(), Some("libkrun"));
    }

    #[test]
    fn get_namespace_falls_back_to_default() {
        crate::workers::observability::metrics::ensure_default_meter();
        let registry = WorkerConnectionRegistry::new();
        let worker = make_worker();
        let worker_id = worker.id;
        assert!(worker.namespace.is_none());
        registry.register_worker(worker);

        assert_eq!(registry.get_namespace(&worker_id), DEFAULT_NAMESPACE);
    }

    #[test]
    fn get_namespace_returns_connection_namespace() {
        crate::workers::observability::metrics::ensure_default_meter();
        let registry = WorkerConnectionRegistry::new();
        let worker = make_worker();
        let worker_id = worker.id;
        registry.register_worker(worker);

        registry.update_worker_metadata(
            &worker_id,
            "node".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("orders".to_string()),
        );

        assert_eq!(registry.get_namespace(&worker_id), "orders");
    }

    #[test]
    fn worker_connection_constructors_default_isolation_to_none() {
        let (tx, _rx) = mpsc::channel::<Outbound>(1);
        let worker = WorkerConnection::new(tx);
        assert!(worker.isolation.is_none());
    }
}
