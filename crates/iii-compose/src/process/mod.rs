// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Child supervision.
//!
//! Two invariants drive this module:
//!
//! 1. **A child is a process group, not a process.** Workers spawn language
//!    runtimes, build tools and their own children; signalling only the direct
//!    child leaves grandchildren holding ports and engine connections.
//! 2. **A PID is never signalled unless its identity is verified.** PIDs are
//!    recycled. After a daemon restart the recorded PID may belong to an
//!    unrelated process, so a start-time fingerprint is compared first and a
//!    mismatch reports for manual cleanup instead of signalling.

use std::{process::ExitStatus, time::Duration};

#[cfg(unix)]
pub mod unix;

#[cfg(unix)]
pub use unix::{Supervised, spawn_supervised};

/// Fingerprint that distinguishes a live process from a recycled PID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BirthIdentity {
    /// Kernel start time, in clock ticks since boot (linux `/proc/<pid>/stat`).
    StartTime(u64),
    /// The platform has no cheap fingerprint available. Treated as
    /// unverifiable: a recorded PID with this identity is never signalled.
    Unavailable,
}

impl BirthIdentity {
    /// Whether `self` (recorded earlier) may be signalled given the identity
    /// read from the live PID now. Unverifiable on either side means no.
    pub fn matches(&self, current: &BirthIdentity) -> bool {
        match (self, current) {
            (Self::StartTime(recorded), Self::StartTime(current)) => recorded == current,
            _ => false,
        }
    }
}

/// How a supervised child ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Exited(ExitStatus),
    /// Still running when the caller stopped waiting.
    Running,
}

/// Grace period between SIGTERM and SIGKILL during teardown.
pub const DEFAULT_STOP_GRACE: Duration = Duration::from_secs(10);

/// Reads the birth identity of a live PID.
pub fn birth_identity(pid: u32) -> BirthIdentity {
    #[cfg(target_os = "linux")]
    {
        linux_start_time(pid)
            .map(BirthIdentity::StartTime)
            .unwrap_or(BirthIdentity::Unavailable)
    }
    // macOS needs libproc for the same fact; until that path is written and
    // tested on a mac, every non-linux PID is deliberately unverifiable.
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        BirthIdentity::Unavailable
    }
}

/// Field 22 of `/proc/<pid>/stat`. The comm field is parenthesised and may
/// itself contain spaces and parens, so parsing starts after its last `)`.
#[cfg(target_os = "linux")]
fn linux_start_time(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = &stat[stat.rfind(')')? + 1..];
    after_comm.split_whitespace().nth(19)?.parse().ok()
}
