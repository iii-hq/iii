// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Spawns non-built-in workers via `iii-worker start`.
//! All registry resolution, binary download, and OCI management is handled
//! by `iii-worker` itself — the engine only manages the child process lifecycle.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use serde_json::Value;
use tokio::sync::Mutex;

use crate::{engine::Engine, workers::traits::Worker};

// =============================================================================
// Path helpers
// =============================================================================

/// `HOME`-relative `.iii` directory, or `None` when `HOME` cannot be resolved.
///
/// Returning `Option` lets callers distinguish "no HOME" from "path under HOME
/// missing." The previous pattern — `dirs::home_dir().unwrap_or_default()` —
/// silently collapsed an unresolvable HOME into `""`, which then joined to
/// relative paths and produced file reads against the current working
/// directory. In `is_alive`, that made every liveness probe fail on machines
/// without a HOME, which the reload loop interprets as "dead → restart,"
/// producing a restart storm.
fn iii_home() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".iii"))
}

/// Candidate pidfile paths for `worker_name`, ordered by probe priority.
///
/// - OCI/VM workers: `~/.iii/managed/{name}/vm.pid`
/// - Binary workers: `~/.iii/pids/{name}.pid`
///
/// MUST stay in sync with `crates/iii-worker/src/cli/status.rs::pid_file_candidates`.
/// Engine and iii-worker are sibling crates with no shared dep; duplicating
/// this function keeps the path convention single-sourced within each crate.
/// If you add a third worker type or location here, mirror it there.
fn pid_file_candidates(home: &std::path::Path, worker_name: &str) -> [PathBuf; 2] {
    [
        home.join("managed").join(worker_name).join("vm.pid"),
        home.join("pids").join(format!("{}.pid", worker_name)),
    ]
}

/// Hardened pidfile read. Mirrors `iii-worker::cli::pidfile::read_pid`;
/// engine has no dep on iii-worker so the check is duplicated inline
/// alongside `pid_file_candidates`.
///
/// Defends against a local-user pidfile planting attack: without
/// O_NOFOLLOW + ownership check, an attacker with write access to
/// `~/.iii/pids/` (or `~/.iii/managed/<name>/`) can symlink a worker's
/// pidfile to any numeric-content file (e.g. `/proc/1/sched`, an
/// attacker-owned file with "1\n"). The engine then reads a wrong PID
/// and either (a) `kill(pid, 0)` succeeds against an unrelated live
/// process, making `is_alive` return true forever so the real worker
/// never gets restarted, or (b) in worst-case future uses, signals the
/// wrong PID. Requiring euid-ownership rejects planted files.
///
/// Returns `None` on any failure — the caller (`is_alive`) treats
/// unreadable pidfiles as "no live pidfile" and falls through to the
/// grace window, which is the correct conservative behavior.
#[cfg(unix)]
fn read_pid_hardened(path: &std::path::Path) -> Option<u32> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    let our_uid = unsafe { nix::libc::geteuid() };
    if meta.uid() != our_uid {
        return None;
    }
    let mut buf = [0u8; 32];
    let n = file.read(&mut buf).ok()?;
    let s = std::str::from_utf8(&buf[..n]).ok()?;
    s.trim().parse::<u32>().ok()
}

#[cfg(not(unix))]
fn read_pid_hardened(path: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

/// Build the argv handed to `iii-worker start` when the engine auto-spawns a
/// non-builtin worker. Kept pure so the CLI/engine IPC contract can be
/// regression-tested without spawning a real process (see tests).
///
/// Any drift here silently breaks the non-default `iii-worker-manager` port
/// case — the exact bug this module exists to fix.
fn spawn_args(worker_name: &str, port: u16) -> [String; 5] {
    // `--no-wait` keeps the child process short-lived. Without it the default
    // `wait=true` path pulls `wait_for_ready` → `watch_until_ready`, which
    // eprintln's a 500ms status-panel redraw loop into the engine-redirected
    // stderr.log. `iii worker logs -f` then tails that noise interleaved with
    // the VM's actual console output. The engine already probes liveness via
    // `is_alive`, so blocking the child on the panel loop is pure pollution.
    [
        "start".into(),
        worker_name.into(),
        "--port".into(),
        port.to_string(),
        "--no-wait".into(),
    ]
}

// =============================================================================
// iii-worker binary resolution
// =============================================================================

/// Resolve the `iii-worker` binary. Checks ~/.local/bin/ and system PATH.
pub fn resolve_iii_worker_binary() -> Option<PathBuf> {
    let exe_name = if cfg!(target_os = "windows") {
        "iii-worker.exe"
    } else {
        "iii-worker"
    };

    // Check ~/.local/bin/ (standard managed binary location)
    if let Some(home) = dirs::home_dir() {
        let managed_path = home.join(".local").join("bin").join(exe_name);
        if managed_path.exists() {
            return Some(managed_path);
        }
    }

    // Check system PATH
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(exe_name))
            .find(|p| p.exists())
    })
}

// =============================================================================
// ExternalWorkerProcess
// =============================================================================

/// A non-built-in worker process spawned via `iii-worker start`.
/// Handles both binary and OCI workers — iii-worker determines the type
/// and auto-installs from the registry if needed.
pub struct ExternalWorkerProcess {
    pub name: String,
    pub child: Arc<Mutex<Option<tokio::process::Child>>>,
    /// When the process was spawned. `is_alive` grants a grace window from
    /// this instant to cover the gap between `iii-worker start` exiting and
    /// the detached VM writing its pidfile.
    pub spawned_at: std::time::Instant,
}

/// How long after spawn we trust "still booting, no pidfile yet" as alive.
///
/// iii-worker start returns immediately after forking the detached VM boot
/// process. The VM then provisions rootfs, installs deps, and writes
/// `~/.iii/managed/{name}/vm.pid` only once libkrun is up. On a warm cache
/// this is sub-second; a cold first-boot with dep install can take tens of
/// seconds. 30s is conservative enough to avoid false-negative "dead" reads
/// during boot without masking a genuine crash for long.
const SPAWN_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

impl ExternalWorkerProcess {
    /// Spawns `iii-worker start <name> --port <port>` as a detached child.
    ///
    /// `port` is the engine's configured `iii-worker-manager` port; the CLI
    /// uses it to build the `III_ENGINE_URL` env var handed to the spawned
    /// VM-based worker so it connects back to the right place. When the
    /// engine runs on the default port this is equivalent to the pre-fix
    /// behavior; when it runs on a non-default port (e.g. SDK integration
    /// tests with multiple `iii-worker-manager` entries), the spawned worker
    /// no longer silently connects to the wrong port.
    pub async fn spawn(name: &str, port: u16) -> Result<Self, String> {
        let worker_binary = resolve_iii_worker_binary()
            .ok_or_else(|| {
                "iii-worker binary not found. Install with `iii update worker` or place in ~/.local/bin/".to_string()
            })?;

        let logs_dir = dirs::home_dir()
            .unwrap_or_default()
            .join(".iii/logs")
            .join(name);
        std::fs::create_dir_all(&logs_dir)
            .map_err(|e| format!("Failed to create logs dir: {}", e))?;

        let stdout_file = std::fs::File::create(logs_dir.join("stdout.log"))
            .map_err(|e| format!("Failed to create stdout log: {}", e))?;
        let stderr_file = std::fs::File::create(logs_dir.join("stderr.log"))
            .map_err(|e| format!("Failed to create stderr log: {}", e))?;

        let args = spawn_args(name, port);
        let mut cmd = tokio::process::Command::new(&worker_binary);
        cmd.args(&args).stdout(stdout_file).stderr(stderr_file);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn iii-worker for '{}': {}", name, e))?;

        tracing::info!(
            worker = %name,
            pid = ?child.id(),
            port = port,
            "Worker starting via iii-worker (logs: `iii worker logs {}`)", name
        );

        Ok(Self {
            name: name.to_string(),
            child: Arc::new(Mutex::new(Some(child))),
            spawned_at: std::time::Instant::now(),
        })
    }

    /// Probes whether the detached worker process is still alive.
    ///
    /// The real PID lives in one of two places depending on worker type:
    /// - OCI/VM workers: `~/.iii/managed/{name}/vm.pid`
    /// - Binary workers: `~/.iii/pids/{name}.pid`
    ///
    /// The tokio `Child` handle is stale because `iii-worker start` exits
    /// immediately after spawning the detached boot/worker process. Mirrors
    /// the dual-candidate lookup used by `iii worker status` (`read_pid` in
    /// `crates/iii-worker/src/cli/status.rs`).
    ///
    /// Returns:
    /// - `true` if any candidate pidfile exists and its PID responds to signal 0
    /// - `true` if we're still inside the post-spawn grace window (worker might
    ///   just be finishing boot and writing its pidfile)
    /// - `false` otherwise (crashed, force-stopped, or never booted)
    pub fn is_alive(&self) -> bool {
        // HOME-less machines can't observe pidfiles at all — skip the disk
        // probe entirely and let the grace window handle the early-boot case.
        // A permanent "unable to observe" state for a real worker will still
        // surface later as a dead probe once grace elapses, rather than a
        // false-positive restart storm triggered by nonsensical paths.
        let Some(home) = iii_home() else {
            return self.spawned_at.elapsed() < SPAWN_GRACE;
        };

        for pidfile in pid_file_candidates(&home, &self.name) {
            // `read_pid_hardened` uses O_NOFOLLOW + euid-ownership check
            // to reject attacker-planted symlinks or foreign-owned files;
            // see its docstring for the attacker model.
            if let Some(pid) = read_pid_hardened(&pidfile) {
                #[cfg(unix)]
                {
                    use nix::sys::signal::kill;
                    use nix::unistd::Pid;
                    if kill(Pid::from_raw(pid as i32), None).is_ok() {
                        return true;
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = pid;
                    return true;
                }
            }
        }

        // No live pidfile at either candidate. Grant a grace window from spawn
        // so we don't misread a still-booting worker as dead.
        self.spawned_at.elapsed() < SPAWN_GRACE
    }

    pub async fn stop(&self) {
        // iii-worker start spawns the actual worker as a detached process and
        // exits immediately, so the child handle here is already gone.
        // Use `iii-worker stop <name>` which reads the PID file and kills the
        // actual worker process.
        if let Some(binary) = resolve_iii_worker_binary() {
            let result = tokio::process::Command::new(&binary)
                .args(["stop", &self.name])
                .output()
                .await;
            match result {
                Ok(output) if output.status.success() => {
                    tracing::info!(worker = %self.name, "Worker stopped via iii-worker");
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    tracing::warn!(
                        worker = %self.name,
                        stderr = %stderr.trim(),
                        "iii-worker stop returned non-zero"
                    );
                }
                Err(e) => {
                    tracing::warn!(worker = %self.name, error = %e, "Failed to run iii-worker stop");
                }
            }
        } else {
            tracing::warn!(worker = %self.name, "Cannot stop worker: iii-worker binary not found");
        }

        // Clean up the child handle if it's still around
        let _ = self.child.lock().await.take();
    }
}

// =============================================================================
// ExternalWorkerWrapper (Worker trait impl)
// =============================================================================

/// Worker trait wrapper for external workers (binary or OCI via iii-worker).
pub struct ExternalWorkerWrapper {
    process: ExternalWorkerProcess,
    display_name: &'static str,
}

/// Intern a display name so the same worker name only ever leaks once.
///
/// The `Worker::name()` trait method returns `&'static str`, so the wrapper
/// must materialize a `&'static str` somewhere. Pre-fix, every call to
/// `ExternalWorkerWrapper::new` leaked a fresh boxed string, producing
/// unbounded growth under hot-reload (config watcher recreates wrappers for
/// every CHANGED/REVIVED entry). The intern cache caps the total leak at one
/// allocation per unique worker name for the engine's lifetime, which is the
/// natural upper bound of names the engine can legitimately reference.
///
/// Proper fix (changing `Worker::name` to return `&str` borrowed from `&self`)
/// would cascade to every `impl Worker` in the workspace — out of scope here.
fn intern_display_name(name: &str) -> &'static str {
    static INTERNED: OnceLock<std::sync::Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let cache = INTERNED.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut guard = cache.lock().expect("intern cache poisoned");
    if let Some(&existing) = guard.get(name) {
        return existing;
    }
    let display = format!("ExternalWorker({})", name);
    let leaked: &'static str = Box::leak(display.into_boxed_str());
    guard.insert(name.to_string(), leaked);
    leaked
}

impl ExternalWorkerWrapper {
    pub fn new(process: ExternalWorkerProcess) -> Self {
        let display_name = intern_display_name(&process.name);
        Self {
            process,
            display_name,
        }
    }
}

#[async_trait::async_trait]
impl Worker for ExternalWorkerWrapper {
    fn name(&self) -> &'static str {
        self.display_name
    }

    async fn create(_engine: Arc<Engine>, _config: Option<Value>) -> anyhow::Result<Box<dyn Worker>>
    where
        Self: Sized,
    {
        Err(anyhow::anyhow!(
            "ExternalWorkerWrapper::create should not be called directly"
        ))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        _shutdown_rx: tokio::sync::watch::Receiver<bool>,
        _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        // Shutdown is handled by destroy() which calls `iii-worker stop`.
        // No background task needed here since iii-worker start exits
        // immediately and the actual worker runs as a detached process.
        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        self.process.stop().await;
        Ok(())
    }

    async fn is_alive(&self) -> bool {
        self.process.is_alive()
    }

    fn register_functions(&self, _engine: Arc<Engine>) {
        // External workers register their own functions via the bridge protocol
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time assertion: ExternalWorkerProcess must be Send + Sync
    const _: () = {
        fn assert_send_sync<T: Send + Sync>() {}
        fn check() {
            assert_send_sync::<ExternalWorkerProcess>();
        }
        let _ = check;
    };

    /// Serializes tests that mutate HOME. HOME is process-global, so any two
    /// tests that override it must run one at a time. Uses `std::sync::Mutex`
    /// (blocking) rather than the tokio re-export pulled in by `super::*`.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that overrides HOME for the duration of a test and restores
    /// the original value (or removes the var if it was unset) on drop.
    struct HomeGuard {
        original: Option<std::ffi::OsString>,
    }

    impl HomeGuard {
        fn new(path: &std::path::Path) -> Self {
            let original = std::env::var_os("HOME");
            // SAFETY: test-only, serialized via ENV_LOCK.
            unsafe {
                std::env::set_var("HOME", path);
            }
            Self { original }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            // SAFETY: test-only, serialized via ENV_LOCK.
            unsafe {
                match &self.original {
                    Some(v) => std::env::set_var("HOME", v),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }

    /// Builds a process with `spawned_at` set far enough in the past that the
    /// grace-window fallback in `is_alive()` cannot mask a missing pidfile.
    fn stale_process(name: &str) -> ExternalWorkerProcess {
        let spawned_at = std::time::Instant::now()
            .checked_sub(SPAWN_GRACE * 2)
            .expect("clock has enough runway to subtract grace window");
        ExternalWorkerProcess {
            name: name.to_string(),
            child: Arc::new(Mutex::new(None)),
            spawned_at,
        }
    }

    /// Binary workers write their pid to `~/.iii/pids/{name}.pid`. `is_alive`
    /// must see it even when the VM pidfile is absent — the bug this guards
    /// against is the reload loop restarting healthy binary workers after the
    /// spawn grace window elapses.
    #[test]
    fn is_alive_finds_binary_worker_pidfile() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _home = HomeGuard::new(tmp.path());

        let name = "bin-worker";
        let pids_dir = tmp.path().join(".iii/pids");
        std::fs::create_dir_all(&pids_dir).unwrap();
        std::fs::write(
            pids_dir.join(format!("{}.pid", name)),
            std::process::id().to_string(),
        )
        .unwrap();

        let process = stale_process(name);
        assert!(
            process.is_alive(),
            "binary worker pidfile should keep is_alive true past the grace window"
        );
    }

    /// VM/OCI workers write their pid to `~/.iii/managed/{name}/vm.pid`.
    /// Ensures the dual-candidate refactor didn't break the original path.
    #[test]
    fn is_alive_finds_vm_worker_pidfile() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _home = HomeGuard::new(tmp.path());

        let name = "vm-worker";
        let managed_dir = tmp.path().join(".iii/managed").join(name);
        std::fs::create_dir_all(&managed_dir).unwrap();
        std::fs::write(managed_dir.join("vm.pid"), std::process::id().to_string()).unwrap();

        let process = stale_process(name);
        assert!(
            process.is_alive(),
            "vm.pid must still be honored after the dual-candidate refactor"
        );
    }

    /// No pidfile at either candidate and the grace window has elapsed →
    /// worker is dead. This is the signal `promote_dead_unchanged` uses to
    /// force a restart on reload.
    #[test]
    fn is_alive_returns_false_when_no_pidfile_and_grace_elapsed() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _home = HomeGuard::new(tmp.path());

        let process = stale_process("ghost-worker");
        assert!(
            !process.is_alive(),
            "no pidfile + grace window elapsed should report dead"
        );
    }

    /// Within the grace window, a missing pidfile is treated as "still
    /// booting", not dead. This is what keeps fresh `iii-worker start` calls
    /// from being immediately flagged for restart.
    #[test]
    fn is_alive_returns_true_within_grace_window() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _home = HomeGuard::new(tmp.path());

        let process = ExternalWorkerProcess {
            name: "booting-worker".to_string(),
            child: Arc::new(Mutex::new(None)),
            spawned_at: std::time::Instant::now(),
        };
        assert!(
            process.is_alive(),
            "freshly spawned worker with no pidfile yet must be alive during grace window"
        );
    }

    #[test]
    fn external_worker_wrapper_name_format() {
        let process = ExternalWorkerProcess {
            name: "test-worker".to_string(),
            child: Arc::new(Mutex::new(None)),
            spawned_at: std::time::Instant::now(),
        };
        let wrapper = ExternalWorkerWrapper::new(process);
        assert_eq!(wrapper.name(), "ExternalWorker(test-worker)");
    }

    #[tokio::test]
    async fn external_worker_wrapper_create_returns_error() {
        let engine = Arc::new(crate::engine::Engine::new());
        let result = ExternalWorkerWrapper::create(engine, None).await;
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("should not be called directly"));
    }

    #[tokio::test]
    async fn external_worker_wrapper_initialize_succeeds() {
        let process = ExternalWorkerProcess {
            name: "init-test".to_string(),
            child: Arc::new(Mutex::new(None)),
            spawned_at: std::time::Instant::now(),
        };
        let wrapper = ExternalWorkerWrapper::new(process);
        assert!(wrapper.initialize().await.is_ok());
    }

    #[tokio::test]
    async fn external_worker_wrapper_destroy_succeeds_with_no_child() {
        let process = ExternalWorkerProcess {
            name: "destroy-test".to_string(),
            child: Arc::new(Mutex::new(None)),
            spawned_at: std::time::Instant::now(),
        };
        let wrapper = ExternalWorkerWrapper::new(process);
        assert!(wrapper.destroy().await.is_ok());
    }

    /// The engine/CLI IPC contract: argv is `start <name> --port <port>` in
    /// that exact order. `iii-worker`'s clap parser is matched to this shape
    /// (see `crates/iii-worker/tests/worker_integration.rs::
    /// start_subcommand_matches_engine_spawn_args`). Drift here silently
    /// re-breaks non-default `iii-worker-manager` port setups — the exact
    /// regression this whole module exists to fix.
    #[test]
    fn spawn_args_emit_start_name_port_in_order() {
        let args = spawn_args("pdfkit", 49199);
        assert_eq!(
            args,
            [
                "start".to_string(),
                "pdfkit".to_string(),
                "--port".to_string(),
                "49199".to_string(),
                "--no-wait".to_string(),
            ],
        );
    }

    /// Regression lock: the engine auto-spawn MUST pass `--no-wait`.
    /// Dropping it resurrects the status-panel redraw loop that poisons
    /// `~/.iii/logs/<name>/stderr.log` (visible via `iii worker logs -f`).
    #[test]
    fn spawn_args_always_passes_no_wait() {
        let args = spawn_args("anything", 1234);
        assert!(
            args.iter().any(|a| a == "--no-wait"),
            "engine spawn must include --no-wait to avoid polluting stderr.log"
        );
    }

    #[test]
    fn spawn_args_default_port_serializes_as_digits() {
        // Pin that port formatting is decimal digits, not something clap
        // would reject like "0x1234". u16::MAX is the boundary case.
        let args = spawn_args("x", u16::MAX);
        assert_eq!(args[3], "65535");
    }

    /// Intern cache caps the Box::leak growth: the same worker name must
    /// resolve to the same `&'static str` across wrappers. If this flips
    /// (e.g. someone removes the HashMap lookup), every config reload will
    /// leak a fresh string for the same name and the original unbounded-
    /// growth bug returns.
    #[test]
    fn intern_display_name_returns_same_pointer_for_same_name() {
        let a = intern_display_name("interned-worker");
        let b = intern_display_name("interned-worker");
        assert!(
            std::ptr::eq(a.as_ptr(), b.as_ptr()),
            "same name must intern to the same allocation"
        );
    }

    #[test]
    fn intern_display_name_differs_across_names() {
        let a = intern_display_name("worker-a-unique");
        let b = intern_display_name("worker-b-unique");
        assert_ne!(a, b);
        assert!(a.contains("worker-a-unique"));
        assert!(b.contains("worker-b-unique"));
    }

    #[test]
    fn pid_file_candidates_orders_vm_then_binary() {
        let home = std::path::Path::new("/fake/home/.iii");
        let got = pid_file_candidates(home, "demo");
        assert_eq!(got[0], home.join("managed/demo/vm.pid"));
        assert_eq!(got[1], home.join("pids/demo.pid"));
    }

    /// Engine-side mirror of iii-worker's
    /// `pidfile::tests::known_call_sites_route_through_module`. Pins the
    /// three hardening invariants in `read_pid_hardened` so a future
    /// refactor can't silently drop O_NOFOLLOW, the uid-ownership check,
    /// or the regular-file check and regress the attacker model.
    /// Engine has no dep on iii-worker, so the hardening is duplicated
    /// inline above — this grep-style test is the only thing preventing
    /// the duplicate from drifting out of sync with the canonical copy.
    #[test]
    #[cfg(unix)]
    fn read_pid_hardened_retains_hardening_tokens() {
        // `file!()` is relative to the crate root, so anchor via
        // `CARGO_MANIFEST_DIR` for reliable lookup under any `cargo test`
        // invocation (workspace-relative vs crate-relative cwd).
        let manifest = env!("CARGO_MANIFEST_DIR");
        let path = std::path::Path::new(manifest).join("src/workers/registry_worker.rs");
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {:?} for self-grep: {}", path, e));
        for token in ["O_NOFOLLOW", "meta.uid()", "file_type().is_file()"] {
            assert!(
                src.contains(token),
                "read_pid_hardened must reference `{}` — dropping it regresses the pidfile \
                 attacker model (see iii-worker::cli::pidfile docstring). If you genuinely \
                 need to remove this check, reproduce the security rationale in the commit \
                 message and update this test.",
                token,
            );
        }
    }
}
