// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The `compose::*` control surface.
//!
//! Registered in the daemon's own namespace, which is how one machine is
//! reached rather than another:
//!
//! ```text
//! iii call compose::up --namespace dev file=./worker-compose.yaml
//! ```
//!
//! The flag picks the daemon; `file=` picks the project. Nothing else names a
//! project — a name someone chose would be a second identity for the same
//! thing, and one that can be pointed at the wrong file.
//!
//! `list` and `stop` are about the daemon itself and take no file. The rest
//! fall back to `worker-compose.yaml` in the daemon's own directory, and say
//! so when there is none.

use std::{
    collections::BTreeMap, future::Future, panic::AssertUnwindSafe, path::PathBuf, sync::Arc,
};

use futures::FutureExt;

use iii_sdk::{Error, OutputStream, RegisterFunction};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    caller_report::{CallerReport, CallerTerminal, Ending},
    daemon::{Daemon, MutationOutcome},
    edit::WorkerInput,
    error::ComposeError,
    logs::{LogCursor, LogStream, LogsOutcome},
    project::ContainerStatus,
};

/// Payload accepted by every `compose::*` function. All fields optional: the
/// bare call is the common one.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct ComposeRequest {
    /// Which daemon the caller believed they were reaching.
    ///
    /// A guard, never a route: the engine resolves a call by the `--namespace`
    /// flag and never reads the payload, so this cannot select a daemon — it
    /// can only catch having reached the wrong one. Sending it alone is the
    /// mistake it exists to name, since the call then lands wherever the flag
    /// (or its absence) pointed.
    pub namespace: Option<String>,
    /// Which project: its compose file, and the only thing that names one. A
    /// daemon started inside a project may leave it out.
    pub file: Option<String>,
    /// Restrict the operation to one container and what it needs.
    pub container: Option<String>,
    /// Require a matching existing lock and skip package selector resolution.
    /// Used by `compose::up`.
    pub frozen: Option<bool>,
    /// The worker a call is about.
    ///
    /// `compose::update` reads a spec: `name` or `name@version`. Omit both
    /// worker fields to update every declared package worker to latest.
    /// `compose::down`, `compose::remove` and `compose::restart` read a
    /// container key, where it
    /// is the spelling for `container` — an operator naming a worker should not
    /// have to know which of the two words this call wanted. `compose::add`,
    /// `compose::update`, and `compose::remove` keep this field as a
    /// compatibility alias for a single list item.
    pub worker: Option<String>,
    /// Workers to add, update, or remove in one file edit and one reconciliation.
    ///
    /// This is the canonical JSON field for all three operations. The singular
    /// `worker` field remains accepted for clients that used the old contract.
    /// Add also accepts objects with the fields of a compose container.
    pub workers: Option<Vec<WorkerInput>>,
    /// Caller-selected operation id used to bind mutation progress before submission.
    pub operation_id: Option<String>,
    /// `compose::remove`: delete the data of each removed worker's VM too.
    pub prune: Option<bool>,
    /// Last cursor returned for each selected worker.
    pub cursors: Option<BTreeMap<String, LogCursor>>,
    /// Number of recent lines returned when no cursor is supplied.
    pub tail: Option<usize>,
    /// Restrict logs to stdout or stderr.
    pub stream: Option<LogStream>,
    /// Long-poll budget. Bounded by the daemon.
    pub wait_ms: Option<u64>,
    /// Operation id used by operation status and cancellation calls.
    pub progress_operation_id: Option<String>,
    /// Function or file contract requested by `compose::schema`.
    pub function_id: Option<String>,
    /// The stream a mutation draws its progress into. With it, the call answers once the
    /// operation finished, and fails when it failed. `compose::logs` writes its stderr lines
    /// into it.
    pub stderr: Option<OutputStream>,
    /// `compose::logs`: the stream its stdout lines, or all of them without `stderr`, are
    /// written into.
    pub stdout: Option<OutputStream>,
    /// `compose::logs` with streams: keep reading until the caller leaves.
    pub follow: Option<bool>,
    /// What `iii call` says of its terminal, so the progress is drawn for it.
    pub terminal: Option<CallerTerminal>,
}

/// The stream a call draws its progress into, and the terminal it reaches.
struct Drawing {
    stderr: OutputStream,
    terminal: CallerTerminal,
}

/// Request fields used by daemon-wide operations.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct DaemonOptions {
    /// Which daemon the caller believed it reached. This is a guard, not a
    /// route; use the `iii call --namespace` flag to select the daemon.
    namespace: Option<String>,
}

/// Request fields used by project lifecycle operations.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct LifecycleOptions {
    /// Optional daemon guard. Use the `iii call --namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to the project this daemon
    /// serves, else `worker-compose.yaml` in the daemon working directory.
    file: Option<String>,
    /// Restrict the lifecycle operation to one container.
    container: Option<String>,
    /// Alias for `container`.
    worker: Option<String>,
}

/// Request fields used by `compose::up`.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct UpOptions {
    /// Optional daemon guard. Use the `iii call --namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to the project this daemon
    /// serves, else `worker-compose.yaml` in the daemon working directory.
    file: Option<String>,
    /// Restrict startup to one container and its dependencies.
    container: Option<String>,
    /// Require the compose file and existing lock to match. Package selectors
    /// are not resolved, but missing cached artifacts are downloaded from the lock.
    frozen: Option<bool>,
}

/// Request fields used by project read operations.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct ProjectOptions {
    /// Optional daemon guard. Use the `iii call --namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to the project this daemon
    /// serves, else `worker-compose.yaml` in the daemon working directory.
    file: Option<String>,
}

/// Request fields used by batch worker mutations.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct BatchWorkerOptions<T> {
    /// Optional daemon guard. Use the `iii call --namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to the project this daemon
    /// serves, else `worker-compose.yaml` in the daemon working directory.
    file: Option<String>,
    /// Canonical list of workers to mutate together. Add accepts worker names,
    /// `name@version` references, registry references, local paths, or container
    /// objects with scripts, dependencies, and configuration fields. Update
    /// accepts package names or `name@version` references. Remove accepts
    /// declared worker keys.
    workers: Option<Vec<T>>,
    /// Backward-compatible form for mutating one worker.
    worker: Option<String>,
    /// Caller-selected operation id for race-free progress subscription.
    operation_id: Option<String>,
}

/// `compose::restart` accepts either spelling for one container.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct RestartOptions {
    /// Optional daemon guard. Use the `iii call --namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to the project this daemon
    /// serves, else `worker-compose.yaml` in the daemon working directory.
    file: Option<String>,
    /// Container key to restart.
    container: Option<String>,
    /// Alias for `container`.
    worker: Option<String>,
}

/// Request fields used by `compose::build`.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct BuildOptions {
    /// Optional daemon guard. Use the `iii call --namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to the project this daemon
    /// serves, else `worker-compose.yaml` in the daemon working directory.
    file: Option<String>,
    /// Container key to build. Omit to build every container with a non-empty
    /// `scripts.build`.
    worker: Option<String>,
    /// Alias for `worker`.
    container: Option<String>,
    /// Caller-selected operation id for race-free progress subscription.
    operation_id: Option<String>,
}

/// Request fields used by `compose::logs`.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct LogsOptions {
    /// Optional daemon guard. Use the `iii call --namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to the project this daemon
    /// serves, else `worker-compose.yaml` in the daemon working directory.
    file: Option<String>,
    /// Container whose process output should be read. Omit for every worker.
    container: Option<String>,
    /// Alias for `container`.
    worker: Option<String>,
    /// Last cursor returned for each selected worker.
    cursors: Option<BTreeMap<String, LogCursor>>,
    /// Recent line count for the first request. Default 100, maximum 1000.
    tail: Option<usize>,
    /// Restrict output to stdout or stderr.
    stream: Option<LogStream>,
    /// Wait this many milliseconds for new output. Maximum 5000.
    wait_ms: Option<u64>,
}

/// `compose::schema` request. Omit `function_id` to return every contract.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct SchemaRequest {
    /// Optional daemon guard. Use the `iii call --namespace` flag to route.
    namespace: Option<String>,
    /// Function id such as `compose::up`, or the `worker-compose.yaml`
    /// pseudo-id. Omit to return every schema.
    function_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, JsonSchema)]
struct ProjectSummary {
    namespace: String,
    file: PathBuf,
    containers: Vec<ContainerStatus>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, JsonSchema)]
struct ListOutcome {
    daemon: String,
    daemon_namespace: String,
    daemon_pid: u32,
    /// Target triple this daemon resolves `package://` workers for: the
    /// `target` it sends to the registry's `POST /resolve` first. Absent when
    /// this build of iii has no triple for its platform.
    target: Option<String>,
    /// Every target triple this host runs, preferred first. Compose resolves
    /// for a later one only when the registry answers `unsupported_platform`
    /// and lists it as available. Absent with `target`.
    targets: Option<Vec<String>>,
    projects: Vec<ProjectSummary>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, JsonSchema)]
struct StatusOutcome {
    namespace: String,
    file: PathBuf,
    state_dir: PathBuf,
    daemon_pid: u32,
    /// Target triple this daemon resolves `package://` workers for: the
    /// `target` it sends to the registry's `POST /resolve` first. Absent when
    /// this build of iii has no triple for its platform.
    target: Option<String>,
    /// Every target triple this host runs, preferred first. Compose resolves
    /// for a later one only when the registry answers `unsupported_platform`
    /// and lists it as available. Absent with `target`.
    targets: Option<Vec<String>>,
    containers: Vec<ContainerStatus>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, JsonSchema)]
struct PsOutcome {
    daemon_pid: u32,
    processes: Vec<crate::ps::IiiProcess>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, JsonSchema)]
struct ValidateOutcome {
    namespace: String,
    start_order: Vec<String>,
    deferred_packages: Vec<String>,
    /// What does not fail validation but is worth acting on, such as a due migration.
    warnings: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, JsonSchema)]
struct StopOutcome {
    daemon: String,
    daemon_pid: u32,
    stopping: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct CancelOutcome {
    operation_id: String,
    cancelled: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct OperationAcceptedOutcome {
    operation_id: String,
    /// `accepted`. A `compose::add` that drew its progress into `stderr` answers once it
    /// finished: `succeeded`, with its `outcome`.
    status: String,
    requested: usize,
    /// What the finished operation did, when the call waited for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<MutationOutcome>,
}

/// What a `compose::build` that drew its progress into `stderr` answers once every build
/// finished.
#[derive(Debug, Clone, Serialize, JsonSchema)]
struct BuildFinished {
    operation_id: String,
    /// `succeeded`.
    status: String,
    requested: usize,
    /// One result per container, as the finished operation carries them.
    results: Vec<crate::lifecycle::BuildResult>,
}

/// The answers of `compose::build`: accepted at once without a stream, finished with one.
#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(untagged)]
enum BuildAnswer {
    Accepted(OperationAcceptedOutcome),
    Finished(BuildFinished),
}

/// What `compose::logs` answers after writing the lines into the caller's streams.
#[derive(Debug, Clone, Serialize, JsonSchema)]
struct LogsStreamed {
    /// Where each worker's log was read to, to resume from: only past lines that were written.
    cursors: BTreeMap<String, LogCursor>,
}

/// The answers of `compose::logs`: the lines themselves without a stream, the cursors to resume
/// from with one.
#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(untagged)]
enum LogsAnswer {
    Read(LogsOutcome),
    Streamed(LogsStreamed),
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct SchemaEntry {
    function_id: String,
    description: String,
    request: Value,
    response: Value,
    /// Recommended client timeout for the operation.
    default_timeout_ms: u64,
    /// Whether retrying the same payload is safe.
    idempotent: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct SchemaResponse {
    schemas: Vec<SchemaEntry>,
}

/// Registers the control surface. Every id is verbatim — compose never renames
/// a function.
pub fn register(daemon: &Arc<Daemon>) {
    register_matching(daemon, |_| true);
}

/// Registers operations that cannot conflict with the foreground renderer.
pub fn register_controls(daemon: &Arc<Daemon>) {
    register_matching(daemon, |operation| !operation.is_mutation());
}

/// Registers operations that mutate projects after foreground startup ends.
pub fn register_mutations(daemon: &Arc<Daemon>) {
    register_matching(daemon, Operation::is_mutation);
}

fn register_matching(daemon: &Arc<Daemon>, include: impl Fn(Operation) -> bool) {
    let client = daemon.engine().client();

    for &(name, kind) in REGISTERED_OPERATIONS {
        if !include(kind) {
            continue;
        }
        let daemon = Arc::clone(daemon);
        let function = format!("compose::{name}");
        let guard_name = name.to_string();

        let registration = RegisterFunction::new_async(move |request: ComposeRequest| {
            let daemon = Arc::clone(&daemon);
            let guard_name = guard_name.clone();
            async move {
                let Some(op) = reported_op(kind) else {
                    return dispatch(daemon, kind, guard_name, request).await;
                };
                let began = std::time::Instant::now();
                let result = dispatch(daemon, kind, guard_name, request).await;
                report_op(op, &result, began.elapsed());
                result
            }
        });
        client.register_function(function.clone(), describe_op(registration, &function));
    }
}

#[derive(Debug, Clone, Copy)]
enum Operation {
    Up,
    Down,
    List,
    Status,
    Ps,
    Logs,
    Stop,
    Validate,
    Add,
    Remove,
    Restart,
    Update,
    Build,
    Schema,
    Snapshot,
    Cancel,
}

/// The name one call reports under, or `None` for a call that reports
/// nothing.
///
/// Only the mutations that answer with their own outcome are here. Add,
/// remove and update answer as soon as they are accepted and finish in the
/// background, so they report from [`spawn_mutation`] instead. A read-only
/// call reports nothing at all.
fn reported_op(kind: Operation) -> Option<&'static str> {
    match kind {
        Operation::Up => Some("up"),
        Operation::Down => Some("down"),
        Operation::Restart => Some("restart"),
        _ => None,
    }
}

/// Reports one mutation that answered with its outcome.
///
/// The outcome comes out of the answer itself: an `up` whose container failed
/// still returns `Ok`, carrying `status: failed` and the code of the first
/// failure. Nothing but the code is taken; the message belongs to the
/// operator.
///
/// The send is spawned: the daemon that answered is still running, so the
/// caller gets its answer now and the report finishes on its own.
fn report_op(op: &'static str, result: &Result<Value, Error>, elapsed: std::time::Duration) {
    let (outcome, error_kind) = match result {
        Ok(value) => (
            value.get("status").and_then(Value::as_str).unwrap_or("ok"),
            value
                .get("error")
                .and_then(|error| error.get("code"))
                .and_then(Value::as_str),
        ),
        Err(Error::Remote { code, .. }) => (
            // A cancellation is the operator's decision, not a fault.
            if code == "OPERATION_CANCELLED" {
                "cancelled"
            } else {
                "failed"
            },
            Some(code.as_str()),
        ),
        Err(_) => ("failed", None),
    };
    tokio::spawn(crate::telemetry::report(
        crate::telemetry::OP_FINISHED,
        crate::telemetry::op_properties(op, outcome, elapsed, error_kind),
    ));
}

impl Operation {
    fn is_mutation(self) -> bool {
        matches!(
            self,
            Self::Up
                | Self::Down
                | Self::Add
                | Self::Remove
                | Self::Restart
                | Self::Update
                | Self::Build
        )
    }
}

/// The container a `compose::stop` request names, if any. Both fields are
/// checked in turn: a blank `container` must not hide a `worker`, or
/// `{"container": " ", "worker": "api"}` would stop the whole daemon.
fn stop_target(request: &ComposeRequest) -> Option<&str> {
    [request.container.as_deref(), request.worker.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

/// The container a `compose::build` request names, if any: `worker`, else
/// `container`. A blank one must not hide the other, or
/// `{"worker": "", "container": "api"}` would build every container.
fn build_target(request: &ComposeRequest) -> Option<&str> {
    [request.worker.as_deref(), request.container.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

/// What telemetry reports of a finished `compose::build`: cancelled, failed with the code of
/// its first failure, or ok.
fn build_report(result: &crate::error::Result<crate::lifecycle::BuildRun>) -> Result<Value, Error> {
    match result {
        Ok(run) if run.cancelled => Ok(json!({
            "status": "cancelled",
            "error": { "code": "OPERATION_CANCELLED" },
        })),
        Ok(run) => Ok(
            match run.results.iter().find_map(|result| result.error.as_ref()) {
                Some(error) => json!({ "status": "failed", "error": { "code": error.code } }),
                None => json!({ "status": "ok" }),
            },
        ),
        Err(error) => Err(compose_error(error)),
    }
}

/// The container a lifecycle call names: `container`, else `worker`, the
/// spelling an operator who thinks in workers reaches for. `compose::down` once
/// read only `container`, so `worker=api` stopped the whole project.
fn named_container(request: &ComposeRequest) -> Option<&str> {
    request.container.as_deref().or(request.worker.as_deref())
}

/// Canonical list of functions exposed by the compose daemon.
const REGISTERED_OPERATIONS: &[(&str, Operation)] = &[
    ("up", Operation::Up),
    ("down", Operation::Down),
    ("list", Operation::List),
    ("status", Operation::Status),
    ("ps", Operation::Ps),
    ("logs", Operation::Logs),
    ("stop", Operation::Stop),
    ("validate", Operation::Validate),
    ("add", Operation::Add),
    ("remove", Operation::Remove),
    ("restart", Operation::Restart),
    ("update", Operation::Update),
    ("build", Operation::Build),
    ("schema", Operation::Schema),
    ("operation", Operation::Snapshot),
    ("cancel", Operation::Cancel),
];

async fn dispatch(
    daemon: Arc<Daemon>,
    operation: Operation,
    function: String,
    mut request: ComposeRequest,
) -> Result<Value, Error> {
    // Checked before anything runs. A caller who named a daemon and reached a
    // different one meant a different machine, and acting on this one would be
    // the wrong project brought up somewhere nobody was looking.
    if let Some(addressed) = &request.namespace
        && addressed != &daemon.daemon_namespace
    {
        return Err(Error::Remote {
            code: "WRONG_DAEMON".to_string(),
            message: format!(
                "this daemon serves namespace '{}', not '{addressed}'. The namespace is a flag, \
                 not a payload field: iii call compose::{function} --namespace {addressed} …",
                daemon.daemon_namespace
            ),
            stacktrace: None,
        });
    }

    let file = request.file.as_ref().map(std::path::PathBuf::from);
    // The stream the progress is drawn into, when the caller bound one, as `iii call` does.
    let drawing = request.stderr.take().map(|stderr| Drawing {
        stderr,
        terminal: request.terminal.unwrap_or_default(),
    });

    match operation {
        Operation::Up => {
            if let Some(drawing) = drawing {
                let frozen = request.frozen.unwrap_or(false);
                let container = request.container.clone();
                let title = match &container {
                    Some(container) => format!("starting {container}"),
                    None => "starting the project".to_string(),
                };
                let daemon_task = Arc::clone(&daemon);
                return draw_lifecycle(
                    &daemon,
                    drawing,
                    title,
                    UP_DETAILS,
                    move |operation_id| async move {
                        let result = if frozen {
                            daemon_task
                                .up_frozen(file.as_deref(), container.as_deref(), operation_id)
                                .await
                        } else {
                            daemon_task
                                .up(file.as_deref(), container.as_deref(), operation_id)
                                .await
                        }?;
                        Ok(MutationOutcome::from_operations(
                            result.status,
                            result.changed,
                            container.as_deref(),
                            None,
                            None,
                            std::iter::once(&result),
                        ))
                    },
                )
                .await;
            }
            match if request.frozen.unwrap_or(false) {
                daemon
                    .up_frozen(
                        file.as_deref(),
                        request.container.as_deref(),
                        operation_id(),
                    )
                    .await
            } else {
                daemon
                    .up(
                        file.as_deref(),
                        request.container.as_deref(),
                        operation_id(),
                    )
                    .await
            } {
                Ok(result) => Ok(to_value(&MutationOutcome::from_operations(
                    result.status,
                    result.changed,
                    request.container.as_deref(),
                    None,
                    None,
                    std::iter::once(&result),
                ))),
                Err(err) => Err(compose_error(&err)),
            }
        }
        Operation::Add => {
            let workers = requested_workers(request.workers, request.worker);
            if workers.is_empty() {
                return daemon
                    .add(file.as_deref(), &[], operation_id())
                    .await
                    .map(|outcome| to_value(&outcome))
                    .map_err(|err| compose_error(&err));
            }

            let requested = workers.len();
            let (operation, accepted) = admit_mutation(
                &daemon,
                request.operation_id,
                requested,
                format!("adding {requested} requested workers"),
            )
            .await?;
            // Subscribed before the mutation runs, so the caller's report misses none of it.
            let events = drawing.as_ref().map(|_| operation.subscribe());

            let daemon_task = Arc::clone(&daemon);
            let task_operation_id = accepted.operation_id.clone();
            let task_operation = Arc::clone(&operation);
            let mutation = async move {
                task_operation
                    .emit(None, "resolving", "resolving dependency trees")
                    .await;
                daemon_task
                    .add_configured(file.as_deref(), &workers, task_operation_id)
                    .await
            };
            let finished = spawn_mutation(Arc::clone(&operation), mutation, ADD_DETAILS);
            let title = format!("adding {}", count(requested, "worker"));
            draw_mutation(
                drawing.zip(events),
                title,
                &operation,
                finished,
                accepted,
                ADD_DETAILS,
            )
            .await
        }
        Operation::Remove => {
            let prune = request.prune.unwrap_or(false);
            let workers = requested_specs(request.workers, request.worker)?;
            if workers.is_empty() {
                return daemon
                    .remove(file.as_deref(), &[], operation_id(), false)
                    .await
                    .map(|outcome| to_value(&outcome))
                    .map_err(|err| compose_error(&err));
            }
            let requested = workers.len();
            let (operation, accepted) = admit_mutation(
                &daemon,
                request.operation_id,
                requested,
                format!("removing {requested} requested workers"),
            )
            .await?;
            let events = drawing.as_ref().map(|_| operation.subscribe());

            let daemon_task = Arc::clone(&daemon);
            let task_operation_id = accepted.operation_id.clone();
            let task_operation = Arc::clone(&operation);
            let mutation = async move {
                task_operation
                    .emit(
                        None,
                        "removing",
                        format!("removing {}", count(requested, "worker")),
                    )
                    .await;
                daemon_task
                    .remove(file.as_deref(), &workers, task_operation_id, prune)
                    .await
            };
            let finished = spawn_mutation(Arc::clone(&operation), mutation, REMOVE_DETAILS);
            let title = format!("removing {}", count(requested, "worker"));
            draw_mutation(
                drawing.zip(events),
                title,
                &operation,
                finished,
                accepted,
                REMOVE_DETAILS,
            )
            .await
        }
        Operation::Restart => {
            // Either spelling names the same thing here.
            let container = named_container(&request).map(str::to_string);
            if let Some(drawing) = drawing {
                let title = match &container {
                    Some(container) => format!("restarting {container}"),
                    None => "restarting the project".to_string(),
                };
                let daemon_task = Arc::clone(&daemon);
                return draw_lifecycle(
                    &daemon,
                    drawing,
                    title,
                    RESTART_DETAILS,
                    move |id| async move {
                        daemon_task
                            .restart(file.as_deref(), container.as_deref(), id)
                            .await
                    },
                )
                .await;
            }
            match daemon
                .restart(file.as_deref(), container.as_deref(), operation_id())
                .await
            {
                Ok(result) => Ok(to_value(&result)),
                Err(err) => Err(compose_error(&err)),
            }
        }
        Operation::Update => {
            if request.workers.as_ref().is_some_and(Vec::is_empty) {
                return Err(compose_error(&ComposeError::InvalidWorkerSpec {
                    spec: String::new(),
                    reason:
                        "workers cannot be empty. Omit worker and workers to update all packages"
                            .to_string(),
                }));
            }
            let workers = requested_specs(request.workers, request.worker)?;
            let requested = workers.len();
            let selection = if workers.is_empty() {
                "all declared package workers".to_string()
            } else {
                format!("{requested} requested workers")
            };
            let (operation, accepted) = admit_mutation(
                &daemon,
                request.operation_id,
                requested,
                format!("updating {selection}"),
            )
            .await?;
            let events = drawing.as_ref().map(|_| operation.subscribe());
            let title = if workers.is_empty() {
                "updating every package worker".to_string()
            } else {
                format!("updating {}", count(requested, "worker"))
            };

            let daemon_task = Arc::clone(&daemon);
            let task_operation_id = accepted.operation_id.clone();
            let task_operation = Arc::clone(&operation);
            let mutation = async move {
                task_operation
                    .emit(
                        None,
                        "resolving",
                        format!("resolving updates for {selection}"),
                    )
                    .await;
                daemon_task
                    .update(file.as_deref(), &workers, task_operation_id)
                    .await
            };
            let finished = spawn_mutation(Arc::clone(&operation), mutation, UPDATE_DETAILS);
            draw_mutation(
                drawing.zip(events),
                title,
                &operation,
                finished,
                accepted,
                UPDATE_DETAILS,
            )
            .await
        }
        Operation::Build => {
            let worker = build_target(&request).map(str::to_string);
            // The file is the one admitted: a project loaded before the task starts must not
            // change which one a call without `file` builds.
            let file = daemon
                .resolve_file(file.as_deref())
                .await
                .map_err(|err| compose_error(&err))?;
            let requested = daemon
                .build_targets(Some(&file), worker.as_deref())
                .await
                .map_err(|err| compose_error(&err))?
                .len();
            let title = match &worker {
                Some(worker) => format!("building {worker}"),
                None => format!("building {requested} workers"),
            };
            let (operation, accepted) =
                admit_mutation(&daemon, request.operation_id, requested, title.clone()).await?;
            let events = drawing.as_ref().map(|_| operation.subscribe());
            let task_operation_id = accepted.operation_id.clone();
            let task_accepted = accepted.clone();
            let task_daemon = Arc::clone(&daemon);
            let task_file = file.clone();
            let task_operation = Arc::clone(&operation);
            let finished = tokio::spawn(async move {
                #[cfg(test)]
                tests::hold_build(&task_operation_id).await;
                let began = std::time::Instant::now();
                let result = AssertUnwindSafe(task_daemon.build(
                    Some(&task_file),
                    worker.as_deref(),
                    task_operation_id,
                ))
                .catch_unwind()
                .await;
                let result = match result {
                    Ok(result) => result,
                    Err(panic) => {
                        finish_stopped(&task_operation, "build").await;
                        std::panic::resume_unwind(panic)
                    }
                };
                report_op("build", &build_report(&result), began.elapsed());
                finish_build(&task_operation, result, task_accepted).await
            });
            let Some((drawing, events)) = drawing.zip(events) else {
                return Ok(to_value(&accepted));
            };
            // The build's own lines come with the operation's events, before the row that says
            // how the build ended.
            let mut report = CallerReport::start(drawing.stderr, drawing.terminal, title).await;
            report.follow(&operation, events).await;
            let (ending, answer) = finished.await.unwrap_or_else(|error| {
                let message = format!("the build operation stopped: {error}");
                (
                    Ending::Failed(message.clone()),
                    Err(Error::Handler(message)),
                )
            });
            report.finish(&ending).await;
            answer
        }
        Operation::Down => {
            let container = named_container(&request).map(str::to_string);
            if let Some(drawing) = drawing {
                let title = match &container {
                    Some(container) => format!("stopping {container}"),
                    None => "stopping the project".to_string(),
                };
                let daemon_task = Arc::clone(&daemon);
                return draw_lifecycle(
                    &daemon,
                    drawing,
                    title,
                    DOWN_DETAILS,
                    move |id| async move {
                        let result = daemon_task
                            .down(file.as_deref(), container.as_deref(), id)
                            .await?;
                        Ok(MutationOutcome::from_operations(
                            result.status,
                            result.changed,
                            container.as_deref(),
                            None,
                            None,
                            std::iter::once(&result),
                        ))
                    },
                )
                .await;
            }
            match daemon
                .down(file.as_deref(), container.as_deref(), operation_id())
                .await
            {
                Ok(result) => Ok(to_value(&MutationOutcome::from_operations(
                    result.status,
                    result.changed,
                    container.as_deref(),
                    None,
                    None,
                    std::iter::once(&result),
                ))),
                Err(err) => Err(compose_error(&err)),
            }
        }
        // Every project this daemon holds. No id: this is the call an operator
        // makes when they have forgotten what is running.
        Operation::List => {
            let projects = tokio::time::timeout(
                std::time::Duration::from_millis(op_metadata("compose::list").0),
                daemon.list(),
            )
            .await
            .map_err(read_timeout)?
            .map_err(|err| compose_error(&err))?;
            Ok(with_host_targets(json!({
                "daemon": daemon.worker_name,
                // Which machine answered, even when reached by namespace.
                "daemon_namespace": daemon.daemon_namespace,
                "daemon_pid": std::process::id(),
                "projects": projects,
            })))
        }
        Operation::Status => {
            // Loading can adopt/reap recovered children: do not add a new
            // cancellation point to that lifecycle work. Only snapshot
            // acquisition below is safe to drop at the existing read deadline.
            let project = daemon
                .status(file.as_deref())
                .await
                .map_err(|err| compose_error(&err))?;
            let containers = tokio::time::timeout(
                std::time::Duration::from_millis(op_metadata("compose::status").0),
                project.status(),
            )
            .await
            .map_err(read_timeout)?
            .map_err(|err| compose_error(&err))?;
            Ok(with_host_targets(json!({
                "namespace": project.project_namespace,
                "file": project.file_path(),
                // Derived from the file, so nobody can guess it: where this
                // project's state, delivered config and container logs live.
                "state_dir": project.state_dir(),
                // Which process is answering. `--detach` waits on this: another
                // daemon would otherwise answer for the one being launched.
                "daemon_pid": std::process::id(),
                "containers": containers,
            })))
        }
        // What the operating system sees, which is not always what the daemon
        // believes: descendants of a container, and anything still carrying
        // this project's environment after the process compose started is
        // gone.
        Operation::Ps => {
            let wanted = match file {
                Some(file) => vec![file.canonicalize().unwrap_or(file)],
                None => daemon.project_files().await,
            };
            let me = std::process::id();
            let processes: Vec<crate::ps::IiiProcess> =
                tokio::task::spawn_blocking(crate::ps::scan)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|process| {
                        process.pid == me
                            || process.ppid == Some(me)
                            || process
                                .compose_file
                                .as_ref()
                                .is_some_and(|file| wanted.contains(file))
                    })
                    .collect();
            Ok(json!({ "daemon_pid": me, "processes": processes }))
        }
        Operation::Logs if drawing.is_some() || request.stdout.is_some() => {
            let (stderr, terminal) = match drawing {
                Some(drawing) => (Some(drawing.stderr), drawing.terminal),
                None => (None, request.terminal.unwrap_or_default()),
            };
            let streams = LogStreams {
                stdout: request.stdout.take(),
                stderr,
                terminal,
            };
            let query = LogsQuery {
                file,
                container: request.container.or(request.worker),
                cursors: request.cursors.unwrap_or_default(),
                tail: request.tail.unwrap_or(crate::logs::DEFAULT_TAIL_LINES),
                stream: request.stream,
                wait_ms: request.wait_ms.unwrap_or_default(),
                follow: request.follow.unwrap_or(false),
            };
            print_logs(&daemon, query, streams).await
        }
        Operation::Logs => match daemon
            .logs(
                file.as_deref(),
                request.container.as_deref().or(request.worker.as_deref()),
                request.cursors.unwrap_or_default(),
                request.tail.unwrap_or(crate::logs::DEFAULT_TAIL_LINES),
                request.stream,
                request.wait_ms.unwrap_or_default(),
            )
            .await
        {
            Ok(logs) => Ok(to_value(&logs)),
            Err(err) => Err(compose_error(&err)),
        },
        // Answers first, exits after: the serve loop picks the request up and
        // runs the same teardown a signal would. Stop takes the whole daemon
        // down, so a request that names a container is refused, not ignored.
        Operation::Stop => match stop_target(&request) {
            Some(container) => Err(compose_error(&ComposeError::StopTakesNoContainer {
                container: container.to_string(),
            })),
            None => Ok(daemon.request_stop().await),
        },
        // Validation is a question about a file, so it holds nothing: naming a
        // file here must not leave the daemon owning a project, and must not
        // write the durable state that would bind that id to it.
        Operation::Validate => match daemon.validate(file.as_deref()).await {
            Ok(report) => Ok(json!({
                "namespace": report.namespace,
                "start_order": report.start_order,
                "deferred_packages": report.deferred_packages,
                "warnings": report.warnings,
            })),
            Err(err) => Err(compose_error(&err)),
        },
        Operation::Schema => Ok(to_value(&build_schema_response(
            request.function_id.as_deref(),
        ))),
        Operation::Snapshot => {
            let id = request
                .progress_operation_id
                .or(request.operation_id)
                .ok_or_else(|| Error::Handler("operation_id is required".into()))?;
            let operation = daemon
                .operations
                .get(&id)
                .await
                .ok_or_else(|| Error::Handler(format!("unknown compose operation '{id}'")))?;
            Ok(to_value(&operation.snapshot().await))
        }
        Operation::Cancel => {
            let id = request
                .progress_operation_id
                .or(request.operation_id)
                .ok_or_else(|| Error::Handler("operation_id is required".into()))?;
            Ok(json!({ "operation_id": id, "cancelled": daemon.operations.cancel(&id).await }))
        }
    }
}

async fn admit_mutation(
    daemon: &Daemon,
    requested_id: Option<String>,
    requested: usize,
    detail: String,
) -> Result<(Arc<crate::operation::Operation>, OperationAcceptedOutcome), Error> {
    let operation_id = requested_id.unwrap_or_else(|| format!("compose:{}", uuid::Uuid::new_v4()));
    let operation = daemon
        .operations
        .create_with_id(operation_id.clone(), requested)
        .await
        .map_err(|_| Error::Remote {
            code: "OPERATION_ID_ALREADY_EXISTS".to_string(),
            message: format!("compose operation '{operation_id}' already exists"),
            stacktrace: None,
        })?;
    operation.emit(None, "accepted", detail).await;
    let accepted = OperationAcceptedOutcome {
        operation_id,
        status: "accepted".to_string(),
        requested,
        outcome: None,
    };
    Ok((operation, accepted))
}

/// Compose the terminal detail for a mutation that succeeded.
///
/// A non-required container that never reached its target state leaves the
/// operation succeeding, which is by design — but the blanket success line
/// then claims every worker is up while one is not, and a caller polling
/// `compose::operation` has no other place to learn it. Name those containers
/// instead (MOT-4761).
///
/// The list is the project's, not this operation's request: reconciliation
/// reports every container it touched, so one left broken by an earlier
/// operation keeps being named until it starts. That is the point — the
/// alternative is a success line that is false about the project.
fn succeeded_detail(details: MutationDetails, not_required_failures: &[String]) -> String {
    if not_required_failures.is_empty() {
        return details.success.to_string();
    }
    format!(
        "{}: {}. The operation still succeeded because they are not required; \
         check compose::status and their logs before using them.",
        details.partial,
        not_required_failures.join(", ")
    )
}

/// The three terminal lines one mutation can finish with. Named fields rather
/// than three positional `&str`s: they are the same type, so a swap at a call
/// site would compile and only show up as a wrong sentence in a live run.
#[derive(Clone, Copy)]
struct MutationDetails {
    /// What this mutation is called in telemetry.
    op: &'static str,
    /// Every container reached its target state.
    success: &'static str,
    /// The operation succeeded, but some non-required container did not.
    partial: &'static str,
    /// A required container did not, so the operation failed.
    failed: &'static str,
    /// It succeeded without changing anything: the file already said it.
    unchanged: &'static str,
}

const ADD_DETAILS: MutationDetails = MutationDetails {
    op: "add",
    success: "all requested workers are ready",
    partial: "workers that did not start",
    failed: "one or more workers failed",
    unchanged: "every requested worker was already declared; nothing changed",
};

const REMOVE_DETAILS: MutationDetails = MutationDetails {
    op: "remove",
    success: "all requested workers were removed",
    partial: "workers that could not be removed",
    failed: "one or more workers could not be removed",
    unchanged: "no requested worker was declared; nothing changed",
};

const UP_DETAILS: MutationDetails = MutationDetails {
    op: "up",
    success: "every container is ready",
    partial: "containers that did not start",
    failed: "one or more containers failed",
    unchanged: "everything was already running",
};

const DOWN_DETAILS: MutationDetails = MutationDetails {
    op: "down",
    success: "every container stopped",
    partial: "containers that did not stop",
    failed: "one or more containers could not be stopped",
    unchanged: "nothing was running",
};

const RESTART_DETAILS: MutationDetails = MutationDetails {
    op: "restart",
    success: "every container restarted",
    partial: "containers that did not start again",
    failed: "one or more containers did not start again",
    unchanged: "nothing to restart",
};

const UPDATE_DETAILS: MutationDetails = MutationDetails {
    op: "update",
    success: "all requested workers were updated",
    partial: "workers that could not be updated",
    failed: "one or more workers could not be updated",
    unchanged: "everything already up to date",
};

fn spawn_mutation<F>(
    operation: Arc<crate::operation::Operation>,
    mutation: F,
    details: MutationDetails,
) -> tokio::task::JoinHandle<Result<MutationOutcome, ComposeError>>
where
    F: Future<Output = Result<MutationOutcome, ComposeError>> + Send + 'static,
{
    spawn_operation(operation, mutation, details, true)
}

/// Runs `mutation` as `operation` and finishes the operation with how it ended. `telemetry`
/// reports it as its own `op`: a lifecycle call that is drawn is reported by
/// [`register_matching`] already, like the same call answered without a stream.
fn spawn_operation<F>(
    operation: Arc<crate::operation::Operation>,
    mutation: F,
    details: MutationDetails,
    telemetry: bool,
) -> tokio::task::JoinHandle<Result<MutationOutcome, ComposeError>>
where
    F: Future<Output = Result<MutationOutcome, ComposeError>> + Send + 'static,
{
    tokio::spawn(async move {
        let began = std::time::Instant::now();
        if operation.is_cancelled() {
            operation
                .finish(
                    crate::operation::OperationStatus::Cancelled,
                    "operation cancelled",
                )
                .await;
            return Err(ComposeError::OperationCancelled {
                operation_id: operation.id().to_string(),
            });
        }

        let result = match AssertUnwindSafe(mutation).catch_unwind().await {
            Ok(result) => result,
            Err(panic) => {
                finish_stopped(&operation, details.op).await;
                std::panic::resume_unwind(panic)
            }
        };
        if telemetry {
            report_mutation(details.op, &result, began.elapsed());
        }
        match &result {
            Ok(outcome) => {
                let failed = outcome.is_failed();
                operation
                    .finish(
                        if failed {
                            crate::operation::OperationStatus::Failed
                        } else {
                            crate::operation::OperationStatus::Succeeded
                        },
                        if failed {
                            details.failed.to_string()
                        } else {
                            succeeded_detail(details, outcome.not_required_failures())
                        },
                    )
                    .await;
            }
            Err(
                error @ (ComposeError::OperationCancelled { .. }
                | ComposeError::RebuildCancelled { .. }),
            ) => {
                // Says which containers a cancelled add or update rebuilt and left unrestarted.
                operation
                    .finish(
                        crate::operation::OperationStatus::Cancelled,
                        error.to_string(),
                    )
                    .await;
            }
            Err(error) => {
                operation
                    .finish(crate::operation::OperationStatus::Failed, error.to_string())
                    .await;
            }
        }
        result
    })
}

/// `1 worker`, `2 workers`.
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// Answers an accepted add, update, or remove: at once without a stream, and with how it ended
/// once it finished when the caller bound one to draw it into.
async fn draw_mutation(
    drawing: Option<(Drawing, crate::operation::Subscription)>,
    title: String,
    operation: &crate::operation::Operation,
    finished: tokio::task::JoinHandle<Result<MutationOutcome, ComposeError>>,
    accepted: OperationAcceptedOutcome,
    details: MutationDetails,
) -> Result<Value, Error> {
    let Some((drawing, events)) = drawing else {
        return Ok(to_value(&accepted));
    };
    let report = CallerReport::start(drawing.stderr, drawing.terminal, title).await;
    answer_drawn(
        report,
        operation,
        events,
        finished,
        details,
        move |outcome| {
            to_value(&OperationAcceptedOutcome {
                status: "succeeded".to_string(),
                outcome: Some(outcome),
                ..accepted
            })
        },
    )
    .await
}

/// Runs an `up`, `down`, or `restart` as an operation the caller's stream draws, and answers as
/// the call does without one: with the outcome, or with its error when it failed.
async fn draw_lifecycle<F, Fut>(
    daemon: &Daemon,
    drawing: Drawing,
    title: String,
    details: MutationDetails,
    run: F,
) -> Result<Value, Error>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<MutationOutcome, ComposeError>> + Send + 'static,
{
    let operation = daemon.operations.create(0).await;
    let events = operation.subscribe();
    let finished = spawn_operation(
        Arc::clone(&operation),
        run(operation.id().to_string()),
        details,
        false,
    );
    let report = CallerReport::start(drawing.stderr, drawing.terminal, title).await;
    answer_drawn(report, &operation, events, finished, details, |outcome| {
        to_value(&outcome)
    })
    .await
}

/// Draws a mutation into the caller's `stderr` stream until it finished, and answers with how it
/// ended: `answer` of its outcome when it succeeded, its error when it failed or was cancelled,
/// so `iii call` exits non-zero.
async fn answer_drawn(
    mut report: CallerReport,
    operation: &crate::operation::Operation,
    events: crate::operation::Subscription,
    finished: tokio::task::JoinHandle<Result<MutationOutcome, ComposeError>>,
    details: MutationDetails,
    answer: impl FnOnce(MutationOutcome) -> Value,
) -> Result<Value, Error> {
    report.follow(operation, events).await;
    let (ending, answer) = match finished.await {
        Ok(result) => drawn_answer(result, details, answer),
        Err(error) => {
            let message = format!("the {} operation stopped: {error}", details.op);
            (
                Ending::Failed(message.clone()),
                Err(Error::Handler(message)),
            )
        }
    };
    report.finish(&ending).await;
    answer
}

/// The last line a drawn mutation shows, and the answer of its call.
fn drawn_answer(
    result: Result<MutationOutcome, ComposeError>,
    details: MutationDetails,
    answer: impl FnOnce(MutationOutcome) -> Value,
) -> (Ending, Result<Value, Error>) {
    match result {
        Ok(outcome) if outcome.is_failed() => {
            let (code, message) = outcome.error().unwrap_or(("OPERATION_FAILED", ""));
            let message = if message.is_empty() {
                details.failed.to_string()
            } else {
                format!("{}: {message}", details.failed)
            };
            (
                Ending::Failed(details.failed.to_string()),
                Err(Error::Remote {
                    code: code.to_string(),
                    message,
                    stacktrace: None,
                }),
            )
        }
        Ok(outcome) => {
            let ending = match outcome.not_required_failures() {
                [] if outcome.changed() => Ending::Succeeded(details.success.to_string()),
                [] => Ending::Succeeded(details.unchanged.to_string()),
                failures => {
                    Ending::Partial(format!("{}: {}", details.partial, failures.join(", ")))
                }
            };
            (ending, Ok(answer(outcome)))
        }
        Err(
            error @ (ComposeError::OperationCancelled { .. }
            | ComposeError::RebuildCancelled { .. }),
        ) => (
            Ending::Cancelled(error.to_string()),
            Err(compose_error(&error)),
        ),
        Err(error) => (
            Ending::Failed(error.to_string()),
            Err(compose_error(&error)),
        ),
    }
}

/// Finishes a build operation with how it ended, and returns the last line a drawn build shows
/// with the answer of its call: the results when every build finished, the first failure's code
/// otherwise.
async fn finish_build(
    operation: &crate::operation::Operation,
    result: crate::error::Result<crate::lifecycle::BuildRun>,
    accepted: OperationAcceptedOutcome,
) -> (Ending, Result<Value, Error>) {
    use crate::operation::OperationStatus;
    match result {
        Ok(crate::lifecycle::BuildRun { results, cancelled }) => {
            // The terminal event names each failure, with its own code.
            let failed: Vec<String> = results
                .iter()
                .filter_map(|result| {
                    result.error.as_ref().map(|error| {
                        format!("{} ({}: {})", result.worker, error.code, error.message)
                    })
                })
                .collect();
            let first_code = results
                .iter()
                .find_map(|result| result.error.as_ref().map(|error| error.code.clone()));
            let value = to_value(&results);
            // Stopped by compose::cancel or down, it is cancelled, whatever a build its stop
            // killed reported.
            if cancelled {
                operation
                    .finish_with_results(
                        OperationStatus::Cancelled,
                        "operation cancelled",
                        Some(value),
                    )
                    .await;
                let error = ComposeError::OperationCancelled {
                    operation_id: accepted.operation_id,
                };
                return (
                    Ending::Cancelled(error.to_string()),
                    Err(compose_error(&error)),
                );
            }
            if let Some(code) = first_code {
                let detail = format!("builds failed: {}", failed.join("; "));
                operation
                    .finish_with_results(OperationStatus::Failed, detail.clone(), Some(value))
                    .await;
                // The last line names each failure by its code: the build's own lines, printed
                // above, say the rest, and the call's error carries all of it.
                let named: Vec<String> = results
                    .iter()
                    .filter_map(|result| {
                        let error = result.error.as_ref()?;
                        Some(format!("{} ({})", result.worker, error.code))
                    })
                    .collect();
                return (
                    Ending::Failed(format!("builds failed: {}", named.join(", "))),
                    Err(Error::Remote {
                        code,
                        message: detail,
                        stacktrace: None,
                    }),
                );
            }
            operation
                .finish_with_results(
                    OperationStatus::Succeeded,
                    "every requested build finished",
                    Some(value.clone()),
                )
                .await;
            let ending = if accepted.requested == 0 {
                "nothing to build"
            } else {
                "every requested build finished"
            };
            let answer = to_value(&BuildFinished {
                operation_id: accepted.operation_id,
                status: "succeeded".to_string(),
                requested: accepted.requested,
                results,
            });
            (Ending::Succeeded(ending.to_string()), Ok(answer))
        }
        Err(error @ ComposeError::OperationCancelled { .. }) => {
            operation
                .finish(OperationStatus::Cancelled, "operation cancelled")
                .await;
            (
                Ending::Cancelled(error.to_string()),
                Err(compose_error(&error)),
            )
        }
        Err(error) => {
            operation
                .finish(OperationStatus::Failed, error.to_string())
                .await;
            (
                Ending::Failed(error.to_string()),
                Err(compose_error(&error)),
            )
        }
    }
}

/// How long a followed `compose::logs` waits for new lines before it checks the caller is still
/// reading.
const LOGS_FOLLOW_WAIT_MS: u64 = 1_000;

/// What a `compose::logs` with streams reads.
struct LogsQuery {
    file: Option<PathBuf>,
    container: Option<String>,
    cursors: BTreeMap<String, LogCursor>,
    tail: usize,
    stream: Option<LogStream>,
    wait_ms: u64,
    follow: bool,
}

/// Where a `compose::logs` with streams writes.
struct LogStreams {
    stdout: Option<OutputStream>,
    stderr: Option<OutputStream>,
    terminal: CallerTerminal,
}

/// `compose::logs` into the caller's streams, as text: stdout's lines into `stdout` and
/// stderr's into `stderr` (each into the one bound when only one is), labelled with their worker
/// when every worker is read. With `follow`, it reads on until the caller leaves. Answers with
/// the cursors to resume from.
async fn print_logs(
    daemon: &Daemon,
    mut query: LogsQuery,
    streams: LogStreams,
) -> Result<Value, Error> {
    let labelled = query.container.is_none();
    // The project the session reads, chosen once: another project loaded meanwhile must not
    // change which log a cursor is read against.
    let file = daemon
        .resolve_file(query.file.as_deref())
        .await
        .map_err(|err| compose_error(&err))?;
    let mut wait_ms = query.wait_ms;
    let mut tail = query.tail;
    let mut left = false;
    while !left {
        let outcome = daemon
            .logs(
                Some(&file),
                query.container.as_deref(),
                query.cursors.clone(),
                tail,
                query.stream,
                wait_ms,
            )
            .await
            .map_err(|err| compose_error(&err))?;
        let (mut out, mut err) = (String::new(), String::new());
        let mut cursors = query.cursors.clone();
        for batch in outcome.containers {
            for entry in &batch.entries {
                let to_stderr = (entry.stream == LogStream::Stderr && streams.stderr.is_some())
                    || streams.stdout.is_none();
                let (text, terminal) = if to_stderr {
                    (&mut err, streams.terminal.stderr)
                } else {
                    (&mut out, streams.terminal.stdout)
                };
                if labelled {
                    let color = terminal && streams.terminal.color;
                    text.push_str(&crate::caller_report::label(&batch.container, color));
                    text.push(' ');
                }
                text.push_str(&entry.message);
                text.push('\n');
            }
            if batch.truncated {
                let notes = if streams.stderr.is_some() {
                    &mut err
                } else {
                    &mut out
                };
                notes.push_str(&format!(
                    "{}: older output is no longer retained; showing the most recent lines\n",
                    batch.container
                ));
            }
            if let Some(cursor) = batch.cursor {
                cursors.insert(batch.container, cursor);
            }
        }
        // An empty write sends nothing, and fails once the reader left.
        for (stream, text) in [(&streams.stdout, out), (&streams.stderr, err)] {
            if let Some(stream) = stream
                && stream.write(text.as_bytes()).await.is_err()
            {
                left = true;
            }
        }
        // The cursors move past this batch only once all of it was written: a caller that
        // resumes from them must not skip lines it never got.
        if !left {
            query.cursors = cursors;
        }
        if !query.follow {
            break;
        }
        wait_ms = LOGS_FOLLOW_WAIT_MS;
        // `tail` chose where reading began. From the cursors on, it is the most lines one read
        // returns, so `tail=0` must not stay one: nothing would ever be read again.
        tail = crate::logs::MAX_TAIL_LINES;
    }
    if !left {
        for stream in [&streams.stdout, &streams.stderr].into_iter().flatten() {
            let _ = stream.close().await;
        }
    }
    Ok(to_value(&LogsStreamed {
        cursors: query.cursors,
    }))
}

/// Ends an operation whose task panicked: its subscribers (the caller's report) get a terminal
/// event instead of waiting for one forever, and the task's join error says the rest.
async fn finish_stopped(operation: &crate::operation::Operation, op: &str) {
    operation
        .finish(
            crate::operation::OperationStatus::Failed,
            format!("the {op} operation stopped unexpectedly"),
        )
        .await;
}

/// Reports one mutation that ran in the background after it was accepted.
fn report_mutation(
    op: &'static str,
    result: &Result<MutationOutcome, ComposeError>,
    elapsed: std::time::Duration,
) {
    let reported = match result {
        Ok(outcome) => Ok(to_value(outcome)),
        Err(error) => Err(compose_error(error)),
    };
    report_op(op, &reported, elapsed);
}

/// Adds what this host asks the registry for, so a caller checking for
/// updates asks for a build compose would install.
fn with_host_targets(mut answer: Value) -> Value {
    insert_host_targets(
        &mut answer,
        crate::registry::host_target(),
        crate::registry::host_targets(),
    );
    answer
}

/// Inserts `target` and `targets` into an object answer. `target` is the
/// triple compose sends first to the registry's `POST /resolve`, and `targets`
/// every triple it tries, in order. A build with no triple for its platform
/// reports `unknown`, which no registry serves, so the answer leaves both out
/// rather than offer it to a caller.
fn insert_host_targets(answer: &mut Value, target: &str, targets: &[&str]) {
    if target == "unknown" {
        return;
    }
    if let Some(fields) = answer.as_object_mut() {
        fields.insert("target".to_string(), json!(target));
        fields.insert("targets".to_string(), json!(targets));
    }
}

/// Serialize the generated root schema into the value carried over the wire.
fn schema_for_value<T: JsonSchema>() -> Option<Value> {
    serde_json::to_value(schema_for!(T)).ok()
}

fn requested_workers(
    workers: Option<Vec<WorkerInput>>,
    worker: Option<String>,
) -> Vec<WorkerInput> {
    workers
        .or_else(|| worker.map(|worker| vec![WorkerInput::Spec(worker)]))
        .unwrap_or_default()
}

/// Resolve legacy worker inputs and reject container objects for update and remove.
fn requested_specs(
    workers: Option<Vec<WorkerInput>>,
    worker: Option<String>,
) -> Result<Vec<String>, Error> {
    requested_workers(workers, worker)
        .into_iter()
        .map(|input| match input {
            WorkerInput::Spec(spec) => Ok(spec),
            WorkerInput::Definition(_) => Err(compose_error(&ComposeError::InvalidWorkerSpec {
                spec: "container object".to_string(),
                reason: "container objects are supported only by compose::add".to_string(),
            })),
        })
        .collect()
}

/// Batch worker mutations accept their canonical list or the old singular field.
///
/// Both fields are optional in the Rust shape so they can share the common
/// namespace and file properties. `anyOf` states the runtime rule that at
/// least one input form must be present. Supplying both is valid; dispatch
/// gives the canonical `workers` list precedence.
fn batch_worker_options_schema() -> Option<Value> {
    let mut schema = schema_for_value::<BatchWorkerOptions<String>>()?;
    for field in ["workers", "worker"] {
        let types = schema
            .pointer_mut(&format!("/properties/{field}/type"))?
            .as_array_mut()?;
        types.retain(|kind| kind != "null");
    }
    schema
        .pointer_mut("/properties/workers")?
        .as_object_mut()?
        .insert("minItems".to_string(), json!(1));
    for pointer in ["/properties/worker", "/properties/workers/items"] {
        schema
            .pointer_mut(pointer)?
            .as_object_mut()?
            .insert("pattern".to_string(), json!(r"\S"));
    }
    schema.as_object_mut()?.insert(
        "anyOf".to_string(),
        json!([
            { "required": ["workers"] },
            { "required": ["worker"] },
        ]),
    );
    Some(schema)
}

/// Remove also takes `prune`, for the data of the removed workers' VMs (spec 8.2).
fn remove_worker_options_schema() -> Option<Value> {
    let mut schema = batch_worker_options_schema()?;
    schema.pointer_mut("/properties")?.as_object_mut()?.insert(
        "prune".to_string(),
        json!({
            "description": "Also delete the data of each removed worker's VM. Without it the \
                            adapter keeps the data, and a worker added again under the same \
                            key boots with it.",
            "type": ["boolean", "null"],
        }),
    );
    Some(schema)
}

/// Update defaults to all packages, but explicit inputs must still be valid.
fn update_worker_options_schema() -> Option<Value> {
    let mut schema = batch_worker_options_schema()?;
    schema.as_object_mut()?.remove("anyOf");
    schema["description"] = json!(
        "Omit worker and workers to update all declared package workers to latest. Path workers are skipped."
    );
    Some(schema)
}

/// Extend the batch contract with the container object form accepted by add.
fn add_worker_options_schema() -> Option<Value> {
    let mut schema = batch_worker_options_schema()?;
    let detailed = schema_for_value::<BatchWorkerOptions<WorkerInput>>()?;
    schema["properties"]["workers"]["items"] = detailed["properties"]["workers"]["items"].clone();
    schema["definitions"] = detailed["definitions"].clone();
    schema["definitions"]["WorkerInput"]["anyOf"][0]["pattern"] = json!(r"\S");
    schema["definitions"]["RawContainer"]["properties"]["worker"]["pattern"] = json!(r"\S");
    Some(schema)
}

/// What a request schema says of the stream a call draws its progress into: `stderr`, which
/// `iii call` binds to its own, and `terminal`, which it fills.
const PROGRESS_STREAM: &str = "A stream the progress is drawn into. With it, the call answers \
                               once the operation finished, and fails when it failed or was \
                               cancelled; a reader that leaves cancels the operation. `iii call` \
                               binds its own standard error to it.";

/// Adds the `stderr` progress stream and the `terminal` it is drawn for to a request schema.
fn with_progress(schema: Option<Value>) -> Option<Value> {
    let mut schema = schema?;
    let properties = schema.pointer_mut("/properties")?.as_object_mut()?;
    properties.insert("stderr".to_string(), output_stream_schema(PROGRESS_STREAM));
    properties.insert("terminal".to_string(), caller_terminal_schema());
    Some(schema)
}

/// `compose::logs` takes streams to write its lines into, and `follow` to keep writing.
fn logs_options_schema() -> Option<Value> {
    let mut schema = schema_for_value::<LogsOptions>()?;
    let properties = schema.pointer_mut("/properties")?.as_object_mut()?;
    properties.insert(
        "stdout".to_string(),
        output_stream_schema(
            "A stream the stdout lines are written into, as text, labelled with their worker when \
             every worker is read; the stderr lines too without `stderr`. With it, the call \
             answers with the cursors to resume from. `iii call` binds its own standard output \
             to it.",
        ),
    );
    properties.insert(
        "stderr".to_string(),
        output_stream_schema(
            "A stream the stderr lines are written into, as text. `iii call` binds its own \
             standard error to it.",
        ),
    );
    properties.insert(
        "follow".to_string(),
        json!({
            "description": "With a stream: keep writing new lines until the caller leaves.",
            "type": ["boolean", "null"],
        }),
    );
    properties.insert("terminal".to_string(), caller_terminal_schema());
    Some(schema)
}

/// A stream the function writes: the write end of an engine pipe, as every SDK describes an
/// `OutputStream`, so `iii call` binds its own output to a field named `stdout` or `stderr`.
fn output_stream_schema(description: &str) -> Value {
    json!({
        "title": "OutputStream",
        "description": description,
        "type": "object",
        "x-iii-stream": "write",
        "required": ["access_key", "channel_id", "direction"],
        "properties": {
            "channel_id": { "type": "string" },
            "access_key": { "type": "string" },
            "direction": { "type": "string", "const": "write" },
        },
    })
}

/// The terminal the progress is drawn for. `iii call` fills a field marked `x-iii-terminal`.
fn caller_terminal_schema() -> Value {
    let size = |what: &str| {
        json!({
            "description": format!("{what} of the terminal, when known."),
            "type": ["integer", "null"],
            "minimum": 0,
        })
    };
    json!({
        "description": "The caller's terminal: a terminal gets a spinner redrawn in place, \
                        anything else one line per change. `iii call` fills it.",
        "type": ["object", "null"],
        "x-iii-terminal": true,
        "properties": {
            "stdout": { "description": "Standard output is a terminal.", "type": "boolean" },
            "stderr": { "description": "Standard error is a terminal.", "type": "boolean" },
            "columns": size("Width in columns"),
            "rows": size("Height in rows"),
            "color": { "description": "Colour is wanted.", "type": "boolean" },
        },
    })
}

/// One source of truth for function registration and `compose::schema`.
fn op_description(function_id: &str) -> &'static str {
    match function_id {
        "compose::up" => {
            "Start a compose project, or one container and its dependencies. \
             Repeated calls leave ready containers running. A container whose \
             effective required value is false is named in \
             not_required_failures when it fails, and the operation still \
             returns ok. Frozen mode requires the selected compose file's lock to match and \
             skips package resolution. With a `stderr` stream, draws the progress into it."
        }
        "compose::down" => {
            "Stop a compose project, or one container and its dependents, in \
             reverse dependency order. With a `stderr` stream, draws the progress into it."
        }
        "compose::list" => {
            "List every project loaded by this compose daemon, and the target \
             triples this host resolves packages for. Waits for current per-project \
             snapshots; cancellation or the request deadline fails the whole list."
        }
        "compose::status" => {
            "Report the project namespace, state directory, daemon pid, the \
             target triples this host resolves packages for, and current state \
             of every declared container, with a package's declared version \
             selector as 'requested'. A container declaring a 'restart' policy \
             reports as 'restarting' while it waits for the supervisor's next \
             attempt. Waits for a current snapshot; cancellation or the request \
             deadline returns an error, never persisted fallback state."
        }
        "compose::ps" => {
            "List the operating-system processes of this daemon's projects, or of \
             one project: the daemon, its managed engine, every worker and \
             whatever those workers spawned. A worker marked orphan is no \
             longer supervised by any compose daemon."
        }
        "compose::logs" => {
            "Read bounded worker stdout and stderr. A cursor continues from the last response; \
             callers may long-poll for new output. With `stdout` or `stderr` streams, writes \
             the lines into them as text, and with `follow` keeps writing until the caller \
             leaves."
        }
        "compose::stop" => {
            "Ask this compose daemon to stop every project and exit after it \
             answers the caller."
        }
        "compose::validate" => {
            "Validate a worker-compose.yaml file offline without loading or \
             starting its project."
        }
        "compose::add" => {
            "Accept an observable operation that declares one or more workers and their registry \
             dependencies in the compose file, locks resolved packages, then reconciles changed \
             workers once. Explicit selectors remain in the compose file. With a `stderr` \
             stream, draws the progress into it and answers once the operation finished."
        }
        "compose::remove" => {
            "Accept an observable operation that removes one or more declared workers and \
             dependency references to them, stops only those workers, then reconciles anything \
             already missing. With a `stderr` stream, draws the progress into it and answers \
             once the operation finished."
        }
        "compose::restart" => {
            "Restart the whole project, or restart one named container without \
             changing its dependency graph. With a `stderr` stream, draws the progress into it."
        }
        "compose::build" => {
            "Accept an observable operation that runs scripts.build for one container, or for \
             every container that declares one, under the project's mutation lock. It never \
             restarts a container; the finished operation carries one result per container. \
             compose::cancel or compose::down stops it. With a `stderr` stream, draws the \
             progress and the build lines into it and answers once the operation finished."
        }
        "compose::update" => {
            "Accept an observable operation that refreshes declared package selectors or moves \
             workers to requested versions, including their dependency graphs. Omit worker and \
             workers to move all declared packages to latest; path workers are skipped. The \
             project restarts only when runtime content or topology changes. With a `stderr` \
             stream, draws the progress into it and answers once the operation finished."
        }
        "compose::schema" => {
            "Return request and response JSON Schemas for compose::* functions. \
             Optional function_id filters one entry. The worker-compose.yaml \
             pseudo-id returns the compose-file schema and an example."
        }
        "worker-compose.yaml" => {
            "JSON Schema for a worker-compose.yaml file. The response is a \
             complete small example keyed by its file name."
        }
        "compose::operation" => {
            "Return the latest snapshot for reconnecting to a pushed Compose operation."
        }
        "compose::cancel" => "Request cancellation of a running Compose operation.",
        _ => "",
    }
}

/// Recommended timeout and retry safety for each operation.
fn op_metadata(function_id: &str) -> (u64, bool) {
    match function_id {
        "compose::up" => (600_000, true),
        "compose::down" => (60_000, true),
        "compose::list" => (10_000, true),
        "compose::status" => (10_000, true),
        "compose::ps" => (10_000, true),
        "compose::logs" => (10_000, true),
        "compose::stop" => (30_000, false),
        "compose::validate" => (10_000, true),
        "compose::add" => (600_000, false),
        "compose::remove" => (600_000, true),
        "compose::restart" => (600_000, false),
        "compose::update" => (600_000, false),
        "compose::build" => (10_000, false),
        "compose::schema" | "worker-compose.yaml" => (10_000, true),
        "compose::operation" | "compose::cancel" => (10_000, true),
        _ => (30_000, false),
    }
}

/// Every callable compose function plus the compose-file authoring contract.
type SchemaTriple = (&'static str, Option<Value>, Option<Value>);

/// Build every schema once and share it between registration and requests.
fn schema_table() -> &'static [SchemaTriple] {
    static TABLE: std::sync::LazyLock<Vec<SchemaTriple>> = std::sync::LazyLock::new(|| {
        vec![
            (
                "compose::up",
                with_progress(schema_for_value::<UpOptions>()),
                schema_for_value::<MutationOutcome>(),
            ),
            (
                "compose::down",
                with_progress(schema_for_value::<LifecycleOptions>()),
                schema_for_value::<MutationOutcome>(),
            ),
            (
                "compose::list",
                schema_for_value::<DaemonOptions>(),
                schema_for_value::<ListOutcome>(),
            ),
            (
                "compose::status",
                schema_for_value::<ProjectOptions>(),
                schema_for_value::<StatusOutcome>(),
            ),
            (
                "compose::ps",
                schema_for_value::<ProjectOptions>(),
                schema_for_value::<PsOutcome>(),
            ),
            (
                "compose::logs",
                logs_options_schema(),
                schema_for_value::<LogsAnswer>(),
            ),
            (
                "compose::stop",
                schema_for_value::<DaemonOptions>(),
                schema_for_value::<StopOutcome>(),
            ),
            (
                "compose::validate",
                schema_for_value::<ProjectOptions>(),
                schema_for_value::<ValidateOutcome>(),
            ),
            (
                "compose::add",
                with_progress(add_worker_options_schema()),
                schema_for_value::<OperationAcceptedOutcome>(),
            ),
            (
                "compose::remove",
                with_progress(remove_worker_options_schema()),
                schema_for_value::<OperationAcceptedOutcome>(),
            ),
            (
                "compose::restart",
                with_progress(schema_for_value::<RestartOptions>()),
                schema_for_value::<MutationOutcome>(),
            ),
            (
                "compose::update",
                with_progress(update_worker_options_schema()),
                schema_for_value::<OperationAcceptedOutcome>(),
            ),
            (
                "compose::build",
                with_progress(schema_for_value::<BuildOptions>()),
                schema_for_value::<BuildAnswer>(),
            ),
            (
                "compose::schema",
                schema_for_value::<SchemaRequest>(),
                schema_for_value::<SchemaResponse>(),
            ),
            (
                "compose::operation",
                schema_for_value::<ComposeRequest>(),
                schema_for_value::<crate::operation::OperationSnapshot>(),
            ),
            (
                "compose::cancel",
                schema_for_value::<ComposeRequest>(),
                schema_for_value::<CancelOutcome>(),
            ),
            (
                "worker-compose.yaml",
                Some(crate::config::worker_compose_schema_json()),
                Some(crate::config::worker_compose_example_json()),
            ),
        ]
    });
    &TABLE
}

/// Select one schema when requested, or return the complete table.
fn build_schema_response(filter: Option<&str>) -> SchemaResponse {
    let schemas = schema_table()
        .iter()
        .filter(|(id, _, _)| filter.is_none_or(|filter| filter == *id))
        .map(|(function_id, request, response)| {
            let (default_timeout_ms, idempotent) = op_metadata(function_id);
            SchemaEntry {
                function_id: (*function_id).to_string(),
                description: op_description(function_id).to_string(),
                request: request.clone().unwrap_or(Value::Null),
                response: response.clone().unwrap_or(Value::Null),
                default_timeout_ms,
                idempotent,
            }
        })
        .collect();
    SchemaResponse { schemas }
}

/// Add descriptions, operation metadata, and the same schemas returned by
/// `compose::schema` to the engine function registry.
fn describe_op(registration: RegisterFunction, function_id: &str) -> RegisterFunction {
    let (default_timeout_ms, idempotent) = op_metadata(function_id);
    let mut registration = registration
        .description(op_description(function_id))
        .metadata(json!({
            "default_timeout_ms": default_timeout_ms,
            "idempotent": idempotent,
        }));
    if let Some((_, request, response)) =
        schema_table().iter().find(|(id, _, _)| *id == function_id)
    {
        if let Some(request) = request {
            registration = registration.request_format(request.clone());
        }
        if let Some(response) = response {
            registration = registration.response_format(response.clone());
        }
    }
    registration
}

/// Compose errors cross the wire as their stable code plus the message, so a
/// caller can match on `UNKNOWN_PROJECT` without parsing prose. `Error::Remote`
/// is the only variant the SDK forwards verbatim into the wire `ErrorBody`.
fn compose_error(error: &ComposeError) -> Error {
    Error::Remote {
        code: error.code().to_string(),
        message: error.to_string(),
        stacktrace: None,
    }
}

/// Handler failures must be Remote to preserve TIMEOUT in the wire ErrorBody.
fn read_timeout(_: tokio::time::error::Elapsed) -> Error {
    Error::Remote {
        code: "TIMEOUT".to_string(),
        message: Error::Timeout.to_string(),
        stacktrace: None,
    }
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or_else(|err| json!({ "error": err.to_string() }))
}

fn operation_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds held between admission and their task, by operation id: released when the sender
    /// is dropped or sends.
    static BUILD_HOLDS: std::sync::Mutex<
        std::collections::BTreeMap<String, tokio::sync::oneshot::Receiver<()>>,
    > = std::sync::Mutex::new(std::collections::BTreeMap::new());

    pub(super) async fn hold_build(operation_id: &str) {
        let hold = BUILD_HOLDS.lock().unwrap().remove(operation_id);
        if let Some(hold) = hold {
            let _ = hold.await;
        }
    }

    /// A `compose::build` without `file` builds the project its admission resolved, though
    /// another one is loaded before its task starts.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_build_runs_on_the_file_its_admission_resolved() {
        let tmp = tempfile::tempdir().unwrap();
        let files: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|project| {
                let dir = tmp.path().join(project);
                std::fs::create_dir_all(dir.join("api")).unwrap();
                let file = dir.join("worker-compose.yaml");
                std::fs::write(
                    &file,
                    "containers:\n  api: {worker: path://./api, scripts: {build: 'touch built', run: x}}\n",
                )
                .unwrap();
                file
            })
            .collect();
        let daemon = Daemon::start(
            "ws://127.0.0.1:1/ws".to_string(),
            format!("build-admitted-{}", uuid::Uuid::new_v4()),
            None,
            crate::daemon::EnginePolicy::External,
        );
        daemon.project(&files[0]).await.unwrap();
        let (release, hold) = tokio::sync::oneshot::channel();
        BUILD_HOLDS
            .lock()
            .unwrap()
            .insert("build-admitted".to_string(), hold);

        dispatch(
            Arc::clone(&daemon),
            Operation::Build,
            "build".to_string(),
            ComposeRequest {
                operation_id: Some("build-admitted".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        daemon.project(&files[1]).await.unwrap();
        release.send(()).unwrap();

        let operation = daemon.operations.get("build-admitted").await.unwrap();
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let snapshot = operation.snapshot().await;
                if snapshot.status != crate::operation::OperationStatus::Running {
                    return snapshot;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let snapshot = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(snapshot["status"], "succeeded", "{snapshot}");
        assert!(tmp.path().join("a/api/built").exists());
        assert!(!tmp.path().join("b/api/built").exists());
        daemon.shutdown().await;
    }

    /// A build stopped by `compose::cancel` ends `cancelled`, the build it killed in `results` as
    /// `failed`: the CLI exits 130 only then.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_cancelled_compose_build_ends_cancelled() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("slow")).unwrap();
        let file = tmp.path().join("worker-compose.yaml");
        std::fs::write(
            &file,
            "containers:\n  slow: {worker: path://./slow, scripts: {build: 'touch started; sleep 30', run: x}}\n",
        )
        .unwrap();
        let daemon = Daemon::start(
            "ws://127.0.0.1:1/ws".to_string(),
            format!("build-cancel-{}", uuid::Uuid::new_v4()),
            None,
            crate::daemon::EnginePolicy::External,
        );
        let request = |operation: Operation, function: &str| {
            dispatch(
                Arc::clone(&daemon),
                operation,
                function.to_string(),
                ComposeRequest {
                    file: Some(file.to_string_lossy().into_owned()),
                    operation_id: Some("build-slow".to_string()),
                    ..Default::default()
                },
            )
        };
        request(Operation::Build, "build").await.unwrap();
        let operation = daemon.operations.get("build-slow").await.unwrap();
        // Its script starts, or the operation ends before it does: bounded either way.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        while !tmp.path().join("slow/started").exists() {
            let snapshot = operation.snapshot().await;
            let ended = snapshot.status != crate::operation::OperationStatus::Running;
            if ended || tokio::time::Instant::now() >= deadline {
                daemon.shutdown().await;
                panic!(
                    "the build's script never started: {}",
                    serde_json::to_value(&snapshot).unwrap()
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        let cancelled = request(Operation::Cancel, "cancel").await.unwrap();
        assert_eq!(cancelled["cancelled"], true, "{cancelled}");
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let snapshot = operation.snapshot().await;
                if snapshot.status != crate::operation::OperationStatus::Running {
                    return snapshot;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let snapshot = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(snapshot["status"], "cancelled", "{snapshot}");
        assert_eq!(snapshot["results"][0]["worker"], "slow", "{snapshot}");
        assert_eq!(snapshot["results"][0]["status"], "failed", "{snapshot}");
        daemon.shutdown().await;
    }

    /// Spec 7.4: accepted at once with `requested`, then a snapshot whose top-level `results`
    /// holds one entry per container, and a terminal `detail` naming each failure.
    #[tokio::test]
    async fn compose_build_is_an_observable_operation_with_results() {
        let tmp = tempfile::tempdir().unwrap();
        for worker in ["api", "bad", "none"] {
            std::fs::create_dir(tmp.path().join(worker)).unwrap();
        }
        let file = tmp.path().join("worker-compose.yaml");
        std::fs::write(
            &file,
            "containers:\n  api: {worker: path://./api, scripts: {build: 'echo ok', run: x}}\n  bad: {worker: path://./bad, scripts: {build: 'echo broken&& exit 2', run: x}}\n  none: {worker: path://./none, scripts: {build: '', run: x}}\n",
        )
        .unwrap();
        let daemon = Daemon::start(
            "ws://127.0.0.1:1/ws".to_string(),
            format!("build-test-{}", uuid::Uuid::new_v4()),
            None,
            crate::daemon::EnginePolicy::External,
        );
        let accepted = dispatch(
            Arc::clone(&daemon),
            Operation::Build,
            "build".to_string(),
            ComposeRequest {
                file: Some(file.to_string_lossy().into_owned()),
                operation_id: Some("build-all".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(
            accepted,
            json!({ "operation_id": "build-all", "status": "accepted", "requested": 3 })
        );
        let operation = daemon.operations.get("build-all").await.unwrap();
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let snapshot = operation.snapshot().await;
                if snapshot.status != crate::operation::OperationStatus::Running {
                    return snapshot;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let snapshot = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(snapshot["status"], "failed", "{snapshot}");
        assert_eq!(
            snapshot["results"],
            json!([
                { "worker": "api", "status": "built" },
                { "worker": "bad", "status": "failed", "error": { "code": "EXIT_2", "message": "broken" } },
                { "worker": "none", "status": "skipped" },
            ])
        );
        assert_eq!(
            snapshot["last_event"]["detail"],
            "builds failed: bad (EXIT_2: broken)"
        );
        assert_eq!(snapshot["last_event"]["terminal"], true);
        daemon.shutdown().await;
    }

    #[tokio::test]
    async fn update_without_worker_fields_returns_an_observable_operation() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("worker-compose.yaml");
        std::fs::write(
            &file,
            "containers:\n  api:\n    worker: path://.\n    scripts:\n      run: ./api\n",
        )
        .unwrap();
        let daemon = Daemon::start(
            "ws://127.0.0.1:1/ws".to_string(),
            "update-test".to_string(),
            None,
            crate::daemon::EnginePolicy::External,
        );
        let accepted = dispatch(
            Arc::clone(&daemon),
            Operation::Update,
            "update".to_string(),
            ComposeRequest {
                file: Some(file.to_string_lossy().into_owned()),
                operation_id: Some("update-all".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(accepted["status"], "accepted");
        assert_eq!(accepted["operation_id"], "update-all");
        let operation = daemon.operations.get("update-all").await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let snapshot = operation.snapshot().await;
                if snapshot.status == crate::operation::OperationStatus::Succeeded {
                    break;
                }
                assert_ne!(snapshot.status, crate::operation::OperationStatus::Failed);
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("update all should complete without starting a path worker");
    }

    #[tokio::test]
    async fn update_rejects_an_explicit_empty_list_even_with_a_singular_worker() {
        let daemon = Daemon::start(
            "ws://127.0.0.1:1/ws".to_string(),
            "update-test".to_string(),
            None,
            crate::daemon::EnginePolicy::External,
        );
        for worker in [None, Some("state".to_string())] {
            let error = dispatch(
                Arc::clone(&daemon),
                Operation::Update,
                "update".to_string(),
                ComposeRequest {
                    workers: Some(Vec::new()),
                    worker,
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
            assert!(matches!(error, Error::Remote { code, .. } if code == "INVALID_WORKER_SPEC"));
        }
    }

    /// A project with a tag, a range and an exact version, one selector the
    /// file changed after its lock was written, and a path worker.
    fn project_with_selectors(dir: &std::path::Path) -> std::path::PathBuf {
        let file = dir.join("worker-compose.yaml");
        std::fs::write(
            &file,
            "containers:
  tagged:
    worker: package://api.workers.iii.dev/state
    version: latest
  ranged:
    worker: package://api.workers.iii.dev/queue
    version: ^0.22
  pinned:
    worker: package://api.workers.iii.dev/cache
    version: 0.22.8
  edited:
    worker: package://api.workers.iii.dev/stream
    version: ^0.23
  api:
    worker: path://.
    scripts:
      run: ./api
",
        )
        .unwrap();
        let entry = |name: &str, requested: &str, version: &str| {
            json!({
                "worker": format!("package://api.workers.iii.dev/{name}"),
                "requested": requested,
                "resolved": {
                    "name": name,
                    "version": version,
                    "type": "binary",
                    "artifacts": {
                        (crate::registry::host_target()): {
                            "url": format!("https://example.com/{name}.tar.gz"),
                            "sha256": "a".repeat(64),
                        },
                    },
                },
            })
        };
        let lock = json!({
            "version": 1,
            "containers": {
                "tagged": entry("state", "latest", "0.22.8"),
                "ranged": entry("queue", "^0.22", "0.22.3"),
                "pinned": entry("cache", "0.22.8", "0.22.8"),
                // Locked while the file still declared `^0.22`.
                "edited": entry("stream", "^0.22", "0.22.5"),
            },
        });
        std::fs::write(
            crate::lockfile::lock_path(&file),
            serde_yaml::to_string(&lock).unwrap(),
        )
        .unwrap();
        file
    }

    fn selector_daemon() -> Arc<Daemon> {
        Daemon::start(
            "ws://127.0.0.1:1/ws".to_string(),
            "status-test".to_string(),
            None,
            crate::daemon::EnginePolicy::External,
        )
    }

    async fn status_of(daemon: &Arc<Daemon>, file: &std::path::Path) -> Value {
        dispatch(
            Arc::clone(daemon),
            Operation::Status,
            "status".to_string(),
            ComposeRequest {
                file: Some(file.to_string_lossy().into_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn status_reports_each_declared_selector_beside_its_locked_version() {
        let tmp = tempfile::tempdir().unwrap();
        let file = project_with_selectors(tmp.path());
        let daemon = selector_daemon();

        let status = status_of(&daemon, &file).await;

        let containers = status["containers"].as_array().unwrap();
        let reported = |key: &str| {
            let container = containers
                .iter()
                .find(|container| container["container"] == key)
                .unwrap_or_else(|| panic!("status is missing {key}"));
            (
                container.get("requested").cloned(),
                container.get("version").cloned(),
            )
        };
        assert_eq!(
            reported("tagged"),
            (Some(json!("latest")), Some(json!("0.22.8")))
        );
        assert_eq!(
            reported("ranged"),
            (Some(json!("^0.22")), Some(json!("0.22.3")))
        );
        assert_eq!(
            reported("pinned"),
            (Some(json!("0.22.8")), Some(json!("0.22.8")))
        );
        // The selector is the file's, which is what `compose::update` reads.
        // The lock entry for the old selector supplies neither field.
        assert_eq!(reported("edited"), (Some(json!("^0.23")), None));
        assert_eq!(reported("api"), (None, Some(json!("local"))));
        daemon.shutdown().await;
    }

    #[tokio::test]
    async fn status_and_list_carry_the_targets_the_registry_is_asked_for() {
        let tmp = tempfile::tempdir().unwrap();
        let file = project_with_selectors(tmp.path());
        let daemon = selector_daemon();
        let status = status_of(&daemon, &file).await;

        let list = dispatch(
            Arc::clone(&daemon),
            Operation::List,
            "list".to_string(),
            ComposeRequest::default(),
        )
        .await
        .unwrap();

        // Wiring: both answers carry the values resolution sends. What those
        // values are per platform is tested in the registry module.
        assert_eq!(status["target"], crate::registry::host_target());
        assert_eq!(status["targets"], json!(crate::registry::host_targets()));
        assert_eq!(status["targets"][0], status["target"]);
        assert_eq!(
            (&list["target"], &list["targets"]),
            (&status["target"], &status["targets"])
        );
        assert_eq!(list["projects"][0]["containers"], status["containers"]);
        daemon.shutdown().await;
    }

    #[test]
    fn host_targets_are_reported_in_resolution_order() {
        let mut answer = json!({ "daemon_pid": 1 });

        insert_host_targets(
            &mut answer,
            "x86_64-unknown-linux-musl",
            &["x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu"],
        );

        assert_eq!(
            answer,
            json!({
                "daemon_pid": 1,
                "target": "x86_64-unknown-linux-musl",
                "targets": ["x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu"],
            })
        );
    }

    #[test]
    fn a_platform_without_a_triple_reports_no_targets() {
        let mut answer = json!({ "daemon_pid": 1 });

        insert_host_targets(&mut answer, "unknown", &["unknown"]);

        assert_eq!(answer, json!({ "daemon_pid": 1 }));
    }

    #[tokio::test]
    async fn status_and_list_answers_match_their_published_schemas() {
        let tmp = tempfile::tempdir().unwrap();
        let file = project_with_selectors(tmp.path());
        let daemon = selector_daemon();
        let status = status_of(&daemon, &file).await;
        let list = dispatch(
            Arc::clone(&daemon),
            Operation::List,
            "list".to_string(),
            ComposeRequest::default(),
        )
        .await
        .unwrap();
        daemon.shutdown().await;

        for (function_id, answer, containers) in [
            ("compose::status", &status, &status["containers"]),
            ("compose::list", &list, &list["projects"][0]["containers"]),
        ] {
            let schema = schema_entry(function_id).2.as_ref().unwrap();
            let validator = jsonschema::Validator::new(schema).unwrap();
            assert!(
                validator.is_valid(answer),
                "{function_id} answer does not match its schema: {answer}"
            );
            // Nothing the answer carries is missing from the published schema.
            let envelope = schema["properties"].as_object().unwrap();
            for key in answer.as_object().unwrap().keys() {
                assert!(
                    envelope.contains_key(key),
                    "{function_id} schema is missing {key}"
                );
            }
            let container = schema["definitions"]["ContainerStatus"]["properties"]
                .as_object()
                .unwrap();
            for key in containers
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|entry| entry.as_object().unwrap().keys())
            {
                assert!(
                    container.contains_key(key),
                    "{function_id} container schema is missing {key}"
                );
            }
            // Absent on a platform with no triple, and from older daemons.
            let required = schema["required"].as_array().unwrap();
            assert!(!required.contains(&json!("target")));
            assert!(!required.contains(&json!("targets")));
            assert!(envelope.contains_key("target") && envelope.contains_key("targets"));
            let container_required = schema["definitions"]["ContainerStatus"]["required"]
                .as_array()
                .unwrap();
            assert!(!container_required.contains(&json!("requested")));
        }
    }

    #[test]
    fn stop_target_checks_both_fields_and_ignores_blanks() {
        let bare = ComposeRequest::default();
        assert_eq!(stop_target(&bare), None);

        let blank = ComposeRequest {
            container: Some("   ".to_string()),
            worker: Some("".to_string()),
            ..ComposeRequest::default()
        };
        assert_eq!(stop_target(&blank), None, "blanks are no target");

        let hidden = ComposeRequest {
            container: Some(" ".to_string()),
            worker: Some("api".to_string()),
            ..ComposeRequest::default()
        };
        assert_eq!(
            stop_target(&hidden),
            Some("api"),
            "a blank container must not hide the worker"
        );

        let container = ComposeRequest {
            container: Some(" harness ".to_string()),
            ..ComposeRequest::default()
        };
        assert_eq!(stop_target(&container), Some("harness"));
    }

    #[test]
    fn build_target_reads_worker_then_container_and_ignores_blanks() {
        assert_eq!(build_target(&ComposeRequest::default()), None);
        let hidden = ComposeRequest {
            worker: Some(String::new()),
            container: Some(" api ".to_string()),
            ..ComposeRequest::default()
        };
        assert_eq!(
            build_target(&hidden),
            Some("api"),
            "a blank worker must not hide the container"
        );
        let both = ComposeRequest {
            worker: Some("web".to_string()),
            container: Some("api".to_string()),
            ..ComposeRequest::default()
        };
        assert_eq!(build_target(&both), Some("web"));
    }

    #[test]
    fn a_build_reports_cancelled_failed_or_ok() {
        use crate::lifecycle::{BuildResult, BuildRun, OpError};

        let run = |results: Vec<BuildResult>, cancelled| Ok(BuildRun { results, cancelled });
        let failed = BuildResult::failed(
            "api",
            OpError::from_parts("EXIT_1".to_string(), "no".to_string()),
        );
        assert_eq!(
            build_report(&run(vec![failed.clone()], true)).unwrap(),
            json!({ "status": "cancelled", "error": { "code": "OPERATION_CANCELLED" } })
        );
        assert_eq!(
            build_report(&run(vec![BuildResult::skipped("web"), failed], false)).unwrap(),
            json!({ "status": "failed", "error": { "code": "EXIT_1" } })
        );
        assert_eq!(
            build_report(&run(vec![BuildResult::built("api", None)], false)).unwrap(),
            json!({ "status": "ok" })
        );
        assert!(
            build_report(&Err(ComposeError::UnknownContainer {
                container: "x".to_string()
            }))
            .is_err()
        );
    }

    fn schema_entry(id: &str) -> &'static SchemaTriple {
        schema_table()
            .iter()
            .find(|(entry_id, _, _)| *entry_id == id)
            .unwrap_or_else(|| panic!("schema_table is missing {id}"))
    }

    #[test]
    fn schema_table_covers_every_registered_operation() {
        for (name, _) in REGISTERED_OPERATIONS {
            let operation = format!("compose::{name}");
            let (_, request, response) = schema_entry(&operation);
            assert!(request.is_some(), "{operation} request schema missing");
            assert!(response.is_some(), "{operation} response schema missing");
            assert!(!op_description(&operation).is_empty());
        }
    }

    #[test]
    fn batch_worker_request_accepts_a_list_and_the_singular_compatibility_field() {
        let listed: ComposeRequest = serde_json::from_value(json!({
            "workers": ["database", "web"],
        }))
        .expect("workers should deserialize");
        assert_eq!(
            listed.workers,
            Some(vec![
                WorkerInput::Spec("database".to_string()),
                WorkerInput::Spec("web".to_string())
            ])
        );
        assert_eq!(listed.worker, None);

        let singular: ComposeRequest = serde_json::from_value(json!({
            "worker": "database",
        }))
        .expect("the old singular field should deserialize");
        assert_eq!(singular.worker.as_deref(), Some("database"));
        assert_eq!(singular.workers, None);

        assert_eq!(
            requested_specs(None, singular.worker).unwrap(),
            vec!["database".to_string()]
        );
        assert_eq!(
            requested_specs(listed.workers, Some("ignored".to_string())).unwrap(),
            vec!["database".to_string(), "web".to_string()]
        );
    }

    #[test]
    fn only_add_accepts_full_container_objects_in_mixed_batches() {
        let payload = json!({"workers": ["database", {
            "worker": "./api",
            "scripts": {"run": "./api", "pre_run": "echo ready", "pre_run_timeout": "10s", "post_run": "echo stopped"},
            "start_after": ["database"],
            "config_name": "api",
            "config_override": {"port": 3000},
            "working_dir": ".",
            "environment": {"MODE": "dev"},
            "env_file": ["./api.env"],
            "startup_timeout": "5s"
        }]});
        let request: ComposeRequest = serde_json::from_value(payload.clone()).unwrap();
        assert!(requested_specs(request.workers, request.worker).is_err());
        for function_id in ["compose::add", "compose::update", "compose::remove"] {
            let schema = schema_entry(function_id).1.as_ref().unwrap();
            let validator = jsonschema::Validator::new(schema).unwrap();
            assert_eq!(
                validator.is_valid(&payload),
                function_id == "compose::add",
                "{function_id}: {:?}",
                validator.iter_errors(&payload).collect::<Vec<_>>()
            );
        }
        let schema = schema_entry("compose::add").1.as_ref().unwrap();
        let validator = jsonschema::Validator::new(schema).unwrap();
        for invalid in [
            json!({"workers": [{"worker": " "}]}),
            json!({"workers": [{"worker": "api", "script": {"run": "./api"}}]}),
            json!({"workers": [{"worker": "api", "scripts": {"unknown": "./api"}}]}),
            json!({"workers": [{"scripts": {"run": "./api"}}]}),
        ] {
            assert!(!validator.is_valid(&invalid), "accepted {invalid}");
        }
    }

    #[test]
    fn only_remove_publishes_prune_and_it_is_an_optional_boolean() {
        let schema = remove_worker_options_schema().unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        for payload in [
            json!({"worker": "vm"}),
            json!({"worker": "vm", "prune": false}),
            json!({"workers": ["vm"], "prune": true}),
        ] {
            assert!(validator.is_valid(&payload), "{payload}");
            let request: ComposeRequest = serde_json::from_value(payload.clone()).unwrap();
            assert_eq!(request.prune.unwrap_or(false), payload["prune"] == true);
        }
        assert!(!validator.is_valid(&json!({"worker": "vm", "prune": "true"})));
        assert!(
            add_worker_options_schema().unwrap()["properties"]
                .get("prune")
                .is_none()
        );
        assert!(
            update_worker_options_schema().unwrap()["properties"]
                .get("prune")
                .is_none()
        );
    }

    #[test]
    fn batch_worker_schemas_validate_explicit_worker_forms() {
        for function_id in ["compose::add", "compose::update", "compose::remove"] {
            let schema = schema_entry(function_id).1.as_ref().unwrap();
            let validator = jsonschema::Validator::new(schema)
                .unwrap_or_else(|error| panic!("{function_id} schema should compile: {error}"));

            for valid in [
                json!({ "workers": ["database", "web"] }),
                json!({ "worker": "database" }),
            ] {
                let errors = validator
                    .iter_errors(&valid)
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>();
                assert!(
                    errors.is_empty(),
                    "{function_id} should accept {valid}: {errors:?}"
                );
            }

            for invalid in [
                json!({ "workers": [] }),
                json!({ "workers": null }),
                json!({ "worker": null }),
                json!({ "workers": [], "worker": "database" }),
                json!({ "worker": "" }),
                json!({ "worker": "  " }),
                json!({ "workers": [""] }),
                json!({ "workers": ["  "] }),
            ] {
                assert!(
                    !validator.is_valid(&invalid),
                    "{function_id} should reject {invalid}"
                );
            }
        }
    }

    #[test]
    fn only_update_allows_omitting_both_worker_fields() {
        for function_id in ["compose::add", "compose::update", "compose::remove"] {
            let schema = schema_entry(function_id).1.as_ref().unwrap();
            let validator = jsonschema::Validator::new(schema).unwrap();
            for payload in [json!({}), json!({"file": "worker-compose.yaml"})] {
                assert_eq!(
                    validator.is_valid(&payload),
                    function_id == "compose::update",
                    "{function_id}: {payload}"
                );
            }
        }
    }

    #[test]
    fn operation_requests_expose_only_supported_fields() {
        let (_, up, _) = schema_entry("compose::up");
        let up = up.as_ref().unwrap()["properties"].as_object().unwrap();
        assert!(up.contains_key("namespace"));
        assert!(up.contains_key("file"));
        assert!(up.contains_key("container"));
        assert!(up.contains_key("frozen"));
        assert!(!up.contains_key("worker"));

        for function_id in ["compose::add", "compose::update", "compose::remove"] {
            let (_, request, _) = schema_entry(function_id);
            let properties = request.as_ref().unwrap()["properties"].as_object().unwrap();
            for field in ["namespace", "file", "workers", "worker", "operation_id"] {
                assert!(
                    properties.contains_key(field),
                    "{function_id} is missing {field}"
                );
            }
            assert!(!properties.contains_key("container"));
            assert_eq!(properties["workers"]["minItems"], 1);
            if function_id == "compose::update" {
                assert!(request.as_ref().unwrap().get("anyOf").is_none());
                continue;
            }
            let alternatives = request.as_ref().unwrap()["anyOf"]
                .as_array()
                .unwrap_or_else(|| panic!("{function_id} should require either request form"));
            for field in ["workers", "worker"] {
                assert!(alternatives.iter().any(|alternative| {
                    alternative["required"]
                        .as_array()
                        .is_some_and(|required| required.iter().any(|item| item == field))
                }));
            }
        }

        let (_, list, _) = schema_entry("compose::list");
        let list = list.as_ref().unwrap()["properties"].as_object().unwrap();
        assert_eq!(list.keys().collect::<Vec<_>>(), vec!["namespace"]);

        let (_, logs, _) = schema_entry("compose::logs");
        let logs = logs.as_ref().unwrap()["properties"].as_object().unwrap();
        for field in [
            "namespace",
            "file",
            "container",
            "worker",
            "cursors",
            "tail",
            "stream",
            "wait_ms",
        ] {
            assert!(logs.contains_key(field), "compose::logs is missing {field}");
        }
        assert!(!logs.contains_key("workers"));
    }

    #[test]
    fn down_reads_worker_as_the_spelling_for_container() {
        let (_, down, _) = schema_entry("compose::down");
        let down = down.as_ref().unwrap()["properties"].as_object().unwrap();
        assert!(down.contains_key("container"));
        assert!(down.contains_key("worker"));

        let worker = ComposeRequest {
            worker: Some("queue".into()),
            ..Default::default()
        };
        assert_eq!(named_container(&worker), Some("queue"));
        let both = ComposeRequest {
            container: Some("api".into()),
            worker: Some("queue".into()),
            ..Default::default()
        };
        assert_eq!(named_container(&both), Some("api"));
        assert_eq!(named_container(&ComposeRequest::default()), None);
    }

    #[test]
    fn schema_response_filters_one_function_or_returns_all() {
        let filtered = build_schema_response(Some("compose::up"));
        assert_eq!(filtered.schemas.len(), 1);
        assert_eq!(filtered.schemas[0].function_id, "compose::up");

        let all = build_schema_response(None);
        assert_eq!(all.schemas.len(), schema_table().len());
        assert!(
            all.schemas
                .iter()
                .any(|entry| entry.function_id == "worker-compose.yaml")
        );
    }

    #[test]
    fn unpinned_registry_edits_are_not_advertised_as_idempotent() {
        assert_eq!(op_metadata("compose::add"), (600_000, false));
        assert_eq!(op_metadata("compose::update"), (600_000, false));
    }

    #[test]
    fn compose_file_pseudo_entry_carries_schema_and_example() {
        let (_, request, response) = schema_entry("worker-compose.yaml");
        let request = request.as_ref().unwrap();
        assert_eq!(request["type"], "object");
        assert!(request["properties"]["containers"].is_object());

        let response = response.as_ref().unwrap();
        assert!(
            response["worker-compose.yaml"]
                .as_str()
                .is_some_and(|text| text.contains("containers:"))
        );
    }

    #[test]
    fn mutation_schemas_do_not_expose_reconciliation_internals() {
        for id in ["compose::up", "compose::down", "compose::restart"] {
            let properties = schema_entry(id).2.as_ref().unwrap()["properties"]
                .as_object()
                .unwrap();
            for internal in ["operation_id", "containers", "up", "down", "restarted"] {
                assert!(
                    !properties.contains_key(internal),
                    "{id} exposes internal field {internal}"
                );
            }
            assert!(properties.contains_key("status"));
            assert!(properties.contains_key("changed"));
        }
    }

    #[test]
    fn observable_mutation_responses_are_operation_admissions() {
        for function_id in ["compose::add", "compose::update", "compose::remove"] {
            let properties = schema_entry(function_id).2.as_ref().unwrap()["properties"]
                .as_object()
                .unwrap();
            assert!(
                properties.contains_key("operation_id"),
                "{function_id} is missing operation_id"
            );
            assert!(
                properties.contains_key("status"),
                "{function_id} is missing status"
            );
            assert!(
                properties.contains_key("requested"),
                "{function_id} is missing requested"
            );
            assert!(
                !properties.contains_key("containers"),
                "{function_id} exposes container internals"
            );
        }
    }

    #[test]
    fn a_succeeding_mutation_names_the_workers_that_did_not_start() {
        for details in [
            super::ADD_DETAILS,
            super::REMOVE_DETAILS,
            super::UPDATE_DETAILS,
        ] {
            assert_eq!(
                super::succeeded_detail(details, &[]),
                details.success,
                "nothing failed, so the blanket line is true"
            );
            let partial = super::succeeded_detail(
                details,
                &["bulk-importer".to_string(), "analytics".to_string()],
            );
            assert!(
                partial.starts_with(&format!("{}: bulk-importer, analytics.", details.partial)),
                "the terminal detail names them: {partial}"
            );
            assert!(
                !partial.contains(details.success),
                "and never claims they are ready: {partial}"
            );
            assert!(
                partial.contains("compose::status"),
                "pointing at where to look: {partial}"
            );
        }
    }

    /// The three lines are the same type, so the compiler cannot catch a swap
    /// at a call site; distinctness is what makes a swap visible in a test.
    #[test]
    fn every_mutation_names_its_own_three_outcomes() {
        let all = [
            super::ADD_DETAILS,
            super::REMOVE_DETAILS,
            super::UPDATE_DETAILS,
        ];
        let mut lines: Vec<&str> = all
            .iter()
            .flat_map(|d| [d.success, d.partial, d.failed])
            .collect();
        let total = lines.len();
        lines.sort_unstable();
        lines.dedup();
        assert_eq!(lines.len(), total, "two mutations share a terminal line");
        for d in all {
            assert!(!d.partial.contains("ready") && !d.partial.contains("were "));
        }
    }

    /// A duration no other test uses, so the report it stamps can be found.
    fn unique_elapsed() -> std::time::Duration {
        std::time::Duration::from_millis(uuid::Uuid::new_v4().as_u128() as u64 % 1_000_000_000)
    }

    #[tokio::test]
    async fn an_answered_operation_reports_after_the_answer_not_before() {
        let recorder = crate::telemetry::recorder::install();
        let elapsed = unique_elapsed();
        let ms = elapsed.as_millis() as u64;

        let began = std::time::Instant::now();
        report_op(
            "up",
            &Ok(json!({"status": "failed", "error": {"code": "SPAWN_FAILED"}})),
            elapsed,
        );

        assert!(
            began.elapsed() < crate::telemetry::recorder::HOLD,
            "the report held the answer"
        );
        let reports = recorder.wait_for(|p| p["duration_ms"] == ms).await;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].0, crate::telemetry::OP_FINISHED);
        assert_eq!(reports[0].1["op"], "up");
        assert_eq!(reports[0].1["outcome"], "failed");
        assert_eq!(reports[0].1["error_kind"], "SPAWN_FAILED");
    }

    #[tokio::test]
    async fn a_cancelled_operation_reports_as_cancelled_with_its_code() {
        let recorder = crate::telemetry::recorder::install();
        let elapsed = unique_elapsed();
        let ms = elapsed.as_millis() as u64;

        report_op(
            "restart",
            &Err(Error::Remote {
                code: "OPERATION_CANCELLED".to_string(),
                message: "a message that must never be reported".to_string(),
                stacktrace: None,
            }),
            elapsed,
        );

        let reports = recorder.wait_for(|p| p["duration_ms"] == ms).await;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].1["op"], "restart");
        assert_eq!(reports[0].1["outcome"], "cancelled");
        assert_eq!(reports[0].1["error_kind"], "OPERATION_CANCELLED");
        assert!(!reports[0].1.to_string().contains("must never"));
    }

    #[tokio::test]
    async fn a_plain_success_reports_ok_with_no_error_kind() {
        let recorder = crate::telemetry::recorder::install();
        let elapsed = unique_elapsed();
        let ms = elapsed.as_millis() as u64;

        report_op("down", &Ok(json!({"changed": true})), elapsed);

        let reports = recorder.wait_for(|p| p["duration_ms"] == ms).await;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].1["outcome"], "ok");
        assert!(reports[0].1["error_kind"].is_null());
    }

    #[test]
    fn mutations_take_a_progress_stream_and_the_terminal_it_reaches() {
        for function_id in [
            "compose::add",
            "compose::update",
            "compose::remove",
            "compose::build",
            "compose::up",
            "compose::down",
            "compose::restart",
        ] {
            let schema = schema_entry(function_id).1.as_ref().unwrap();
            let stderr = &schema["properties"]["stderr"];
            assert_eq!(stderr["x-iii-stream"], "write", "{function_id}");
            assert_eq!(
                stderr["properties"]["direction"]["const"], "write",
                "{function_id}"
            );
            assert_eq!(
                schema["properties"]["terminal"]["x-iii-terminal"], true,
                "{function_id}"
            );
            let mut fields: Vec<&str> = schema["properties"]["terminal"]["properties"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            fields.sort_unstable();
            assert_eq!(fields, ["color", "columns", "rows", "stderr", "stdout"]);
        }
        // The stream is optional: a caller without one gets the answer it got before.
        let add = schema_entry("compose::add").1.as_ref().unwrap();
        let validator = jsonschema::Validator::new(add).unwrap();
        assert!(validator.is_valid(&json!({ "worker": "state" })));
    }

    #[test]
    fn reads_take_no_stream_and_logs_takes_two() {
        for function_id in [
            "compose::status",
            "compose::list",
            "compose::ps",
            "compose::validate",
            "compose::schema",
            "compose::stop",
        ] {
            let schema = schema_entry(function_id).1.as_ref().unwrap();
            for field in ["stdout", "stderr", "terminal"] {
                assert!(
                    schema["properties"].get(field).is_none(),
                    "{function_id} {field}"
                );
            }
        }
        let logs = schema_entry("compose::logs").1.as_ref().unwrap();
        for field in ["stdout", "stderr"] {
            assert_eq!(
                logs["properties"][field]["x-iii-stream"], "write",
                "{field}"
            );
        }
        assert_eq!(logs["properties"]["terminal"]["x-iii-terminal"], true);
        assert!(logs["properties"]["follow"].is_object());
    }

    fn accepted() -> OperationAcceptedOutcome {
        OperationAcceptedOutcome {
            operation_id: "compose:test".into(),
            status: "accepted".into(),
            requested: 1,
            outcome: None,
        }
    }

    /// How a drawn add answers a success.
    fn succeeded(outcome: MutationOutcome) -> Value {
        to_value(&OperationAcceptedOutcome {
            status: "succeeded".into(),
            outcome: Some(outcome),
            ..accepted()
        })
    }

    fn outcome(status: crate::lifecycle::OpStatus, changed: bool) -> MutationOutcome {
        MutationOutcome::from_operations(
            status,
            changed,
            Some("state"),
            None,
            None,
            std::iter::empty::<&crate::lifecycle::OpResult>(),
        )
    }

    #[test]
    fn a_drawn_add_that_succeeded_answers_with_its_outcome() {
        let (ending, answer) = drawn_answer(
            Ok(outcome(crate::lifecycle::OpStatus::Ok, true)),
            ADD_DETAILS,
            succeeded,
        );
        assert_eq!(ending, Ending::Succeeded(ADD_DETAILS.success.into()));
        let answer = answer.unwrap();
        assert_eq!(answer["operation_id"], "compose:test");
        assert_eq!(answer["status"], "succeeded");
        assert_eq!(answer["outcome"]["worker"], "state");
        assert_eq!(answer["outcome"]["changed"], true);
    }

    #[test]
    fn a_drawn_up_answers_with_the_outcome_it_answers_without_a_stream() {
        let up = outcome(crate::lifecycle::OpStatus::Ok, false);
        let (ending, answer) =
            drawn_answer(Ok(up.clone()), UP_DETAILS, |outcome| to_value(&outcome));
        assert_eq!(ending, Ending::Succeeded(UP_DETAILS.unchanged.into()));
        assert_eq!(answer.unwrap(), to_value(&up));
    }

    #[test]
    fn a_drawn_add_that_changed_nothing_says_so() {
        let (ending, answer) = drawn_answer(
            Ok(outcome(crate::lifecycle::OpStatus::Ok, false)),
            ADD_DETAILS,
            succeeded,
        );
        assert_eq!(ending, Ending::Succeeded(ADD_DETAILS.unchanged.into()));
        assert!(answer.is_ok());
    }

    #[test]
    fn a_drawn_add_that_failed_fails_the_call() {
        let (ending, answer) = drawn_answer(
            Ok(outcome(crate::lifecycle::OpStatus::Failed, true)),
            ADD_DETAILS,
            succeeded,
        );
        assert_eq!(ending, Ending::Failed(ADD_DETAILS.failed.into()));
        match answer {
            Err(Error::Remote { code, message, .. }) => {
                assert_eq!(code, "OPERATION_FAILED");
                assert_eq!(message, ADD_DETAILS.failed);
            }
            other => panic!("expected a remote error, got {other:?}"),
        }
    }

    #[test]
    fn a_drawn_add_that_was_cancelled_fails_with_its_code() {
        let (ending, answer) = drawn_answer(
            Err(ComposeError::OperationCancelled {
                operation_id: "compose:test".into(),
            }),
            ADD_DETAILS,
            succeeded,
        );
        assert_eq!(ending, Ending::Cancelled("operation cancelled".into()));
        match answer {
            Err(Error::Remote { code, .. }) => assert_eq!(code, "OPERATION_CANCELLED"),
            other => panic!("expected a remote error, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod timeout_tests;
