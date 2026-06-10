// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Host-side daemon. Registers `worker::*` SDK triggers; each handler
//! routes through the same `crate::core::*::run` + `CliHostShim` adapter
//! that backs `iii worker <cmd>`, so a remote `iii.trigger("worker::add",
//! ...)` and a local `iii worker add foo` exercise the same body.
//!
//! On top of the callable surface, the daemon also registers the
//! `worker` custom trigger type so other workers can subscribe to
//! lifecycle events via `iii.register_trigger("worker", config, fn)`.
//! Each mutating op uses an `IIIEventSink` (replacing the historical
//! `NullSink`) that fans `WorkerOpEvent`s out to matching subscribers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::core::{
    AddOptions, AddOutcome, ClearOptions, ClearOutcome, EventSink, ListOptions, ListOutcome,
    LogsOptions, LogsOutcome, NullSink, ProjectCtx, RemoveOptions, RemoveOutcome, StartOptions,
    StartOutcome, StopOptions, StopOutcome, UpdateOptions, UpdateOutcome, WorkerOpError,
    WorkerOpErrorKind, add as core_add, clear as core_clear, list as core_list, logs as core_logs,
    remove as core_remove, start as core_start, stop as core_stop, update as core_update,
};
use iii_observability::OtelConfig;
use iii_sdk::{
    III, IIIError, InitOptions, RegisterFunction, RegisterTriggerType, WorkerMetadata,
    register_worker,
};
use schemars::{JsonSchema, schema_for};
use serde_json::{Value, json};

use crate::cli::app::WorkerManagerDaemonArgs;
use crate::cli::host_shim::CliHostShim;
use crate::cli::worker_trigger::{
    IIIEventSink, Subscriptions, WorkerCallRequest, WorkerTriggerConfig, WorkerTriggerHandler,
};
use crate::core::add::CallerMode;

pub async fn run(args: WorkerManagerDaemonArgs) -> i32 {
    // FIRST statement, before any await: snapshot spawn-time facts (current
    // ppid + III_ENGINE_PID). The exit-watch is polled only after SDK init +
    // ~10 registrations; if the engine died in that window we'd baseline
    // against the ADOPTER and never notice (cross-model review finding).
    let exit_watch = crate::daemon_exit::ExitWatch::arm_at_startup();

    let project_root = args
        .project_root
        .or_else(|| std::env::var_os("IIIWORKER_PROJECT_ROOT").map(Into::into))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    tracing::info!(url = %args.engine, ?project_root, "connecting to III engine");

    let iii = register_worker(
        &args.engine,
        InitOptions {
            otel: Some(OtelConfig::default()),
            metadata: Some(WorkerMetadata {
                name: "iii-worker-ops".to_string(),
                description: Some(
                    "Manages installed workers: add/remove/update/start/stop/list/clear/logs, \
                     plus worker::schema introspection and the `worker` lifecycle \
                     trigger type."
                        .to_string(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    // Register the `worker` trigger type and build a fan-out sink that
    // shares the same subscription map. The handler stores subscriber
    // configs; the sink reads them when an op emits a `WorkerOpEvent`.
    let subs: Subscriptions = Arc::new(Mutex::new(HashMap::new()));
    iii.register_trigger_type(
        RegisterTriggerType::new(
            "worker",
            "Worker lifecycle events emitted by every worker::* op. \
             Subscribe with `operations` / `stages` / `workers` filters.",
            WorkerTriggerHandler::new(subs.clone()),
        )
        .trigger_request_format::<WorkerTriggerConfig>()
        .call_request_format::<WorkerCallRequest>(),
    );
    let event_sink: Arc<IIIEventSink> =
        Arc::new(IIIEventSink::new(iii.clone(), subs, CallerMode::Trigger));

    register_all(&iii, project_root, event_sink);

    tracing::info!("worker-manager-daemon ready");

    // Exit on SIGINT/SIGTERM/SIGHUP or engine death — see crate::daemon_exit
    // for the full design (PID handshake + hardened reparent fallback).
    // shutdown_async is a best-effort flush; the connection thread is not
    // joined before exit.
    let reason = exit_watch.wait("worker-manager-daemon").await;
    tracing::info!(reason, "worker-manager-daemon shutting down");
    iii.shutdown_async().await;
    0
}

#[doc(hidden)]
pub fn err_payload(e: &WorkerOpError) -> String {
    serde_json::to_string(&e.to_payload()).unwrap_or_else(|_| e.to_string())
}

/// Map a serde failure into the W105 envelope so bad payloads return the
/// same `{ type, code, details }` shape as handler-level errors. The
/// envelope carries a `hint` pointing at `worker::schema` so callers (LLMs
/// included) can self-correct without out-of-band docs.
#[doc(hidden)]
pub fn bad_request_payload(function_id: &str, e: &serde_json::Error) -> String {
    let err = WorkerOpError::BadRequest {
        function_id: function_id.into(),
        reason: e.to_string(),
    };
    err_payload(&err)
}

/// Surface op failures as `IIIError::Remote` so the wire `ErrorBody.code`
/// is the stable W-code (instead of the generic `invocation_failed`) and no
/// dispatch-loop backtrace gets attached to an expected error.
fn op_error(e: &WorkerOpError) -> IIIError {
    IIIError::Remote {
        code: e.kind().code().to_string(),
        message: err_payload(e),
        stacktrace: None,
    }
}

fn bad_request_error(function_id: &str, e: &serde_json::Error) -> IIIError {
    IIIError::Remote {
        code: WorkerOpErrorKind::BadRequest.code().to_string(),
        message: bad_request_payload(function_id, e),
        stacktrace: None,
    }
}

fn schema_for_value<T: JsonSchema>() -> Option<Value> {
    serde_json::to_value(schema_for!(T)).ok()
}

/// One-line description per op. Single source of truth shared by the
/// function registrations (surfaced via `engine::functions::info`) and the
/// `worker::schema` response.
#[doc(hidden)]
pub fn op_description(function_id: &str) -> &'static str {
    match function_id {
        "worker::add" => "Install a worker from registry name or OCI ref",
        "worker::remove" => "Uninstall workers and clear their artifacts",
        "worker::update" => "Reinstall workers preserving config",
        "worker::start" => "Start a configured worker",
        "worker::stop" => "Stop a running worker",
        "worker::list" => "List installed workers",
        "worker::clear" => "Wipe worker artifacts",
        "worker::logs" => {
            "Read a worker's recent stdout/stderr log lines from the engine host. \
             `tail` bounds lines per stream (default 100, max 1000)."
        }
        "worker::schema" => {
            "Introspect request/response schemas for worker::* triggers. \
             Optional `function_id` filters to a single trigger."
        }
        _ => "",
    }
}

/// Stamp the standard introspection contract onto a registration:
/// description plus timeout/idempotency metadata. The request/response JSON
/// Schemas come from the SDK's typed-handler auto-extraction (the handlers
/// take the typed options structs directly), so together with this stamp
/// `engine::functions::info { function_id: "worker::add" }` is
/// self-sufficient for callers that have never seen this API.
fn describe_op(rf: RegisterFunction, function_id: &str) -> RegisterFunction {
    let (default_timeout_ms, idempotent) = op_metadata(function_id);
    rf.description(op_description(function_id)).metadata(json!({
        "default_timeout_ms": default_timeout_ms,
        "idempotent": idempotent,
    }))
}

fn register_all(iii: &III, project_root: PathBuf, sink: Arc<IIIEventSink>) {
    register_add(iii, project_root.clone(), sink.clone());
    register_remove(iii, project_root.clone(), sink.clone());
    register_update(iii, project_root.clone(), sink.clone());
    register_start(iii, project_root.clone(), sink.clone());
    register_stop(iii, project_root.clone(), sink.clone());
    register_list(iii, project_root.clone());
    register_clear(iii, project_root, sink);
    register_logs(iii);
    register_schema(iii);
}

#[derive(serde::Deserialize, JsonSchema)]
struct SchemaRequest {
    /// Trigger id to introspect (e.g. `"worker::add"`). Omit to return all.
    #[serde(default)]
    function_id: Option<String>,
}

#[derive(serde::Serialize, JsonSchema)]
struct SchemaEntry {
    function_id: String,
    description: String,
    request: serde_json::Value,
    response: serde_json::Value,
    /// Recommended client timeout. `add`/`update` exceed the SDK 30s default.
    default_timeout_ms: u64,
    /// Safe to retry on the same payload. `false` = stateful (start/stop).
    idempotent: bool,
}

/// (default_timeout_ms, idempotent). Mirrors the table in the
/// worker-management-triggers doc (currently
/// `docs/0-11-0/workers/worker-management-triggers.mdx`).
///
/// `idempotent` describes the DEFAULT request: `worker::add`/`worker::update`
/// are idempotent only when `force`/`reset_config` are unset. With `force:
/// true` they stop the worker, delete artifacts, and re-run the manifest's
/// install scripts — so an automation/LLM that retries a forced call based on
/// this flag re-runs those side effects. Retry-on-timeout is safe for the
/// default shape, not for forced replacement.
#[doc(hidden)]
pub fn op_metadata(function_id: &str) -> (u64, bool) {
    match function_id {
        "worker::add" => (600_000, true),
        "worker::remove" => (30_000, true),
        "worker::update" => (600_000, true),
        "worker::start" => (60_000, false),
        "worker::stop" => (30_000, false),
        "worker::list" => (10_000, true),
        "worker::clear" => (30_000, true),
        "worker::logs" => (10_000, true),
        "worker::schema" => (10_000, true),
        _ => (30_000, false),
    }
}

#[derive(serde::Serialize, JsonSchema)]
struct SchemaResponse {
    schemas: Vec<SchemaEntry>,
}

/// The 9 worker::* (function_id, request, response) schema triples, built
/// once via schemars reflection and reused on every `worker::schema` call.
/// Regenerating all 16 schemas per invocation was wasted CPU/allocation on
/// an endpoint LLM/automation callers hit repeatedly; only the matched
/// entries are cloned per request.
fn schema_table() -> &'static [(&'static str, Option<Value>, Option<Value>)] {
    static TABLE: std::sync::LazyLock<Vec<(&'static str, Option<Value>, Option<Value>)>> =
        std::sync::LazyLock::new(|| {
            vec![
                (
                    "worker::add",
                    schema_for_value::<AddOptions>(),
                    schema_for_value::<AddOutcome>(),
                ),
                (
                    "worker::remove",
                    schema_for_value::<RemoveOptions>(),
                    schema_for_value::<RemoveOutcome>(),
                ),
                (
                    "worker::update",
                    schema_for_value::<UpdateOptions>(),
                    schema_for_value::<UpdateOutcome>(),
                ),
                (
                    "worker::start",
                    schema_for_value::<StartOptions>(),
                    schema_for_value::<StartOutcome>(),
                ),
                (
                    "worker::stop",
                    schema_for_value::<StopOptions>(),
                    schema_for_value::<StopOutcome>(),
                ),
                (
                    "worker::list",
                    schema_for_value::<ListOptions>(),
                    schema_for_value::<ListOutcome>(),
                ),
                (
                    "worker::clear",
                    schema_for_value::<ClearOptions>(),
                    schema_for_value::<ClearOutcome>(),
                ),
                (
                    "worker::logs",
                    schema_for_value::<LogsOptions>(),
                    schema_for_value::<LogsOutcome>(),
                ),
                (
                    "worker::schema",
                    schema_for_value::<SchemaRequest>(),
                    schema_for_value::<SchemaResponse>(),
                ),
            ]
        });
    &TABLE
}

fn register_logs(iii: &III) {
    let _ =
        iii.register_function(
            "worker::logs",
            describe_op(
                RegisterFunction::new_async_with_bad_request(
                    |opts: LogsOptions| async move {
                        core_logs::run(opts).await.map_err(|e| op_error(&e))
                    },
                    |e| bad_request_error("worker::logs", &e),
                ),
                "worker::logs",
            ),
        );
}

fn register_schema(iii: &III) {
    let rf = RegisterFunction::new_async_with_bad_request(
        |req: SchemaRequest| async move {
            let filter = req.function_id.as_deref();
            let schemas: Vec<SchemaEntry> = schema_table()
                .iter()
                .filter(|(id, _, _)| filter.is_none_or(|f| f == *id))
                .map(|(id, req, resp)| {
                    let (timeout_ms, idempotent) = op_metadata(id);
                    SchemaEntry {
                        function_id: (*id).into(),
                        description: op_description(id).into(),
                        request: req.clone().unwrap_or(Value::Null),
                        response: resp.clone().unwrap_or(Value::Null),
                        default_timeout_ms: timeout_ms,
                        idempotent,
                    }
                })
                .collect();
            Ok::<_, IIIError>(SchemaResponse { schemas })
        },
        |e| bad_request_error("worker::schema", &e),
    );
    let _ = iii.register_function("worker::schema", describe_op(rf, "worker::schema"));
}

fn sink_ref<'a>(sink: &'a Arc<IIIEventSink>) -> &'a dyn EventSink {
    // `IIIEventSink` is the only mutating-op sink today, but the
    // orchestrators take `&dyn EventSink` — this helper makes the
    // coercion site explicit at every call site.
    &**sink
}

fn register_add(iii: &III, project_root: PathBuf, sink: Arc<IIIEventSink>) {
    let _ = iii.register_function(
        "worker::add",
        describe_op(
            RegisterFunction::new_async_with_bad_request(
                move |opts: AddOptions| {
                    let project_root = project_root.clone();
                    let sink = sink.clone();
                    async move {
                        let ctx = ProjectCtx::open(project_root).map_err(|e| op_error(&e))?;
                        core_add::run(opts, &ctx, sink_ref(&sink), &CliHostShim)
                            .await
                            .map_err(|e| op_error(&e))
                    }
                },
                |e| bad_request_error("worker::add", &e),
            ),
            "worker::add",
        ),
    );
}

fn register_remove(iii: &III, project_root: PathBuf, sink: Arc<IIIEventSink>) {
    let _ = iii.register_function(
        "worker::remove",
        describe_op(
            RegisterFunction::new_async_with_bad_request(
                move |opts: RemoveOptions| {
                    let project_root = project_root.clone();
                    let sink = sink.clone();
                    async move {
                        let ctx = ProjectCtx::open(project_root).map_err(|e| op_error(&e))?;
                        core_remove::run(opts, &ctx, sink_ref(&sink), &CliHostShim)
                            .await
                            .map_err(|e| op_error(&e))
                    }
                },
                |e| bad_request_error("worker::remove", &e),
            ),
            "worker::remove",
        ),
    );
}

fn register_update(iii: &III, project_root: PathBuf, sink: Arc<IIIEventSink>) {
    let _ = iii.register_function(
        "worker::update",
        describe_op(
            RegisterFunction::new_async_with_bad_request(
                move |opts: UpdateOptions| {
                    let project_root = project_root.clone();
                    let sink = sink.clone();
                    async move {
                        let ctx = ProjectCtx::open(project_root).map_err(|e| op_error(&e))?;
                        core_update::run(opts, &ctx, sink_ref(&sink), &CliHostShim)
                            .await
                            .map_err(|e| op_error(&e))
                    }
                },
                |e| bad_request_error("worker::update", &e),
            ),
            "worker::update",
        ),
    );
}

fn register_start(iii: &III, project_root: PathBuf, sink: Arc<IIIEventSink>) {
    let _ = iii.register_function(
        "worker::start",
        describe_op(
            RegisterFunction::new_async_with_bad_request(
                move |opts: StartOptions| {
                    let project_root = project_root.clone();
                    let sink = sink.clone();
                    async move {
                        let ctx = ProjectCtx::open(project_root).map_err(|e| op_error(&e))?;
                        core_start::run(opts, &ctx, sink_ref(&sink), &CliHostShim)
                            .await
                            .map_err(|e| op_error(&e))
                    }
                },
                |e| bad_request_error("worker::start", &e),
            ),
            "worker::start",
        ),
    );
}

fn register_stop(iii: &III, project_root: PathBuf, sink: Arc<IIIEventSink>) {
    let _ = iii.register_function(
        "worker::stop",
        describe_op(
            RegisterFunction::new_async_with_bad_request(
                move |opts: StopOptions| {
                    let project_root = project_root.clone();
                    let sink = sink.clone();
                    async move {
                        let ctx = ProjectCtx::open(project_root).map_err(|e| op_error(&e))?;
                        core_stop::run(opts, &ctx, sink_ref(&sink), &CliHostShim)
                            .await
                            .map_err(|e| op_error(&e))
                    }
                },
                |e| bad_request_error("worker::stop", &e),
            ),
            "worker::stop",
        ),
    );
}

fn register_list(iii: &III, project_root: PathBuf) {
    // `Option<ListOptions>` keeps the lenient default: a `null` payload
    // deserializes to `None` (→ defaults) and `{}` to `Some(default)`, while
    // any other malformed shape still fails deserialization and returns the
    // W105 envelope so the caller can tell typos apart from "no args".
    let rf = RegisterFunction::new_async_with_bad_request(
        move |opts: Option<ListOptions>| {
            let project_root = project_root.clone();
            async move {
                let ctx = ProjectCtx::open_unlocked(project_root);
                core_list::run(opts.unwrap_or_default(), &ctx, &NullSink, &CliHostShim)
                    .await
                    .map_err(|e| op_error(&e))
            }
        },
        |e| bad_request_error("worker::list", &e),
    );
    // `Option<T>` auto-extracts a nullable wrapper schema; override with the
    // plain `ListOptions` schema so `engine::functions::info` serves the same
    // bytes as `worker::schema`.
    let mut rf = describe_op(rf, "worker::list");
    if let Some(schema) = schema_for_value::<ListOptions>() {
        rf = rf.request_format(schema);
    }
    let _ = iii.register_function("worker::list", rf);
}

fn register_clear(iii: &III, project_root: PathBuf, sink: Arc<IIIEventSink>) {
    let _ = iii.register_function(
        "worker::clear",
        describe_op(
            RegisterFunction::new_async_with_bad_request(
                move |opts: ClearOptions| {
                    let project_root = project_root.clone();
                    let sink = sink.clone();
                    async move {
                        let ctx = ProjectCtx::open(project_root).map_err(|e| op_error(&e))?;
                        core_clear::run(opts, &ctx, sink_ref(&sink), &CliHostShim)
                            .await
                            .map_err(|e| op_error(&e))
                    }
                },
                |e| bad_request_error("worker::clear", &e),
            ),
            "worker::clear",
        ),
    );
}

