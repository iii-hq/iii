// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! PID-1 supervision of the user worker process.
//!
//! Two modes, selected by the presence of `III_CONTROL_PORT` in the
//! environment:
//!
//! - **Legacy mode** (`III_CONTROL_PORT` unset). Spawn the worker via
//!   `/bin/sh -c $III_WORKER_CMD`, forward SIGTERM/SIGINT to it, run the
//!   classic `waitpid(-1)` reap loop, and exit with the child's code
//!   when it dies. Used for production workloads that don't need
//!   host-driven fast restart.
//!
//! - **Supervisor mode** (`III_CONTROL_PORT` set to the virtio-console
//!   port name, e.g. `iii.control`). Same spawn + signal forwarding,
//!   plus a background thread that serves the host's control-channel
//!   RPC (`Restart`/`Shutdown`/`Ping`/`Status`) by delegating to the
//!   `iii_supervisor` library. On `Restart`, the library kills the
//!   current child and respawns a fresh one in-place. The PID-1 reap
//!   loop tolerates these transitions: it exits only when the child
//!   dies and is not being replaced by a restart in flight.
//!
//! Supervisor mode replaces the earlier architecture where a separate
//! `iii-supervisor` binary lived at `/opt/iii/supervisor` inside the
//! rootfs and was exec'd from the boot script. The init binary already
//! runs as PID 1; absorbing the control-channel loop removes the extra
//! binary, the extra exec hop, and the install plumbing that shipped it
//! into every rootfs.

use std::io::BufReader;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicI32, Ordering};

use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;

use crate::error::InitError;

/// Virtio-console port name (as seen in `/sys/class/virtio-ports/*/name`)
/// that the supervisor serves its RPC channel on. When set, init enters
/// supervisor mode; when unset, init stays on the legacy exec_worker path.
const III_CONTROL_PORT_ENV: &str = "III_CONTROL_PORT";

/// Working directory env for supervisor mode. Defaults to `/workspace`
/// (local-path worker convention). The legacy mode ignores this — the
/// worker inherits whatever cwd libkrun's Exec config picked.
const III_WORKER_WORKDIR_ENV: &str = "III_WORKER_WORKDIR";

/// Stores the current child worker PID for async-signal-safe signal
/// forwarding. 0 means no child has been spawned yet. Updated both on
/// initial spawn and after every respawn inside supervisor mode.
static CHILD_PID: AtomicI32 = AtomicI32::new(0);

/// Signal handler that forwards SIGTERM/SIGINT to the current child.
///
/// If no child has been spawned yet (CHILD_PID == 0), exits immediately
/// with code 128 + signal number.
///
/// Only calls async-signal-safe functions: atomic load, libc::kill, libc::_exit.
unsafe extern "C" fn signal_handler(sig: libc::c_int) {
    let pid = CHILD_PID.load(Ordering::SeqCst);
    if pid > 0 {
        unsafe { libc::kill(pid, sig) };
    } else {
        unsafe { libc::_exit(128 + sig) };
    }
}

/// Installs signal handlers for SIGTERM and SIGINT using `libc::sigaction`.
///
/// Handlers are installed with `SA_RESTART` so interrupted syscalls are
/// automatically restarted.
fn install_signal_handlers() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = signal_handler as *const () as usize;
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);

        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
    }
}

/// Move `pid` into the worker cgroup (best-effort — the cgroup setup is
/// done in `mount.rs` and may not exist on all kernels).
fn attach_to_worker_cgroup(pid: i32) {
    let _ = std::fs::write("/sys/fs/cgroup/worker/cgroup.procs", pid.to_string());
}

/// Entry point called from `main::run` after all boot setup is complete.
///
/// Dispatches between legacy and supervisor mode based on env.
pub fn exec_worker() -> Result<(), InitError> {
    let cmd = std::env::var("III_WORKER_CMD").map_err(|_| InitError::MissingWorkerCmd)?;

    // Install handlers BEFORE spawning so a signal arriving mid-spawn
    // can still find a live CHILD_PID (racily) or exit cleanly via the
    // _exit fallback.
    install_signal_handlers();

    if let Ok(port_name) = std::env::var(III_CONTROL_PORT_ENV) {
        let workdir =
            std::env::var(III_WORKER_WORKDIR_ENV).unwrap_or_else(|_| "/workspace".to_string());
        return run_supervised(cmd, workdir, port_name);
    }

    run_legacy(cmd)
}

/// Legacy path: spawn worker, reap zombies, exit with child's code.
/// Identical to the pre-merge behavior of this module.
fn run_legacy(cmd: String) -> Result<(), InitError> {
    let child = Command::new("/bin/sh")
        .arg("-c")
        .arg(&cmd)
        .spawn()
        .map_err(InitError::SpawnWorker)?;

    let child_pid = child.id() as i32;
    CHILD_PID.store(child_pid, Ordering::SeqCst);

    attach_to_worker_cgroup(child_pid);

    // PID 1 supervisor loop: wait for children, reap orphans (INIT-07).
    let status = loop {
        match waitpid(Pid::from_raw(-1), None) {
            Ok(WaitStatus::Exited(pid, code)) if pid.as_raw() == child_pid => {
                break code;
            }
            Ok(WaitStatus::Signaled(pid, sig, _)) if pid.as_raw() == child_pid => {
                break 128 + sig as i32;
            }
            Ok(_) => continue,                  // reaped an orphan, keep waiting
            Err(nix::Error::ECHILD) => break 0, // no more children
            Err(_) => break 1,                  // unexpected error
        }
    };

    std::process::exit(status);
}

/// Supervisor path: spawn worker via `iii_supervisor::child::State`,
/// serve host-driven restart/shutdown RPCs on a background thread,
/// run a restart-tolerant PID-1 reap loop.
fn run_supervised(cmd: String, workdir: String, port_name: String) -> Result<(), InitError> {
    use iii_supervisor::child::{Config, State};

    let state = State::new(Config {
        run_cmd: cmd,
        workdir,
    });

    let initial_pid = state.spawn_initial().map_err(|e| {
        InitError::SpawnWorker(std::io::Error::other(format!(
            "supervisor spawn_initial failed: {e}"
        )))
    })?;
    CHILD_PID.store(initial_pid as i32, Ordering::SeqCst);
    attach_to_worker_cgroup(initial_pid as i32);

    // Open the named virtio-console port and spawn the control loop on
    // a dedicated thread. If the port isn't present (sysfs not mounted,
    // or the host forgot to wire it), we log a warning and continue
    // running without the fast-restart channel — the child is already
    // spawned and the PID-1 reap loop below keeps the VM healthy; the
    // host watcher will fall back to full VM restarts.
    match iii_supervisor::control::find_virtio_port_by_name(&port_name) {
        Some(port_path) => {
            let state_for_control = state.clone();
            std::thread::Builder::new()
                .name("iii-init-control".to_string())
                .spawn(move || {
                    if let Err(e) = run_control_loop(state_for_control, &port_path) {
                        eprintln!("iii-init: control loop error: {e}");
                    }
                })
                .expect("spawn control thread");
        }
        None => {
            eprintln!(
                "iii-init: warning: III_CONTROL_PORT={port_name} but no matching port \
                 in /sys/class/virtio-ports. Fast-restart disabled; \
                 host watcher will fall back to full VM restart."
            );
        }
    }

    // PID-1 reap loop. Distinguishes our child (state.pid()) from orphans
    // and tolerates restart-driven child replacements. See
    // `exit_is_terminal` for the coordination contract.
    let status = loop {
        match waitpid(Pid::from_raw(-1), None) {
            Ok(WaitStatus::Exited(pid, code)) => {
                if exit_is_terminal(&state, pid.as_raw()) {
                    break code;
                }
            }
            Ok(WaitStatus::Signaled(pid, sig, _)) => {
                if exit_is_terminal(&state, pid.as_raw()) {
                    break 128 + sig as i32;
                }
            }
            Ok(_) => continue,                  // stop/continue/etc — keep waiting
            Err(nix::Error::ECHILD) => break 0, // no more children
            Err(_) => break 1,
        }
    };

    std::process::exit(status);
}

/// Is this dead PID a terminal exit, or was it replaced by a restart?
///
/// Coordinates with [`iii_supervisor::child::State::kill_and_respawn`]
/// via the state's internal mutex:
///
/// - When the control thread is mid-restart, it holds the state lock
///   across kill + wait + spawn. A concurrent call to `state.pid()`
///   blocks until the new child is installed, so by the time we read
///   `state.pid()` we see either the new pid (restart successful,
///   `dead_pid` is stale, not terminal) or `None` (spawn failed,
///   nothing replaced the child — terminal).
///
/// - When there's no restart in flight and the child crashed on its own,
///   `state.pid()` returns `Some(dead_pid)` (the stored child handle
///   still reports its pid even after external reaping via waitpid(-1)),
///   which we treat as terminal.
///
/// - `None` in any case means "no child is alive and none is being
///   spawned" — terminal.
fn exit_is_terminal(state: &iii_supervisor::child::State, dead_pid: i32) -> bool {
    match state.pid() {
        None => true,
        Some(current) if current as i32 == dead_pid => true,
        Some(_) => false,
    }
}

/// Serve the host's control RPCs on `port_path`. Runs on a dedicated
/// thread; returning from here means the host closed its end of the
/// channel or a `Shutdown` RPC was processed. Either way, the PID-1
/// main thread keeps running until the child exits, at which point
/// the whole VM powers down via `process::exit`.
///
/// Delegates the read-dispatch-write loop to
/// `iii_supervisor::control::serve_with`, hooking the post-dispatch
/// callback to sync [`CHILD_PID`] + the worker cgroup after every
/// successful `Restart`. We can't sync these from a signal handler
/// (not async-signal-safe), so they live here.
fn run_control_loop(state: iii_supervisor::child::State, port_path: &Path) -> anyhow::Result<()> {
    use iii_supervisor::control::serve_with;
    use iii_supervisor::protocol::{Request, Response};

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(port_path)?;
    let writer = file.try_clone()?;
    let reader = BufReader::new(file);

    serve_with(state, reader, writer, |req, resp, state| {
        if matches!(req, Request::Restart)
            && matches!(resp, Response::Ok)
            && let Some(new_pid) = state.pid()
        {
            CHILD_PID.store(new_pid as i32, Ordering::SeqCst);
            attach_to_worker_cgroup(new_pid as i32);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_worker_cmd() {
        // Ensure III_WORKER_CMD is not set in this test's environment.
        // SAFETY: remove_var is unsafe in edition 2024 because it is inherently
        // racy in multi-threaded programs. This is acceptable in a test where we
        // control the environment.
        unsafe { std::env::remove_var("III_WORKER_CMD") };
        unsafe { std::env::remove_var(III_CONTROL_PORT_ENV) };
        let result = exec_worker();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, InitError::MissingWorkerCmd),
            "expected MissingWorkerCmd, got: {err}"
        );
    }

    #[test]
    fn test_signal_handler_signature() {
        // Compile-time check that the signal handler has the correct
        // extern "C" fn(c_int) signature required by libc::sigaction.
        let _: unsafe extern "C" fn(libc::c_int) = signal_handler;
    }

    #[test]
    fn exit_is_terminal_agrees_with_state_pid() {
        use iii_supervisor::child::{Config, State};
        let state = State::new(Config {
            run_cmd: "sleep 5".to_string(),
            workdir: "/tmp".to_string(),
        });
        let pid = state.spawn_initial().unwrap();

        // Dead pid matches current child → terminal.
        assert!(exit_is_terminal(&state, pid as i32));

        // Some other pid dying (e.g. an orphan) → not terminal.
        assert!(!exit_is_terminal(&state, (pid + 999) as i32));

        // After a respawn, the old pid is stale → not terminal.
        let old_pid = pid;
        let new_pid = state.kill_and_respawn().unwrap();
        assert_ne!(old_pid, new_pid);
        assert!(!exit_is_terminal(&state, old_pid as i32));
        assert!(exit_is_terminal(&state, new_pid as i32));

        state.kill_for_shutdown().unwrap();
        // After shutdown, state has no child → terminal.
        assert!(exit_is_terminal(&state, new_pid as i32));
    }
}
