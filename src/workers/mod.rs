pub mod traits;

use std::{collections::HashSet, sync::Arc};

use dashmap::DashMap;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use crate::{
    engine::Outbound,
    function::FunctionHandler,
    invocation::{Invocation, InvocationHandler},
    protocol::{ErrorBody, Message},
    trigger::TriggerRegistrator,
};

pub trait WorkerTrait:
    Clone + Send + Sync + TriggerRegistrator + InvocationHandler + FunctionHandler
{
    fn id(&self) -> &Uuid;
    fn channel(&self) -> &mpsc::Sender<Outbound>;
    fn invocations(&self) -> &Arc<RwLock<DashMap<Uuid, Invocation>>>;

    async fn include_function_path(&self, function_path: &str);
    async fn add_invocation(&self, invocation: Invocation);
    async fn halt_invocation(&self, invocation_id: &Uuid);
}

pub struct WorkerRegistry<W: WorkerTrait> {
    pub workers: Arc<RwLock<DashMap<Uuid, W>>>,
}

impl<W: WorkerTrait> Default for WorkerRegistry<W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: WorkerTrait> WorkerRegistry<W> {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(RwLock::new(DashMap::new())),
        }
    }
    pub async fn get_worker(&self, id: &Uuid) -> Option<W> {
        self.workers.read().await.get(id).map(|w| w.value().clone())
    }

    pub async fn register_worker(&self, worker: W) {
        self.workers.write().await.insert(*worker.id(), worker);
    }

    pub async fn unregister_worker(&self, worker_id: &Uuid) {
        tracing::info!("Unregistering worker: {}", worker_id);
        self.workers.write().await.remove(worker_id);
    }
}

#[derive(Clone)]
pub struct Worker {
    pub id: Uuid,
    pub channel: mpsc::Sender<Outbound>,
    pub invocations: Arc<RwLock<DashMap<Uuid, Invocation>>>,
    pub function_paths: Arc<RwLock<HashSet<String>>>,
}
impl Worker {
    pub fn new(channel: mpsc::Sender<Outbound>) -> Self {
        Self {
            id: Uuid::new_v4(),
            channel,
            invocations: Arc::new(RwLock::new(DashMap::new())),
            function_paths: Arc::new(RwLock::new(HashSet::new())),
        }
    }
}

impl WorkerTrait for Worker {
    fn id(&self) -> &Uuid {
        &self.id
    }
    fn channel(&self) -> &mpsc::Sender<Outbound> {
        &self.channel
    }
    fn invocations(&self) -> &Arc<RwLock<DashMap<Uuid, Invocation>>> {
        &self.invocations
    }
    async fn include_function_path(&self, function_path: &str) {
        self.function_paths
            .write()
            .await
            .insert(function_path.to_owned());
    }

    async fn add_invocation(&self, invocation: Invocation) {
        self.invocations
            .write()
            .await
            .insert(invocation.invocation_id, invocation);
    }

    async fn halt_invocation(&self, invocation_id: &Uuid) {
        self.invocations.write().await.remove(invocation_id);
        self.channel
            .send(Outbound::Protocol(Message::InvocationResult {
                invocation_id: *invocation_id,
                function_path: "".to_string(), // we don't need the function path here because the invocation is stopped
                result: None,
                error: Some(ErrorBody {
                    code: "invocation_stopped".to_string(),
                    message: "Invocation stopped".to_string(),
                }),
            }))
            .await
            .unwrap();
    }
}
