// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.

//! Integration tests for the lifecycle `sandbox::*` triggers
//! (create/exec/stop/list).
//!
//! Sister file to `sandbox_fs_integration.rs`. Drives the four lifecycle
//! handler functions directly, with `FakeShellRunner`, `FakeVmStopper`,
//! and `FakeVmLauncher` from `common::sandbox_fakes` standing in for the
//! real adapters. No libkrun, no shell socket, no III WebSocket — pure
//! handler-glue coverage that catches:
//!
//! - per-handler error code stability
//! - the registry mutex must not be held across an adapter `await`
//!   (otherwise concurrent triggers wedge each other)
//! - `exec_in_progress` is cleared on error paths, not just success
//! - state transitions: `stopped` rejects further exec with S004
//! - cross-handler invariants (concurrent trigger calls don't deadlock)
//!
//! Note on `handle_create`: the function calls `rootfs_cache::resolve_cached`
//! and `OverlayLayout::ensure_dirs` BEFORE `launcher.boot`, both of which
//! touch the host filesystem. That makes happy-path create coverage best
//! served by the tier-C real-VM tests in `vm_integration.rs`. This file
//! covers the validation-path errors only — early returns in `handle_create`
//! that happen before any filesystem work.

mod common;

use std::sync::Arc;
use std::time::Instant;

use uuid::Uuid;

use iii_worker::sandbox_daemon::config::SandboxConfig;
use iii_worker::sandbox_daemon::create::{CreateRequest, handle_create};
use iii_worker::sandbox_daemon::errors::SandboxError;
use iii_worker::sandbox_daemon::exec::{ExecRequest, ExecResponse, handle_exec};
use iii_worker::sandbox_daemon::list::{ListRequest, handle_list};
use iii_worker::sandbox_daemon::registry::{SandboxRegistry, SandboxState};
use iii_worker::sandbox_daemon::stop::{StopRequest, handle_stop};

use common::sandbox_fakes::{FakeShellRunner, FakeVmLauncher, FakeVmStopper, ShellMode};

// --------------------------------------------------------------------
// Test fixtures
// --------------------------------------------------------------------

fn make_state(id: Uuid) -> SandboxState {
    SandboxState {
        id,
        name: None,
        image: "python".into(),
        rootfs: std::path::PathBuf::from("/tmp/rootfs"),
        workdir: std::path::PathBuf::from("/tmp/work"),
        shell_sock: std::path::PathBuf::from("/tmp/shell.sock"),
        vm_pid: Some(4321),
        created_at: Instant::now(),
        last_exec_at: Instant::now(),
        exec_in_progress: false,
        idle_timeout_secs: 300,
        stopped: false,
    }
}

async fn live_sandbox(reg: &SandboxRegistry) -> Uuid {
    let id = Uuid::new_v4();
    reg.insert(make_state(id)).await;
    id
}

fn build_req(id: Uuid, cmd: impl Into<String>) -> ExecRequest {
    ExecRequest {
        sandbox_id: id.to_string(),
        cmd: cmd.into(),
        args: vec![],
        stdin: None,
        env: vec![],
        timeout_ms: None,
        workdir: None,
    }
}

fn ok_response(stdout: &str) -> ExecResponse {
    ExecResponse {
        stdout: stdout.into(),
        stderr: String::new(),
        exit_code: Some(0),
        timed_out: false,
        duration_ms: 1,
        success: true,
    }
}

// ====================================================================
// sandbox::create — validation-path coverage only (post-launcher paths
// require host filesystem setup; covered by tier-C vm_integration.rs).
// ====================================================================

#[tokio::test]
async fn create_resource_limit_returns_s400_before_allowlist() {
    let cfg = SandboxConfig {
        max_concurrent_sandboxes: 1,
        auto_install: false,
        image_allowlist: vec!["python".into()],
        ..Default::default()
    };
    let reg = SandboxRegistry::new();
    let _id = live_sandbox(&reg).await;
    let launcher = FakeVmLauncher::ok(1234);

    let err = handle_create(
        CreateRequest {
            image: "python".into(),
            cpus: None,
            memory_mb: None,
            name: None,
            network: None,
            idle_timeout_secs: None,
            env: vec![],
        },
        &cfg,
        &reg,
        &launcher,
        |_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(err.code().as_str(), "S400");
    assert_eq!(
        launcher
            .call_count
            .load(std::sync::atomic::Ordering::SeqCst),
        0,
        "launcher must not run when resource limit reached"
    );
}

#[tokio::test]
async fn create_image_not_in_catalog_skips_launcher() {
    let cfg = SandboxConfig {
        max_concurrent_sandboxes: 5,
        auto_install: false,
        image_allowlist: vec!["python".into()],
        ..Default::default()
    };
    let reg = SandboxRegistry::new();
    let launcher = FakeVmLauncher::ok(1234);

    let err = handle_create(
        CreateRequest {
            image: "definitely-not-allowed".into(),
            cpus: None,
            memory_mb: None,
            name: None,
            network: None,
            idle_timeout_secs: None,
            env: vec![],
        },
        &cfg,
        &reg,
        &launcher,
        |_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(err.code().as_str(), "S100");
    assert_eq!(
        launcher
            .call_count
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

// ====================================================================
// sandbox::exec
// ====================================================================

#[tokio::test]
async fn exec_happy_path_returns_response_and_clears_in_progress() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let runner = FakeShellRunner::ok("hi\n", 0);

    let resp = handle_exec(build_req(id, "/bin/echo"), &reg, &runner)
        .await
        .unwrap();
    assert_eq!(resp.stdout, "hi\n");
    assert_eq!(resp.exit_code, Some(0));
    assert!(resp.success);

    let state = reg.get(id).await.unwrap();
    assert!(!state.exec_in_progress, "in-progress flag must be cleared");
    assert_eq!(
        runner.call_count.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn exec_invalid_uuid_returns_s001() {
    let reg = SandboxRegistry::new();
    let runner = FakeShellRunner::ok("", 0);
    let mut req = build_req(Uuid::new_v4(), "/bin/true");
    req.sandbox_id = "not-a-uuid".into();

    let err = handle_exec(req, &reg, &runner).await.unwrap_err();
    assert_eq!(err.code().as_str(), "S001");
    assert_eq!(
        runner.call_count.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn exec_unknown_sandbox_returns_s002() {
    let reg = SandboxRegistry::new();
    let runner = FakeShellRunner::ok("", 0);
    let err = handle_exec(build_req(Uuid::new_v4(), "/bin/true"), &reg, &runner)
        .await
        .unwrap_err();
    assert_eq!(err.code().as_str(), "S002");
}

#[tokio::test]
async fn exec_on_stopped_sandbox_returns_s004() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    reg.mark_stopped(id).await;
    let runner = FakeShellRunner::ok("", 0);

    let err = handle_exec(build_req(id, "/bin/true"), &reg, &runner)
        .await
        .unwrap_err();
    assert_eq!(err.code().as_str(), "S004");
    assert_eq!(
        runner.call_count.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn exec_concurrent_on_same_sandbox_returns_s003() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let _claim = reg.begin_exec(id).await.unwrap();
    let runner = FakeShellRunner::ok("", 0);

    let err = handle_exec(build_req(id, "/bin/true"), &reg, &runner)
        .await
        .unwrap_err();
    assert_eq!(err.code().as_str(), "S003");
}

#[tokio::test]
async fn exec_runner_error_clears_in_progress_flag() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let runner = FakeShellRunner::err(SandboxError::BootFailed("vm wedged".into()));

    let err = handle_exec(build_req(id, "/bin/true"), &reg, &runner)
        .await
        .unwrap_err();
    assert!(matches!(err, SandboxError::BootFailed(_)));
    let state = reg.get(id).await.unwrap();
    assert!(
        !state.exec_in_progress,
        "exec_in_progress must clear even when runner returns Err"
    );
}

#[tokio::test]
async fn exec_nonzero_exit_surfaced_as_response_not_error() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let runner = FakeShellRunner::ok("oom\n", 137);

    let resp = handle_exec(build_req(id, "/bin/sleep"), &reg, &runner)
        .await
        .expect("non-zero exit is a normal response, not an Err");
    assert_eq!(resp.exit_code, Some(137));
    assert!(!resp.success);
}

#[tokio::test]
async fn exec_argv_passed_verbatim_no_shell_interpretation() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let runner = FakeShellRunner::ok("", 0);
    let nasty = vec!["; rm -rf /".into(), "$(whoami)".into(), "&& ls".into()];
    let req = ExecRequest {
        sandbox_id: id.to_string(),
        cmd: "/bin/echo".into(),
        args: nasty.clone(),
        stdin: None,
        env: vec![],
        timeout_ms: None,
        workdir: None,
    };

    let _ = handle_exec(req, &reg, &runner).await.unwrap();
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].1.cmd, "/bin/echo");
    assert_eq!(
        calls.calls[0].1.args, nasty,
        "argv must pass through unmodified"
    );
}

#[tokio::test]
async fn exec_does_not_hold_registry_mutex_across_adapter_await() {
    let reg = Arc::new(SandboxRegistry::new());
    let id = live_sandbox(&reg).await;
    let (runner, release) = FakeShellRunner::blocking();
    let runner = Arc::new(runner);

    let runner_for_task = runner.clone();
    let reg_for_task = reg.clone();
    let blocked_task = tokio::spawn(async move {
        handle_exec(
            build_req(id, "/bin/true"),
            &*reg_for_task,
            &*runner_for_task,
        )
        .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let list_resp = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        handle_list(ListRequest::default(), &reg),
    )
    .await
    .expect("handle_list must complete while runner is mid-call");
    assert_eq!(list_resp.sandboxes.len(), 1);

    release
        .send(ShellMode::Response(ok_response("released\n")))
        .map_err(|_| ())
        .expect("release channel must be open");
    let resp = blocked_task.await.unwrap().unwrap();
    assert_eq!(resp.stdout, "released\n");
}

#[tokio::test]
async fn exec_timeout_response_passes_through_unchanged() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let runner = FakeShellRunner::ok("", 0);
    runner.set_mode(ShellMode::Response(ExecResponse {
        stdout: String::new(),
        stderr: "killed\n".into(),
        exit_code: None,
        timed_out: true,
        duration_ms: 30_000,
        success: false,
    }));

    let resp = handle_exec(build_req(id, "/bin/sleep"), &reg, &runner)
        .await
        .unwrap();
    assert!(resp.timed_out);
    assert_eq!(resp.exit_code, None);
    assert!(!resp.success);
}

// ====================================================================
// sandbox::stop
// ====================================================================

#[tokio::test]
async fn stop_happy_path_calls_stopper_and_removes_entry() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let stopper = FakeVmStopper::ok();

    let resp = handle_stop(
        StopRequest {
            sandbox_id: id.to_string(),
            wait: true,
        },
        &reg,
        &stopper,
    )
    .await
    .unwrap();

    assert!(resp.stopped);
    assert_eq!(stopper.called_with(), vec![4321u32]);
    assert!(
        reg.get(id).await.is_err(),
        "registry entry must be removed after successful stop"
    );
}

#[tokio::test]
async fn stop_invalid_uuid_returns_s001() {
    let reg = SandboxRegistry::new();
    let stopper = FakeVmStopper::ok();
    let err = handle_stop(
        StopRequest {
            sandbox_id: "not-a-uuid".into(),
            wait: false,
        },
        &reg,
        &stopper,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code().as_str(), "S001");
    assert_eq!(
        stopper.call_count.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn stop_unknown_sandbox_returns_s002() {
    let reg = SandboxRegistry::new();
    let stopper = FakeVmStopper::ok();
    let err = handle_stop(
        StopRequest {
            sandbox_id: Uuid::new_v4().to_string(),
            wait: false,
        },
        &reg,
        &stopper,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code().as_str(), "S002");
    assert_eq!(
        stopper.call_count.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn stop_on_already_stopped_sandbox_is_idempotent_and_skips_stopper() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    reg.mark_stopped(id).await;
    let stopper = FakeVmStopper::ok();

    let resp = handle_stop(
        StopRequest {
            sandbox_id: id.to_string(),
            wait: false,
        },
        &reg,
        &stopper,
    )
    .await
    .unwrap();

    assert!(resp.stopped);
    assert_eq!(
        stopper.call_count.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "stopper must not be invoked for an already-stopped sandbox"
    );
}

#[tokio::test]
async fn stop_signal_error_preserves_state_for_retry() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let stopper = FakeVmStopper::err(SandboxError::BootFailed("transient".into()));

    let err = handle_stop(
        StopRequest {
            sandbox_id: id.to_string(),
            wait: true,
        },
        &reg,
        &stopper,
    )
    .await
    .unwrap_err();

    assert!(matches!(err, SandboxError::BootFailed(_)));
    let state = reg
        .get(id)
        .await
        .expect("registry entry must remain after a failed stop");
    assert!(
        !state.stopped,
        "stopped flag must NOT be set when stopper returned Err"
    );
}

// ====================================================================
// sandbox::list
// ====================================================================

#[tokio::test]
async fn list_empty_registry_returns_empty_array() {
    let reg = SandboxRegistry::new();
    let resp = handle_list(ListRequest::default(), &reg).await;
    assert!(resp.sandboxes.is_empty());
}

#[tokio::test]
async fn list_returns_summary_for_each_sandbox() {
    let reg = SandboxRegistry::new();
    let _ = live_sandbox(&reg).await;
    let _ = live_sandbox(&reg).await;
    let _ = live_sandbox(&reg).await;
    let resp = handle_list(ListRequest::default(), &reg).await;
    assert_eq!(resp.sandboxes.len(), 3);
    for s in &resp.sandboxes {
        assert_eq!(s.image, "python");
        assert!(!s.exec_in_progress);
        assert!(!s.stopped);
    }
}

#[tokio::test]
async fn list_during_concurrent_state_churn_never_panics() {
    let reg = Arc::new(SandboxRegistry::new());
    let mut ids = Vec::new();
    for _ in 0..10 {
        ids.push(live_sandbox(&reg).await);
    }

    let mut handles = Vec::new();
    for _ in 0..5 {
        let r = reg.clone();
        handles.push(tokio::spawn(async move {
            let id = Uuid::new_v4();
            r.insert(make_state(id)).await;
            r.mark_stopped(id).await;
        }));
    }
    for &id in &ids {
        let r = reg.clone();
        handles.push(tokio::spawn(async move {
            r.mark_stopped(id).await;
        }));
    }
    let r = reg.clone();
    handles.push(tokio::spawn(async move {
        for _ in 0..50 {
            let _ = handle_list(ListRequest::default(), &r).await;
            tokio::task::yield_now().await;
        }
    }));

    for h in handles {
        h.await.unwrap();
    }
    let final_resp = handle_list(ListRequest::default(), &reg).await;
    assert_eq!(final_resp.sandboxes.len(), 15);
}

// ====================================================================
// Cross-handler invariants — race scenarios
// ====================================================================

/// Stop in flight while exec races. The exact outcome (exec finishes
/// or sees stopped first) is order-dependent; the invariant is that
/// neither call deadlocks and the registry settles consistently.
#[tokio::test]
async fn stop_during_exec_does_not_deadlock_and_settles_consistently() {
    let reg = Arc::new(SandboxRegistry::new());
    let id = live_sandbox(&reg).await;

    let (runner, release) = FakeShellRunner::blocking();
    let runner = Arc::new(runner);
    let stopper = Arc::new(FakeVmStopper::ok());

    let r1 = reg.clone();
    let runner_t = runner.clone();
    let exec_task =
        tokio::spawn(
            async move { handle_exec(build_req(id, "/bin/sleep"), &*r1, &*runner_t).await },
        );
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let r2 = reg.clone();
    let stopper_t = stopper.clone();
    let stop_task = tokio::spawn(async move {
        handle_stop(
            StopRequest {
                sandbox_id: id.to_string(),
                wait: false,
            },
            &*r2,
            &*stopper_t,
        )
        .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let _ = release.send(ShellMode::Response(ok_response("done\n")));

    let _exec_outcome = tokio::time::timeout(std::time::Duration::from_secs(2), exec_task)
        .await
        .expect("exec task must not hang")
        .unwrap();
    let stop_outcome = tokio::time::timeout(std::time::Duration::from_secs(2), stop_task)
        .await
        .expect("stop task must not hang")
        .unwrap();

    let stop_resp = stop_outcome.expect("stop must succeed");
    assert!(stop_resp.stopped);
    assert!(reg.get(id).await.is_err());
}
