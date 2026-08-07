// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod traits;

use std::{
    collections::HashSet,
    str::FromStr,
    sync::{Arc, atomic::AtomicBool},
};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use opentelemetry::KeyValue;
use tokio::sync::{Notify, RwLock, mpsc, oneshot, watch};
use uuid::Uuid;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    engine::Outbound,
    protocol::ErrorBody,
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
    ) {
        if let Some(mut worker) = self.workers.get_mut(worker_id) {
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
        }
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

    /// Removes an orphaned invocation id from whichever worker's in-flight set
    /// holds it. Used by the dispatch-timeout path, which abandons an
    /// invocation without knowing its worker binding (the `Invocation` is
    /// created with `worker_id: None`; routing happens deeper, in
    /// `WorkerConnection::handle_function`). Without this, a timed-out
    /// dispatch to a worker that never replies leaves its id in
    /// `WorkerConnection::invocations` until disconnect, inflating
    /// `invocation_count()`. Cold path — a linear scan over workers is fine.
    pub async fn remove_invocation_from_workers(&self, invocation_id: &Uuid) {
        // Snapshot the per-worker set handles first so no DashMap shard guard
        // is held across an await.
        let sets: Vec<_> = self
            .workers
            .iter()
            .map(|w| w.value().invocations.clone())
            .collect();
        for set in sets {
            if set.write().await.remove(invocation_id) {
                return;
            }
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
    pub session: Option<Arc<Session>>,
    /// In-flight `Message::RegisterTrigger` deliveries awaiting this worker's
    /// `TriggerRegistrationResult` ack, keyed by trigger id. Only ownerless
    /// (function-path) registrations wait on an entry here — see the
    /// `TriggerRegistrator` impl in `traits.rs`.
    pub pending_trigger_acks: Arc<DashMap<String, oneshot::Sender<Option<ErrorBody>>>>,
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
    /// public (`engine::workers::list`, the workers-available trigger) and
    /// must not suffice to evict a live worker.
    pub reattach_token: Uuid,
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
            session: None,
            pending_trigger_acks: Arc::new(DashMap::new()),
            evict: Arc::new(Notify::new()),
            has_reader: Arc::new(AtomicBool::new(false)),
            cleanup_claimed: Arc::new(AtomicBool::new(false)),
            cleanup_done: Arc::new(watch::channel(false).0),
            reattach_token: Uuid::new_v4(),
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
            session: Some(Arc::new(session)),
            pending_trigger_acks: Arc::new(DashMap::new()),
            evict: Arc::new(Notify::new()),
            has_reader: Arc::new(AtomicBool::new(false)),
            cleanup_claimed: Arc::new(AtomicBool::new(false)),
            cleanup_done: Arc::new(watch::channel(false).0),
            reattach_token: Uuid::new_v4(),
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

    pub async fn remove_invocation(&self, invocation_id: &Uuid) {
        self.invocations.write().await.remove(invocation_id);
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
            allowed_functions: vec![],
            forbidden_functions: vec![],
            allowed_trigger_types: Some(vec![]),
            allow_function_registration: false,
            allow_trigger_type_registration: false,
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
        );

        let stored = registry.get_worker(&worker_id).expect("worker exists");
        assert_eq!(stored.pid, Some(1234u32));
        assert_eq!(stored.isolation.as_deref(), Some("libkrun"));
    }

    #[test]
    fn worker_connection_constructors_default_isolation_to_none() {
        let (tx, _rx) = mpsc::channel::<Outbound>(1);
        let worker = WorkerConnection::new(tx);
        assert!(worker.isolation.is_none());
    }
}
