use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{Invocation, Outbound};

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
}

#[derive(Clone)]
pub struct Worker {
    pub id: Uuid,
    pub channel: mpsc::Sender<Outbound>,
    pub invocations: Arc<DashMap<Uuid, Invocation>>,
}
