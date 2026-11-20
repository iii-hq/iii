// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The streams `iii call` carries: this process's stdio bound to the stream fields of the
//! payload, the streams of the result followed to stdout and stderr, and the first SIGINT or
//! SIGTERM turned into a stop.
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
use std::sync::{Arc, Mutex};

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

    /// Refuses, before any stream is opened, the bindings no call can carry: a stream bound in a
    /// payload that is not an object, or two descriptors bound to one field, where the second
    /// ref would overwrite the first after both pipes were opened.
    pub fn check(&self, payload: &Value) -> anyhow::Result<()> {
        if !self.is_empty() && !payload.is_object() {
            anyhow::bail!("a stream can only be bound in an object payload");
        }
        let mut seen: Vec<(Stdio, &str)> = Vec::new();
        for (stdio, field) in self.fields() {
            if let Some((first, _)) = seen.iter().find(|(_, taken)| *taken == field) {
                anyhow::bail!(
                    "--{} and --{} both bind the field {field:?}",
                    first.default_field(),
                    stdio.default_field()
                );
            }
            seen.push((stdio, field));
        }
        Ok(())
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

    /// Binds the descriptors no flag bound to the fields the function's request schema declares:
    /// an `OutputStream` named `stdout` or `stderr`, an `InputStream` named `stdin`. A field the
    /// payload already sets, or one a flag took, is left alone.
    pub fn complete_from_schema(&mut self, schema: &Value, payload: &Value) {
        let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
            return;
        };
        let taken: Vec<String> = self.fields().map(|(_, field)| field.to_string()).collect();
        for stdio in [Stdio::Stdout, Stdio::Stderr, Stdio::Stdin] {
            let field = stdio.default_field();
            if self.slot(stdio).is_some()
                || taken.iter().any(|taken| taken == field)
                || payload.get(field).is_some()
            {
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
/// `unread` before the stream is aborted, so the call learns of it before the function answers.
async fn copy_in(output: OutputStream, unread: Arc<Mutex<Option<String>>>) {
    let fail = |error: String| {
        *unread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error);
    };
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
                    Err(error) => Err(error),
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

/// SIGINT and SIGTERM while streams are carried: the first one is a stop request, a second one
/// ends the process at once, with 128 plus its number.
pub struct Interrupt(watch::Receiver<Option<i32>>);

impl Interrupt {
    pub fn install() -> std::io::Result<Self> {
        let (sender, received) = watch::channel(None);
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let mut terminate = signal(SignalKind::terminate())?;
            let mut interrupt = signal(SignalKind::interrupt())?;
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
                    if !first {
                        std::process::exit(128 + number);
                    }
                    first = false;
                    sender.send_replace(Some(number));
                }
            });
        }
        #[cfg(windows)]
        {
            let mut ctrl_c = tokio::signal::windows::ctrl_c()?;
            tokio::spawn(async move {
                if ctrl_c.recv().await.is_none() {
                    return;
                }
                sender.send_replace(Some(2));
                if ctrl_c.recv().await.is_some() {
                    std::process::exit(130);
                }
            });
        }
        Ok(Self(received))
    }

    /// An interrupt that takes its signals from the returned sender, for tests.
    #[cfg(test)]
    pub fn manual() -> (watch::Sender<Option<i32>>, Self) {
        let (sender, received) = watch::channel(None);
        (sender, Self(received))
    }

    /// Resolves to the first signal's number; never without one.
    pub async fn requested(&self) -> i32 {
        let mut received = self.0.clone();
        match received.wait_for(Option::is_some).await {
            Ok(signal) => signal.unwrap_or(2),
            Err(_) => std::future::pending().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn flags_and_payload_fields_win_over_the_schema() {
        let mut bindings = Bindings {
            stdout: Some("logs".into()),
            ..Default::default()
        };
        bindings.complete_from_schema(&schema(), &json!({ "stderr": "given" }));
        assert_eq!(bindings.stdout.as_deref(), Some("logs"));
        assert_eq!(bindings.stderr, None, "the payload already sets stderr");
        assert_eq!(bindings.stdin.as_deref(), Some("stdin"));

        // A field one flag took is not bound again to another descriptor.
        let mut bindings = Bindings {
            stderr: Some("stdout".into()),
            ..Default::default()
        };
        bindings.complete_from_schema(&schema(), &json!({}));
        assert_eq!(bindings.stdout, None);
    }

    #[test]
    fn a_binding_needs_an_object_payload() {
        let bindings = Bindings {
            stdout: Some("out".into()),
            ..Default::default()
        };
        assert!(bindings.check(&json!({})).is_ok());
        let error = bindings.check(&json!([1])).unwrap_err().to_string();
        assert!(error.contains("object payload"), "{error}");
        assert!(
            Bindings::default().check(&json!([1])).is_ok(),
            "a call that binds nothing takes any payload"
        );
    }

    #[test]
    fn two_descriptors_cannot_bind_one_field() {
        let bindings = Bindings {
            stdout: Some("io".into()),
            stdin: Some("io".into()),
            ..Default::default()
        };
        let error = bindings.check(&json!({})).unwrap_err().to_string();
        assert_eq!(error, "--stdout and --stdin both bind the field \"io\"");
        // One descriptor may still replace a value the payload sets.
        let bindings = Bindings {
            stdout: Some("io".into()),
            stdin: Some("input".into()),
            ..Default::default()
        };
        assert!(bindings.check(&json!({ "io": "given" })).is_ok());
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
}
