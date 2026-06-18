// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
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
use std::time::{Duration, Instant};

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

/// Virtio-console port name carrying the host↔guest shell-exec channel
/// (`iii worker exec`). Independent of `III_CONTROL_PORT`: the shell
/// dispatcher always runs on a dedicated thread alongside the worker
/// child and the optional control thread. If the env var is unset, or
/// the named port can't be found in sysfs, exec is simply unavailable
/// for this VM — the worker child keeps running normally.
const III_SHELL_PORT_ENV: &str = "III_SHELL_PORT";

/// Marker file the host boot script touches after a successful
/// setup+install (see `local_worker.rs::build_libkrun_local_script`). Its
/// presence makes subsequent boots skip the install step and reuse the
/// VM-local dep dirs. `/var` is host-backed, so the marker — and the
/// cached deps — survive VM restarts.
///
/// The gate trusts the install command's exit code, not the resulting
/// dep tree. An install that exits 0 but leaves an incomplete `node_modules`
/// (e.g. a transient drop of one package) gets frozen behind the marker:
/// every later boot reuses the broken deps and the worker crashes on the
/// same missing import forever. Deleting this marker on an early crash
/// forces the next boot to re-run install and self-heal.
const PREPARED_MARKER: &str = "/var/.iii-prepared";

/// If the worker child exits non-zero within this window of its initial
/// spawn — or shortly after this boot creates the prepared marker — treat it
/// as a failed boot (broken deps / bad install) rather than a runtime crash
/// and invalidate the prepared marker.
const BOOT_CRASH_WINDOW: Duration = Duration::from_secs(30);

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

/// Delete the prepared marker so the next boot re-runs setup/install.
/// Best-effort: a missing marker (already gone, or never prepared) and a
/// read-only `/var` are both non-fatal — we only log the unexpected case.
fn invalidate_prepared_marker_at(path: &str) {
    match std::fs::remove_file(path) {
        Ok(()) => eprintln!(
            "iii-init: worker exited non-zero during boot; cleared {path} \
             so the next start re-runs install"
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!("iii-init: warning: could not clear {path}: {e}"),
    }
}

/// True when a child exit should invalidate the prepared marker: a real
/// process exit (not a signal — `kill_for_shutdown` uses SIGTERM and must
/// not be mistaken for a crash) with a non-zero code, either soon enough
/// after initial spawn or soon enough after this boot created the marker to
/// implicate the boot/deps rather than runtime behavior.
fn should_invalidate_prepared_marker(
    was_exit: bool,
    code: i32,
    spawned_at: Instant,
    marker_existed_at_spawn: bool,
) -> bool {
    crate::prepared_marker::should_invalidate(
        was_exit,
        code,
        spawned_at,
        BOOT_CRASH_WINDOW,
        marker_existed_at_spawn,
        crate::prepared_marker::marker_is_recent(Path::new(PREPARED_MARKER), BOOT_CRASH_WINDOW),
    )
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

    // Spawn the shell-exec dispatcher thread if wired. Independent of
    // legacy/supervisor mode — exec sessions are separate children, not
    // substitutes for the worker child. If the named port isn't in
    // sysfs (host forgot `--shell-sock`, or sysfs not mounted), we
    // log and continue without exec; the worker still runs normally.
    maybe_spawn_shell_dispatcher();

    if let Ok(port_name) = std::env::var(III_CONTROL_PORT_ENV) {
        let workdir =
            std::env::var(III_WORKER_WORKDIR_ENV).unwrap_or_else(|_| "/workspace".to_string());
        return run_supervised(cmd, workdir, port_name);
    }

    run_legacy(cmd)
}

/// Look up `III_SHELL_PORT`, resolve the named virtio-console port via
/// sysfs, and spawn the dispatcher thread on it. Silently returns if
/// the env var is unset (feature not enabled for this VM) and logs a
/// warning if it's set but the port can't be found.
///
/// The spawned thread runs for the lifetime of the VM. It does not
/// block or join back with PID 1 — its death (port closed, read
/// error) simply disables future exec sessions; the worker and any
/// in-flight exec children keep running.
fn maybe_spawn_shell_dispatcher() {
    let port_name = match std::env::var(III_SHELL_PORT_ENV) {
        Ok(p) => p,
        Err(_) => return,
    };
    match iii_supervisor::control::find_virtio_port_by_name(&port_name) {
        Some(port_path) => {
            std::thread::Builder::new()
                .name("iii-init-shell".to_string())
                .spawn(move || {
                    if let Err(e) = crate::shell_dispatcher::run(&port_path) {
                        eprintln!("iii-init: shell dispatcher error: {e}");
                    }
                })
                .expect("spawn shell dispatcher thread");
        }
        None => {
            eprintln!(
                "iii-init: warning: III_SHELL_PORT={port_name} but no matching port \
                 in /sys/class/virtio-ports. `iii worker exec` disabled for this VM."
            );
        }
    }
}

/// Legacy path: spawn worker, reap zombies, exit with child's code.
/// Identical to the pre-merge behavior of this module.
fn run_legacy(cmd: String) -> Result<(), InitError> {
    let marker_existed_at_spawn = Path::new(PREPARED_MARKER).exists();
    let child = Command::new("/bin/sh")
        .arg("-c")
        .arg(&cmd)
        .spawn()
        .map_err(InitError::SpawnWorker)?;

    let child_pid = child.id() as i32;
    CHILD_PID.store(child_pid, Ordering::SeqCst);

    attach_to_worker_cgroup(child_pid);
    let spawned_at = Instant::now();

    // PID 1 supervisor loop: wait for children, reap orphans (INIT-07).
    // Before applying our own termination logic, consult the shell
    // dispatcher's exit registry — exits belonging to `iii worker
    // exec` children must be forwarded to the dispatcher's waiter
    // thread, not silently discarded as "orphans".
    //
    // The loop breaks with `(code, was_exit)` so the post-loop boot-crash
    // check can tell a real non-zero process exit from a signal death.
    let (status, was_exit) = loop {
        match waitpid(Pid::from_raw(-1), None) {
            Ok(WaitStatus::Exited(pid, code)) => {
                if crate::child_exits::dispatch_exit(pid.as_raw(), code) {
                    continue;
                }
                if pid.as_raw() == child_pid {
                    break (code, true);
                }
            }
            Ok(WaitStatus::Signaled(pid, sig, _)) => {
                if crate::child_exits::dispatch_exit(pid.as_raw(), 128 + sig as i32) {
                    continue;
                }
                if pid.as_raw() == child_pid {
                    break (128 + sig as i32, false);
                }
            }
            Ok(_) => continue, // stop/continue/etc, keep waiting
            Err(nix::Error::ECHILD) => break (0, false), // no more children
            Err(_) => break (1, false), // unexpected error
        }
    };

    if should_invalidate_prepared_marker(was_exit, status, spawned_at, marker_existed_at_spawn) {
        invalidate_prepared_marker_at(PREPARED_MARKER);
    }

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

    let marker_existed_at_spawn = Path::new(PREPARED_MARKER).exists();
    let initial_pid = state.spawn_initial().map_err(|e| {
        InitError::SpawnWorker(std::io::Error::other(format!(
            "supervisor spawn_initial failed: {e}"
        )))
    })?;
    CHILD_PID.store(initial_pid as i32, Ordering::SeqCst);
    attach_to_worker_cgroup(initial_pid as i32);
    // Timed from the initial spawn, not from any host-driven restart. The
    // marker-recency check below covers slow setup/install flows where the
    // marker is created long after this instant, just before the worker
    // crashes on incomplete deps.
    let spawned_at = Instant::now();

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
    //
    // Dispatcher-owned children (`iii worker exec`) are peeled off via
    // the shared `child_exits` registry before we check
    // `exit_is_terminal` — otherwise an exec child's exit code would
    // look like an orphan and the dispatcher's waiter thread would
    // wait forever.
    let (status, was_exit) = loop {
        match waitpid(Pid::from_raw(-1), None) {
            Ok(WaitStatus::Exited(pid, code)) => {
                if crate::child_exits::dispatch_exit(pid.as_raw(), code) {
                    continue;
                }
                if exit_is_terminal(&state, pid.as_raw()) {
                    break (code, true);
                }
            }
            Ok(WaitStatus::Signaled(pid, sig, _)) => {
                if crate::child_exits::dispatch_exit(pid.as_raw(), 128 + sig as i32) {
                    continue;
                }
                if exit_is_terminal(&state, pid.as_raw()) {
                    break (128 + sig as i32, false);
                }
            }
            Ok(_) => continue, // stop/continue/etc — keep waiting
            Err(nix::Error::ECHILD) => break (0, false), // no more children
            Err(_) => break (1, false),
        }
    };

    if should_invalidate_prepared_marker(was_exit, status, spawned_at, marker_existed_at_spawn) {
        invalidate_prepared_marker_at(PREPARED_MARKER);
    }

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

    #[test]
    fn invalidate_prepared_marker_removes_and_tolerates_missing() {
        let path =
            std::env::temp_dir().join(format!("iii-init-prepared-test-{}", std::process::id()));
        let path_str = path.to_str().unwrap();

        std::fs::write(&path, b"prepared").unwrap();
        assert!(path.exists());

        // Removes an existing marker.
        invalidate_prepared_marker_at(path_str);
        assert!(!path.exists());

        // Tolerates an already-missing marker (no panic, no error).
        invalidate_prepared_marker_at(path_str);
        assert!(!path.exists());
    }
}
