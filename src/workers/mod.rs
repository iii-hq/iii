// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod traits;

use std::{collections::HashSet, str::FromStr, sync::Arc};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use opentelemetry::KeyValue;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use serde::{Deserialize, Serialize};

use crate::{engine::Outbound, modules::observability::metrics::get_engine_metrics};

#[derive(Clone, Deserialize, Serialize, Default)]
pub struct WorkerTelemetryMeta {
    pub language: Option<String>,
    pub project_name: Option<String>,
    pub framework: Option<String>,
}

impl std::fmt::Debug for WorkerTelemetryMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerTelemetryMeta")
            .field("language", &self.language)
            .field("project_name", &self.project_name)
            .field("framework", &self.framework)
            .finish()
    }
}

#[derive(Default)]
pub struct WorkerRegistry {
    pub workers: Arc<DashMap<Uuid, Worker>>,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(DashMap::new()),
        }
    }

    pub fn get_worker(&self, id: &Uuid) -> Option<Worker> {
        self.workers.get(id).map(|w| w.value().clone())
    }

    pub fn register_worker(&self, worker: Worker) {
        self.workers.insert(worker.id, worker);
        let count = self.workers.len() as i64;

        // Update metrics
        let metrics = get_engine_metrics();
        metrics.workers_active.record(count, &[]);
        metrics.workers_spawns_total.add(1, &[]);

        // Update accumulator for readable metrics
        let acc = crate::modules::observability::metrics::get_metrics_accumulator();
        acc.workers_spawns
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        crate::modules::telemetry::collector::track_peak_workers(count as u64);
    }

    pub fn unregister_worker(&self, worker_id: &Uuid) {
        tracing::debug!("Unregistering worker: {}", worker_id);
        self.workers.remove(worker_id);
        let count = self.workers.len() as i64;

        // Update metrics
        let metrics = get_engine_metrics();
        metrics.workers_active.record(count, &[]);
        metrics.workers_deaths_total.add(1, &[]);

        // Update accumulator for readable metrics
        let acc = crate::modules::observability::metrics::get_metrics_accumulator();
        acc.workers_deaths
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        telemetry: Option<WorkerTelemetryMeta>,
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
            worker.telemetry = telemetry;
        }
    }

    pub fn update_worker_status(&self, worker_id: &Uuid, status: WorkerStatus) {
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
    pub function_ids: Arc<RwLock<HashSet<String>>>,
    pub external_function_ids: Arc<RwLock<HashSet<String>>>,
    pub invocations: Arc<RwLock<HashSet<Uuid>>>,
    pub runtime: Option<String>,
    pub version: Option<String>,
    pub connected_at: DateTime<Utc>,
    pub name: Option<String>,
    pub os: Option<String>,
    pub ip_address: Option<String>,
    pub status: WorkerStatus,
    pub telemetry: Option<WorkerTelemetryMeta>,
}

impl Worker {
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
            os: None,
            ip_address: None,
            status: WorkerStatus::Connected,
            telemetry: None,
        }
    }

    pub fn with_ip(channel: mpsc::Sender<Outbound>, ip_address: String) -> Self {
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
            os: None,
            ip_address: Some(ip_address),
            status: WorkerStatus::Connected,
            telemetry: None,
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

    fn make_worker() -> Worker {
        let (tx, _rx) = mpsc::channel(8);
        Worker::new(tx)
    }

    #[test]
    fn worker_telemetry_meta_debug_includes_all_fields() {
        let telemetry = WorkerTelemetryMeta {
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
        assert_eq!(WorkerStatus::Connected.as_str(), "connected");
        assert_eq!(WorkerStatus::Available.as_str(), "available");
        assert_eq!(WorkerStatus::Busy.as_str(), "busy");
        assert_eq!(WorkerStatus::Disconnected.as_str(), "disconnected");

        assert_eq!(
            WorkerStatus::from_str("available").unwrap(),
            WorkerStatus::Available
        );
        assert_eq!(WorkerStatus::from_str("BUSY").unwrap(), WorkerStatus::Busy);
        assert_eq!(
            WorkerStatus::from_str("disconnected").unwrap(),
            WorkerStatus::Disconnected
        );
        assert_eq!(
            WorkerStatus::from_str("unknown").unwrap(),
            WorkerStatus::Connected
        );
    }

    #[tokio::test]
    async fn worker_with_ip_tracks_functions_external_ids_and_invocations() {
        let (tx, _rx) = mpsc::channel(8);
        let worker = Worker::with_ip(tx, "127.0.0.1".to_string());
        assert_eq!(worker.ip_address.as_deref(), Some("127.0.0.1"));

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

    #[tokio::test]
    async fn worker_registry_registers_updates_and_unregisters_workers() {
        crate::modules::observability::metrics::ensure_default_meter();

        let registry = WorkerRegistry::new();
        let worker = make_worker();
        let worker_id = worker.id;
        registry.register_worker(worker);

        let telemetry = WorkerTelemetryMeta {
            language: Some("rust".to_string()),
            project_name: Some("iii".to_string()),
            framework: Some("tokio".to_string()),
        };

        registry.update_worker_metadata(
            &worker_id,
            "node".to_string(),
            Some("1.0.0".to_string()),
            Some("worker-a".to_string()),
            Some("linux".to_string()),
            Some(telemetry.clone()),
        );
        registry.update_worker_status(&worker_id, WorkerStatus::Busy);
        registry.update_worker_status(&Uuid::new_v4(), WorkerStatus::Available);

        let stored = registry.get_worker(&worker_id).expect("registered worker");
        assert_eq!(stored.runtime.as_deref(), Some("node"));
        assert_eq!(stored.version.as_deref(), Some("1.0.0"));
        assert_eq!(stored.name.as_deref(), Some("worker-a"));
        assert_eq!(stored.os.as_deref(), Some("linux"));
        assert_eq!(stored.status, WorkerStatus::Busy);
        assert_eq!(
            serde_json::to_value(stored.telemetry).expect("serialize telemetry"),
            json!(telemetry)
        );

        assert_eq!(registry.list_workers().len(), 1);
        registry.unregister_worker(&worker_id);
        assert!(registry.get_worker(&worker_id).is_none());
        assert!(registry.list_workers().is_empty());
    }
}
