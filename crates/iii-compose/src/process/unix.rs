// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Unix supervision: one process group per child, teardown by group.

use std::{process::ExitStatus, time::Duration};

use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use tokio::sync::watch;

use super::{BirthIdentity, Outcome, birth_identity};

/// A running child plus everything needed to end it safely.
#[derive(Debug)]
pub struct Supervised {
    pub pid: u32,
    /// Process group id. Equal to `pid`, since the child is spawned as a group
    /// leader — but kept explicit because it is what gets signalled.
    pub pgid: i32,
    pub birth: BirthIdentity,
    exit: watch::Receiver<Option<ExitStatus>>,
}

/// Spawns `command` as its own process group leader and starts reaping it.
///
/// The caller must not have taken the child's exit status elsewhere: this
/// function owns the reaping so that `stop` and `exit_watch` observe the same
/// event.
pub fn spawn_supervised(mut command: tokio::process::Command) -> std::io::Result<Supervised> {
    // Group leader: killpg then reaches the worker and everything it spawns.
    command.process_group(0);

    let mut child = command.spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| std::io::Error::other("child exited before its pid could be read"))?;

    let (tx, exit) = watch::channel(None);
    tokio::spawn(async move {
        let status = child.wait().await;
        // A failed wait is reported as "gone": the process is no longer ours to
        // signal either way.
        let _ = tx.send(Some(status.unwrap_or_else(|_| exited_unknown())));
    });

    Ok(Supervised {
        pid,
        pgid: pid as i32,
        birth: birth_identity(pid),
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
            // The sender lives in the reaper task and is only dropped after it
            // sends, so a closed channel means the status was already observed.
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

    /// SIGTERM the group, wait up to `grace`, then SIGKILL it. Returns the exit
    /// status once the child is gone.
    pub async fn stop(&self, grace: Duration) -> ExitStatus {
        if let Outcome::Exited(status) = self.poll() {
            return status;
        }

        signal_group(self.pgid, Signal::SIGTERM);
        if let Ok(status) = tokio::time::timeout(grace, self.wait()).await {
            return status;
        }

        signal_group(self.pgid, Signal::SIGKILL);
        self.wait().await
    }
}

/// Signals a whole process group, ignoring "already gone".
pub fn signal_group(pgid: i32, signal: Signal) {
    let _ = killpg(Pid::from_raw(pgid), signal);
}

/// Signal 0: asks the kernel whether the PID exists without delivering
/// anything.
pub fn is_running(pid: u32) -> bool {
    nix::sys::signal::kill(Pid::from_raw(pid as i32), None).is_ok()
}

/// Whether a PID recorded earlier still refers to the same process, and may
/// therefore be signalled. A recycled or unverifiable PID answers `false`.
pub fn is_same_process(pid: u32, recorded: &BirthIdentity) -> bool {
    recorded.matches(&birth_identity(pid))
}

/// Placeholder status for a child whose real status could not be collected.
/// Not a real exit code — callers only use it to mean "gone".
fn exited_unknown() -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(0)
}
