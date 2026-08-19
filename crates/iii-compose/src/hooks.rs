// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Container hooks.
//!
//! `pre_run` blocks the container's start and is budgeted: a hung migration
//! must fail the container instead of holding the whole graph. `post_run` fires
//! after the container's exit is confirmed and is never awaited — a cleanup
//! script that hangs cannot be allowed to hold up a teardown.
//!
//! Both run with the same environment and working directory as the container
//! itself, and both get their own process group so a timeout can take their
//! children down too.

use std::{process::Stdio, time::Duration};

use colored::Colorize;
use tokio::io::AsyncReadExt;

use crate::{
    manifest::StartSpec,
    report,
    spawn::{SpawnCtx, spawn_plan},
};

#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("{hook} could not start: {source}")]
    Spawn {
        hook: &'static str,
        #[source]
        source: std::io::Error,
    },

    #[error("{hook} failed with exit code {code}")]
    Failed { hook: &'static str, code: i32 },

    #[error("{hook} timed out after {}s", timeout.as_secs())]
    Timeout {
        hook: &'static str,
        timeout: Duration,
    },
}

impl HookError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Spawn { .. } => "HOOK_SPAWN_FAILED",
            Self::Failed { .. } => "HOOK_FAILED",
            Self::Timeout { .. } => "HOOK_TIMEOUT",
        }
    }
}

/// Runs `pre_run` to completion. On timeout the hook's whole process group is
/// killed before the error returns, so nothing survives into the next attempt.
pub async fn await_pre_run(
    ctx: &SpawnCtx<'_>,
    script: &str,
    timeout: Duration,
) -> Result<(), HookError> {
    const HOOK: &str = "pre_run";

    let mut hook =
        spawn_hook(ctx, script).map_err(|source| HookError::Spawn { hook: HOOK, source })?;
    let child = &mut hook.child;

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();

    // Draining both pipes while waiting: a hook that writes more than a pipe
    // buffer would otherwise block forever and look like a timeout.
    let run = async {
        let (status, out, err) =
            tokio::join!(child.wait(), read_all(&mut stdout), read_all(&mut stderr));
        (status, out, err)
    };

    match tokio::time::timeout(timeout, run).await {
        Ok((status, out, err)) => {
            log_output(HOOK, ctx.container_key, &out, &err);
            let status = status.map_err(|source| HookError::Spawn { hook: HOOK, source })?;
            if status.success() {
                Ok(())
            } else {
                Err(HookError::Failed {
                    hook: HOOK,
                    code: status.code().unwrap_or(-1),
                })
            }
        }
        Err(_) => {
            // Kill before waiting: the hook is hung by definition, so waiting
            // first would never return.
            hook.kill_tree();
            let _ = hook.child.wait().await;
            Err(HookError::Timeout {
                hook: HOOK,
                timeout,
            })
        }
    }
}

/// Fires `post_run` and returns immediately. Its outcome is logged; a failing
/// cleanup script never turns into an error the caller has to handle.
pub fn fire_post_run(ctx: &SpawnCtx<'_>, script: &str) {
    const HOOK: &str = "post_run";

    let container = ctx.container_key.to_string();
    let mut hook = match spawn_hook(ctx, script) {
        Ok(hook) => hook,
        Err(err) => {
            report::line(&format!(
                "{} {}",
                format!("[{HOOK}:{container}]").dimmed(),
                format!("could not start: {err}").red()
            ));
            return;
        }
    };

    tokio::spawn(async move {
        let child = &mut hook.child;
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let (status, out, err) =
            tokio::join!(child.wait(), read_all(&mut stdout), read_all(&mut stderr));
        log_output(HOOK, &container, &out, &err);
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => report::line(&format!(
                "{} {}",
                format!("[{HOOK}:{container}]").dimmed(),
                format!("exited with {}", status.code().unwrap_or(-1)).yellow()
            )),
            Err(err) => report::line(&format!(
                "{} {}",
                format!("[{HOOK}:{container}]").dimmed(),
                format!("could not be waited on: {err}").yellow()
            )),
        }
    });
}

/// A hook process plus whatever the platform needs to take its descendants
/// down: a process group on unix, a job object on windows.
struct HookProcess {
    child: tokio::process::Child,
    #[cfg(windows)]
    job: Option<crate::process::windows::JobHandle>,
}

impl HookProcess {
    fn kill_tree(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            crate::process::unix::signal_group(pid as i32, nix::sys::signal::Signal::SIGKILL);
        }
        #[cfg(windows)]
        match &self.job {
            Some(job) => job.terminate(),
            // No job: the hook itself still dies, its own children may not.
            None => {
                let _ = self.child.start_kill();
            }
        }
    }
}

/// Builds a hook command that mirrors the container's environment and cwd.
fn spawn_hook(ctx: &SpawnCtx<'_>, script: &str) -> std::io::Result<HookProcess> {
    let start = StartSpec::Shell(script.to_string());
    let mut hook_ctx = ctx.clone();
    hook_ctx.start = &start;

    // A hook is a shell script and runs on the host even for a container that
    // does not, so the plan always yields a command here.
    let mut command = spawn_plan(&hook_ctx)
        .command()
        .expect("a hook is always a host shell command");
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);

    let child = command.spawn()?;
    #[cfg(windows)]
    let job = crate::process::windows::attach_job(&child);

    Ok(HookProcess {
        child,
        #[cfg(windows)]
        job,
    })
}

async fn read_all<R: AsyncReadExt + Unpin>(source: &mut Option<R>) -> String {
    let mut buffer = String::new();
    if let Some(source) = source.as_mut() {
        let _ = source.read_to_string(&mut buffer).await;
    }
    buffer
}

/// Prefixes every line a hook printed with where it came from. The prefix is
/// dimmed and the line is left exactly as the script wrote it: this is the
/// operator's own output, not ours to restyle.
fn log_output(hook: &str, container: &str, stdout: &str, stderr: &str) {
    for (stream, text) in [("out", stdout), ("err", stderr)] {
        for text in text.lines() {
            // Through `report::line`, not `eprintln!`: a hook runs while its
            // container's spinner is turning, and writing past it shreds both.
            report::line(&format!(
                "{} {text}",
                format!("[{hook}:{container}:{stream}]").dimmed()
            ));
        }
    }
}
