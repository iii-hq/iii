// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The streams `iii call` carries: this process's stdio bound to the stream fields of the
//! payload, the streams of the result followed to stdout and stderr, and the first SIGINT or
//! SIGTERM (Ctrl+C or Ctrl+Break on Windows) turned into a stop.
//!
//! A function that streams to its caller lasts as long as the caller reads. Cancelling the
//! streams is how `iii call` asks it to stop: the engine closes the function's end, its next
//! write fails, and it answers.

use futures::StreamExt;
use iii_sdk::{
    IIIClient, InputStream, OutputStream,
    protocol::{ChannelDirection, ChannelItem, StreamChannelRef},
};
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::watch,
    task::JoinHandle,
};

/// One of this process's descriptors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stdio {
    Stdout,
    Stderr,
    Stdin,
}

impl Stdio {
    /// The payload field the descriptor binds to when the function's schema has it.
    pub fn default_field(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::Stdin => "stdin",
        }
    }

    /// The stream a field must be to take this descriptor: the function writes stdout and
    /// stderr, and reads stdin.
    fn direction(self) -> ChannelDirection {
        match self {
            Self::Stdout | Self::Stderr => ChannelDirection::Write,
            Self::Stdin => ChannelDirection::Read,
        }
    }
}

/// Which payload field each descriptor is bound to.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Bindings {
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub stdin: Option<String>,
}

impl Bindings {
    pub fn is_empty(&self) -> bool {
        self.stdout.is_none() && self.stderr.is_none() && self.stdin.is_none()
    }

    fn fields(&self) -> impl Iterator<Item = (Stdio, &str)> {
        [
            (Stdio::Stdout, &self.stdout),
            (Stdio::Stderr, &self.stderr),
            (Stdio::Stdin, &self.stdin),
        ]
        .into_iter()
        .filter_map(|(stdio, field)| field.as_deref().map(|field| (stdio, field)))
    }

    fn slot(&mut self, stdio: Stdio) -> &mut Option<String> {
        match stdio {
            Stdio::Stdout => &mut self.stdout,
            Stdio::Stderr => &mut self.stderr,
            Stdio::Stdin => &mut self.stdin,
        }
    }

    /// Binds the fields the function's request schema declares: an `OutputStream` named `stdout`
    /// or `stderr`, an `InputStream` named `stdin`. A field the payload already sets is left
    /// alone.
    pub fn complete_from_schema(&mut self, schema: &Value, payload: &Value) {
        let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
            return;
        };
        for stdio in [Stdio::Stdout, Stdio::Stderr, Stdio::Stdin] {
            let field = stdio.default_field();
            if self.slot(stdio).is_some() || payload.get(field).is_some() {
                continue;
            }
            if properties
                .get(field)
                .and_then(|property| stream_direction(schema, property, 0))
                == Some(stdio.direction())
            {
                *self.slot(stdio) = Some(field.to_string());
            }
        }
    }
}

/// The direction of the stream a property's schema describes, `None` when it is no stream. Every
/// SDK describes a stream as its ref object, whose `direction` is a constant: `write` for an
/// `OutputStream`, `read` for an `InputStream`. A schema marked with `x-iii-stream` (Rust, Go) is
/// a stream; one without the marker must have the ref's `channel_id` and `access_key` too, so an
/// ordinary object that only has a constant `direction` is not taken for one. Follows
/// local `$ref`s and the members of `anyOf`, `oneOf` and `allOf`, as an optional field has them.
fn stream_direction(root: &Value, schema: &Value, depth: usize) -> Option<ChannelDirection> {
    if depth > 8 {
        return None;
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let target = root.pointer(reference.strip_prefix('#')?)?;
        return stream_direction(root, target, depth + 1);
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(direction) = schema
            .get(key)
            .and_then(Value::as_array)
            .and_then(|members| {
                members
                    .iter()
                    .find_map(|member| stream_direction(root, member, depth + 1))
            })
        {
            return Some(direction);
        }
    }
    let marker = match schema.get("x-iii-stream") {
        Some(marker) => marker,
        None => {
            let properties = schema.get("properties")?;
            properties.get("channel_id")?;
            properties.get("access_key")?;
            properties.pointer("/direction/const")?
        }
    };
    match marker.as_str()? {
        "read" => Some(ChannelDirection::Read),
        "write" => Some(ChannelDirection::Write),
        _ => None,
    }
}

/// The payload field a request schema marks `x-iii-terminal`: the function draws for the terminal
/// its streams reach, and `iii call` says what that terminal is. `None` when no field is marked or
/// the payload already sets the marked one. Follows local `$ref`s and the members of `anyOf`,
/// `oneOf` and `allOf`, as an optional field has them.
pub fn terminal_field<'a>(schema: &'a Value, payload: &Value) -> Option<&'a str> {
    schema
        .get("properties")?
        .as_object()?
        .iter()
        .find(|(field, property)| {
            payload.get(field.as_str()).is_none() && marks_terminal(schema, property, 0)
        })
        .map(|(field, _)| field.as_str())
}

fn marks_terminal(root: &Value, schema: &Value, depth: usize) -> bool {
    if depth > 8 {
        return false;
    }
    if schema.get("x-iii-terminal") == Some(&Value::Bool(true)) {
        return true;
    }
    if let Some(target) = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix('#'))
        .and_then(|pointer| root.pointer(pointer))
    {
        return marks_terminal(root, target, depth + 1);
    }
    ["anyOf", "oneOf", "allOf"].iter().any(|key| {
        schema
            .get(*key)
            .and_then(Value::as_array)
            .is_some_and(|members| {
                members
                    .iter()
                    .any(|member| marks_terminal(root, member, depth + 1))
            })
    })
}

/// What this process's terminal is, for the field [`terminal_field`] names: whether stdout and
/// stderr redraw a line, the terminal's size, and whether colour is wanted (`NO_COLOR` unset).
pub fn terminal_description() -> Value {
    let stderr = console::Term::stderr();
    let stdout = console::Term::stdout();
    describe_terminal(
        redraws(&stdout),
        redraws(&stderr),
        stderr.size_checked().or_else(|| stdout.size_checked()),
        std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty()),
    )
}

fn describe_terminal(stdout: bool, stderr: bool, size: Option<(u16, u16)>, color: bool) -> Value {
    serde_json::json!({
        "stdout": stdout,
        "stderr": stderr,
        "columns": size.map(|(_, columns)| columns),
        "rows": size.map(|(rows, _)| rows),
        "color": color,
    })
}

/// Whether `term` redraws a line: a terminal, and not `TERM=dumb`. On Windows the console must also
/// take escape sequences, which `console` turns on as it checks.
fn redraws(term: &console::Term) -> bool {
    if !term.is_term() || std::env::var_os("TERM").is_some_and(|name| name == "dumb") {
        return false;
    }
    if cfg!(windows) {
        return term.features().colors_supported();
    }
    true
}

/// The streams at `/stdout` and `/stderr` of a result, to follow: refs to read.
pub fn result_streams(result: &Value) -> Vec<(Stdio, StreamChannelRef)> {
    [Stdio::Stdout, Stdio::Stderr]
        .into_iter()
        .filter_map(|stdio| {
            let channel: StreamChannelRef =
                serde_json::from_value(result.get(stdio.default_field())?.clone()).ok()?;
            (channel.direction == ChannelDirection::Read).then_some((stdio, channel))
        })
        .collect()
}

/// How a copy into stdout or stderr failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyFailure {
    /// The stream ended in an error. A stream the call cancelled ends this way too.
    Stream(String),
    /// This process's output could not take what the stream carried: that output is lost,
    /// whatever stopped the stream.
    Output(String),
}

impl CopyFailure {
    fn labelled(self, label: &str) -> Self {
        match self {
            Self::Stream(error) => Self::Stream(format!("the {label} stream failed: {error}")),
            Self::Output(error) => Self::Output(format!("the {label} stream failed: {error}")),
        }
    }
}

impl std::fmt::Display for CopyFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stream(message) | Self::Output(message) => f.write_str(message),
        }
    }
}

/// How a copy into stdout or stderr ended.
type Copied = Result<(), CopyFailure>;

/// The streams of one call: the read ends this process copies out, the write end it fills from
/// stdin, and the tasks doing it.
#[derive(Default)]
pub struct Carried {
    readers: Vec<InputStream>,
    writers: Vec<OutputStream>,
    copies: Vec<(&'static str, JoinHandle<Copied>)>,
    stdin: Option<JoinHandle<()>>,
    /// Why this process's stdin could not be read, once a read failed. Kept apart from the stdin
    /// task, which `finish` does not wait for, and set before the function learns of it.
    unread: Arc<Mutex<Option<String>>>,
    /// The copy that failed, kept across a `finish` that was interrupted.
    failure: Option<CopyFailure>,
}

impl Carried {
    /// Whether any stream is bound or followed.
    pub fn is_empty(&self) -> bool {
        self.copies.is_empty() && self.stdin.is_none()
    }

    /// Opens one pipe per binding and puts the end the function uses in `payload` at its field:
    /// the write end for stdout and stderr, the read end for stdin.
    pub async fn bind(
        &mut self,
        iii: &IIIClient,
        bindings: &Bindings,
        payload: &mut Value,
    ) -> anyhow::Result<()> {
        let Some(object) = payload.as_object_mut() else {
            anyhow::bail!("a stream can only be bound in an object payload");
        };
        for (stdio, field) in bindings.fields() {
            let pipe = iii
                .pipe()
                .await
                .map_err(|error| anyhow::anyhow!("could not open a stream for {field}: {error}"))?;
            match stdio {
                Stdio::Stdout | Stdio::Stderr => {
                    object.insert(field.to_string(), serde_json::to_value(&pipe.output)?);
                    self.copy_out(stdio, pipe.input);
                }
                Stdio::Stdin => {
                    object.insert(field.to_string(), serde_json::to_value(&pipe.input)?);
                    self.writers.push(pipe.output.clone());
                    self.stdin = Some(tokio::spawn(copy_in(pipe.output, self.unread.clone())));
                }
            }
        }
        Ok(())
    }

    /// Follows the streams of a result into stdout and stderr.
    pub fn follow(
        &mut self,
        iii: &IIIClient,
        streams: Vec<(Stdio, StreamChannelRef)>,
    ) -> anyhow::Result<()> {
        for (stdio, channel) in streams {
            let input = InputStream::from_ref(channel, iii).map_err(|error| {
                anyhow::anyhow!(
                    "could not read the {} stream of the result: {error}",
                    stdio.default_field()
                )
            })?;
            self.copy_out(stdio, input);
        }
        Ok(())
    }

    fn copy_out(&mut self, stdio: Stdio, input: InputStream) {
        self.readers.push(input.clone());
        let label = stdio.default_field();
        let task = match stdio {
            Stdio::Stderr => tokio::spawn(copy_out(input, tokio::io::stderr())),
            Stdio::Stdout | Stdio::Stdin => tokio::spawn(copy_out(input, tokio::io::stdout())),
        };
        self.copies.push((label, task));
    }

    /// Stops reading and writing every stream: the function sees its sinks close and its source
    /// abort.
    pub async fn cancel(&self) {
        for reader in &self.readers {
            reader.cancel().await;
        }
        for writer in &self.writers {
            let _ = writer.abort("iii call was interrupted").await;
        }
        if let Some(stdin) = &self.stdin {
            stdin.abort();
        }
    }

    /// Stops every copy without waiting for it, as an interrupted call leaves them: a copy can sit
    /// in a write into an output whose reader stopped reading, which never ends.
    pub fn abandon(&mut self) {
        for (_, task) in self.copies.drain(..) {
            task.abort();
        }
        if let Some(stdin) = self.stdin.take() {
            stdin.abort();
        }
    }

    /// Waits for every copy into stdout and stderr to end; the copy that failed, by label: the
    /// first that lost output, else the first stream that failed. The stdin copy is not waited
    /// for: it may sit on a terminal that never ends. Input this process could not read is lost
    /// like output it could not write, and fails the call the same way.
    ///
    /// Each copy is awaited in place and removed once it ended, so a `finish` dropped half way,
    /// when a signal wins the race, leaves the copies still running here for the next `finish`
    /// to join.
    pub async fn finish(&mut self) -> Result<(), CopyFailure> {
        while let Some((label, task)) = self.copies.first_mut() {
            let label = *label;
            let ended = match task.await {
                Ok(ended) => ended,
                Err(error) => Err(CopyFailure::Stream(error.to_string())),
            };
            self.copies.remove(0);
            if let Err(failure) = ended {
                // Lost output outranks a stream that failed: a stop makes every stream fail.
                let keep = matches!(
                    (&self.failure, &failure),
                    (None, _) | (Some(CopyFailure::Stream(_)), CopyFailure::Output(_))
                );
                if keep {
                    self.failure = Some(failure.labelled(label));
                }
            }
        }
        if let Some(stdin) = self.stdin.take() {
            stdin.abort();
        }
        let unread = self
            .unread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(error) = unread
            && !matches!(self.failure, Some(CopyFailure::Output(_)))
        {
            self.failure = Some(CopyFailure::Output(format!(
                "the stdin stream failed: could not read it: {error}"
            )));
        }
        self.failure.take().map_or(Ok(()), Err)
    }
}

/// Copies a stream to `out` until it ends: bytes as they come, each message on its own line.
/// Empty frames are only there to notice a reader that left, and write nothing.
async fn copy_out(input: InputStream, mut out: impl AsyncWrite + Unpin) -> Copied {
    let mut frames = input.frames();
    while let Some(frame) = frames.next().await {
        let written = match frame.map_err(|error| CopyFailure::Stream(error.to_string()))? {
            ChannelItem::Binary(bytes) if bytes.is_empty() => continue,
            ChannelItem::Binary(bytes) => out.write_all(&bytes).await,
            ChannelItem::Text(text) => match out.write_all(text.as_bytes()).await {
                Ok(()) => out.write_all(b"\n").await,
                Err(error) => Err(error),
            },
        };
        let written = match written {
            Ok(()) => out.flush().await,
            Err(error) => Err(error),
        };
        if let Err(error) = written {
            // The output cannot take more: stop the function's stream too.
            input.cancel().await;
            // A reader that left (`| head`) ends the output, as the caller wanted. Any other
            // failure (a full disk) lost output, and the call must not report success.
            if error.kind() == std::io::ErrorKind::BrokenPipe {
                return Ok(());
            }
            return Err(CopyFailure::Output(format!("could not write it: {error}")));
        }
    }
    Ok(())
}

/// Copies this process's stdin into the stream the function reads, and ends it at EOF.
///
/// The reads run on a thread of their own. A blocking read cannot be cancelled, and the runtime
/// waits for its blocking pool when it shuts down, so a read there (as `tokio::io::stdin` does
/// it) held up the exit of a call that answered without reading a stdin its parent kept open.
/// The process does not wait for this thread when it exits. A read that fails is recorded in
/// `unread` by the thread itself, before it is queued and the stream aborted: a function that
/// answers before this copy takes the failure still leaves the call failing for the lost input.
async fn copy_in(output: OutputStream, unread: Arc<Mutex<Option<String>>>) {
    let fail = |error: String| record_unread(&unread, error);
    let read_failures = Arc::clone(&unread);
    let (sender, mut chunks) = tokio::sync::mpsc::channel::<std::io::Result<Vec<u8>>>(4);
    let reader = std::thread::Builder::new()
        .name("iii-call-stdin".to_string())
        .spawn(move || {
            use std::io::Read;
            let mut stdin = std::io::stdin().lock();
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let chunk = match stdin.read(&mut buffer) {
                    // EOF: the sender, dropped, ends the copy.
                    Ok(0) => return,
                    Ok(read) => Ok(buffer[..read].to_vec()),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        record_unread(&read_failures, error.to_string());
                        Err(error)
                    }
                };
                let failed = chunk.is_err();
                // The send fails once the copy is gone: nobody wants more of stdin.
                if sender.blocking_send(chunk).is_err() || failed {
                    return;
                }
            }
        });
    if let Err(error) = reader {
        fail(error.to_string());
        let _ = output.abort(error.to_string()).await;
        return;
    }
    while let Some(chunk) = chunks.recv().await {
        match chunk {
            Ok(bytes) => {
                if output.write(&bytes).await.is_err() {
                    return;
                }
            }
            Err(error) => {
                fail(error.to_string());
                let _ = output.abort(error.to_string()).await;
                return;
            }
        }
    }
    let _ = output.close().await;
}

/// Keeps the first stdin failure: the thread that read and the copy that met it can both report
/// the same one.
fn record_unread(unread: &Mutex<Option<String>>, error: String) {
    unread
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_or_insert(error);
}

/// SIGINT and SIGTERM. While streams are carried, the first one is a stop request and a second one
/// ends the process at once, with 128 plus its number. Before [`Interrupt::arm`], nothing can be
/// stopped, and the first one already ends the process so. On Windows, Ctrl+C is SIGINT and
/// Ctrl+Break is SIGTERM: the console event a process manager sends to stop a process group.
pub struct Interrupt {
    received: watch::Receiver<Option<i32>>,
    /// Whether the first signal is a stop request: set once streams are carried.
    armed: Arc<AtomicBool>,
}

impl Interrupt {
    /// Takes SIGINT and SIGTERM from now on. Until [`Interrupt::arm`], a signal ends the process
    /// at once with 128 plus its number.
    pub fn install() -> std::io::Result<Self> {
        let (sender, received) = watch::channel(None);
        let armed = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        let (mut terminate, mut interrupt) = {
            use tokio::signal::unix::{SignalKind, signal};
            (
                signal(SignalKind::terminate())?,
                signal(SignalKind::interrupt())?,
            )
        };
        #[cfg(windows)]
        let (mut terminate, mut interrupt) = (
            tokio::signal::windows::ctrl_break()?,
            tokio::signal::windows::ctrl_c()?,
        );
        let stops = Arc::clone(&armed);
        tokio::spawn(async move {
            let mut first = true;
            loop {
                let number = tokio::select! {
                    received = terminate.recv() => received.map(|()| 15),
                    received = interrupt.recv() => received.map(|()| 2),
                };
                let Some(number) = number else {
                    return;
                };
                if ends_at_once(first, stops.load(Ordering::Acquire)) {
                    std::process::exit(128 + number);
                }
                first = false;
                sender.send_replace(Some(number));
            }
        });
        Ok(Self { received, armed })
    }

    /// Makes the first signal a stop request: streams are carried, and closing them asks the
    /// function to stop.
    pub fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    /// An armed interrupt that takes its signals from the returned sender, for tests.
    #[cfg(test)]
    pub fn manual() -> (watch::Sender<Option<i32>>, Self) {
        let (sender, received) = watch::channel(None);
        let armed = Arc::new(AtomicBool::new(true));
        (sender, Self { received, armed })
    }

    /// Resolves to the first signal's number; never without one.
    pub async fn requested(&self) -> i32 {
        let mut received = self.received.clone();
        match received.wait_for(Option::is_some).await {
            Ok(signal) => signal.unwrap_or(2),
            Err(_) => std::future::pending().await,
        }
    }
}

/// Whether a signal ends the process at once: a second one always does, and a first one does when
/// no stream is carried, since there is nothing to close to ask the function to stop.
fn ends_at_once(first: bool, armed: bool) -> bool {
    !first || !armed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn the_field_marked_x_iii_terminal_is_filled_unless_the_payload_sets_it() {
        let marked = json!({ "type": ["object", "null"], "x-iii-terminal": true });
        let schema = json!({
            "properties": {
                "worker": { "type": "string" },
                "terminal": marked,
            }
        });
        assert_eq!(terminal_field(&schema, &json!({})), Some("terminal"));
        assert_eq!(
            terminal_field(&schema, &json!({ "terminal": { "stderr": false } })),
            None,
            "the payload's own value is kept"
        );

        let behind_a_ref = json!({
            "properties": { "tty": { "anyOf": [{ "$ref": "#/definitions/Tty" }, { "type": "null" }] } },
            "definitions": { "Tty": marked },
        });
        assert_eq!(terminal_field(&behind_a_ref, &json!({})), Some("tty"));

        let unmarked = json!({ "properties": { "terminal": { "type": "boolean" } } });
        assert_eq!(terminal_field(&unmarked, &json!({})), None);
    }

    #[test]
    fn the_terminal_description_is_what_compose_reads() {
        let described = describe_terminal(false, true, Some((40, 120)), true);
        let terminal: iii_compose::caller_report::CallerTerminal =
            serde_json::from_value(described).expect("compose reads the description");
        assert_eq!(
            terminal,
            iii_compose::caller_report::CallerTerminal {
                stdout: false,
                stderr: true,
                columns: Some(120),
                rows: Some(40),
                color: true,
            }
        );
        assert_eq!(
            describe_terminal(false, false, None, false),
            json!({ "stdout": false, "stderr": false, "columns": null, "rows": null, "color": false })
        );
    }

    /// A request schema as `schemars` writes it for an SDK struct with stream fields.
    fn schema() -> Value {
        let stream = |title: &str, direction: &str| {
            json!({
                "title": title,
                "type": "object",
                "required": ["access_key", "channel_id", "direction"],
                "properties": {
                    "channel_id": { "type": "string" },
                    "access_key": { "type": "string" },
                    "direction": { "type": "string", "const": direction }
                }
            })
        };
        json!({
            "type": "object",
            "properties": {
                "worker": { "type": "string" },
                "stdout": { "$ref": "#/definitions/OutputStream" },
                "stderr": { "anyOf": [{ "$ref": "#/definitions/OutputStream" }, { "type": "null" }] },
                "stdin": { "$ref": "#/definitions/InputStream" },
                "logs": { "$ref": "#/definitions/OutputStream" }
            },
            "definitions": {
                "OutputStream": stream("OutputStream", "write"),
                "InputStream": stream("InputStream", "read")
            }
        })
    }

    #[test]
    fn the_schema_binds_stdout_stderr_and_stdin_by_name_and_direction() {
        let mut bindings = Bindings::default();
        bindings.complete_from_schema(&schema(), &json!({ "worker": "api" }));
        assert_eq!(
            bindings,
            Bindings {
                stdout: Some("stdout".into()),
                stderr: Some("stderr".into()),
                stdin: Some("stdin".into()),
            }
        );
    }

    #[test]
    fn a_field_of_the_wrong_direction_is_not_bound() {
        let mut schema = schema();
        schema["properties"]["stdout"] = json!({ "$ref": "#/definitions/InputStream" });
        schema["properties"]["stdin"] = json!({ "type": "string" });
        let mut bindings = Bindings::default();
        bindings.complete_from_schema(&schema, &json!({}));
        assert_eq!(bindings.stdout, None);
        assert_eq!(bindings.stdin, None);
        assert_eq!(bindings.stderr.as_deref(), Some("stderr"));
    }

    #[test]
    fn a_payload_field_wins_over_the_schema() {
        let mut bindings = Bindings::default();
        bindings.complete_from_schema(&schema(), &json!({ "stderr": "given" }));
        assert_eq!(bindings.stdout.as_deref(), Some("stdout"));
        assert_eq!(bindings.stderr, None, "the payload already sets stderr");
        assert_eq!(bindings.stdin.as_deref(), Some("stdin"));
    }

    #[test]
    fn a_go_stream_is_read_from_its_marker() {
        let schema = json!({
            "properties": { "stdout": { "type": "object", "x-iii-stream": "write" } }
        });
        let mut bindings = Bindings::default();
        bindings.complete_from_schema(&schema, &json!({}));
        assert_eq!(bindings.stdout.as_deref(), Some("stdout"));
    }

    #[test]
    fn an_object_that_only_has_a_direction_is_no_stream() {
        let schema = json!({
            "properties": { "stdout": {
                "type": "object",
                "required": ["path", "direction"],
                "properties": {
                    "path": { "type": "string" },
                    "direction": { "type": "string", "const": "write" }
                }
            } }
        });
        let mut bindings = Bindings::default();
        bindings.complete_from_schema(&schema, &json!({}));
        assert!(bindings.is_empty(), "{bindings:?}");
    }

    #[test]
    fn a_schema_without_properties_binds_nothing() {
        let mut bindings = Bindings::default();
        bindings.complete_from_schema(&json!({ "type": "object" }), &json!({}));
        assert!(bindings.is_empty());
    }

    #[tokio::test]
    async fn an_interrupted_finish_keeps_the_copies_for_the_next() {
        let (release, released) = tokio::sync::oneshot::channel::<()>();
        let mut carried = Carried::default();
        carried.copies.push((
            "stdout",
            tokio::spawn(async move {
                let _ = released.await;
                Ok(())
            }),
        ));
        carried.copies.push((
            "stderr",
            tokio::spawn(async { Err(CopyFailure::Stream("aborted".into())) }),
        ));
        let interrupted =
            tokio::time::timeout(std::time::Duration::from_millis(50), carried.finish()).await;
        assert!(interrupted.is_err(), "the stdout copy is still running");
        assert_eq!(
            carried.copies.len(),
            2,
            "the interrupted finish dropped copies"
        );
        release.send(()).unwrap();
        assert_eq!(
            carried.finish().await,
            Err(CopyFailure::Stream(
                "the stderr stream failed: aborted".into()
            ))
        );
        assert!(carried.is_empty());
    }

    #[tokio::test]
    async fn lost_output_outranks_a_stream_that_failed() {
        let mut carried = Carried::default();
        carried.copies.push((
            "stdout",
            tokio::spawn(async { Err(CopyFailure::Stream("cancelled".into())) }),
        ));
        carried.copies.push((
            "stderr",
            tokio::spawn(async { Err(CopyFailure::Output("could not write it: full".into())) }),
        ));
        assert_eq!(
            carried.finish().await,
            Err(CopyFailure::Output(
                "the stderr stream failed: could not write it: full".into()
            ))
        );
    }

    /// An output whose every write fails with `kind`.
    struct Failing(std::io::ErrorKind);

    impl AsyncWrite for Failing {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            _: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::task::Poll::Ready(Err(self.0.into()))
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn a_failed_write_fails_the_copy_unless_the_reader_left() {
        let full = copy_out(
            InputStream::from_bytes("data"),
            Failing(std::io::ErrorKind::StorageFull),
        )
        .await;
        assert!(
            matches!(&full, Err(CopyFailure::Output(error)) if error.contains("could not write it")),
            "{full:?}"
        );
        let left = copy_out(
            InputStream::from_bytes("data"),
            Failing(std::io::ErrorKind::BrokenPipe),
        )
        .await;
        assert_eq!(left, Ok(()));
    }

    #[test]
    fn only_read_refs_at_stdout_and_stderr_of_a_result_are_followed() {
        let reference = |direction: &str| json!({ "channel_id": "c", "access_key": "k", "direction": direction });
        let followed = result_streams(&json!({
            "stdout": reference("read"),
            "stderr": reference("write"),
            "other": reference("read"),
        }));
        assert_eq!(followed.len(), 1);
        assert_eq!(followed[0].0, Stdio::Stdout);
        assert!(result_streams(&json!({ "stdout": "text" })).is_empty());
        assert!(result_streams(&json!(42)).is_empty());
    }

    /// Without a carried stream there is nothing to stop: the first signal ends the process, as
    /// SIGINT and SIGTERM do on Unix, and as Ctrl+C and Ctrl+Break must on Windows.
    #[test]
    fn the_first_signal_ends_a_call_that_carries_no_stream() {
        assert!(ends_at_once(true, false));
    }

    #[test]
    fn the_first_signal_is_a_stop_once_streams_are_carried() {
        assert!(!ends_at_once(true, true));
    }

    #[test]
    fn the_second_signal_ends_the_call() {
        assert!(ends_at_once(false, true));
    }
}
