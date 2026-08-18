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

/// The live block: every container of the operation, and what it is doing.
///
/// One spinner was enough while containers started one at a time. They start in
/// waves now, so what an operator wants is not "which one is compose on" but
/// "where is all of it" — which is a block that is redrawn, not a line that is
/// replaced.
struct Console {
    rows: Vec<Row>,
    /// How many lines the block occupies on screen, so the next draw knows how
    /// far up to go. Zero when nothing is drawn.
    drawn: usize,
    frame: usize,
}

struct Row {
    key: String,
    /// How far in it sits: a container is drawn under the one that waits for
    /// it, so a graph reads as what needs what.
    depth: usize,
    state: RowState,
}

#[derive(Clone)]
enum RowState {
    /// Declared, and waiting on something earlier in the graph.
    Waiting,
    Starting,
    Ready(Duration),
    Failed,
    /// Already running, or otherwise not this operation's to start.
    Skipped(String),
}

fn console() -> &'static Mutex<Console> {
    static CONSOLE: OnceLock<Mutex<Console>> = OnceLock::new();
    CONSOLE.get_or_init(|| {
        Mutex::new(Console {
            rows: Vec::new(),
            drawn: 0,
            frame: 0,
        })
    })
}

/// Announces what this operation will touch, in the shape it will touch it.
///
/// `rows` is `(container, depth)` in the order to draw. Nothing is drawn on a
/// terminal that cannot animate: there, each container reports itself as it
/// settles, which is what a log wants anyway.
pub fn plan(rows: &[(String, usize)]) {
    if !animated() {
        return;
    }
    {
        let console = console();
        let mut state = console
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.rows = rows
            .iter()
            .map(|(key, depth)| Row {
                key: key.clone(),
                depth: *depth,
                state: RowState::Waiting,
            })
            .collect();
        state.drawn = 0;
        state.frame = 0;
        redraw(&mut state);
    }
    ensure_ticker();
}

/// Releases the block so later output does not overwrite it.
pub fn plan_done() {
    let console = console();
    let mut state = console
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    state.rows.clear();
    state.drawn = 0;
}

fn set(key: &str, to: RowState) -> bool {
    let console = console();
    let mut state = console
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(row) = state.rows.iter_mut().find(|row| row.key == key) else {
        return false;
    };
    row.state = to;
    redraw(&mut state);
    true
}

/// Redraws the block in place: up over what was drawn, then every row again.
fn redraw(state: &mut Console) {
    let mut out = String::new();
    if state.drawn > 0 {
        out.push_str(&format!("\x1b[{}A", state.drawn));
    }
    for row in &state.rows {
        out.push_str(CLEAR_LINE);
        out.push_str(&render_row(row, state.frame));
        out.push('\n');
    }
    state.drawn = state.rows.len();
    let mut stderr = std::io::stderr().lock();
    let _ = write!(stderr, "{out}");
    let _ = stderr.flush();
}

fn render_row(row: &Row, frame: usize) -> String {
    let indent = "  ".repeat(row.depth);
    match &row.state {
        RowState::Waiting => format!("{indent}{} {}", SKIPPED.dimmed(), row.key.dimmed()),
        RowState::Starting => format!(
            "{indent}{} {}",
            FRAMES[frame % FRAMES.len()].cyan(),
            row.key.bold()
        ),
        RowState::Ready(elapsed) => format!(
            "{indent}{} {} {}",
            OK.green(),
            row.key.bold(),
            format!("({})", format_elapsed(*elapsed)).dimmed()
        ),
        RowState::Failed => format!("{indent}{} {}", FAILED.red(), row.key.bold().red()),
        RowState::Skipped(why) => format!(
            "{indent}{} {} {}",
            SKIPPED.dimmed(),
            row.key.dimmed(),
            why.dimmed()
        ),
    }
}

/// Whether progress can animate./// Whether progress can animate. A pipe or a file gets static lines.
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
    let mut state = console
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut stderr = std::io::stderr().lock();

    // The block owns the bottom of the screen, so a line goes above it: rewind
    // over it, print, and draw it again underneath.
    if state.drawn > 0 {
        let _ = write!(stderr, "\x1b[{}A", state.drawn);
        for _ in 0..state.drawn {
            let _ = writeln!(stderr, "{CLEAR_LINE}");
        }
        let _ = write!(stderr, "\x1b[{}A", state.drawn);
    }
    let _ = writeln!(stderr, "{text}");
    if state.drawn > 0 {
        state.drawn = 0;
        redraw(&mut state);
    } else {
        let _ = stderr.flush();
    }
}

/// Turns the frame for the whole block. One task, not one per container: the
/// rows share a frame so they turn together instead of beating against each
/// other.
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
                let turning = state
                    .rows
                    .iter()
                    .any(|row| matches!(row.state, RowState::Starting));
                if turning {
                    state.frame = state.frame.wrapping_add(1);
                    redraw(&mut state);
                }
            }
        });
    });
}

/// A container is being worked on. On a terminal this spins until the container
/// settles; anywhere else it is a plain line.
pub fn starting(key: &str, what: &str) {
    if set(key, RowState::Starting) {
        return;
    }
    line(&format!(
        "{} {} {}",
        RUNNING.dimmed(),
        key.bold(),
        what.dimmed()
    ));
}

pub fn ready(key: &str, elapsed: Duration) {
    if set(key, RowState::Ready(elapsed)) {
        return;
    }
    line(&format!(
        "{} {} {} {}",
        OK.green(),
        key.bold(),
        "ready".green(),
        format!("({})", format_elapsed(elapsed)).dimmed()
    ));
}

pub fn failed(key: &str, code: &str, message: &str) {
    // Marked in the block, and the reason printed above it: a row is one line
    // wide and the reason is the part worth reading.
    set(key, RowState::Failed);
    line(&format!(
        "{} {} {} {}",
        FAILED.red(),
        key.bold(),
        code.red().bold(),
        message.red()
    ));
}

/// Already in the desired state — nothing was done to it.
pub fn unchanged(key: &str, what: &str) {
    if set(key, RowState::Skipped(what.to_string())) {
        return;
    }
    line(&format!(
        "{} {} {}",
        SKIPPED.dimmed(),
        key.bold(),
        what.dimmed()
    ));
}

pub fn stopped(key: &str) {
    line(&format!(
        "{} {} {}",
        OK.dimmed(),
        key.bold(),
        "stopped".dimmed()
    ));
}

/// Undone by a rollback. Amber, not red: nothing went wrong with this one.
pub fn rolled_back(key: &str) {
    line(&format!(
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
    line(&format!(
        "{} {}",
        body.green(),
        format!("in {}", format_elapsed(elapsed)).dimmed()
    ));
}

pub fn summary_failed(action: &str, code: &str, elapsed: Duration) {
    line(&format!(
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
        let row = Row {
            key: "api".to_string(),
            depth: 0,
            state: RowState::Starting,
        };
        assert!(!render_row(&row, usize::MAX).is_empty());
    }

    /// Depth is what makes the block a graph rather than a list, and it is
    /// drawn as indentation, so a nested row starts further in than its parent.
    #[test]
    fn depth_is_drawn_as_indentation() {
        let parent = Row {
            key: "harness".to_string(),
            depth: 0,
            state: RowState::Ready(Duration::from_millis(10)),
        };
        let child = Row {
            key: "queue".to_string(),
            depth: 1,
            state: RowState::Ready(Duration::from_millis(10)),
        };
        let drawn_parent = render_row(&parent, 0);
        let drawn_child = render_row(&child, 0);
        assert!(!drawn_parent.starts_with(' '), "{drawn_parent:?}");
        assert!(drawn_child.starts_with("  "), "{drawn_child:?}");
    }
}
