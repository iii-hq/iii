// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Regression tests for the orphaned `worker-manager-daemon` leak and its
//! graceful-exit paths.
//!
//! The engine spawns the daemon in its own session (setsid), so an engine that
//! exits abnormally (SIGKILL, OOM, crash, dev hard-restart) used to leave the
//! daemon with no signal at all — it reconnect-looped forever as an orphan
//! (PPID 1), and every engine restart leaked another. The fix makes the daemon
//! watch for losing the parent that spawned it and exit on its own, and adds
//! SIGTERM/SIGHUP graceful exits (the engine's kill_child sends SIGTERM, which
//! previously killed the daemon via default disposition).
//!
//! `daemon_exits_when_orphaned_from_engine` reproduces the leak with a real
//! daemon process: an intermediate `sh` becomes the daemon's parent, then we
//! kill `sh` to orphan the daemon and assert it exits on its own. Without the
//! fix it runs forever and this times out. Readiness is gated on the daemon's
//! own "parent exit-watch armed" log line (not a blind sleep), and the daemon
//! must demonstrably SURVIVE at least one armed poll while its parent is alive
//! before we orphan it — so a false-positive exit reads as a failure instead
//! of being mistaken for the expected one.

#![cfg(unix)]

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

/// Sized against the daemon's REPARENT_POLL_INTERVAL (2s): generous for
/// loaded CI runners, far below the suite timeout.
const EXIT_DEADLINE: Duration = Duration::from_secs(15);
const PROBE_INTERVAL: Duration = Duration::from_millis(200);
/// One full daemon poll interval plus slack — long enough that at least one
/// armed `getppid()` poll completes with the parent alive.
const ARMED_SURVIVAL_WINDOW: Duration = Duration::from_millis(2500);

/// Liveness probe via `kill(pid, 0)` — no subprocess per poll.
fn pid_alive(pid: i32) -> bool {
    kill(Pid::from_raw(pid), None).is_ok()
}

/// True when `pid` currently belongs to an iii-worker process. Guards the
/// failure-path SIGKILL (and final diagnostics) against PID reuse hitting an
/// unrelated same-user process.
fn pid_is_iii_worker(pid: i32) -> bool {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("iii-worker"))
        .unwrap_or(false)
}

/// Kills the intermediate `sh` and (identity-checked) the daemon on EVERY
/// exit path, including assertion panics — `std::process::Child` does not
/// kill on drop, and a leaked daemon here reconnect-loops forever.
struct ProcGuard {
    sh: std::process::Child,
    daemon_pid: Option<i32>,
}

impl Drop for ProcGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.daemon_pid
            && pid > 1
            && pid_is_iii_worker(pid)
        {
            let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
        }
        let _ = self.sh.kill();
        let _ = self.sh.wait();
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

#[test]
fn daemon_exits_when_orphaned_from_engine() {
    let bin = env!("CARGO_BIN_EXE_iii-worker");
    let tmp = tempfile::tempdir().unwrap();
    let pidfile = tmp.path().join("daemon.pid");
    let logfile = tmp.path().join("daemon.log");

    // Intermediate `sh` backgrounds the daemon (so the daemon's parent is sh,
    // not the test runner) and then `wait`s, staying alive until we kill it.
    // Paths travel via the environment, not string interpolation — no shell
    // quoting surface, and paths containing quotes or spaces still work. The
    // engine URL points at a dead port so the daemon just reconnect-loops.
    // RUST_LOG=info makes the daemon's readiness/armed lines observable in
    // $DAEMON_LOG (the default filter is warn).
    // env_remove: this test exercises the REPARENT FALLBACK, which only arms
    // when no engine pid was declared (a real engine sets III_ENGINE_PID).
    let sh = Command::new("sh")
        .arg("-c")
        .arg(r#""$DAEMON_BIN" worker-manager-daemon --engine ws://127.0.0.1:1 >>"$DAEMON_LOG" 2>&1 & echo $! > "$DAEMON_PIDFILE"; wait"#)
        .env("DAEMON_BIN", bin)
        .env("DAEMON_PIDFILE", &pidfile)
        .env("DAEMON_LOG", &logfile)
        .env("RUST_LOG", "info")
        .env("HOME", tmp.path())
        .env_remove("III_ENGINE_PID")
        .spawn()
        .expect("spawn intermediate sh");
    let mut guard = ProcGuard {
        sh,
        daemon_pid: None,
    };

    // Read the daemon's pid once sh has launched it. Reject pid <= 1 so a
    // corrupt pidfile can never aim the probes (or the guard's SIGKILL) at
    // init or the whole session.
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
    guard.daemon_pid = Some(daemon_pid);

    // Deterministic readiness: wait for the daemon's own log line proving it
    // snapshotted its parent and armed the watch WHILE sh was still alive —
    // no blind startup sleep, no arming race under CI load.
    let (armed, log) = wait_for_log_line(&logfile, "parent exit-watch armed", Duration::from_secs(10));
    assert!(armed, "daemon never armed its exit watch; log:\n{log}");

    // Negative path: with its parent alive, the daemon must SURVIVE at least
    // one full armed poll interval — a watch that false-fires on the first
    // poll must fail here, not be mistaken for the expected exit below.
    std::thread::sleep(ARMED_SURVIVAL_WINDOW);
    assert!(
        pid_alive(daemon_pid),
        "daemon exited while its parent was still alive (false-positive watch)"
    );

    // Orphan it: SIGKILL only the intermediate sh. The daemon is a separate
    // child, so it survives and is reparented to init/launchd — exactly the
    // production scenario where the engine dies without killing the daemon.
    let _ = guard.sh.kill();
    let _ = guard.sh.wait();

    // The fix must make the daemon notice the lost parent and exit on its own.
    let deadline = Instant::now() + EXIT_DEADLINE;
    let exited = loop {
        if !pid_alive(daemon_pid) {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(PROBE_INTERVAL);
    };

    let final_log = std::fs::read_to_string(&logfile).unwrap_or_default();
    assert!(
        exited,
        "orphaned daemon did not self-exit within {EXIT_DEADLINE:?} — the leak regressed; log:\n{final_log}"
    );
    // ProcGuard::drop still runs (identity-checked no-op for the dead daemon).
}

/// Lifeline path: with a lifeline pipe attached (what the engine actually
/// does at spawn), dropping the spawner-side handle must make the daemon
/// exit gracefully — the EOF is instant, no poll interval involved. The
/// daemon's direct parent (this test) stays alive throughout, so neither
/// the reparent fallback nor a signal can be the cause.
#[test]
fn daemon_exits_when_lifeline_drops() {
    let bin = env!("CARGO_BIN_EXE_iii-worker");
    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("daemon.log");

    let log = std::fs::File::create(&logfile).unwrap();
    let log_err = log.try_clone().unwrap();
    let mut cmd = Command::new(bin);
    cmd.args(["worker-manager-daemon", "--engine", "ws://127.0.0.1:1"])
        .env("RUST_LOG", "info")
        .env("HOME", tmp.path())
        .env_remove("III_ENGINE_PID")
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err));
    let lifeline =
        iii_worker::daemon_exit::attach_lifeline_std(&mut cmd).expect("attach lifeline");
    let mut daemon = cmd.spawn().expect("spawn daemon");

    let (armed, log) =
        wait_for_log_line(&logfile, "lifeline exit-watch armed", Duration::from_secs(10));
    if !armed {
        let _ = daemon.kill();
        panic!("daemon never armed the lifeline watch; log:\n{log}");
    }

    // Held lifeline → daemon must keep running.
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        daemon.try_wait().expect("try_wait").is_none(),
        "daemon exited while the lifeline was held"
    );

    // Drop the spawner-side handle: the daemon's read sees EOF instantly.
    drop(lifeline);

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(s) = daemon.try_wait().expect("try_wait") {
            break s;
        }
        if Instant::now() >= deadline {
            let _ = daemon.kill();
            let final_log = std::fs::read_to_string(&logfile).unwrap_or_default();
            panic!("daemon ignored lifeline EOF; log:\n{final_log}");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(0), "lifeline exit must be graceful");
}

/// PID-handshake path: with III_ENGINE_PID declared, the daemon must exit
/// when THAT pid dies — even though its direct parent (this test) stays
/// alive. This is the production engine contract (engine/src/workers/
/// external.rs exports the env), and it is what makes the watch immune to
/// wrappers/shims sitting between the engine and the daemon.
#[test]
fn daemon_exits_when_declared_engine_pid_dies() {
    let bin = env!("CARGO_BIN_EXE_iii-worker");
    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("daemon.log");

    // Fake engine: a sleeper we control. Killed below to simulate engine death.
    let mut fake_engine = Command::new("sleep")
        .arg("300")
        .spawn()
        .expect("spawn fake engine");
    let engine_pid = fake_engine.id() as i32;

    let log = std::fs::File::create(&logfile).unwrap();
    let log_err = log.try_clone().unwrap();
    let mut daemon = Command::new(bin)
        .args(["worker-manager-daemon", "--engine", "ws://127.0.0.1:1"])
        .env("RUST_LOG", "info")
        .env("HOME", tmp.path())
        .env("III_ENGINE_PID", engine_pid.to_string())
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err))
        .spawn()
        .expect("spawn daemon");

    let (armed, log) =
        wait_for_log_line(&logfile, "engine exit-watch armed", Duration::from_secs(10));
    if !armed {
        let _ = daemon.kill();
        let _ = fake_engine.kill();
        panic!("daemon never armed the engine-pid watch; log:\n{log}");
    }

    // Kill the declared engine. The daemon's parent (this test) stays alive,
    // so only the handshake can trigger the exit.
    let _ = fake_engine.kill();
    let _ = fake_engine.wait();

    let deadline = Instant::now() + EXIT_DEADLINE;
    let status = loop {
        if let Some(s) = daemon.try_wait().expect("try_wait") {
            break s;
        }
        if Instant::now() >= deadline {
            let _ = daemon.kill();
            let final_log = std::fs::read_to_string(&logfile).unwrap_or_default();
            panic!("daemon ignored declared engine death; log:\n{final_log}");
        }
        std::thread::sleep(PROBE_INTERVAL);
    };
    assert_eq!(status.code(), Some(0), "engine-gone exit must be graceful");
}

/// Handshake precedence: a daemon whose declared engine is ALIVE must
/// survive losing its direct parent — a wrapper/shim exiting is not engine
/// death. (The reparent fallback would have exited here; the handshake must
/// win.)
#[test]
fn daemon_survives_orphaning_when_declared_engine_alive() {
    let bin = env!("CARGO_BIN_EXE_iii-worker");
    let tmp = tempfile::tempdir().unwrap();
    let pidfile = tmp.path().join("daemon.pid");
    let logfile = tmp.path().join("daemon.log");

    // Declared engine = this test process (alive throughout).
    let sh = Command::new("sh")
        .arg("-c")
        .arg(r#""$DAEMON_BIN" worker-manager-daemon --engine ws://127.0.0.1:1 >>"$DAEMON_LOG" 2>&1 & echo $! > "$DAEMON_PIDFILE"; wait"#)
        .env("DAEMON_BIN", bin)
        .env("DAEMON_PIDFILE", &pidfile)
        .env("DAEMON_LOG", &logfile)
        .env("RUST_LOG", "info")
        .env("HOME", tmp.path())
        .env("III_ENGINE_PID", std::process::id().to_string())
        .spawn()
        .expect("spawn intermediate sh");
    let mut guard = ProcGuard {
        sh,
        daemon_pid: None,
    };

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
    guard.daemon_pid = Some(daemon_pid);

    let (armed, log) =
        wait_for_log_line(&logfile, "engine exit-watch armed", Duration::from_secs(10));
    assert!(armed, "daemon never armed the engine-pid watch; log:\n{log}");

    // Orphan the daemon (kill its direct parent). The declared engine (us)
    // is alive, so the daemon must keep running through multiple polls.
    let _ = guard.sh.kill();
    let _ = guard.sh.wait();

    std::thread::sleep(ARMED_SURVIVAL_WINDOW);
    assert!(
        pid_alive(daemon_pid),
        "daemon exited on parent loss despite its declared engine being alive"
    );
    // ProcGuard::drop kills the (intentionally surviving) daemon.
}

/// The engine's kill_child sends SIGTERM; SIGINT is the hand-run Ctrl-C path.
/// Both must exit GRACEFULLY through wait_for_exit (process exits with code
/// 0). Pre-fix, SIGTERM killed via default disposition: terminated-by-signal,
/// status.code() == None — which is exactly what this asserts against.
#[test]
fn daemon_exits_gracefully_on_sigterm_and_sigint() {
    for sig in [Signal::SIGTERM, Signal::SIGINT] {
        let bin = env!("CARGO_BIN_EXE_iii-worker");
        let tmp = tempfile::tempdir().unwrap();
        let logfile = tmp.path().join("daemon.log");

        // Direct child: its parent (this test) stays alive, so the reparent
        // watch arms but must never fire — the signal is the only exit.
        // tracing_subscriber::fmt() writes to STDOUT, so both streams go to
        // the logfile.
        let log = std::fs::File::create(&logfile).unwrap();
        let log_err = log.try_clone().unwrap();
        let mut daemon = Command::new(bin)
            .args(["worker-manager-daemon", "--engine", "ws://127.0.0.1:1"])
            .env("RUST_LOG", "info")
            .env("HOME", tmp.path())
            .env_remove("III_ENGINE_PID")
            .stdout(std::process::Stdio::from(log))
            .stderr(std::process::Stdio::from(log_err))
            .spawn()
            .expect("spawn daemon");
        let pid = daemon.id() as i32;

        // Signal only after the handlers are demonstrably installed — the
        // "armed" line is logged from the same select that owns the signal
        // arms, so its presence means a too-early default-disposition death
        // can no longer happen.
        let (armed, log) =
            wait_for_log_line(&logfile, "parent exit-watch armed", Duration::from_secs(10));
        if !armed {
            let _ = daemon.kill();
            panic!("daemon never armed its exit watch ({sig}); log:\n{log}");
        }

        kill(Pid::from_raw(pid), sig).expect("send signal");

        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(s) = daemon.try_wait().expect("try_wait") {
                break s;
            }
            if Instant::now() >= deadline {
                let _ = daemon.kill();
                panic!("daemon ignored {sig}");
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        assert_eq!(
            status.code(),
            Some(0),
            "{sig} must exit gracefully via wait_for_exit (None = killed by default disposition)"
        );
    }
}
