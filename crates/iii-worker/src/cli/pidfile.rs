// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Shared hardened pidfile I/O.
//!
//! Every pidfile a worker writes OR reads MUST go through this module —
//! no direct `fs::write` + `set_permissions`, no bare `fs::read_to_string`
//! on `.pid` paths. Sites today:
//!   - `~/.iii/managed/<name>/watch.pid` — source-watcher sidecar
//!     (`local_worker.rs`)
//!   - `~/.iii/managed/<name>/vm.pid` — libkrun VM
//!     (`worker_manager/libkrun.rs`)
//!   - `~/.iii/pids/<name>.pid` — binary workers (`managed.rs`)
//!
//! Reads in the engine crate (`engine/src/workers/registry_worker.rs`)
//! are a sibling concern; that crate has no dep on iii-worker, so it
//! re-implements the same O_NOFOLLOW + uid check inline and tests are
//! mirrored there.
//!
//! A local attacker with write access to any of these directories can
//! pre-plant the target path as a symlink pointing at a sensitive file
//! (e.g. `~/.ssh/authorized_keys`); a naive `std::fs::write` follows
//! the symlink and clobbers the target with a PID string. A naive
//! `fs::read_to_string` on a symlink-planted pidfile lets the attacker
//! redirect the read to arbitrary numeric-looking content, making
//! `is_alive(pid)` return true forever and wedging the restart loop.
//!
//! `write_pid_file` / `read_pid` open with `O_NOFOLLOW` on Unix so
//! symlink-planted pre-images cause the open to fail rather than
//! propagate the operation to the target. Writes lock mode to `0o600`;
//! reads verify the file is regular and owned by the current euid,
//! rejecting attacker-planted files even if the attacker won the
//! first-write race. Falls back to plain `std::fs` on non-Unix.
//!
//! Write failures are logged and swallowed at call sites that treat
//! pidfile writes as best-effort (the sidecar's PID helps stop-path
//! reaping but isn't required for the watcher itself to work). Callers
//! that need hard-fail semantics (e.g. libkrun's `vm.pid` — we kill
//! the spawned child if the write fails so we don't leak an untracked
//! VM) should use the `_strict` variant which returns `io::Result`.

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

/// Read `path` as a pidfile, returning `Some(pid)` when contents parse as `u32`.
///
/// On Unix, opens with `O_NOFOLLOW`, requires the opened file to be a
/// regular file owned by the current euid, and reads at most 32 bytes
/// (a PID is at most 10 decimal digits). Defends against the read-side
/// symlink-replace attack: a local attacker who planted `path` as a
/// symlink to `/etc/hostname`, `/proc/1/sched`, or any attacker-owned
/// file cannot trick us into treating its contents as a valid pid
/// marker that we then `kill()` or probe with `kill(pid, 0)`.
///
/// Returns `None` on any failure (open, metadata, ownership mismatch,
/// non-regular file, parse failure). Callers treat pidfile absence as
/// equivalent to pidfile invalidity, so silent `None` is correct.
///
/// On non-Unix platforms falls back to plain `read_to_string`.
pub fn read_pid(path: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
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
        // Cap at a few bytes — a PID is at most 10 decimal digits.
        let mut buf = [0u8; 32];
        let n = file.read(&mut buf).ok()?;
        let s = std::str::from_utf8(&buf[..n]).ok()?;
        s.trim().parse::<u32>().ok()
    }
    #[cfg(not(unix))]
    {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
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

    #[test]
    #[cfg(unix)]
    fn read_pid_refuses_to_follow_symlink() {
        // Attacker plants the pidfile as a symlink to a file they
        // control whose contents parse as a PID. A naive `read_to_string`
        // would return Some(pid) and let the attacker influence
        // liveness probes; `read_pid` must refuse to follow and return None.
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("planted_pid");
        std::fs::write(&target, "1234\n").unwrap();
        let pid_file = dir.path().join("vm.pid");
        std::os::unix::fs::symlink(&target, &pid_file).unwrap();

        assert_eq!(
            read_pid(&pid_file),
            None,
            "O_NOFOLLOW must refuse to follow symlinked pidfile"
        );
    }

    #[test]
    #[cfg(unix)]
    fn read_pid_round_trips_well_formed_file() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("vm.pid");
        write_pid_file_strict(&pid_file, 1234).unwrap();
        assert_eq!(read_pid(&pid_file), Some(1234));
    }

    #[test]
    #[cfg(unix)]
    fn read_pid_rejects_malformed_contents() {
        // Matches the cases the status::tests module exercises, but
        // pinned here so future callers can't accidentally weaken the
        // shared parser: empty, garbage, negative, overflow, whitespace.
        let dir = tempfile::tempdir().unwrap();
        for (name, contents) in [
            ("empty", ""),
            ("garbage", "not-a-pid"),
            ("negative", "-1"),
            ("overflow", "9999999999999"),
            ("whitespace-only", "   \n"),
        ] {
            let p = dir.path().join(format!("{}.pid", name));
            std::fs::write(&p, contents).unwrap();
            assert_eq!(
                read_pid(&p),
                None,
                "malformed pidfile content {:?} must yield None",
                contents
            );
        }
        // Surrounding whitespace is tolerated.
        let padded = dir.path().join("padded.pid");
        std::fs::write(&padded, "  12345  \n").unwrap();
        assert_eq!(read_pid(&padded), Some(12345));
    }

    /// Enforce the module invariant: known pidfile call sites must route
    /// through this module rather than reimplementing the hardening
    /// inline. This is a grep-style positive check — a future refactor
    /// that drops the `pidfile::*` reference in any of these files will
    /// trip the test and surface the regression at `cargo test` time,
    /// without needing a separate CI lint pass.
    #[test]
    fn known_call_sites_route_through_module() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let cases: &[(&str, &str)] = &[
            ("src/cli/local_worker.rs", "pidfile::write_pid_file"),
            ("src/cli/managed.rs", "pidfile::write_pid_file"),
            (
                "src/cli/worker_manager/libkrun.rs",
                "pidfile::write_pid_file_strict",
            ),
            ("src/cli/managed.rs", "pidfile::read_pid"),
            ("src/cli/status.rs", "pidfile::read_pid"),
        ];
        for (rel_path, token) in cases {
            let p = std::path::Path::new(manifest).join(rel_path);
            let contents =
                std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {:?}: {}", p, e));
            assert!(
                contents.contains(token),
                "{} must reference `{}` (pidfile module invariant — do not \
                 reimplement the hardening inline; if this call site was \
                 removed, drop it from this test)",
                rel_path,
                token,
            );
        }
    }
}
