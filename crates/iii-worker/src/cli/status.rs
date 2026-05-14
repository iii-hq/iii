// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii worker status` — inspect what's happening to a worker without
//! context-switching to the engine terminal.
//!
//! Probes filesystem state (config.yaml entry, managed dir, prepared marker,
//! pid file, logs freshness) and the engine's TCP port, then renders a
//! human-readable snapshot. Supports `--watch` for a refreshing live view and
//! is also the primitive that powers `iii worker add --wait`.

use colored::Colorize;
use std::time::{Duration, Instant, SystemTime};

use super::config_file::{ResolvedWorkerType, resolve_worker_type, worker_exists};
use super::managed::{is_engine_running_on, is_worker_running};

/// Terminal phase for waiters — once we reach `Ready` or `Failed` we stop
/// polling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Worker absent from config.yaml.
    NotInConfig,
    /// In config.yaml but engine isn't running, so nothing will boot it.
    EngineDown,
    /// In config.yaml, engine up, but no managed dir yet.
    Queued,
    /// Managed dir exists, rootfs being prepared or deps installing.
    Preparing,
    /// `.iii-prepared` marker present but no live pid yet.
    Booting,
    /// Pid file points at a live process.
    Ready,
    /// Sentinel for callers that want to treat "engine up + worker unknown"
    /// as a non-fatal intermediate state.
    Failed,
}

/// Inputs to [`derive_phase`]. Pure-function boundary so the classification
/// rules can be tested without touching disk or sockets.
pub struct DeriveInputs {
    pub engine_running: bool,
    pub is_binary: bool,
    /// Local-path workers run an in-VM deps-install script that terminates
    /// by writing `/var/.iii-prepared` (see `build_libkrun_local_script` in
    /// `local_worker.rs`). Only they produce or consume that marker. OCI
    /// images bake their deps at build time, so the marker is meaningless
    /// there — treating it as a readiness gate for OCI keeps workers stuck
    /// in `Phase::Preparing` forever, even after they've connected to the
    /// engine and registered functions.
    pub is_local: bool,
    pub running: bool,
    pub prepared: bool,
    pub managed_dir_exists: bool,
}

/// Classify a worker into a [`Phase`] given its observable state.
///
/// Worker-kind-specific gates, evaluated in order:
///
/// **Binary** — plain host processes, independent of the engine's TCP port
/// once spawned. Live pid = Ready; dead = Failed. Gating on `engine_running`
/// would shadow binary workers as `EngineDown` during an engine restart, and
/// would leave `iii worker add --wait <binary>` hanging because `EngineDown`
/// is not a terminal wait phase.
///
/// **OCI** — image has deps baked in; no in-VM install step runs. The
/// `.iii-prepared` marker is never written (see `start_oci_worker`). For
/// these, `running` alone is the honest ready signal: the worker process
/// is up inside libkrun and has already connected to the engine.
///
/// **Local** — rootfs cloned, then an in-guest init script runs
/// setup/install (which can take minutes on first boot with a cold dep
/// cache). `Ready` requires BOTH a live VM pid AND `.iii-prepared`, because
/// the VM pid goes live the moment libkrun boots — well before the deps
/// install finishes.
pub fn derive_phase(inputs: DeriveInputs) -> Phase {
    let DeriveInputs {
        engine_running,
        is_binary,
        is_local,
        running,
        prepared,
        managed_dir_exists,
    } = inputs;

    if is_binary {
        // Binary workers are host processes spawned by the engine's
        // file-watcher pipeline: config.yaml change → reload →
        // ExternalWorkerProcess::spawn → iii-worker start → child process
        // writes ~/.iii/pids/{name}.pid. The config-to-pid window is
        // typically 100ms-several-seconds depending on engine load.
        //
        // A missing pidfile during that window is NOT "Failed" — it's
        // "engine hasn't spawned it yet." Returning `Failed` here used to
        // make `iii worker add <binary> --wait` exit "failed after 0s"
        // because Phase::Failed is terminal for wait, so watch_until_ready
        // bailed before the engine even saw the config change.
        //
        // Use `Queued` when the engine is up (non-terminal → wait keeps
        // polling until pid appears OR the 120s timeout fires) and
        // `EngineDown` when it isn't (the engine has to come up first;
        // same semantics as VM workers in that state).
        return if running {
            Phase::Ready
        } else if engine_running {
            Phase::Queued
        } else {
            Phase::EngineDown
        };
    }
    if !engine_running {
        return Phase::EngineDown;
    }
    if !is_local {
        // OCI (or builtin `Config`) paths: no deps-install marker lifecycle.
        // Running is the honest ready signal; absence reverts to the boot
        // staging signals we can still observe.
        return if running {
            Phase::Ready
        } else if managed_dir_exists {
            Phase::Preparing
        } else {
            Phase::Queued
        };
    }
    if running && prepared {
        Phase::Ready
    } else if running {
        // VM is up, deps still installing. Surface as Preparing — the
        // sandbox line already explains the detail.
        Phase::Preparing
    } else if prepared {
        // Deps installed previously, VM restarting (e.g. after --force we
        // wiped the managed dir, but this branch covers the "second boot on
        // a warm cache" window).
        Phase::Booting
    } else if managed_dir_exists {
        Phase::Preparing
    } else {
        Phase::Queued
    }
}

/// Full snapshot of one worker at one point in time.
pub struct WorkerStatus {
    pub name: String,
    pub phase: Phase,
    pub engine_running: bool,
    pub worker_type: Option<&'static str>,
    pub worker_path: Option<String>,
    pub managed_dir_exists: bool,
    pub prepared: bool,
    pub pid: Option<u32>,
    /// True iff the VM process is responding to signal 0. Tracked separately
    /// from phase so the render can distinguish "alive, deps still
    /// installing" (Preparing) from "stale pidfile" (dead).
    pub alive: bool,
    pub logs_dir: Option<std::path::PathBuf>,
    pub logs_last_modified: Option<SystemTime>,
}

impl WorkerStatus {
    /// Probes using the configured `iii-worker-manager` port from
    /// config.yaml. Prefer [`Self::probe_on`] at call sites that already
    /// hold a port so a `--port` override isn't silently ignored.
    pub fn probe(name: &str) -> Self {
        Self::probe_on(name, super::config_file::manager_port())
    }

    pub fn probe_on(name: &str, port: u16) -> Self {
        let engine_running = is_engine_running_on(port);
        let exists_in_config = worker_exists(name);

        if !exists_in_config {
            return Self {
                name: name.to_string(),
                phase: Phase::NotInConfig,
                engine_running,
                worker_type: None,
                worker_path: None,
                managed_dir_exists: false,
                prepared: false,
                pid: None,
                alive: false,
                logs_dir: None,
                logs_last_modified: None,
            };
        }

        let resolved = resolve_worker_type(name);
        let (worker_type, worker_path) = match &resolved {
            ResolvedWorkerType::Local { worker_path } => ("local", Some(worker_path.clone())),
            ResolvedWorkerType::Oci { .. } => ("oci", None),
            ResolvedWorkerType::Binary { .. } => ("binary", None),
            ResolvedWorkerType::Config => ("config", None),
        };
        let is_binary = matches!(resolved, ResolvedWorkerType::Binary { .. });
        let is_local = matches!(resolved, ResolvedWorkerType::Local { .. });

        let home = dirs::home_dir().unwrap_or_default();
        let managed_dir = home.join(".iii/managed").join(name);
        let managed_dir_exists = managed_dir.is_dir();
        let prepared = managed_dir.join("var").join(".iii-prepared").exists();

        let pid = read_pid(name);
        let running = is_worker_running(name);

        let logs_dir_candidate = home.join(".iii/logs").join(name);
        let logs_dir = if logs_dir_candidate.is_dir() {
            Some(logs_dir_candidate.clone())
        } else {
            None
        };
        let logs_last_modified = logs_dir.as_ref().and_then(|d| {
            ["stdout.log", "stderr.log"]
                .iter()
                .filter_map(|f| std::fs::metadata(d.join(f)).ok())
                .filter_map(|m| m.modified().ok())
                .max()
        });

        let phase = derive_phase(DeriveInputs {
            engine_running,
            is_binary,
            is_local,
            running,
            prepared,
            managed_dir_exists,
        });

        Self {
            name: name.to_string(),
            phase,
            engine_running,
            worker_type: Some(worker_type),
            worker_path,
            managed_dir_exists,
            prepared,
            pid,
            alive: running,
            logs_dir,
            logs_last_modified,
        }
    }

    /// Human-readable one-line headline, used for --wait spinners and the
    /// banner on `iii worker status`.
    pub fn headline(&self) -> String {
        match self.phase {
            Phase::NotInConfig => format!("{} {} not in config.yaml", "✗".red(), self.name.bold()),
            Phase::EngineDown => {
                format!("{} engine not running (start it with `iii`)", "⚠".yellow())
            }
            Phase::Queued => {
                // Binary workers don't have a sandbox; the engine spawns
                // them as plain host processes. Tailor the message so we
                // don't confuse the user into looking for a VM that will
                // never exist.
                let tail = if self.worker_type == Some("binary") {
                    "engine will spawn it shortly"
                } else {
                    "engine will boot its sandbox shortly"
                };
                format!("{} {} queued — {}", "⟳".cyan(), self.name.bold(), tail)
            }
            Phase::Preparing => {
                if self.alive {
                    // VM kernel booted; init script is running setup/install
                    // inside the guest. Surface the pid so the user knows
                    // something is actually happening.
                    format!(
                        "{} {} installing deps inside VM (pid {})",
                        "⟳".cyan(),
                        self.name.bold(),
                        self.pid.map(|p| p.to_string()).unwrap_or_default()
                    )
                } else {
                    format!(
                        "{} {} preparing sandbox (rootfs / deps)",
                        "⟳".cyan(),
                        self.name.bold()
                    )
                }
            }
            Phase::Booting => format!("{} {} booting VM", "⟳".cyan(), self.name.bold()),
            Phase::Ready => format!(
                "{} {} ready (pid {})",
                "✓".green(),
                self.name.bold(),
                self.pid.map(|p| p.to_string()).unwrap_or_default()
            ),
            Phase::Failed => format!("{} {} failed", "✗".red(), self.name.bold()),
        }
    }

    pub fn is_terminal_for_wait(&self) -> bool {
        matches!(
            self.phase,
            Phase::Ready | Phase::NotInConfig | Phase::Failed
        )
    }

    /// Render a detailed multi-line snapshot. Returns the number of lines
    /// written so `--watch` knows how much to rewind.
    pub fn render(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(String::new());
        out.push(format!("  {}", self.headline()));
        out.push(String::new());

        let engine_line = if self.engine_running {
            format!("{:>12}  {}", "engine:".dimmed(), "running".green())
        } else {
            format!(
                "{:>12}  {} {}",
                "engine:".dimmed(),
                "stopped".red(),
                "(run `iii` in another terminal)".dimmed()
            )
        };
        out.push(engine_line);

        let config_line = if matches!(self.phase, Phase::NotInConfig) {
            format!(
                "{:>12}  {} {}",
                "config:".dimmed(),
                "missing".red(),
                "(add with `iii worker add <path-or-name>`)".dimmed()
            )
        } else {
            let ty = self.worker_type.unwrap_or("?");
            let path = self
                .worker_path
                .as_deref()
                .map(|p| format!(" ({})", p.dimmed()))
                .unwrap_or_default();
            format!(
                "{:>12}  {} type={}{}",
                "config:".dimmed(),
                "present".green(),
                ty,
                path
            )
        };
        out.push(config_line);

        let sandbox_line = if self.worker_type == Some("binary") {
            format!(
                "{:>12}  {}",
                "sandbox:".dimmed(),
                "n/a (binary worker runs on host)".dimmed()
            )
        } else if self.worker_type == Some("oci") {
            // OCI images bake deps at build time — no in-VM install, no
            // `.iii-prepared` marker. The honest statuses here are:
            //   - no managed dir yet (pre-boot)
            //   - running (libkrun up, deps were in the image)
            // Suppressing the local-only "deps still installing" line.
            match (self.managed_dir_exists, self.alive) {
                (false, _) => format!(
                    "{:>12}  {}",
                    "sandbox:".dimmed(),
                    "no managed dir yet".dimmed()
                ),
                (true, true) => format!(
                    "{:>12}  {} (image-baked deps)",
                    "sandbox:".dimmed(),
                    "running".green()
                ),
                (true, false) => format!(
                    "{:>12}  {} (rootfs ready)",
                    "sandbox:".dimmed(),
                    "prepared".green()
                ),
            }
        } else {
            match (self.managed_dir_exists, self.prepared) {
                (false, _) => format!(
                    "{:>12}  {}",
                    "sandbox:".dimmed(),
                    "no managed dir yet".dimmed()
                ),
                (true, false) => format!(
                    "{:>12}  {} (rootfs cloned, deps still installing)",
                    "sandbox:".dimmed(),
                    "preparing".yellow()
                ),
                (true, true) => format!(
                    "{:>12}  {} (rootfs + deps cached)",
                    "sandbox:".dimmed(),
                    "prepared".green()
                ),
            }
        };
        out.push(sandbox_line);

        let process_line = match (self.pid, self.alive) {
            (Some(p), true) => {
                format!("{:>12}  {} pid={}", "process:".dimmed(), "alive".green(), p)
            }
            (Some(p), false) => format!(
                "{:>12}  {} pid={} (stale pidfile)",
                "process:".dimmed(),
                "dead".red(),
                p
            ),
            (None, _) => format!("{:>12}  {}", "process:".dimmed(), "not started".dimmed()),
        };
        out.push(process_line);

        let logs_line = match (&self.logs_dir, self.logs_last_modified) {
            (Some(_), Some(_)) => format!(
                "{:>12}  {} {}",
                "logs:".dimmed(),
                "available".green(),
                format!("(tail with `iii worker logs {} -f`)", self.name).dimmed()
            ),
            (Some(_), None) => format!(
                "{:>12}  {}",
                "logs:".dimmed(),
                "directory exists, no content yet".dimmed()
            ),
            (None, _) => format!("{:>12}  {}", "logs:".dimmed(), "none yet".dimmed()),
        };
        out.push(logs_line);
        out.push(String::new());
        out
    }
}

/// Candidate pidfile paths for `worker_name`, ordered by probe priority.
///
/// - OCI/VM workers: `~/.iii/managed/{name}/vm.pid`
/// - Binary workers: `~/.iii/pids/{name}.pid`
///
/// MUST stay in sync with `engine/src/workers/registry_worker.rs::
/// pid_file_candidates`. Engine and iii-worker are sibling crates with no
/// shared dep; duplicating this function keeps the path convention single-
/// sourced within each crate. If you add a third worker type or location
/// here, mirror it there.
pub(crate) fn pid_file_candidates(
    home: &std::path::Path,
    worker_name: &str,
) -> [std::path::PathBuf; 2] {
    [
        home.join("managed").join(worker_name).join("vm.pid"),
        home.join("pids").join(format!("{}.pid", worker_name)),
    ]
}

pub(crate) fn read_pid(name: &str) -> Option<u32> {
    let home = dirs::home_dir()?.join(".iii");
    for path in pid_file_candidates(&home, name) {
        // Route through the shared hardened reader so the status CLI
        // can't be tricked by a symlink-planted pidfile into displaying
        // or waiting on an attacker-chosen PID. See `pidfile` module
        // docstring for the attacker model.
        if let Some(pid) = super::pidfile::read_pid(&path) {
            return Some(pid);
        }
    }
    None
}

/// Entry point for `iii worker status`. Resolves the engine port from
/// config.yaml; there is no CLI override for this command today (it has
/// no `--port` flag) because `iii worker status` is a diagnostic.
pub async fn handle_worker_status(worker_name: &str, watch: bool) -> i32 {
    let port = super::config_file::manager_port();
    if !watch {
        let status = WorkerStatus::probe_on(worker_name, port);
        for line in status.render() {
            eprintln!("{}", line);
        }
        return match status.phase {
            Phase::Ready => 0,
            Phase::NotInConfig => 1,
            _ => 0,
        };
    }

    // --watch: live-redraw with no timeout (Ctrl-C to abort).
    let final_status = watch_until_ready(worker_name, None, port).await;
    match final_status.phase {
        Phase::Ready => 0,
        Phase::NotInConfig => 1,
        _ => 0,
    }
}

/// Live-redraw the snapshot in place every 500ms until the worker reaches a
/// terminal wait phase (Ready / NotInConfig / Failed), `timeout` elapses, or
/// the process is killed.
///
/// `timeout = None` means "wait forever" (used by `--watch`); `Some(d)` is
/// used by `--wait` so the CLI doesn't hang on a stuck VM. `port` is the
/// engine's `iii-worker-manager` port — each probe re-checks it so the
/// "engine: running/stopped" line reflects the correct engine even when
/// the user runs on a non-default port.
///
/// Returns the final status so callers can render a closing message.
pub async fn watch_until_ready(
    worker_name: &str,
    timeout: Option<Duration>,
    port: u16,
) -> WorkerStatus {
    let started = Instant::now();
    let mut first = true;
    loop {
        let status = WorkerStatus::probe_on(worker_name, port);
        let lines = status.render();

        if !first {
            // `\x1b[{n}F` = move cursor up n lines to start of line,
            // `\x1b[J` = clear from cursor to end of screen. This keeps the
            // snapshot pinned in place even as line counts shrink.
            let n = lines.len();
            eprint!("\x1b[{}F\x1b[J", n);
        }
        first = false;

        for line in &lines {
            eprintln!("{}", line);
        }

        if status.is_terminal_for_wait() {
            return status;
        }
        if let Some(t) = timeout
            && started.elapsed() >= t
        {
            return status;
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_is_terminal_for_wait_only_on_ready_or_missing_or_failed() {
        // Building a WorkerStatus manually to avoid touching disk.
        let base = WorkerStatus {
            name: "t".into(),
            phase: Phase::Ready,
            engine_running: true,
            worker_type: Some("local"),
            worker_path: None,
            managed_dir_exists: true,
            prepared: true,
            pid: Some(42),
            alive: true,
            logs_dir: None,
            logs_last_modified: None,
        };
        assert!(base.is_terminal_for_wait());

        let mut not_in = WorkerStatus {
            phase: Phase::NotInConfig,
            ..base
        };
        assert!(not_in.is_terminal_for_wait());

        not_in.phase = Phase::Queued;
        assert!(!not_in.is_terminal_for_wait());

        not_in.phase = Phase::Preparing;
        assert!(!not_in.is_terminal_for_wait());

        not_in.phase = Phase::Booting;
        assert!(!not_in.is_terminal_for_wait());

        not_in.phase = Phase::EngineDown;
        assert!(!not_in.is_terminal_for_wait());

        not_in.phase = Phase::Failed;
        assert!(not_in.is_terminal_for_wait());
    }

    #[test]
    fn render_includes_all_major_sections() {
        let s = WorkerStatus {
            name: "demo".into(),
            phase: Phase::Ready,
            engine_running: true,
            worker_type: Some("local"),
            worker_path: Some("/abs/path".into()),
            managed_dir_exists: true,
            prepared: true,
            pid: Some(123),
            alive: true,
            logs_dir: Some(std::path::PathBuf::from("/tmp/logs")),
            logs_last_modified: Some(SystemTime::now()),
        };
        let text = s.render().join("\n");
        assert!(text.contains("engine:"));
        assert!(text.contains("config:"));
        assert!(text.contains("sandbox:"));
        assert!(text.contains("process:"));
        assert!(text.contains("logs:"));
        assert!(text.contains("demo"));
    }

    #[test]
    fn headline_variants() {
        let base = WorkerStatus {
            name: "w".into(),
            phase: Phase::NotInConfig,
            engine_running: false,
            worker_type: None,
            worker_path: None,
            managed_dir_exists: false,
            prepared: false,
            pid: None,
            alive: false,
            logs_dir: None,
            logs_last_modified: None,
        };
        assert!(base.headline().contains("not in config.yaml"));

        let s = WorkerStatus {
            phase: Phase::EngineDown,
            ..base
        };
        assert!(s.headline().contains("start it with `iii`"));
    }

    #[test]
    fn preparing_headline_with_live_vm_mentions_deps_installing() {
        // Regression: `--watch` used to close the moment the libkrun pid
        // went alive, even though the in-guest init script was still
        // running npm/pip/etc. The honest ready signal is pid alive AND
        // .iii-prepared marker, so this intermediate state must render as
        // "installing deps inside VM" not as Ready.
        let s = WorkerStatus {
            name: "todo-worker-python".into(),
            phase: Phase::Preparing,
            engine_running: true,
            worker_type: Some("local"),
            worker_path: None,
            managed_dir_exists: true,
            prepared: false,
            pid: Some(16795),
            alive: true,
            logs_dir: None,
            logs_last_modified: None,
        };
        let h = s.headline();
        assert!(
            h.contains("installing deps inside VM"),
            "headline should say 'installing deps inside VM', got: {}",
            h
        );
        assert!(
            h.contains("16795"),
            "headline should include pid, got: {}",
            h
        );
        assert!(
            !s.is_terminal_for_wait(),
            "preparing-with-live-vm must NOT be terminal — --watch must keep running"
        );
    }

    /// Regression: binary workers have no VM and no `.iii-prepared` marker.
    /// The probe used to fall through to `Preparing` with the "installing deps
    /// inside VM" headline and the status watcher would hang forever waiting
    /// for a marker that would never appear. For binary workers: alive pid =
    /// Ready, immediately.
    #[test]
    fn binary_worker_alive_headline_and_render() {
        let s = WorkerStatus {
            name: "image-resize".into(),
            phase: Phase::Ready,
            engine_running: true,
            worker_type: Some("binary"),
            worker_path: None,
            managed_dir_exists: false,
            prepared: false,
            pid: Some(48350),
            alive: true,
            logs_dir: None,
            logs_last_modified: None,
        };
        let h = s.headline();
        assert!(
            h.contains("ready"),
            "binary+alive must headline as ready, got: {}",
            h
        );
        assert!(
            !h.contains("installing deps inside VM"),
            "binary workers have no VM — this string must not appear, got: {}",
            h
        );
        let text = s.render().join("\n");
        assert!(
            text.contains("n/a (binary worker runs on host)"),
            "binary worker sandbox row must say n/a, got:\n{}",
            text
        );
        assert!(
            !text.contains("no managed dir yet"),
            "sandbox row must not dangle 'no managed dir yet' for a binary worker"
        );
        assert!(
            s.is_terminal_for_wait(),
            "binary+alive must be terminal so --wait exits"
        );
    }

    #[test]
    fn process_line_shows_alive_based_on_alive_field_not_phase() {
        // Regression: process row used to check `matches!(phase, Ready)`
        // and would print "dead pid=X (stale pidfile)" for a genuinely
        // alive pid whose phase was Preparing (deps still installing).
        let s = WorkerStatus {
            name: "w".into(),
            phase: Phase::Preparing,
            engine_running: true,
            worker_type: Some("local"),
            worker_path: None,
            managed_dir_exists: true,
            prepared: false,
            pid: Some(16795),
            alive: true,
            logs_dir: None,
            logs_last_modified: None,
        };
        let text = s.render().join("\n");
        assert!(
            text.contains("alive") && text.contains("16795"),
            "process row must show 'alive pid=16795', got:\n{}",
            text
        );
        assert!(
            !text.contains("stale pidfile"),
            "process row must NOT call a live pid stale, got:\n{}",
            text
        );
    }

    /// Regression for the classifier bug where binary workers reported
    /// `Phase::EngineDown` whenever the engine's TCP port was down, even
    /// though a binary worker's host process is independent of that port
    /// once spawned. The consequences: the `status` headline would hide the
    /// worker behind an "engine not running" banner, and
    /// `iii worker add --wait <binary>` would hang forever because
    /// `EngineDown` is not a terminal wait phase.
    ///
    /// Fix: evaluate `is_binary` before `engine_running` in `derive_phase`.
    /// If either assertion flips (e.g., someone re-introduces the old gate),
    /// `--wait` on binary workers will silently hang again.
    #[test]
    fn derive_phase_binary_alive_ignores_engine_down() {
        let p = derive_phase(DeriveInputs {
            engine_running: false,
            is_binary: true,
            is_local: false,
            running: true,
            prepared: false,
            managed_dir_exists: false,
        });
        assert_eq!(
            p,
            Phase::Ready,
            "binary worker with a live pid must classify as Ready regardless of engine port state"
        );
    }

    #[test]
    fn derive_phase_binary_engine_down_is_engine_down_not_failed() {
        // Binary workers are spawned by the engine's file watcher; without
        // a live engine nothing will spawn them. `EngineDown` is the honest
        // signal here — the previous `Failed` classification conflated
        // "engine not up" with "crashed after spawn" and led users to
        // restart things that didn't actually need restarting.
        let p = derive_phase(DeriveInputs {
            engine_running: false,
            is_binary: true,
            is_local: false,
            running: false,
            prepared: false,
            managed_dir_exists: false,
        });
        assert_eq!(p, Phase::EngineDown);
    }

    /// Regression for the reported bug where `iii worker add image-resize`
    /// (a binary worker) printed "✗ image-resize failed" / "not ready
    /// after 0s" the instant the `add --wait` path invoked the probe —
    /// before the engine had a chance to pick up the config change and
    /// spawn the worker.
    ///
    /// Root cause: binary + not running returned Phase::Failed
    /// unconditionally. Phase::Failed is terminal for wait
    /// (`is_terminal_for_wait`), so `watch_until_ready` exited immediately.
    ///
    /// Fix: binary + engine_running + not running = Phase::Queued
    /// (non-terminal, wait keeps polling). The wait-loop's own 120s
    /// timeout still bounds genuine crash loops.
    #[test]
    fn derive_phase_binary_waiting_for_engine_to_spawn_is_queued() {
        let p = derive_phase(DeriveInputs {
            engine_running: true,
            is_binary: true,
            is_local: false,
            running: false,
            prepared: false,
            managed_dir_exists: false,
        });
        assert_eq!(p, Phase::Queued);
        // Also assert non-terminal so a future change to is_terminal_for_wait
        // can't silently re-introduce the "failed after 0s" UX.
        let status = WorkerStatus {
            name: "bin".into(),
            phase: p,
            engine_running: true,
            worker_type: Some("binary"),
            worker_path: None,
            managed_dir_exists: false,
            prepared: false,
            pid: None,
            alive: false,
            logs_dir: None,
            logs_last_modified: None,
        };
        assert!(
            !status.is_terminal_for_wait(),
            "binary worker pending spawn must NOT be terminal — \
             otherwise watch_until_ready exits before the engine spawns."
        );
    }

    #[test]
    fn derive_phase_vm_worker_engine_down_still_classifies_as_engine_down() {
        // VM workers DO depend on the engine to boot libkrun, so the
        // EngineDown gate is still load-bearing for them. If this flips,
        // VM workers will be misclassified as Queued/Preparing during an
        // engine restart and `--wait` will spin on a phase that can never
        // advance.
        let p = derive_phase(DeriveInputs {
            engine_running: false,
            is_binary: false,
            is_local: true,
            running: false,
            prepared: false,
            managed_dir_exists: false,
        });
        assert_eq!(p, Phase::EngineDown);
    }

    #[test]
    fn derive_phase_local_worker_happy_path_is_ready_when_running_and_prepared() {
        let p = derive_phase(DeriveInputs {
            engine_running: true,
            is_binary: false,
            is_local: true,
            running: true,
            prepared: true,
            managed_dir_exists: true,
        });
        assert_eq!(p, Phase::Ready);
    }

    #[test]
    fn derive_phase_local_worker_running_without_marker_is_preparing() {
        // The honest "installing deps inside VM" window: libkrun is up but
        // the in-guest init script hasn't written `.iii-prepared` yet.
        // This only applies to local-path workers — OCI images don't
        // have an install phase.
        let p = derive_phase(DeriveInputs {
            engine_running: true,
            is_binary: false,
            is_local: true,
            running: true,
            prepared: false,
            managed_dir_exists: true,
        });
        assert_eq!(p, Phase::Preparing);
    }

    #[test]
    fn derive_phase_local_worker_prepared_but_dead_is_booting() {
        let p = derive_phase(DeriveInputs {
            engine_running: true,
            is_binary: false,
            is_local: true,
            running: false,
            prepared: true,
            managed_dir_exists: true,
        });
        assert_eq!(p, Phase::Booting);
    }

    #[test]
    fn derive_phase_local_worker_managed_dir_without_marker_is_preparing() {
        let p = derive_phase(DeriveInputs {
            engine_running: true,
            is_binary: false,
            is_local: true,
            running: false,
            prepared: false,
            managed_dir_exists: true,
        });
        assert_eq!(p, Phase::Preparing);
    }

    #[test]
    fn derive_phase_local_worker_fresh_is_queued() {
        let p = derive_phase(DeriveInputs {
            engine_running: true,
            is_binary: false,
            is_local: true,
            running: false,
            prepared: false,
            managed_dir_exists: false,
        });
        assert_eq!(p, Phase::Queued);
    }

    /// OCI regression: `iii worker add docker.io/foo/bar` images bake their
    /// deps at build time — the in-VM install script that writes
    /// `/var/.iii-prepared` never runs for these. Before the fix, the
    /// probe treated a running-but-no-marker OCI worker as Preparing
    /// forever, even after the worker had already connected to the engine
    /// and registered its functions. This is the bug reported for
    /// `todo-worker-python` (OCI image shows "preparing (rootfs cloned,
    /// deps still installing)" with engine logs proving REGISTERED endpoints).
    ///
    /// Fix: a running OCI worker is Ready. The `.iii-prepared` marker
    /// is a local-worker-only concept.
    #[test]
    fn derive_phase_oci_worker_running_is_ready_without_marker() {
        let p = derive_phase(DeriveInputs {
            engine_running: true,
            is_binary: false,
            is_local: false,
            running: true,
            prepared: false,
            managed_dir_exists: true,
        });
        assert_eq!(
            p,
            Phase::Ready,
            "OCI worker with a live VM pid must classify as Ready — \
             deps are baked into the image, no in-VM install step runs."
        );
    }

    #[test]
    fn derive_phase_oci_worker_no_managed_dir_is_queued() {
        let p = derive_phase(DeriveInputs {
            engine_running: true,
            is_binary: false,
            is_local: false,
            running: false,
            prepared: false,
            managed_dir_exists: false,
        });
        assert_eq!(p, Phase::Queued);
    }

    #[test]
    fn derive_phase_oci_worker_managed_dir_but_not_running_is_preparing() {
        // Intermediate state between "queued" and "running": rootfs was
        // extracted, libkrun hasn't come up yet. Brief window during boot.
        let p = derive_phase(DeriveInputs {
            engine_running: true,
            is_binary: false,
            is_local: false,
            running: false,
            prepared: false,
            managed_dir_exists: true,
        });
        assert_eq!(p, Phase::Preparing);
    }

    // ---------------------------------------------------------------------
    // read_pid + pid_file_candidates pure-function tests.
    //
    // These don't need HOME mutation — pid_file_candidates takes the `.iii`
    // root as an argument, so we can exercise every malformed-pidfile path
    // without racing other tests for a process-global env var.
    // ---------------------------------------------------------------------

    #[test]
    fn pid_file_candidates_orders_vm_then_binary() {
        let home = std::path::Path::new("/fake/.iii");
        let got = pid_file_candidates(home, "demo");
        assert_eq!(got[0], home.join("managed/demo/vm.pid"));
        assert_eq!(got[1], home.join("pids/demo.pid"));
    }

    /// read_pid must silently skip malformed pidfile contents. Any panic or
    /// wrong-typed parse (e.g. i32 instead of u32) would bubble through to
    /// the status renderer and either blow up `iii worker status` or mis-
    /// display a pid. Cases drawn from malformed-file bugs we've hit in the
    /// wild: trailing whitespace, empty files, race with a writer that
    /// flushed an empty buffer, accidental negative numbers.
    #[test]
    fn read_pid_silently_skips_malformed_pidfiles() {
        // Must serialize under test_support::lock_home because HOME is process-global
        // and every probe test mutates it. Without the guard, parallel
        // probe_* tests win the race and read_pid sees the wrong HOME.
        let _g = lock_home();
        let tmp = tempfile::tempdir().unwrap();
        let _env = ProbeEnvGuard::new(tmp.path());

        let pids = tmp.path().join(".iii/pids");
        std::fs::create_dir_all(&pids).unwrap();

        for (name, contents) in [
            ("empty", ""),
            ("garbage", "not-a-pid"),
            ("negative", "-1"),
            ("overflow", "9999999999999"),
            ("whitespace-only", "   \n"),
        ] {
            std::fs::write(pids.join(format!("{}.pid", name)), contents).unwrap();
            assert_eq!(
                read_pid(name),
                None,
                "malformed pidfile content {:?} must yield None, not panic or mis-parse",
                contents,
            );
        }

        // Well-formed pidfile with surrounding whitespace still parses.
        std::fs::write(pids.join("padded.pid"), "  12345  \n").unwrap();
        assert_eq!(read_pid("padded"), Some(12345));
    }

    /// watch_until_ready must honor its timeout even when the worker never
    /// reaches a terminal phase. A regression that ignores the timeout arg
    /// would deadlock `iii worker add --wait` and any caller of
    /// wait_for_ready. We build a scenario where probe repeatedly reports
    /// a non-terminal phase (no config.yaml + empty HOME → NotInConfig is
    /// terminal, so we stage a config.yaml entry but keep engine "down" via
    /// a missing socket → Phase::EngineDown, non-terminal).
    #[tokio::test(flavor = "multi_thread")]
    async fn watch_until_ready_honors_timeout() {
        // Shares test_support::lock_home with every other test that mutates
        // HOME+CWD. Using a separate lock would let the tempdirs overlap
        // and corrupt each other's filesystem view.
        let _g = lock_home();
        let tmp = tempfile::tempdir().unwrap();
        let _env = ProbeEnvGuard::new(tmp.path());

        // Minimal config.yaml with one VM worker so probe() does not early-
        // return NotInConfig (terminal). We leave the engine socket closed
        // so probe classifies as EngineDown (non-terminal for VM workers).
        std::fs::write(
            tmp.path().join("config.yaml"),
            "workers:\n  - name: pending-vm\n",
        )
        .unwrap();

        let start = std::time::Instant::now();
        // Port 1 is privileged and never bound by an engine, so
        // is_engine_running_on returns false → Phase::EngineDown (non-terminal)
        // regardless of what's actually running on the host. Pinning this
        // makes the test self-contained — it doesn't flake if a real engine
        // happens to be bound on DEFAULT_PORT when the test runs.
        let status = watch_until_ready("pending-vm", Some(Duration::from_millis(200)), 1).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(3),
            "watch_until_ready must return within the timeout window, elapsed = {:?}",
            elapsed
        );
        assert!(
            !status.is_terminal_for_wait(),
            "timeout path must return a non-terminal status (got {:?}); \
             otherwise the function returned for the wrong reason",
            status.phase
        );
    }

    // ---------------------------------------------------------------------
    // WorkerStatus::probe coverage. Builds a fake HOME + CWD with the
    // filesystem layout probe() reads, then asserts the probe output. This
    // is the gluing layer between disk state and Phase classification —
    // previously exercised only indirectly through integration paths.
    // ---------------------------------------------------------------------

    /// Shared guard for tests that mutate HOME + CWD. Delegates to
    /// the crate-wide `test_support::lock_home` so tests across modules
    /// (supervisor_ctl, etc.) that read HOME via `dirs::home_dir()`
    /// don't race against HOME mutations here and pick up a tempdir
    /// path that breaches SUN_LEN. Using the helper (rather than
    /// open-coding `.lock().unwrap_or_else(…)` at each call site)
    /// centralizes the poison-recovery contract in one place and keeps
    /// the pattern discoverable for future tests.
    use super::super::test_support::lock_home;

    struct ProbeEnvGuard {
        home: Option<std::ffi::OsString>,
        cwd: Option<std::path::PathBuf>,
    }

    impl ProbeEnvGuard {
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

    impl Drop for ProbeEnvGuard {
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

    #[test]
    fn probe_returns_not_in_config_when_worker_missing() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().unwrap();
        let _env = ProbeEnvGuard::new(tmp.path());
        // No config.yaml written — worker_exists returns false.
        let s = WorkerStatus::probe("ghost");
        assert_eq!(s.phase, Phase::NotInConfig);
        assert!(!s.managed_dir_exists);
        assert!(s.pid.is_none());
        assert!(!s.alive);
    }

    /// Binary worker with a live pidfile must probe to Ready regardless of
    /// engine port state. This is the end-to-end equivalent of
    /// `derive_phase_binary_alive_ignores_engine_down` — ensures the probe
    /// layer plumbs the binary flag correctly into derive_phase.
    ///
    /// `resolve_worker_type` classifies a worker as `Binary` only when a
    /// file exists at `~/.iii/workers/{name}` (see `check_binary_fallback`
    /// in config_file.rs); the config.yaml entry alone is insufficient.
    #[test]
    fn probe_binary_worker_with_live_pidfile_is_ready() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().unwrap();
        let _env = ProbeEnvGuard::new(tmp.path());

        let name = "bin-w";
        std::fs::write(
            tmp.path().join("config.yaml"),
            format!("workers:\n  - name: {}\n", name),
        )
        .unwrap();

        // Binary worker is detected by presence of the binary file under
        // ~/.iii/workers/{name}. Staging an empty file is enough for the
        // classifier — we don't execute it.
        let workers_dir = tmp.path().join(".iii/workers");
        std::fs::create_dir_all(&workers_dir).unwrap();
        std::fs::write(workers_dir.join(name), b"").unwrap();

        let pids = tmp.path().join(".iii/pids");
        std::fs::create_dir_all(&pids).unwrap();
        // Write OUR pid so kill(pid, 0) succeeds on the alive check.
        std::fs::write(
            pids.join(format!("{}.pid", name)),
            std::process::id().to_string(),
        )
        .unwrap();

        let s = WorkerStatus::probe(name);
        assert_eq!(s.pid, Some(std::process::id()));
        assert!(s.alive, "is_worker_running must see our live pidfile");
        assert_eq!(
            s.worker_type,
            Some("binary"),
            "binary workers must be classified as binary, not config; got {:?}",
            s.worker_type
        );
        // Phase depends on engine liveness which we can't control from a
        // unit test, but regardless of engine state a binary with a live
        // pid MUST be Ready (that's the point of the classifier fix).
        assert_eq!(s.phase, Phase::Ready);
    }
}
