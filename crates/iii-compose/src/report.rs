// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! What the daemon says while it works.
//!
//! An `up` can sit for a minute waiting on readiness, so the container being
//! waited on spins: a spinner is the difference between "slow" and "hung" while
//! nothing else is being printed.
//!
//! Everything the daemon writes goes through [`line`] or [`finish`], and both
//! take the console lock. That is not tidiness — the children are writing to the
//! same terminal at the same time, and a spinner that does not clear its own
//! line before someone else writes leaves the two shredded together.
//!
//! The spinner only exists when stderr is a terminal. Redirected to a file or a
//! pipe it is replaced by a plain line, because a log full of `⠹⠸⠼` frames is
//! worse than no progress at all.
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

use std::{
    io::{IsTerminal, Write},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use colored::{Color, Colorize};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Marks left of a container name.
const OK: &str = "✓";
const FAILED: &str = "✗";
const SKIPPED: &str = "·";
const RUNNING: &str = "→";

/// Braille frames: one cell wide in every terminal font, so the lines that
/// follow do not shift sideways as it turns.
const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Fast enough to read as motion, slow enough not to churn a remote terminal.
const FRAME_INTERVAL: Duration = Duration::from_millis(90);

/// Erase the current line and return to its start.
const CLEAR_LINE: &str = "\r\x1b[2K";

struct Console {
    /// The container currently being waited on, if any. `up` starts containers
    /// one at a time, so there is never more than one.
    spinner: Option<Spinner>,
}

struct Spinner {
    /// Already-styled text drawn to the right of the frame.
    label: String,
    frame: usize,
}

fn console() -> &'static Mutex<Console> {
    static CONSOLE: OnceLock<Mutex<Console>> = OnceLock::new();
    CONSOLE.get_or_init(|| Mutex::new(Console { spinner: None }))
}

/// Whether progress can animate. A pipe or a file gets static lines.
fn animated() -> bool {
    static ANIMATED: OnceLock<bool> = OnceLock::new();
    *ANIMATED.get_or_init(|| std::io::stderr().is_terminal())
}

/// Writes one line, stepping around the spinner if one is turning.
///
/// Every line the daemon or a child emits goes through here.
pub fn line(text: &str) {
    let console = console();
    let state = console
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut stderr = std::io::stderr().lock();

    if state.spinner.is_some() {
        let _ = write!(stderr, "{CLEAR_LINE}");
    }
    let _ = writeln!(stderr, "{text}");
    if let Some(spinner) = &state.spinner {
        let _ = write!(stderr, "{}", frame_of(spinner));
        let _ = stderr.flush();
    }
}

/// Replaces the spinner with its final line.
fn finish(text: &str) {
    let console = console();
    let mut state = console
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut stderr = std::io::stderr().lock();

    if state.spinner.take().is_some() {
        let _ = write!(stderr, "{CLEAR_LINE}");
    }
    let _ = writeln!(stderr, "{text}");
}

fn frame_of(spinner: &Spinner) -> String {
    format!(
        "{} {}",
        FRAMES[spinner.frame % FRAMES.len()].cyan(),
        spinner.label
    )
}

/// Advances every spinner that appears, for as long as the process lives. One
/// task, started on the first spinner: a task per container would leave the
/// frames beating against each other.
fn ensure_ticker() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(FRAME_INTERVAL).await;
                let console = console();
                let mut state = console
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(spinner) = &mut state.spinner {
                    spinner.frame = spinner.frame.wrapping_add(1);
                    let mut stderr = std::io::stderr().lock();
                    let _ = write!(stderr, "{CLEAR_LINE}{}", frame_of(spinner));
                    let _ = stderr.flush();
                }
            }
        });
    });
}

/// A container is being worked on. On a terminal this spins until the container
/// settles; anywhere else it is a plain line.
pub fn starting(key: &str, what: &str) {
    if !animated() {
        line(&format!(
            "{} {} {}",
            RUNNING.dimmed(),
            key.bold(),
            what.dimmed()
        ));
        return;
    }

    let label = format!("{} {}", key.bold(), what.dimmed());
    let console = console();
    {
        let mut state = console
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.spinner = Some(Spinner { label, frame: 0 });
        let mut stderr = std::io::stderr().lock();
        if let Some(spinner) = &state.spinner {
            let _ = write!(stderr, "{CLEAR_LINE}{}", frame_of(spinner));
            let _ = stderr.flush();
        }
    }
    ensure_ticker();
}

pub fn ready(key: &str, elapsed: Duration) {
    finish(&format!(
        "{} {} {} {}",
        OK.green(),
        key.bold(),
        "ready".green(),
        format!("({})", format_elapsed(elapsed)).dimmed()
    ));
}

pub fn failed(key: &str, code: &str, message: &str) {
    finish(&format!(
        "{} {} {} {}",
        FAILED.red(),
        key.bold(),
        code.red().bold(),
        message.red()
    ));
}

/// Already in the desired state — nothing was done to it.
pub fn unchanged(key: &str, what: &str) {
    finish(&format!(
        "{} {} {}",
        SKIPPED.dimmed(),
        key.bold(),
        what.dimmed()
    ));
}

pub fn stopped(key: &str) {
    finish(&format!(
        "{} {} {}",
        OK.dimmed(),
        key.bold(),
        "stopped".dimmed()
    ));
}

/// Undone by a rollback. Amber, not red: nothing went wrong with this one.
pub fn rolled_back(key: &str) {
    finish(&format!(
        "{} {} {}",
        SKIPPED.yellow(),
        key.bold(),
        "rolled back".yellow()
    ));
}

/// Closing line of an operation.
pub fn summary_ok(action: &str, changed: usize, total: usize, elapsed: Duration) {
    let body = if changed == 0 {
        format!("{action}: nothing to do ({total} already in place)")
    } else {
        format!("{action}: {changed} of {total} changed")
    };
    finish(&format!(
        "{} {}",
        body.green(),
        format!("in {}", format_elapsed(elapsed)).dimmed()
    ));
}

pub fn summary_failed(action: &str, code: &str, elapsed: Duration) {
    finish(&format!(
        "{} {} {}",
        format!("{action} failed").red().bold(),
        format!("[{code}]").red(),
        format!("after {}", format_elapsed(elapsed)).dimmed()
    ));
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
/// output is findable in a wall of five workers logging at once. Both streams
/// go out through [`line`], which is what keeps them from colliding with a
/// spinner that is turning at the same time.
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
            while let Ok(Some(text)) = lines.next_line().await {
                line(&format!("{tag} {text}"));
            }
        });
    }

    if let Some(stderr) = stderr {
        let tag = format!("[{key}]").color(color).bold().to_string();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(text)) = lines.next_line().await {
                line(&format!("{tag} {text}"));
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

    /// The frame index is advanced with wrapping arithmetic and only ever used
    /// modulo the frame count, so a long wait cannot panic on overflow.
    #[test]
    fn frames_wrap_instead_of_overflowing() {
        let spinner = Spinner {
            label: String::new(),
            frame: usize::MAX,
        };
        assert!(!frame_of(&spinner).is_empty());
    }
}
