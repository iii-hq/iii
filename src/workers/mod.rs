pub mod traits;

use std::{collections::HashSet, str::FromStr, sync::Arc};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use crate::engine::Outbound;

// =============================================================================
// Worker Metrics - Tiered based on Sergio's EKS/SRE feedback
// =============================================================================

/// Tier 1: Essential process-level metrics (Must Have)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessMetrics {
    /// Current CPU usage percentage (0-100)
    pub cpu_percent: Option<f64>,
    /// Process memory usage in bytes
    pub memory_used_bytes: Option<u64>,
    /// Total available memory in bytes
    pub memory_total_bytes: Option<u64>,
    /// Time since worker started in seconds
    pub process_uptime_secs: Option<u64>,
}

/// Tier 2: Important process-level metrics (Should Have)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Number of active threads
    pub thread_count: Option<u32>,
    /// Number of open network connections
    pub open_connections: Option<u32>,
    /// Invocations processed per second
    pub invocations_per_sec: Option<f64>,
    /// Average invocation latency in milliseconds
    pub avg_latency_ms: Option<f64>,
}

/// Tier 3: Nice to have process-level metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtendedMetrics {
    /// Disk read bytes
    pub disk_read_bytes: Option<u64>,
    /// Disk write bytes
    pub disk_write_bytes: Option<u64>,
    /// Network bytes received
    pub network_rx_bytes: Option<u64>,
    /// Network bytes transmitted
    pub network_tx_bytes: Option<u64>,
    /// Number of open file descriptors
    pub open_file_descriptors: Option<u32>,
    /// Total failed invocations count
    pub error_count: Option<u64>,
}

/// Kubernetes/EKS-specific identifiers for correlation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KubernetesIdentifiers {
    /// Cluster name
    pub cluster: Option<String>,
    /// Kubernetes namespace
    pub namespace: Option<String>,
    /// Pod name
    pub pod_name: Option<String>,
    /// Container name
    pub container_name: Option<String>,
    /// Node name
    pub node_name: Option<String>,
    /// Pod UID for unique identification
    pub pod_uid: Option<String>,
}

/// Tier 1: Essential Kubernetes metrics (Must Have for EKS/K8s)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KubernetesCoreMetrics {
    /// CPU usage in cores (or millicores)
    pub cpu_usage_cores: Option<f64>,
    /// Memory working set bytes
    pub memory_working_set_bytes: Option<u64>,
    /// Pod phase (Pending, Running, Succeeded, Failed, Unknown)
    pub pod_phase: Option<String>,
    /// Whether pod is ready to accept traffic
    pub pod_ready: Option<bool>,
    /// Total container restarts
    pub container_restarts_total: Option<u32>,
    /// Last termination reason (e.g., OOMKilled, Error)
    pub last_termination_reason: Option<String>,
    /// Container uptime in seconds
    pub uptime_seconds: Option<u64>,
}

/// Tier 2: Important Kubernetes metrics (Should Have for EKS/K8s)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KubernetesResourceMetrics {
    /// CPU requests in cores
    pub cpu_requests_cores: Option<f64>,
    /// CPU limits in cores
    pub cpu_limits_cores: Option<f64>,
    /// Memory requests in bytes
    pub memory_requests_bytes: Option<u64>,
    /// Memory limits in bytes
    pub memory_limits_bytes: Option<u64>,
    /// CPU throttled time in seconds
    pub cpu_throttled_seconds_total: Option<f64>,
    /// Time pod spent in pending state
    pub pod_pending_seconds: Option<f64>,
}

/// Tier 3: Nice to have Kubernetes metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KubernetesExtendedMetrics {
    /// Network received bytes (pod/container)
    pub network_rx_bytes_total: Option<u64>,
    /// Network transmitted bytes (pod/container)
    pub network_tx_bytes_total: Option<u64>,
    /// Filesystem usage in bytes
    pub fs_usage_bytes: Option<u64>,
    /// Node memory pressure
    pub node_memory_pressure: Option<bool>,
    /// Node disk pressure
    pub node_disk_pressure: Option<bool>,
    /// Node PID pressure
    pub node_pid_pressure: Option<bool>,
}

/// Complete worker metrics payload
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerMetrics {
    /// Timestamp when metrics were collected (Unix epoch ms)
    pub collected_at_ms: u64,

    // Process-level metrics (all runtimes)
    /// Tier 1: Essential process metrics
    #[serde(default)]
    pub process: ProcessMetrics,
    /// Tier 2: Performance metrics
    #[serde(default)]
    pub performance: PerformanceMetrics,
    /// Tier 3: Extended metrics
    #[serde(default)]
    pub extended: ExtendedMetrics,

    // Kubernetes-specific (when running on K8s/EKS)
    /// Kubernetes identifiers for correlation
    #[serde(default)]
    pub k8s_identifiers: Option<KubernetesIdentifiers>,
    /// Tier 1: Essential K8s metrics
    #[serde(default)]
    pub k8s_core: Option<KubernetesCoreMetrics>,
    /// Tier 2: K8s resource metrics
    #[serde(default)]
    pub k8s_resources: Option<KubernetesResourceMetrics>,
    /// Tier 3: Extended K8s metrics
    #[serde(default)]
    pub k8s_extended: Option<KubernetesExtendedMetrics>,
}

// =============================================================================
// Worker Registry
// =============================================================================

#[derive(Default)]
pub struct WorkerRegistry {
    pub workers: Arc<RwLock<DashMap<Uuid, Worker>>>,
    pub metrics: Arc<DashMap<Uuid, WorkerMetrics>>,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(RwLock::new(DashMap::new())),
            metrics: Arc::new(DashMap::new()),
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
        self.metrics.remove(worker_id);
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
