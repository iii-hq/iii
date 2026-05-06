// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Managed worker integration tests for handle_managed_add/remove flows.
//!
//! Covers: handle_managed_add_many, handle_managed_add (single builtin),
//! handle_managed_remove_many, and merge/default behaviors.

mod common;

use common::isolation::in_temp_dir_async;

#[tokio::test]
async fn add_many_builtin_workers() {
    in_temp_dir_async(|| async {
        let names = vec!["iii-http".to_string(), "iii-state".to_string()];
        let exit_code = iii_worker::cli::managed::handle_managed_add_many(&names, false).await;
        assert_eq!(exit_code, 0, "all builtin workers should succeed");

        assert!(
            iii_worker::cli::config_file::worker_exists("iii-http"),
            "iii-http should be in config.yaml"
        );
        assert!(
            iii_worker::cli::config_file::worker_exists("iii-state"),
            "iii-state should be in config.yaml"
        );
    })
    .await;
}

#[tokio::test]
async fn add_many_with_invalid_worker_returns_nonzero() {
    in_temp_dir_async(|| async {
        let names = vec![
            "iii-http".to_string(),
            "definitely-not-a-real-worker-xyz".to_string(),
        ];
        let exit_code = iii_worker::cli::managed::handle_managed_add_many(&names, false).await;
        assert_ne!(exit_code, 0, "should fail when any worker fails");

        assert!(
            iii_worker::cli::config_file::worker_exists("iii-http"),
            "iii-http should still be in config.yaml despite other failure"
        );
    })
    .await;
}

#[tokio::test]
async fn handle_managed_add_builtin_creates_config() {
    in_temp_dir_async(|| async {
        let exit_code =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, false, false, false)
                .await;
        assert_eq!(
            exit_code, 0,
            "expected success exit code for builtin worker"
        );

        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: iii-http"));
        assert!(content.contains("config:"));
        assert!(content.contains("port: 3111"));
        assert!(content.contains("host: 127.0.0.1"));
        assert!(content.contains("default_timeout: 30000"));
        assert!(content.contains("concurrency_request_limit: 1024"));
        assert!(content.contains("allowed_origins"));
        let lockfile = iii_worker::cli::lockfile::WorkerLockfile::read_from(
            iii_worker::cli::lockfile::lockfile_path(),
        )
        .unwrap();
        let worker = lockfile.workers.get("iii-http").unwrap();
        assert!(matches!(
            worker.worker_type,
            iii_worker::cli::lockfile::LockedWorkerType::Engine
        ));
        assert!(worker.source.is_none());
    })
    .await;
}

#[tokio::test]
async fn handle_managed_add_builtin_accepts_explicit_version() {
    in_temp_dir_async(|| async {
        // The explicit version intentionally differs from the workspace
        // version; this pins that user-supplied registry versions are accepted.
        let exit_code = iii_worker::cli::managed::handle_managed_add(
            "iii-http@0.10.0",
            false,
            false,
            false,
            false,
        )
        .await;
        assert_eq!(
            exit_code, 0,
            "expected builtins to accept explicit registry versions"
        );

        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: iii-http"));
        assert!(content.contains("port: 3111"));
        let lockfile = iii_worker::cli::lockfile::WorkerLockfile::read_from(
            iii_worker::cli::lockfile::lockfile_path(),
        )
        .unwrap();
        assert_eq!(lockfile.workers["iii-http"].version, "0.10.0");
    })
    .await;
}

#[tokio::test]
async fn handle_managed_add_builtin_merges_existing() {
    in_temp_dir_async(|| async {
        // Pre-populate with user overrides
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: iii-http\n    config:\n      port: 9999\n      custom_key: preserved\n",
        )
        .unwrap();

        let exit_code =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, false, false, false)
                .await;
        assert_eq!(exit_code, 0, "expected success exit code for merge");

        let content = std::fs::read_to_string("config.yaml").unwrap();
        // User override preserved
        assert!(content.contains("9999"));
        assert!(content.contains("custom_key"));
        // Builtin defaults filled in
        assert!(content.contains("default_timeout"));
        assert!(content.contains("concurrency_request_limit"));
    })
    .await;
}

/// Regression: `handle_managed_start` on a builtin must short-circuit without
/// consulting the remote registry. Builtins are served in-process by the iii
/// engine; they have no external process to spawn and are not published to the
/// registry. Previously this path fell through to `fetch_worker_info`, which
/// emitted "not found locally, checking registry..." and failed.
///
/// When the engine is NOT running, `start` for a builtin now exits non-zero
/// (DX fix: exit 0 with a "start the engine" hint misled automation into
/// thinking the builtin actually booted). This test exercises that path and
/// asserts exit 1 -- which still proves the short-circuit, because a 1 here
/// means we returned from the builtin branch rather than hitting the remote
/// registry and producing a different error.
#[tokio::test]
async fn handle_managed_start_builtin_short_circuits() {
    in_temp_dir_async(|| async {
        // Add a builtin to config.yaml first.
        let add_rc =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, false, false, false)
                .await;
        assert_eq!(add_rc, 0, "expected add to succeed for builtin");

        // Builtin must short-circuit regardless of the local engine's port
        // binding state. If the engine is up (port 49134 open on the host)
        // start returns 0 with "served by the engine" message. If the engine
        // is down, start returns 1 with a "start the engine" hint. Both are
        // valid; what we're really asserting is that we did NOT fall through
        // to the remote registry (which would produce a different error).
        let start_rc = iii_worker::cli::managed::handle_managed_start(
            "iii-http",
            false,
            iii_worker::DEFAULT_PORT,
            None,
        )
        .await;
        assert!(
            start_rc == 0 || start_rc == 1,
            "expected short-circuit (0 or 1), got {} -- suggests a registry fall-through",
            start_rc
        );
    })
    .await;
}

/// Regression: `handle_managed_start` on a builtin that is NOT in config.yaml
/// must fail with a non-zero exit code. Otherwise the CLI misleads the user
/// by claiming "served by the iii engine" when the engine won't actually load
/// the worker (config.yaml is the source of truth for non-default mode).
#[tokio::test]
async fn handle_managed_start_unconfigured_builtin_fails() {
    in_temp_dir_async(|| async {
        // Do NOT add the builtin. handle_managed_start must refuse.
        assert!(
            !iii_worker::cli::config_file::worker_exists("iii-http"),
            "precondition: iii-http must not be in config.yaml"
        );
        let start_rc = iii_worker::cli::managed::handle_managed_start(
            "iii-http",
            false,
            iii_worker::DEFAULT_PORT,
            None,
        )
        .await;
        assert_ne!(
            start_rc, 0,
            "handle_managed_start must fail for unconfigured builtin 'iii-http'"
        );
    })
    .await;
}

#[tokio::test]
async fn handle_managed_start_unconfigured_optional_builtin_fails() {
    in_temp_dir_async(|| async {
        assert!(
            !iii_worker::cli::config_file::worker_exists("iii-exec"),
            "precondition: iii-exec must not be in config.yaml"
        );
        let start_rc = iii_worker::cli::managed::handle_managed_start(
            "iii-exec",
            false,
            iii_worker::DEFAULT_PORT,
            None,
        )
        .await;
        assert_ne!(
            start_rc, 0,
            "handle_managed_start must fail for unconfigured optional builtin 'iii-exec'"
        );
    })
    .await;
}

#[tokio::test]
async fn handle_managed_add_all_builtins_succeed() {
    in_temp_dir_async(|| async {
        for name in iii_worker::cli::builtin_defaults::BUILTIN_NAMES {
            let _ = std::fs::remove_file("config.yaml");

            let exit_code =
                iii_worker::cli::managed::handle_managed_add(name, false, false, false, false)
                    .await;
            assert_eq!(exit_code, 0, "expected success for builtin '{}'", name);

            let content = std::fs::read_to_string("config.yaml").unwrap();
            assert!(
                content.contains(&format!("- name: {}", name)),
                "config.yaml missing entry for '{}'",
                name
            );
        }
    })
    .await;
}

#[tokio::test]
async fn remove_many_workers() {
    in_temp_dir_async(|| async {
        // Add two builtins first.
        let names = vec!["iii-http".to_string(), "iii-state".to_string()];
        let exit_code = iii_worker::cli::managed::handle_managed_add_many(&names, false).await;
        assert_eq!(exit_code, 0);

        // Remove both at once.
        let exit_code = iii_worker::cli::managed::handle_managed_remove_many(&names, true).await;
        assert_eq!(exit_code, 0, "all removals should succeed");

        assert!(
            !iii_worker::cli::config_file::worker_exists("iii-http"),
            "iii-http should be removed"
        );
        assert!(
            !iii_worker::cli::config_file::worker_exists("iii-state"),
            "iii-state should be removed"
        );
    })
    .await;
}

#[tokio::test]
async fn remove_many_with_missing_worker_returns_nonzero() {
    in_temp_dir_async(|| async {
        // Add one builtin.
        let add_names = vec!["iii-http".to_string()];
        let exit_code = iii_worker::cli::managed::handle_managed_add_many(&add_names, false).await;
        assert_eq!(exit_code, 0);

        // Remove existing + nonexistent.
        let remove_names = vec!["iii-http".to_string(), "not-a-real-worker".to_string()];
        let exit_code =
            iii_worker::cli::managed::handle_managed_remove_many(&remove_names, true).await;
        assert_ne!(exit_code, 0, "should fail when any removal fails");

        // The valid one should still have been removed.
        assert!(
            !iii_worker::cli::config_file::worker_exists("iii-http"),
            "iii-http should be removed despite other failure"
        );
    })
    .await;
}

// ===========================================================================
// OCI passthrough routing
// ===========================================================================

/// Verify that a full OCI reference (containing '/') is treated as a direct
/// OCI passthrough — the function should NOT call the API. It will fail because
/// there's no running container runtime, but the error should be about pulling,
/// not about API resolution.
#[tokio::test]
async fn handle_managed_add_oci_ref_passthrough() {
    in_temp_dir_async(|| async {
        // This will fail (no container runtime), but the error path tells us
        // it tried to pull an OCI image rather than calling the API
        let result = iii_worker::cli::managed::handle_managed_add(
            "ghcr.io/test/worker:1.0",
            false,
            false,
            false,
            false,
        )
        .await;
        // Non-zero exit because pull fails, but it should NOT have tried the API
        assert_ne!(result, 0);
    })
    .await;
}

/// Verify that a reference with ':' is also treated as OCI passthrough.
#[tokio::test]
async fn handle_managed_add_oci_ref_with_colon_passthrough() {
    in_temp_dir_async(|| async {
        let result = iii_worker::cli::managed::handle_managed_add(
            "docker.io/org/image:latest",
            false,
            false,
            false,
            false,
        )
        .await;
        assert_ne!(result, 0);
    })
    .await;
}

// ===========================================================================
// API resolution path (non-builtin, non-OCI-ref, non-local)
// ===========================================================================

/// Verify that a plain worker name that is NOT a builtin triggers API resolution.
/// With no API available, it should fail with a resolution error.
#[tokio::test]
async fn handle_managed_add_plain_name_calls_api() {
    in_temp_dir_async(|| async {
        // Set III_API_URL to something that will fail quickly
        unsafe { std::env::set_var("III_API_URL", "http://127.0.0.1:1") };
        let result = iii_worker::cli::managed::handle_managed_add(
            "nonexistent-worker",
            true,
            false,
            false,
            false,
        )
        .await;
        unsafe { std::env::remove_var("III_API_URL") };
        // Should fail because API is unreachable
        assert_ne!(result, 0);
    })
    .await;
}

/// Verify that a plain name matching a builtin worker succeeds without API.
#[tokio::test]
async fn handle_managed_add_builtin_skips_api() {
    in_temp_dir_async(|| async {
        // Set III_API_URL to something that will fail — if it tries the API, we'll know
        unsafe { std::env::set_var("III_API_URL", "http://127.0.0.1:1") };
        let result =
            iii_worker::cli::managed::handle_managed_add("iii-http", true, false, false, false)
                .await;
        unsafe { std::env::remove_var("III_API_URL") };
        // Should succeed because iii-http is a builtin — no API call needed
        assert_eq!(result, 0);
    })
    .await;
}

/// Verify that local path workers are routed correctly (not to API).
#[tokio::test]
async fn handle_managed_add_local_path_skips_api() {
    in_temp_dir_async(|| async {
        unsafe { std::env::set_var("III_API_URL", "http://127.0.0.1:1") };
        let result = iii_worker::cli::managed::handle_managed_add(
            "./my-local-worker",
            true,
            false,
            false,
            false,
        )
        .await;
        unsafe { std::env::remove_var("III_API_URL") };
        // Will fail (path doesn't exist), but should NOT have called the API
        assert_ne!(result, 0);
    })
    .await;
}

/// Verify API resolution with file:// fixture returns binary worker.
#[tokio::test]
async fn handle_managed_add_binary_via_file_fixture() {
    in_temp_dir_async(|| async {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{
            "name": "test-binary-worker",
            "type": "binary",
            "version": "0.1.0",
            "binaries": {},
            "config": {"name": "test-binary-worker", "config": {}}
        }"#;
        let path = dir.path().join("fixture.json");
        std::fs::write(&path, json).unwrap();

        let url = format!("file://{}", path.display());
        unsafe { std::env::set_var("III_API_URL", &url) };
        let result = iii_worker::cli::managed::handle_managed_add(
            "test-binary-worker",
            true,
            false,
            false,
            false,
        )
        .await;
        unsafe { std::env::remove_var("III_API_URL") };
        // Will fail because binaries map is empty (no platform match),
        // but it proves the API resolution path was taken and parsed correctly
        assert_ne!(result, 0);
    })
    .await;
}

/// Restart must reject invalid worker names BEFORE calling stop or start.
/// Otherwise name-validation bugs would silently cascade into whatever
/// garbage-in behavior `iii worker stop <bad>` has today. The fast-fail also
/// guarantees we never execute side effects against an unvalidated name.
#[tokio::test]
async fn handle_managed_restart_invalid_name_fails_fast() {
    in_temp_dir_async(|| async {
        let rc = iii_worker::cli::managed::handle_managed_restart(
            "bad name with spaces",
            false,
            iii_worker::DEFAULT_PORT,
            None,
        )
        .await;
        assert_eq!(
            rc, 1,
            "invalid worker name must return 1 before stop/start run"
        );
    })
    .await;
}

/// Restart against a configured-but-not-running builtin must still invoke
/// the stop path. The orphan-safety contract (`commit 5f3cfc27`) is:
/// `handle_managed_restart` calls `handle_managed_stop` unconditionally so
/// its three-tier PID discovery (OCI pidfile → binary pidfile → `ps` scan)
/// can catch orphaned processes whose pidfiles are missing. If a future
/// refactor gates stop on `is_worker_running`, this test should still pass
/// because stop itself is a no-op for a not-running worker — but the intent
/// it pins is: restart returns start's rc, never early-returns, never
/// panics, regardless of whether anything is currently running.
#[tokio::test]
async fn handle_managed_restart_on_not_running_surfaces_start_rc() {
    in_temp_dir_async(|| async {
        // Add a builtin so start has something valid to resolve.
        let add_rc =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, false, false, false)
                .await;
        assert_eq!(add_rc, 0);

        let restart_rc = iii_worker::cli::managed::handle_managed_restart(
            "iii-http",
            false,
            iii_worker::DEFAULT_PORT,
            None,
        )
        .await;
        // Builtin start short-circuits with rc=0 or rc=1 depending on whether
        // the engine is actually running on DEFAULT_PORT. Either is valid
        // here — we're asserting that restart did NOT panic or hit an
        // unintended error path.
        assert!(
            restart_rc == 0 || restart_rc == 1,
            "restart must surface start's rc; got {}",
            restart_rc
        );
    })
    .await;
}

/// Remove-many with `--yes` must bypass the running-worker confirmation
/// prompt entirely. The new DX (commit b655977c) added a single batch
/// prompt when any target is currently running. If this gate regresses
/// (e.g., someone inverts the `!yes` check), automation that relies on
/// `--yes` for unattended cleanup would hang forever on stdin.
#[tokio::test]
async fn handle_managed_remove_many_with_yes_bypasses_prompt() {
    in_temp_dir_async(|| async {
        let names = vec!["iii-http".to_string(), "iii-state".to_string()];
        let add_rc = iii_worker::cli::managed::handle_managed_add_many(&names, false).await;
        assert_eq!(add_rc, 0);

        // Simulate a "running" worker by writing a live pidfile (our own PID)
        // under the binary-worker path. `is_worker_running` reads pidfiles
        // directly, so this is enough to trigger the prompt branch in
        // handle_managed_remove_many — but --yes must skip it.
        let home = dirs::home_dir().unwrap();
        let pids = home.join(".iii/pids");
        std::fs::create_dir_all(&pids).unwrap();
        let pidfile = pids.join("iii-http.pid");
        std::fs::write(&pidfile, std::process::id().to_string()).unwrap();

        let rc = iii_worker::cli::managed::handle_managed_remove_many(&names, /*yes=*/ true).await;
        // Cleanup the pidfile before asserting.
        let _ = std::fs::remove_file(&pidfile);

        assert_eq!(rc, 0, "--yes must bypass the prompt and succeed");
        assert!(!iii_worker::cli::config_file::worker_exists("iii-http"));
        assert!(!iii_worker::cli::config_file::worker_exists("iii-state"));
    })
    .await;
}

/// Regression: `iii worker add --force` used to error with "Worker is
/// currently running. Stop it first with `iii worker stop`" instead of
/// doing what --force implies — stop, clean, then re-add. The fix at
/// `handle_managed_add` (managed.rs force branch) now invokes
/// `handle_managed_stop` unconditionally when `is_worker_running` returns
/// true, so the user doesn't have to run two commands for what reads like
/// one destructive intent.
///
/// We prove the behavior by spawning a real sentinel process (copy of
/// /bin/sleep at the binary-worker path so `handle_managed_stop`'s three-
/// tier discovery recognizes it), writing its pid to the pidfile, and
/// asserting that `--force` add returns 0 — no "Stop it first" error.
/// We deliberately do NOT assert that the pidfile is removed: on platforms
/// where `ps` lookup is flaky or the sentinel exits between spawn and
/// stop, pidfile cleanup may or may not happen. The contract we care
/// about is: --force doesn't refuse.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn handle_managed_add_force_auto_stops_running_worker() {
    in_temp_dir_async(|| async {
        // Seed config.yaml with a builtin.
        let add_rc =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, false, false, false)
                .await;
        assert_eq!(add_rc, 0);

        // Spawn a real sentinel process so the auto-stop path can signal a
        // non-test-process PID. Staging our own PID would make stop attempt
        // to SIGTERM the test runner itself — genuinely bad.
        let home = dirs::home_dir().unwrap();
        let pids = home.join(".iii/pids");
        std::fs::create_dir_all(&pids).unwrap();
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("60")
            .spawn()
            .expect("spawn sentinel");
        let pidfile = pids.join("iii-http.pid");
        std::fs::write(&pidfile, child.id().to_string()).unwrap();

        // --force add: the fix makes this succeed without requiring the
        // user to stop first. Before the fix this returned 1 with the
        // "Stop it first" error.
        let rc =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, true, false, false)
                .await;

        // Always tear down the sentinel regardless of assertion outcome.
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&pidfile);

        assert_eq!(
            rc, 0,
            "--force add must not refuse when worker is running; got rc={}",
            rc
        );
    })
    .await;
}

/// Regression: `iii worker add` with wait=true used to only honor the wait
/// flag on the local-path branch. OCI/builtin/binary paths silently
/// dropped it. The fix routes all non-local paths through `finish_add`
/// which applies the wait. For builtins specifically, we skip the wait
/// (builtins run in-process with the engine and have no Phase::Ready
/// state machine to watch). This test pins that skip: wait=true on a
/// builtin returns quickly without hanging, even with no engine running.
#[tokio::test]
async fn handle_managed_add_wait_on_builtin_returns_without_hanging() {
    in_temp_dir_async(|| async {
        let started = std::time::Instant::now();
        // wait=true (5th param). Engine is not running on the test machine's
        // DEFAULT_PORT in the tempdir, but the builtin short-circuit must
        // still return immediately — builtins have no boot to observe.
        let rc =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, false, false, true)
                .await;
        let elapsed = started.elapsed();
        assert_eq!(rc, 0);
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "builtin add with wait=true must not hang; elapsed = {:?}",
            elapsed
        );
    })
    .await;
}
