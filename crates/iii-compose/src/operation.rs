// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Observable operations for long Compose mutations.
//!
//! Every progress event is a call of [`PROGRESS_FUNCTION_ID`], a private
//! function whose handler returns the event. Clients observe it before
//! submitting an add, update, or remove, with filters on `input.operation_id`
//! and `input.terminal`, and receive structured mutation transitions. A
//! compact snapshot remains available for reconnect/recovery only; normal
//! progress delivery never polls.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::Instant,
};

use iii_sdk::{
    CallAction, Error, IIIClient, RegisterFunction, protocol::CallRequest, runtime::FunctionRef,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, watch};

/// The function every progress event is a call of. Observers filter on
/// `input.operation_id` to follow one operation and on `input.terminal` for
/// its final success or failure.
pub const PROGRESS_FUNCTION_ID: &str = "compose::on-operation-progress";

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

#[derive(Clone)]
pub struct ProgressEmitter {
    client: IIIClient,
    source: FunctionRef,
}

impl ProgressEmitter {
    pub fn register(client: &IIIClient) -> Self {
        // An event source: the handler returns the event it is called with,
        // and only this daemon calls it. Each call is an ordinary engine
        // trace whether or not anything observes it.
        let source = client.register_function(
            PROGRESS_FUNCTION_ID,
            RegisterFunction::new(|event: ProgressEvent| Ok::<_, Error>(event))
                .description(
                    "Structured progress of Compose dependency-tree operations. Observe it \
                     with filters on input.operation_id and input.terminal.",
                )
                .private(),
        );
        Self {
            client: client.clone(),
            source,
        }
    }

    async fn publish(&self, event: &ProgressEvent) {
        // An in-process call runs inline, so its observers see events in
        // order. A call through the engine is fire-and-forget: a slow or
        // unreachable engine never holds up the operation.
        let action = (!self.source.local_dispatch_active()).then_some(CallAction::Void);
        let request = CallRequest {
            function_id: PROGRESS_FUNCTION_ID.into(),
            payload: serde_json::to_value(event).unwrap_or_default(),
            action,
            timeout_ms: Some(5_000),
        };
        let _ = self.client.call(request).await;
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
    warnings: BTreeSet<String>,
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
                warnings: BTreeSet::new(),
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
        self.cancel.send_replace(true);
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

    pub(crate) async fn warn_once(&self, key: String, container: &str, detail: String) {
        if !self.state.write().await.warnings.insert(key) {
            return;
        }
        crate::report::daemon_line(&format!("warning: {detail}"), true);
        self.emit(Some(container), "warning", detail).await;
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

    #[tokio::test]
    async fn cancellation_is_latched_before_a_receiver_subscribes() {
        let client = IIIClient::new("ws://127.0.0.1:1/ws");
        let manager = OperationManager::new(&client);
        let operation = manager.create(1).await;
        operation.cancel();
        assert!(operation.is_cancelled());
        assert!(*operation.cancellation().borrow());
        client.shutdown_async().await;
    }

    #[tokio::test]
    async fn alias_warnings_are_emitted_once_per_operation_even_with_concurrent_installs() {
        let emitter = ProgressEmitter::register(&IIIClient::new("ws://127.0.0.1:1/ws"));
        let operation = Operation::new("compose:alias-warning".into(), 1, emitter.clone());
        tokio::join!(
            crate::registry::warn_alias("shell", "console", Some("shell"), Some(&operation)),
            crate::registry::warn_alias(
                "shell",
                "api.workers.iii.dev/console",
                Some("shell"),
                Some(&operation)
            ),
        );
        let snapshot = operation.snapshot().await;
        assert_eq!(snapshot.last_sequence, 1);
        let event = snapshot.last_event.unwrap();
        assert_eq!(event.phase, "warning");
        assert_eq!(event.container.as_deref(), Some("shell"));
        assert!(event.detail.contains("'console' is an alias of 'shell'"));
        assert!(!event.terminal);
        let next = Operation::new("compose:next-alias-warning".into(), 1, emitter);
        crate::registry::warn_alias("shell", "console", Some("shell"), Some(&next)).await;
        assert_eq!(next.snapshot().await.last_sequence, 1);
    }

    #[test]
    fn lifecycle_suffixes_resolve_to_the_root_operation() {
        let client = IIIClient::new("ws://127.0.0.1:1/ws");
        let emitter = ProgressEmitter::register(&client);
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
    fn observer_filters_select_one_operation_and_its_terminal_event() {
        let filter = iii_sdk::Filter::parse(&serde_json::json!({
            "input.operation_id": { "$eq": "compose:test" },
            "input.terminal": { "$eq": true },
        }))
        .expect("filters should parse");

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
        let other = ProgressEvent {
            operation_id: "compose:other".into(),
            ..terminal.clone()
        };

        let call = |event: &ProgressEvent| serde_json::json!({ "input": event, "output": event });
        assert!(!filter.matches_value(&call(&progress)));
        assert!(filter.matches_value(&call(&terminal)));
        assert!(!filter.matches_value(&call(&other)));
    }
}
