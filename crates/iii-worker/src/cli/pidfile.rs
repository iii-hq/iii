// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Shared hardened pidfile I/O.
//!
//! Every pidfile a worker writes MUST go through this module — no
//! direct `fs::write` + `set_permissions`. Sites today:
//!   - `~/.iii/managed/<name>/watch.pid` — source-watcher sidecar
//!     (`local_worker.rs`)
//!   - `~/.iii/managed/<name>/vm.pid` — libkrun VM
//!     (`worker_manager/libkrun.rs`)
//!   - `~/.iii/pids/<name>.pid` — binary workers (`managed.rs`)
//!
//! A local attacker with write access to any of these directories can
//! pre-plant the target path as a symlink pointing at a sensitive file
//! (e.g. `~/.ssh/authorized_keys`); a naive `std::fs::write` follows
//! the symlink and clobbers the target with a PID string.
//!
//! `write_pid_file` opens with `O_NOFOLLOW` on Unix so symlink-planted
//! pre-images cause the open to fail rather than propagate the write to
//! the target. Mode is locked to `0o600` so neighboring same-uid
//! attackers can't race the permissions after creation. Falls back to
//! `std::fs::write` on non-Unix.
//!
//! Failures are logged and swallowed at call sites that treat pidfile
//! writes as best-effort (the sidecar's PID helps stop-path reaping but
//! isn't required for the watcher itself to work). Callers that need
//! hard-fail semantics (e.g. libkrun's `vm.pid` — we kill the spawned
//! child if the write fails so we don't leak an untracked VM) should
//! use the `_strict` variant which returns `io::Result`.

use std::path::Path;

/// Open a pidfile for writing with symlink-replace defense.
///
/// Unix: `O_NOFOLLOW | O_CREAT | O_WRONLY | O_TRUNC`, mode `0o600`.
/// Non-Unix: plain `OpenOptions::create+write+truncate`.
fn open_pid_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(nix::libc::O_NOFOLLOW);
        opts.mode(0o600);
    }
    opts.open(path)
}

/// Write `pid` to `path` with symlink defense. Returns the io::Result
/// so callers that care (e.g. VM spawn, where a failed pidfile means we
/// must kill the just-spawned child) can react.
pub fn write_pid_file_strict(path: &Path, pid: u32) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = open_pid_file(path)?;
    write!(f, "{}", pid)?;
    Ok(())
}

/// Write `pid` to `path` with symlink defense. Errors are logged at
/// warn level and swallowed — use when the pidfile is a reap hint
/// rather than a correctness requirement (sidecar watchers, etc).
pub fn write_pid_file(path: &Path, pid: u32) {
    if let Err(e) = write_pid_file_strict(path, pid) {
        tracing::warn!(path = %path.display(), error = %e, "failed to write pidfile");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn strict_refuses_to_follow_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("secret");
        std::fs::write(&target, "original").unwrap();
        let pid_file = dir.path().join("vm.pid");
        std::os::unix::fs::symlink(&target, &pid_file).unwrap();

        let res = write_pid_file_strict(&pid_file, 42);
        assert!(res.is_err(), "O_NOFOLLOW must refuse symlinked pidfile");
        // Target must be untouched.
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "original");
    }

    #[test]
    #[cfg(unix)]
    fn strict_creates_file_with_owner_only_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("vm.pid");
        write_pid_file_strict(&pid_file, 1234).unwrap();
        let meta = std::fs::metadata(&pid_file).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "pidfile must be 0o600, got {:o}", mode);
        assert_eq!(std::fs::read_to_string(&pid_file).unwrap(), "1234");
    }

    #[test]
    fn lossy_swallows_errors() {
        // Path with a non-existent parent should fail on open but not
        // panic; just observe no file was created.
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("does_not_exist").join("vm.pid");
        write_pid_file(&bad, 42);
        assert!(!bad.exists());
    }
}
