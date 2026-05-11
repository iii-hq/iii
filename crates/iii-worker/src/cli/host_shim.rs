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
        // recover the canonical worker name on success. For OCI / local
        // installs `source_label_name` returns a raw reference/path, which
        // lockfile lookups miss and clients can't feed back into other
        // worker::* triggers. The canonical name is whatever the installer
        // actually wrote to config.yaml.
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
            // Falls back to the raw label for idempotent re-adds where the
            // entry already existed (in which case the label IS the name
            // for the registry path, and the lockfile lookup at least
            // returns None cleanly for OCI / local sources).
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
        use crate::cli::app::DEFAULT_PORT;
        let port = opts.port.unwrap_or(DEFAULT_PORT);
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
                port: opts.port,
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
            disk_names.into_iter().chain(ps_names.into_iter()).collect();
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

        let mut workers = Vec::with_capacity(config_names.len() + orphan_names.len());

        for name in &config_names {
            // Mirrors handle_worker_list's status logic: a worker counts as
            // running if its own pidfile/ps match is alive, OR if it's a
            // config-only / engine-builtin entry and the engine is up
            // (those workers run inside the engine process, not standalone).
            let worker_running = is_worker_running(name);
            let alive = worker_running
                || (matches!(
                    crate::cli::config_file::resolve_worker_type(name),
                    crate::cli::config_file::ResolvedWorkerType::Config
                ) && engine_running);
            let pid = find_worker_pid_from_ps(name);
            workers.push(WorkerEntry {
                name: name.clone(),
                version: version_for(name),
                running: alive,
                pid,
            });
        }

        for name in &orphan_names {
            let pid = find_worker_pid_from_ps(name);
            workers.push(WorkerEntry {
                name: name.clone(),
                version: version_for(name),
                running: true, // orphan filter already requires alive
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

/// Resolve the canonical worker name after a successful add by diffing
/// config.yaml entries. Returns the first name that appears post-install
/// but didn't exist beforehand. Falls back to:
///   - the registry name (for `Registry` sources, which is already canonical)
///   - the raw label otherwise (OCI ref / local path), preserving the prior
///     behavior for idempotent re-adds where the diff is empty.
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
        _ => fallback_label.to_string(),
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
}
