// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! What the caller of `compose::add` sees of it on its own terminal.
//!
//! [`crate::report`] draws for the daemon's terminal, which the caller of `iii call` does not
//! see: without a stream, `compose::add` answers with an operation id and finishes in the
//! background. When the call binds a `stderr` stream, the daemon draws the operation into it,
//! for the terminal `iii call` describes in `terminal`: one row per container it starts,
//! redrawn while the operation runs when standard error is a terminal, and one line per change
//! when it is a pipe or a file, because a log full of spinner frames is worse than no progress.
//!
//! The caller leaving (the first Ctrl+C of `iii call`) is a cancellation: the next write fails,
//! the operation is cancelled, and its events are still read to the end, unwritten, so the
//! handler answers once the operation finished.
//!
//! Colour is written as escape codes here, not with `colored`: that crate decides from the
//! daemon's own stdout, and this is the caller's terminal.

use std::{
    fmt::Write as _,
    time::{Duration, Instant},
};

use iii_sdk::OutputStream;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::{
    operation::{Operation, ProgressEvent},
    report::format_elapsed,
};

/// What `iii call` says of its terminal, in the request field marked `x-iii-terminal`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CallerTerminal {
    /// Standard output is a terminal that can redraw a line.
    pub stdout: bool,
    /// Standard error is a terminal that can redraw a line.
    pub stderr: bool,
    /// Width of the terminal in columns, when known.
    pub columns: Option<u16>,
    /// Height of the terminal in rows, when known.
    pub rows: Option<u16>,
    /// Colour is wanted: `NO_COLOR` is not set.
    pub color: bool,
}

/// Braille frames, as the daemon's own spinner: one cell wide in every terminal font.
const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Fast enough to read as motion, slow enough not to churn a remote terminal.
const FRAME_INTERVAL: Duration = Duration::from_millis(90);

/// How often a caller that gets no frames is checked for having left. The check writes nothing.
const PROBE_INTERVAL: Duration = Duration::from_millis(250);

/// What reconciliation says of a container this operation did not start. Its row is not drawn:
/// an add into a project of fourteen workers is about the ones it adds.
const ALREADY_RUNNING: &str = "already running";

/// The terminal's size when `iii call` could not read it.
const DEFAULT_COLUMNS: usize = 80;
const DEFAULT_ROWS: usize = 24;

/// How the operation ended, as its last line says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    Succeeded(String),
    /// It succeeded, and something not required did not: worth a look, nothing broke.
    Partial(String),
    Failed(String),
    Cancelled(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RowState {
    Working(String),
    Done(String),
    Failed(String),
    /// Started, then stopped again because the operation failed.
    Undone(String),
}

#[derive(Debug, Clone)]
struct Row {
    key: String,
    state: RowState,
    began: Instant,
    /// How long it took to finish, once it did.
    took: Option<Duration>,
}

/// The containers of one operation, in the order they started.
#[derive(Debug, Default)]
struct Rows {
    rows: Vec<Row>,
    /// What the operation as a whole said last, such as resolving its dependency trees.
    phase: Option<String>,
}

impl Rows {
    /// Applies one container event. Returns the row when what it says changed.
    fn apply(&mut self, key: &str, phase: &str, detail: &str, now: Instant) -> Option<&Row> {
        let detail = one_line(detail);
        let existing = self.rows.iter().position(|row| row.key == key);
        let state = match phase {
            // The plan, before anything moves: a row appears once its container does.
            "waiting" => return None,
            "ready" if detail == ALREADY_RUNNING && existing.is_none() => return None,
            "ready" | "stopped" | "removed" => RowState::Done(phase.to_string()),
            "updated" | "unchanged" | "built" | "skipped" => RowState::Done(detail),
            "failed" => RowState::Failed(detail),
            "rolled_back" => RowState::Undone("rolled back".to_string()),
            _ => RowState::Working(detail),
        };
        let index = existing.unwrap_or_else(|| {
            self.rows.push(Row {
                key: key.to_string(),
                state: RowState::Working(String::new()),
                began: now,
                took: None,
            });
            self.rows.len() - 1
        });
        let row = &mut self.rows[index];
        if row.state == state {
            return None;
        }
        row.took = match state {
            RowState::Working(_) => None,
            RowState::Done(_) | RowState::Failed(_) | RowState::Undone(_) => {
                Some(now.saturating_duration_since(row.began))
            }
        };
        row.state = state;
        Some(row)
    }
}

/// Escape codes, or nothing when the caller wants no colour.
#[derive(Debug, Clone, Copy)]
struct Paint(bool);

impl Paint {
    fn wrap(self, code: &str, text: &str) -> String {
        if self.0 {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
    fn bold(self, text: &str) -> String {
        self.wrap("1", text)
    }
    fn dim(self, text: &str) -> String {
        self.wrap("2", text)
    }
    fn red(self, text: &str) -> String {
        self.wrap("31", text)
    }
    fn green(self, text: &str) -> String {
        self.wrap("32", text)
    }
    fn amber(self, text: &str) -> String {
        self.wrap("33", text)
    }
    fn cyan(self, text: &str) -> String {
        self.wrap("36", text)
    }
}

/// The first line of `text`, its other control characters spaces: a row is one line on screen,
/// and a detail must not move the cursor of the block it is drawn in.
fn one_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// A row in the block: turning while it works when `frame` is given, still otherwise.
fn row_line(row: &Row, paint: Paint, frame: Option<usize>, now: Instant) -> String {
    let key = paint.bold(&row.key);
    match &row.state {
        RowState::Working(detail) => {
            let mark = match frame {
                Some(frame) => paint.cyan(FRAMES[frame % FRAMES.len()]),
                None => paint.dim("·"),
            };
            let elapsed = format_elapsed(now.saturating_duration_since(row.began));
            format!(
                "  {mark} {key} {} {}",
                paint.dim(detail),
                paint.dim(&format!("({elapsed})"))
            )
        }
        RowState::Done(detail) => format!(
            "  {} {key} {} {}",
            paint.green("✓"),
            paint.dim(detail),
            paint.dim(&format!(
                "({})",
                format_elapsed(row.took.unwrap_or_default())
            ))
        ),
        RowState::Failed(detail) => format!("  {} {key} {}", paint.red("✗"), paint.red(detail)),
        RowState::Undone(detail) => format!("  {} {key} {}", paint.amber("·"), paint.dim(detail)),
    }
}

/// A row as one line of a log: what changed, without colour or cursor movement.
fn plain_line(row: &Row) -> String {
    match &row.state {
        RowState::Working(detail) => format!("{}: {detail}", row.key),
        RowState::Done(detail) => format!(
            "{}: {detail} ({})",
            row.key,
            format_elapsed(row.took.unwrap_or_default())
        ),
        RowState::Failed(detail) => format!("{}: failed: {detail}", row.key),
        RowState::Undone(detail) => format!("{}: {detail}", row.key),
    }
}

/// `container │`, dim when `color`: how a line of output names the worker it comes from.
pub fn label(container: &str, color: bool) -> String {
    Paint(color).dim(&format!("{container} │"))
}

/// A line of output as it can be printed above a block: control characters, escape sequences
/// among them, become spaces, so the line cannot move the cursor the block is drawn from.
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Draws one operation into the caller's `stderr` stream.
pub struct CallerReport {
    stream: OutputStream,
    terminal: CallerTerminal,
    title: String,
    rows: Rows,
    began: Instant,
    frame: usize,
    /// Lines the block takes on screen, so the next draw knows how far up to go.
    drawn: usize,
    /// The caller stopped reading: nothing is written any more.
    left: bool,
}

impl CallerReport {
    /// Starts drawing an operation that `title` names, such as `adding 2 requested workers`.
    pub async fn start(
        stream: OutputStream,
        terminal: CallerTerminal,
        title: impl Into<String>,
    ) -> Self {
        let title = one_line(&title.into());
        let mut report = Self {
            stream,
            terminal,
            // A phase that only says the title again is not printed twice.
            rows: Rows {
                phase: Some(title.clone()),
                ..Rows::default()
            },
            title,
            began: Instant::now(),
            frame: 0,
            drawn: 0,
            left: false,
        };
        let opening = if report.animated() {
            report.draw()
        } else {
            format!("{}\n", report.title)
        };
        report.write(opening).await;
        report
    }

    /// Whether the caller stopped reading.
    pub fn left(&self) -> bool {
        self.left
    }

    /// Draws the events of `operation` until its terminal one. Once the caller left, the
    /// operation is cancelled, and the rest of its events are read without being drawn.
    pub async fn follow(
        &mut self,
        operation: &Operation,
        events: UnboundedReceiver<ProgressEvent>,
    ) {
        self.follow_with(operation, events, None).await;
    }

    /// [`Self::follow`], printing each `(container, line)` of `output` as it comes: above the
    /// block on a terminal, as a line of its own in a pipe.
    pub async fn follow_with(
        &mut self,
        operation: &Operation,
        mut events: UnboundedReceiver<ProgressEvent>,
        mut output: Option<&mut UnboundedReceiver<(String, String)>>,
    ) {
        let mut ticks = tokio::time::interval(if self.animated() {
            FRAME_INTERVAL
        } else {
            PROBE_INTERVAL
        });
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut cancelled = false;
        loop {
            tokio::select! {
                // Events first when a line is ready too: the ending is never held up by output,
                // and what is left of it is printed after, by `print_rest`.
                biased;
                event = events.recv() => match event {
                    Some(event) if !event.terminal => self.apply(event).await,
                    _ => break,
                },
                line = async {
                    match output.as_mut() {
                        Some(output) => output.recv().await,
                        None => std::future::pending().await,
                    }
                } => match line {
                    Some((container, line)) => self.print(&container, &line).await,
                    // Its source ended: nothing more comes from it.
                    None => output = None,
                },
                _ = ticks.tick() => self.tick().await,
            }
            if self.left && !cancelled {
                operation.cancel();
                cancelled = true;
            }
        }
    }

    /// Prints what is left of `output`, once its source ended.
    pub async fn print_rest(&mut self, output: &mut UnboundedReceiver<(String, String)>) {
        while let Some((container, line)) = output.recv().await {
            self.print(&container, &line).await;
        }
    }

    /// Prints one line of `container`'s output, labelled with it.
    async fn print(&mut self, container: &str, line: &str) {
        let label = label(container, self.paint().0);
        self.say(&format!("{label} {}", printable(line))).await;
    }

    /// Ends the drawing with the line of `ending`, and closes the stream.
    pub async fn finish(mut self, ending: &Ending) {
        let paint = self.paint();
        let now = Instant::now();
        let mut out = String::new();
        if self.animated() {
            // The last frame stays on screen: every row, still, and not cut to the width, since
            // nothing redraws it.
            out.push_str(&self.erase());
            for row in &self.rows.rows {
                out.push_str(&row_line(row, paint, None, now));
                out.push('\n');
            }
        }
        let (mark, text) = match ending {
            Ending::Succeeded(text) => (paint.green("✓"), text),
            Ending::Partial(text) => (paint.amber("!"), text),
            Ending::Failed(text) => (paint.red("✗"), text),
            Ending::Cancelled(text) => (paint.amber("·"), text),
        };
        let elapsed = format_elapsed(now.saturating_duration_since(self.began));
        let _ = writeln!(
            out,
            "{mark} {} {}",
            one_line(text),
            paint.dim(&format!("({elapsed})"))
        );
        self.write(out).await;
        if !self.left {
            let _ = self.stream.close().await;
        }
    }

    fn animated(&self) -> bool {
        self.terminal.stderr
    }

    fn paint(&self) -> Paint {
        Paint(self.terminal.stderr && self.terminal.color)
    }

    async fn apply(&mut self, event: ProgressEvent) {
        match event.container.as_deref() {
            None => {
                let detail = one_line(&event.detail);
                if detail.is_empty() || self.rows.phase.as_deref() == Some(detail.as_str()) {
                    return;
                }
                self.rows.phase = Some(detail.clone());
                if !self.animated() {
                    self.write(format!("{detail}\n")).await;
                }
            }
            Some(_) if event.phase == "warning" => {
                let line = format!(
                    "{} {}",
                    self.paint().amber("warning:"),
                    one_line(&event.detail)
                );
                self.say(&line).await;
            }
            Some(key) => {
                let line = self
                    .rows
                    .apply(key, &event.phase, &event.detail, Instant::now())
                    .map(plain_line);
                if let Some(line) = line
                    && !self.animated()
                {
                    self.write(format!("{line}\n")).await;
                }
            }
        }
    }

    /// Writes a line that stays, above the block when one is drawn.
    async fn say(&mut self, line: &str) {
        let out = if self.animated() {
            let mut out = self.erase();
            out.push_str(line);
            out.push('\n');
            out.push_str(&self.draw());
            out
        } else {
            format!("{line}\n")
        };
        self.write(out).await;
    }

    async fn tick(&mut self) {
        if self.animated() {
            self.frame = self.frame.wrapping_add(1);
            let mut out = self.erase();
            out.push_str(&self.draw());
            self.write(out).await;
        } else if !self.left && self.stream.write(&[]).await.is_err() {
            // An empty write sends nothing, and fails once the reader left.
            self.left = true;
        }
    }

    /// The block: what the operation does, then a row per container. Every line is cut to the
    /// width, and the block to the height, so going back up over it lands where it began.
    fn draw(&mut self) -> String {
        let paint = self.paint();
        let now = Instant::now();
        let width = self
            .terminal
            .columns
            .map_or(DEFAULT_COLUMNS, usize::from)
            .max(2)
            - 1;
        // The line under the block holds the cursor.
        let room = self.terminal.rows.map_or(DEFAULT_ROWS, usize::from).max(4) - 2;

        let mut header = format!(
            "{} {}",
            paint.cyan(FRAMES[self.frame % FRAMES.len()]),
            self.title
        );
        if self.rows.rows.is_empty()
            && let Some(phase) = &self.rows.phase
            && phase != &self.title
        {
            let _ = write!(header, " {}", paint.dim(&format!("· {phase}")));
        }
        let elapsed = format_elapsed(now.saturating_duration_since(self.began));
        let _ = write!(header, " {}", paint.dim(&format!("({elapsed})")));

        let mut lines = vec![header];
        let rows = &self.rows.rows;
        if rows.len() <= room {
            lines.extend(
                rows.iter()
                    .map(|row| row_line(row, paint, Some(self.frame), now)),
            );
        } else {
            // What still moves, or failed, is what an operator watches; the finished are counted.
            let shown: Vec<&Row> = rows
                .iter()
                .filter(|row| !matches!(row.state, RowState::Done(_)))
                .take(room - 1)
                .collect();
            let more = rows.len() - shown.len();
            lines.extend(
                shown
                    .iter()
                    .map(|row| row_line(row, paint, Some(self.frame), now)),
            );
            lines.push(paint.dim(&format!("  … {more} more")));
        }

        self.drawn = lines.len();
        let mut out = String::new();
        for line in lines {
            out.push_str(&console::truncate_str(&line, width, "…"));
            out.push('\n');
        }
        out
    }

    /// Goes back up over the block and clears it.
    fn erase(&mut self) -> String {
        match std::mem::take(&mut self.drawn) {
            0 => String::new(),
            drawn => format!("\x1b[{drawn}A\r\x1b[J"),
        }
    }

    async fn write(&mut self, text: String) {
        if self.left || text.is_empty() {
            return;
        }
        if self.stream.write(text.as_bytes()).await.is_err() {
            self.left = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        pin::Pin,
        sync::{Arc, Mutex},
        task::{Context, Poll},
    };

    use iii_sdk::IIIClient;
    use tokio::io::AsyncWrite;

    use super::*;
    use crate::operation::{OperationManager, OperationStatus};

    /// Everything written into an [`OutputStream::to_writer`] sink, or a sink whose reader left.
    #[derive(Clone, Default)]
    struct Sink {
        written: Arc<Mutex<Vec<u8>>>,
        gone: bool,
    }

    impl Sink {
        fn text(&self) -> String {
            String::from_utf8(self.written.lock().unwrap().clone()).unwrap()
        }
    }

    impl AsyncWrite for Sink {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            if self.gone {
                return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
            }
            self.written.lock().unwrap().extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    async fn operation() -> (IIIClient, Arc<Operation>) {
        let client = IIIClient::new("ws://127.0.0.1:1/ws");
        let operation = OperationManager::new(&client).create(1).await;
        (client, operation)
    }

    /// Draws an add of `state` that starts it and finishes, into `sink` for `terminal`.
    async fn draw_an_add(sink: &Sink, terminal: CallerTerminal) {
        let (client, operation) = operation().await;
        let events = operation.subscribe();
        let mut report = CallerReport::start(
            OutputStream::to_writer(sink.clone()),
            terminal,
            "adding 1 worker",
        )
        .await;
        let emitted = async {
            operation
                .emit(None, "resolving", "resolving dependency trees")
                .await;
            operation
                .emit_tree("api", 0, "waiting for dependencies")
                .await;
            operation
                .emit_tree("state", 1, "waiting for dependencies")
                .await;
            operation.emit(Some("api"), "ready", ALREADY_RUNNING).await;
            operation
                .emit(Some("state"), "starting", "starting worker")
                .await;
            // Long enough for frames to be drawn over each other.
            tokio::time::sleep(Duration::from_millis(250)).await;
            operation
                .emit(Some("state"), "warning", "'kv' is an alias of 'state'")
                .await;
            operation
                .emit(Some("state"), "ready", "registered with engine")
                .await;
            operation
                .finish(
                    OperationStatus::Succeeded,
                    "all requested workers are ready",
                )
                .await;
        };
        tokio::join!(report.follow(&operation, events), emitted);
        report
            .finish(&Ending::Succeeded("all requested workers are ready".into()))
            .await;
        client.shutdown_async().await;
    }

    #[test]
    fn the_terminal_iii_call_describes_deserializes() {
        let terminal: CallerTerminal = serde_json::from_value(serde_json::json!({
            "stdout": false, "stderr": true, "columns": 120, "rows": 40, "color": true,
        }))
        .unwrap();
        assert_eq!(
            terminal,
            CallerTerminal {
                stdout: false,
                stderr: true,
                columns: Some(120),
                rows: Some(40),
                color: true,
            }
        );
    }

    #[test]
    fn a_row_appears_once_its_container_starts_and_never_for_one_already_running() {
        let now = Instant::now();
        let mut rows = Rows::default();
        assert!(
            rows.apply("api", "waiting", "waiting for dependencies", now)
                .is_none()
        );
        assert!(rows.apply("api", "ready", ALREADY_RUNNING, now).is_none());
        assert!(rows.rows.is_empty());

        let started = rows
            .apply("state", "starting", "starting worker", now)
            .cloned();
        assert_eq!(
            started.map(|row| row.state),
            Some(RowState::Working("starting worker".into()))
        );
        assert!(
            rows.apply("state", "starting", "starting worker", now)
                .is_none(),
            "the same state again is no change"
        );
        let later = now + Duration::from_millis(1200);
        let ready = rows
            .apply("state", "ready", "registered with engine", later)
            .unwrap();
        assert_eq!(ready.state, RowState::Done("ready".into()));
        assert_eq!(ready.took, Some(Duration::from_millis(1200)));
        assert_eq!(plain_line(ready), "state: ready (1.2s)");
    }

    #[test]
    fn stopping_removing_and_rolling_back_end_a_row() {
        let now = Instant::now();
        let mut rows = Rows::default();
        rows.apply("api", "stopping", "stopping", now);
        let stopped = rows.apply("api", "stopped", "stopped", now).unwrap();
        assert_eq!(stopped.state, RowState::Done("stopped".into()));
        let removed = rows.apply("api", "removed", "removed", now).unwrap();
        assert_eq!(plain_line(removed), "api: removed (0ms)");

        rows.apply("web", "ready", "registered with engine", now);
        let undone = rows
            .apply("web", "rolled_back", "rolled back", now)
            .unwrap();
        assert_eq!(undone.state, RowState::Undone("rolled back".into()));
        assert_eq!(plain_line(undone), "web: rolled back");
    }

    #[tokio::test]
    async fn output_lines_are_labelled_and_printed_above_the_progress() {
        let (client, operation) = operation().await;
        let events = operation.subscribe();
        let sink = Sink::default();
        let mut report = CallerReport::start(
            OutputStream::to_writer(sink.clone()),
            CallerTerminal::default(),
            "building web",
        )
        .await;
        let (lines, mut output) = tokio::sync::mpsc::unbounded_channel();
        let emitted = async {
            operation
                .emit(Some("web"), "building", "running scripts.build")
                .await;
            // Apart, so the report reads them in this order.
            tokio::time::sleep(Duration::from_millis(50)).await;
            lines
                .send(("web".to_string(), "compiling \x1b[1mweb\x1b[0m".to_string()))
                .unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            operation
                .emit(Some("web"), "built", "scripts.build finished")
                .await;
            operation
                .finish(OperationStatus::Succeeded, "every requested build finished")
                .await;
            // Logged before the build ended, read after: still printed, before the last line.
            lines.send(("web".to_string(), "done".to_string())).unwrap();
            drop(lines);
        };
        tokio::join!(
            report.follow_with(&operation, events, Some(&mut output)),
            emitted
        );
        report.print_rest(&mut output).await;
        report
            .finish(&Ending::Succeeded("every requested build finished".into()))
            .await;
        client.shutdown_async().await;

        let text = sink.text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[..3],
            [
                "building web",
                "web: running scripts.build",
                "web │ compiling  [1mweb [0m"
            ],
            "{text}"
        );
        assert!(
            lines[3].starts_with("web: scripts.build finished ("),
            "{text}"
        );
        assert_eq!(lines[4], "web │ done", "{text}");
        assert!(
            lines[5].starts_with("✓ every requested build finished ("),
            "{text}"
        );
    }

    #[test]
    fn a_detail_is_one_line_without_escape_sequences() {
        assert_eq!(one_line("boom \x1b[2J\nIt last said: secret"), "boom  [2J");
    }

    #[tokio::test]
    async fn a_pipe_gets_one_line_per_change_and_no_escape_sequence() {
        let sink = Sink::default();
        draw_an_add(&sink, CallerTerminal::default()).await;
        let text = sink.text();
        assert!(!text.contains('\x1b'), "{text:?}");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[..4],
            [
                "adding 1 worker",
                "resolving dependency trees",
                "state: starting worker",
                "warning: 'kv' is an alias of 'state'",
            ],
            "{text}"
        );
        assert!(lines[4].starts_with("state: ready ("), "{text}");
        assert!(
            lines[5].starts_with("✓ all requested workers are ready ("),
            "{text}"
        );
        assert_eq!(lines.len(), 6, "{text}");
    }

    #[tokio::test]
    async fn a_terminal_keeps_the_last_frame_and_the_warning_above_it() {
        let sink = Sink::default();
        let terminal = CallerTerminal {
            stderr: true,
            columns: Some(60),
            rows: Some(20),
            color: true,
            ..CallerTerminal::default()
        };
        draw_an_add(&sink, terminal).await;
        let text = sink.text();
        assert!(text.contains("\x1b["), "frames go back up over the block");

        let mut screen = vt100::Parser::new(20, 60, 0);
        // A terminal turns each newline into a carriage return and a line feed.
        screen.process(text.replace('\n', "\r\n").as_bytes());
        let contents = screen.screen().contents();
        let lines: Vec<&str> = contents.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(lines.len(), 3, "{contents}");
        assert_eq!(lines[0], "warning: 'kv' is an alias of 'state'");
        assert!(lines[1].starts_with("  ✓ state ready ("), "{contents}");
        assert!(
            lines[2].starts_with("✓ all requested workers are ready ("),
            "{contents}"
        );
    }

    #[tokio::test]
    async fn no_frame_is_wider_than_the_terminal() {
        let terminal = CallerTerminal {
            stderr: true,
            columns: Some(20),
            rows: Some(5),
            ..CallerTerminal::default()
        };
        let mut report = CallerReport::start(
            OutputStream::to_writer(Sink::default()),
            terminal,
            "adding 12 workers with a long title",
        )
        .await;
        let now = Instant::now();
        for index in 0..12 {
            report.rows.apply(
                &format!("worker-{index}"),
                "starting",
                "starting worker",
                now,
            );
        }
        let frame = report.draw();
        assert_eq!(report.drawn, 4, "the cursor's line stays free: {frame}");
        for line in frame.lines() {
            assert!(console::measure_text_width(line) < 20, "{line:?}");
        }
        assert!(frame.contains("more"), "{frame}");
    }

    #[tokio::test]
    async fn a_caller_that_left_cancels_the_operation_and_its_events_are_still_read() {
        let (client, operation) = operation().await;
        let events = operation.subscribe();
        let sink = Sink {
            gone: true,
            ..Sink::default()
        };
        let mut report = CallerReport::start(
            OutputStream::to_writer(sink),
            CallerTerminal::default(),
            "adding 1 worker",
        )
        .await;
        assert!(report.left());

        let mut cancellation = operation.cancellation();
        let cancelled = async {
            cancellation.wait_for(|cancelled| *cancelled).await.unwrap();
            operation.emit(Some("state"), "failed", "stopped").await;
            operation
                .finish(OperationStatus::Cancelled, "operation cancelled")
                .await;
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(report.follow(&operation, events), cancelled)
        })
        .await
        .expect("the report ends with the cancelled operation");
        assert!(operation.is_cancelled());
        report
            .finish(&Ending::Cancelled("operation cancelled".into()))
            .await;
        client.shutdown_async().await;
    }
}
