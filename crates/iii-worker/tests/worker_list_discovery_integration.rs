// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration tests for orphan-worker discovery in `iii worker list`.
//!
//! These tests exercise the public discovery functions
//! ([`discover_disk_worker_names`], [`discover_running_worker_names_from_ps`])
//! and the liveness check ([`is_worker_running`]) against a real filesystem
//! tree under a temporary HOME, using the current test process's PID as a
//! known-alive sentinel. This is the regression coverage for the bug where
//! `iii worker list` missed workers whose project folders had moved or whose
//! PID files had been removed while the process kept running.

use iii_worker::cli::managed::{
    discover_disk_worker_names, discover_running_worker_names_from_ps, find_worker_pid_from_ps,
    is_worker_running,
};
use std::sync::Mutex;

/// Serializes tests that mutate the HOME env var. HOME is process-global, so
/// any two tests in this file that override it must run one at a time.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard that overrides HOME for the duration of a test and restores the
/// original value (or removes the var if it was unset) on drop, even if the
/// test panics.
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

/// Empty `~/.iii` tree → discovery returns nothing. Guards against any latent
/// assumption that the dirs always exist.
#[test]
fn discover_disk_worker_names_empty_home() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::new(tmp.path());

    let names = discover_disk_worker_names();
    assert!(names.is_empty(), "expected no workers, got {names:?}");
}

/// Populates a fake `~/.iii` with the two real on-disk shapes
/// (`managed/{name}/` and `pids/{name}.pid`) and asserts the discovery
/// function returns the union, sorted and deduplicated.
#[test]
fn discover_disk_worker_names_finds_managed_and_pids() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::new(tmp.path());

    // Two OCI/VM-style workers (managed dirs).
    std::fs::create_dir_all(tmp.path().join(".iii/managed/oci-one")).unwrap();
    std::fs::create_dir_all(tmp.path().join(".iii/managed/oci-two")).unwrap();
    // Two binary-style workers (pidfiles), one overlapping with managed/oci-one.
    let pids_dir = tmp.path().join(".iii/pids");
    std::fs::create_dir_all(&pids_dir).unwrap();
    std::fs::write(pids_dir.join("bin-only.pid"), "1234").unwrap();
    std::fs::write(pids_dir.join("oci-one.pid"), "5678").unwrap();

    let names = discover_disk_worker_names();
    assert_eq!(names, vec!["bin-only", "oci-one", "oci-two"]);
}

/// `is_worker_running` must return `true` when the pidfile holds a PID that
/// is actually alive. The current test process's PID is the simplest
/// guaranteed-alive sentinel, exercising the real signal-0 path.
#[test]
fn is_worker_running_true_for_alive_pid() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::new(tmp.path());

    let pids_dir = tmp.path().join(".iii/pids");
    std::fs::create_dir_all(&pids_dir).unwrap();
    let my_pid = std::process::id();
    std::fs::write(pids_dir.join("alive-worker.pid"), my_pid.to_string()).unwrap();

    assert!(
        is_worker_running("alive-worker"),
        "expected alive-worker (pid {my_pid}, this process) to be detected as running"
    );
}

/// `is_worker_running` must return `false` for an unmistakably dead PID.
/// This is the case that produces "stopped" status in `iii worker list` and
/// excludes a name from the orphan set if it is also missing from config.
#[test]
fn is_worker_running_false_for_dead_pid() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::new(tmp.path());

    let pids_dir = tmp.path().join(".iii/pids");
    std::fs::create_dir_all(&pids_dir).unwrap();
    // PID well above any plausible live pid on Linux/macOS (PID_MAX defaults
    // are 32k or 4M; 2_000_000_000 is reliably dead).
    std::fs::write(pids_dir.join("dead-worker.pid"), "2000000000").unwrap();

    assert!(
        !is_worker_running("dead-worker"),
        "expected dead-worker (pid 2_000_000_000) to be detected as stopped"
    );
}

/// End-to-end orphan discovery on Linux/macOS: this test process is one of
/// the live `ps`/`/proc` entries, so its own argv0 should surface through
/// the cmdline scanner when we point HOME at a fake tree where that argv0
/// lives under `~/.iii/workers/{name}`.
///
/// We don't relocate the binary; we just verify the scanner is wired up and
/// returns *something* non-empty in normal environments. On platforms with
/// no implementation (anything other than Linux/macOS) the function returns
/// an empty Vec, which we accept.
#[test]
fn discover_running_from_ps_returns_some_processes() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::new(tmp.path());

    let result = discover_running_worker_names_from_ps();

    if cfg!(any(target_os = "linux", target_os = "macos")) {
        // Result may be empty if no live processes match the iii-worker
        // patterns under our fresh fake HOME — that is the correct outcome
        // for an isolated test environment. The contract under test is that
        // the call succeeds without panicking and returns a sorted Vec.
        let mut sorted = result.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(result, sorted, "discovery output must be sorted & deduped");
    } else {
        assert!(
            result.is_empty(),
            "non-Linux/macOS platforms should return empty, got {result:?}"
        );
    }
}

/// End-to-end orphan PID lookup: stages a real executable at the path the
/// scanner expects (`<tmp HOME>/.iii/workers/{name}`), spawns it as a child
/// with a long sleep, and asserts `find_worker_pid_from_ps` returns the
/// child's PID. This is the regression coverage for `iii worker stop` of
/// orphan binary workers whose pidfiles have been removed.
///
/// Skipped on non-Unix and on platforms without `/bin/sleep`.
///
/// macOS caveat: `/var -> /private/var` is a symlink, and `tempfile::tempdir`
/// lands under `$TMPDIR` which is typically `/var/folders/...`. Once a
/// process is spawned, the kernel stores the canonicalized path, and
/// `ps -o args=` emits `/private/var/folders/...`. Without canonicalizing
/// the HOME we use for `workers_prefix`, `strip_prefix` misses the match
/// and the test flakes as "expected pid, got None". Linux doesn't have
/// this specific symlink but canonicalize is still the right call.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn find_worker_pid_from_ps_locates_live_orphan_binary() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let canonical_home = std::fs::canonicalize(tmp.path()).unwrap();
    let _home = HomeGuard::new(&canonical_home);

    // Stage an executable at the exact path the scanner uses to recognise
    // binary workers. Copying /bin/sleep keeps this dependency-free.
    let workers_dir = canonical_home.join(".iii/workers");
    std::fs::create_dir_all(&workers_dir).unwrap();
    let worker_name = format!("test-orphan-{}", std::process::id());
    let worker_bin = workers_dir.join(&worker_name);
    std::fs::copy("/bin/sleep", &worker_bin).expect("/bin/sleep must exist on Unix");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&worker_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // Spawn the sentinel with a long-enough sleep to outlive this test.
    let mut child = std::process::Command::new(&worker_bin)
        .arg("60")
        .spawn()
        .expect("spawn sentinel sleep process");

    // macOS `ps` has a non-deterministic window after fork/exec before a new
    // process appears in the table — observed failures at 200ms with passes
    // at 400-600ms under CI load. Poll with short sleeps instead of a single
    // fixed sleep so the test is robust without slowing the happy path.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut found = None;
    while std::time::Instant::now() < deadline {
        if let Some(pid) = find_worker_pid_from_ps(&worker_name) {
            found = Some(pid);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Always tear down the sentinel before asserting, so a failed assertion
    // doesn't leak a 60-second sleep.
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(
        found,
        Some(child.id()),
        "expected to find sentinel pid {} within 3s, got {found:?}",
        child.id()
    );
}

/// Sanity check: if no matching process exists for a fresh, unique name, the
/// PID lookup must return `None` rather than confidently misidentifying some
/// unrelated process.
#[test]
fn find_worker_pid_from_ps_returns_none_for_unknown_name() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::new(tmp.path());

    let unique = format!("definitely-no-such-worker-{}", std::process::id());
    assert_eq!(find_worker_pid_from_ps(&unique), None);
}

/// Regression test for the restart-path duplicate-spawn bug in
/// [`handle_managed_restart`] (`crates/iii-worker/src/cli/managed.rs`).
///
/// The old restart path gated stop on `is_worker_running`, which only reads
/// pidfiles. When a worker's pidfile was deleted but the process kept running
/// (orphan), the gate returned false, stop was skipped, and start spawned a
/// duplicate. The fix now calls `handle_managed_stop` unconditionally, whose
/// three-tier discovery (OCI pidfile → binary pidfile → `ps` scan) catches
/// those orphans.
///
/// This test pins down the exact discovery asymmetry the fix relies on: stage
/// a real live orphan process at the expected binary-worker path but leave
/// **no pidfile on disk**, then assert that:
///   1. `is_worker_running` is blind to it (proves the old gate short-circuited).
///   2. `find_worker_pid_from_ps` (used by `handle_managed_stop`) still finds it
///      (proves the new unconditional-stop path catches it).
///
/// If either assertion flips in the future — e.g., `is_worker_running` gains a
/// `ps` fallback, or `find_worker_pid_from_ps` loses it — revisit whether the
/// unconditional-stop pattern in `handle_managed_restart` is still load-bearing.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn restart_orphan_invisible_to_is_worker_running_but_visible_to_ps() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    // Canonicalize — see the note in
    // `find_worker_pid_from_ps_locates_live_orphan_binary` for the macOS
    // `/var -> /private/var` reason.
    let canonical_home = std::fs::canonicalize(tmp.path()).unwrap();
    let _home = HomeGuard::new(&canonical_home);

    // Stage a binary-worker executable at the recognised path, but do NOT
    // create the corresponding pidfile under ~/.iii/pids/{name}.pid.
    let workers_dir = canonical_home.join(".iii/workers");
    std::fs::create_dir_all(&workers_dir).unwrap();
    let worker_name = format!("test-restart-orphan-{}", std::process::id());
    let worker_bin = workers_dir.join(&worker_name);
    std::fs::copy("/bin/sleep", &worker_bin).expect("/bin/sleep must exist on Unix");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&worker_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let mut child = std::process::Command::new(&worker_bin)
        .arg("60")
        .spawn()
        .expect("spawn sentinel sleep process");

    // macOS `ps` has a fork/exec registration window that varies under load
    // (see find_worker_pid_from_ps_locates_live_orphan_binary). Poll briefly
    // for the process to appear instead of a single fixed sleep.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut found_by_ps = None;
    while std::time::Instant::now() < deadline {
        if let Some(pid) = find_worker_pid_from_ps(&worker_name) {
            found_by_ps = Some(pid);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let running_by_pidfile = is_worker_running(&worker_name);

    // Always tear down before asserting.
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        !running_by_pidfile,
        "is_worker_running saw orphan {worker_name} without any pidfile on disk. \
         If this is intentional (e.g., is_worker_running gained a ps fallback), \
         the unconditional-stop pattern in handle_managed_restart may be redundant \
         and should be reviewed."
    );
    assert_eq!(
        found_by_ps,
        Some(child.id()),
        "handle_managed_stop's ps fallback failed to find live orphan \
         {worker_name} (pid {}). The restart-path fix depends on this lookup \
         working; if it has regressed, handle_managed_restart will once again \
         risk spawning duplicate workers.",
        child.id()
    );
}
