pub mod traits;

use std::{collections::HashSet, sync::Arc};

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
}

#[derive(Clone)]
pub struct Worker {
    pub id: Uuid,
    pub channel: mpsc::Sender<Outbound>,
    pub function_paths: Arc<RwLock<HashSet<String>>>,
    pub invocations: Arc<RwLock<HashSet<Uuid>>>,
}

impl Worker {
    pub fn new(channel: mpsc::Sender<Outbound>) -> Self {
        let id = Uuid::new_v4();
        Self {
            id,
            channel,
            invocations: Arc::new(RwLock::new(HashSet::new())),
            function_paths: Arc::new(RwLock::new(HashSet::new())),
        }
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
