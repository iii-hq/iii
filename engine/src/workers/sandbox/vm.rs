// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `sandbox::build` and `sandbox::start`: one Compose container's VM, built and run on top of the
//! adapter's `worker::*` calls (tech-specs/2026-11-19-iii-call-sandbox, section 4).
//!
//! A container's scripts call them through `iii call`, which binds its stdout and stderr to the
//! request's `stdout` and `stderr` sinks. Both last as long as their caller reads: a sink whose
//! reader left is the stop. Unlike the ephemeral `sandbox::*` operations, they go to the adapter
//! the request names, not to the router's binding, since a VM worker uses its own project's
//! adapter.

use std::{sync::Arc, time::Duration};

use axum::body::Bytes;
use iii_sandbox_contract::{
    self as contract, ADAPTER_INFO_SUFFIX, AdapterInfo, AdapterInfoRequest, CONTRACT_MAJOR,
    FEATURE_BINARY, FEATURE_BUILD, FEATURE_LEASE, FEATURE_LOGS_FROM_END, FEATURE_SCOPED_AUTH,
    LogRecord, LogStream, SandboxBuildRequest, SandboxBuildResponse, SandboxErrorCode,
    SandboxStartRequest, SandboxStartResponse, State, WORKER_BUILD, WORKER_LOGS, WORKER_START,
    WORKER_STATUS, WORKER_STOP, WorkerBuildRef, WorkerBuildRequest, WorkerBuildResponse,
    WorkerLogsRequest, WorkerLogsResponse, WorkerSource, WorkerStartRequest, WorkerStartResponse,
    WorkerStatus, WorkerStatusRequest, WorkerStopRequest, adapter_function_id, code_of,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::{sync::mpsc, task::JoinHandle, time::Instant};

use crate::{
    engine::Engine,
    protocol::ErrorBody,
    workers::worker::channels::{ChannelItem, ChannelManager},
};

/// How long the adapter may take to register `adapter::info` after it connects.
const INFO_WAIT: Duration = Duration::from_secs(if cfg!(test) { 2 } else { 30 });
/// `worker::start` boots the VM.
const START_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Every other adapter call's budget.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// How long after a log stream closed on a running VM (or would not open) it is asked for again,
/// and the pause between unanswered stops.
const RETRY_INTERVAL: Duration = Duration::from_millis(if cfg!(test) { 100 } else { 1000 });
/// How long `worker::status` may go unanswered before the VM is given up: the adapter's reattach
/// grace, after which its lease has stopped a VM nobody follows.
const LOST_AFTER: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 45 });
/// How long the output of a build, or of a VM that exited, may take to end.
const DRAIN_WAIT: Duration = Duration::from_secs(if cfg!(test) { 2 } else { 10 });
/// A stop's budget when the request names none.
const DEFAULT_STOP_GRACE: Duration = Duration::from_secs(10);
/// "No limit" for a build without `timeout_ms`: a year, which still adds to an `Instant`.
const NO_LIMIT: Duration = Duration::from_secs(365 * 24 * 60 * 60);
/// Frames a log channel buffers.
const LOG_BUFFER: usize = 64;
/// What a container's own VM needs from the adapter beyond contract 1.1: its own VM's token, a
/// VM that stops once nobody follows it, a reattach that does not replay the log, and builds.
const REQUIRED_FEATURES: [&str; 4] = [
    FEATURE_SCOPED_AUTH,
    FEATURE_LEASE,
    FEATURE_LOGS_FROM_END,
    FEATURE_BUILD,
];

/// The request does not read, or names a sink that is no open pipe.
pub const INVALID_REQUEST: &str = "SANDBOX_INVALID_REQUEST";
/// The adapter lacks `worker.build` or another feature a container's VM needs.
pub const BUILD_UNSUPPORTED: &str = "SANDBOX_BUILD_UNSUPPORTED";
/// The adapter lacks `worker.binary`, which a binary package or a base image override needs.
pub const BINARY_UNSUPPORTED: &str = "SANDBOX_BINARY_UNSUPPORTED";
/// The adapter implements another major version of the sandbox contract.
pub const CONTRACT_UNSUPPORTED: &str = "SANDBOX_CONTRACT_UNSUPPORTED";
/// The adapter was not reached, or did not answer as it should.
pub const ENGINE_CALL_FAILED: &str = "ENGINE_CALL_FAILED";
/// The build ran past its `timeout_ms`.
pub const TIMED_OUT: &str = "TIMED_OUT";
/// The build's caller left before it ended.
pub const OPERATION_CANCELLED: &str = "OPERATION_CANCELLED";
/// The VM exited with another code than 0, or on a signal.
pub const WORKER_EXITED: &str = "WORKER_EXITED";

fn failure(code: &str, message: impl Into<String>) -> ErrorBody {
    ErrorBody {
        code: code.to_string(),
        message: message.into(),
        stacktrace: None,
    }
}

fn parse<T: DeserializeOwned>(input: Value) -> Result<T, ErrorBody> {
    serde_json::from_value(input).map_err(|error| {
        failure(
            INVALID_REQUEST,
            format!("the request does not read: {error}"),
        )
    })
}

fn reply(value: impl Serialize) -> Result<Value, ErrorBody> {
    serde_json::to_value(value).map_err(|error| failure(ENGINE_CALL_FAILED, error.to_string()))
}

/// The S-code an adapter refused with, and its own words with the docs link it names; `None` for
/// anything else. The adapter's error carries its payload as JSON after its first `{`.
fn refusal(error: &ErrorBody) -> Option<(SandboxErrorCode, String)> {
    let code = code_of(&error.message)?;
    let words = payload(error)
        .and_then(|payload| {
            let words = payload["message"].as_str()?.to_string();
            Some(match payload["docs_url"].as_str() {
                Some(docs) => format!("{words} ({docs})"),
                None => words,
            })
        })
        .unwrap_or_else(|| error.message.clone());
    Some((code, words))
}

fn payload(error: &ErrorBody) -> Option<Value> {
    code_of(&error.message)?;
    serde_json::from_str(&error.message[error.message.find('{')?..]).ok()
}

/// The live boot an S502 names in its `fix`: the VM to reattach to.
fn live_instance(error: &ErrorBody) -> Option<WorkerStartResponse> {
    if refusal(error)?.0 != SandboxErrorCode::S502 {
        return None;
    }
    serde_json::from_value(payload(error)?.get("fix")?.clone()).ok()
}

async fn until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// The adapter one VM goes through.
struct Adapter<'a> {
    engine: &'a Engine,
    name: &'a str,
    namespace: &'a str,
}

impl Adapter<'_> {
    async fn call<T: DeserializeOwned>(
        &self,
        suffix: &str,
        request: &impl Serialize,
        timeout: Duration,
    ) -> Result<T, ErrorBody> {
        let input = serde_json::to_value(request)
            .map_err(|error| failure(INVALID_REQUEST, error.to_string()))?;
        let id = adapter_function_id(self.name, suffix);
        let reply = tokio::time::timeout(
            timeout,
            self.engine
                .call_value_with_metadata_ns(self.namespace, &id, input, None),
        )
        .await
        .map_err(|_| {
            failure(
                "TIMEOUT",
                format!("{id} did not answer within {}s", timeout.as_secs()),
            )
        })??;
        serde_json::from_value(reply.unwrap_or(Value::Null)).map_err(|error| {
            failure(
                ENGINE_CALL_FAILED,
                format!("{id} answered with what this engine cannot read: {error}"),
            )
        })
    }

    /// `adapter::info`, asked again while the adapter has not registered it yet: it starts before
    /// the container that uses it, but registers its functions a moment after it connects.
    async fn info(&self) -> Result<AdapterInfo, ErrorBody> {
        let deadline = Instant::now() + INFO_WAIT;
        loop {
            match self
                .call::<AdapterInfo>(ADAPTER_INFO_SUFFIX, &AdapterInfoRequest {}, CALL_TIMEOUT)
                .await
            {
                Err(error) if error.code == "function_not_found" && Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                reply => return reply.map_err(|error| self.failed("adapter::info", error)),
            }
        }
    }

    /// A failed call as this function's error: the adapter's S-code and own words when it
    /// refused, else `ENGINE_CALL_FAILED`.
    fn failed(&self, function: &str, error: ErrorBody) -> ErrorBody {
        if let Some((code, words)) = refusal(&error) {
            return failure(code.as_str(), format!("{function}: {words}"));
        }
        let hint = if error.code == "function_not_found" {
            format!(
                "; is {} connected in namespace {}?",
                self.name, self.namespace
            )
        } else {
            String::new()
        };
        failure(
            ENGINE_CALL_FAILED,
            format!("{function}: {}{hint}", error.message),
        )
    }

    /// Refuses an adapter that cannot drive a container's VM: another contract major, one
    /// without the features of [`REQUIRED_FEATURES`], or, for a binary package or a base image
    /// override (`binary`), one without `worker.binary`.
    fn check(&self, info: &AdapterInfo, binary: bool) -> Result<(), ErrorBody> {
        let (adapter, version) = (self.name, &info.adapter_version);
        if info.contract.major != CONTRACT_MAJOR {
            return Err(failure(
                CONTRACT_UNSUPPORTED,
                format!(
                    "{adapter} {version} implements sandbox contract {}.{}, but this engine \
                     needs {CONTRACT_MAJOR}.x",
                    info.contract.major, info.contract.minor
                ),
            ));
        }
        let missing: Vec<&str> = REQUIRED_FEATURES
            .into_iter()
            .filter(|feature| !info.supports(feature))
            .collect();
        if !missing.is_empty() {
            let legacy = if missing.contains(&FEATURE_BUILD) {
                " An adapter offers worker.build only when it can boot a build: sandbox-libkrun \
                 does not in legacy mode (III_ROOTFS_MODE=legacy, or a build without an \
                 embedded iii-init)."
            } else {
                ""
            };
            return Err(failure(
                BUILD_UNSUPPORTED,
                format!(
                    "{adapter} {version} does not support {}, which a container's VM needs from \
                     sandbox contract 1.4.{legacy} {}",
                    missing.join(", "),
                    UPDATE_HINT
                ),
            ));
        }
        if binary && !info.supports(FEATURE_BINARY) {
            return Err(failure(
                BINARY_UNSUPPORTED,
                format!(
                    "{adapter} {version} does not support {FEATURE_BINARY}, which a binary \
                     package or a runtime.base_image override needs from sandbox contract 1.4. \
                     {UPDATE_HINT}"
                ),
            ));
        }
        Ok(())
    }
}

/// How to move a package adapter to a release with the features this engine needs.
const UPDATE_HINT: &str = "A package adapter moves to its newest release with: iii call \
                           compose::update worker=<its container key>@latest; if its version does \
                           not change, no release has them yet";

/// The caller's `stdout` and `stderr`: the write ends of its pipes, held by this function.
/// Dropping them ends the streams, so the caller reads to the end.
#[derive(Clone, Default)]
struct Sinks {
    stdout: Option<mpsc::Sender<ChannelItem>>,
    stderr: Option<mpsc::Sender<ChannelItem>>,
}

impl Sinks {
    async fn open(
        engine: &Engine,
        stdout: Option<&contract::StreamChannelRef>,
        stderr: Option<&contract::StreamChannelRef>,
    ) -> Result<Self, ErrorBody> {
        Ok(Self {
            stdout: take_sink(engine, "stdout", stdout).await?,
            stderr: take_sink(engine, "stderr", stderr).await?,
        })
    }

    /// Writes `line` to the sink of its stream; a stderr line goes to stdout when there is no
    /// stderr sink, and nowhere without either.
    async fn write(&self, stream: &LogStream, line: &str) {
        let sink = match stream {
            LogStream::Stdout => self.stdout.as_ref(),
            LogStream::Stderr => self.stderr.as_ref().or(self.stdout.as_ref()),
        };
        if let Some(sink) = sink {
            let mut bytes = Vec::with_capacity(line.len() + 1);
            bytes.extend_from_slice(line.as_bytes());
            bytes.push(b'\n');
            let _ = sink.send(ChannelItem::Binary(Bytes::from(bytes))).await;
        }
    }

    /// Resolves once the reader of a sink left: the caller asks for a stop. Never without a
    /// sink.
    async fn closed(&self) {
        match (&self.stdout, &self.stderr) {
            (Some(stdout), Some(stderr)) => tokio::select! {
                () = stdout.closed() => {}
                () = stderr.closed() => {}
            },
            (Some(sink), None) | (None, Some(sink)) => sink.closed().await,
            (None, None) => std::future::pending().await,
        }
    }
}

/// The write end of the caller's pipe at `field`, taken from the engine: no other writer can
/// attach to it while this function holds it.
async fn take_sink(
    engine: &Engine,
    field: &str,
    sink: Option<&contract::StreamChannelRef>,
) -> Result<Option<mpsc::Sender<ChannelItem>>, ErrorBody> {
    let Some(sink) = sink else {
        return Ok(None);
    };
    if !matches!(sink.direction, contract::ChannelDirection::Write) {
        return Err(failure(
            INVALID_REQUEST,
            format!("{field} must be the write end of a pipe"),
        ));
    }
    engine
        .channel_manager
        .take_sender(&sink.channel_id, &sink.access_key)
        .await
        .map(Some)
        .ok_or_else(|| {
            failure(
                INVALID_REQUEST,
                format!("{field} is not an open pipe, or another writer has it"),
            )
        })
}

/// A channel the adapter writes `LogRecord`s into, and the task copying each record's line to
/// the sinks. Dropped, it stops the copy and removes the channel, so the adapter's writer fails:
/// that aborts a build, and only ends a log stream.
struct Capture {
    channels: Arc<ChannelManager>,
    channel_id: String,
    task: JoinHandle<Option<u64>>,
}

impl Capture {
    /// Opens the channel; the adapter gets the returned write end.
    async fn open(engine: &Engine, sinks: &Sinks) -> Option<(contract::StreamChannelRef, Self)> {
        let channels = engine.channel_manager.clone();
        let (writer, reader) = channels.create_channel(LOG_BUFFER, None);
        let Some(mut records) = channels
            .take_receiver(&reader.channel_id, &reader.access_key)
            .await
        else {
            channels.remove_channel(&reader.channel_id);
            return None;
        };
        let sinks = sinks.clone();
        let task = tokio::spawn(async move {
            let mut last = None;
            while let Some(item) = records.recv().await {
                match item {
                    ChannelItem::Text(text) => {
                        if let Ok(record) = serde_json::from_str::<LogRecord>(&text) {
                            sinks.write(&record.stream, &record.line).await;
                            last = Some(record.cursor);
                        }
                    }
                    // A keepalive: records come in text frames only.
                    ChannelItem::Binary(_) => {}
                    ChannelItem::Abort(_) => break,
                }
            }
            last
        });
        let writer = contract::StreamChannelRef {
            channel_id: writer.channel_id,
            access_key: writer.access_key,
            direction: contract::ChannelDirection::Write,
        };
        Some((
            writer,
            Self {
                channels,
                channel_id: reader.channel_id,
                task,
            },
        ))
    }

    /// Waits for the adapter to close the channel, at most `within`; the cursor of the last
    /// record copied.
    async fn end(mut self, within: Duration) -> Option<u64> {
        tokio::time::timeout(within, &mut self.task)
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.task.abort();
        self.channels.remove_channel(&self.channel_id);
    }
}

/// `sandbox::build`: checks the adapter, then `worker::build` with a log channel whose lines go
/// to the sinks. A sink closing, or `timeout_ms` passing, drops the channel, which aborts the
/// build and keeps the previous image.
pub async fn build(engine: &Engine, input: Value) -> Result<Value, ErrorBody> {
    let request: SandboxBuildRequest = parse(input)?;
    let sinks = Sinks::open(engine, request.stdout.as_ref(), request.stderr.as_ref()).await?;
    let adapter = Adapter {
        engine,
        name: &request.adapter,
        namespace: &request.namespace,
    };
    let worker = &request.worker;
    let deadline = request
        .timeout_ms
        .map(|ms| Instant::now() + Duration::from_millis(ms));
    let cancelled = || {
        failure(
            OPERATION_CANCELLED,
            format!("the build of {worker} was cancelled: its caller left"),
        )
    };
    let timed_out = || {
        failure(
            TIMED_OUT,
            format!(
                "the build of {worker} ran out of its {}s",
                request.timeout_ms.unwrap_or_default() / 1000
            ),
        )
    };

    let info = tokio::select! {
        biased;
        () = sinks.closed() => return Err(cancelled()),
        () = until(deadline) => return Err(timed_out()),
        info = adapter.info() => info?,
    };
    let binary =
        matches!(request.source, WorkerSource::Binary { .. }) || request.base_image.is_some();
    adapter.check(&info, binary)?;

    let Some((logs, capture)) = Capture::open(engine, &sinks).await else {
        return Err(failure(
            ENGINE_CALL_FAILED,
            "could not open the channel for the build's output",
        ));
    };
    let call = WorkerBuildRequest {
        auth: request.auth.clone(),
        state_key: request.state_key.clone(),
        worker_name: worker.clone(),
        source: request.source.clone(),
        output_dir: request.build_dir.clone(),
        env: request.env.clone(),
        logs,
        base_image: request.base_image.clone(),
    };
    let budget = deadline.map_or(NO_LIMIT, |deadline| {
        deadline.saturating_duration_since(Instant::now()) + CALL_TIMEOUT
    });
    let built = tokio::select! {
        // An answer that is there wins: its image is already in place.
        biased;
        built = adapter.call::<WorkerBuildResponse>(WORKER_BUILD, &call, budget) => built,
        () = sinks.closed() => return Err(cancelled()),
        () = until(deadline) => return Err(timed_out()),
    };
    // The build's last lines, until the adapter closes the channel.
    capture.end(DRAIN_WAIT).await;
    let built = built.map_err(|error| adapter.failed("worker::build", error))?;
    reply(SandboxBuildResponse {
        build_id: built.build_id,
        base_image: built.base_image,
        built_at_ms: built.built_at_ms,
        adapter: info.adapter,
        adapter_version: info.adapter_version,
        contract: info.contract,
    })
}

/// One boot of the VM.
struct Vm {
    id: String,
    instance: u64,
}

enum Outcome {
    Running,
    /// The adapter did not say.
    Unknown,
    Exited {
        code: Option<i32>,
        signal: Option<i32>,
    },
}

fn outcome_of(status: Result<WorkerStatus, ErrorBody>) -> Outcome {
    match status {
        Ok(status) => match status.state {
            State::Preparing | State::Running => Outcome::Running,
            State::Unknown => Outcome::Unknown,
            State::Exited | State::Stopped => Outcome::Exited {
                code: status.exit_code,
                signal: status.signal,
            },
        },
        // S503: another boot of the same state replaced this one. S504: the adapter no longer
        // knows this one.
        Err(error) => match refusal(&error).map(|(code, _)| code) {
            Some(SandboxErrorCode::S503 | SandboxErrorCode::S504) => Outcome::Exited {
                code: None,
                signal: None,
            },
            _ => Outcome::Unknown,
        },
    }
}

/// The guest's log, copied to the sinks: one `worker::logs` stream at a time, asked for again
/// from the last record when it closed while the VM runs.
struct Follow {
    /// Reattached and no stream opened yet: the first starts at the log's end, after what the
    /// previous follower already copied.
    from_end: bool,
    cursor: Option<u64>,
    capture: Option<Capture>,
}

impl Follow {
    async fn open(&mut self, adapter: &Adapter<'_>, vm: &Vm, token: &str, sinks: &Sinks) {
        if self
            .capture
            .as_ref()
            .is_some_and(|capture| !capture.task.is_finished())
        {
            return;
        }
        if let Some(done) = self.capture.take()
            && let Some(last) = done.end(Duration::ZERO).await
        {
            self.cursor = Some(last);
        }
        let Some((channel, capture)) = Capture::open(adapter.engine, sinks).await else {
            return;
        };
        let request = WorkerLogsRequest {
            auth: token.to_string(),
            vm_id: vm.id.clone(),
            cursor: self.cursor,
            follow: true,
            channel,
            from_end: self.from_end,
        };
        if let Ok(started) = adapter
            .call::<WorkerLogsResponse>(WORKER_LOGS, &request, CALL_TIMEOUT)
            .await
        {
            if std::mem::take(&mut self.from_end) {
                self.cursor = started.cursor;
            }
            self.capture = Some(capture);
        }
    }

    /// Waits for the open stream to close and keeps its last cursor; `false` at once with none
    /// open. Cancel-safe: dropped before the stream closes, it leaves the stream in place.
    async fn closed(&mut self) -> bool {
        let Some(capture) = self.capture.as_mut() else {
            return false;
        };
        let last = (&mut capture.task).await.ok().flatten();
        self.capture = None;
        if let Some(last) = last {
            self.cursor = Some(last);
        }
        true
    }

    /// After the VM exited: reads what its log still holds, so its last lines are copied.
    async fn finish(&mut self, adapter: &Adapter<'_>, vm: &Vm, token: &str, sinks: &Sinks) {
        self.open(adapter, vm, token, sinks).await;
        if let Some(capture) = self.capture.take() {
            capture.end(DRAIN_WAIT).await;
        }
    }
}

/// `sandbox::start`: boots the build, or reattaches to its live boot (S502), copies the guest's
/// lines to the sinks, and stays with the VM until it exits or the caller leaves, which stops it.
pub async fn start(engine: &Engine, input: Value) -> Result<Value, ErrorBody> {
    let request: SandboxStartRequest = parse(input)?;
    let source = match (request.source.clone(), request.command.clone()) {
        (WorkerSource::Local { host_path, .. }, Some(command)) => WorkerSource::Local {
            host_path,
            run_override: Some(command),
        },
        (_, Some(_)) => {
            return Err(failure(
                INVALID_REQUEST,
                "command replaces the start of a path:// worker only: a registry package runs \
                 what it was published with",
            ));
        }
        (source, None) => source,
    };
    let sinks = Sinks::open(engine, request.stdout.as_ref(), request.stderr.as_ref()).await?;
    let adapter = Adapter {
        engine,
        name: &request.adapter,
        namespace: &request.namespace,
    };
    let token = request.auth.as_str();
    let grace = request
        .stop_grace_ms
        .map_or(DEFAULT_STOP_GRACE, Duration::from_millis);
    let no_vm = || SandboxStartResponse {
        vm_id: String::new(),
        instance: 0,
        stopped: true,
        exit_code: None,
        signal: None,
    };

    let info = tokio::select! {
        biased;
        () = sinks.closed() => return reply(no_vm()),
        info = adapter.info() => info?,
    };
    adapter.check(&info, matches!(source, WorkerSource::Binary { .. }))?;

    // The build is the guest's whole root: no `worker::prepare`, nothing installed at boot.
    let boot = WorkerStartRequest {
        auth: token.to_string(),
        state_key: request.state_key.clone(),
        worker_name: request.worker.clone(),
        source,
        env: request.env.clone(),
        engine_url: request.engine_url.clone(),
        build: Some(WorkerBuildRef {
            build_id: request.build_id.clone(),
            build_dir: request.build_dir.clone(),
        }),
    };
    let call = adapter.call::<WorkerStartResponse>(WORKER_START, &boot, START_TIMEOUT);
    tokio::pin!(call);
    // A caller that leaves does not abandon the start: its answer names the boot to stop. One
    // that does not come within the stop's budget leaves the VM to the adapter's lease.
    let (started, gone) = tokio::select! {
        biased;
        started = &mut call => (started, false),
        () = sinks.closed() => match tokio::time::timeout(grace, &mut call).await {
            Ok(started) => (started, true),
            Err(_) => return reply(no_vm()),
        },
    };
    let (started, reattached) = match started {
        Ok(started) => (started, false),
        Err(error) => match live_instance(&error) {
            Some(live) => (live, true),
            None => {
                let mut failed = adapter.failed("worker::start", error);
                if failed.code == SandboxErrorCode::S508.as_str() {
                    failed.message.push_str(&format!(
                        "; rebuild it with `iii call compose::build worker={}`",
                        request.worker
                    ));
                }
                return Err(failed);
            }
        },
    };
    let vm = Vm {
        id: started.vm_id,
        instance: started.instance,
    };
    if gone {
        let (exit_code, signal) = stop(&adapter, &vm, token, grace).await;
        return reply(stopped(&vm, exit_code, signal));
    }
    if reattached {
        sinks
            .write(
                &LogStream::Stderr,
                &format!(
                    "sandbox::start: reattached to VM {} instance {}",
                    vm.id, vm.instance
                ),
            )
            .await;
    }

    let mut follow = Follow {
        from_end: reattached,
        cursor: None,
        capture: None,
    };
    let mut unanswered_since: Option<Instant> = None;
    let mut pause = Duration::ZERO;
    loop {
        // The pause, the stream and the status all race the caller leaving.
        let tick = tokio::select! {
            biased;
            () = sinks.closed() => None,
            tick = poll(&adapter, &vm, token, &sinks, &mut follow, pause) => Some(tick),
        };
        let Some((outcome, followed)) = tick else {
            follow.capture = None;
            let (exit_code, signal) = stop(&adapter, &vm, token, grace).await;
            return reply(stopped(&vm, exit_code, signal));
        };
        pause = RETRY_INTERVAL;
        // A stream the adapter kept open is an answer, so the wait for one starts over.
        if followed {
            unanswered_since = None;
        }
        match outcome {
            Outcome::Exited { code, signal } => {
                tokio::select! {
                    biased;
                    () = sinks.closed() => {}
                    () = follow.finish(&adapter, &vm, token, &sinks) => {}
                }
                return exited(&request.worker, &vm, code, signal);
            }
            Outcome::Running => unanswered_since = None,
            Outcome::Unknown => {
                let since = *unanswered_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= LOST_AFTER {
                    return Err(failure(
                        ENGINE_CALL_FAILED,
                        format!(
                            "{} has not answered about VM {} instance {} for {}s; giving it up, \
                             as the adapter's lease does",
                            adapter.name,
                            vm.id,
                            vm.instance,
                            LOST_AFTER.as_secs()
                        ),
                    ));
                }
            }
        }
    }
}

/// One pass: after `pause`, keeps the log stream open and waits for it to close, then asks how
/// the VM is. The adapter closes the stream once the VM has exited and its log is drained, so a
/// running VM costs no call. With no stream open, it asks at once.
async fn poll(
    adapter: &Adapter<'_>,
    vm: &Vm,
    token: &str,
    sinks: &Sinks,
    follow: &mut Follow,
    pause: Duration,
) -> (Outcome, bool) {
    tokio::time::sleep(pause).await;
    follow.open(adapter, vm, token, sinks).await;
    let opened = Instant::now();
    // One that closed at once says nothing about the adapter.
    let followed = follow.closed().await && opened.elapsed() >= RETRY_INTERVAL;
    let request = WorkerStatusRequest {
        auth: token.to_string(),
        vm_id: vm.id.clone(),
        instance: vm.instance,
    };
    let status = adapter
        .call::<WorkerStatus>(WORKER_STATUS, &request, CALL_TIMEOUT)
        .await;
    (outcome_of(status), followed)
}

/// `worker::stop`, asked again until the adapter confirms the VM ended or `grace` passes; every
/// call and its own grace fit in what is left. A stop never confirmed leaves the VM to the
/// adapter's lease, which stops a VM nobody follows.
async fn stop(
    adapter: &Adapter<'_>,
    vm: &Vm,
    token: &str,
    grace: Duration,
) -> (Option<i32>, Option<i32>) {
    let deadline = Instant::now() + grace;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return (None, None);
        }
        let request = WorkerStopRequest {
            auth: token.to_string(),
            vm_id: vm.id.clone(),
            instance: vm.instance,
            // The rest of what is left is for the answer.
            grace_ms: (left * 8 / 9).as_millis() as u64,
        };
        let status = adapter
            .call::<WorkerStatus>(WORKER_STOP, &request, left)
            .await;
        if let Outcome::Exited { code, signal } = outcome_of(status) {
            return (code, signal);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        tokio::time::sleep(RETRY_INTERVAL.min(left)).await;
    }
}

fn stopped(vm: &Vm, exit_code: Option<i32>, signal: Option<i32>) -> SandboxStartResponse {
    SandboxStartResponse {
        vm_id: vm.id.clone(),
        instance: vm.instance,
        stopped: true,
        exit_code,
        signal,
    }
}

/// The answer for a VM that exited by itself: success only with code 0.
fn exited(
    worker: &str,
    vm: &Vm,
    code: Option<i32>,
    signal: Option<i32>,
) -> Result<Value, ErrorBody> {
    let how = match (code, signal) {
        (Some(0), None) => {
            return reply(SandboxStartResponse {
                vm_id: vm.id.clone(),
                instance: vm.instance,
                stopped: false,
                exit_code: Some(0),
                signal: None,
            });
        }
        (Some(code), _) => format!("exited with code {code}"),
        (None, Some(signal)) => format!("ended on signal {signal}"),
        (None, None) => "ended without reporting how".to_string(),
    };
    Err(failure(
        WORKER_EXITED,
        format!("{worker}: VM {} instance {} {how}", vm.id, vm.instance),
    ))
}
