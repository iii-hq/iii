// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Ties every supervised process group to the life of this process.
//!
//! Children lead their own process groups so a terminal's Ctrl+C reaches
//! compose alone and compose can stop them in dependency order. The cost of
//! that isolation was that nothing stopped them when compose died without
//! running its teardown: SIGKILL, a panic, a second Ctrl+C. They kept running
//! with no owner, reconnecting to an engine that was gone, until somebody
//! found them by hand.
//!
//! The reaper closes that gap from outside. It is a small process of its own
//! whose stdin is a pipe that only this process holds open. Each spawned group
//! is announced on it, and withdrawn when this process lets go of the handle.
//! When the pipe reaches EOF, which the kernel guarantees however this process
//! ends, the reaper stops every group still announced and exits.
//!
//! Opt-in: the host binary names the program that runs [`run`] through
//! [`set_program`]. Without it nothing is spawned and supervision behaves as it
//! always did.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{BufRead, Write},
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use nix::sys::signal::Signal;

use super::{
    BirthIdentity, DEFAULT_STOP_GRACE,
    unix::{group_has_members, is_running, is_same_process, signal_group},
};

/// Present in the environment of the process that should run [`run`].
pub const ENV: &str = super::REAPER_ENV;

static PROGRAM: OnceLock<(PathBuf, Vec<OsString>)> = OnceLock::new();
static PIPE: OnceLock<Option<Mutex<std::process::ChildStdin>>> = OnceLock::new();

/// Names the program that runs [`run`] when started with [`ENV`] set.
pub fn set_program(program: PathBuf, args: Vec<OsString>) {
    let _ = PROGRAM.set((program, args));
}

/// Whether this process was started to be a reaper.
pub fn requested() -> bool {
    std::env::var_os(ENV).is_some()
}

/// Starts the reaper if the host enabled it. Called before every spawn, so the
/// pipe exists before any child is forked and no child can inherit its write
/// end in the moment before it is marked close-on-exec.
pub(crate) fn ensure() {
    pipe();
}

/// Announces a group this process now owns.
pub(crate) fn announce(pgid: i32, birth: &BirthIdentity) {
    let birth = match birth {
        BirthIdentity::StartTime(start) => start.to_string(),
        BirthIdentity::Unavailable => "-".to_string(),
    };
    send(&format!("+{pgid} {birth}\n"));
}

/// Withdraws a group this process no longer owns: it was stopped, or handed
/// back to the daemon that really owns it.
pub(crate) fn withdraw(pgid: i32) {
    send(&format!("-{pgid}\n"));
}

fn send(line: &str) {
    if let Some(pipe) = pipe() {
        // A reaper that died early answers EPIPE. Supervision itself still
        // works; only the safety net is gone.
        let _ = pipe
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .write_all(line.as_bytes());
    }
}

fn pipe() -> Option<&'static Mutex<std::process::ChildStdin>> {
    PIPE.get_or_init(|| {
        use std::os::unix::process::CommandExt;

        let (program, args) = PROGRAM.get()?;
        let mut child = std::process::Command::new(program)
            .arg0("iii-compose-reaper")
            .args(args)
            .env(ENV, "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            // Its own group: the Ctrl+C meant for compose must not end it
            // before compose has had the chance to stop anything.
            .process_group(0)
            .spawn()
            .ok()?;
        Some(Mutex::new(child.stdin.take()?))
    })
    .as_ref()
}

/// The reaper's whole life: track announcements until the owner's end of the
/// pipe closes, stop what is still announced, exit.
pub fn run() -> ! {
    // Terminal signals belong to compose. The reaper ends on EOF only.
    unsafe {
        let _ = nix::sys::signal::signal(Signal::SIGINT, nix::sys::signal::SigHandler::SigIgn);
        let _ = nix::sys::signal::signal(Signal::SIGHUP, nix::sys::signal::SigHandler::SigIgn);
    }
    serve(std::io::stdin().lock(), DEFAULT_STOP_GRACE);
    std::process::exit(0)
}

/// Reads announcements until EOF, then stops every group still announced.
///
/// Counted, not set: a project reopened in place adopts a group before the
/// handle it replaces is dropped, and that late withdrawal must not cancel the
/// new owner's announcement.
pub fn serve(input: impl BufRead, grace: Duration) {
    let mut groups: BTreeMap<i32, (BirthIdentity, usize)> = BTreeMap::new();
    for line in input.lines() {
        let Ok(line) = line else { break };
        if let Some(rest) = line.strip_prefix('+') {
            let mut fields = rest.split(' ');
            let Some(Ok(pgid)) = fields.next().map(str::parse) else {
                continue;
            };
            let birth = match fields.next().map(str::parse) {
                Some(Ok(start)) => BirthIdentity::StartTime(start),
                _ => BirthIdentity::Unavailable,
            };
            let entry = groups.entry(pgid).or_insert((birth.clone(), 0));
            entry.0 = birth;
            entry.1 += 1;
        } else if let Some(Ok(pgid)) = line.strip_prefix('-').map(str::parse)
            && let Some(entry) = groups.get_mut(&pgid)
        {
            entry.1 -= 1;
            if entry.1 == 0 {
                groups.remove(&pgid);
            }
        }
    }
    let groups = groups
        .into_iter()
        .map(|(pgid, (birth, _))| (pgid, birth))
        .collect();
    reap(&groups, grace);
}

/// SIGTERM, up to `grace` for the groups to empty, SIGKILL for what is left.
fn reap(groups: &BTreeMap<i32, BirthIdentity>, grace: Duration) {
    let ours: Vec<i32> = groups
        .iter()
        .filter(|(pgid, birth)| still_ours(**pgid, birth))
        .map(|(pgid, _)| *pgid)
        .collect();
    for &pgid in &ours {
        signal_group(pgid, Signal::SIGTERM);
    }
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline && ours.iter().any(|&pgid| group_has_members(pgid)) {
        std::thread::sleep(Duration::from_millis(50));
    }
    for &pgid in &ours {
        if group_has_members(pgid) {
            signal_group(pgid, Signal::SIGKILL);
        }
    }
}

/// The rule `Supervised::stop` follows: a live leader must still be the
/// recorded process, and a dead leader's group can only hold what was spawned
/// into it, because a group id is not reused while the group has members.
fn still_ours(pgid: i32, birth: &BirthIdentity) -> bool {
    if is_running(pgid as u32) {
        is_same_process(pgid as u32, birth)
    } else {
        group_has_members(pgid)
    }
}
