// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
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
use super::managed::{is_engine_running, is_worker_running};

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
    pub fn probe(name: &str) -> Self {
        let engine_running = is_engine_running();
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

        // Ready requires BOTH a live VM process AND the `.iii-prepared`
        // marker. The VM pid goes live the moment libkrun boots, but the
        // in-guest init script then runs setup/install (which can take
        // minutes on first boot with a cold dep cache). The marker is only
        // written at the end of that script, so it's the honest signal that
        // the worker is actually ready to handle events — not just that the
        // VM kernel is up.
        let phase = if !engine_running {
            Phase::EngineDown
        } else if is_binary {
            // Binary workers are plain host processes — no VM, no rootfs, no
            // `.iii-prepared` marker. A live pid IS ready; a dead one means
            // the process exited (or never started).
            if running { Phase::Ready } else { Phase::Failed }
        } else if running && prepared {
            Phase::Ready
        } else if running {
            // VM is up, deps still installing. Surface as Preparing — the
            // sandbox line already explains the detail.
            Phase::Preparing
        } else if prepared {
            // Deps installed previously, VM restarting (e.g. after --force
            // we wiped the managed dir, but this branch covers the "second
            // boot on a warm cache" window).
            Phase::Booting
        } else if managed_dir_exists {
            Phase::Preparing
        } else {
            Phase::Queued
        };

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
            Phase::EngineDown => format!(
                "{} engine not running (start it with `iii start`)",
                "⚠".yellow()
            ),
            Phase::Queued => format!(
                "{} {} queued — engine will boot its sandbox shortly",
                "⟳".cyan(),
                self.name.bold()
            ),
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
                "(run `iii start` in another terminal)".dimmed()
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

fn read_pid(name: &str) -> Option<u32> {
    let home = dirs::home_dir().unwrap_or_default();
    let candidates = [
        home.join(".iii/managed").join(name).join("vm.pid"),
        home.join(".iii/pids").join(format!("{}.pid", name)),
    ];
    for path in candidates {
        if let Ok(s) = std::fs::read_to_string(&path)
            && let Ok(pid) = s.trim().parse::<u32>()
        {
            return Some(pid);
        }
    }
    None
}

/// Entry point for `iii worker status`.
pub async fn handle_worker_status(worker_name: &str, watch: bool) -> i32 {
    if !watch {
        let status = WorkerStatus::probe(worker_name);
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
    let final_status = watch_until_ready(worker_name, None).await;
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
/// used by `--wait` so the CLI doesn't hang on a stuck VM.
///
/// Returns the final status so callers can render a closing message.
pub async fn watch_until_ready(worker_name: &str, timeout: Option<Duration>) -> WorkerStatus {
    let started = Instant::now();
    let mut first = true;
    loop {
        let status = WorkerStatus::probe(worker_name);
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
        assert!(s.headline().contains("iii start"));
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
}
