// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! What the daemon says while it works.
//!
//! An `up` can sit for a minute waiting on readiness. Without progress the
//! operator cannot tell a slow container from a hung one, so every container
//! announces itself as it is reached and again when it settles.
//!
//! The palette is four decisions, used consistently across the whole crate:
//!
//! - **red** — this failed;
//! - **amber** — look, but nothing broke (rolled back, skipped, unverifiable);
//! - **bold** — an identity: a container key, a daemon id, a project name;
//! - **dim** — scaffolding: labels, paths, elapsed times, prefixes.
//!
//! Everything goes to stderr so a caller can pipe a machine-readable result out
//! of stdout without the progress in the way.

use std::time::Duration;

use colored::{Color, Colorize};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Marks left of a container name. Two columns wide so the names line up
/// whatever state they are in.
const RUNNING: &str = "→";
const OK: &str = "✓";
const FAILED: &str = "✗";
const SKIPPED: &str = "·";

/// A container is being worked on. No newline discipline games: the line is
/// printed now, and the result prints its own line under it, so a child's own
/// output cannot land in the middle of a half-written status.
pub fn starting(key: &str, what: &str) {
    eprintln!("{} {} {}", RUNNING.dimmed(), key.bold(), what.dimmed());
}

pub fn ready(key: &str, elapsed: Duration) {
    eprintln!(
        "{} {} {} {}",
        OK.green(),
        key.bold(),
        "ready".green(),
        format!("({})", format_elapsed(elapsed)).dimmed()
    );
}

pub fn failed(key: &str, code: &str, message: &str) {
    eprintln!(
        "{} {} {} {}",
        FAILED.red(),
        key.bold(),
        code.red().bold(),
        message.red()
    );
}

/// Already in the desired state — nothing was done to it.
pub fn unchanged(key: &str, what: &str) {
    eprintln!("{} {} {}", SKIPPED.dimmed(), key.bold(), what.dimmed());
}

pub fn stopped(key: &str) {
    eprintln!("{} {} {}", OK.dimmed(), key.bold(), "stopped".dimmed());
}

/// Undone by a rollback. Amber, not red: nothing went wrong with this one.
pub fn rolled_back(key: &str) {
    eprintln!(
        "{} {} {}",
        SKIPPED.yellow(),
        key.bold(),
        "rolled back".yellow()
    );
}

/// Closing line of an operation.
pub fn summary_ok(action: &str, changed: usize, total: usize, elapsed: Duration) {
    let body = if changed == 0 {
        format!("{action}: nothing to do ({total} already in place)")
    } else {
        format!("{action}: {changed} of {total} changed")
    };
    eprintln!(
        "{} {}",
        body.green(),
        format!("in {}", format_elapsed(elapsed)).dimmed()
    );
}

pub fn summary_failed(action: &str, code: &str, elapsed: Duration) {
    eprintln!(
        "{} {} {}",
        format!("{action} failed").red().bold(),
        format!("[{code}]").red(),
        format!("after {}", format_elapsed(elapsed)).dimmed()
    );
}

/// Sub-second work is reported in milliseconds; past that the decimal is noise.
fn format_elapsed(elapsed: Duration) -> String {
    if elapsed.as_secs() == 0 {
        format!("{}ms", elapsed.as_millis())
    } else {
        format!("{:.1}s", elapsed.as_secs_f32())
    }
}

/// Colours a container's output is tagged with, in the order they are handed
/// out. Chosen to stay distinguishable on both light and dark terminals; red is
/// deliberately absent, because in this output red means "this failed".
const CONTAINER_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Magenta,
    Color::Blue,
    Color::Green,
    Color::Yellow,
    Color::BrightBlue,
];

/// A container's colour, derived from its name so it is the same on every run
/// and in every daemon that has the same project.
pub fn container_color(key: &str) -> Color {
    let sum: usize = key.bytes().map(|byte| byte as usize).sum();
    CONTAINER_COLORS[sum % CONTAINER_COLORS.len()]
}

/// Re-emits a child's output with a `[container]` tag in that container's
/// colour.
///
/// The line itself is never restyled: it belongs to the worker, and a worker
/// that colours its own logs must keep them. Only the tag is ours.
///
/// stderr is tagged the same way but the tag is bold, so a project's error
/// output is findable in a wall of five workers logging at once.
pub fn pump_output(
    key: &str,
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
) {
    let color = container_color(key);

    if let Some(stdout) = stdout {
        let tag = format!("[{key}]").color(color).to_string();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                println!("{tag} {line}");
            }
        });
    }

    if let Some(stderr) = stderr {
        let tag = format!("[{key}]").color(color).bold().to_string();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("{tag} {line}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same name, same colour, every run — a container keeps its identity
    /// across restarts of the daemon.
    #[test]
    fn a_containers_colour_is_stable_and_never_red() {
        assert_eq!(container_color("api"), container_color("api"));
        for key in ["api", "database", "web", "queue", "todo", "worker-9"] {
            assert_ne!(
                container_color(key),
                Color::Red,
                "red is reserved for failures"
            );
        }
    }

    #[test]
    fn elapsed_switches_unit_at_one_second() {
        assert_eq!(format_elapsed(Duration::from_millis(4)), "4ms");
        assert_eq!(format_elapsed(Duration::from_millis(999)), "999ms");
        assert_eq!(format_elapsed(Duration::from_millis(1500)), "1.5s");
    }
}
