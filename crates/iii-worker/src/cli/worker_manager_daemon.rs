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
    StartOutcome, StatusOptions, StatusOutcome, StopOptions, StopOutcome, UpdateOptions,
    UpdateOutcome, WorkerOpError, WorkerOpErrorKind, add as core_add, clear as core_clear,
    list as core_list, logs as core_logs, remove as core_remove, start as core_start,
    stop as core_stop, update as core_update,
};
use iii_helpers::observability::OtelConfig;
use iii_sdk::runtime::WorkerMetadata;
use iii_sdk::{
    Error, IIIClient, InitOptions, RegisterFunction, RegisterTriggerType, register_worker,
};
use schemars::{JsonSchema, schema_for};
use serde_json::{Value, json};

use crate::cli::app::WorkerManagerDaemonArgs;
use crate::cli::host_shim::CliHostShim;
use crate::cli::project::{MAX_LOCAL_MANIFEST_BYTES, WORKER_MANIFEST};
use crate::cli::worker_manifest::{
    ManifestReport, ValidateOptions, hello_world_example_json, manifest_schema_json,
    report_from_str,
};
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
                     plus worker::schema introspection (including the iii.worker.yaml \
                     manifest schema), worker::validate manifest dry-runs, and the \
                     `worker` lifecycle trigger type."
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
    // for the full design (lifeline pipe + PID handshake + hardened reparent
    // fallback). shutdown_async is a best-effort flush; the connection
    // thread is not joined before exit.
    let reason = exit_watch.wait("worker-manager-daemon").await;
    tracing::info!(reason, "worker-manager-daemon shutting down");
    if reason == "engine-gone" {
        // Session reaper: nothing the engine started may outlive it. The
        // engine cannot kill its tree post-mortem (no macOS PDEATHSIG, and
        // workers are setsid'd session leaders), but THIS daemon notices
        // engine death and still has the full host-side stop machinery —
        // handle_managed_stop kills the VM (or binary worker process) AND
        // its source-watcher sidecar per worker, no engine required. VMs
        // also self-watch the engine pid as defense for the case where this
        // daemon was killed first.
        reap_managed_workers().await;
    }
    iii.shutdown_async().await;
    0
}

/// Stop every config.yaml worker, each bounded so one wedged stop can't
/// stall the daemon's own exit. Best-effort by design: the daemon is going
/// down either way, and per-worker failures are logged, not fatal.
async fn reap_managed_workers() {
    let names = crate::cli::config_file::list_worker_names();
    tracing::warn!(
        count = names.len(),
        "engine gone — reaping managed workers before exit"
    );
    for name in names {
        match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            crate::cli::managed::handle_managed_stop(&name),
        )
        .await
        {
            Ok(rc) if rc == 0 => tracing::info!(worker = %name, "reaped"),
            Ok(rc) => tracing::warn!(worker = %name, rc, "reap stop returned nonzero"),
            Err(_) => tracing::warn!(worker = %name, "reap stop timed out after 10s"),
        }
    }
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

/// Surface op failures as `Error::Remote` so the wire `ErrorBody.code`
/// is the stable W-code (instead of the generic `invocation_failed`) and no
/// dispatch-loop backtrace gets attached to an expected error.
fn op_error(e: &WorkerOpError) -> Error {
    Error::Remote {
        code: e.kind().code().to_string(),
        message: err_payload(e),
        stacktrace: None,
    }
}

fn bad_request_error(function_id: &str, e: &serde_json::Error) -> Error {
    Error::Remote {
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
        "worker::add" => {
            "Install a worker from a registry slug, an OCI image ref, or a LOCAL \
             project directory (one containing an iii.worker.yaml). Call \
             worker::schema to see each source shape, and worker::validate to \
             dry-run an iii.worker.yaml before installing. LOCAL installs run \
             the project directory LIVE and start a source watcher: code edits \
             auto-restart the worker, so do NOT re-add after changing source \
             files — re-add (force: true) only when iii.worker.yaml itself \
             changes. Installs can outlive a bus invocation timeout and \
             CONTINUE server-side — on timeout, poll worker::status instead of \
             re-issuing (the project lock stays held until the in-flight \
             install finishes)."
        }
        "worker::remove" => "Uninstall workers and clear their artifacts",
        "worker::update" => {
            "Reinstall REGISTRY-installed workers preserving config (re-resolves \
             iii.lock pins). LOCAL-path workers are NOT touched by this: their \
             source watcher already auto-restarts them on code edits; only an \
             iii.worker.yaml change needs worker::add with force: true. Like \
             worker::add, updates can outlive a bus timeout and continue \
             server-side — poll worker::status."
        }
        "worker::start" => "Start a configured worker",
        "worker::stop" => "Stop a running worker",
        "worker::list" => "List installed workers",
        "worker::status" => {
            "Install/runtime status for ONE worker: installed?, running?, pid, \
             version, a sanitized recent log tail, and a next-step hint. THE poll \
             target after worker::add { wait: false } or after an add/update call \
             hit a bus timeout (the install keeps running server-side)."
        }
        "worker::clear" => "Wipe worker artifacts",
        "worker::logs" => {
            "Read a worker's recent stdout/stderr log lines from the engine host. \
             `tail` bounds lines per stream (default 100, max 1000)."
        }
        "worker::schema" => {
            "Introspect request/response schemas for worker::* triggers. \
             Optional `function_id` filters to a single trigger. Also serves \
             the iii.worker.yaml manifest schema under the pseudo-id \
             \"iii.worker.yaml\"."
        }
        "worker::validate" => {
            "Dry-run validation of an iii.worker.yaml WITHOUT installing \
             anything. Pass `manifest` (inline YAML text) or `path` (host \
             file/dir). Returns errors, unknown keys, deprecated keys, and \
             warnings — author, validate, then worker::add."
        }
        "iii.worker.yaml" => {
            "JSON Schema for the iii.worker.yaml manifest file (not a callable \
             trigger). Fetch via worker::schema { function_id: \
             \"iii.worker.yaml\" }: `request` = the manifest schema, `response` \
             = a complete minimal Node hello-world worker as { path: file \
             contents } ready to write to disk (uses the `iii-sdk` npm \
             package). Check a concrete manifest with worker::validate."
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

fn register_all(iii: &IIIClient, project_root: PathBuf, sink: Arc<IIIEventSink>) {
    register_add(iii, project_root.clone(), sink.clone());
    register_remove(iii, project_root.clone(), sink.clone());
    register_update(iii, project_root.clone(), sink.clone());
    register_start(iii, project_root.clone(), sink.clone());
    register_stop(iii, project_root.clone(), sink.clone());
    register_list(iii, project_root.clone());
    register_clear(iii, project_root, sink);
    register_logs(iii);
    register_schema(iii);
    register_status(iii);
    register_validate(iii);
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
        "worker::status" => (10_000, true),
        "worker::validate" => (10_000, true),
        // Pseudo-id for the manifest schema served by worker::schema; not a
        // callable trigger, but the SchemaEntry row still carries metadata.
        "iii.worker.yaml" => (10_000, true),
        _ => (30_000, false),
    }
}

#[derive(serde::Serialize, JsonSchema)]
struct SchemaResponse {
    schemas: Vec<SchemaEntry>,
}

/// The 10 worker::* (function_id, request, response) schema triples plus the
/// `iii.worker.yaml` pseudo-entry (the manifest file's own JSON Schema, so
/// LLMs can author a manifest from the real contract), built once via
/// schemars reflection and reused on every `worker::schema` call.
/// Regenerating every schema per invocation was wasted CPU/allocation on
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
                (
                    "worker::status",
                    schema_for_value::<StatusOptions>(),
                    schema_for_value::<StatusOutcome>(),
                ),
                (
                    "worker::validate",
                    schema_for_value::<ValidateOptions>(),
                    schema_for_value::<ManifestReport>(),
                ),
                // Pseudo-entry: the iii.worker.yaml authoring contract.
                // request = the manifest's own JSON Schema; response = a
                // complete minimal Node hello-world worker as { path → file
                // contents }, ready to write to disk. Fetch via
                // worker::schema { function_id: "iii.worker.yaml" }.
                (
                    "iii.worker.yaml",
                    Some(manifest_schema_json()),
                    Some(hello_world_example_json()),
                ),
            ]
        });
    &TABLE
}

fn register_logs(iii: &IIIClient) {
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

fn register_schema(iii: &IIIClient) {
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
            Ok::<_, Error>(SchemaResponse { schemas })
        },
        |e| bad_request_error("worker::schema", &e),
    );
    let _ = iii.register_function("worker::schema", describe_op(rf, "worker::schema"));
}

/// Compose one worker's status from host-side observables: config.yaml
/// declaration, process liveness, lockfile version, and a sanitized log
/// tail. Born from a harness session where, after `worker::add` timed out at
/// the bus gate, the caller had NO way to tell "still installing" from
/// "crashed" (worker::list only says running:false) and went spelunking with
/// shell commands instead.
async fn build_status(name: &str) -> Result<StatusOutcome, WorkerOpError> {
    use crate::cli::config_file::{self, ResolvedWorkerType};
    use crate::cli::managed::{find_worker_pid_from_ps, is_engine_running, is_worker_running};

    crate::core::types::validate_worker_name(name).map_err(|reason| WorkerOpError::BadRequest {
        function_id: "worker::status".into(),
        reason,
    })?;

    let installed = config_file::worker_exists(name);
    let worker_type = if !installed {
        "not-installed".to_string()
    } else {
        match config_file::resolve_worker_type(name) {
            ResolvedWorkerType::Oci { .. } => "oci".to_string(),
            ResolvedWorkerType::Local { .. } => "local".to_string(),
            ResolvedWorkerType::Binary { .. } => "binary".to_string(),
            ResolvedWorkerType::Bundle { .. } => "bundle".to_string(),
            ResolvedWorkerType::Config => "builtin".to_string(),
        }
    };

    let worker_running = is_worker_running(name);
    // Engine builtins run inside the engine process; count them as running
    // when the engine is up (mirrors CliHostShim::list / `iii worker list`).
    let running = worker_running || (installed && worker_type == "builtin" && is_engine_running());
    let pid = find_worker_pid_from_ps(name);
    let version =
        crate::cli::lockfile::WorkerLockfile::read_from(crate::cli::lockfile::lockfile_path())
            .ok()
            .and_then(|lf| lf.workers.get(name).map(|w| w.version.clone()));

    let logs = core_logs::run(LogsOptions {
        name: name.to_string(),
        tail: 20,
        raw: false,
    })
    .await?;

    let hint = if !installed {
        "not in config.yaml — install with worker::add (dry-run the manifest first \
         with worker::validate)"
            .to_string()
    } else if running {
        format!("running; see its functions with engine::functions::list {{ search: {name:?} }}")
    } else if logs.stderr.is_empty() && logs.stdout.is_empty() {
        "installed but not running and no logs yet — likely still provisioning; \
         poll again shortly, or worker::start to boot it"
            .to_string()
    } else {
        "installed but not running — check stderr_tail/stdout_tail for the failure \
         (e.g. a dependency install error), fix the worker source, then \
         worker::add { force: true } to reinstall or worker::start to retry"
            .to_string()
    };

    Ok(StatusOutcome {
        name: name.to_string(),
        installed,
        worker_type,
        running,
        pid,
        version,
        logs_dir: logs.logs_dir,
        stderr_tail: logs.stderr,
        stdout_tail: logs.stdout,
        hint,
    })
}

fn register_status(iii: &IIIClient) {
    let rf = RegisterFunction::new_async_with_bad_request(
        |opts: StatusOptions| async move { build_status(&opts.name).await.map_err(|e| op_error(&e)) },
        |e| bad_request_error("worker::status", &e),
    );
    let _ = iii.register_function("worker::status", describe_op(rf, "worker::status"));
}

/// `worker::validate` — dry-run an `iii.worker.yaml` without installing.
/// Read-only and side-effect free: the LLM/automation loop is author →
/// validate → fix → `worker::add`. Accepts exactly one of `manifest` (inline
/// YAML text, preferred for authoring) or `path` (host file or worker dir,
/// resolved like `worker::add { kind: "local" }`).
fn register_validate(iii: &IIIClient) {
    let rf = RegisterFunction::new_async_with_bad_request(
        |opts: ValidateOptions| async move {
            let report: ManifestReport = match (opts.manifest, opts.path) {
                (Some(_), Some(_)) | (None, None) => {
                    return Err(op_error(&WorkerOpError::BadRequest {
                        function_id: "worker::validate".into(),
                        reason: "pass exactly one of `manifest` (inline YAML) or `path` \
                                 (host file or worker directory)"
                            .into(),
                    }));
                }
                (Some(text), None) => report_from_str(&text),
                (None, Some(p)) => {
                    let file = if p.is_dir() {
                        p.join(WORKER_MANIFEST)
                    } else {
                        p
                    };
                    // Stat-first so a huge file is rejected without reading it
                    // into memory (same cap the add path enforces).
                    match std::fs::metadata(&file) {
                        Ok(meta) if meta.len() > MAX_LOCAL_MANIFEST_BYTES => {
                            let mut r = ManifestReport::default();
                            r.errors.push(format!(
                                "{} is {} bytes; iii.worker.yaml is capped at \
                                 {MAX_LOCAL_MANIFEST_BYTES} bytes",
                                file.display(),
                                meta.len(),
                            ));
                            r
                        }
                        Ok(_) => match std::fs::read_to_string(&file) {
                            Ok(content) => report_from_str(&content),
                            Err(e) => {
                                let mut r = ManifestReport::default();
                                r.errors
                                    .push(format!("cannot read {}: {e}", file.display()));
                                r
                            }
                        },
                        Err(e) => {
                            let mut r = ManifestReport::default();
                            r.errors.push(format!(
                                "cannot stat {} on the engine/daemon host: {e}",
                                file.display(),
                            ));
                            r
                        }
                    }
                }
            };
            Ok::<_, Error>(report)
        },
        |e| bad_request_error("worker::validate", &e),
    );
    let _ = iii.register_function("worker::validate", describe_op(rf, "worker::validate"));
}

fn sink_ref(sink: &Arc<IIIEventSink>) -> &dyn EventSink {
    // `IIIEventSink` is the only mutating-op sink today, but the
    // orchestrators take `&dyn EventSink` — this helper makes the
    // coercion site explicit at every call site.
    &**sink
}

fn register_add(iii: &IIIClient, project_root: PathBuf, sink: Arc<IIIEventSink>) {
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

fn register_remove(iii: &IIIClient, project_root: PathBuf, sink: Arc<IIIEventSink>) {
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

fn register_update(iii: &IIIClient, project_root: PathBuf, sink: Arc<IIIEventSink>) {
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

fn register_start(iii: &IIIClient, project_root: PathBuf, sink: Arc<IIIEventSink>) {
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

fn register_stop(iii: &IIIClient, project_root: PathBuf, sink: Arc<IIIEventSink>) {
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

fn register_list(iii: &IIIClient, project_root: PathBuf) {
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

fn register_clear(iii: &IIIClient, project_root: PathBuf, sink: Arc<IIIEventSink>) {
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

#[cfg(test)]
mod tests {
    use super::super::test_support::lock_home;
    use super::*;

    #[test]
    fn op_description_documents_the_manifest_pseudo_id() {
        let desc = op_description("iii.worker.yaml");

        assert!(desc.contains("JSON Schema"));
        assert!(desc.contains("worker::schema"));
        assert!(desc.contains("worker::validate"));
    }

    fn schema_entry(id: &str) -> &'static (&'static str, Option<Value>, Option<Value>) {
        schema_table()
            .iter()
            .find(|(entry_id, _, _)| *entry_id == id)
            .unwrap_or_else(|| panic!("schema_table is missing {id}"))
    }

    #[test]
    fn schema_table_serves_status_and_validate_entries() {
        let (_, status_req, status_resp) = schema_entry("worker::status");
        let (_, validate_req, validate_resp) = schema_entry("worker::validate");

        let status_props = status_req.as_ref().unwrap()["properties"]
            .as_object()
            .unwrap();
        assert!(status_props.contains_key("name"));
        assert!(status_resp.as_ref().unwrap()["properties"]["hint"].is_object());

        let validate_props = validate_req.as_ref().unwrap()["properties"]
            .as_object()
            .unwrap();
        assert!(validate_props.contains_key("manifest"));
        assert!(validate_props.contains_key("path"));
        assert!(validate_resp.as_ref().unwrap()["properties"]["errors"].is_object());
    }

    #[test]
    fn schema_table_manifest_pseudo_entry_carries_schema_and_example() {
        let (_, req, resp) = schema_entry("iii.worker.yaml");

        assert_eq!(req.as_ref().unwrap(), &manifest_schema_json());
        assert_eq!(resp.as_ref().unwrap(), &hello_world_example_json());
    }

    // `IIIClient` exposes no registry listing, so the duplicate-registration
    // panic is the only externally observable proof that `register_all`
    // claimed a function id.
    fn offline_iii_with_sink() -> (IIIClient, Arc<IIIEventSink>) {
        let iii = IIIClient::new("ws://127.0.0.1:9");
        let subs: Subscriptions = Arc::new(Mutex::new(HashMap::new()));
        let sink = Arc::new(IIIEventSink::new(iii.clone(), subs, CallerMode::Trigger));
        (iii, sink)
    }

    #[tokio::test]
    #[should_panic(expected = "function id 'worker::status' already registered")]
    async fn register_all_claims_the_worker_status_id() {
        let (iii, sink) = offline_iii_with_sink();
        register_all(&iii, PathBuf::from("."), sink);

        iii.register_function(
            "worker::status",
            RegisterFunction::new_async(|input: Value| async move { Ok::<_, Error>(input) }),
        );
    }

    #[tokio::test]
    #[should_panic(expected = "function id 'worker::validate' already registered")]
    async fn register_all_claims_the_worker_validate_id() {
        let (iii, sink) = offline_iii_with_sink();
        register_all(&iii, PathBuf::from("."), sink);

        iii.register_function(
            "worker::validate",
            RegisterFunction::new_async(|input: Value| async move { Ok::<_, Error>(input) }),
        );
    }

    struct EnvGuard {
        home: Option<std::ffi::OsString>,
        cwd: Option<PathBuf>,
    }

    impl EnvGuard {
        fn new(home: &std::path::Path) -> Self {
            let original_home = std::env::var_os("HOME");
            let original_cwd = std::env::current_dir().ok();
            // SAFETY: test-only, serialized via test_support::lock_home.
            unsafe { std::env::set_var("HOME", home) };
            std::env::set_current_dir(home).unwrap();
            Self {
                home: original_home,
                cwd: original_cwd,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(cwd) = &self.cwd {
                let _ = std::env::set_current_dir(cwd);
            }
            // SAFETY: test-only, serialized via test_support::lock_home.
            unsafe {
                match &self.home {
                    Some(v) => std::env::set_var("HOME", v),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }

    #[tokio::test]
    async fn build_status_rejects_traversal_worker_name() {
        let err = build_status("../evil").await.unwrap_err();

        assert_eq!(err.kind(), WorkerOpErrorKind::BadRequest);
        assert!(matches!(
            err,
            WorkerOpError::BadRequest { ref function_id, .. }
                if function_id == "worker::status"
        ));
    }

    #[tokio::test]
    async fn build_status_reports_not_installed_worker() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().unwrap();
        let _env = EnvGuard::new(tmp.path());

        let s = build_status("wmd-ghost").await.unwrap();

        assert!(!s.installed);
        assert_eq!(s.worker_type, "not-installed");
        assert!(!s.running);
        assert!(s.pid.is_none());
        assert!(s.version.is_none());
        assert!(s.logs_dir.is_none());
        assert!(s.hint.contains("worker::add"));
    }

    #[tokio::test]
    async fn build_status_reports_running_binary_worker() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().unwrap();
        let _env = EnvGuard::new(tmp.path());

        let name = "wmd-bin";
        std::fs::write(
            tmp.path().join("config.yaml"),
            format!("workers:\n  - name: {name}\n"),
        )
        .unwrap();
        let workers_dir = tmp.path().join(".iii/workers");
        std::fs::create_dir_all(&workers_dir).unwrap();
        std::fs::write(workers_dir.join(name), b"").unwrap();
        let pids = tmp.path().join(".iii/pids");
        std::fs::create_dir_all(&pids).unwrap();
        // Our own pid makes the kill(pid, 0) liveness check succeed.
        std::fs::write(
            pids.join(format!("{name}.pid")),
            std::process::id().to_string(),
        )
        .unwrap();

        let s = build_status(name).await.unwrap();

        assert!(s.installed);
        assert_eq!(s.worker_type, "binary");
        assert!(s.running);
        assert!(s.hint.contains("engine::functions::list"));
    }

    #[tokio::test]
    async fn build_status_hints_provisioning_for_local_worker_without_logs() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().unwrap();
        let _env = EnvGuard::new(tmp.path());

        let name = "wmd-local";
        std::fs::write(
            tmp.path().join("config.yaml"),
            format!("workers:\n  - name: {name}\n    worker_path: /tmp/{name}\n"),
        )
        .unwrap();

        let s = build_status(name).await.unwrap();

        assert!(s.installed);
        assert_eq!(s.worker_type, "local");
        assert!(!s.running);
        assert!(s.hint.contains("still provisioning"));
    }

    #[tokio::test]
    async fn build_status_surfaces_stderr_tail_and_version_for_failed_oci_worker() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().unwrap();
        let _env = EnvGuard::new(tmp.path());

        let name = "wmd-oci";
        std::fs::write(
            tmp.path().join("config.yaml"),
            format!("workers:\n  - name: {name}\n    image: ghcr.io/acme/{name}:1\n"),
        )
        .unwrap();
        let logs = tmp.path().join(".iii/logs").join(name);
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("stderr.log"), "npm ERR! boom\n").unwrap();
        let digest = "a".repeat(64);
        std::fs::write(
            tmp.path().join("iii.lock"),
            format!(
                "version: 1\nworkers:\n  {name}:\n    version: 9.9.9\n    type: image\n    \
                 source:\n      kind: image\n      image: ghcr.io/acme/{name}@sha256:{digest}\n"
            ),
        )
        .unwrap();

        let s = build_status(name).await.unwrap();

        assert!(s.installed);
        assert_eq!(s.worker_type, "oci");
        assert!(!s.running);
        assert_eq!(s.version.as_deref(), Some("9.9.9"));
        assert!(s.logs_dir.is_some());
        assert_eq!(s.stderr_tail, vec!["npm ERR! boom".to_string()]);
        assert!(s.hint.contains("stderr_tail"));
    }

    #[tokio::test]
    async fn build_status_classifies_bundle_worker() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().unwrap();
        let _env = EnvGuard::new(tmp.path());

        let name = "wmd-bundle";
        std::fs::write(
            tmp.path().join("config.yaml"),
            format!("workers:\n  - name: {name}\n"),
        )
        .unwrap();
        let bundle = tmp.path().join(".iii/workers-bundle").join(name);
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(bundle.join("iii.worker.yaml"), format!("name: {name}\n")).unwrap();

        let s = build_status(name).await.unwrap();

        assert!(s.installed);
        assert_eq!(s.worker_type, "bundle");
        assert!(!s.running);
    }

    #[tokio::test]
    async fn build_status_counts_builtin_as_running_when_engine_port_listens() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().unwrap();
        let _env = EnvGuard::new(tmp.path());

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let name = "wmd-builtin";
        std::fs::write(
            tmp.path().join("config.yaml"),
            format!(
                "workers:\n  - name: iii-worker-manager\n    config:\n      port: {port}\n  - name: {name}\n"
            ),
        )
        .unwrap();

        let s = build_status(name).await.unwrap();

        assert!(s.installed);
        assert_eq!(s.worker_type, "builtin");
        assert!(
            s.running,
            "builtin worker must count as running while the engine port is up"
        );
        drop(listener);
    }
}
