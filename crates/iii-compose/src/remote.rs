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
//! iii trigger compose::up --namespace dev file=./worker-compose.yaml
//! ```
//!
//! The flag picks the daemon; `file=` picks the project. Nothing else names a
//! project — a name someone chose would be a second identity for the same
//! thing, and one that can be pointed at the wrong file.
//!
//! `list` and `stop` are about the daemon itself and take no file. The rest
//! fall back to `worker-compose.yaml` in the daemon's own directory, and say
//! so when there is none.

use std::{collections::BTreeMap, future::Future, path::PathBuf, sync::Arc};

use iii_sdk::{Error, RegisterFunction};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    daemon::{Daemon, MutationOutcome},
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
    /// The worker a call is about.
    ///
    /// `compose::update` reads a spec: `name`, `name@version`, or a path.
    /// `compose::remove` and `compose::restart` read a container key, where it
    /// is the spelling for `container` — an operator naming a worker should not
    /// have to know which of the two words this call wanted. `compose::add` and
    /// `compose::remove` keep this field as a compatibility alias for a single
    /// list item.
    pub worker: Option<String>,
    /// Workers to add or remove in one file edit and one reconciliation.
    ///
    /// This is the canonical JSON field for both operations. The singular
    /// `worker` field remains accepted for clients that used the old contract.
    pub workers: Option<Vec<String>>,
    /// Caller-selected operation id used to bind mutation progress before submission.
    pub operation_id: Option<String>,
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
}

/// Request fields used by daemon-wide operations.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct DaemonOptions {
    /// Which daemon the caller believed it reached. This is a guard, not a
    /// route; use the trigger `--namespace` flag to select the daemon.
    namespace: Option<String>,
}

/// Request fields used by project lifecycle operations.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct LifecycleOptions {
    /// Optional daemon guard. Use the trigger `--namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to `worker-compose.yaml` in
    /// the daemon working directory.
    file: Option<String>,
    /// Restrict the lifecycle operation to one container.
    container: Option<String>,
}

/// Request fields used by project read operations.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct ProjectOptions {
    /// Optional daemon guard. Use the trigger `--namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to `worker-compose.yaml` in
    /// the daemon working directory.
    file: Option<String>,
}

/// Request fields used by batch worker mutations.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct BatchWorkerOptions {
    /// Optional daemon guard. Use the trigger `--namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to `worker-compose.yaml` in
    /// the daemon working directory.
    file: Option<String>,
    /// Canonical list of workers to mutate together. Add accepts worker names,
    /// `name@version` references, registry references, or local paths. Remove
    /// accepts declared worker keys.
    workers: Option<Vec<String>>,
    /// Backward-compatible form for mutating one worker.
    worker: Option<String>,
    /// Caller-selected operation id for race-free progress subscription.
    operation_id: Option<String>,
}

/// Request fields used by single-worker compose-file edits.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct WorkerOptions {
    /// Optional daemon guard. Use the trigger `--namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to `worker-compose.yaml` in
    /// the daemon working directory.
    file: Option<String>,
    /// Worker name, `name@version`, registry reference, or local path. The
    /// accepted form depends on the operation.
    worker: String,
    /// Caller-selected operation id for race-free progress subscription.
    operation_id: Option<String>,
}

/// `compose::restart` accepts either spelling for one container.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct RestartOptions {
    /// Optional daemon guard. Use the trigger `--namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to `worker-compose.yaml` in
    /// the daemon working directory.
    file: Option<String>,
    /// Container key to restart.
    container: Option<String>,
    /// Alias for `container`.
    worker: Option<String>,
}

/// Request fields used by `compose::logs`.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct LogsOptions {
    /// Optional daemon guard. Use the trigger `--namespace` flag to route.
    namespace: Option<String>,
    /// Compose file on the daemon host. Defaults to `worker-compose.yaml` in
    /// the daemon working directory.
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
    /// Optional daemon guard. Use the trigger `--namespace` flag to route.
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
    projects: Vec<ProjectSummary>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, JsonSchema)]
struct StatusOutcome {
    namespace: String,
    file: PathBuf,
    state_dir: PathBuf,
    daemon_pid: u32,
    containers: Vec<ContainerStatus>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, JsonSchema)]
struct ValidateOutcome {
    namespace: String,
    start_order: Vec<String>,
    deferred_packages: Vec<String>,
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
    status: String,
    requested: usize,
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
            async move { dispatch(daemon, kind, guard_name, request).await }
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
    Logs,
    Stop,
    Validate,
    Add,
    Remove,
    Restart,
    Update,
    Schema,
    Snapshot,
    Cancel,
}

impl Operation {
    fn is_mutation(self) -> bool {
        matches!(
            self,
            Self::Up | Self::Down | Self::Add | Self::Remove | Self::Restart | Self::Update
        )
    }
}

/// Canonical list of functions exposed by the compose daemon.
const REGISTERED_OPERATIONS: &[(&str, Operation)] = &[
    ("up", Operation::Up),
    ("down", Operation::Down),
    ("list", Operation::List),
    ("status", Operation::Status),
    ("logs", Operation::Logs),
    ("stop", Operation::Stop),
    ("validate", Operation::Validate),
    ("add", Operation::Add),
    ("remove", Operation::Remove),
    ("restart", Operation::Restart),
    ("update", Operation::Update),
    ("schema", Operation::Schema),
    ("operation", Operation::Snapshot),
    ("cancel", Operation::Cancel),
];

async fn dispatch(
    daemon: Arc<Daemon>,
    operation: Operation,
    function: String,
    request: ComposeRequest,
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
                 not a payload field: iii trigger compose::{function} --namespace {addressed} …",
                daemon.daemon_namespace
            ),
            stacktrace: None,
        });
    }

    let file = request.file.as_ref().map(std::path::PathBuf::from);

    match operation {
        Operation::Up => match daemon
            .up(
                file.as_deref(),
                request.container.as_deref(),
                operation_id(),
            )
            .await
        {
            Ok(result) => Ok(to_value(&MutationOutcome::from_operations(
                result.status,
                result.changed,
                request.container.as_deref(),
                None,
                None,
                std::iter::once(&result),
            ))),
            Err(err) => Err(compose_error(&err)),
        },
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

            let daemon_task = Arc::clone(&daemon);
            let task_operation_id = accepted.operation_id.clone();
            let task_operation = Arc::clone(&operation);
            let mutation = async move {
                task_operation
                    .emit(None, "resolving", "resolving dependency trees")
                    .await;
                daemon_task
                    .add(file.as_deref(), &workers, task_operation_id)
                    .await
            };
            spawn_mutation(
                operation,
                mutation,
                "all requested workers are ready",
                "one or more workers failed",
            );
            Ok(to_value(&accepted))
        }
        Operation::Remove => {
            let workers = requested_workers(request.workers, request.worker);
            if workers.is_empty() {
                return daemon
                    .remove(file.as_deref(), &[], operation_id())
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

            let daemon_task = Arc::clone(&daemon);
            let task_operation_id = accepted.operation_id.clone();
            let task_operation = Arc::clone(&operation);
            let mutation = async move {
                task_operation
                    .emit(None, "removing", format!("removing {requested} workers"))
                    .await;
                daemon_task
                    .remove(file.as_deref(), &workers, task_operation_id)
                    .await
            };
            spawn_mutation(
                operation,
                mutation,
                "all requested workers were removed",
                "one or more workers could not be removed",
            );
            Ok(to_value(&accepted))
        }
        Operation::Restart => match daemon
            .restart(
                file.as_deref(),
                // Either spelling names the same thing here.
                request.container.as_deref().or(request.worker.as_deref()),
                operation_id(),
            )
            .await
        {
            Ok(result) => Ok(to_value(&result)),
            Err(err) => Err(compose_error(&err)),
        },
        Operation::Update => {
            let Some(worker) = request.worker else {
                return daemon
                    .update(file.as_deref(), None, operation_id())
                    .await
                    .map(|outcome| to_value(&outcome))
                    .map_err(|err| compose_error(&err));
            };
            let (operation, accepted) = admit_mutation(
                &daemon,
                request.operation_id,
                1,
                format!("updating worker {worker}"),
            )
            .await?;

            let daemon_task = Arc::clone(&daemon);
            let task_operation_id = accepted.operation_id.clone();
            let task_operation = Arc::clone(&operation);
            let mutation = async move {
                task_operation
                    .emit(None, "resolving", format!("resolving update for {worker}"))
                    .await;
                daemon_task
                    .update(file.as_deref(), Some(&worker), task_operation_id)
                    .await
            };
            spawn_mutation(
                operation,
                mutation,
                "worker updated",
                "worker update failed",
            );
            Ok(to_value(&accepted))
        }
        Operation::Down => match daemon
            .down(
                file.as_deref(),
                request.container.as_deref(),
                operation_id(),
            )
            .await
        {
            Ok(result) => Ok(to_value(&MutationOutcome::from_operations(
                result.status,
                result.changed,
                request.container.as_deref(),
                None,
                None,
                std::iter::once(&result),
            ))),
            Err(err) => Err(compose_error(&err)),
        },
        // Every project this daemon holds. No id: this is the call an operator
        // makes when they have forgotten what is running.
        Operation::List => Ok(json!({
            "daemon": daemon.worker_name,
            // Which machine answered. Several daemons share an engine, so a
            // caller who reached one by namespace can confirm it was the one
            // they meant.
            "daemon_namespace": daemon.daemon_namespace,
            "daemon_pid": std::process::id(),
            "projects": daemon.list().await,
        })),
        Operation::Status => match daemon.status(file.as_deref()).await {
            Ok(project) => Ok(json!({
                "namespace": project.project_namespace,
                "file": project.file_path(),
                // Derived from the file, so nobody can guess it: where this
                // project's state, delivered config and container logs live.
                "state_dir": project.state_dir(),
                // Which process is answering. `--detach` waits on this: another
                // daemon would otherwise answer for the one being launched.
                "daemon_pid": std::process::id(),
                "containers": project.status().await,
            })),
            Err(err) => Err(compose_error(&err)),
        },
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
        // runs the same teardown a signal would.
        Operation::Stop => Ok(daemon.request_stop().await),
        // Validation is a question about a file, so it holds nothing: naming a
        // file here must not leave the daemon owning a project, and must not
        // write the durable state that would bind that id to it.
        Operation::Validate => match daemon.validate(file.as_deref()).await {
            Ok(report) => Ok(json!({
                "namespace": report.namespace,
                "start_order": report.start_order,
                "deferred_packages": report.deferred_packages,
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
    };
    Ok((operation, accepted))
}

fn spawn_mutation<F>(
    operation: Arc<crate::operation::Operation>,
    mutation: F,
    success_detail: &'static str,
    failed_detail: &'static str,
) where
    F: Future<Output = Result<MutationOutcome, ComposeError>> + Send + 'static,
{
    tokio::spawn(async move {
        if operation.is_cancelled() {
            operation
                .finish(
                    crate::operation::OperationStatus::Cancelled,
                    "operation cancelled",
                )
                .await;
            return;
        }

        match mutation.await {
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
                            failed_detail
                        } else {
                            success_detail
                        },
                    )
                    .await;
            }
            Err(ComposeError::OperationCancelled { .. }) => {
                operation
                    .finish(
                        crate::operation::OperationStatus::Cancelled,
                        "operation cancelled",
                    )
                    .await;
            }
            Err(error) => {
                operation
                    .finish(crate::operation::OperationStatus::Failed, error.to_string())
                    .await;
            }
        }
    });
}

/// Serialize the generated root schema into the value carried over the wire.
fn schema_for_value<T: JsonSchema>() -> Option<Value> {
    serde_json::to_value(schema_for!(T)).ok()
}

fn requested_workers(workers: Option<Vec<String>>, worker: Option<String>) -> Vec<String> {
    workers
        .or_else(|| worker.map(|worker| vec![worker]))
        .unwrap_or_default()
}

/// Batch worker mutations accept their canonical list or the old singular field.
///
/// Both fields are optional in the Rust shape so they can share the common
/// namespace and file properties. `anyOf` states the runtime rule that at
/// least one input form must be present. Supplying both is valid; dispatch
/// gives the canonical `workers` list precedence.
fn batch_worker_options_schema() -> Option<Value> {
    let mut schema = schema_for_value::<BatchWorkerOptions>()?;
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

/// One source of truth for function registration and `compose::schema`.
fn op_description(function_id: &str) -> &'static str {
    match function_id {
        "compose::up" => {
            "Start a compose project, or one container and its dependencies. \
             Repeated calls leave ready containers running."
        }
        "compose::down" => {
            "Stop a compose project, or one container and its dependents, in \
             reverse dependency order."
        }
        "compose::list" => "List every project loaded by this compose daemon.",
        "compose::status" => {
            "Report the project namespace, state directory, daemon pid, and \
             current state of every declared container."
        }
        "compose::logs" => {
            "Read bounded worker stdout and stderr. A cursor continues from the last response; \
             callers may long-poll for new output."
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
             dependencies in the compose file, pins resolved versions, then reconciles changed \
             workers once."
        }
        "compose::remove" => {
            "Accept an observable operation that removes one or more declared workers and \
             dependency references to them, stops only those workers, then reconciles anything \
             already missing."
        }
        "compose::restart" => {
            "Restart the whole project, or restart one named container without \
             changing its dependency graph."
        }
        "compose::update" => {
            "Accept an observable operation that moves one declared package worker to a \
             requested or latest version, then restarts the project."
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
        "compose::logs" => (10_000, true),
        "compose::stop" => (30_000, false),
        "compose::validate" => (10_000, true),
        "compose::add" => (600_000, false),
        "compose::remove" => (600_000, true),
        "compose::restart" => (600_000, false),
        "compose::update" => (600_000, false),
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
                schema_for_value::<LifecycleOptions>(),
                schema_for_value::<MutationOutcome>(),
            ),
            (
                "compose::down",
                schema_for_value::<LifecycleOptions>(),
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
                "compose::logs",
                schema_for_value::<LogsOptions>(),
                schema_for_value::<LogsOutcome>(),
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
                batch_worker_options_schema(),
                schema_for_value::<OperationAcceptedOutcome>(),
            ),
            (
                "compose::remove",
                batch_worker_options_schema(),
                schema_for_value::<OperationAcceptedOutcome>(),
            ),
            (
                "compose::restart",
                schema_for_value::<RestartOptions>(),
                schema_for_value::<MutationOutcome>(),
            ),
            (
                "compose::update",
                schema_for_value::<WorkerOptions>(),
                schema_for_value::<OperationAcceptedOutcome>(),
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

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or_else(|err| json!({ "error": err.to_string() }))
}

fn operation_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Some(vec!["database".to_string(), "web".to_string()])
        );
        assert_eq!(listed.worker, None);

        let singular: ComposeRequest = serde_json::from_value(json!({
            "worker": "database",
        }))
        .expect("the old singular field should deserialize");
        assert_eq!(singular.worker.as_deref(), Some("database"));
        assert_eq!(singular.workers, None);

        assert_eq!(
            requested_workers(None, singular.worker),
            vec!["database".to_string()]
        );
        assert_eq!(
            requested_workers(listed.workers, Some("ignored".to_string())),
            vec!["database".to_string(), "web".to_string()]
        );
    }

    #[test]
    fn batch_worker_schemas_require_one_non_null_non_empty_worker_form() {
        for function_id in ["compose::add", "compose::remove"] {
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
                json!({}),
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
    fn operation_requests_expose_only_supported_fields() {
        let (_, up, _) = schema_entry("compose::up");
        let up = up.as_ref().unwrap()["properties"].as_object().unwrap();
        assert!(up.contains_key("namespace"));
        assert!(up.contains_key("file"));
        assert!(up.contains_key("container"));
        assert!(!up.contains_key("worker"));

        for function_id in ["compose::add", "compose::remove"] {
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

        let (_, update, _) = schema_entry("compose::update");
        let update = update.as_ref().unwrap()["properties"].as_object().unwrap();
        for field in ["namespace", "file", "worker", "operation_id"] {
            assert!(
                update.contains_key(field),
                "compose::update is missing {field}"
            );
        }
        assert!(!update.contains_key("container"));
        assert!(!update.contains_key("workers"));

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
}
