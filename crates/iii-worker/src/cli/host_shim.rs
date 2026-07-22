// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Production `WorkerHostShim`. Each method adapts a `core::*` op to the
//! existing `handle_managed_*` CLI handler bodies and lifts their stderr +
//! `i32` failure surface into typed `WorkerOpError` envelopes.

use crate::core::{
    AddOptions, AddOutcome, ClearOptions, ClearOutcome, EventSink, ListOptions, ListOutcome,
    ProjectCtx, RemoveOptions, RemoveOutcome, StartOptions, StartOutcome, StopOptions, StopOutcome,
    UpdateOptions, UpdateOutcome, WorkerHostShim, WorkerOpError,
};
use async_trait::async_trait;

pub struct CliHostShim;

fn source_label_name(s: &crate::core::WorkerSource) -> String {
    match s {
        crate::core::WorkerSource::Registry { name, .. } => name.clone(),
        crate::core::WorkerSource::Oci { reference } => reference.clone(),
        crate::core::WorkerSource::Local { path } => path.display().to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Stderr capture for the daemon trigger path.
//
// Why: `handle_managed_*` reports user-facing errors via `eprintln!`. On the
// CLI path the user sees the colored output directly. On the trigger path the
// daemon's stderr is forwarded to the engine's terminal but the trigger
// response only carries `WorkerOpError::Internal { "rc N" }` — the actual
// reason ("Worker 'X' not found", "HTTP 422 ...") is lost to the caller.
//
// Fix: redirect fd 2 to a tempfile during the handle_managed_* call,
// then read it back. Mirror the captured bytes to the original stderr so
// the daemon's log still reflects what happened. Serialized by a Mutex
// because dup2 on fd 2 is process-global.
//
// CRITICAL: skip capture when stderr is a TTY. The CLI path needs stderr
// to flow live to the user — `handle_managed_remove_many` (and others)
// emit interactive prompts like "Continue? [y/N]" before reading stdin.
// Buffering stderr means the user can't see the prompt until after the
// read returns, which makes the prompt unusable. The daemon never has a
// TTY on stderr (engine pipes it), so the daemon path captures normally.
//
// Limitation: while a handler is running, ALL stderr writes from the process
// (other tracing layers, tokio runtime, etc.) get captured into the same
// tempfile. Concurrent handler invocations queue on the mutex. Acceptable
// trade for clear error surfaces; revisit if concurrency becomes hot.
// ─────────────────────────────────────────────────────────────────────────────

static STDERR_CAPTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Returns `true` when fd 2 is a regular file. Used to distinguish the
/// engine-spawned subprocess case (stderr → on-disk log file) from the
/// daemon trigger case (stderr → pipe). On the file path we want stderr
/// to flow through live so the wait UI's `tail:` row sees progress in
/// real time; on the pipe path we still want to capture for clean
/// trigger-response error reporting.
fn stderr_is_regular_file() -> bool {
    use std::os::unix::io::FromRawFd;
    // dup fd 2 so the temporary `File` wrapper's Drop only closes the
    // dup, not the real stderr.
    let dup = unsafe { libc::dup(2) };
    if dup < 0 {
        return false;
    }
    let file = unsafe { std::fs::File::from_raw_fd(dup) };
    let is_regular = file
        .metadata()
        .map(|m| m.file_type().is_file())
        .unwrap_or(false);
    drop(file); // closes `dup`
    is_regular
}

async fn run_capturing_stderr<F, Fut>(f: F) -> (i32, String)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = i32>,
{
    use std::io::{IsTerminal, Read, Seek};
    use std::os::unix::io::AsRawFd;

    // CLI path — stderr is live for the user (interactive prompts, colored
    // progress). Skip capture entirely; let stderr flow through.
    if std::io::stderr().is_terminal() {
        return (f().await, String::new());
    }

    // Engine-spawned subprocess: when the engine reload spawns
    // `iii-worker start <name>` via `ExternalWorkerProcess::spawn`, it
    // redirects the subprocess's stderr to a REGULAR FILE
    // (`~/.iii/logs/<name>/stderr.log`) that the wait UI tails. The
    // original `is_terminal()` check assumed non-tty == daemon-pipe and
    // captured-then-mirrored stderr; that buffered every progress line
    // from `prepare_rootfs` / OCI pull / "Preparing sandbox..." into a
    // tempfile until the handler completed, dumping it all at once at
    // the end and leaving the wait UI's `tail:` row stuck on the first
    // line ("• starting <name>") for minutes. Detect "stderr is a
    // regular file" and let it flow through live too. The SDK trigger
    // path (daemon → engine pipe → stderr=FIFO) still captures, so
    // clean error responses to trigger callers are preserved.
    if stderr_is_regular_file() {
        return (f().await, String::new());
    }

    let _guard = STDERR_CAPTURE_LOCK.lock().await;

    // Snapshot the current stderr fd; restore at the end.
    let orig_stderr = unsafe { libc::dup(2) };
    if orig_stderr < 0 {
        // dup failed; just run without capture.
        return (f().await, String::new());
    }

    // Open a tempfile to receive stderr writes. Use a unique path under
    // the system temp dir so concurrent unrelated processes can't collide.
    let tmp_path = std::env::temp_dir().join(format!(
        "iii-worker-stderr-{}-{}.log",
        std::process::id(),
        uuid::Uuid::new_v4(),
    ));
    let mut tmp = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp_path)
    {
        Ok(f) => f,
        Err(_) => {
            unsafe {
                libc::close(orig_stderr);
            }
            return (f().await, String::new());
        }
    };
    let tmp_fd = tmp.as_raw_fd();

    // Point fd 2 at the tempfile.
    if unsafe { libc::dup2(tmp_fd, 2) } < 0 {
        unsafe {
            libc::close(orig_stderr);
        }
        return (f().await, String::new());
    }

    // Run the inner work. handle_managed_* writes its eprintlns into tmp.
    let rc = f().await;

    // Flush stderr (libc) and stdout's stderr-mirroring sibling so the
    // tempfile sees everything before we read.
    unsafe {
        libc::fflush(std::ptr::null_mut());
    }

    // Restore the original stderr fd.
    unsafe {
        libc::dup2(orig_stderr, 2);
        libc::close(orig_stderr);
    }

    // Read what was captured.
    let mut captured = String::new();
    let _ = tmp.rewind();
    let _ = tmp.read_to_string(&mut captured);

    // Mirror to the real stderr so the daemon's log is unchanged.
    if !captured.is_empty() {
        eprint!("{captured}");
    }

    // Best-effort cleanup of the tempfile.
    let _ = std::fs::remove_file(&tmp_path);

    (rc, captured)
}

/// Pull the most useful error line out of captured stderr. Many handlers
/// print progress lines first ("Resolving X...") and the actual failure on
/// a line starting with "error:". Prefer the first such line; fall back to
/// the last non-empty line; fall back to the raw capture.
fn extract_error_message(captured: &str) -> String {
    if captured.is_empty() {
        return String::new();
    }
    // Strip ANSI color codes for cleaner output in the trigger response.
    let stripped: String = strip_ansi(captured);
    let lines: Vec<&str> = stripped.lines().collect();

    if let Some(line) = lines.iter().find(|l| l.trim_start().starts_with("error:")) {
        return line
            .trim_start()
            .trim_start_matches("error:")
            .trim()
            .to_string();
    }
    lines
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|| stripped.trim().to_string())
}

/// Classify a non-zero rc + stderr capture into a typed `WorkerOpError`.
///
/// The underlying CLI handlers signal failure via i32 + stderr text (no
/// structured errors). For known patterns we lift to typed variants so
/// trigger callers see W110 (NotFound) etc. instead of W900 with a
/// human-readable message that varies across releases.
///
/// `op` describes the operation ("add"/"remove"/...), used in the
/// fallback `Internal` message when no pattern matches. `name_hint` is
/// the worker name (or registry name) the caller passed; surfaced into
/// `details.name` for typed not-found errors when the stderr line does
/// not include a quoted name itself.
#[doc(hidden)]
pub fn classify_handler_error(rc: i32, captured: &str, op: &str, name_hint: &str) -> WorkerOpError {
    let detail = extract_error_message(captured);
    let lower = detail.to_lowercase();

    // Validation rejections (e.g. invalid worker name characters) → W100.
    // The handler's `Worker name '<x>' contains invalid characters` line
    // surfaces a name-shape problem, not an internal failure.
    if lower.contains("invalid characters")
        || lower.contains("validation failed")
        || lower.contains("invalid worker name")
    {
        let name = extract_quoted(&detail).unwrap_or_else(|| name_hint.to_string());
        return WorkerOpError::invalid_name(name, strip_internal_prefix(&detail));
    }

    // "not found" / "could not find" / "no such worker" → W110.
    if lower.contains("not found") || lower.contains("no such") || lower.contains("could not find")
    {
        let name = extract_quoted(&detail).unwrap_or_else(|| name_hint.to_string());
        return WorkerOpError::not_found(name);
    }

    // Everything else stays as W900 but without the "(rc N)" leak.
    let msg = if detail.is_empty() {
        format!("iii worker {op} exited with rc {rc}")
    } else {
        detail
    };
    WorkerOpError::Internal { message: msg }
}

/// True when `e` is the bare rc-only wrapper from [`classify_handler_error`]
/// — produced when the handler ran UNCAPTURED (TTY or log-file stderr), i.e.
/// its real, colored error already reached the user directly. CLI callers
/// should exit nonzero without re-printing it: the wrapper carries nothing
/// the user hasn't seen, and "[W900] internal" mislabels what is usually a
/// plain user error (caught by the live DX audit). Daemon trigger callers
/// still receive it over the bus as a last-resort envelope when stderr
/// capture itself failed.
pub fn is_bare_rc_wrapper(e: &WorkerOpError) -> bool {
    matches!(
        e,
        WorkerOpError::Internal { message }
            if message.starts_with("iii worker ") && message.contains(" exited with rc ")
    )
}

/// Strip a leading `internal: ` from handler output before re-wrapping into
/// a typed validation error. The handler emits a self-describing message
/// already; the prefix is misleading once we route to W100.
fn strip_internal_prefix(s: &str) -> String {
    s.strip_prefix("internal: ").unwrap_or(s).to_string()
}

/// Pull the first single-quoted token out of an error line, e.g.
/// `Worker 'pdfkit' not found` → `pdfkit`. Returns `None` when no
/// single-quoted segment is present.
fn extract_quoted(s: &str) -> Option<String> {
    let mut iter = s.splitn(3, '\'');
    iter.next();
    iter.next().map(|s| s.to_string())
}

fn strip_ansi(s: &str) -> String {
    // Minimal ANSI escape stripper — enough for `colored`'s output.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Consume until 'm' (CSI ... m) or any letter.
            for c2 in chars.by_ref() {
                if c2.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// What the engine reports via `engine::workers::list`: every non-disconnected
/// identifier (worker-reported `name` and engine-side `id` — in-process
/// builtins carry the config entry name as `id`) plus per-identifier versions.
pub(crate) struct EngineWorkerSnapshot {
    pub(crate) names: std::collections::HashSet<String>,
    pub(crate) versions: std::collections::HashMap<String, String>,
}

/// A held connection to the engine for repeated snapshot queries (e.g. the
/// `iii worker status` watch loop). One connect/disconnect pair per probe
/// lifetime — connecting per query would fire the engine's workers-available
/// triggers on every poll.
pub(crate) struct EngineProbe {
    iii: iii_sdk::IIIClient,
}

impl EngineProbe {
    pub(crate) fn connect(port: u16) -> Self {
        let iii = iii_sdk::register_worker(
            &format!("ws://127.0.0.1:{port}"),
            iii_sdk::InitOptions {
                // Identify the transient connection so it's attributable in
                // the engine console instead of an anonymous
                // `<hostname>:<pid>`.
                metadata: Some(iii_sdk::runtime::WorkerMetadata {
                    name: "iii-cli".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        Self { iii }
    }

    pub(crate) async fn snapshot(&self) -> Option<EngineWorkerSnapshot> {
        use iii_sdk::protocol::TriggerRequest;
        let result = self
            .iii
            .trigger(TriggerRequest {
                function_id: "engine::workers::list".to_string(),
                payload: serde_json::json!({}),
                action: None,
                // The engine is local; a short deadline keeps callers snappy
                // when the port is open but the engine is wedged.
                timeout_ms: Some(3_000),
            })
            .await;
        parse_engine_worker_snapshot(&result.ok()?)
    }

    /// Non-joining shutdown: `shutdown()` joins the connection thread, which
    /// parks forever inside connect_async() when the port is open but the WS
    /// handshake never completes (wedged engine, port squatter). The thread
    /// leaks; the process is a short-lived CLI (or the daemon, where a leak
    /// per wedged-engine event is bounded).
    pub(crate) async fn close(self) {
        self.iii.shutdown_async().await;
    }
}

/// One-shot: ask the engine what's actually connected/registered. `None`
/// when the engine is unreachable, the call times out, or RBAC denies the
/// discovery function — callers fall back to local pidfile/ps heuristics
/// (MOT-3931: those heuristics alone made `iii worker list` report dead VMs
/// as running).
async fn engine_worker_snapshot(port: u16) -> Option<EngineWorkerSnapshot> {
    let probe = EngineProbe::connect(port);
    let snap = probe.snapshot().await;
    probe.close().await;
    snap
}

/// Status derivation for a config worker when the engine answered. Engine
/// truth wins: only a connected/registered worker is "running". A live local
/// process the engine can't see is "starting" — booting, or wedged mid-
/// connect (MOT-3931: this state used to read as "running"). Returns
/// `(running, status)`.
fn engine_status_for(connected: bool, worker_running: bool) -> (bool, &'static str) {
    if connected {
        (true, "running")
    } else if worker_running {
        (false, "starting")
    } else {
        (false, "stopped")
    }
}

/// Pure parser for the `engine::workers::list` result. Exposed for testing.
fn parse_engine_worker_snapshot(value: &serde_json::Value) -> Option<EngineWorkerSnapshot> {
    let workers = value.get("workers")?.as_array()?;
    let mut names = std::collections::HashSet::new();
    let mut versions = std::collections::HashMap::new();
    for w in workers {
        if w.get("status").and_then(|v| v.as_str()) == Some("disconnected") {
            continue;
        }
        let version = w.get("version").and_then(|v| v.as_str());
        for key in ["name", "id"] {
            if let Some(n) = w.get(key).and_then(|v| v.as_str())
                && !n.is_empty()
            {
                names.insert(n.to_string());
                if let Some(ver) = version {
                    versions
                        .entry(n.to_string())
                        .or_insert_with(|| ver.to_string());
                }
            }
        }
    }
    Some(EngineWorkerSnapshot { names, versions })
}

#[async_trait]
impl WorkerHostShim for CliHostShim {
    async fn add(
        &self,
        opts: AddOptions,
        ctx: &ProjectCtx,
        _events: &dyn EventSink,
    ) -> Result<AddOutcome, WorkerOpError> {
        use crate::core::{AddStatus, WorkerSource};

        // Translate AddOptions back to `handle_managed_add`'s &str form.
        // status is LOSSY: the underlying i32 API doesn't surface whether
        // the worker was already-current or replaced — every success maps
        // to `Installed`. version is recovered post-install from iii.lock.
        let (image_or_name, is_local) = match &opts.source {
            WorkerSource::Registry { name, version } => {
                let s = match version {
                    Some(v) => format!("{name}@{v}"),
                    None => name.clone(),
                };
                (s, false)
            }
            WorkerSource::Oci { reference } => (reference.clone(), false),
            WorkerSource::Local { path } => (path.display().to_string(), true),
        };

        let brief = false;
        let image_or_name_clone = image_or_name.clone();
        let force = opts.force;
        let reset_config = opts.reset_config;
        let wait = opts.wait;
        // Snapshot config workers BEFORE the install so we can diff and
        // recover the canonical worker name on success. For OCI installs
        // `source_label_name` returns a raw reference, which lockfile
        // lookups miss and clients can't feed back into other worker::*
        // triggers. The canonical name is whatever the installer actually
        // wrote to config.yaml; local re-adds (no diff) resolve it from
        // the manifest instead.
        let pre_config_names: std::collections::HashSet<String> =
            crate::cli::config_file::list_worker_names()
                .into_iter()
                .collect();
        let (rc, captured) = run_capturing_stderr(|| async move {
            if is_local {
                crate::cli::local_worker::handle_local_add(
                    &image_or_name_clone,
                    force,
                    reset_config,
                    brief,
                    wait,
                )
                .await
            } else {
                crate::cli::managed::handle_managed_add(
                    &image_or_name_clone,
                    brief,
                    force,
                    reset_config,
                    wait,
                )
                .await
            }
        })
        .await;

        if rc == 0 {
            let label = source_label_name(&opts.source);
            // First post-install canonical name we didn't see beforehand.
            // For idempotent re-adds where the entry already existed:
            // registry sources use the slug, local sources resolve the
            // manifest name, and only OCI falls back to the raw label
            // (the lockfile lookup returns None cleanly there).
            let name = post_install_worker_name(&opts.source, &pre_config_names, &label);
            let version = read_locked_version(&name);
            Ok(AddOutcome {
                name,
                version,
                status: AddStatus::Installed, // status diagnostic still lossy; see roadmap.
                awaited_ready: opts.wait,
                config_path: ctx.config_path(),
            })
        } else {
            Err(classify_handler_error(
                rc,
                &captured,
                "add",
                &source_label_name(&opts.source),
            ))
        }
    }

    async fn remove(
        &self,
        opts: RemoveOptions,
        _ctx: &ProjectCtx,
        _events: &dyn EventSink,
    ) -> Result<RemoveOutcome, WorkerOpError> {
        // Validate and resolve targets up front so we report what was attempted.
        let targets = resolve_remove_targets(&opts)?;
        let yes = opts.yes;
        let names_for_call = targets.clone();
        let (rc, captured) = run_capturing_stderr(|| async move {
            crate::cli::managed::handle_managed_remove_many(&names_for_call, yes).await
        })
        .await;
        if rc == 0 {
            Ok(RemoveOutcome { removed: targets })
        } else {
            let hint = targets.first().cloned().unwrap_or_default();
            Err(classify_handler_error(rc, &captured, "remove", &hint))
        }
    }

    async fn update(
        &self,
        opts: UpdateOptions,
        _ctx: &ProjectCtx,
        _events: &dyn EventSink,
    ) -> Result<UpdateOutcome, WorkerOpError> {
        // Snapshot pre-update versions so we can compute the structured diff.
        let pre_versions = snapshot_locked_versions();
        // Update accepts EITHER a single name (legacy) OR a list. Map the
        // multi-name list to repeated single-name calls (the underlying
        // handler is single-name today).
        let targets = if opts.names.is_empty() {
            vec![None] // None = "update all"
        } else {
            opts.names.iter().cloned().map(Some).collect()
        };
        let mut all_captured = String::new();
        let mut last_rc = 0i32;
        for target in targets {
            let target_for_call = target.clone();
            let (rc, captured) = run_capturing_stderr(|| async move {
                crate::cli::managed::handle_worker_update(target_for_call.as_deref()).await
            })
            .await;
            all_captured.push_str(&captured);
            last_rc = rc;
            if rc != 0 {
                let hint = target.clone().unwrap_or_default();
                return Err(classify_handler_error(rc, &all_captured, "update", &hint));
            }
        }
        let _ = last_rc;
        let post_versions = snapshot_locked_versions();
        let updated = diff_versions(&pre_versions, &post_versions);
        Ok(UpdateOutcome { updated })
    }

    async fn start(
        &self,
        opts: StartOptions,
        _ctx: &ProjectCtx,
        _events: &dyn EventSink,
    ) -> Result<StartOutcome, WorkerOpError> {
        // handle_managed_start expects port: u16 + config: Option<&Path>.
        // No explicit port in the request: resolve the engine's actual
        // manager port from config.yaml (honors III_CONFIG_PATH) instead of
        // assuming the compiled-in default — a daemon serving a non-default
        // engine must not point spawned workers at 49134 (workers#526).
        let port = opts
            .port
            .unwrap_or_else(crate::cli::config_file::manager_port);
        let name_for_call = opts.name.clone();
        let config_for_call = opts.config.clone();
        let wait = opts.wait;
        let (rc, captured) = run_capturing_stderr(|| async move {
            let config_path = config_for_call.as_deref().map(std::path::Path::new);
            crate::cli::managed::handle_managed_start(&name_for_call, wait, port, config_path).await
        })
        .await;
        if rc == 0 {
            // Read the real pid from ps after the start completes. None when
            // the worker doesn't surface a pid (engine builtins).
            let pid = crate::cli::managed::find_worker_pid_from_ps(&opts.name);
            Ok(StartOutcome {
                name: opts.name,
                pid,
                port: Some(port),
            })
        } else {
            Err(classify_handler_error(rc, &captured, "start", &opts.name))
        }
    }

    async fn stop(
        &self,
        opts: StopOptions,
        _ctx: &ProjectCtx,
        _events: &dyn EventSink,
    ) -> Result<StopOutcome, WorkerOpError> {
        if !opts.yes {
            return Err(WorkerOpError::ConsentRequired { op: "stop".into() });
        }
        let name_for_call = opts.name.clone();
        let (rc, captured) = run_capturing_stderr(|| async move {
            crate::cli::managed::handle_managed_stop(&name_for_call).await
        })
        .await;
        if rc == 0 {
            // Verify the worker is actually no longer running (the handler may
            // exit 0 even if the process is in a weird state).
            let stopped = !crate::cli::managed::is_worker_running(&opts.name);
            Ok(StopOutcome {
                name: opts.name,
                stopped,
            })
        } else {
            Err(classify_handler_error(rc, &captured, "stop", &opts.name))
        }
    }

    async fn list(
        &self,
        opts: ListOptions,
        _ctx: &ProjectCtx,
        _events: &dyn EventSink,
    ) -> Result<ListOutcome, WorkerOpError> {
        use crate::cli::managed::{
            discover_disk_worker_names, discover_running_worker_names_from_ps,
            find_worker_pid_from_ps, is_engine_running, is_worker_running,
        };
        use crate::core::WorkerEntry;

        // Source of truth: union of declared workers (config.yaml) + on-disk
        // managed dirs + live processes. Same shape `iii worker list` prints,
        // but structured so trigger callers get the data, not just stdout.
        let config_names = crate::cli::config_file::list_worker_names();
        let disk_names = discover_disk_worker_names();
        let ps_names = discover_running_worker_names_from_ps();
        let ps_set: std::collections::HashSet<String> = ps_names.iter().cloned().collect();
        let config_set: std::collections::HashSet<&str> =
            config_names.iter().map(String::as_str).collect();

        let candidate_names: std::collections::BTreeSet<String> =
            disk_names.into_iter().chain(ps_names).collect();
        let orphan_names: Vec<String> = candidate_names
            .into_iter()
            .filter(|n| !config_set.contains(n.as_str()))
            .filter(|n| ps_set.contains(n) || is_worker_running(n))
            .collect();

        // Try to enrich with versions from the lockfile. Failure to read is
        // not fatal — list is observational, not contractual.
        let lockfile =
            crate::cli::lockfile::WorkerLockfile::read_from(crate::cli::lockfile::lockfile_path())
                .ok();
        // Returns the lockfile-tracked version when present. None means the
        // worker is not in iii.lock (engine builtins, orphan processes that
        // bypass the lockfile path).
        let version_for = |name: &str| -> Option<String> {
            lockfile
                .as_ref()
                .and_then(|lf| lf.workers.get(name).map(|w| w.version.clone()))
        };

        let engine_running = is_engine_running();
        // Engine truth: what's actually connected/registered right now.
        // `None` = engine down or query failed; local signals only.
        let engine = if engine_running {
            // Outer deadline as a backstop over the SDK's own trigger
            // timeout: list must never hang on a half-up engine.
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                engine_worker_snapshot(crate::cli::config_file::manager_port()),
            )
            .await
            .ok()
            .flatten()
        } else {
            None
        };

        let mut workers = Vec::with_capacity(config_names.len() + orphan_names.len());

        for name in &config_names {
            // `is_worker_running` is identity-checked (pidfile PID must map
            // to this worker in the process table), so a live hit is a real
            // local process — but a live process is NOT a serving worker: a
            // VM can sit booting (or stuck reconnecting) for minutes. When
            // the engine answered, connected/registered is the only signal
            // that earns "running"; a live-but-unregistered process is
            // "starting" (MOT-3931: list must not say running for a worker
            // the engine can't see).
            let worker_running = is_worker_running(name);
            let (alive, status) = match &engine {
                Some(snap) => {
                    let (alive, status) =
                        engine_status_for(snap.names.contains(name.as_str()), worker_running);
                    (alive, Some(status.to_string()))
                }
                // Engine unreachable: config-only / engine-builtin entries
                // run inside the engine process, so the port probe is the
                // only signal available for them. status stays None —
                // consumers fall back to the `running` bool.
                None => (
                    worker_running
                        || (matches!(
                            crate::cli::config_file::resolve_worker_type(name),
                            crate::cli::config_file::ResolvedWorkerType::Config
                        ) && engine_running),
                    None,
                ),
            };
            let pid = find_worker_pid_from_ps(name);
            let version = version_for(name).or_else(|| {
                engine
                    .as_ref()
                    .and_then(|snap| snap.versions.get(name.as_str()).cloned())
            });
            workers.push(WorkerEntry {
                name: name.clone(),
                version,
                running: alive,
                status,
                pid,
            });
        }

        for name in &orphan_names {
            let pid = find_worker_pid_from_ps(name);
            workers.push(WorkerEntry {
                name: name.clone(),
                version: version_for(name),
                running: true, // orphan filter already requires alive
                status: None,
                pid,
            });
        }

        if opts.running_only {
            workers.retain(|w| w.running);
        }

        Ok(ListOutcome { workers })
    }

    async fn clear(
        &self,
        opts: ClearOptions,
        _ctx: &ProjectCtx,
        _events: &dyn EventSink,
    ) -> Result<ClearOutcome, WorkerOpError> {
        // Resolve targets and validate explicit consent for the wide blast radius.
        let targets = resolve_clear_targets(&opts)?;
        let yes = opts.yes;
        // Sum sizes BEFORE wipe — handle_managed_clear only returns rc.
        let cleared_bytes: u64 = if opts.all {
            // all-wipe: sum every name under ~/.iii/managed + ~/.iii/workers.
            crate::cli::managed::discover_disk_worker_names()
                .iter()
                .map(|n| measure_artifact_bytes(n))
                .sum()
        } else {
            targets.iter().map(|n| measure_artifact_bytes(n)).sum()
        };

        let names_for_call = if opts.all { vec![] } else { targets.clone() };
        let (rc, captured) = run_capturing_stderr(|| async move {
            // handle_managed_clear is single-name; loop for multi-name, None for all.
            if names_for_call.is_empty() {
                crate::cli::managed::handle_managed_clear(None, yes)
            } else {
                let mut rc = 0;
                for n in &names_for_call {
                    let r = crate::cli::managed::handle_managed_clear(Some(n.as_str()), yes);
                    if r != 0 {
                        rc = r;
                    }
                }
                rc
            }
        })
        .await;
        if rc == 0 {
            Ok(ClearOutcome { cleared_bytes })
        } else {
            let hint = targets.first().cloned().unwrap_or_default();
            Err(classify_handler_error(rc, &captured, "clear", &hint))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers used by the adapters above. All read-only against the lockfile and
// on-disk worker dirs; safe to call concurrently.
// ─────────────────────────────────────────────────────────────────────────────

fn post_install_worker_name(
    source: &crate::core::WorkerSource,
    pre: &std::collections::HashSet<String>,
    fallback_label: &str,
) -> String {
    if let Some(new_name) = crate::cli::config_file::list_worker_names()
        .into_iter()
        .find(|n| !pre.contains(n))
    {
        return new_name;
    }
    match source {
        crate::core::WorkerSource::Registry { name, .. } => name.clone(),
        // Re-adds (force: true) leave the config.yaml entry in place, so the
        // pre/post diff finds nothing new. Resolve from the manifest (falling
        // back to the directory name) — the raw path label is not a worker
        // name and no other worker::* trigger accepts it.
        crate::core::WorkerSource::Local { path } => {
            crate::cli::local_worker::resolve_worker_name(path)
        }
        crate::core::WorkerSource::Oci { .. } => fallback_label.to_string(),
    }
}

fn read_locked_version(name: &str) -> Option<String> {
    crate::cli::lockfile::WorkerLockfile::read_from(crate::cli::lockfile::lockfile_path())
        .ok()
        .and_then(|lf| lf.workers.get(name).map(|w| w.version.clone()))
}

fn snapshot_locked_versions() -> std::collections::BTreeMap<String, String> {
    crate::cli::lockfile::WorkerLockfile::read_from(crate::cli::lockfile::lockfile_path())
        .ok()
        .map(|lf| {
            lf.workers
                .into_iter()
                .map(|(k, v)| (k, v.version))
                .collect()
        })
        .unwrap_or_default()
}

fn diff_versions(
    pre: &std::collections::BTreeMap<String, String>,
    post: &std::collections::BTreeMap<String, String>,
) -> Vec<crate::core::UpdateEntry> {
    let mut out = Vec::new();
    for (name, after) in post {
        if let Some(before) = pre.get(name)
            && before != after
        {
            out.push(crate::core::UpdateEntry {
                name: name.clone(),
                from_version: before.clone(),
                to_version: after.clone(),
            });
        }
    }
    out
}

/// Validate `RemoveOptions` and return the concrete name list to act on.
/// Rejects missing consent (W104), empty lists without `all = true`
/// (W103), and non-empty + `all` combinations (W103) as ambiguous.
#[doc(hidden)]
pub fn resolve_remove_targets(opts: &RemoveOptions) -> Result<Vec<String>, WorkerOpError> {
    if !opts.yes {
        return Err(WorkerOpError::ConsentRequired {
            op: "remove".into(),
        });
    }
    match (opts.names.is_empty(), opts.all) {
        (true, false) => Err(WorkerOpError::MissingTarget {
            op: "remove".into(),
            reason: "names is empty; pass non-empty names or all:true".into(),
        }),
        (false, true) => Err(WorkerOpError::MissingTarget {
            op: "remove".into(),
            reason: "all:true conflicts with explicit names; pick one".into(),
        }),
        (true, true) => Ok(crate::cli::config_file::list_worker_names()),
        (false, false) => Ok(opts.names.clone()),
    }
}

/// Validate `ClearOptions` and return the names to wipe. `all` lets the
/// caller request a full sweep without listing names; both must be set
/// explicitly so `{}` no longer wipes everything by accident.
#[doc(hidden)]
pub fn resolve_clear_targets(opts: &ClearOptions) -> Result<Vec<String>, WorkerOpError> {
    if !opts.yes {
        return Err(WorkerOpError::ConsentRequired { op: "clear".into() });
    }
    match (opts.names.is_empty(), opts.all) {
        (true, false) => Err(WorkerOpError::MissingTarget {
            op: "clear".into(),
            reason: "names is empty; pass non-empty names or all:true".into(),
        }),
        (false, true) => Err(WorkerOpError::MissingTarget {
            op: "clear".into(),
            reason: "all:true conflicts with explicit names; pick one".into(),
        }),
        (true, true) => Ok(Vec::new()), // signals all-mode to the caller
        (false, false) => Ok(opts.names.clone()),
    }
}

fn measure_artifact_bytes(worker_name: &str) -> u64 {
    let home = match dirs::home_dir() {
        Some(p) => p,
        None => return 0,
    };
    let mut total = 0u64;
    for sub in ["managed", "workers"] {
        let dir = home.join(".iii").join(sub).join(worker_name);
        if dir.is_dir() {
            total = total.saturating_add(dir_size(&dir));
        }
    }
    total
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(path) {
        for entry in rd.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    total = total.saturating_add(dir_size(&entry.path()));
                } else {
                    total = total.saturating_add(meta.len());
                }
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::WorkerOpErrorKind;

    #[test]
    fn engine_status_mapping_is_engine_truth_first() {
        // Connected wins regardless of local process signals (builtins have
        // no local process at all).
        assert_eq!(engine_status_for(true, true), (true, "running"));
        assert_eq!(engine_status_for(true, false), (true, "running"));
        // The MOT-3931 lie: process alive but engine can't see it — that is
        // "starting", never "running".
        assert_eq!(engine_status_for(false, true), (false, "starting"));
        assert_eq!(engine_status_for(false, false), (false, "stopped"));
    }

    #[test]
    fn engine_snapshot_parses_names_ids_versions_and_skips_disconnected() {
        let value = serde_json::json!({
            "workers": [
                // WS worker: matchable by self-reported name.
                {"name": "scrapling", "id": "b1e2...", "status": "connected", "version": "0.2.3"},
                // In-process builtin: matchable by engine-side id.
                {"name": "state", "id": "iii-state", "status": "available", "version": "0.21.2"},
                // Disconnected workers must not read as running.
                {"name": "ghost", "id": "dead...", "status": "disconnected", "version": "1.0.0"},
                {"name": null, "id": "anon", "status": "connected"},
            ]
        });
        let snap = parse_engine_worker_snapshot(&value).unwrap();
        assert!(snap.names.contains("scrapling"));
        assert!(snap.names.contains("iii-state"));
        assert!(snap.names.contains("state"));
        assert!(snap.names.contains("anon"));
        assert!(!snap.names.contains("ghost"));
        assert_eq!(
            snap.versions.get("scrapling").map(String::as_str),
            Some("0.2.3")
        );
        assert_eq!(
            snap.versions.get("iii-state").map(String::as_str),
            Some("0.21.2")
        );
        assert!(snap.versions.get("anon").is_none());
        // Not a list result at all → None, caller falls back.
        assert!(parse_engine_worker_snapshot(&serde_json::json!({"nope": 1})).is_none());
    }

    /// Shared across every test that swaps fd 2 — fd 2 is process-
    /// global, so a per-test local `static` Mutex (one per function)
    /// is THREE separate locks, not one, which lets cargo test's
    /// default parallel execution race the three swappers. CI on
    /// Linux reliably trips the race (each test reads the OTHER
    /// test's swap and asserts on it); macOS happens to win the race
    /// most of the time. One module-level lock fixes it for both.
    static FD_SWAP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Regression: when fd 2 has been redirected to a regular file
    /// (the engine-spawned subprocess case), `stderr_is_regular_file`
    /// must return true so `run_capturing_stderr` passes stderr
    /// through live instead of buffering it. The whole wait UI
    /// `tail:` row depends on this.
    #[test]
    fn stderr_is_regular_file_detects_redirected_log() {
        use std::os::unix::io::AsRawFd;
        // Serialize fd 2 swaps; concurrent tests would see each
        // other's redirects.
        let _guard = FD_SWAP_LOCK.lock().unwrap();

        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let tmp_fd = tmp.as_file().as_raw_fd();

        // SAFETY: fd 2 is process-global; the mutex above serializes;
        // we restore on every path.
        let orig = unsafe { libc::dup(2) };
        assert!(orig >= 0, "dup(2) failed");
        assert!(unsafe { libc::dup2(tmp_fd, 2) } >= 0, "dup2 failed");

        let result = stderr_is_regular_file();

        // Restore before asserting so a panic doesn't leave fd 2
        // pointing at the temp file for the next test.
        assert!(unsafe { libc::dup2(orig, 2) } >= 0, "restore dup2 failed");
        unsafe { libc::close(orig) };

        assert!(
            result,
            "stderr_is_regular_file must return true when fd 2 points at a regular file"
        );
    }

    /// Regression: when fd 2 is a character device (/dev/null), the
    /// helper must return false so the daemon-pipe capture path stays
    /// in effect.
    #[test]
    fn stderr_is_regular_file_rejects_chardev() {
        use std::os::unix::io::AsRawFd;
        let _guard = FD_SWAP_LOCK.lock().unwrap();

        let devnull = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .expect("open /dev/null");
        let dn_fd = devnull.as_raw_fd();

        let orig = unsafe { libc::dup(2) };
        unsafe { libc::dup2(dn_fd, 2) };

        let result = stderr_is_regular_file();

        unsafe { libc::dup2(orig, 2) };
        unsafe { libc::close(orig) };

        assert!(
            !result,
            "stderr_is_regular_file must return false for /dev/null"
        );
    }

    /// Regression: when fd 2 is a pipe (the daemon-trigger case the
    /// capture path was designed for), the helper must return false.
    /// If this ever flips, trigger callers stop seeing clean error
    /// messages in their responses.
    #[test]
    fn stderr_is_regular_file_rejects_pipe() {
        let _guard = FD_SWAP_LOCK.lock().unwrap();

        let mut pipe_fds: [libc::c_int; 2] = [0; 2];
        let rc = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
        assert_eq!(rc, 0, "pipe() failed");
        let (read_end, write_end) = (pipe_fds[0], pipe_fds[1]);

        let orig = unsafe { libc::dup(2) };
        unsafe { libc::dup2(write_end, 2) };

        let result = stderr_is_regular_file();

        unsafe { libc::dup2(orig, 2) };
        unsafe { libc::close(orig) };
        unsafe { libc::close(read_end) };
        unsafe { libc::close(write_end) };

        assert!(
            !result,
            "stderr_is_regular_file must return false for pipes (daemon-trigger path)"
        );
    }

    #[test]
    fn classify_handler_error_maps_not_found_to_w110() {
        let captured = "Resolving worker...\nerror: Worker 'pdfkit' not found\n";
        let err = classify_handler_error(1, captured, "add", "pdfkit");
        assert_eq!(err.kind(), WorkerOpErrorKind::NotFound);
        assert_eq!(err.to_payload()["details"]["name"], "pdfkit");
        // No "(rc 1)" leak in the message.
        assert!(!err.to_string().contains("rc 1"));
    }

    #[test]
    fn classify_handler_error_uses_name_hint_when_no_quote() {
        let captured = "error: registry says: not found\n";
        let err = classify_handler_error(2, captured, "update", "myworker");
        assert_eq!(err.kind(), WorkerOpErrorKind::NotFound);
        assert_eq!(err.to_payload()["details"]["name"], "myworker");
    }

    #[test]
    fn classify_handler_error_maps_invalid_chars_to_w100() {
        let captured = "error: Worker name 'foo;rm -rf' contains invalid characters. Only alphanumeric, dash, underscore, and dot are allowed.\n";
        let err = classify_handler_error(1, captured, "stop", "foo;rm -rf");
        assert_eq!(err.kind(), WorkerOpErrorKind::InvalidName);
        // Must not say "internal:" anymore.
        assert!(!err.to_string().contains("internal:"));
    }

    #[test]
    fn classify_handler_error_falls_back_to_internal_without_pattern() {
        let captured = "error: build failed: missing toolchain\n";
        let err = classify_handler_error(3, captured, "add", "x");
        assert_eq!(err.kind(), WorkerOpErrorKind::Internal);
        // Internal message keeps the detail but does NOT append "(rc N)".
        let msg = err.to_string();
        assert!(msg.contains("build failed"));
        assert!(!msg.contains("(rc 3)"));
    }

    #[test]
    fn classify_handler_error_with_empty_capture_uses_op_label() {
        let err = classify_handler_error(7, "", "remove", "");
        assert_eq!(err.kind(), WorkerOpErrorKind::Internal);
        assert!(err.to_string().contains("remove"));
        assert!(err.to_string().contains("rc 7"));
    }

    #[test]
    fn extract_quoted_pulls_first_single_quoted_token() {
        assert_eq!(extract_quoted("Worker 'foo' not found"), Some("foo".into()));
        assert_eq!(extract_quoted("'a' and 'b'"), Some("a".into()));
        assert_eq!(extract_quoted("no quotes here"), None);
    }

    #[test]
    fn resolve_remove_targets_consent_returns_w104() {
        let opts = RemoveOptions {
            names: vec!["x".into()],
            all: false,
            yes: false,
        };
        let err = resolve_remove_targets(&opts).unwrap_err();
        assert_eq!(err.kind(), WorkerOpErrorKind::ConsentRequired);
        assert_eq!(err.to_payload()["details"]["op"], "remove");
    }

    #[test]
    fn resolve_remove_targets_empty_returns_w103_missing_target() {
        let opts = RemoveOptions {
            names: vec![],
            all: false,
            yes: true,
        };
        let err = resolve_remove_targets(&opts).unwrap_err();
        assert_eq!(err.kind(), WorkerOpErrorKind::MissingTarget);
        assert_eq!(err.to_payload()["details"]["op"], "remove");
    }

    #[test]
    fn resolve_remove_targets_ambiguous_returns_w103() {
        let opts = RemoveOptions {
            names: vec!["a".into()],
            all: true,
            yes: true,
        };
        let err = resolve_remove_targets(&opts).unwrap_err();
        assert_eq!(err.kind(), WorkerOpErrorKind::MissingTarget);
    }

    #[test]
    fn resolve_remove_targets_explicit_names_pass_through() {
        let opts = RemoveOptions {
            names: vec!["pdfkit".into()],
            all: false,
            yes: true,
        };
        assert_eq!(
            resolve_remove_targets(&opts).unwrap(),
            vec!["pdfkit".to_string()]
        );
    }

    #[test]
    fn resolve_clear_targets_consent_returns_w104() {
        let opts = ClearOptions {
            names: vec![],
            all: true,
            yes: false,
        };
        let err = resolve_clear_targets(&opts).unwrap_err();
        assert_eq!(err.kind(), WorkerOpErrorKind::ConsentRequired);
        assert_eq!(err.to_payload()["details"]["op"], "clear");
    }

    #[test]
    fn resolve_clear_targets_empty_returns_w103() {
        let opts = ClearOptions {
            names: vec![],
            all: false,
            yes: true,
        };
        let err = resolve_clear_targets(&opts).unwrap_err();
        assert_eq!(err.kind(), WorkerOpErrorKind::MissingTarget);
    }

    #[test]
    fn bare_rc_wrapper_detected_only_for_uncaptured_failures() {
        // Empty capture (CLI TTY / log-file stderr) → the bare wrapper the
        // CLI must NOT re-print: the handler already showed the real error.
        let bare = classify_handler_error(1, "", "add", "w");
        assert!(is_bare_rc_wrapper(&bare));

        // Captured detail → a real message the CLI/daemon caller needs.
        let detailed = classify_handler_error(1, "error: HTTP 422 from registry", "add", "w");
        assert!(!is_bare_rc_wrapper(&detailed));

        // Other typed errors never match.
        let not_found = classify_handler_error(1, "Worker 'w' not found", "add", "w");
        assert!(!is_bare_rc_wrapper(&not_found));
    }

    /// Regression: a forced re-add of a local worker found no NEW name in
    /// the config.yaml pre/post diff (the entry already existed) and fell
    /// back to the raw path label — `worker::add` returned
    /// `name: "/tmp/hello-world"`, which no other worker::* trigger
    /// accepts. Local sources must resolve the name from the manifest.
    #[test]
    fn post_install_worker_name_local_readd_resolves_manifest_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("iii.worker.yaml"),
            "name: hello-world\nscripts:\n  start: \"node src/index.js\"\n",
        )
        .expect("write manifest");

        let source = crate::core::WorkerSource::Local {
            path: dir.path().to_path_buf(),
        };
        // Snapshot the CURRENT config names as `pre` so the diff finds
        // nothing new — the forced re-add shape.
        let pre: std::collections::HashSet<String> = crate::cli::config_file::list_worker_names()
            .into_iter()
            .collect();
        let label = dir.path().display().to_string();

        let name = post_install_worker_name(&source, &pre, &label);
        assert_eq!(
            name, "hello-world",
            "local re-add must return the manifest name, not the path"
        );
    }

    /// Without a manifest name, the local fallback is the directory name —
    /// still a valid single-segment worker id, never the full path.
    #[test]
    fn post_install_worker_name_local_readd_without_manifest_uses_dir_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = crate::core::WorkerSource::Local {
            path: dir.path().to_path_buf(),
        };
        let pre: std::collections::HashSet<String> = crate::cli::config_file::list_worker_names()
            .into_iter()
            .collect();
        let label = dir.path().display().to_string();

        let name = post_install_worker_name(&source, &pre, &label);
        let dir_name = dir
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap()
            .to_string();
        assert_eq!(name, dir_name);
        assert!(
            !name.contains('/'),
            "worker name must be a single path segment, got {name}"
        );
    }

    /// OCI re-adds have no manifest dir or registry slug to resolve a name
    /// from, so the only safe answer is the raw reference label (the
    /// lockfile version lookup returns None cleanly for it).
    #[test]
    fn post_install_worker_name_oci_readd_falls_back_to_reference_label() {
        let reference = "ghcr.io/iii-hq/node:latest";
        let source = crate::core::WorkerSource::Oci {
            reference: reference.to_string(),
        };
        let pre: std::collections::HashSet<String> = crate::cli::config_file::list_worker_names()
            .into_iter()
            .collect();

        let name = post_install_worker_name(&source, &pre, reference);
        assert_eq!(
            name, reference,
            "OCI re-add must fall back to the reference label"
        );
    }
}
