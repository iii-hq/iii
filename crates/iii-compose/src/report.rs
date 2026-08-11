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
/// A line from the daemon itself, not from any project.
///
/// Compose prints only its own lines now: a worker's output belongs to the
/// engine, which is where it is read from. Interleaving the two made the
/// console useless for both.
pub fn daemon_line(message: &str, warn: bool) {
    use colored::Colorize;
    let prefix = "[compose]".dimmed();
    if warn {
        line(&format!("{prefix} {}", message.yellow()));
    } else {
        line(&format!("{prefix} {message}"));
    }
}

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

/// Colours handed out to containers, in this order. Chosen to stay
/// distinguishable on both light and dark terminals; red is deliberately
/// absent, because in this output red means "this failed".
const CONTAINER_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Magenta,
    Color::Green,
    Color::BrightBlue,
    Color::Yellow,
    Color::BrightMagenta,
];

/// Which colour each container was given, and which one went out last.
#[derive(Default)]
struct Palette {
    assigned: std::collections::HashMap<String, Color>,
    last: Option<Color>,
}

fn palette() -> &'static Mutex<Palette> {
    static PALETTE: OnceLock<Mutex<Palette>> = OnceLock::new();
    PALETTE.get_or_init(|| Mutex::new(Palette::default()))
}

/// The colour a container's output is tagged with.
///
/// Assigned on first use rather than hashed from the name. Hashing looked
/// tidier — same name, same colour, no state — but names in one project rhyme:
/// `node-api` and `python-api` hashed to the same colour, and two of the three
/// workers came out indistinguishable.
///
/// Assignment gives the guarantee that actually matters: no container ever
/// carries the colour of the one before it, and while the palette lasts every
/// container in a project is distinct. Containers are coloured in the order
/// they first appear, which for a project is its start order — deterministic,
/// so a container keeps its colour across restarts of the same project.
pub fn container_color(key: &str) -> Color {
    let palette = palette();
    let mut state = palette
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(color) = state.assigned.get(key) {
        return *color;
    }

    let taken: Vec<Color> = state.assigned.values().copied().collect();
    let color = pick_color(&taken, state.last);

    state.assigned.insert(key.to_string(), color);
    state.last = Some(color);
    color
}

/// The assignment policy, separated from the registry that holds it so it can
/// be tested without process-wide state.
///
/// Prefers a colour nobody has. Once the palette is exhausted it repeats — but
/// never with the colour that just went out, because two neighbours sharing a
/// tag is the thing this exists to prevent.
fn pick_color(taken: &[Color], last: Option<Color>) -> Color {
    CONTAINER_COLORS
        .iter()
        .find(|candidate| !taken.contains(candidate))
        .or_else(|| {
            CONTAINER_COLORS
                .iter()
                .find(|candidate| Some(**candidate) != last)
        })
        .copied()
        .unwrap_or(CONTAINER_COLORS[0])
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
/// Keeps a child's startup output in a file beside its project's state, until
/// the child can speak for itself.
///
/// Compose neither prints nor serves a worker's log: that belongs to the
/// engine, which is where it is read from. The one case the engine cannot
/// cover is a worker that dies before it ever connects, whose only account of
/// itself is on its own stderr. That window is what this covers, and
/// [`Capture::stop`] closes it the moment readiness makes the engine the
/// better source.
///
/// Draining never stops, only writing does. A pipe nobody reads fills at
/// 64 KiB and the child then blocks forever on its next `println!`, so a
/// capture that ended by dropping the reader would hang exactly the workers
/// that log the most.
pub fn capture_output(
    key: &str,
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
    log_dir: &std::path::Path,
) -> Capture {
    let path = log_dir.join(format!("{key}.log"));
    let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if std::fs::create_dir_all(log_dir).is_err() {
        return Capture(stopped);
    }

    for stream in [
        stdout.map(|out| Box::new(out) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        stderr.map(|err| Box::new(err) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let path = path.clone();
        let stopped = std::sync::Arc::clone(&stopped);
        tokio::spawn(async move {
            // Appended, never truncated: a container that restarts extends its
            // account rather than erasing the run that explains why.
            let Ok(file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            else {
                return;
            };
            let mut lines = BufReader::new(stream).lines();
            let mut sink = Some(file);
            while let Ok(Some(text)) = lines.next_line().await {
                if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                    // Releases the descriptor and keeps reading: the worker is
                    // registered, so from here its own logging is the record.
                    sink = None;
                    continue;
                }
                if let Some(file) = sink.as_mut() {
                    use std::io::Write;
                    let _ = writeln!(file, "{text}");
                }
            }
        });
    }
    Capture(stopped)
}

/// Ends a capture without ending the drain.
///
/// Dropping it does nothing: the tasks own their own copy of the flag, and a
/// container whose start failed keeps its output to the end so the error can
/// carry the last of it.
pub struct Capture(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Capture {
    /// Stop writing the child's output down.
    pub fn stop(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A container keeps its colour once it has one, and red is never handed
    /// out — in this output red means a failure.
    #[test]
    fn a_containers_colour_is_stable_and_never_red() {
        assert_eq!(container_color("stable-a"), container_color("stable-a"));
        for key in [
            "red-1", "red-2", "red-3", "red-4", "red-5", "red-6", "red-7",
        ] {
            assert_ne!(
                container_color(key),
                Color::Red,
                "red is reserved for failures"
            );
        }
    }

    /// Drives the policy the way the registry does, without touching the
    /// process-wide one: containers arrive one at a time, each seeing what the
    /// ones before it took.
    fn assign_in_sequence(count: usize) -> Vec<Color> {
        let mut taken: Vec<Color> = Vec::new();
        let mut last = None;
        let mut out = Vec::new();
        for _ in 0..count {
            let color = pick_color(&taken, last);
            taken.push(color);
            last = Some(color);
            out.push(color);
        }
        out
    }

    /// The bug this replaced a hash to fix: `node-api` and `python-api` summed
    /// to the same byte value modulo the palette, so two of three workers in
    /// one project came out indistinguishable.
    #[test]
    fn containers_of_one_project_do_not_share_a_colour() {
        let colors = assign_in_sequence(CONTAINER_COLORS.len());
        for (index, color) in colors.iter().enumerate() {
            assert!(
                !colors[..index].contains(color),
                "a project within the palette size must have no repeats: {colors:?}"
            );
        }
    }

    /// Past the palette size colours must repeat, but never back to back.
    #[test]
    fn no_container_repeats_the_colour_of_the_one_before_it() {
        let colors = assign_in_sequence(40);
        for pair in colors.windows(2) {
            assert_ne!(
                pair[0], pair[1],
                "adjacent containers must never share a colour"
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
