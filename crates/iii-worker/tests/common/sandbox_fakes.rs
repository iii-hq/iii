// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.

//! Shared fakes for the sandbox::* trigger handlers.
//!
//! Three trait impls used by `sandbox_lifecycle_integration.rs` and
//! `sandbox_workflow_integration.rs`:
//!
//! - `FakeShellRunner` for `ShellRunner` (sandbox::exec)
//! - `FakeVmStopper` for `VmStopper` (sandbox::stop)
//! - `FakeVmLauncher` for `VmLauncher` (sandbox::create)
//!
//! Each fake supports a configurable response, a configurable error mode,
//! and a `block_until` hook (oneshot receiver) so concurrency tests can hold
//! one call mid-flight while another races. The block hook is what makes
//! "registry must not hold its mutex across an adapter call" testable.
//!
//! Per-call recording uses `parking_lot`-style sync mutex (std::sync::Mutex)
//! because the recorders are touched only at call boundaries — the async
//! adapter await happens on the channel/oneshot, not inside the lock.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use tokio::sync::oneshot;

use iii_worker::sandbox_daemon::create::{BootHandle, BootParams, VmLauncher};
use iii_worker::sandbox_daemon::errors::SandboxError;
use iii_worker::sandbox_daemon::exec::{ExecRequest, ExecResponse, ShellRunner};
use iii_worker::sandbox_daemon::stop::VmStopper;

// ────────────────────────────────────────────────────────────────────
// FakeShellRunner — for sandbox::exec
// ────────────────────────────────────────────────────────────────────

/// What the fake should return on the next `run()` call. Set to
/// `Response` for a happy answer, `Error` to inject a typed error, or
/// `Block` to hold the call open until `release` fires.
pub enum ShellMode {
    Response(ExecResponse),
    Error(SandboxError),
    Block {
        release: oneshot::Receiver<ShellMode>,
    },
}

#[derive(Default)]
pub struct ShellCallLog {
    pub calls: Vec<(PathBuf, ExecRequest)>,
}

pub struct FakeShellRunner {
    mode: Mutex<Option<ShellMode>>,
    pub calls: Mutex<ShellCallLog>,
    pub call_count: AtomicUsize,
}

impl FakeShellRunner {
    pub fn ok(stdout: impl Into<String>, exit_code: i32) -> Self {
        Self {
            mode: Mutex::new(Some(ShellMode::Response(ExecResponse {
                stdout: stdout.into(),
                stderr: String::new(),
                exit_code: Some(exit_code),
                timed_out: false,
                duration_ms: 1,
                success: exit_code == 0,
            }))),
            calls: Mutex::new(ShellCallLog::default()),
            call_count: AtomicUsize::new(0),
        }
    }

    pub fn err(err: SandboxError) -> Self {
        Self {
            mode: Mutex::new(Some(ShellMode::Error(err))),
            calls: Mutex::new(ShellCallLog::default()),
            call_count: AtomicUsize::new(0),
        }
    }

    /// Build a runner that holds the next `run()` call open until the
    /// returned sender resolves with the actual mode to use. Lets tests
    /// pin a concurrent exec mid-flight and observe contention.
    pub fn blocking() -> (Self, oneshot::Sender<ShellMode>) {
        let (tx, rx) = oneshot::channel();
        let runner = Self {
            mode: Mutex::new(Some(ShellMode::Block { release: rx })),
            calls: Mutex::new(ShellCallLog::default()),
            call_count: AtomicUsize::new(0),
        };
        (runner, tx)
    }

    pub fn set_mode(&self, mode: ShellMode) {
        *self.mode.lock().unwrap() = Some(mode);
    }
}

#[async_trait]
impl ShellRunner for FakeShellRunner {
    async fn run(
        &self,
        shell_sock: PathBuf,
        req: &ExecRequest,
    ) -> Result<ExecResponse, SandboxError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.calls
            .lock()
            .unwrap()
            .calls
            .push((shell_sock, clone_exec_req(req)));

        let mode = self.mode.lock().unwrap().take().unwrap_or_else(|| {
            // Panic instead of returning a bogus SandboxError. A test that
            // calls run() more times than the fake was configured for is a
            // bug in the test, not a behaviour to assert on. A loud panic
            // surfaces it; a silent FsIo error would let the test pass while
            // asserting on the wrong response.
            panic!(
                "FakeShellRunner exhausted: run() called more times than \
                 set_mode()/ok()/err()/blocking() configured. Call \
                 set_mode() before each subsequent call."
            )
        });

        match mode {
            ShellMode::Response(r) => Ok(r),
            ShellMode::Error(e) => Err(e),
            ShellMode::Block { release } => match release.await {
                Ok(ShellMode::Response(r)) => Ok(r),
                Ok(ShellMode::Error(e)) => Err(e),
                Ok(ShellMode::Block { .. }) => Err(SandboxError::FsIo(
                    "FakeShellRunner: nested Block mode is not supported".into(),
                )),
                Err(_) => Err(SandboxError::FsChannelAborted(
                    "FakeShellRunner: release channel dropped before resolution".into(),
                )),
            },
        }
    }
}

fn clone_exec_req(req: &ExecRequest) -> ExecRequest {
    ExecRequest {
        sandbox_id: req.sandbox_id.clone(),
        cmd: req.cmd.clone(),
        args: req.args.clone(),
        stdin: req.stdin.clone(),
        env: req.env.clone(),
        timeout_ms: req.timeout_ms,
        workdir: req.workdir.clone(),
    }
}

// ────────────────────────────────────────────────────────────────────
// FakeVmStopper — for sandbox::stop
// ────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct StopperLog {
    pub pids: Vec<u32>,
}

pub struct FakeVmStopper {
    fail_with: Mutex<Option<SandboxError>>,
    pub log: Mutex<StopperLog>,
    pub call_count: AtomicUsize,
}

impl FakeVmStopper {
    pub fn ok() -> Self {
        Self {
            fail_with: Mutex::new(None),
            log: Mutex::new(StopperLog::default()),
            call_count: AtomicUsize::new(0),
        }
    }

    pub fn err(err: SandboxError) -> Self {
        Self {
            fail_with: Mutex::new(Some(err)),
            log: Mutex::new(StopperLog::default()),
            call_count: AtomicUsize::new(0),
        }
    }

    pub fn called_with(&self) -> Vec<u32> {
        self.log.lock().unwrap().pids.clone()
    }
}

#[async_trait]
impl VmStopper for FakeVmStopper {
    async fn stop(&self, vm_pid: u32) -> Result<(), SandboxError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.log.lock().unwrap().pids.push(vm_pid);
        if let Some(err) = self.fail_with.lock().unwrap().take() {
            return Err(err);
        }
        Ok(())
    }
}

// ────────────────────────────────────────────────────────────────────
// FakeVmLauncher — for sandbox::create
// ────────────────────────────────────────────────────────────────────

pub enum LaunchMode {
    Ok {
        vm_pid: u32,
    },
    Err(SandboxError),
    Block {
        release: oneshot::Receiver<LaunchMode>,
    },
}

#[derive(Default)]
pub struct LauncherLog {
    pub boots: Vec<LauncherBootRecord>,
}

#[derive(Debug, Clone)]
pub struct LauncherBootRecord {
    pub rootfs: PathBuf,
    pub workdir: PathBuf,
    pub shell_sock: PathBuf,
    pub cpus: u32,
    pub memory_mb: u32,
    pub env: Vec<String>,
    pub network: bool,
}

pub struct FakeVmLauncher {
    mode: Mutex<Option<LaunchMode>>,
    pub log: Mutex<LauncherLog>,
    pub call_count: AtomicUsize,
}

impl FakeVmLauncher {
    pub fn ok(vm_pid: u32) -> Self {
        Self {
            mode: Mutex::new(Some(LaunchMode::Ok { vm_pid })),
            log: Mutex::new(LauncherLog::default()),
            call_count: AtomicUsize::new(0),
        }
    }

    pub fn err(err: SandboxError) -> Self {
        Self {
            mode: Mutex::new(Some(LaunchMode::Err(err))),
            log: Mutex::new(LauncherLog::default()),
            call_count: AtomicUsize::new(0),
        }
    }

    pub fn boots(&self) -> Vec<LauncherBootRecord> {
        self.log.lock().unwrap().boots.clone()
    }
}

#[async_trait]
impl VmLauncher for FakeVmLauncher {
    async fn boot(&self, params: &BootParams) -> Result<BootHandle, SandboxError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.log.lock().unwrap().boots.push(LauncherBootRecord {
            rootfs: params.rootfs.clone(),
            workdir: params.workdir.clone(),
            shell_sock: params.shell_sock.clone(),
            cpus: params.cpus,
            memory_mb: params.memory_mb,
            env: params.env.clone(),
            network: params.network,
        });
        let mode = self.mode.lock().unwrap().take().unwrap_or_else(|| {
            panic!(
                "FakeVmLauncher exhausted: boot() called more times than \
                 the fake was configured for. Construct a fresh FakeVmLauncher \
                 per call, or extend it to support a queue of modes."
            )
        });
        match mode {
            LaunchMode::Ok { vm_pid } => Ok(BootHandle { vm_pid }),
            LaunchMode::Err(e) => Err(e),
            LaunchMode::Block { release } => match release.await {
                Ok(LaunchMode::Ok { vm_pid }) => Ok(BootHandle { vm_pid }),
                Ok(LaunchMode::Err(e)) => Err(e),
                Ok(LaunchMode::Block { .. }) => Err(SandboxError::BootFailed(
                    "FakeVmLauncher: nested Block is not supported".into(),
                )),
                Err(_) => Err(SandboxError::BootFailed(
                    "FakeVmLauncher: release channel dropped".into(),
                )),
            },
        }
    }
}
