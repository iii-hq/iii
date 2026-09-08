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
//! take the console lock. Worker output is retained by the log store instead
//! of writing into this terminal, but concurrent lifecycle operations can
//! still update the same progress block.
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
    time::{Duration, Instant},
};

use colored::{Color, Colorize};

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
    startup: Option<StartupRows>,
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
    Starting {
        what: String,
        began: Instant,
    },
    Ready {
        what: String,
        elapsed: Duration,
    },
    Failed,
    Error {
        what: String,
        elapsed: Duration,
    },
    /// Already running, or otherwise not this operation's to start.
    Skipped(String),
}

struct StartupRows {
    engine: Row,
    containers: Row,
}

/// Owns the foreground startup panel. Early returns must leave a final status,
/// never an animated row above an error.
pub(crate) struct StartupProgress {
    finished: bool,
}

impl StartupProgress {
    pub(crate) fn start(managed: bool) -> Self {
        let mut state = console().lock().unwrap_or_else(|p| p.into_inner());
        state.startup = Some(StartupRows::new(managed));
        if animated() {
            redraw(&mut state);
            ensure_ticker();
        } else if let Some(startup) = &state.startup {
            eprintln!("{}", render_row(&startup.engine, 0, false));
            eprintln!("{}", render_row(&startup.containers, 0, false));
        }
        Self { finished: false }
    }

    pub(crate) fn engine_waiting(&self) {
        self.update_engine("Waiting for connection", false);
    }

    pub(crate) fn engine_ready(&self) {
        self.update_engine("Ready", true);
    }

    fn update_engine(&self, message: &str, ready: bool) {
        let mut state = console().lock().unwrap_or_else(|p| p.into_inner());
        let Some(startup) = &mut state.startup else {
            return;
        };
        let RowState::Starting { what, began } = &mut startup.engine.state else {
            return;
        };
        if ready {
            startup.engine.state = RowState::Ready {
                what: message.to_string(),
                elapsed: began.elapsed(),
            };
        } else {
            *what = message.to_string();
        }
        if animated() {
            redraw(&mut state);
        } else {
            eprintln!("{}", render_row(&startup.engine, 0, false));
        }
    }

    pub(crate) fn containers_starting(&self) {
        let mut state = console().lock().unwrap_or_else(|p| p.into_inner());
        let Some(startup) = &mut state.startup else {
            return;
        };
        startup.containers.state = RowState::Starting {
            what: "Starting".to_string(),
            began: Instant::now(),
        };
        if animated() {
            redraw(&mut state);
        } else {
            eprintln!("{}", render_row(&startup.containers, 0, false));
        }
    }

    pub(crate) fn finish(&mut self, success: bool, message: &str) {
        if self.finished {
            return;
        }
        self.finished = true;
        let mut state = console().lock().unwrap_or_else(|p| p.into_inner());
        let Some(startup) = &mut state.startup else {
            return;
        };
        startup.finish(success, message);
        // Pending/active child rows cannot keep spinning after a failed or
        // cancelled operation.
        for row in &mut state.rows {
            match row.state {
                RowState::Waiting => row.state = RowState::Skipped("Not started".to_string()),
                RowState::Starting { .. } => row.state = RowState::Skipped("Cancelled".to_string()),
                _ => {}
            }
        }
        if animated() {
            redraw(&mut state);
        } else if let Some(startup) = &state.startup {
            if !matches!(startup.engine.state, RowState::Ready { .. }) {
                eprintln!("{}", render_row(&startup.engine, 0, false));
            }
            eprintln!("{}", render_row(&startup.containers, 0, false));
        }
        state.startup = None;
        state.rows.clear();
        state.drawn = 0;
    }
}

impl Drop for StartupProgress {
    fn drop(&mut self) {
        self.finish(false, "Failed");
    }
}

impl StartupRows {
    fn new(managed: bool) -> Self {
        Self {
            engine: Row {
                key: "Engine".to_string(),
                depth: 0,
                state: RowState::Starting {
                    what: if managed { "Starting" } else { "Connecting" }.to_string(),
                    began: Instant::now(),
                },
            },
            containers: Row {
                key: "Containers".to_string(),
                depth: 0,
                state: RowState::Waiting,
            },
        }
    }

    fn finish(&mut self, success: bool, message: &str) {
        let row = if matches!(self.containers.state, RowState::Waiting)
            && !matches!(self.engine.state, RowState::Ready { .. })
        {
            self.containers.state = RowState::Skipped("Not started".to_string());
            &mut self.engine
        } else {
            // An empty project can finish even while an external engine is
            // unavailable. Completing that project does not prove connection.
            if matches!(self.engine.state, RowState::Starting { .. }) {
                self.engine.state = RowState::Skipped("Not connected".to_string());
            }
            &mut self.containers
        };
        let elapsed = match &row.state {
            RowState::Starting { began, .. } => began.elapsed(),
            _ => Duration::ZERO,
        };
        row.state = if success {
            RowState::Ready {
                what: message.to_string(),
                elapsed,
            }
        } else if message == "Cancelled" {
            RowState::Skipped(message.to_string())
        } else {
            RowState::Error {
                what: message.to_string(),
                elapsed,
            }
        };
    }
}

fn console() -> &'static Mutex<Console> {
    static CONSOLE: OnceLock<Mutex<Console>> = OnceLock::new();
    CONSOLE.get_or_init(|| {
        Mutex::new(Console {
            startup: None,
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
        // The engine panel already owns the block above these new child rows.
        if state.startup.is_none() {
            state.drawn = 0;
        }
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
    if state.startup.is_none() {
        state.rows.clear();
        state.drawn = 0;
    }
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
    let ready = state
        .rows
        .iter()
        .filter(|row| {
            matches!(row.state, RowState::Ready { .. })
                || matches!(&row.state, RowState::Skipped(why) if why == "already running")
        })
        .count();
    let total = state.rows.len();
    if let Some(startup) = &mut state.startup
        && let RowState::Starting { what, .. } = &mut startup.containers.state
    {
        *what = format!("Starting ({ready}/{total})");
    }
    redraw(&mut state);
    true
}

/// Redraws the block in place: up over what was drawn, then every row again.
fn redraw(state: &mut Console) {
    let mut out = String::new();
    if state.drawn > 0 {
        out.push_str(&format!("\x1b[{}A", state.drawn));
    }
    let headers = state
        .startup
        .iter()
        .flat_map(|startup| [&startup.engine, &startup.containers])
        .map(|row| (row, ""));
    let indent = if state.startup.is_some() { "  " } else { "" };
    for (row, prefix) in headers.chain(state.rows.iter().map(|row| (row, indent))) {
        out.push_str(CLEAR_LINE);
        out.push_str(prefix);
        out.push_str(&render_row(row, state.frame, true));
        out.push('\n');
    }
    state.drawn = state.rows.len() + if state.startup.is_some() { 2 } else { 0 };
    let mut stderr = std::io::stderr().lock();
    let _ = write!(stderr, "{out}");
    let _ = stderr.flush();
}

fn render_row(row: &Row, frame: usize, animate: bool) -> String {
    let indent = "  ".repeat(row.depth);
    match &row.state {
        RowState::Waiting => format!(
            "{indent}{} {} {}",
            SKIPPED.dimmed(),
            row.key.dimmed(),
            "Pending".dimmed()
        ),
        RowState::Starting { what, began } => format!(
            "{indent}{} {} {} {}",
            if animate {
                FRAMES[frame % FRAMES.len()]
            } else {
                RUNNING
            }
            .cyan(),
            row.key.bold(),
            what.dimmed(),
            format!("({})", format_elapsed(began.elapsed())).dimmed(),
        ),
        RowState::Ready { what, elapsed } => format!(
            "{indent}{} {} {} {}",
            OK.green(),
            row.key.bold(),
            what.green(),
            format!("({})", format_elapsed(*elapsed)).dimmed()
        ),
        RowState::Failed => format!("{indent}{} {}", FAILED.red(), row.key.bold().red()),
        RowState::Error { what, elapsed } => format!(
            "{indent}{} {} {} {}",
            FAILED.red(),
            row.key.bold(),
            what.red(),
            format!("({})", format_elapsed(*elapsed)).dimmed(),
        ),
        RowState::Skipped(why) => format!(
            "{indent}{} {} {}",
            SKIPPED.dimmed(),
            row.key.dimmed(),
            why.dimmed()
        ),
    }
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
                let turning = state.startup.is_some()
                    || state
                        .rows
                        .iter()
                        .any(|row| matches!(row.state, RowState::Starting { .. }));
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
    let began = {
        let state = console().lock().unwrap_or_else(|p| p.into_inner());
        state
            .rows
            .iter()
            .find_map(|row| match &row.state {
                RowState::Starting { began, .. } if row.key == key => Some(*began),
                _ => None,
            })
            .unwrap_or_else(Instant::now)
    };
    if set(
        key,
        RowState::Starting {
            what: what.to_string(),
            began,
        },
    ) {
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
    completed(key, "ready", elapsed);
}

pub fn completed(key: &str, what: &str, elapsed: Duration) {
    if set(
        key,
        RowState::Ready {
            what: what.to_string(),
            elapsed,
        },
    ) {
        return;
    }
    line(&format!(
        "{} {} {} {}",
        OK.green(),
        key.bold(),
        what.green(),
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
    set(key, RowState::Skipped("rolled back".to_string()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_failure_leaves_containers_not_started() {
        let mut startup = StartupRows::new(true);
        startup.finish(false, "Connection timed out");
        assert!(matches!(startup.engine.state, RowState::Error { .. }));
        assert!(
            matches!(&startup.containers.state, RowState::Skipped(why) if why == "Not started")
        );
    }

    #[test]
    fn container_failure_preserves_confirmed_engine_readiness() {
        let mut startup = StartupRows::new(true);
        startup.engine.state = RowState::Ready {
            what: "Ready".to_string(),
            elapsed: Duration::from_secs(6),
        };
        startup.containers.state = RowState::Starting {
            what: "Starting (1/3)".to_string(),
            began: Instant::now(),
        };
        startup.finish(false, "Failed");
        assert!(matches!(startup.engine.state, RowState::Ready { .. }));
        assert!(matches!(startup.containers.state, RowState::Error { .. }));
    }

    #[test]
    fn empty_project_does_not_claim_an_unconnected_engine_is_ready() {
        let mut startup = StartupRows::new(false);
        startup.containers.state = RowState::Starting {
            what: "Starting".to_string(),
            began: Instant::now(),
        };
        startup.finish(true, "Ready");
        assert!(matches!(&startup.engine.state, RowState::Skipped(why) if why == "Not connected"));
        assert!(matches!(startup.containers.state, RowState::Ready { .. }));
    }

    #[test]
    fn redirected_startup_reports_transitions_without_animation() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "report::tests::startup_panel_fixture",
                "--nocapture",
            ])
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        let starting = stderr.find("Engine Starting").unwrap();
        let waiting = stderr.find("Engine Waiting for connection").unwrap();
        let ready = stderr.find("Engine Ready").unwrap();
        let containers = stderr.find("Containers Starting").unwrap();
        let done = stderr.find("Containers Ready").unwrap();
        assert!(
            starting < waiting && waiting < ready && ready < containers && containers < done,
            "{stderr}"
        );
        assert!(
            !stderr.contains('\x1b') && !FRAMES.iter().any(|frame| stderr.contains(frame)),
            "{stderr}"
        );
    }

    /// Also usable under a PTY to inspect redraws without starting real workers.
    #[tokio::test]
    #[ignore = "subprocess fixture for the progress renderer"]
    async fn startup_panel_fixture() {
        let mut progress = StartupProgress::start(true);
        progress.engine_waiting();
        tokio::time::sleep(Duration::from_millis(250)).await;
        progress.engine_ready();
        line("compose serving");
        progress.containers_starting();
        plan(&[("api".to_string(), 0), ("redis".to_string(), 1)]);
        starting("redis", "waiting for engine registration");
        tokio::time::sleep(Duration::from_millis(250)).await;
        ready("redis", Duration::from_millis(250));
        starting("api", "installing package");
        tokio::time::sleep(Duration::from_millis(250)).await;
        starting("api", "waiting for engine registration");
        tokio::time::sleep(Duration::from_millis(250)).await;
        ready("api", Duration::from_millis(500));
        plan_done();
        progress.finish(true, "Ready");
        // The finished panel must not be redrawn over subsequent output.
        line("after startup");
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

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
            state: RowState::Starting {
                what: "starting".to_string(),
                began: Instant::now(),
            },
        };
        assert!(!render_row(&row, usize::MAX, true).is_empty());
    }

    /// Depth is what makes the block a graph rather than a list, and it is
    /// drawn as indentation, so a nested row starts further in than its parent.
    #[test]
    fn depth_is_drawn_as_indentation() {
        let parent = Row {
            key: "harness".to_string(),
            depth: 0,
            state: RowState::Ready {
                what: "ready".to_string(),
                elapsed: Duration::from_millis(10),
            },
        };
        let child = Row {
            key: "queue".to_string(),
            depth: 1,
            state: RowState::Ready {
                what: "ready".to_string(),
                elapsed: Duration::from_millis(10),
            },
        };
        let drawn_parent = render_row(&parent, 0, true);
        let drawn_child = render_row(&child, 0, true);
        assert!(!drawn_parent.starts_with(' '), "{drawn_parent:?}");
        assert!(drawn_child.starts_with("  "), "{drawn_child:?}");
    }
}
