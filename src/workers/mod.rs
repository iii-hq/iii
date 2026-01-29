pub mod traits;

use std::{collections::HashSet, str::FromStr, sync::Arc};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use crate::engine::Outbound;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessMetrics {
    pub cpu_percent: Option<f64>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub process_uptime_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub thread_count: Option<u32>,
    pub open_connections: Option<u32>,
    pub invocations_per_sec: Option<f64>,
    pub avg_latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtendedMetrics {
    pub disk_read_bytes: Option<u64>,
    pub disk_write_bytes: Option<u64>,
    pub network_rx_bytes: Option<u64>,
    pub network_tx_bytes: Option<u64>,
    pub open_file_descriptors: Option<u32>,
    pub error_count: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KubernetesIdentifiers {
    pub cluster: Option<String>,
    pub namespace: Option<String>,
    pub pod_name: Option<String>,
    pub container_name: Option<String>,
    pub node_name: Option<String>,
    pub pod_uid: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KubernetesCoreMetrics {
    pub cpu_usage_cores: Option<f64>,
    pub memory_working_set_bytes: Option<u64>,
    pub pod_phase: Option<String>,
    pub pod_ready: Option<bool>,
    pub container_restarts_total: Option<u32>,
    pub last_termination_reason: Option<String>,
    pub uptime_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KubernetesResourceMetrics {
    pub cpu_requests_cores: Option<f64>,
    pub cpu_limits_cores: Option<f64>,
    pub memory_requests_bytes: Option<u64>,
    pub memory_limits_bytes: Option<u64>,
    pub cpu_throttled_seconds_total: Option<f64>,
    pub pod_pending_seconds: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KubernetesExtendedMetrics {
    pub network_rx_bytes_total: Option<u64>,
    pub network_tx_bytes_total: Option<u64>,
    pub fs_usage_bytes: Option<u64>,
    pub node_memory_pressure: Option<bool>,
    pub node_disk_pressure: Option<bool>,
    pub node_pid_pressure: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerMetrics {
    pub collected_at_ms: u64,
    #[serde(default)]
    pub process: ProcessMetrics,
    #[serde(default)]
    pub performance: PerformanceMetrics,
    #[serde(default)]
    pub extended: ExtendedMetrics,
    #[serde(default)]
    pub k8s_identifiers: Option<KubernetesIdentifiers>,
    #[serde(default)]
    pub k8s_core: Option<KubernetesCoreMetrics>,
    #[serde(default)]
    pub k8s_resources: Option<KubernetesResourceMetrics>,
    #[serde(default)]
    pub k8s_extended: Option<KubernetesExtendedMetrics>,
}

#[derive(Default)]
pub struct WorkerRegistry {
    pub workers: Arc<DashMap<Uuid, Worker>>,
    pub metrics: Arc<DashMap<Uuid, WorkerMetrics>>,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(DashMap::new()),
            metrics: Arc::new(DashMap::new()),
        }
    }

    pub fn get_worker(&self, id: &Uuid) -> Option<Worker> {
        self.workers.get(id).map(|w| w.value().clone())
    }

    pub fn register_worker(&self, worker: Worker) {
        self.workers.insert(worker.id, worker);
    }

    pub fn unregister_worker(&self, worker_id: &Uuid) {
        tracing::debug!("Unregistering worker: {}", worker_id);
        self.workers.remove(worker_id);
        self.metrics.remove(worker_id);
    }

    pub fn list_workers(&self) -> Vec<Worker> {
        self.workers
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn update_worker_metadata(
        &self,
        worker_id: &Uuid,
        runtime: String,
        version: Option<String>,
        name: Option<String>,
        os: Option<String>,
    ) {
        if let Some(mut worker) = self.workers.get_mut(worker_id) {
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

    pub fn update_worker_status(&self, worker_id: &Uuid, status: WorkerStatus) {
        if let Some(mut worker) = self.workers.get_mut(worker_id) {
            worker.status = status;
        }
    }

    pub fn update_worker_metrics(&self, worker_id: &Uuid, metrics: WorkerMetrics) {
        self.metrics.insert(*worker_id, metrics);
    }

    pub fn get_worker_metrics(&self, worker_id: &Uuid) -> Option<WorkerMetrics> {
        self.metrics.get(worker_id).map(|m| m.value().clone())
    }

    pub fn get_all_metrics(&self) -> Vec<(Uuid, WorkerMetrics)> {
        self.metrics
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect()
    }

    pub fn remove_worker_metrics(&self, worker_id: &Uuid) {
        self.metrics.remove(worker_id);
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
}

impl FromStr for WorkerStatus {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "available" => WorkerStatus::Available,
            "busy" => WorkerStatus::Busy,
            "disconnected" => WorkerStatus::Disconnected,
            _ => WorkerStatus::Connected,
        })
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
