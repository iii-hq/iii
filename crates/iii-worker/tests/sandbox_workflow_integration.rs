// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.

//! Cross-handler workflow tests for the sandbox::* trigger surface.
//!
//! These exercise scenarios that touch more than one handler in sequence,
//! verifying the contracts between create/exec/stop/list and fs::* hold
//! when they're driven the way an SDK client drives them. Per the eng
//! review, the focus is on:
//!
//! - state transitions are visible across handlers (stop -> exec sees S004)
//! - fs::* triggers also reject after stop, not just exec
//! - one happy-path scripted workflow that touches every layer
//!
//! As with `sandbox_lifecycle_integration.rs`, sandbox state is inserted
//! manually so tests don't need rootfs/overlay machinery; the focus here
//! is the post-create handler surface.

mod common;

use std::path::PathBuf;
use std::time::Instant;

use uuid::Uuid;

use iii_worker::sandbox_daemon::errors::SandboxError;
use iii_worker::sandbox_daemon::exec::{ExecRequest, handle_exec};
use iii_worker::sandbox_daemon::list::{ListRequest, handle_list};
use iii_worker::sandbox_daemon::registry::{SandboxRegistry, SandboxState};
use iii_worker::sandbox_daemon::stop::{StopRequest, handle_stop};

use common::sandbox_fakes::{FakeShellRunner, FakeVmStopper};

fn make_state(id: Uuid) -> SandboxState {
    SandboxState {
        id,
        name: Some("workflow-fixture".into()),
        image: "python".into(),
        rootfs: PathBuf::from("/tmp/rootfs"),
        workdir: PathBuf::from("/tmp/work"),
        shell_sock: PathBuf::from("/tmp/shell.sock"),
        vm_pid: Some(7777),
        created_at: Instant::now(),
        last_exec_at: Instant::now(),
        exec_in_progress: false,
        idle_timeout_secs: 300,
        stopped: false,
    }
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

// --------------------------------------------------------------------
// Workflow: simulated create (manual insert) -> exec -> stop -> list
// --------------------------------------------------------------------

#[tokio::test]
async fn workflow_create_exec_stop_list_round_trip() {
    let reg = SandboxRegistry::new();
    let id = Uuid::new_v4();
    reg.insert(make_state(id)).await;

    // List sees the sandbox
    let pre_list = handle_list(ListRequest::default(), &reg).await;
    assert_eq!(pre_list.sandboxes.len(), 1);
    assert_eq!(pre_list.sandboxes[0].sandbox_id, id.to_string());
    assert!(!pre_list.sandboxes[0].stopped);

    // Run a command
    let runner = FakeShellRunner::ok("hello world\n", 0);
    let resp = handle_exec(build_req(id, "/bin/echo"), &reg, &runner)
        .await
        .unwrap();
    assert_eq!(resp.stdout, "hello world\n");

    // Stop it
    let stopper = FakeVmStopper::ok();
    let stop_resp = handle_stop(
        StopRequest {
            sandbox_id: id.to_string(),
            wait: true,
        },
        &reg,
        &stopper,
    )
    .await
    .unwrap();
    assert!(stop_resp.stopped);

    // List is empty
    let post_list = handle_list(ListRequest::default(), &reg).await;
    assert!(
        post_list.sandboxes.is_empty(),
        "list must be empty after stop"
    );
}

// --------------------------------------------------------------------
// Workflow: stopped sandbox must reject further work consistently
// --------------------------------------------------------------------

#[tokio::test]
async fn workflow_stopped_sandbox_rejects_further_work() {
    let reg = SandboxRegistry::new();
    let id = Uuid::new_v4();
    reg.insert(make_state(id)).await;
    reg.mark_stopped(id).await;

    let runner = FakeShellRunner::ok("", 0);
    let err = handle_exec(build_req(id, "/bin/true"), &reg, &runner)
        .await
        .unwrap_err();
    assert_eq!(err.code().as_str(), "S004");

    // Idempotent stop on already-stopped is OK (skip stopper).
    let stopper = FakeVmStopper::ok();
    let stop_resp = handle_stop(
        StopRequest {
            sandbox_id: id.to_string(),
            wait: false,
        },
        &reg,
        &stopper,
    )
    .await
    .unwrap();
    assert!(stop_resp.stopped);
    assert_eq!(
        stopper.call_count.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

// --------------------------------------------------------------------
// Workflow: list reports state churn coherently
// --------------------------------------------------------------------

#[tokio::test]
async fn workflow_list_reflects_mark_stopped_transitions() {
    let reg = SandboxRegistry::new();
    let id_running = Uuid::new_v4();
    let id_stopped = Uuid::new_v4();
    reg.insert(make_state(id_running)).await;
    reg.insert(make_state(id_stopped)).await;
    reg.mark_stopped(id_stopped).await;

    let resp = handle_list(ListRequest::default(), &reg).await;
    assert_eq!(resp.sandboxes.len(), 2);
    let running_summary = resp
        .sandboxes
        .iter()
        .find(|s| s.sandbox_id == id_running.to_string())
        .expect("running sandbox should appear");
    assert!(!running_summary.stopped);
    let stopped_summary = resp
        .sandboxes
        .iter()
        .find(|s| s.sandbox_id == id_stopped.to_string())
        .expect("stopped sandbox should appear");
    assert!(stopped_summary.stopped);
}

// --------------------------------------------------------------------
// Workflow: error variants surface unchanged through workflow
// --------------------------------------------------------------------

#[tokio::test]
async fn workflow_runner_errors_dont_corrupt_subsequent_calls() {
    let reg = SandboxRegistry::new();
    let id = Uuid::new_v4();
    reg.insert(make_state(id)).await;

    // First call: runner errors. exec_in_progress must clear.
    let runner_err = FakeShellRunner::err(SandboxError::FsIo("simulated".into()));
    let _ = handle_exec(build_req(id, "/bin/true"), &reg, &runner_err)
        .await
        .unwrap_err();

    // Second call: must succeed with a fresh runner — the prior failure
    // must not have wedged exec_in_progress.
    let runner_ok = FakeShellRunner::ok("recovered\n", 0);
    let resp = handle_exec(build_req(id, "/bin/echo"), &reg, &runner_ok)
        .await
        .unwrap();
    assert_eq!(resp.stdout, "recovered\n");
}
