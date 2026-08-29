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

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use iii_sdk::{Error, RegisterFunction};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    daemon::Daemon,
    error::ComposeError,
    lifecycle::{OpResult, OpStatus},
    logs::{LogCursor, LogStream, LogsOutcome},
    project::ContainerStatus,
};

/// Payload accepted by every `compose::*` function. All fields optional: the
/// bare call is the common one.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
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
    /// Canonical root worker-compose.yaml path for local stack operations.
    pub path: Option<String>,
    /// Named stack within the root worker-compose.yaml.
    pub stack: Option<String>,
    /// Wait for readiness before returning. Defaults to true.
    pub wait: Option<bool>,
    /// Restrict the operation to one container and what it needs.
    pub container: Option<String>,
    /// Compatibility alias for the container selected by restart or logs.
    pub worker: Option<String>,
    /// Last cursor returned for each selected worker.
    pub cursors: Option<BTreeMap<String, LogCursor>>,
    /// Number of recent lines returned when no cursor is supplied.
    pub tail: Option<usize>,
    /// Restrict logs to stdout or stderr.
    pub stream: Option<LogStream>,
    /// Long-poll budget. Bounded by the daemon.
    pub wait_ms: Option<u64>,
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
    /// Root worker-compose.yaml on the daemon host.
    path: String,
    /// Named stack. Omit for `default`, or when the file has one stack.
    stack: Option<String>,
    /// Wait for worker readiness before returning. Defaults to true.
    wait: Option<bool>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
struct ValidateOptions {
    namespace: Option<String>,
    path: String,
    stack: Option<String>,
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

#[allow(dead_code)]
#[derive(Debug, Clone, JsonSchema)]
struct RestartOutcome {
    status: OpStatus,
    container: Option<String>,
    changed: bool,
    restarted: Option<OpResult>,
    down: Option<OpResult>,
    up: Option<OpResult>,
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
    let client = daemon.engine().client();

    for &(name, kind) in REGISTERED_OPERATIONS {
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
    Restart,
    Schema,
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
    ("restart", Operation::Restart),
    ("schema", Operation::Schema),
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
    let stack_path = request.path.as_ref().map(std::path::PathBuf::from);

    match operation {
        Operation::Up => match daemon
            .up_stack(
                required_path(stack_path.as_deref(), "compose::up")?,
                request.stack.as_deref(),
                None,
                operation_id(),
            )
            .await
        {
            Ok(result) => Ok(to_value(&result)),
            Err(err) => Err(compose_error(&err)),
        },
        Operation::Restart => match daemon
            .restart(
                file.as_deref(),
                // Either spelling names the same thing here.
                request.container.as_deref().or(request.worker.as_deref()),
                operation_id(),
            )
            .await
        {
            Ok(result) => Ok(result),
            Err(err) => Err(compose_error(&err)),
        },
        Operation::Down => match daemon
            .down_stack(
                required_path(stack_path.as_deref(), "compose::down")?,
                request.stack.as_deref(),
                None,
                operation_id(),
            )
            .await
        {
            Ok(result) => Ok(to_value(&result)),
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
        Operation::Validate => match daemon
            .validate_stack(
                required_path(stack_path.as_deref(), "compose::validate")?,
                request.stack.as_deref(),
            )
            .await
        {
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
    }
}

fn required_path<'a>(
    path: Option<&'a std::path::Path>,
    function: &str,
) -> Result<Option<&'a std::path::Path>, Error> {
    path.map(Some).ok_or_else(|| Error::Remote {
        code: "INVALID_COMPOSE_REQUEST".to_string(),
        message: format!("{function} requires path=<worker-compose.yaml>"),
        stacktrace: None,
    })
}

/// Serialize the generated root schema into the value carried over the wire.
fn schema_for_value<T: JsonSchema>() -> Option<Value> {
    serde_json::to_value(schema_for!(T)).ok()
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
        "compose::restart" => {
            "Restart the whole project, or restart one named container without \
             changing its dependency graph."
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
        "compose::restart" => (600_000, false),
        "compose::schema" | "worker-compose.yaml" => (10_000, true),
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
                schema_for_value::<OpResult>(),
            ),
            (
                "compose::down",
                schema_for_value::<LifecycleOptions>(),
                schema_for_value::<OpResult>(),
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
                schema_for_value::<ValidateOptions>(),
                schema_for_value::<ValidateOutcome>(),
            ),
            (
                "compose::restart",
                schema_for_value::<RestartOptions>(),
                schema_for_value::<RestartOutcome>(),
            ),
            (
                "compose::schema",
                schema_for_value::<SchemaRequest>(),
                schema_for_value::<SchemaResponse>(),
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
    fn operation_requests_expose_only_supported_fields() {
        let (_, up, _) = schema_entry("compose::up");
        let up = up.as_ref().unwrap()["properties"].as_object().unwrap();
        assert!(up.contains_key("namespace"));
        assert!(up.contains_key("path"));
        assert!(up.contains_key("stack"));
        assert!(up.contains_key("wait"));
        assert!(!up.contains_key("worker"));

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
    fn compose_file_pseudo_entry_carries_schema_and_example() {
        let (_, request, response) = schema_entry("worker-compose.yaml");
        let request = request.as_ref().unwrap();
        assert_eq!(request["type"], "object");
        assert!(request["properties"]["workers"].is_object());
        assert!(request["properties"]["stacks"].is_object());

        let response = response.as_ref().unwrap();
        assert!(
            response["worker-compose.yaml"]
                .as_str()
                .is_some_and(|text| text.contains("containers:"))
        );
    }
}
