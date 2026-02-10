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

use crate::{engine::Outbound, modules::observability::metrics::get_engine_metrics};

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
            function_ids: Arc::new(RwLock::new(HashSet::new())),
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
            function_ids: Arc::new(RwLock::new(HashSet::new())),
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
        self.function_ids.read().await.len()
    }

    pub async fn invocation_count(&self) -> usize {
        self.invocations.read().await.len()
    }

    pub async fn get_function_ids(&self) -> Vec<String> {
        self.function_ids.read().await.iter().cloned().collect()
    }
    pub async fn include_function_id(&self, function_id: &str) {
        self.function_ids
            .write()
            .await
            .insert(function_id.to_owned());
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
    use super::*;
    use crate::engine::Outbound;

    #[test]
    fn test_worker_registry_new() {
        let registry = WorkerRegistry::new();
        assert_eq!(registry.workers.len(), 0);
    }

    #[test]
    fn test_register_worker() {
        let registry = WorkerRegistry::new();
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        registry.register_worker(worker.clone());
        assert_eq!(registry.workers.len(), 1);
        assert!(registry.workers.contains_key(&worker.id));
    }

    #[test]
    fn test_get_worker() {
        let registry = WorkerRegistry::new();
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        registry.register_worker(worker.clone());
        let retrieved = registry.get_worker(&worker.id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, worker.id);
    }

    #[test]
    fn test_get_worker_nonexistent() {
        let registry = WorkerRegistry::new();
        let worker_id = Uuid::new_v4();
        let retrieved = registry.get_worker(&worker_id);
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_unregister_worker() {
        let registry = WorkerRegistry::new();
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        registry.register_worker(worker.clone());
        assert_eq!(registry.workers.len(), 1);

        registry.unregister_worker(&worker.id);
        assert_eq!(registry.workers.len(), 0);
        assert!(registry.get_worker(&worker.id).is_none());
    }

    #[test]
    fn test_list_workers() {
        let registry = WorkerRegistry::new();
        let (_tx1, _rx1) = mpsc::channel::<Outbound>(10);
        let (_tx2, _rx2) = mpsc::channel::<Outbound>(10);
        let worker1 = Worker::new(_tx1);
        let worker2 = Worker::new(_tx2);

        registry.register_worker(worker1.clone());
        registry.register_worker(worker2.clone());

        let workers = registry.list_workers();
        assert_eq!(workers.len(), 2);
        let ids: Vec<Uuid> = workers.iter().map(|w| w.id).collect();
        assert!(ids.contains(&worker1.id));
        assert!(ids.contains(&worker2.id));
    }

    #[test]
    fn test_update_worker_metadata() {
        let registry = WorkerRegistry::new();
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        registry.register_worker(worker.clone());
        registry.update_worker_metadata(
            &worker.id,
            "node".to_string(),
            Some("18.0.0".to_string()),
            Some("worker-1".to_string()),
            Some("linux".to_string()),
        );

        let updated = registry.get_worker(&worker.id).unwrap();
        assert_eq!(updated.runtime, Some("node".to_string()));
        assert_eq!(updated.version, Some("18.0.0".to_string()));
        assert_eq!(updated.name, Some("worker-1".to_string()));
        assert_eq!(updated.os, Some("linux".to_string()));
    }

    #[test]
    fn test_update_worker_metadata_partial() {
        let registry = WorkerRegistry::new();
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        registry.register_worker(worker.clone());
        registry.update_worker_metadata(
            &worker.id,
            "rust".to_string(),
            Some("1.70.0".to_string()),
            None,
            None,
        );

        let updated = registry.get_worker(&worker.id).unwrap();
        assert_eq!(updated.runtime, Some("rust".to_string()));
        assert_eq!(updated.version, Some("1.70.0".to_string()));
    }

    #[test]
    fn test_update_worker_status() {
        let registry = WorkerRegistry::new();
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        registry.register_worker(worker.clone());
        assert_eq!(registry.get_worker(&worker.id).unwrap().status, WorkerStatus::Connected);

        registry.update_worker_status(&worker.id, WorkerStatus::Available);
        assert_eq!(
            registry.get_worker(&worker.id).unwrap().status,
            WorkerStatus::Available
        );

        registry.update_worker_status(&worker.id, WorkerStatus::Busy);
        assert_eq!(registry.get_worker(&worker.id).unwrap().status, WorkerStatus::Busy);
    }

    #[test]
    fn test_worker_status_from_str() {
        assert_eq!(
            WorkerStatus::from_str("available").unwrap(),
            WorkerStatus::Available
        );
        assert_eq!(WorkerStatus::from_str("busy").unwrap(), WorkerStatus::Busy);
        assert_eq!(
            WorkerStatus::from_str("disconnected").unwrap(),
            WorkerStatus::Disconnected
        );
        assert_eq!(
            WorkerStatus::from_str("unknown").unwrap(),
            WorkerStatus::Connected
        );
        assert_eq!(
            WorkerStatus::from_str("CONNECTED").unwrap(),
            WorkerStatus::Connected
        );
    }

    #[test]
    fn test_worker_status_as_str() {
        assert_eq!(WorkerStatus::Connected.as_str(), "connected");
        assert_eq!(WorkerStatus::Available.as_str(), "available");
        assert_eq!(WorkerStatus::Busy.as_str(), "busy");
        assert_eq!(WorkerStatus::Disconnected.as_str(), "disconnected");
    }

    #[test]
    fn test_worker_status_round_trip() {
        for status in [
            WorkerStatus::Connected,
            WorkerStatus::Available,
            WorkerStatus::Busy,
            WorkerStatus::Disconnected,
        ] {
            let status_str = status.as_str();
            let parsed = WorkerStatus::from_str(status_str).unwrap();
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn test_worker_new() {
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        assert_eq!(worker.status, WorkerStatus::Connected);
        assert_eq!(worker.runtime, None);
        assert_eq!(worker.version, None);
        assert_eq!(worker.name, None);
        assert_eq!(worker.os, None);
        assert_eq!(worker.ip_address, None);
    }

    #[test]
    fn test_worker_with_ip() {
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::with_ip(_tx, "192.168.1.1".to_string());

        assert_eq!(worker.ip_address, Some("192.168.1.1".to_string()));
        assert_eq!(worker.status, WorkerStatus::Connected);
    }

    #[tokio::test]
    async fn test_worker_function_count() {
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        assert_eq!(worker.function_count().await, 0);

        worker.include_function_id("test.function1").await;
        assert_eq!(worker.function_count().await, 1);

        worker.include_function_id("test.function2").await;
        assert_eq!(worker.function_count().await, 2);
    }

    #[tokio::test]
    async fn test_worker_invocation_count() {
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        assert_eq!(worker.invocation_count().await, 0);

        let id1 = Uuid::new_v4();
        worker.add_invocation(id1).await;
        assert_eq!(worker.invocation_count().await, 1);

        let id2 = Uuid::new_v4();
        worker.add_invocation(id2).await;
        assert_eq!(worker.invocation_count().await, 2);
    }

    #[tokio::test]
    async fn test_worker_get_function_ids() {
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        assert_eq!(worker.get_function_ids().await.len(), 0);

        worker.include_function_id("test.function1").await;
        worker.include_function_id("test.function2").await;

        let ids = worker.get_function_ids().await;
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"test.function1".to_string()));
        assert!(ids.contains(&"test.function2".to_string()));
    }

    #[tokio::test]
    async fn test_worker_include_function_id() {
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        worker.include_function_id("test.function").await;
        assert!(worker.get_function_ids().await.contains(&"test.function".to_string()));

        worker.include_function_id("test.function").await;
        assert_eq!(worker.function_count().await, 1);
    }

    #[tokio::test]
    async fn test_worker_add_invocation() {
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        let id = Uuid::new_v4();
        worker.add_invocation(id).await;
        assert_eq!(worker.invocation_count().await, 1);

        worker.add_invocation(id).await;
        assert_eq!(worker.invocation_count().await, 1);
    }

    #[tokio::test]
    async fn test_worker_remove_invocation() {
        let (_tx, _rx) = mpsc::channel::<Outbound>(10);
        let worker = Worker::new(_tx);

        let id = Uuid::new_v4();
        worker.add_invocation(id).await;
        assert_eq!(worker.invocation_count().await, 1);

        worker.remove_invocation(&id).await;
        assert_eq!(worker.invocation_count().await, 0);

        worker.remove_invocation(&id).await;
        assert_eq!(worker.invocation_count().await, 0);
    }
}
