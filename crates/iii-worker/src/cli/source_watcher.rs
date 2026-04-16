// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Host-side source watcher for local-path workers.
//!
//! Virtiofs propagates file content and metadata from the host into the
//! guest VM, but inotify events do NOT cross the FUSE boundary — so
//! watchers inside the VM (tsx watch, node --watch, cargo watch, etc.)
//! never fire on host edits unless they opt into polling. Some runtimes
//! (notably tsx 4.x) don't support a polling fallback at all.
//!
//! This module works around that by watching the project directory on
//! the *host* with `notify`, debouncing rapid writes, and re-invoking
//! `iii-worker start <name>` to kill the stale VM and boot a fresh one.
//! The engine re-registers the worker automatically when its websocket
//! reconnects.
//!
//! Spawned as a hidden `__watch-source` subprocess alongside the VM so
//! the watcher survives independent of the short-lived `iii worker start`
//! CLI invocation.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

/// Debounce window: collapse rapid writes (editor save-then-rename,
/// multi-file renames, etc.) into a single restart.
pub const DEBOUNCE_MS: u64 = 500;

/// Directories whose contents should NEVER trigger a restart. These are
/// the high-churn artifact directories that tooling writes to during
/// normal operation — including during the restart itself, which would
/// otherwise produce an infinite restart loop.
pub const IGNORED_DIR_NAMES: &[&str] = &[
    // Node / TypeScript
    "node_modules",
    ".pnpm-store",
    ".parcel-cache",
    ".turbo",
    ".next",
    ".nuxt",
    ".svelte-kit",
    // Rust
    "target",
    // Python
    ".venv",
    "venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    // Go / PHP / Ruby share "vendor"
    "vendor",
    // Ruby
    ".bundle",
    // JVM (Maven, Gradle)
    ".gradle",
    ".m2",
    // .NET
    "obj",
    ".nuget",
    // Elixir / Erlang
    "_build",
    "deps",
    // Generic build output
    "dist",
    "build",
    "out",
    "coverage",
    ".cache",
    // VCS
    ".git",
    ".svn",
    ".hg",
    // IDE / OS
    ".idea",
    ".vscode",
    ".DS_Store",
];

/// Dependency manifest filenames. A change to any of these forces a
/// full VM restart (rerun install + reboot) instead of the fast
/// supervisor-level restart — the VM-local dep caches need to see new
/// packages, which only happens at boot.
pub const DEP_MANIFEST_NAMES: &[&str] = &[
    // Node / TypeScript
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    // Python
    "requirements.txt",
    "pyproject.toml",
    "poetry.lock",
    "Pipfile",
    "Pipfile.lock",
    "uv.lock",
    // Rust
    "Cargo.toml",
    "Cargo.lock",
    // Go
    "go.mod",
    "go.sum",
    // Ruby
    "Gemfile",
    "Gemfile.lock",
    // Elixir
    "mix.exs",
    "mix.lock",
    // JVM
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    // .NET
    "packages.config",
];

/// Categorizes a changed path so the watcher can pick fast path vs.
/// full restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Regular source file. Try supervisor fast restart first; fall
    /// back to a full VM restart on any error.
    Source,
    /// A dep-manifest change. Go straight to a full VM restart so the
    /// install step reruns inside the guest.
    DepManifest,
}

/// O(1) lookup set for dep-manifest classification. Built once; used on
/// every watcher burst.
static DEP_MANIFEST_SET: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| DEP_MANIFEST_NAMES.iter().copied().collect());

/// O(1) lookup set for ignore-path classification. Built once; hit on
/// every notify event, which can fire thousands of times per second
/// during bulk operations.
static IGNORED_DIR_SET: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| IGNORED_DIR_NAMES.iter().copied().collect());

/// Returns `true` if `path` is a dep manifest that should force a full
/// VM restart (and not a fast supervisor cycle).
pub fn is_dep_manifest(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    DEP_MANIFEST_SET.contains(name)
}

/// Returns true if a notify event on this path should be ignored.
///
/// A path is ignored when any of its components (after stripping the
/// project root prefix) matches [`IGNORED_DIR_NAMES`]. This correctly
/// ignores writes deep inside `node_modules/.cache/...` as well as the
/// directory itself.
///
/// Hidden files at the project root (e.g. `.env.local`) are NOT
/// ignored — users frequently edit those and expect a restart. Only
/// named artifact dirs are filtered.
pub fn should_ignore_path(path: &Path, project_root: &Path) -> bool {
    let rel = path.strip_prefix(project_root).unwrap_or(path);
    for component in rel.components() {
        if let std::path::Component::Normal(os) = component
            && let Some(s) = os.to_str()
            && IGNORED_DIR_SET.contains(s)
        {
            return true;
        }
    }
    false
}

/// Register non-recursive watches on every directory under `root`
/// EXCEPT those whose name (or any ancestor's name) matches
/// [`IGNORED_DIR_NAMES`]. This prunes inotify watch registration at
/// setup time, avoiding thousands of useless descriptors inside
/// `node_modules`/`target`/`.git` and cutting per-event filter cost
/// (events from ignored subtrees never fire at all).
///
/// Best-effort: permission errors on subdirs are logged and skipped,
/// not propagated. The watcher stays online for the rest of the tree.
fn watch_pruned(watcher: &mut notify::RecommendedWatcher, root: &Path) -> anyhow::Result<()> {
    use std::collections::VecDeque;

    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(root.to_path_buf());

    while let Some(dir) = queue.pop_front() {
        if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
            tracing::debug!(
                path = %dir.display(),
                error = %e,
                "watch_pruned: skipping unwatchable dir"
            );
            continue;
        }

        let read = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(
                    path = %dir.display(),
                    error = %e,
                    "watch_pruned: cannot enumerate, skipping children"
                );
                continue;
            }
        };

        for entry in read.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if !ft.is_dir() || ft.is_symlink() {
                continue;
            }
            let name = entry.file_name();
            if let Some(s) = name.to_str()
                && IGNORED_DIR_SET.contains(s)
            {
                continue;
            }
            queue.push_back(entry.path());
        }
    }

    Ok(())
}

/// Register `NonRecursive` watches on any paths from a burst that turn
/// out to be non-ignored directories. The initial `watch_pruned` pass
/// only covers directories that exist at startup; this catches
/// post-startup `mkdir` calls so edits inside new subdirs don't go
/// silent on Linux. Paths that aren't directories (files, stat errors,
/// ignored-subtree matches) are skipped. Duplicate registrations are
/// harmless — `notify` treats `watch(existing_path)` as a no-op on
/// inotify. Errors are logged at debug level and swallowed so a single
/// unwatchable dir doesn't tear down the watcher.
///
/// macOS FSEvents is inherently recursive and auto-covers new subdirs
/// already, so this call is a no-op there aside from the logging cost.
fn register_new_dirs(watcher: &mut notify::RecommendedWatcher, paths: &[PathBuf], root: &Path) {
    for p in paths {
        // Cheap rejection first — ignored subtrees (node_modules etc.)
        // must never be watched even if they were just created.
        if should_ignore_path(p, root) {
            continue;
        }
        // Skip non-directories. `is_dir` follows symlinks, which is
        // fine here: a user-placed symlink-to-dir is legitimately
        // something they want reloaded on edit. Errors from stat (file
        // already gone, permission denied) make is_dir return false.
        if !p.is_dir() {
            continue;
        }
        if let Err(e) = watcher.watch(p, RecursiveMode::NonRecursive) {
            tracing::debug!(
                path = %p.display(),
                error = %e,
                "source watcher: could not register new dir, skipping"
            );
        }
    }
}

/// Run the watch loop: watch `project_path` recursively, debounce
/// events, and invoke `on_change(worker_name)` whenever a non-ignored
/// path fires. `on_change` is expected to trigger the restart (in
/// production this execs `iii-worker start <name>`).
///
/// Runs until the watcher or channel errors out. Callers decide whether
/// to exit or retry on error — typically the supervising process will
/// exit, and the CLI `iii worker stop` path cleans up the sidecar pid
/// file.
pub async fn watch_and_restart<F>(
    worker_name: String,
    project_path: PathBuf,
    mut on_change: F,
) -> anyhow::Result<()>
where
    F: FnMut(&str, ChangeKind) + Send + 'static,
{
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<notify::Event>();

    // Canonicalize the project root for event-path comparison.
    // macOS FSEvents reports events with canonical `/private/var/...`
    // paths even when the caller watched the uncanonical `/var/...`
    // (symlink) form. Without this, the bare-root filter below fails
    // (`p != &root_for_filter` becomes trivially true when one side
    // is canonical and the other isn't) and every node_modules write
    // leaks through the filter. Production callers already canonicalize
    // upstream, but tests that pass `tempfile::tempdir().path()` hit
    // the mismatch on macOS CI runners. Falling back to the raw path
    // on canonicalize failure (e.g. project dir just got deleted)
    // preserves today's behavior.
    let root_for_filter =
        std::fs::canonicalize(&project_path).unwrap_or_else(|_| project_path.clone());
    let mut watcher = notify::RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            // Backend-level errors (inotify queue overflow on Linux,
            // FSEvents stream errors on macOS, permission changes
            // revoking an existing watch) are surfaced here. Previously
            // we silently swallowed them, which made "why did my
            // watcher stop restarting?" impossible to diagnose from
            // logs alone. Plumbing errors all the way to the consumer
            // channel would require a `Result`-carrying channel and
            // offers no more actionable information — the consumer
            // can't retry a per-event backend error anyway. Logging
            // here is the smallest change that closes the observability
            // gap.
            let event = match res {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, "source watcher: notify backend error");
                    return;
                }
            };
            use notify::EventKind;
            let is_change = matches!(
                event.kind,
                EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
            );
            if !is_change {
                return;
            }
            // On macOS FSEvents reports bare directory-level events
            // on the project root whenever any descendant changes
            // (including inside node_modules). A "path == root"
            // entry tells us nothing a specific-file event doesn't,
            // so drop it alongside ignored subtrees. If every path
            // in the event is either an ignored subtree OR the
            // bare root, skip — real edits always name a child path.
            let has_relevant_path = event
                .paths
                .iter()
                .any(|p| !should_ignore_path(p, &root_for_filter) && p != &root_for_filter);
            if !has_relevant_path {
                return;
            }
            let _ = tx.send(event);
        },
        notify::Config::default(),
    )?;

    // Walk the project once and register watches on directories that
    // aren't in the ignore list. notify's RecursiveMode::Recursive
    // otherwise registers inotify watches inside node_modules/target/
    // .git — thousands of watch descriptors on a large project, and
    // per-event post-delivery filtering. NonRecursive on pruned
    // directories avoids both costs.
    watch_pruned(&mut watcher, &project_path)?;

    tracing::info!(
        worker = %worker_name,
        path = %project_path.display(),
        "source watcher: online"
    );

    loop {
        let first = match rx.recv().await {
            Some(e) => e,
            None => {
                tracing::warn!(worker = %worker_name, "source watcher: channel closed");
                break;
            }
        };

        // Debounce window. Collect all paths that fired during this
        // quiet period so we can decide whether the change is a dep
        // manifest (force full VM restart) or a regular source edit
        // (try supervisor fast path).
        let mut burst_paths: Vec<PathBuf> = first.paths.clone();
        tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)).await;
        while let Ok(ev) = rx.try_recv() {
            burst_paths.extend(ev.paths);
        }

        // Register watches on any directories that appeared during this
        // burst. inotify's NonRecursive semantics mean a freshly-created
        // subdir has no descriptor yet, so edits inside it would go
        // silent until the next event in a pre-existing watched dir.
        // We do this once per burst (not on every callback invocation)
        // because the watcher handle lives here, not in the closure.
        // Files written between `mkdir` and this registration are
        // inherently missed — matches inotify's own semantics and is
        // best-effort by design.
        register_new_dirs(&mut watcher, &burst_paths, &project_path);

        let kind = if burst_paths.iter().any(|p| is_dep_manifest(p)) {
            ChangeKind::DepManifest
        } else {
            ChangeKind::Source
        };

        tracing::info!(
            worker = %worker_name,
            kind = ?kind,
            paths = burst_paths.len(),
            "source watcher: change detected"
        );
        on_change(&worker_name, kind);
    }

    Ok(())
}

/// Production restart dispatcher.
///
/// For `ChangeKind::Source`, tries the supervisor fast path first. If
/// the supervisor is unreachable (not installed, crashed, timeout), or
/// if the change is a `DepManifest`, falls back to a full VM restart
/// via `iii-worker start <name>`.
///
/// Invoked by `watch_and_restart` on every debounced file-change
/// burst.
pub fn restart_via_cli(worker_name: &str, kind: ChangeKind) {
    if matches!(kind, ChangeKind::Source) {
        match try_fast_restart(worker_name) {
            Ok(()) => {
                tracing::info!(
                    worker = %worker_name,
                    path = "fast",
                    "source watcher: restart ok"
                );
                return;
            }
            Err(e) => {
                tracing::info!(
                    worker = %worker_name,
                    error = %e,
                    "source watcher: fast path unavailable, falling back to full VM restart"
                );
            }
        }
    } else {
        tracing::info!(
            worker = %worker_name,
            "source watcher: dep manifest changed, forcing full VM restart"
        );
    }

    restart_via_full_vm(worker_name);
}

/// Fast path — talk to the in-VM supervisor over the unix socket that
/// `__vm-boot`'s proxy publishes. Blocks until the supervisor answers
/// or the 500ms timeout fires.
///
/// Uses the blocking `std::os::unix::net` variant because this function
/// is invoked from the watcher's sync callback, which itself runs
/// inside a tokio runtime — nesting tokio runtimes here panics with
/// "Cannot start a runtime from within a runtime". The wire protocol
/// and socket are identical to the async path; only the I/O primitive
/// differs.
fn try_fast_restart(worker_name: &str) -> anyhow::Result<()> {
    super::supervisor_ctl::request_restart_blocking(worker_name)
}

/// Slow path — spawn `iii-worker start <name>` which internally calls
/// `kill_stale_worker` (killing the old VM + watcher sidecar) and
/// boots a fresh VM. Taken when the supervisor is unreachable or when
/// the changed file is a dep manifest.
fn restart_via_full_vm(worker_name: &str) {
    let self_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "source watcher: current_exe() failed");
            return;
        }
    };

    let output = std::process::Command::new(&self_exe)
        .arg("start")
        .arg(worker_name)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .output();

    match output {
        Ok(o) if o.status.success() => {
            tracing::info!(
                worker = %worker_name,
                path = "full",
                "source watcher: restart ok"
            );
        }
        Ok(o) => {
            tracing::warn!(
                worker = %worker_name,
                code = ?o.status.code(),
                "source watcher: restart exited non-zero"
            );
        }
        Err(e) => {
            tracing::error!(worker = %worker_name, error = %e, "source watcher: restart spawn failed");
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/proj")
    }

    #[test]
    fn ignores_node_modules_root() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/node_modules"),
            &root()
        ));
    }

    #[test]
    fn ignores_nested_under_node_modules() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/node_modules/foo/bar.js"),
            &root()
        ));
    }

    #[test]
    fn ignores_target() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/target/debug/build.rs"),
            &root()
        ));
    }

    #[test]
    fn ignores_git_dir() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/.git/HEAD"),
            &root()
        ));
    }

    #[test]
    fn ignores_python_artifact_dirs() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/.venv/bin/python"),
            &root()
        ));
        assert!(should_ignore_path(
            &PathBuf::from("/proj/src/__pycache__/mod.cpython.pyc"),
            &root()
        ));
    }

    #[test]
    fn ignores_next_build_dirs() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/.next/cache/foo"),
            &root()
        ));
        assert!(should_ignore_path(
            &PathBuf::from("/proj/dist/index.js"),
            &root()
        ));
    }

    #[test]
    fn ignores_go_vendor() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/vendor/github.com/pkg/errors/errors.go"),
            &root()
        ));
    }

    #[test]
    fn ignores_jvm_build_dirs() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/.gradle/caches/x"),
            &root()
        ));
        assert!(should_ignore_path(
            &PathBuf::from("/proj/.m2/repository/a/b.jar"),
            &root()
        ));
    }

    #[test]
    fn ignores_dotnet_artifacts() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/obj/Debug/Foo.dll"),
            &root()
        ));
        assert!(should_ignore_path(
            &PathBuf::from("/proj/.nuget/packages/x"),
            &root()
        ));
    }

    #[test]
    fn ignores_elixir_artifacts() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/_build/dev/lib/app.beam"),
            &root()
        ));
        assert!(should_ignore_path(
            &PathBuf::from("/proj/deps/phoenix/mix.exs"),
            &root()
        ));
    }

    #[test]
    fn ignores_ruby_bundle() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/.bundle/config"),
            &root()
        ));
    }

    #[test]
    fn ignores_python_extended_caches() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/.mypy_cache/3.11/foo.data.json"),
            &root()
        ));
        assert!(should_ignore_path(
            &PathBuf::from("/proj/.ruff_cache/0.1.0/foo.py"),
            &root()
        ));
        assert!(should_ignore_path(
            &PathBuf::from("/proj/.tox/py311/lib/x"),
            &root()
        ));
    }

    #[test]
    fn ignores_pnpm_and_parcel_caches() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/.pnpm-store/v3/foo"),
            &root()
        ));
        assert!(should_ignore_path(
            &PathBuf::from("/proj/.parcel-cache/foo"),
            &root()
        ));
    }

    #[test]
    fn ignores_generic_coverage_and_out() {
        assert!(should_ignore_path(
            &PathBuf::from("/proj/coverage/lcov-report/index.html"),
            &root()
        ));
        assert!(should_ignore_path(
            &PathBuf::from("/proj/out/bundle.js"),
            &root()
        ));
    }

    #[test]
    fn does_not_ignore_bin_directory() {
        // `bin/` is too generic — some projects commit CLI shims there.
        // Never ignore it.
        assert!(!should_ignore_path(
            &PathBuf::from("/proj/bin/cli.ts"),
            &root()
        ));
    }

    #[test]
    fn does_not_ignore_src_files() {
        assert!(!should_ignore_path(
            &PathBuf::from("/proj/src/index.ts"),
            &root()
        ));
    }

    #[test]
    fn does_not_ignore_root_dotfiles() {
        // .env.local, .iii.worker.yaml etc. should trigger restart.
        assert!(!should_ignore_path(
            &PathBuf::from("/proj/.env.local"),
            &root()
        ));
    }

    #[test]
    fn does_not_ignore_file_named_like_artifact_dir() {
        // `node_modules.md` is not `node_modules`.
        assert!(!should_ignore_path(
            &PathBuf::from("/proj/docs/node_modules.md"),
            &root()
        ));
    }

    #[test]
    fn ignores_when_project_root_prefix_does_not_match() {
        // If notify emits an absolute path outside the root (shouldn't
        // normally happen, but be defensive), we still filter by name.
        assert!(should_ignore_path(
            &PathBuf::from("/other/node_modules/pkg/foo.js"),
            &root()
        ));
    }

    #[tokio::test]
    async fn watch_and_restart_fires_on_change() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::write(root.join("seed.txt"), "x").unwrap();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_cb = counter.clone();

        let worker_name = "test-worker".to_string();
        let root_clone = root.clone();
        let handle = tokio::spawn(async move {
            let _ = watch_and_restart(worker_name, root_clone, move |_, _| {
                counter_cb.fetch_add(1, Ordering::SeqCst);
            })
            .await;
        });

        // Let the watcher start.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Trigger a change.
        std::fs::write(root.join("src.ts"), "console.log('hi')").unwrap();

        // Wait > debounce window.
        tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS + 400)).await;

        assert!(
            counter.load(Ordering::SeqCst) >= 1,
            "expected >= 1 restart, got {}",
            counter.load(Ordering::SeqCst)
        );

        handle.abort();
    }

    #[tokio::test]
    async fn watch_and_restart_debounces_bursts() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_cb = counter.clone();

        let worker_name = "debounce-worker".to_string();
        let root_clone = root.clone();
        let handle = tokio::spawn(async move {
            let _ = watch_and_restart(worker_name, root_clone, move |_, _| {
                counter_cb.fetch_add(1, Ordering::SeqCst);
            })
            .await;
        });

        tokio::time::sleep(Duration::from_millis(200)).await;

        // Burst of 10 writes within the debounce window.
        for i in 0..10 {
            std::fs::write(root.join(format!("f{}.ts", i)), "x").unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        // Wait for debounce + drain.
        tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS + 400)).await;

        // One burst should coalesce to 1 or 2 restarts (events may straddle
        // the window boundary), never 10.
        let fired = counter.load(Ordering::SeqCst);
        assert!(
            fired >= 1 && fired <= 2,
            "expected 1-2 restarts, got {}",
            fired
        );

        handle.abort();
    }

    #[test]
    fn is_dep_manifest_detects_node() {
        assert!(is_dep_manifest(&PathBuf::from("/proj/package.json")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/package-lock.json")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/pnpm-lock.yaml")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/yarn.lock")));
    }

    #[test]
    fn is_dep_manifest_detects_python() {
        assert!(is_dep_manifest(&PathBuf::from("/proj/requirements.txt")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/pyproject.toml")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/poetry.lock")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/Pipfile")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/uv.lock")));
    }

    #[test]
    fn is_dep_manifest_detects_rust_go_ruby_elixir() {
        assert!(is_dep_manifest(&PathBuf::from("/proj/Cargo.toml")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/Cargo.lock")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/go.mod")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/go.sum")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/Gemfile")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/mix.exs")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/mix.lock")));
    }

    #[test]
    fn is_dep_manifest_detects_jvm_and_dotnet() {
        assert!(is_dep_manifest(&PathBuf::from("/proj/pom.xml")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/build.gradle")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/build.gradle.kts")));
        assert!(is_dep_manifest(&PathBuf::from("/proj/packages.config")));
    }

    #[test]
    fn is_dep_manifest_rejects_regular_source_files() {
        assert!(!is_dep_manifest(&PathBuf::from("/proj/src/index.ts")));
        assert!(!is_dep_manifest(&PathBuf::from("/proj/main.py")));
        assert!(!is_dep_manifest(&PathBuf::from("/proj/README.md")));
        // Not the same thing as package.json — some tools write these.
        assert!(!is_dep_manifest(&PathBuf::from("/proj/manifest.json")));
    }

    #[test]
    fn is_dep_manifest_rejects_empty_path() {
        assert!(!is_dep_manifest(&PathBuf::from("")));
    }

    #[tokio::test]
    async fn watch_and_restart_classifies_dep_manifest_change() {
        use std::sync::{Arc, Mutex};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::write(root.join("package.json"), "{}").unwrap();

        let observed: Arc<Mutex<Vec<ChangeKind>>> = Arc::new(Mutex::new(Vec::new()));
        let observed_cb = observed.clone();
        let root_clone = root.clone();
        let handle = tokio::spawn(async move {
            let _ = watch_and_restart("depworker".to_string(), root_clone, move |_, kind| {
                observed_cb.lock().unwrap().push(kind);
            })
            .await;
        });

        tokio::time::sleep(Duration::from_millis(200)).await;
        std::fs::write(root.join("package.json"), "{\"version\":\"1\"}").unwrap();
        tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS + 400)).await;

        let kinds = observed.lock().unwrap().clone();
        assert!(
            kinds.iter().any(|k| matches!(k, ChangeKind::DepManifest)),
            "expected at least one DepManifest classification, got {kinds:?}"
        );

        handle.abort();
    }

    #[tokio::test]
    async fn watch_and_restart_classifies_source_change() {
        use std::sync::{Arc, Mutex};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        let observed: Arc<Mutex<Vec<ChangeKind>>> = Arc::new(Mutex::new(Vec::new()));
        let observed_cb = observed.clone();
        let root_clone = root.clone();
        let handle = tokio::spawn(async move {
            let _ = watch_and_restart("srcworker".to_string(), root_clone, move |_, kind| {
                observed_cb.lock().unwrap().push(kind);
            })
            .await;
        });

        tokio::time::sleep(Duration::from_millis(200)).await;
        std::fs::write(root.join("index.ts"), "console.log(1)").unwrap();
        tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS + 400)).await;

        let kinds = observed.lock().unwrap().clone();
        assert!(
            kinds.iter().all(|k| matches!(k, ChangeKind::Source)),
            "expected only Source classifications, got {kinds:?}"
        );
        assert!(!kinds.is_empty(), "expected at least one classification");

        handle.abort();
    }

    #[tokio::test]
    async fn watch_and_restart_coalesces_mixed_burst_as_dep_manifest() {
        // Any single dep-manifest path in the debounced burst must
        // escalate the classification — otherwise we'd fast-restart
        // a worker whose dependencies just changed and be stuck with
        // stale node_modules until the next real restart.
        use std::sync::{Arc, Mutex};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        let observed: Arc<Mutex<Vec<ChangeKind>>> = Arc::new(Mutex::new(Vec::new()));
        let observed_cb = observed.clone();
        let root_clone = root.clone();
        let handle = tokio::spawn(async move {
            let _ = watch_and_restart("mixworker".to_string(), root_clone, move |_, kind| {
                observed_cb.lock().unwrap().push(kind);
            })
            .await;
        });

        tokio::time::sleep(Duration::from_millis(200)).await;
        // Mixed burst: three source files and one dep manifest.
        std::fs::write(root.join("a.ts"), "1").unwrap();
        std::fs::write(root.join("b.ts"), "1").unwrap();
        std::fs::write(root.join("package.json"), "{}").unwrap();
        std::fs::write(root.join("c.ts"), "1").unwrap();
        tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS + 400)).await;

        let kinds = observed.lock().unwrap().clone();
        assert!(
            kinds.iter().any(|k| matches!(k, ChangeKind::DepManifest)),
            "mixed burst with package.json must escalate to DepManifest, got {kinds:?}"
        );

        handle.abort();
    }

    #[tokio::test]
    async fn watch_and_restart_ignores_node_modules_writes() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let nm = root.join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_cb = counter.clone();

        let worker_name = "ignore-worker".to_string();
        let root_clone = root.clone();
        let handle = tokio::spawn(async move {
            let _ = watch_and_restart(worker_name, root_clone, move |_, _| {
                counter_cb.fetch_add(1, Ordering::SeqCst);
            })
            .await;
        });

        tokio::time::sleep(Duration::from_millis(200)).await;

        // Writes only to node_modules — should NOT fire.
        for i in 0..5 {
            std::fs::write(nm.join(format!("pkg{}.js", i)), "x").unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS + 400)).await;

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "node_modules writes must not trigger restarts"
        );

        handle.abort();
    }

    #[tokio::test]
    async fn watch_and_restart_fires_on_edit_inside_newly_created_dir() {
        // Regression: the initial `watch_pruned` pass only watches
        // directories that exist at startup. A `mkdir src/feature`
        // AFTER the watcher boots needs to be picked up so edits inside
        // it trigger restarts. This test exercises the create-dir path
        // (inotify on Linux; on macOS FSEvents handles it anyway, so
        // the assertion holds for different reasons).
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_cb = counter.clone();

        let worker_name = "new-dir-worker".to_string();
        let root_clone = root.clone();
        let handle = tokio::spawn(async move {
            let _ = watch_and_restart(worker_name, root_clone, move |_, _| {
                counter_cb.fetch_add(1, Ordering::SeqCst);
            })
            .await;
        });

        // Let the initial watcher come online.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // mkdir inside the project AFTER startup.
        let new_dir = root.join("feature");
        std::fs::create_dir_all(&new_dir).unwrap();

        // Wait past the debounce window so the mkdir burst completes
        // AND register_new_dirs runs on the resulting event. The
        // mkdir itself may or may not trigger a restart callback
        // depending on platform event coalescence — we don't assert on
        // that count yet, we just want the new-dir watch registered.
        tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS + 400)).await;
        let after_mkdir = counter.load(Ordering::SeqCst);

        // Now write INSIDE the new directory. Without the fix, inotify
        // has no watch descriptor on `feature/` so this write is
        // silent and the counter stays at `after_mkdir`.
        std::fs::write(new_dir.join("x.ts"), "console.log(1)").unwrap();
        tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS + 400)).await;

        let fired_after_nested_write = counter.load(Ordering::SeqCst);
        assert!(
            fired_after_nested_write > after_mkdir,
            "expected a restart after writing inside newly-created dir, \
             counter went {after_mkdir} -> {fired_after_nested_write}"
        );

        handle.abort();
    }
}
