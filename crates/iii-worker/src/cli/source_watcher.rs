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
    let ignored: HashSet<&str> = IGNORED_DIR_NAMES.iter().copied().collect();

    let rel = path.strip_prefix(project_root).unwrap_or(path);
    for component in rel.components() {
        if let std::path::Component::Normal(os) = component
            && let Some(s) = os.to_str()
            && ignored.contains(s)
        {
            return true;
        }
    }
    false
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
    F: FnMut(&str) + Send + 'static,
{
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<notify::Event>();

    let root_for_filter = project_path.clone();
    let mut watcher = notify::RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                use notify::EventKind;
                let is_change = matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                );
                if !is_change {
                    return;
                }
                let all_ignored = !event.paths.is_empty()
                    && event
                        .paths
                        .iter()
                        .all(|p| should_ignore_path(p, &root_for_filter));
                if all_ignored {
                    return;
                }
                let _ = tx.send(event);
            }
        },
        notify::Config::default(),
    )?;

    watcher.watch(&project_path, RecursiveMode::Recursive)?;

    tracing::info!(
        worker = %worker_name,
        path = %project_path.display(),
        "source watcher: online"
    );

    loop {
        let event = match rx.recv().await {
            Some(e) => e,
            None => {
                tracing::warn!(worker = %worker_name, "source watcher: channel closed");
                break;
            }
        };

        // Debounce: swallow the initial event, then drain everything
        // that arrives during the quiet window.
        drop(event);
        tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)).await;
        while rx.try_recv().is_ok() {}

        tracing::info!(
            worker = %worker_name,
            "source watcher: change detected, restarting VM"
        );
        on_change(&worker_name);
    }

    Ok(())
}

/// Production restart callback: spawn `iii-worker start <name>` and
/// wait for it. `start` itself calls `kill_stale_worker` before booting,
/// so a single invocation both tears down the old VM and brings up a
/// fresh one.
pub fn restart_via_cli(worker_name: &str) {
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
            tracing::info!(worker = %worker_name, "source watcher: restart ok");
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
            let _ = watch_and_restart(worker_name, root_clone, move |_| {
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
            let _ = watch_and_restart(worker_name, root_clone, move |_| {
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
            let _ = watch_and_restart(worker_name, root_clone, move |_| {
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
}
