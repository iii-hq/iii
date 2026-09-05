// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Observable operations for long Compose mutations.
//!
//! Compose is a trigger provider. Clients bind `compose-operation` before
//! submitting an add, update, or remove and receive structured mutation
//! transitions. A compact snapshot remains available for reconnect/recovery
//! only; normal progress delivery never polls.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock, Weak},
    time::Instant,
};

use iii_sdk::{
    Error, IIIClient, RegisterTriggerType,
    protocol::{TriggerAction, TriggerRequest},
    trigger::{TriggerConfig, TriggerHandler},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, watch};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProgressSubscription {
    /// Operation to follow. Omit to receive every operation from this daemon.
    pub operation_id: Option<String>,
    /// Deliver only the terminal success/failure event for the operation.
    #[serde(default)]
    pub terminal_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ProgressEvent {
    pub sequence: u64,
    pub operation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    pub phase: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    pub elapsed_ms: u64,
    pub terminal: bool,
}
#[derive(Default)]
struct ProgressTriggers {
    bindings: Arc<Mutex<BTreeMap<String, TriggerConfig>>>,
}

#[async_trait::async_trait]
impl TriggerHandler for ProgressTriggers {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), Error> {
        self.bindings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(config.id.clone(), config);
        Ok(())
    }

    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), Error> {
        self.bindings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&config.id);
        Ok(())
    }
}

#[derive(Clone)]
pub struct ProgressEmitter {
    client: IIIClient,
    bindings: Arc<Mutex<BTreeMap<String, TriggerConfig>>>,
}

impl ProgressEmitter {
    pub fn register(client: &IIIClient) -> Self {
        let handler = ProgressTriggers::default();
        let bindings = Arc::clone(&handler.bindings);
        client.register_trigger_type(
            RegisterTriggerType::new(
                "compose-operation",
                "Streams structured progress for Compose dependency-tree operations",
                handler,
            )
            .trigger_request_format::<ProgressSubscription>()
            .call_request_format::<ProgressEvent>(),
        );
        Self {
            client: client.clone(),
            bindings,
        }
    }

    async fn publish(&self, event: &ProgressEvent) {
        let bindings: Vec<TriggerConfig> = self
            .bindings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter(|binding| {
                serde_json::from_value::<ProgressSubscription>(binding.config.clone())
                    .ok()
                    .is_none_or(|config| {
                        config
                            .operation_id
                            .is_none_or(|id| id == event.operation_id)
                            && (!config.terminal_only || event.terminal)
                    })
            })
            .cloned()
            .collect();
        let deliveries = bindings.into_iter().map(|binding| {
            let client = self.client.clone();
            let event = event.clone();
            async move {
                let function_id = binding.function_id.clone();
                let request = TriggerRequest {
                    function_id: function_id.clone(),
                    payload: serde_json::to_value(event).unwrap_or_default(),
                    action: Some(TriggerAction::Void),
                    timeout_ms: Some(5_000),
                };
                let request = match binding.metadata {
                    Some(metadata) => request.metadata(metadata),
                    None => request.into(),
                };
                let request = if let Some(namespace) = binding.namespace {
                    request.namespace(namespace)
                } else {
                    request
                };
                if let Err(error) = client.trigger(request).await {
                    eprintln!(
                        "compose-operation delivery failed for trigger {} ({}): {}",
                        binding.id, function_id, error
                    );
                }
            }
        });
        futures::future::join_all(deliveries).await;
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OperationSnapshot {
    pub operation_id: String,
    pub status: OperationStatus,
    pub phase: String,
    pub requested: usize,
    pub total: usize,
    pub completed: usize,
    pub last_sequence: u64,
    pub last_event: Option<ProgressEvent>,
}
fn active_operations() -> &'static Mutex<BTreeMap<String, Weak<Operation>>> {
    static ACTIVE: OnceLock<Mutex<BTreeMap<String, Weak<Operation>>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn active(id: &str) -> Option<Arc<Operation>> {
    let operations = active_operations()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Some(operation) = operations.get(id).and_then(Weak::upgrade) {
        return Some(operation);
    }
    // Reconciliation appends descriptive child suffixes (`-up`,
    // `-restart-<key>`). Choose the longest active prefix so those lifecycle
    // events remain attached to the caller-visible root operation.
    operations
        .iter()
        .filter(|(root, _)| id.starts_with(root.as_str()))
        .max_by_key(|(root, _)| root.len())
        .and_then(|(_, operation)| Weak::upgrade(operation))
}

struct State {
    status: OperationStatus,
    phase: String,
    requested: usize,
    total: usize,
    completed: usize,
    sequence: u64,
    last_event: Option<ProgressEvent>,
}

pub struct Operation {
    id: String,
    began: Instant,
    state: RwLock<State>,
    cancel: watch::Sender<bool>,
    emitter: ProgressEmitter,
}

impl Operation {
    fn new(id: String, requested: usize, emitter: ProgressEmitter) -> Arc<Self> {
        let (cancel, _) = watch::channel(false);
        Arc::new(Self {
            id,
            began: Instant::now(),
            state: RwLock::new(State {
                status: OperationStatus::Running,
                phase: "accepted".into(),
                requested,
                total: 0,
                completed: 0,
                sequence: 0,
                last_event: None,
            }),
            cancel,
            emitter,
        })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn cancellation(&self) -> watch::Receiver<bool> {
        self.cancel.subscribe()
    }
    /// Reports whether cancellation has been requested for this operation.
    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }
    pub fn cancel(&self) {
        let _ = self.cancel.send(true);
    }
    pub async fn emit_tree(&self, container: &str, depth: usize, detail: impl Into<String>) {
        self.emit_progress(
            Some(container),
            "waiting",
            detail,
            None,
            Some(depth as u64),
            false,
        )
        .await;
    }

    async fn emit_progress(
        &self,
        container: Option<&str>,
        phase: &str,
        detail: impl Into<String>,
        current: Option<u64>,
        total: Option<u64>,
        terminal: bool,
    ) {
        let event = {
            let mut state = self.state.write().await;
            state.phase = phase.into();
            state.sequence += 1;
            let event = ProgressEvent {
                sequence: state.sequence,
                operation_id: self.id.clone(),
                container: container.map(str::to_string),
                phase: phase.into(),
                detail: detail.into(),
                current,
                total,
                elapsed_ms: self.began.elapsed().as_millis() as u64,
                terminal,
            };
            state.last_event = Some(event.clone());
            event
        };
        self.emitter.publish(&event).await;
    }
    pub async fn emit(&self, container: Option<&str>, phase: &str, detail: impl Into<String>) {
        self.emit_progress(container, phase, detail, None, None, false)
            .await;
    }
    pub async fn plan(&self, total: usize) {
        self.state.write().await.total = total;
    }
    pub async fn completed_one(&self) {
        self.state.write().await.completed += 1;
    }
    pub async fn finish(&self, status: OperationStatus, detail: impl Into<String>) {
        let event = {
            let mut state = self.state.write().await;
            state.status = status;
            state.phase = "complete".into();
            state.sequence += 1;
            let event = ProgressEvent {
                sequence: state.sequence,
                operation_id: self.id.clone(),
                container: None,
                phase: "complete".into(),
                detail: detail.into(),
                current: Some(state.completed as u64),
                total: Some(state.total as u64),
                elapsed_ms: self.began.elapsed().as_millis() as u64,
                terminal: true,
            };
            state.last_event = Some(event.clone());
            event
        };
        self.emitter.publish(&event).await;
    }
    pub async fn snapshot(&self) -> OperationSnapshot {
        let s = self.state.read().await;
        OperationSnapshot {
            operation_id: self.id.clone(),
            status: s.status,
            phase: s.phase.clone(),
            requested: s.requested,
            total: s.total,
            completed: s.completed,
            last_sequence: s.sequence,
            last_event: s.last_event.clone(),
        }
    }
}

pub struct OperationManager {
    operations: RwLock<BTreeMap<String, Arc<Operation>>>,
    emitter: ProgressEmitter,
}

/// The caller-selected operation id is already registered in this daemon.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("compose operation '{operation_id}' already exists")]
pub struct OperationIdAlreadyExists {
    /// The id that is still owned by the first operation.
    pub operation_id: String,
}

impl OperationManager {
    pub fn new(client: &IIIClient) -> Self {
        Self {
            operations: RwLock::new(BTreeMap::new()),
            emitter: ProgressEmitter::register(client),
        }
    }
    /// Creates an operation with a generated unique id.
    pub async fn create(&self, requested: usize) -> Arc<Operation> {
        loop {
            let id = format!("compose:{}", uuid::Uuid::new_v4());
            if let Ok(operation) = self.create_with_id(id, requested).await {
                return operation;
            }
        }
    }
    /// Creates an operation with a caller-selected id.
    ///
    /// # Errors
    ///
    /// Returns [`OperationIdAlreadyExists`] without replacing the first
    /// operation when `id` is already registered.
    pub async fn create_with_id(
        &self,
        id: String,
        requested: usize,
    ) -> std::result::Result<Arc<Operation>, OperationIdAlreadyExists> {
        let mut operations = self.operations.write().await;
        if operations.contains_key(&id) {
            return Err(OperationIdAlreadyExists { operation_id: id });
        }

        let op = Operation::new(id.clone(), requested, self.emitter.clone());
        operations.insert(id.clone(), Arc::clone(&op));
        active_operations()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, Arc::downgrade(&op));
        Ok(op)
    }
    pub async fn get(&self, id: &str) -> Option<Arc<Operation>> {
        self.operations.read().await.get(id).cloned()
    }
    pub async fn cancel(&self, id: &str) -> bool {
        let Some(op) = self.get(id).await else {
            return false;
        };
        op.cancel();
        true
    }
    pub async fn cancel_all(&self) {
        for operation in self.operations.read().await.values() {
            if operation.snapshot().await.status == OperationStatus::Running {
                operation.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_suffixes_resolve_to_the_root_operation() {
        let client = IIIClient::new("ws://127.0.0.1:1/ws");
        let emitter = ProgressEmitter {
            client,
            bindings: Arc::new(Mutex::new(BTreeMap::new())),
        };
        let root = Operation::new("compose:test-root".into(), 1, emitter);
        active_operations()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(root.id().into(), Arc::downgrade(&root));

        assert!(Arc::ptr_eq(&active("compose:test-root-up").unwrap(), &root));
        assert!(Arc::ptr_eq(
            &active("compose:test-root-restart-worker").unwrap(),
            &root
        ));
    }

    #[tokio::test]
    async fn duplicate_id_keeps_the_first_operation() {
        let client = IIIClient::new("ws://127.0.0.1:1/ws");
        let manager = OperationManager::new(&client);
        let id = format!("compose:test-{}", uuid::Uuid::new_v4());

        let first = manager
            .create_with_id(id.clone(), 1)
            .await
            .expect("first operation should be created");
        let error = match manager.create_with_id(id.clone(), 2).await {
            Ok(_) => panic!("duplicate operation should be rejected"),
            Err(error) => error,
        };

        assert_eq!(error.operation_id, id);
        assert!(Arc::ptr_eq(
            &manager
                .get(&id)
                .await
                .expect("first operation should remain"),
            &first
        ));
    }

    #[test]
    fn terminal_only_subscription_filters_progress_events() {
        let config: ProgressSubscription = serde_json::from_value(serde_json::json!({
            "operation_id": "compose:test",
            "terminal_only": true,
        }))
        .expect("subscription should deserialize");

        let progress = ProgressEvent {
            sequence: 1,
            operation_id: "compose:test".into(),
            container: None,
            phase: "waiting".into(),
            detail: "resolving".into(),
            current: None,
            total: None,
            elapsed_ms: 0,
            terminal: false,
        };
        let terminal = ProgressEvent {
            terminal: true,
            sequence: 2,
            phase: "complete".into(),
            detail: "done".into(),
            ..progress.clone()
        };

        let matches = |event: &ProgressEvent| {
            config
                .operation_id
                .as_ref()
                .is_none_or(|id| id == &event.operation_id)
                && (!config.terminal_only || event.terminal)
        };
        assert!(!matches(&progress));
        assert!(matches(&terminal));
    }

    #[test]
    fn subscription_defaults_to_all_events() {
        let config: ProgressSubscription = serde_json::from_value(serde_json::json!({
            "operation_id": "compose:test"
        }))
        .expect("subscription should deserialize");
        assert!(!config.terminal_only);
    }
}
