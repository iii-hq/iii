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
    exit: ExitSource,
}

/// How this process's exit becomes observable.
///
/// A child we spawned is reaped by a task that publishes the status. One
/// adopted from a previous daemon is not our child at all, so `waitpid` would
/// answer ECHILD — its liveness has to be polled instead, and its status can
/// never be recovered.
#[derive(Debug)]
enum ExitSource {
    Reaped(watch::Receiver<Option<ExitStatus>>),
    Adopted,
}

/// How often an adopted process is checked for liveness. Only teardown and
/// supervision wait on this, so the interval trades promptness for idle cost.
const ADOPTED_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How often the group is re-checked while waiting for orphans to leave.
const SWEEP_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Spawns `command` as its own process group leader and starts reaping it.
///
/// The caller must not have taken the child's exit status elsewhere: this
/// function owns the reaping so that `stop` and `exit_watch` observe the same
/// event.
pub fn spawn_supervised(command: tokio::process::Command) -> std::io::Result<Supervised> {
    spawn_supervised_inner(command, false).map(|(child, _)| child)
}

/// Same, but with the child's stdout and stderr piped back instead of inherited.
///
/// Compose owns the prefix on a child's output: a worker should not have to
/// print its own name to be identifiable in a project of five.
pub fn spawn_supervised_piped(
    command: tokio::process::Command,
) -> std::io::Result<(Supervised, ChildOutput)> {
    spawn_supervised_inner(command, true)
}

/// The child's output streams, when they were piped.
#[derive(Debug, Default)]
pub struct ChildOutput {
    pub stdout: Option<tokio::process::ChildStdout>,
    pub stderr: Option<tokio::process::ChildStderr>,
}

fn spawn_supervised_inner(
    mut command: tokio::process::Command,
    piped: bool,
) -> std::io::Result<(Supervised, ChildOutput)> {
    // Group leader: killpg then reaches the worker and everything it spawns.
    command.process_group(0);
    // Workers are background process-group leaders. Inheriting the daemon's
    // terminal stdin lets a read trigger SIGTTIN, leaving the child stopped in
    // `T` state while readiness waits forever. Workers communicate through iii,
    // never through Compose's controlling terminal.
    command.stdin(std::process::Stdio::null());
    if piped {
        command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
    }

    let mut child = command.spawn()?;
    let output = ChildOutput {
        stdout: child.stdout.take(),
        stderr: child.stderr.take(),
    };
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

    Ok((
        Supervised {
            pid,
            pgid: pid as i32,
            birth: birth_identity(pid),
            exit: ExitSource::Reaped(exit),
        },
        output,
    ))
}

impl Supervised {
    /// Takes over a process a previous daemon left running.
    ///
    /// Returns `None` unless the PID still carries the identity recorded for
    /// it: a recycled PID belongs to somebody else, and signalling it would be
    /// the worst kind of bug this module exists to prevent.
    ///
    /// The process group is the PID itself — every child is spawned as a group
    /// leader, so adopting the leader adopts its descendants with it.
    pub fn adopt(pid: u32, recorded: &BirthIdentity) -> Option<Self> {
        if !is_same_process(pid, recorded) {
            return None;
        }
        Some(Supervised {
            pid,
            pgid: pid as i32,
            birth: recorded.clone(),
            exit: ExitSource::Adopted,
        })
    }

    /// Whether this process was inherited rather than spawned here.
    pub fn is_adopted(&self) -> bool {
        matches!(self.exit, ExitSource::Adopted)
    }

    /// Resolves when the child exits. Multiple callers may wait independently.
    ///
    /// An adopted process reports [`exited_unknown`]: its real status went to
    /// the daemon that spawned it, and there is no way to ask the kernel for it
    /// after the fact.
    pub async fn wait(&self) -> ExitStatus {
        let mut exit = match &self.exit {
            ExitSource::Reaped(exit) => exit.clone(),
            ExitSource::Adopted => {
                while is_running(self.pid) {
                    tokio::time::sleep(ADOPTED_POLL_INTERVAL).await;
                }
                return exited_unknown();
            }
        };

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
        match &self.exit {
            ExitSource::Reaped(exit) => match *exit.borrow() {
                Some(status) => Outcome::Exited(status),
                None => Outcome::Running,
            },
            // A PID that is gone, or that came back as somebody else, is gone
            // to us either way.
            ExitSource::Adopted => {
                if is_same_process(self.pid, &self.birth) {
                    Outcome::Running
                } else {
                    Outcome::Exited(exited_unknown())
                }
            }
        }
    }

    /// SIGTERM the group, wait up to `grace`, then SIGKILL it. Returns the exit
    /// status once the child is gone.
    ///
    /// Ends with a sweep in every case. The leader exiting does not empty the
    /// group: `run: ./worker` is executed through a shell, and a shell that is
    /// killed leaves the worker it forked orphaned inside the same group,
    /// holding its port and its name. Returning on the leader's status alone
    /// was how a crashed container kept serving.
    pub async fn stop(&self, grace: Duration) -> ExitStatus {
        if let Outcome::Exited(status) = self.poll() {
            self.sweep_group(grace).await;
            return status;
        }

        signal_group(self.pgid, Signal::SIGTERM);
        let status = match tokio::time::timeout(grace, self.wait()).await {
            Ok(status) => status,
            Err(_) => {
                // Re-check identity before escalating: over a long grace the
                // leader may have exited and its PID gone to somebody else.
                match self.poll() {
                    Outcome::Exited(status) => status,
                    Outcome::Running => {
                        signal_group(self.pgid, Signal::SIGKILL);
                        self.wait().await
                    }
                }
            }
        };
        self.sweep_group(grace).await;
        status
    }

    /// Ends whatever is still in the group after the leader is gone.
    ///
    /// Safe precisely because the leader's PID is free: a process group id is
    /// its leader's pid, and nothing can hold this one without having joined
    /// the group — which only our own spawns do. If the pid came back as
    /// somebody else, the group may be theirs, and nothing is signalled.
    async fn sweep_group(&self, grace: Duration) {
        if is_running(self.pid) || !group_has_members(self.pgid) {
            return;
        }

        signal_group(self.pgid, Signal::SIGTERM);

        // These are not our children, so their exits are never reported to us:
        // emptiness of the group is the only observable.
        let deadline = tokio::time::Instant::now() + grace;
        while group_has_members(self.pgid) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(SWEEP_POLL_INTERVAL).await;
        }

        if group_has_members(self.pgid) {
            signal_group(self.pgid, Signal::SIGKILL);
        }
    }
}

/// Whether any process still belongs to the group. Signal 0 to a group answers
/// ESRCH when it is empty.
pub fn group_has_members(pgid: i32) -> bool {
    killpg(Pid::from_raw(pgid), None).is_ok()
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
