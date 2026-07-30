// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Windows supervision: one Job Object per child, teardown by job.
//!
//! A Job Object is the windows answer to a process group: terminating the job
//! terminates the worker and everything it spawned. The child also gets its own
//! console process group, so a graceful `CTRL_BREAK` can be delivered before the
//! job is terminated outright.
//!
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is deliberately **not** set. It would
//! kill every child the moment the daemon exits, and compose requires the
//! opposite: children survive a daemon crash and are re-adopted on restart
//! (see [`crate::state`]).

use std::{process::ExitStatus, time::Duration};

use tokio::sync::watch;
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT},
    System::{
        Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent},
        JobObjects::{AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject},
        Threading::{
            CREATE_NEW_PROCESS_GROUP, GetProcessTimes, OpenProcess,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        },
    },
};

use super::{BirthIdentity, Outcome, birth_identity};

/// Owns a win32 handle and closes it exactly once.
#[derive(Debug)]
struct OwnedHandle(HANDLE);

// A win32 handle is a process-wide token, not thread-affine: moving one between
// threads is what every supervisor does.
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// A running child plus everything needed to end it safely.
#[derive(Debug)]
pub struct Supervised {
    pub pid: u32,
    pub birth: BirthIdentity,
    job: OwnedHandle,
    exit: watch::Receiver<Option<ExitStatus>>,
}

/// Spawns `command` in its own job and console process group, and starts
/// reaping it.
pub fn spawn_supervised(mut command: tokio::process::Command) -> std::io::Result<Supervised> {
    // Its own console group: CTRL_BREAK can then be aimed at the child alone,
    // rather than at every process sharing the daemon's console.
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);

    let mut child = command.spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| std::io::Error::other("child exited before its pid could be read"))?;

    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        // Without a job there is no way to guarantee the worker's own children
        // come down with it, so refuse to supervise half-blind.
        let err = std::io::Error::last_os_error();
        let _ = child.start_kill();
        return Err(err);
    }
    let job = OwnedHandle(job);

    if let Some(handle) = child.raw_handle() {
        // A failed assignment leaves the child running outside the job: it can
        // still be stopped by pid, only its descendants are not guaranteed.
        unsafe { AssignProcessToJobObject(job.0, handle as HANDLE) };
    }

    let (tx, exit) = watch::channel(None);
    tokio::spawn(async move {
        let status = child.wait().await;
        let _ = tx.send(Some(status.unwrap_or_else(|_| exited_unknown())));
    });

    Ok(Supervised {
        pid,
        birth: birth_identity(pid),
        job,
        exit,
    })
}

impl Supervised {
    /// Resolves when the child exits. Multiple callers may wait independently.
    pub async fn wait(&self) -> ExitStatus {
        let mut exit = self.exit.clone();
        loop {
            if let Some(status) = *exit.borrow_and_update() {
                return status;
            }
            if exit.changed().await.is_err() {
                return exited_unknown();
            }
        }
    }

    /// Current state without blocking.
    pub fn poll(&self) -> Outcome {
        match *self.exit.borrow() {
            Some(status) => Outcome::Exited(status),
            None => Outcome::Running,
        }
    }

    /// CTRL_BREAK first, then terminate the whole job once `grace` elapses.
    pub async fn stop(&self, grace: Duration) -> ExitStatus {
        if let Outcome::Exited(status) = self.poll() {
            return status;
        }

        unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, self.pid) };
        if let Ok(status) = tokio::time::timeout(grace, self.wait()).await {
            return status;
        }

        // 1 becomes the exit code of every process left in the job.
        unsafe { TerminateJobObject(self.job.0, 1) };
        self.wait().await
    }
}

/// A job holding one short-lived process tree, for callers that manage their
/// own child (hooks) but still need to take its descendants down.
#[derive(Debug)]
pub struct JobHandle(OwnedHandle);

impl JobHandle {
    /// Kills every process still in the job.
    pub fn terminate(&self) {
        unsafe { TerminateJobObject(self.0.0, 1) };
    }
}

/// Puts an already-spawned child into a fresh job. `None` when the job could
/// not be created or the child has no handle left to assign.
pub fn attach_job(child: &tokio::process::Child) -> Option<JobHandle> {
    let handle = child.raw_handle()?;
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return None;
    }
    let job = OwnedHandle(job);
    if unsafe { AssignProcessToJobObject(job.0, handle as HANDLE) } == 0 {
        return None;
    }
    Some(JobHandle(job))
}

/// Process creation time as a raw FILETIME: the windows birth fingerprint.
pub fn creation_time(pid: u32) -> Option<u64> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let handle = OwnedHandle(handle);

    let mut creation = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exit = creation;
    let mut kernel = creation;
    let mut user = creation;
    let ok = unsafe { GetProcessTimes(handle.0, &mut creation, &mut exit, &mut kernel, &mut user) };
    if ok == 0 {
        return None;
    }
    Some(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
}

/// Whether the PID belongs to a process that has not exited yet. A handle that
/// cannot be opened is treated as gone.
pub fn is_running(pid: u32) -> bool {
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let handle = OwnedHandle(handle);
    // A zero timeout turns the wait into a state query: still waiting means
    // still running. Reading the exit code instead would misread a process that
    // legitimately exited with STILL_ACTIVE.
    let waited = unsafe { WaitForSingleObject(handle.0, 0) };
    waited == WAIT_TIMEOUT
}

fn exited_unknown() -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(0)
}
