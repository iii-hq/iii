// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Custom `worker` trigger type. Bridges `core::events::WorkerOpEvent`
//! signals (today silently dropped by `NullSink` in the daemon) into SDK
//! `iii.trigger(fn_id, payload, Void)` fan-out so any other worker can
//! subscribe to the lifecycle of `worker::add` / `worker::remove` /
//! `worker::update` / `worker::start` / `worker::stop` / `worker::clear`
//! via `iii.register_trigger("worker", config, fn_id)`.
//!
//! Subscribers narrow which events they receive via [`WorkerTriggerConfig`]:
//!
//! - `operations`: subset of [`WorkerOperation`] (None = all).
//! - `stages`: subset of [`WorkerStage`] (None = all).
//! - `workers`: subset of worker names (None = all).
//!
//! Empty vec (`Some(vec![])`) is treated like `None` so the schema
//! cannot accidentally encode "subscribes to nothing".

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use iii_sdk::{III, IIIError, TriggerAction, TriggerConfig, TriggerHandler, TriggerRequest};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::add::CallerMode;
use crate::core::events::{EventSink, WorkerOpEvent};
use crate::core::types::WorkerSource;

/// Every worker lifecycle operation a subscriber can watch.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorkerOperation {
    Add,
    Remove,
    Update,
    Start,
    Stop,
    Clear,
}

/// Lifecycle stage attached to every event. Each [`WorkerOperation`]
/// emits a fixed sequence.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStage {
    Started,
    Downloading,
    Downloaded,
    Removing,
    Updating,
    Starting,
    Stopping,
    Clearing,
    Done,
    Failed,
}

/// Wire-format mirror of [`CallerMode`] so `WorkerCallRequest` can carry
/// it without forcing serde derives onto the internal control-flow enum
/// in `core::add`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallerModeWire {
    Cli,
    Trigger,
}

impl From<CallerMode> for CallerModeWire {
    fn from(mode: CallerMode) -> Self {
        match mode {
            CallerMode::Cli => Self::Cli,
            CallerMode::Trigger => Self::Trigger,
        }
    }
}

/// Tag for the source variant a `worker::add`/`update` operation came from.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerSourceKind {
    Registry,
    Oci,
    Local,
}

/// Where a worker came from. Only populated on `add`/`update` events.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WorkerSourceInfo {
    pub kind: WorkerSourceKind,
    /// Source-shaped identifier: registry slug (optionally `name@version`),
    /// full OCI reference, or local filesystem path.
    #[serde(rename = "ref")]
    pub reference: String,
}

impl From<&WorkerSource> for WorkerSourceInfo {
    fn from(source: &WorkerSource) -> Self {
        match source {
            WorkerSource::Registry { name, version } => {
                let reference = match version {
                    Some(v) => format!("{name}@{v}"),
                    None => name.clone(),
                };
                Self {
                    kind: WorkerSourceKind::Registry,
                    reference,
                }
            }
            WorkerSource::Oci { reference } => Self {
                kind: WorkerSourceKind::Oci,
                reference: reference.clone(),
            },
            WorkerSource::Local { path } => Self {
                kind: WorkerSourceKind::Local,
                reference: path.display().to_string(),
            },
        }
    }
}

/// Structured failure information, populated on the `failed` stage.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WorkerErrorPayload {
    /// `Wxxx` code lifted from `WorkerOpError` when available, otherwise
    /// `W900` for an opaque internal failure.
    pub code: String,
    pub message: String,
}

/// Event payload subscribers receive (`call_request_format`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerCallRequest {
    pub operation: WorkerOperation,
    /// Canonical worker name (or the source label until a name is resolved).
    pub worker: String,
    pub stage: WorkerStage,
    /// Unix timestamp in milliseconds.
    pub timestamp_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<WorkerSourceInfo>,
    /// Worker version. Populated on terminal `add`/`update` stages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// `installed` / `already_current` / `repaired` / `replaced` for
    /// `add`/`update`; absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub caller_mode: CallerModeWire,
    /// Pull progress (0.0–1.0) for the `downloading` stage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
    /// Populated when `stage == failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WorkerErrorPayload>,
}

/// Subscriber filters (`trigger_request_format`). All fields are
/// optional; semantics are AND across fields, OR within a vector.
/// `Some(vec![])` is treated identically to `None`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct WorkerTriggerConfig {
    /// Subset of operations to subscribe to. `None` = all operations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<WorkerOperation>>,
    /// Subset of stages to subscribe to. `None` = all stages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stages: Option<Vec<WorkerStage>>,
    /// Subset of worker names (exact match). `None` = all workers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workers: Option<Vec<String>>,
}

/// Evaluate a subscriber's filter against an event.
pub fn matches(req: &WorkerCallRequest, cfg: &WorkerTriggerConfig) -> bool {
    let op_ok = cfg
        .operations
        .as_deref()
        .is_none_or(|xs| xs.is_empty() || xs.contains(&req.operation));
    let stage_ok = cfg
        .stages
        .as_deref()
        .is_none_or(|xs| xs.is_empty() || xs.contains(&req.stage));
    let worker_ok = cfg
        .workers
        .as_deref()
        .is_none_or(|xs| xs.is_empty() || xs.iter().any(|n| n == &req.worker));
    op_ok && stage_ok && worker_ok
}

/// In-memory record kept by [`WorkerTriggerHandler`] for each registered
/// subscriber. The `Arc<Mutex<…>>` is shared with [`IIIEventSink`] so the
/// emit path sees registrations made on the SDK's tokio executor without
/// any cross-thread synchronization beyond the mutex itself.
#[derive(Debug, Clone)]
pub struct Subscription {
    pub function_id: String,
    pub config: WorkerTriggerConfig,
}

/// Shared subscription map: trigger id → subscription.
pub type Subscriptions = Arc<Mutex<HashMap<String, Subscription>>>;

/// `TriggerHandler` for the `worker` trigger type. Inserts on register,
/// removes on unregister. Rejects malformed `WorkerTriggerConfig`
/// payloads with [`IIIError::Handler`] so the engine reports them back
/// to the caller with `trigger_registration_failed`.
pub struct WorkerTriggerHandler {
    subs: Subscriptions,
}

impl WorkerTriggerHandler {
    pub fn new(subs: Subscriptions) -> Self {
        Self { subs }
    }
}

#[async_trait]
impl TriggerHandler for WorkerTriggerHandler {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), IIIError> {
        let parsed: WorkerTriggerConfig = if config.config.is_null() {
            WorkerTriggerConfig::default()
        } else {
            serde_json::from_value(config.config.clone())
                .map_err(|e| IIIError::Handler(format!("invalid WorkerTriggerConfig: {e}")))?
        };
        let mut subs = self.subs.lock().unwrap_or_else(|e| e.into_inner());
        subs.insert(
            config.id.clone(),
            Subscription {
                function_id: config.function_id.clone(),
                config: parsed,
            },
        );
        Ok(())
    }

    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), IIIError> {
        let mut subs = self.subs.lock().unwrap_or_else(|e| e.into_inner());
        subs.remove(&config.id);
        Ok(())
    }
}

/// `EventSink` implementation that converts `WorkerOpEvent` → typed
/// `WorkerCallRequest` and fans out to every subscriber whose
/// `WorkerTriggerConfig` matches. Uses `iii.trigger(..., Void)` so a
/// slow subscriber cannot stall the operation thread.
///
/// Events are serialized through a single dispatcher mpsc so a chain of
/// `events.emit(...)` calls reaches the wire in the order the orchestrator
/// emitted them. Without this hop each `emit` would `tokio::spawn` its own
/// trigger task, and on a multi-thread runtime the spawned tasks can race
/// — subscribers might see `done` before `downloaded`. The dispatcher task
/// awaits each `iii.trigger` sequentially.
pub struct IIIEventSink {
    subs: Subscriptions,
    caller_mode: CallerMode,
    dispatch_tx: tokio::sync::mpsc::UnboundedSender<(String, serde_json::Value)>,
}

impl IIIEventSink {
    pub fn new(iii: III, subs: Subscriptions, caller_mode: CallerMode) -> Self {
        let (dispatch_tx, mut dispatch_rx) =
            tokio::sync::mpsc::unbounded_channel::<(String, serde_json::Value)>();
        // Spawn the dispatcher only when a tokio runtime is available.
        // Callers outside a runtime (rare; mostly defensive for tests
        // that construct the sink without ever emitting) just drop
        // events silently — the channel buffers them until the sink is
        // dropped, which closes the sender and exits the (never-spawned)
        // dispatcher cleanly.
        //
        // The dispatcher owns the only clone of `iii` it needs; the sink
        // itself doesn't keep a handle because `emit` is purely an mpsc
        // send. Keeping a second clone would extend `iii`'s lifetime
        // beyond what's necessary and complicate shutdown.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                while let Some((function_id, payload)) = dispatch_rx.recv().await {
                    let _ = iii
                        .trigger(TriggerRequest {
                            function_id,
                            payload,
                            action: Some(TriggerAction::Void),
                            timeout_ms: None,
                        })
                        .await;
                }
            });
        }
        Self {
            subs,
            caller_mode,
            dispatch_tx,
        }
    }
}

impl EventSink for IIIEventSink {
    fn emit(&self, event: WorkerOpEvent) {
        let Some(req) = event_to_request(event, self.caller_mode) else {
            return;
        };
        let Ok(payload) = serde_json::to_value(&req) else {
            return;
        };

        // Snapshot to avoid holding the subscription lock across the
        // mpsc send boundary or while cloning each payload.
        let subs_snapshot: Vec<Subscription> = {
            let guard = self.subs.lock().unwrap_or_else(|e| e.into_inner());
            guard.values().cloned().collect()
        };

        for sub in subs_snapshot {
            if !matches(&req, &sub.config) {
                continue;
            }
            // Synchronous enqueue preserves emit-order across the whole
            // operation lifecycle (started → downloading → downloaded →
            // done). The dispatcher task drains FIFO.
            let _ = self.dispatch_tx.send((sub.function_id, payload.clone()));
        }
    }
}

/// Convert a [`WorkerOpEvent`] into the typed event payload. Returns
/// `None` when the event's `op` / `stage` string doesn't map to a known
/// enum variant — defensive guard against future event additions that
/// aren't yet covered by the wire format.
fn event_to_request(event: WorkerOpEvent, caller_mode: CallerMode) -> Option<WorkerCallRequest> {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let caller_mode = CallerModeWire::from(caller_mode);
    match event {
        WorkerOpEvent::Started { op, worker } => Some(WorkerCallRequest {
            operation: op_from_str(op)?,
            worker,
            stage: WorkerStage::Started,
            timestamp_ms,
            source: None,
            version: None,
            status: None,
            caller_mode,
            progress: None,
            error: None,
        }),
        WorkerOpEvent::Stage { op, stage, worker } => Some(WorkerCallRequest {
            operation: op_from_str(op)?,
            worker,
            stage: stage_from_str(stage)?,
            timestamp_ms,
            source: None,
            version: None,
            status: None,
            caller_mode,
            progress: None,
            error: None,
        }),
        WorkerOpEvent::PullProgress { worker, fraction } => Some(WorkerCallRequest {
            operation: WorkerOperation::Add,
            worker,
            stage: WorkerStage::Downloading,
            timestamp_ms,
            source: None,
            version: None,
            status: None,
            caller_mode,
            progress: Some(fraction),
            error: None,
        }),
        WorkerOpEvent::Done { op, worker } => Some(WorkerCallRequest {
            operation: op_from_str(op)?,
            worker,
            stage: WorkerStage::Done,
            timestamp_ms,
            source: None,
            version: None,
            status: None,
            caller_mode,
            progress: None,
            error: None,
        }),
        WorkerOpEvent::Failed { op, worker, error } => Some(WorkerCallRequest {
            operation: op_from_str(op)?,
            worker,
            stage: WorkerStage::Failed,
            timestamp_ms,
            source: None,
            version: None,
            status: None,
            caller_mode,
            progress: None,
            error: Some(WorkerErrorPayload {
                code: "W900".to_string(),
                message: error,
            }),
        }),
    }
}

fn op_from_str(op: &str) -> Option<WorkerOperation> {
    match op {
        "add" => Some(WorkerOperation::Add),
        "remove" => Some(WorkerOperation::Remove),
        "update" => Some(WorkerOperation::Update),
        "start" => Some(WorkerOperation::Start),
        "stop" => Some(WorkerOperation::Stop),
        "clear" => Some(WorkerOperation::Clear),
        _ => None,
    }
}

fn stage_from_str(stage: &str) -> Option<WorkerStage> {
    match stage {
        "started" => Some(WorkerStage::Started),
        "downloading" => Some(WorkerStage::Downloading),
        "downloaded" => Some(WorkerStage::Downloaded),
        "removing" => Some(WorkerStage::Removing),
        "updating" => Some(WorkerStage::Updating),
        "starting" => Some(WorkerStage::Starting),
        "stopping" => Some(WorkerStage::Stopping),
        "clearing" => Some(WorkerStage::Clearing),
        "done" => Some(WorkerStage::Done),
        "failed" => Some(WorkerStage::Failed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iii_sdk::{InitOptions, register_worker};
    use serde_json::json;

    fn make_subs() -> Subscriptions {
        Arc::new(Mutex::new(HashMap::new()))
    }

    fn sample_req(op: WorkerOperation, stage: WorkerStage, worker: &str) -> WorkerCallRequest {
        WorkerCallRequest {
            operation: op,
            worker: worker.to_string(),
            stage,
            timestamp_ms: 0,
            source: None,
            version: None,
            status: None,
            caller_mode: CallerModeWire::Trigger,
            progress: None,
            error: None,
        }
    }

    #[test]
    fn empty_config_matches_everything() {
        let cfg = WorkerTriggerConfig::default();
        let req = sample_req(WorkerOperation::Add, WorkerStage::Downloaded, "pdfkit");
        assert!(matches(&req, &cfg));
    }

    #[test]
    fn operation_and_stage_filters_compose_as_and() {
        let cfg = WorkerTriggerConfig {
            operations: Some(vec![WorkerOperation::Add]),
            stages: Some(vec![WorkerStage::Downloaded]),
            workers: None,
        };
        assert!(matches(
            &sample_req(WorkerOperation::Add, WorkerStage::Downloaded, "pdfkit"),
            &cfg
        ));
        assert!(!matches(
            &sample_req(WorkerOperation::Add, WorkerStage::Downloading, "pdfkit"),
            &cfg
        ));
        assert!(!matches(
            &sample_req(WorkerOperation::Update, WorkerStage::Downloaded, "pdfkit"),
            &cfg
        ));
    }

    #[test]
    fn empty_vec_is_treated_as_wildcard() {
        let cfg = WorkerTriggerConfig {
            operations: Some(vec![]),
            stages: Some(vec![]),
            workers: Some(vec![]),
        };
        assert!(matches(
            &sample_req(WorkerOperation::Remove, WorkerStage::Done, "anything"),
            &cfg
        ));
    }

    #[test]
    fn workers_filter_narrows_by_name() {
        let cfg = WorkerTriggerConfig {
            operations: None,
            stages: None,
            workers: Some(vec!["pdfkit".into()]),
        };
        assert!(matches(
            &sample_req(WorkerOperation::Start, WorkerStage::Done, "pdfkit"),
            &cfg
        ));
        assert!(!matches(
            &sample_req(WorkerOperation::Start, WorkerStage::Done, "image-resize"),
            &cfg
        ));
    }

    #[test]
    fn vec_within_field_is_or() {
        let cfg = WorkerTriggerConfig {
            operations: Some(vec![WorkerOperation::Add, WorkerOperation::Update]),
            stages: Some(vec![WorkerStage::Downloaded, WorkerStage::Done]),
            workers: None,
        };
        assert!(matches(
            &sample_req(WorkerOperation::Add, WorkerStage::Downloaded, "p"),
            &cfg
        ));
        assert!(matches(
            &sample_req(WorkerOperation::Update, WorkerStage::Done, "p"),
            &cfg
        ));
        assert!(!matches(
            &sample_req(WorkerOperation::Remove, WorkerStage::Done, "p"),
            &cfg
        ));
    }

    #[tokio::test]
    async fn handler_register_inserts_and_unregister_removes() {
        let subs = make_subs();
        let handler = WorkerTriggerHandler::new(subs.clone());
        let cfg = TriggerConfig {
            id: "t1".into(),
            function_id: "myapp::on_event".into(),
            config: json!({ "operations": ["add"], "stages": ["downloaded"] }),
            metadata: None,
        };
        handler.register_trigger(cfg.clone()).await.unwrap();
        assert_eq!(subs.lock().unwrap().len(), 1);
        let stored = subs.lock().unwrap().get("t1").cloned().unwrap();
        assert_eq!(stored.function_id, "myapp::on_event");
        assert_eq!(
            stored.config.operations.as_deref(),
            Some(&[WorkerOperation::Add][..])
        );
        assert_eq!(
            stored.config.stages.as_deref(),
            Some(&[WorkerStage::Downloaded][..])
        );
        handler.unregister_trigger(cfg).await.unwrap();
        assert!(subs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn handler_accepts_null_config_as_default() {
        let subs = make_subs();
        let handler = WorkerTriggerHandler::new(subs.clone());
        let cfg = TriggerConfig {
            id: "t2".into(),
            function_id: "myapp::all".into(),
            config: serde_json::Value::Null,
            metadata: None,
        };
        handler.register_trigger(cfg).await.unwrap();
        let stored = subs.lock().unwrap().get("t2").cloned().unwrap();
        assert!(stored.config.operations.is_none());
        assert!(stored.config.stages.is_none());
        assert!(stored.config.workers.is_none());
    }

    #[tokio::test]
    async fn handler_rejects_malformed_config_with_handler_error() {
        let subs = make_subs();
        let handler = WorkerTriggerHandler::new(subs);
        let cfg = TriggerConfig {
            id: "t3".into(),
            function_id: "myapp::bad".into(),
            config: json!({ "operations": "add" }), // should be array
            metadata: None,
        };
        let err = handler.register_trigger(cfg).await.unwrap_err();
        assert!(matches!(err, IIIError::Handler(_)));
    }

    #[test]
    fn event_to_request_maps_started_stage_done_failed() {
        let started = event_to_request(
            WorkerOpEvent::Started {
                op: "add",
                worker: "pdfkit".into(),
            },
            CallerMode::Trigger,
        )
        .unwrap();
        assert_eq!(started.operation, WorkerOperation::Add);
        assert_eq!(started.stage, WorkerStage::Started);
        assert_eq!(started.caller_mode, CallerModeWire::Trigger);

        let stage = event_to_request(
            WorkerOpEvent::Stage {
                op: "add",
                stage: "downloading",
                worker: "pdfkit".into(),
            },
            CallerMode::Trigger,
        )
        .unwrap();
        assert_eq!(stage.stage, WorkerStage::Downloading);

        let done = event_to_request(
            WorkerOpEvent::Done {
                op: "remove",
                worker: "x".into(),
            },
            CallerMode::Cli,
        )
        .unwrap();
        assert_eq!(done.operation, WorkerOperation::Remove);
        assert_eq!(done.stage, WorkerStage::Done);
        assert_eq!(done.caller_mode, CallerModeWire::Cli);

        let failed = event_to_request(
            WorkerOpEvent::Failed {
                op: "add",
                worker: "x".into(),
                error: "boom".into(),
            },
            CallerMode::Trigger,
        )
        .unwrap();
        assert_eq!(failed.stage, WorkerStage::Failed);
        assert_eq!(failed.error.as_ref().unwrap().message, "boom");
    }

    #[test]
    fn event_to_request_unknown_op_returns_none() {
        let req = event_to_request(
            WorkerOpEvent::Started {
                op: "unknown",
                worker: "x".into(),
            },
            CallerMode::Trigger,
        );
        assert!(req.is_none());
    }

    #[test]
    fn worker_source_info_from_each_variant() {
        let registry: WorkerSourceInfo = (&WorkerSource::Registry {
            name: "pdfkit".into(),
            version: Some("1.0.0".into()),
        })
            .into();
        assert_eq!(registry.kind, WorkerSourceKind::Registry);
        assert_eq!(registry.reference, "pdfkit@1.0.0");

        let oci: WorkerSourceInfo = (&WorkerSource::Oci {
            reference: "ghcr.io/iii-hq/node:latest".into(),
        })
            .into();
        assert_eq!(oci.kind, WorkerSourceKind::Oci);
        assert_eq!(oci.reference, "ghcr.io/iii-hq/node:latest");

        let local: WorkerSourceInfo = (&WorkerSource::Local {
            path: "./builds/x".into(),
        })
            .into();
        assert_eq!(local.kind, WorkerSourceKind::Local);
        assert_eq!(local.reference, "./builds/x");
    }

    #[test]
    fn worker_call_request_roundtrips_documented_example() {
        let raw = json!({
            "operation": "add",
            "worker": "pdfkit",
            "stage": "downloaded",
            "timestamp_ms": 1700000000000_i64,
            "version": "1.0.0",
            "status": "installed",
            "caller_mode": "trigger"
        });
        let parsed: WorkerCallRequest = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(parsed.operation, WorkerOperation::Add);
        assert_eq!(parsed.stage, WorkerStage::Downloaded);
        assert_eq!(parsed.version.as_deref(), Some("1.0.0"));
        assert_eq!(parsed.status.as_deref(), Some("installed"));
        let back = serde_json::to_value(&parsed).unwrap();
        assert_eq!(back["operation"], "add");
        assert_eq!(back["stage"], "downloaded");
        assert!(back.get("error").is_none(), "absent error must be skipped");
    }

    #[test]
    fn worker_trigger_config_roundtrips_filter_example() {
        let raw = json!({ "operations": ["add"], "stages": ["downloaded"] });
        let parsed: WorkerTriggerConfig = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(
            parsed.operations.as_deref(),
            Some(&[WorkerOperation::Add][..])
        );
        assert_eq!(
            parsed.stages.as_deref(),
            Some(&[WorkerStage::Downloaded][..])
        );
        assert!(parsed.workers.is_none());
        let back = serde_json::to_value(&parsed).unwrap();
        assert_eq!(back, raw);
    }

    #[test]
    fn worker_trigger_config_empty_object_deserializes_as_default() {
        let parsed: WorkerTriggerConfig = serde_json::from_value(json!({})).unwrap();
        assert!(parsed.operations.is_none());
        assert!(parsed.stages.is_none());
        assert!(parsed.workers.is_none());
    }

    #[tokio::test]
    async fn sink_emit_with_no_subscribers_is_noop() {
        // Constructing an III without a real engine is fine; trigger
        // sends route through the outbound channel and are dropped if
        // never connected. We just need emit() to not panic.
        let iii = register_worker("ws://127.0.0.1:1", InitOptions::default());
        let subs = make_subs();
        let sink = IIIEventSink::new(iii.clone(), subs, CallerMode::Trigger);
        sink.emit(WorkerOpEvent::Started {
            op: "add",
            worker: "pdfkit".into(),
        });
        iii.shutdown_async().await;
    }

    #[tokio::test]
    async fn sink_emit_unknown_op_string_is_silently_skipped() {
        let iii = register_worker("ws://127.0.0.1:1", InitOptions::default());
        let subs = make_subs();
        // Insert a wildcard subscription so any matching event would fan
        // out; the unknown op should still short-circuit before that.
        subs.lock().unwrap().insert(
            "t".into(),
            Subscription {
                function_id: "any".into(),
                config: WorkerTriggerConfig::default(),
            },
        );
        let sink = IIIEventSink::new(iii.clone(), subs, CallerMode::Trigger);
        sink.emit(WorkerOpEvent::Started {
            op: "not-a-real-op",
            worker: "x".into(),
        });
        iii.shutdown_async().await;
    }
}
