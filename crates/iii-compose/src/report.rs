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

use chrono::Local;
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
#[derive(Default)]
struct Console {
    startup: Option<StartupRows>,
    downloads: Vec<Row>,
    rows: Vec<Row>,
    /// How many lines the block occupies on screen, so the next draw knows how
    /// far up to go. Zero when nothing is drawn.
    drawn: usize,
    frame: usize,
    size: Option<(u16, u16)>,
    /// Once cursor coordinates become unreliable, keep this operation static.
    static_output: bool,
    rendered: Vec<Row>,
}

#[derive(Clone, PartialEq)]
struct Row {
    key: String,
    /// How far in it sits: a container is drawn under the one that waits for
    /// it, so a graph reads as what needs what.
    depth: usize,
    state: RowState,
}

#[derive(Clone, PartialEq)]
enum RowState {
    /// Declared, and waiting on something earlier in the graph.
    Waiting,
    Starting {
        what: String,
        began: Instant,
    },
    Retrying {
        attempt: u32,
        total: u32,
        phase: RetryPhase,
    },
    Downloading {
        downloaded: u64,
        total: Option<u64>,
        began: Instant,
    },
    Downloaded {
        downloaded: u64,
        total: Option<u64>,
        elapsed: Duration,
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

#[derive(Clone, PartialEq)]
enum RetryPhase {
    Waiting(Duration),
    Starting(String),
}

struct StartupRows {
    engine: Row,
    downloads: Row,
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
        *state = Console::default();
        state.startup = Some(StartupRows::new(managed));
        redraw(&mut state);
        if animated() {
            ensure_ticker();
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
        redraw(&mut state);
    }

    pub(crate) fn downloads_starting(&self) {
        let mut state = console().lock().unwrap_or_else(|p| p.into_inner());
        let Some(startup) = &mut state.startup else {
            return;
        };
        startup.downloads.state = RowState::Starting {
            what: "Checking".to_string(),
            began: Instant::now(),
        };
        redraw(&mut state);
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
        let settle = |row: &mut Row| match row.state {
            RowState::Waiting => row.state = RowState::Skipped("Not started".to_string()),
            RowState::Starting { .. } | RowState::Retrying { .. } => {
                row.state = RowState::Skipped("Cancelled".to_string());
            }
            RowState::Downloading { .. } if message == "Cancelled" => {
                row.state = RowState::Skipped("Cancelled".to_string());
            }
            RowState::Downloading { .. } => row.state = RowState::Failed,
            _ => {}
        };
        state.downloads.iter_mut().for_each(settle);
        state.rows.iter_mut().for_each(settle);
        redraw(&mut state);
        *state = Console::default();
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
            downloads: Row {
                key: "Downloads".to_string(),
                depth: 0,
                state: RowState::Waiting,
            },
            containers: Row {
                key: "Containers".to_string(),
                depth: 0,
                state: RowState::Waiting,
            },
        }
    }

    fn finish(&mut self, success: bool, message: &str) {
        let row = if !matches!(self.engine.state, RowState::Ready { .. })
            && matches!(self.downloads.state, RowState::Waiting)
        {
            self.downloads.state = RowState::Skipped("Not started".to_string());
            self.containers.state = RowState::Skipped("Not started".to_string());
            &mut self.engine
        } else if matches!(self.downloads.state, RowState::Starting { .. }) {
            if matches!(self.engine.state, RowState::Starting { .. }) {
                self.engine.state = RowState::Skipped("Not connected".to_string());
            }
            self.containers.state = RowState::Skipped("Not started".to_string());
            &mut self.downloads
        } else {
            // An empty project can finish even while an external engine is
            // unavailable. Completing that project does not prove connection.
            if matches!(self.engine.state, RowState::Starting { .. }) {
                self.engine.state = RowState::Skipped("Not connected".to_string());
            }
            if matches!(self.downloads.state, RowState::Waiting) {
                self.downloads.state = RowState::Skipped("No downloads".to_string());
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

/// Adds one registry artefact to the startup download section.
pub(crate) fn download_started(key: &str, total: Option<u64>) {
    let mut state = console().lock().unwrap_or_else(|p| p.into_inner());
    if state.startup.is_none() {
        return;
    }
    let began = Instant::now();
    if let Some(row) = state.downloads.iter_mut().find(|row| row.key == key) {
        row.state = RowState::Downloading {
            downloaded: 0,
            total,
            began,
        };
    } else {
        state.downloads.push(Row {
            key: key.to_string(),
            depth: 1,
            state: RowState::Downloading {
                downloaded: 0,
                total,
                began,
            },
        });
    }
    update_download_header(&mut state);
    redraw(&mut state);
}

/// Advances a registry artefact's live byte counter.
pub(crate) fn download_progress(key: &str, downloaded: u64) {
    let mut state = console().lock().unwrap_or_else(|p| p.into_inner());
    let Some(row) = state.downloads.iter_mut().find(|row| row.key == key) else {
        return;
    };
    let RowState::Downloading {
        downloaded: current,
        ..
    } = &mut row.state
    else {
        return;
    };
    *current = downloaded;
    if animated() {
        redraw(&mut state);
    }
}

/// Marks a registry artefact that could not be received or verified.
pub(crate) fn download_failed(key: &str) {
    let mut state = console().lock().unwrap_or_else(|p| p.into_inner());
    let Some(row) = state.downloads.iter_mut().find(|row| row.key == key) else {
        return;
    };
    if matches!(row.state, RowState::Downloading { .. }) {
        row.state = RowState::Failed;
        redraw(&mut state);
    }
}

/// Completes one registry artefact after its digest has been verified.
pub(crate) fn download_finished(key: &str, downloaded: u64) {
    let mut state = console().lock().unwrap_or_else(|p| p.into_inner());
    let Some(row) = state.downloads.iter_mut().find(|row| row.key == key) else {
        return;
    };
    let RowState::Downloading { total, began, .. } = &row.state else {
        return;
    };
    row.state = RowState::Downloaded {
        downloaded,
        total: *total,
        elapsed: began.elapsed(),
    };
    update_download_header(&mut state);
    redraw(&mut state);
}

fn update_download_header(state: &mut Console) {
    let complete = state
        .downloads
        .iter()
        .filter(|row| matches!(row.state, RowState::Downloaded { .. }))
        .count();
    let total = state.downloads.len();
    if let Some(startup) = &mut state.startup
        && let RowState::Starting { what, .. } = &mut startup.downloads.state
    {
        *what = format!("Downloading ({complete}/{total})");
    }
}

/// Closes the download section and starts the container section.
pub(crate) fn containers_starting() {
    let mut state = console().lock().unwrap_or_else(|p| p.into_inner());
    let download_count = state.downloads.len();
    let Some(startup) = &mut state.startup else {
        return;
    };
    let elapsed = match &startup.downloads.state {
        RowState::Starting { began, .. } => began.elapsed(),
        _ => Duration::ZERO,
    };
    startup.downloads.state = if download_count == 0 {
        RowState::Skipped("No downloads".to_string())
    } else {
        RowState::Ready {
            what: format!("Complete ({download_count})"),
            elapsed,
        }
    };
    startup.containers.state = RowState::Starting {
        what: "Starting".to_string(),
        began: Instant::now(),
    };
    redraw(&mut state);
}

fn console() -> &'static Mutex<Console> {
    static CONSOLE: OnceLock<Mutex<Console>> = OnceLock::new();
    CONSOLE.get_or_init(|| Mutex::new(Console::default()))
}

/// Announces what this operation will touch, in the shape it will touch it.
///
/// `rows` is `(container, depth)` in the order to draw. A terminal that cannot
/// animate gets state transitions instead of repeated spinner frames.
pub fn plan(rows: &[(String, usize)]) {
    {
        let console = console();
        let mut state = console
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.startup.is_none() {
            *state = Console::default();
        }
        state.rows = rows
            .iter()
            .map(|(key, depth)| Row {
                key: key.clone(),
                depth: *depth,
                state: RowState::Waiting,
            })
            .collect();
        state.frame = 0;
        redraw(&mut state);
    }
    if animated() {
        ensure_ticker();
    }
}

/// Releases the block so later output does not overwrite it.
pub fn plan_done() {
    let console = console();
    let mut state = console
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.startup.is_none() {
        *state = Console::default();
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

impl Console {
    fn observe_size(&mut self, size: Option<(u16, u16)>) {
        if self.drawn > 0 && self.size != size {
            // Resize can reflow the old block into scrollback. Preserve it and
            // emit only subsequent changes; its old cursor offset is unsafe.
            self.static_output = true;
            self.drawn = 0;
        }
        self.size = size;
    }

    fn clear_block(&mut self, out: &mut String) {
        if self.drawn == 0 {
            return;
        }
        out.push_str(&format!("\x1b[{}A", self.drawn));
        for _ in 0..self.drawn {
            out.push_str(CLEAR_LINE);
            out.push('\n');
        }
        out.push_str(&format!("\x1b[{}A", self.drawn));
        self.drawn = 0;
        self.rendered.clear();
    }

    fn render(&mut self, size: Option<(u16, u16)>) -> String {
        self.observe_size(size);
        let mut rows = Vec::new();
        if let Some(startup) = &self.startup {
            rows.extend([startup.engine.clone(), startup.downloads.clone()]);
            rows.extend(self.downloads.iter().cloned());
            rows.push(startup.containers.clone());
            rows.extend(self.rows.iter().cloned().map(|mut row| {
                row.depth += 1;
                row
            }));
        } else {
            rows.extend(self.downloads.iter().cloned());
            rows.extend(self.rows.iter().cloned());
        }
        let lines: Vec<String> = rows
            .iter()
            .map(|row| render_row(row, self.frame, true))
            .collect();
        let height = size.and_then(|(height, width)| {
            let needed = panel_height(&lines, width)?;
            // The last newline leaves the cursor on a row below the panel.
            (needed < usize::from(height)).then_some(needed)
        });
        let mut out = String::new();
        if self.static_output || height.is_none() {
            if self.drawn > 0 {
                // This is a larger new frame, not a resize. The previous
                // frame still fits, so replace it with a static snapshot.
                self.clear_block(&mut out);
            }
            self.static_output = true;
            for row in &rows {
                if !self.rendered.contains(row) {
                    out.push_str(&render_row(row, 0, false));
                    out.push('\n');
                }
            }
        } else {
            self.clear_block(&mut out);
            for line in &lines {
                out.push_str(CLEAR_LINE);
                out.push_str(line);
                out.push('\n');
            }
            self.drawn = height.unwrap_or_default();
        }
        self.rendered = rows;
        out
    }

    fn line(&mut self, text: &str, size: Option<(u16, u16)>) -> String {
        self.observe_size(size);
        let mut out = String::new();
        self.clear_block(&mut out);
        out.push_str(text);
        out.push('\n');
        out.push_str(&self.render(size));
        out
    }
}

/// Animate only complete, unwrapped rows. Wide characters can wrap before the
/// last column, so dividing a string's display width by the terminal width is
/// not a reliable cursor offset.
fn panel_height(lines: &[String], width: u16) -> Option<usize> {
    if width < 2 {
        return None;
    }
    lines
        .iter()
        .all(|line| {
            !console::strip_ansi_codes(line)
                .chars()
                .any(char::is_control)
                && console::measure_text_width(line) < usize::from(width)
        })
        .then_some(lines.len())
}

fn terminal_size() -> Option<(u16, u16)> {
    animated()
        .then(|| console::Term::stderr().size_checked())
        .flatten()
}

/// Redraws only while the entire block and its cursor fit in the terminal.
fn redraw(state: &mut Console) {
    let out = state.render(terminal_size());
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
        RowState::Downloading {
            downloaded,
            total,
            began,
        } => render_download(
            row,
            *downloaded,
            *total,
            began.elapsed(),
            false,
            frame,
            animate,
        ),
        RowState::Downloaded {
            downloaded,
            total,
            elapsed,
        } => render_download(row, *downloaded, *total, *elapsed, true, frame, animate),
        RowState::Retrying {
            attempt,
            total,
            phase,
        } => {
            let phase = retry_label(*attempt, *total, phase);
            format!(
                "{indent}{} {} {}",
                if animate {
                    FRAMES[frame % FRAMES.len()]
                } else {
                    RUNNING
                }
                .cyan(),
                row.key.bold(),
                phase.dimmed(),
            )
        }
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

fn render_download(
    row: &Row,
    downloaded: u64,
    total: Option<u64>,
    elapsed: Duration,
    finished: bool,
    frame: usize,
    animate: bool,
) -> String {
    let indent = "  ".repeat(row.depth);
    let bar = download_bar(downloaded, total, frame, animate);
    let amount = match total.filter(|total| *total > 0) {
        Some(total) => format!(
            "{:>3}% {}/{}",
            ((u128::from(downloaded.min(total)) * 100) / u128::from(total)) as u8,
            format_bytes(downloaded),
            format_bytes(total)
        ),
        None => format_bytes(downloaded),
    };
    let speed = format!("{}/s", format_bytes(download_speed(downloaded, elapsed)));
    let marker = if finished {
        OK.green()
    } else if animate {
        FRAMES[frame % FRAMES.len()].cyan()
    } else {
        RUNNING.cyan()
    };
    let details = format!("{bar} {amount} {speed}");
    if finished {
        format!("{indent}{marker} {} {}", row.key.bold(), details.green())
    } else {
        format!("{indent}{marker} {} {}", row.key.bold(), details.dimmed())
    }
}

fn download_bar(downloaded: u64, total: Option<u64>, frame: usize, animate: bool) -> String {
    const WIDTH: usize = 20;
    let Some(total) = total.filter(|total| *total > 0) else {
        let position = if animate { frame % WIDTH } else { 0 };
        let mut cells = vec![' '; WIDTH];
        cells[position] = '>';
        return format!("[{}]", cells.into_iter().collect::<String>());
    };
    let filled = ((u128::from(downloaded.min(total)) * WIDTH as u128) / u128::from(total)) as usize;
    let mut bar = "=".repeat(filled);
    if filled < WIDTH {
        bar.push('>');
        bar.push_str(&" ".repeat(WIDTH - filled - 1));
    }
    format!("[{bar}]")
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

fn download_speed(downloaded: u64, elapsed: Duration) -> u64 {
    if elapsed.is_zero() {
        return 0;
    }
    (downloaded as f64 / elapsed.as_secs_f64()) as u64
}

/// Whether progress can animate. A pipe or a file gets static lines.
fn animated() -> bool {
    static ANIMATED: OnceLock<bool> = OnceLock::new();
    *ANIMATED.get_or_init(|| {
        std::io::stderr().is_terminal() && !std::env::var("TERM").is_ok_and(|term| term == "dumb")
    })
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
    let out = state.line(text, terminal_size());
    let mut stderr = std::io::stderr().lock();
    let _ = write!(stderr, "{out}");
    let _ = stderr.flush();
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
                let turning = !state.static_output
                    && (state.startup.is_some()
                        || state
                            .downloads
                            .iter()
                            .any(|row| matches!(row.state, RowState::Downloading { .. }))
                        || state.rows.iter().any(|row| {
                            matches!(
                                row.state,
                                RowState::Starting { .. } | RowState::Retrying { .. }
                            )
                        }));
                if turning {
                    state.frame = state.frame.wrapping_add(1);
                    redraw(&mut state);
                }
            }
        });
    });
}

/// Records the local time a restart begins outside the animated progress block.
pub(crate) fn restarting(key: &str) {
    line(&format!(
        "{} {} {}",
        RUNNING.dimmed(),
        key.bold(),
        format!("restarting at {}", Local::now().format("%H:%M:%S")).dimmed()
    ));
}

/// A container is being worked on. On a terminal this spins until the container
/// settles; anywhere else it is a plain line.
pub fn starting(key: &str, what: &str) {
    let retry = {
        let state = console().lock().unwrap_or_else(|p| p.into_inner());
        state.rows.iter().find_map(|row| match &row.state {
            RowState::Retrying {
                attempt,
                total,
                phase,
            } if row.key == key => Some((
                *attempt,
                *total,
                matches!(phase, RetryPhase::Starting(current) if current == what),
            )),
            _ => None,
        })
    };
    if let Some((_, _, true)) = retry {
        return;
    }
    if let Some((attempt, total, false)) = retry {
        show_retry(key, attempt, total, RetryPhase::Starting(what.to_string()));
        return;
    }

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

/// Keeps a supervised restart visible while its next attempt is backing off.
pub(crate) fn retry_waiting(key: &str, attempt: u32, total: u32, delay: Duration) {
    show_retry(key, attempt, total, RetryPhase::Waiting(delay));
}

/// Starts one supervised attempt on the row created during its backoff.
pub(crate) fn retry_starting(key: &str, attempt: u32, total: u32) {
    show_retry(
        key,
        attempt,
        total,
        RetryPhase::Starting("starting".to_string()),
    );
}

/// Leaves the final supervised attempt as a completed terminal line.
pub(crate) fn retry_recovered(key: &str, attempt: u32, total: u32, elapsed: Duration) {
    let row = Row {
        key: key.to_string(),
        depth: 0,
        state: RowState::Ready {
            what: recovered_label(attempt, total),
            elapsed,
        },
    };
    show_retry_row(row);
}

fn show_retry(key: &str, attempt: u32, total: u32, phase: RetryPhase) {
    let row = Row {
        key: key.to_string(),
        depth: 0,
        state: RowState::Retrying {
            attempt,
            total,
            phase,
        },
    };
    show_retry_row(row);
}

fn show_retry_row(row: Row) {
    // During `up`, keep this row inside the dependency tree and preserve its
    // depth. A run-time retry has no active plan, so it falls through and owns
    // a one-row block as before.
    if set(&row.key, row.state.clone()) {
        return;
    }

    {
        let mut state = console()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.rows = vec![row];
        state.frame = 0;
        redraw(&mut state);
    }
    if animated() {
        ensure_ticker();
    }
}

fn retry_label(attempt: u32, total: u32, phase: &RetryPhase) -> String {
    match phase {
        RetryPhase::Waiting(delay) => {
            format!(
                "Retrying {attempt}/{total}, waiting {}",
                format_elapsed(*delay)
            )
        }
        RetryPhase::Starting(what) => format!("Retrying {attempt}/{total}, {what}"),
    }
}

fn recovered_label(attempt: u32, total: u32) -> String {
    format!("Recovered on attempt {attempt}/{total}")
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

pub fn lock_changed(path: &std::path::Path, created: bool) {
    let change = if created { "created" } else { "updated" };
    line(&format!(
        "{} {} {}",
        OK.green(),
        path.display().to_string().bold(),
        change.green()
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

/// Containers that failed with an effective `required` value of `false`.
/// Printed before the closing line so a partial project does not read as a
/// clean start.
pub fn not_required_failed(containers: &[String]) {
    let names = containers
        .iter()
        .map(|container| format!("'{container}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let body = if containers.len() == 1 {
        format!("container {names} failed and is not required: the project is up without it")
    } else {
        format!("containers {names} failed and are not required: the project is up without them")
    };
    line(&body.yellow().to_string());
}

/// Closing line of an operation.
pub fn summary_ok(action: &str, changed: usize, total: usize, elapsed: Duration) {
    let body = format!("{action}: {changed} of {total} changed");
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
#[path = "report/terminal_tests.rs"]
mod terminal_tests;

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
        startup.downloads.state = RowState::Skipped("No downloads".to_string());
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
            .env_remove("CLICOLOR_FORCE")
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        let starting = stderr.find("Engine Starting").unwrap();
        let waiting = stderr.find("Engine Waiting for connection").unwrap();
        let ready = stderr.find("Engine Ready").unwrap();
        let checking = stderr.find("Downloads Checking").unwrap();
        let downloading = stderr.find("Downloads Downloading (0/1)").unwrap();
        let complete = stderr.find("Downloads Complete (1)").unwrap();
        let containers = stderr.find("Containers Starting").unwrap();
        let done = stderr.find("Containers Ready").unwrap();
        assert!(
            starting < waiting
                && waiting < ready
                && ready < checking
                && checking < downloading
                && downloading < complete
                && complete < containers
                && containers < done,
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
        progress.downloads_starting();
        download_started("api", Some(2 * 1024 * 1024));
        download_progress("api", 1024 * 1024);
        download_finished("api", 2 * 1024 * 1024);
        containers_starting();
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

    /// Used by the VHS PR demo to record the real startup progress renderer.
    #[tokio::test]
    #[ignore = "subprocess fixture for the download progress renderer"]
    async fn download_panel_fixture() {
        let mut progress = StartupProgress::start(false);
        tokio::time::sleep(Duration::from_millis(350)).await;
        progress.engine_ready();
        progress.downloads_starting();
        download_started("console", Some(12 * 1024 * 1024));
        download_started("shell", Some(8 * 1024 * 1024));
        for step in 1..=12 {
            download_progress("console", step * 1024 * 1024);
            download_progress("shell", step.min(8) * 1024 * 1024);
            if step == 8 {
                download_finished("shell", 8 * 1024 * 1024);
            }
            tokio::time::sleep(Duration::from_millis(140)).await;
        }
        download_finished("console", 12 * 1024 * 1024);
        tokio::time::sleep(Duration::from_millis(300)).await;
        containers_starting();
        plan(&[("console".to_string(), 0), ("shell".to_string(), 0)]);
        starting("console", "waiting for engine registration");
        starting("shell", "waiting for engine registration");
        tokio::time::sleep(Duration::from_millis(400)).await;
        ready("shell", Duration::from_millis(400));
        tokio::time::sleep(Duration::from_millis(250)).await;
        ready("console", Duration::from_millis(650));
        plan_done();
        progress.finish(true, "Ready");
        tokio::time::sleep(Duration::from_millis(600)).await;
    }

    #[test]
    fn redirected_retry_reports_transitions_without_animation() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "report::tests::retry_panel_fixture",
                "--nocapture",
            ])
            .env_remove("CLICOLOR_FORCE")
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        let waiting = stderr.find("api Retrying 2/5, waiting 1.0s").unwrap();
        let starting = stderr.find("api Retrying 2/5, starting").unwrap();
        let configuring = stderr.find("api Retrying 2/5, configuring").unwrap();
        let recovered = stderr.find("api Recovered on attempt 2/5 (1.8s)").unwrap();
        assert!(
            waiting < starting && starting < configuring && configuring < recovered,
            "{stderr}"
        );
        assert_eq!(
            stderr.matches("api Retrying 2/5, starting").count(),
            1,
            "{stderr}"
        );
        assert!(
            !stderr.contains('\x1b') && !FRAMES.iter().any(|frame| stderr.contains(frame)),
            "{stderr}"
        );
    }

    /// Also usable under a PTY to inspect the retry redraws.
    #[tokio::test]
    #[ignore = "subprocess fixture for the retry progress renderer"]
    async fn retry_panel_fixture() {
        plan(&[("api".to_string(), 0)]);
        retry_waiting("api", 2, 5, Duration::from_secs(1));
        tokio::time::sleep(Duration::from_millis(100)).await;
        retry_starting("api", 2, 5);
        starting("api", "starting");
        starting("api", "configuring");
        tokio::time::sleep(Duration::from_millis(100)).await;
        retry_recovered("api", 2, 5, Duration::from_millis(1800));
        plan_done();
    }

    #[test]
    fn waiting_retry_names_the_next_attempt_and_delay() {
        assert_eq!(
            retry_label(2, 5, &RetryPhase::Waiting(Duration::from_secs(1))),
            "Retrying 2/5, waiting 1.0s"
        );
    }

    #[test]
    fn active_retry_names_the_attempt_and_phase() {
        assert_eq!(
            retry_label(2, 5, &RetryPhase::Starting("configuring".to_string())),
            "Retrying 2/5, configuring"
        );
    }

    #[test]
    fn recovered_retry_names_the_successful_attempt() {
        assert_eq!(recovered_label(2, 5), "Recovered on attempt 2/5");
    }

    #[test]
    fn known_download_size_renders_progress_amount_and_transfer_speed() {
        let row = Row {
            key: "console".to_string(),
            depth: 1,
            state: RowState::Downloading {
                downloaded: 5 * 1024 * 1024,
                total: Some(10 * 1024 * 1024),
                began: Instant::now() - Duration::from_secs(2),
            },
        };

        let rendered = render_row(&row, 0, false);

        assert!(rendered.contains("[==========>         ]"), "{rendered}");
        assert!(rendered.contains("50% 5.0 MiB/10.0 MiB"), "{rendered}");
        assert!(rendered.contains("2.5 MiB/s"), "{rendered}");
    }

    #[test]
    fn unknown_download_size_renders_activity_bytes_and_transfer_speed() {
        let row = Row {
            key: "shell".to_string(),
            depth: 1,
            state: RowState::Downloading {
                downloaded: 2 * 1024,
                total: None,
                began: Instant::now() - Duration::from_secs(2),
            },
        };

        let rendered = render_row(&row, 4, true);

        assert!(rendered.contains("[    >               ]"), "{rendered}");
        assert!(rendered.contains("2.0 KiB"), "{rendered}");
        assert!(rendered.ends_with("/s"), "{rendered}");
    }

    #[test]
    fn download_speed_uses_binary_bytes_per_second() {
        assert_eq!(
            download_speed(5 * 1024 * 1024, Duration::from_secs(2)),
            2_621_440
        );
    }

    #[test]
    fn completed_download_keeps_its_final_transfer_speed() {
        let row = Row {
            key: "state".to_string(),
            depth: 1,
            state: RowState::Downloaded {
                downloaded: 3 * 1024 * 1024,
                total: Some(3 * 1024 * 1024),
                elapsed: Duration::from_secs(3),
            },
        };

        let first = render_row(&row, 0, false);
        std::thread::sleep(Duration::from_millis(5));
        let second = render_row(&row, 0, false);

        assert_eq!(first, second);
        assert!(first.contains("100% 3.0 MiB/3.0 MiB 1.0 MiB/s"), "{first}");
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
