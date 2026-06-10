// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Engine-death cascade behaviors NOT covered by
//! `daemon_orphan_exit_integration.rs` (which pins the basic exit paths:
//! reparent fallback, lifeline EOF, engine-pid handshake, signals).
//!
//! This file pins the layered semantics on top of those paths:
//!
//! - the durable **breadcrumb** in `~/.iii/logs/<daemon>.log` is written on
//!   engine-gone exits (with the right fields) and ONLY on engine-gone exits
//!   — a breadcrumb on every graceful shutdown was a real pre-fix bug;
//! - **precedence**: lifeline EOF is authoritative even while the declared
//!   engine pid is alive, and the pid backstop pins the ENGINE pid over the
//!   spawner pid when both are declared;
//! - the **spawner-pid backstop** covers the one lifeline failure mode (a
//!   leaked write end keeping EOF at bay — the macOS no-`pipe2` race);
//! - garbage `III_ENGINE_PID` degrades to the reparent fallback instead of
//!   crashing or arming a bogus watch;
//! - the session reaper actually KILLS a running worker process (the log-line
//!   test in the sibling file only proves the reaper ran);
//! - `sandbox-daemon` (the second armed daemon) exits on engine death too.
//!
//! Every daemon gets `HOME=<tempdir>` (breadcrumbs and pidfiles are read
//! under `$HOME/.iii`) and `current_dir(<tempdir>)` (worker discovery reads
//! `./config.yaml` relative to the daemon's cwd), so tests can neither see
//! nor touch the developer's real workers.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

/// Sized against the daemon's ENGINE_POLL_INTERVAL (2s): generous for loaded
/// CI runners, far below the suite timeout.
const EXIT_DEADLINE: Duration = Duration::from_secs(15);
const PROBE_INTERVAL: Duration = Duration::from_millis(200);
/// Two full daemon poll intervals plus slack — even on a loaded runner where
/// a tick lands late, at least one armed poll completes while the
/// not-to-be-watched process is already dead, so "survived" means the watch
/// demonstrably ignored that death (a single-interval window can contain
/// zero completed ticks under load, turning the negative assert vacuous).
const ARMED_SURVIVAL_WINDOW: Duration = Duration::from_millis(5000);

/// Kills the child on EVERY exit path including assertion panics —
/// `std::process::Child` does not kill on drop, and a leaked daemon here
/// reconnect-loops forever.
struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Poll `logfile` until it contains `needle` or `deadline` passes. Returns
/// the log contents either way so failures carry diagnostics.
fn wait_for_log_line(logfile: &Path, needle: &str, deadline: Duration) -> (bool, String) {
    let end = Instant::now() + deadline;
    loop {
        let contents = std::fs::read_to_string(logfile).unwrap_or_default();
        if contents.contains(needle) {
            return (true, contents);
        }
        if Instant::now() >= end {
            return (false, contents);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Poll the child until it exits, returning its status; panics with the log
/// contents (and kills the child) on deadline.
fn wait_for_exit(
    child: &mut KillOnDrop,
    deadline: Duration,
    logfile: &Path,
    what: &str,
) -> std::process::ExitStatus {
    let end = Instant::now() + deadline;
    loop {
        if let Some(s) = child.0.try_wait().expect("try_wait") {
            return s;
        }
        if Instant::now() >= end {
            let log = std::fs::read_to_string(logfile).unwrap_or_default();
            let _ = child.0.kill();
            panic!("{what} did not exit within {deadline:?}; log:\n{log}");
        }
        std::thread::sleep(PROBE_INTERVAL);
    }
}

/// A `worker-manager-daemon` Command with the shared isolation/observability
/// wiring: tempdir HOME (breadcrumbs + pidfiles), tempdir cwd (worker
/// discovery), info-level logs into `logfile` (tracing fmt writes to stdout;
/// readiness gating and failure diagnostics need both streams).
fn daemon_cmd(tmp: &Path, logfile: &Path) -> Command {
    let log = std::fs::File::create(logfile).unwrap();
    let log_err = log.try_clone().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_iii-worker"));
    cmd.args(["worker-manager-daemon", "--engine", "ws://127.0.0.1:1"])
        .current_dir(tmp)
        .env("RUST_LOG", "info")
        .env("HOME", tmp)
        .env_remove("III_ENGINE_PID")
        .env_remove("III_LIFELINE_FD")
        .env_remove("III_LIFELINE_SPAWNER_PID")
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err));
    cmd
}

/// The engine-gone breadcrumb the daemon appends under `$HOME/.iii/logs/`.
fn breadcrumb_path(home: &Path, daemon: &str) -> PathBuf {
    home.join(".iii/logs").join(format!("{daemon}.log"))
}

fn spawn_fake_engine() -> std::process::Child {
    Command::new("sleep")
        .arg("300")
        .spawn()
        .expect("spawn fake engine")
}

/// Engine-gone via the PID handshake must leave a durable breadcrumb with
/// the fields a field investigation needs: which daemon, why, which engine
/// pid was watched, and who the spawn parent was. The daemon's stdout/stderr
/// are pipes into the (dead) engine in production, so this file is the ONLY
/// trace of a self-exit.
#[test]
fn engine_gone_breadcrumb_records_engine_pid_and_spawn_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("daemon.log");

    let mut fake_engine = KillOnDrop(spawn_fake_engine());
    let engine_pid = fake_engine.0.id() as i32;

    let mut cmd = daemon_cmd(tmp.path(), &logfile);
    cmd.env("III_ENGINE_PID", engine_pid.to_string());
    let mut daemon = KillOnDrop(cmd.spawn().expect("spawn daemon"));

    let (armed, log) =
        wait_for_log_line(&logfile, "engine exit-watch armed", Duration::from_secs(10));
    assert!(armed, "daemon never armed the engine-pid watch; log:\n{log}");

    let _ = fake_engine.0.kill();
    let _ = fake_engine.0.wait();

    let status = wait_for_exit(&mut daemon, EXIT_DEADLINE, &logfile, "daemon");
    assert_eq!(status.code(), Some(0), "engine-gone exit must be graceful");

    let crumb = std::fs::read_to_string(breadcrumb_path(tmp.path(), "worker-manager-daemon"))
        .expect("engine-gone exit must write a breadcrumb");
    assert!(
        crumb.contains("daemon=worker-manager-daemon"),
        "breadcrumb must name the daemon: {crumb}"
    );
    assert!(
        crumb.contains("reason=engine-gone"),
        "breadcrumb must carry the reason: {crumb}"
    );
    // Trailing delimiters pin the exact field values (engine_pid is always
    // followed by " spawn_parent=", which terminates the line) — a bare
    // substring would also match a longer pid sharing the same prefix.
    assert!(
        crumb.contains(&format!("engine_pid={engine_pid} ")),
        "breadcrumb must record the watched engine pid: {crumb}"
    );
    // The daemon is our direct child here, so spawn parent == this test
    // process. The spawn-time-vs-current distinction is pinned by
    // garbage_engine_pid_falls_back_to_reparent_watch, whose spawn parent
    // (the sh trampoline) is dead by breadcrumb time.
    assert!(
        crumb.contains(&format!("spawn_parent={}\n", std::process::id())),
        "breadcrumb must record the spawn-time parent: {crumb}"
    );
}

/// A graceful SIGNAL exit must NOT write the engine-gone breadcrumb — a
/// pre-fix bug stamped a false "engine-gone" line on 17 of 20 ordinary
/// shutdowns, which would poison any future incident investigation that
/// trusts the file.
#[test]
fn signal_exit_writes_no_breadcrumb() {
    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("daemon.log");

    let mut daemon = KillOnDrop(daemon_cmd(tmp.path(), &logfile).spawn().expect("spawn daemon"));
    let pid = daemon.0.id() as i32;

    let (armed, log) =
        wait_for_log_line(&logfile, "parent exit-watch armed", Duration::from_secs(10));
    assert!(armed, "daemon never armed its exit watch; log:\n{log}");

    kill(Pid::from_raw(pid), Signal::SIGTERM).expect("send SIGTERM");
    let status = wait_for_exit(&mut daemon, EXIT_DEADLINE, &logfile, "daemon");
    assert_eq!(status.code(), Some(0), "SIGTERM exit must be graceful");

    // Nothing else writes this file under the isolated HOME, so its very
    // existence would mean a breadcrumb was stamped on a signal shutdown.
    let crumb_file = breadcrumb_path(tmp.path(), "worker-manager-daemon");
    assert!(
        !crumb_file.exists(),
        "signal shutdown must not write any breadcrumb: {}",
        std::fs::read_to_string(&crumb_file).unwrap_or_default()
    );
}

/// Lifeline EOF is AUTHORITATIVE: the daemon must exit on it even while the
/// declared engine pid is demonstrably alive. In production the engine holds
/// the write end, so EOF ⇔ engine gone with zero latency; a regression that
/// demotes the lifeline behind the pid poll (or worse, requires both) would
/// pass the sibling file's tests and fail only here.
#[test]
fn lifeline_eof_exits_daemon_even_while_declared_engine_alive() {
    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("daemon.log");

    let fake_engine = KillOnDrop(spawn_fake_engine());
    let engine_pid = fake_engine.0.id() as i32;

    let mut cmd = daemon_cmd(tmp.path(), &logfile);
    cmd.env("III_ENGINE_PID", engine_pid.to_string());
    let lifeline =
        iii_worker::daemon_exit::attach_lifeline_std(&mut cmd).expect("attach lifeline");
    let mut daemon = KillOnDrop(cmd.spawn().expect("spawn daemon"));

    let (armed, log) =
        wait_for_log_line(&logfile, "lifeline exit-watch armed", Duration::from_secs(10));
    assert!(armed, "daemon never armed the lifeline watch; log:\n{log}");

    // Lifeline held + engine alive → must keep running across at least one
    // completed pid poll (a watch that fires on its first tick regardless of
    // liveness must fail HERE, not be mistaken for the EOF exit below).
    std::thread::sleep(ARMED_SURVIVAL_WINDOW);
    assert!(
        daemon.0.try_wait().expect("try_wait").is_none(),
        "daemon exited while both the lifeline and the engine were alive"
    );

    drop(lifeline); // EOF — while the engine pid still exists

    let status = wait_for_exit(&mut daemon, Duration::from_secs(5), &logfile, "daemon");
    assert_eq!(status.code(), Some(0), "lifeline exit must be graceful");

    // It is an engine-gone exit (the spawner IS the engine in production),
    // so the breadcrumb must be written, recording the still-alive declared
    // pid — that mismatch is exactly the forensic detail worth keeping.
    let crumb = std::fs::read_to_string(breadcrumb_path(tmp.path(), "worker-manager-daemon"))
        .expect("lifeline engine-gone exit must write a breadcrumb");
    assert!(
        crumb.contains(&format!("engine_pid={engine_pid} ")),
        "breadcrumb must record the declared engine pid: {crumb}"
    );
}

/// The one lifeline failure mode: on macOS `pipe()` + fcntl is not atomic,
/// so a write end can leak into an unrelated child and keep EOF from ever
/// firing. The spawner-pid backstop must cover it — we simulate the leak by
/// HOLDING the write end open in this test while the declared spawner dies.
#[test]
fn spawner_pid_backstop_covers_leaked_lifeline_write_end() {
    use std::os::fd::AsRawFd;

    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("daemon.log");

    let mut fake_spawner = KillOnDrop(spawn_fake_engine());
    let spawner_pid = fake_spawner.0.id() as i32;

    // Hand-rolled lifeline (attach_lifeline_std would pin the spawner pid to
    // this test process, which we cannot kill): a plain pipe is non-CLOEXEC,
    // so the child inherits the read end at the same fd number. Both ends
    // stay open in this test for the whole run — the held write end IS the
    // simulated leak.
    let (read_end, _write_end_leak) = nix::unistd::pipe().expect("pipe");

    let mut cmd = daemon_cmd(tmp.path(), &logfile);
    cmd.env("III_LIFELINE_FD", read_end.as_raw_fd().to_string())
        .env("III_LIFELINE_SPAWNER_PID", spawner_pid.to_string());
    let mut daemon = KillOnDrop(cmd.spawn().expect("spawn daemon"));

    // Both watches must be armed before we kill anything: the lifeline (which
    // will never fire here) and the pid backstop (which must).
    let (armed, log) =
        wait_for_log_line(&logfile, "lifeline exit-watch armed", Duration::from_secs(10));
    assert!(armed, "daemon never armed the lifeline watch; log:\n{log}");
    let (armed, log) =
        wait_for_log_line(&logfile, "engine exit-watch armed", Duration::from_secs(10));
    assert!(armed, "daemon never armed the pid backstop; log:\n{log}");

    // Spawner alive + lifeline open → must keep running across at least one
    // completed pid poll, so the exit below is attributable to the spawner's
    // death and not to a first-tick false fire.
    std::thread::sleep(ARMED_SURVIVAL_WINDOW);
    assert!(
        daemon.0.try_wait().expect("try_wait").is_none(),
        "daemon exited while its spawner was alive"
    );

    let _ = fake_spawner.0.kill();
    let _ = fake_spawner.0.wait();

    // The lifeline never reaches EOF (we hold the write end), so only the
    // pid backstop can produce this exit.
    let status = wait_for_exit(&mut daemon, EXIT_DEADLINE, &logfile, "daemon");
    assert_eq!(status.code(), Some(0), "backstop exit must be graceful");

    let crumb = std::fs::read_to_string(breadcrumb_path(tmp.path(), "worker-manager-daemon"))
        .expect("backstop engine-gone exit must write a breadcrumb");
    assert!(
        crumb.contains("engine_pid=none "),
        "no engine pid was declared, the breadcrumb must say so: {crumb}"
    );
}

/// When BOTH pids are declared, the backstop must pin the ENGINE pid: the
/// spawner dying while the engine lives is a wrapper/shim exiting, not
/// engine death. A regression flipping `engine_pid.or(spawner_pid)` to
/// spawner-first passes every other test and fails only here.
#[test]
fn engine_pid_outranks_spawner_pid_in_backstop() {
    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("daemon.log");

    let mut fake_engine = KillOnDrop(spawn_fake_engine());
    let mut fake_spawner = KillOnDrop(spawn_fake_engine());

    let mut cmd = daemon_cmd(tmp.path(), &logfile);
    cmd.env("III_ENGINE_PID", fake_engine.0.id().to_string())
        .env("III_LIFELINE_SPAWNER_PID", fake_spawner.0.id().to_string());
    let mut daemon = KillOnDrop(cmd.spawn().expect("spawn daemon"));

    let (armed, log) =
        wait_for_log_line(&logfile, "engine exit-watch armed", Duration::from_secs(10));
    assert!(armed, "daemon never armed the pid watch; log:\n{log}");

    // Kill the spawner; the engine lives. The daemon must survive at least
    // one full poll interval — an exit here means the watch picked the
    // wrong pid.
    let _ = fake_spawner.0.kill();
    let _ = fake_spawner.0.wait();
    std::thread::sleep(ARMED_SURVIVAL_WINDOW);
    assert!(
        daemon.0.try_wait().expect("try_wait").is_none(),
        "daemon exited on SPAWNER death while the declared engine was alive"
    );

    // Now the engine dies → the daemon must go.
    let _ = fake_engine.0.kill();
    let _ = fake_engine.0.wait();
    let status = wait_for_exit(&mut daemon, EXIT_DEADLINE, &logfile, "daemon");
    assert_eq!(status.code(), Some(0), "engine-gone exit must be graceful");
}

/// A garbage III_ENGINE_PID (version skew, a broken wrapper exporting junk)
/// must not crash the daemon OR arm a bogus engine watch — it degrades to
/// the reparent fallback, which still detects orphaning.
#[test]
fn garbage_engine_pid_falls_back_to_reparent_watch() {
    let bin = env!("CARGO_BIN_EXE_iii-worker");
    let tmp = tempfile::tempdir().unwrap();
    let pidfile = tmp.path().join("daemon.pid");
    let logfile = tmp.path().join("daemon.log");

    // Intermediate `sh` so the daemon has a killable parent (same trampoline
    // as the sibling file's orphan test; paths travel via env, not
    // interpolation). The cwd is set on `sh` itself and inherited — a
    // `cd && daemon &` compound would background a SUBSHELL, making `$!` the
    // subshell pid and the subshell the daemon's parent.
    let sh = Command::new("sh")
        .arg("-c")
        .arg(r#""$DAEMON_BIN" worker-manager-daemon --engine ws://127.0.0.1:1 >>"$DAEMON_LOG" 2>&1 & echo $! > "$DAEMON_PIDFILE"; wait"#)
        .current_dir(tmp.path())
        .env("DAEMON_BIN", bin)
        .env("DAEMON_PIDFILE", &pidfile)
        .env("DAEMON_LOG", &logfile)
        .env("RUST_LOG", "info")
        .env("HOME", tmp.path())
        .env("III_ENGINE_PID", "not-a-pid")
        .spawn()
        .expect("spawn intermediate sh");
    let mut sh = KillOnDrop(sh);

    let daemon_pid = {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(s) = std::fs::read_to_string(&pidfile)
                && let Ok(pid) = s.trim().parse::<i32>()
                && pid > 1
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "daemon never wrote its pid");
            std::thread::sleep(Duration::from_millis(50));
        }
    };

    // The garbage pid must be REJECTED (reparent watch armed) — not parsed
    // into an engine watch on a nonsense pid.
    let (armed, log) =
        wait_for_log_line(&logfile, "parent exit-watch armed", Duration::from_secs(10));
    if !armed {
        let _ = kill(Pid::from_raw(daemon_pid), Signal::SIGKILL);
        panic!("daemon with garbage engine pid never armed the fallback; log:\n{log}");
    }
    assert!(
        !log.contains("engine exit-watch armed"),
        "garbage III_ENGINE_PID must not arm an engine watch; log:\n{log}"
    );

    // And the fallback must still WORK: orphan the daemon, it self-exits.
    let sh_pid = sh.0.id();
    let _ = sh.0.kill();
    let _ = sh.0.wait();

    let deadline = Instant::now() + EXIT_DEADLINE;
    loop {
        if kill(Pid::from_raw(daemon_pid), None).is_err() {
            break; // gone
        }
        if Instant::now() >= deadline {
            let final_log = std::fs::read_to_string(&logfile).unwrap_or_default();
            let _ = kill(Pid::from_raw(daemon_pid), Signal::SIGKILL);
            panic!("orphaned daemon (garbage engine pid) never self-exited; log:\n{final_log}");
        }
        std::thread::sleep(PROBE_INTERVAL);
    }

    // The breadcrumb's spawn_parent must be the SPAWN-time parent (the sh
    // trampoline, dead by now) — the only topology in this suite where
    // spawn-time and current parent actually differ, so this is what pins
    // "snapshot at startup" over "whatever getppid() says at exit".
    let crumb = std::fs::read_to_string(breadcrumb_path(tmp.path(), "worker-manager-daemon"))
        .expect("reparent engine-gone exit must write a breadcrumb");
    assert!(
        crumb.contains("engine_pid=none ") && crumb.contains(&format!("spawn_parent={sh_pid}\n")),
        "breadcrumb must record no engine pid and the dead spawn-time parent {sh_pid}: {crumb}"
    );
}

/// The session reaper must actually KILL a running worker process — the
/// sibling file only asserts the reaper's log line on an empty project.
/// Binary-tier worker anatomy (all under the isolated $HOME/cwd):
/// `./config.yaml` lists the name, `$HOME/.iii/workers/<name>` marks it a
/// binary worker, `$HOME/.iii/pids/<name>.pid` points at the live process.
#[test]
fn reaper_kills_running_binary_worker_on_engine_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("daemon.log");

    // The "worker": a sleeper this test owns, registered exactly the way the
    // managed-stop machinery discovers binary workers.
    let mut worker = KillOnDrop(spawn_fake_engine());
    let worker_pid = worker.0.id();
    std::fs::write(
        tmp.path().join("config.yaml"),
        "workers:\n  - name: zwreap\n",
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join(".iii/workers")).unwrap();
    std::fs::write(tmp.path().join(".iii/workers/zwreap"), "").unwrap();
    std::fs::create_dir_all(tmp.path().join(".iii/pids")).unwrap();
    let worker_pidfile = tmp.path().join(".iii/pids/zwreap.pid");
    std::fs::write(&worker_pidfile, worker_pid.to_string()).unwrap();

    let mut fake_engine = KillOnDrop(spawn_fake_engine());

    let mut cmd = daemon_cmd(tmp.path(), &logfile);
    cmd.env("III_ENGINE_PID", fake_engine.0.id().to_string());
    let mut daemon = KillOnDrop(cmd.spawn().expect("spawn daemon"));

    let (armed, log) =
        wait_for_log_line(&logfile, "engine exit-watch armed", Duration::from_secs(10));
    assert!(armed, "daemon never armed; log:\n{log}");

    let _ = fake_engine.0.kill();
    let _ = fake_engine.0.wait();

    // The worker must die at the daemon's hand (SIGTERM→SIGKILL): observe it
    // via try_wait on OUR child handle — kill(pid, 0) would still see the
    // zombie until we reap it here.
    let deadline = Instant::now() + EXIT_DEADLINE;
    loop {
        if worker.0.try_wait().expect("try_wait").is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let final_log = std::fs::read_to_string(&logfile).unwrap_or_default();
            panic!("reaper never killed the running worker; log:\n{final_log}");
        }
        std::thread::sleep(PROBE_INTERVAL);
    }

    let status = wait_for_exit(&mut daemon, EXIT_DEADLINE, &logfile, "daemon");
    assert_eq!(status.code(), Some(0), "daemon exit after reap must be graceful");

    let final_log = std::fs::read_to_string(&logfile).unwrap_or_default();
    assert!(
        final_log.contains("reaping managed workers"),
        "reaper pass must be observable; log:\n{final_log}"
    );
    assert!(
        !worker_pidfile.exists(),
        "stop path must clean up the worker pidfile; log:\n{final_log}"
    );
}

/// `sandbox-daemon` is the OTHER engine-spawned daemon (and the worse leak:
/// an orphan holds live multi-GB libkrun VMs). It shares ExitWatch with the
/// worker-manager daemon; this pins that `serve()` actually arms it and
/// honors engine death, with its own breadcrumb.
#[test]
fn sandbox_daemon_exits_when_engine_dies() {
    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("sandbox-daemon.out");
    let config = tmp.path().join("sandbox-config.yaml");
    // Minimal valid config; an empty allowlist denies creates, which is fine
    // — this test never creates a sandbox.
    std::fs::write(&config, "image_allowlist: []\n").unwrap();

    let mut fake_engine = KillOnDrop(spawn_fake_engine());
    let engine_pid = fake_engine.0.id() as i32;

    let log = std::fs::File::create(&logfile).unwrap();
    let log_err = log.try_clone().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_iii-worker"));
    cmd.arg("sandbox-daemon")
        .arg("--config")
        .arg(&config)
        .args(["--engine", "ws://127.0.0.1:1"])
        .current_dir(tmp.path())
        .env("RUST_LOG", "info")
        .env("HOME", tmp.path())
        .env("III_ENGINE_PID", engine_pid.to_string())
        .env_remove("III_LIFELINE_FD")
        .env_remove("III_LIFELINE_SPAWNER_PID")
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err));
    let mut daemon = KillOnDrop(cmd.spawn().expect("spawn sandbox-daemon"));

    let (armed, log) =
        wait_for_log_line(&logfile, "engine exit-watch armed", Duration::from_secs(10));
    assert!(armed, "sandbox-daemon never armed the engine watch; log:\n{log}");

    // Engine alive → daemon stays up across at least one completed pid poll
    // (attributes the exit below to engine death, not a first-tick misfire).
    std::thread::sleep(ARMED_SURVIVAL_WINDOW);
    assert!(
        daemon.0.try_wait().expect("try_wait").is_none(),
        "sandbox-daemon exited while its engine was alive"
    );

    let _ = fake_engine.0.kill();
    let _ = fake_engine.0.wait();

    let status = wait_for_exit(&mut daemon, EXIT_DEADLINE, &logfile, "sandbox-daemon");
    assert_eq!(status.code(), Some(0), "engine-gone exit must be graceful");

    let crumb = std::fs::read_to_string(breadcrumb_path(tmp.path(), "sandbox-daemon"))
        .expect("sandbox-daemon engine-gone exit must write a breadcrumb");
    assert!(
        crumb.contains("daemon=sandbox-daemon") && crumb.contains("reason=engine-gone"),
        "breadcrumb must identify the sandbox daemon and reason: {crumb}"
    );
}
