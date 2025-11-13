pub mod traits;

use std::{collections::HashSet, sync::Arc};

use dashmap::DashMap;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use crate::{Outbound, invocation::Invocation};

#[derive(Default)]
pub struct WorkerRegistry {
    pub workers: DashMap<Uuid, Worker>,
}
impl WorkerRegistry {
    pub fn new() -> Self {
        Self {
            workers: DashMap::new(),
        }
    }
    pub fn get_worker(&self, id: &Uuid) -> Option<Worker> {
        self.workers.get(id).map(|w| w.value().clone())
    }

    pub fn register_worker(&self, worker: Worker) {
        self.workers.insert(worker.id, worker);
    }

    pub fn unregister_worker(&self, worker_id: &Uuid) {
        tracing::info!("Unregistering worker: {}", worker_id);
        self.workers.remove(worker_id);
    }

    pub async fn register_function_path(&self, worker_id: &Uuid, function_path: &String) {
        if let Some(worker) = self.workers.get_mut(worker_id) {
            worker
                .function_paths
                .write()
                .await
                .insert(function_path.clone());
        }
    }
}

#[derive(Clone)]
pub struct Worker {
    pub id: Uuid,
    pub channel: mpsc::Sender<Outbound>,
    pub invocations: Arc<DashMap<Uuid, Invocation>>,
    pub function_paths: Arc<RwLock<HashSet<String>>>,
}

impl Worker {
    pub fn new(channel: mpsc::Sender<Outbound>) -> Self {
        let id = Uuid::new_v4();
        Self {
            id,
            channel,
            invocations: Arc::new(DashMap::new()),
            function_paths: Arc::new(RwLock::new(HashSet::new())),
        }
    }
    pub async fn include_function_path(&self, function_path: &String) {
        self.function_paths
            .write()
            .await
            .insert(function_path.clone());
    }
}
