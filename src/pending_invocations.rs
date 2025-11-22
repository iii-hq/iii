use std::{collections::HashSet, sync::Arc};

use dashmap::DashMap;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Default)]
pub struct PendingInvocations {
    invocations: Arc<RwLock<DashMap<Uuid, Uuid>>>,
}

impl PendingInvocations {
    pub fn new() -> Self {
        Self {
            invocations: Arc::new(RwLock::new(DashMap::new())),
        }
    }

    pub async fn insert(&self, invocation_id: Uuid, worker_id: Uuid) {
        self.invocations
            .write()
            .await
            .insert(invocation_id, worker_id);
    }

    pub async fn remove(&self, invocation_id: &Uuid) -> Option<Uuid> {
        self.invocations
            .write()
            .await
            .remove(invocation_id)
            .map(|(_, worker_id)| worker_id)
    }

    pub async fn invocations_for_worker(&self, worker_id: &Uuid) -> HashSet<Uuid> {
        let lock = self.invocations.read().await;
        lock.iter()
            .filter_map(|entry| {
                if entry.value() == worker_id {
                    Some(*entry.key())
                } else {
                    None
                }
            })
            .collect()
    }
}
