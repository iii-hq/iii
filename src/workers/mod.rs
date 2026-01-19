pub mod traits;

use std::{collections::HashSet, sync::Arc};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use crate::engine::Outbound;

#[derive(Default)]
pub struct WorkerRegistry {
    pub workers: Arc<RwLock<DashMap<Uuid, Worker>>>,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(RwLock::new(DashMap::new())),
        }
    }

    pub async fn get_worker(&self, id: &Uuid) -> Option<Worker> {
        self.workers.read().await.get(id).map(|w| w.value().clone())
    }

    pub async fn register_worker(&self, worker: Worker) {
        self.workers.write().await.insert(worker.id, worker);
    }

    pub async fn unregister_worker(&self, worker_id: &Uuid) {
        tracing::debug!("Unregistering worker: {}", worker_id);
        self.workers.write().await.remove(worker_id);
    }

    pub async fn list_workers(&self) -> Vec<Worker> {
        self.workers
            .read()
            .await
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub async fn update_worker_metadata(
        &self,
        worker_id: &Uuid,
        runtime: String,
        version: Option<String>,
        name: Option<String>,
        os: Option<String>,
    ) {
        if let Some(mut worker) = self.workers.write().await.get_mut(worker_id) {
            worker.runtime = Some(runtime);
            worker.version = version;
            if name.is_some() {
                worker.name = name;
            }
            if os.is_some() {
                worker.os = os;
            }
        }
    }

    pub async fn update_worker_status(&self, worker_id: &Uuid, status: WorkerStatus) {
        if let Some(mut worker) = self.workers.write().await.get_mut(worker_id) {
            worker.status = status;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkerStatus {
    #[default]
    Connected,
    Available,
    Busy,
    Disconnected,
}

impl WorkerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkerStatus::Connected => "connected",
            WorkerStatus::Available => "available",
            WorkerStatus::Busy => "busy",
            WorkerStatus::Disconnected => "disconnected",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "available" => WorkerStatus::Available,
            "busy" => WorkerStatus::Busy,
            "disconnected" => WorkerStatus::Disconnected,
            _ => WorkerStatus::Connected,
        }
    }
}

#[derive(Clone)]
pub struct Worker {
    pub id: Uuid,
    pub channel: mpsc::Sender<Outbound>,
    pub function_paths: Arc<RwLock<HashSet<String>>>,
    pub invocations: Arc<RwLock<HashSet<Uuid>>>,
    pub runtime: Option<String>,
    pub version: Option<String>,
    pub connected_at: DateTime<Utc>,
    pub name: Option<String>,
    pub os: Option<String>,
    pub ip_address: Option<String>,
    pub status: WorkerStatus,
}

impl Worker {
    pub fn new(channel: mpsc::Sender<Outbound>) -> Self {
        let id = Uuid::new_v4();
        Self {
            id,
            channel,
            invocations: Arc::new(RwLock::new(HashSet::new())),
            function_paths: Arc::new(RwLock::new(HashSet::new())),
            runtime: None,
            version: None,
            connected_at: Utc::now(),
            name: None,
            os: None,
            ip_address: None,
            status: WorkerStatus::Connected,
        }
    }

    pub fn with_ip(channel: mpsc::Sender<Outbound>, ip_address: String) -> Self {
        let id = Uuid::new_v4();
        Self {
            id,
            channel,
            invocations: Arc::new(RwLock::new(HashSet::new())),
            function_paths: Arc::new(RwLock::new(HashSet::new())),
            runtime: None,
            version: None,
            connected_at: Utc::now(),
            name: None,
            os: None,
            ip_address: Some(ip_address),
            status: WorkerStatus::Connected,
        }
    }

    pub async fn function_count(&self) -> usize {
        self.function_paths.read().await.len()
    }

    pub async fn invocation_count(&self) -> usize {
        self.invocations.read().await.len()
    }

    pub async fn get_function_paths(&self) -> Vec<String> {
        self.function_paths.read().await.iter().cloned().collect()
    }
    pub async fn include_function_path(&self, function_path: &str) {
        self.function_paths
            .write()
            .await
            .insert(function_path.to_owned());
    }

    pub async fn add_invocation(&self, invocation_id: Uuid) {
        self.invocations.write().await.insert(invocation_id);
    }

    pub async fn remove_invocation(&self, invocation_id: &Uuid) {
        self.invocations.write().await.remove(invocation_id);
    }
}
