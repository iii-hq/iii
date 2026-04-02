use std::process::Command;
use std::sync::atomic::{AtomicI32, Ordering};

use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;

use crate::error::InitError;

/// Stores the child worker PID for signal handler access.
/// 0 means no child has been spawned yet.
static CHILD_PID: AtomicI32 = AtomicI32::new(0);

/// Signal handler that forwards SIGTERM/SIGINT to the child worker process.
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

/// Spawns the worker process and enters the PID 1 supervisor loop.
///
/// Sequence:
/// 1. Read `III_WORKER_CMD` from environment
/// 2. Install SIGTERM/SIGINT handlers (before spawn to prevent race condition)
/// 3. Spawn worker via `/bin/sh -c $III_WORKER_CMD`
/// 4. Store child PID for signal forwarding
/// 5. Enter waitpid(-1) loop: reap orphans, track child exit
/// 6. Exit with child's exit code (or 128 + signal for signaled children)
pub fn exec_worker() -> Result<(), InitError> {
    let cmd = std::env::var("III_WORKER_CMD").map_err(|_| InitError::MissingWorkerCmd)?;

    // Install signal handlers BEFORE spawning child (Pitfall 4 prevention).
    install_signal_handlers();

    let child = Command::new("/bin/sh")
        .arg("-c")
        .arg(&cmd)
        .spawn()
        .map_err(InitError::SpawnWorker)?;

    let child_pid = child.id() as i32;
    CHILD_PID.store(child_pid, Ordering::SeqCst);

    // Move worker into memory-limited cgroup (best-effort, set up by mount.rs).
    let _ = std::fs::write(
        "/sys/fs/cgroup/worker/cgroup.procs",
        child_pid.to_string(),
    );

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
        let result = exec_worker();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, InitError::MissingWorkerCmd),
            "expected MissingWorkerCmd, got: {err}"
        );
    }

    #[test]
    fn test_child_pid_starts_at_zero() {
        assert_eq!(CHILD_PID.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_signal_handler_signature() {
        // Compile-time check that the signal handler has the correct
        // extern "C" fn(c_int) signature required by libc::sigaction.
        let _: unsafe extern "C" fn(libc::c_int) = signal_handler;
    }
}
