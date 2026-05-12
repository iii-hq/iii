// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Host-side daemon. Registers `worker::*` SDK triggers; each handler
//! routes through the same `crate::core::*::run` + `CliHostShim` adapter
//! that backs `iii worker <cmd>`, so a remote `iii.trigger("worker::add",
//! ...)` and a local `iii worker add foo` exercise the same body.

use std::path::PathBuf;

use crate::core::{
    AddOptions, AddOutcome, ClearOptions, ClearOutcome, ListOptions, ListOutcome, NullSink,
    ProjectCtx, RemoveOptions, RemoveOutcome, StartOptions, StartOutcome, StopOptions, StopOutcome,
    UpdateOptions, UpdateOutcome, WorkerOpError, add as core_add, clear as core_clear,
    list as core_list, remove as core_remove, start as core_start, stop as core_stop,
    update as core_update,
};
use iii_sdk::{III, InitOptions, OtelConfig, RegisterFunction, WorkerMetadata, register_worker};
use schemars::{JsonSchema, schema_for};
use serde_json::Value;

use crate::cli::app::WorkerManagerDaemonArgs;
use crate::cli::host_shim::CliHostShim;

pub async fn run(args: WorkerManagerDaemonArgs) -> i32 {
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
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    register_all(&iii, project_root);

    tracing::info!("worker-manager-daemon ready");
    if let Err(e) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %e, "ctrl_c handler failed");
        iii.shutdown_async().await;
        return 1;
    }
    tracing::info!("worker-manager-daemon shutting down");
    iii.shutdown_async().await;
    0
}

#[doc(hidden)]
pub fn err_payload(e: &WorkerOpError) -> String {
    serde_json::to_string(&e.to_payload()).unwrap_or_else(|_| e.to_string())
}

/// Map a serde failure into the W101 envelope so bad payloads return the
/// same `{ type, code, details }` shape as handler-level errors.
#[doc(hidden)]
pub fn bad_request_payload(input_label: &str, e: &serde_json::Error) -> String {
    let err = WorkerOpError::InvalidSource {
        input: input_label.into(),
        reason: e.to_string(),
    };
    err_payload(&err)
}

fn schema_for_value<T: JsonSchema>() -> Option<Value> {
    serde_json::to_value(schema_for!(T)).ok()
}

fn register_all(iii: &III, project_root: PathBuf) {
    register_add(iii, project_root.clone());
    register_remove(iii, project_root.clone());
    register_update(iii, project_root.clone());
    register_start(iii, project_root.clone());
    register_stop(iii, project_root.clone());
    register_list(iii, project_root.clone());
    register_clear(iii, project_root);
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

/// (default_timeout_ms, idempotent). Mirrors the table in
/// `docs/workers/worker-management-triggers.mdx`.
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
        "worker::schema" => (10_000, true),
        _ => (30_000, false),
    }
}

#[derive(serde::Serialize, JsonSchema)]
struct SchemaResponse {
    schemas: Vec<SchemaEntry>,
}

fn register_schema(iii: &III) {
    let _ = iii.register_function(
        RegisterFunction::new_async("worker::schema", |payload: Value| async move {
            let req: SchemaRequest = serde_json::from_value(payload)
                .map_err(|e| bad_request_payload("worker::schema", &e))?;
            let all = vec![
                (
                    "worker::add",
                    "Install a worker from registry name or OCI ref",
                    schema_for_value::<AddOptions>(),
                    schema_for_value::<AddOutcome>(),
                ),
                (
                    "worker::remove",
                    "Uninstall workers and clear their artifacts",
                    schema_for_value::<RemoveOptions>(),
                    schema_for_value::<RemoveOutcome>(),
                ),
                (
                    "worker::update",
                    "Reinstall workers preserving config",
                    schema_for_value::<UpdateOptions>(),
                    schema_for_value::<UpdateOutcome>(),
                ),
                (
                    "worker::start",
                    "Start a configured worker",
                    schema_for_value::<StartOptions>(),
                    schema_for_value::<StartOutcome>(),
                ),
                (
                    "worker::stop",
                    "Stop a running worker",
                    schema_for_value::<StopOptions>(),
                    schema_for_value::<StopOutcome>(),
                ),
                (
                    "worker::list",
                    "List installed workers",
                    schema_for_value::<ListOptions>(),
                    schema_for_value::<ListOutcome>(),
                ),
                (
                    "worker::clear",
                    "Wipe worker artifacts",
                    schema_for_value::<ClearOptions>(),
                    schema_for_value::<ClearOutcome>(),
                ),
                (
                    "worker::schema",
                    "Introspect request/response schemas for worker::* triggers",
                    schema_for_value::<SchemaRequest>(),
                    schema_for_value::<SchemaResponse>(),
                ),
            ];
            let filter = req.function_id.as_deref();
            let schemas: Vec<SchemaEntry> = all
                .into_iter()
                .filter(|(id, _, _, _)| filter.map_or(true, |f| f == *id))
                .map(|(id, desc, req, resp)| {
                    let (timeout_ms, idempotent) = op_metadata(id);
                    SchemaEntry {
                        function_id: id.into(),
                        description: desc.into(),
                        request: req.unwrap_or(Value::Null),
                        response: resp.unwrap_or(Value::Null),
                        default_timeout_ms: timeout_ms,
                        idempotent,
                    }
                })
                .collect();
            Ok::<_, String>(SchemaResponse { schemas })
        })
        .description(
            "Introspect request/response schemas for worker::* triggers. \
             Optional `function_id` filters to a single trigger.",
        ),
    );
}

fn register_add(iii: &III, project_root: PathBuf) {
    let _ = iii.register_function(
        RegisterFunction::new_async("worker::add", move |payload: Value| {
            let project_root = project_root.clone();
            async move {
                let opts: AddOptions = serde_json::from_value(payload)
                    .map_err(|e| bad_request_payload("worker::add", &e))?;
                let ctx = ProjectCtx::open(project_root).map_err(|e| err_payload(&e))?;
                core_add::run(
                    opts,
                    &ctx,
                    &NullSink,
                    &CliHostShim,
                    core_add::CallerMode::Trigger,
                )
                .await
                .map_err(|e| err_payload(&e))
            }
        })
        .description("Install a worker from registry name or OCI ref"),
    );
}

fn register_remove(iii: &III, project_root: PathBuf) {
    let _ = iii.register_function(
        RegisterFunction::new_async("worker::remove", move |payload: Value| {
            let project_root = project_root.clone();
            async move {
                let opts: RemoveOptions = serde_json::from_value(payload)
                    .map_err(|e| bad_request_payload("worker::remove", &e))?;
                let ctx = ProjectCtx::open(project_root).map_err(|e| err_payload(&e))?;
                core_remove::run(opts, &ctx, &NullSink, &CliHostShim)
                    .await
                    .map_err(|e| err_payload(&e))
            }
        })
        .description("Uninstall workers and clear their artifacts"),
    );
}

fn register_update(iii: &III, project_root: PathBuf) {
    let _ = iii.register_function(
        RegisterFunction::new_async("worker::update", move |payload: Value| {
            let project_root = project_root.clone();
            async move {
                let opts: UpdateOptions = serde_json::from_value(payload)
                    .map_err(|e| bad_request_payload("worker::update", &e))?;
                let ctx = ProjectCtx::open(project_root).map_err(|e| err_payload(&e))?;
                core_update::run(opts, &ctx, &NullSink, &CliHostShim)
                    .await
                    .map_err(|e| err_payload(&e))
            }
        })
        .description("Reinstall workers preserving config"),
    );
}

fn register_start(iii: &III, project_root: PathBuf) {
    let _ = iii.register_function(
        RegisterFunction::new_async("worker::start", move |payload: Value| {
            let project_root = project_root.clone();
            async move {
                let opts: StartOptions = serde_json::from_value(payload)
                    .map_err(|e| bad_request_payload("worker::start", &e))?;
                let ctx = ProjectCtx::open(project_root).map_err(|e| err_payload(&e))?;
                core_start::run(opts, &ctx, &NullSink, &CliHostShim)
                    .await
                    .map_err(|e| err_payload(&e))
            }
        })
        .description("Start a configured worker"),
    );
}

fn register_stop(iii: &III, project_root: PathBuf) {
    let _ = iii.register_function(
        RegisterFunction::new_async("worker::stop", move |payload: Value| {
            let project_root = project_root.clone();
            async move {
                let opts: StopOptions = serde_json::from_value(payload)
                    .map_err(|e| bad_request_payload("worker::stop", &e))?;
                let ctx = ProjectCtx::open(project_root).map_err(|e| err_payload(&e))?;
                core_stop::run(opts, &ctx, &NullSink, &CliHostShim)
                    .await
                    .map_err(|e| err_payload(&e))
            }
        })
        .description("Stop a running worker"),
    );
}

fn register_list(iii: &III, project_root: PathBuf) {
    let _ = iii.register_function(
        RegisterFunction::new_async("worker::list", move |payload: Value| {
            let project_root = project_root.clone();
            async move {
                // Lenient default only for null or empty object. Other shapes
                // must deserialize cleanly or return the W101 envelope so the
                // caller can tell typos apart from "no args".
                let opts: ListOptions = match &payload {
                    Value::Null => ListOptions::default(),
                    Value::Object(map) if map.is_empty() => ListOptions::default(),
                    _ => serde_json::from_value(payload)
                        .map_err(|e| bad_request_payload("worker::list", &e))?,
                };
                let ctx = ProjectCtx::open_unlocked(project_root);
                core_list::run(opts, &ctx, &NullSink, &CliHostShim)
                    .await
                    .map_err(|e| err_payload(&e))
            }
        })
        .description("List installed workers"),
    );
}

fn register_clear(iii: &III, project_root: PathBuf) {
    let _ = iii.register_function(
        RegisterFunction::new_async("worker::clear", move |payload: Value| {
            let project_root = project_root.clone();
            async move {
                let opts: ClearOptions = serde_json::from_value(payload)
                    .map_err(|e| bad_request_payload("worker::clear", &e))?;
                let ctx = ProjectCtx::open(project_root).map_err(|e| err_payload(&e))?;
                core_clear::run(opts, &ctx, &NullSink, &CliHostShim)
                    .await
                    .map_err(|e| err_payload(&e))
            }
        })
        .description("Wipe worker artifacts"),
    );
}
