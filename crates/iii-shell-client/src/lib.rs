// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Async pipe-mode client for the iii shell-exec channel.
//!
//! ```text
//!   caller                           iii-shell-client                 relay (host)
//!   ──────                           ────────────────                 ────────────
//!   Session::connect(path) ──────▶   open UnixStream
//!                                    verify ownership (dev, ino, mode)
//!                                    read 4-byte id_offset ◀──────────  write handshake
//!   Session::run(req, sink,   ─────▶ encode Request frame
//!       timeout, stdin)              write frame
//!                                    (optional) write Stdin + EOF frame
//!                                    loop: read frame
//!                                      Stdout → sink.on_stdout() (drain-always)
//!                                      Stderr → sink.on_stderr() (drain-always)
//!                                      Exited → return ExitStatus
//!                                    on timeout: send Signal{KILL}, wait 1s,
//!                                      return timed_out=true
//! ```
//!
//! Design choices:
//!
//! - `OutputSink` caps output at the caller layer, but the session
//!   keeps draining stdout/stderr frames even after the sink says
//!   `StopAppending`, so the terminal `Exited` frame still arrives.
//!   Naïve `run() -> {stdout, stderr, exit}` APIs can't enforce
//!   bounded accumulation without this pattern.
//! - Pipe mode only. TTY, SIGWINCH, raw-mode, SIGINT forwarding are
//!   interactive-CLI concerns and stay in consumer binaries
//!   (iii-worker's `shell_client.rs`).
//! - `verify_shell_socket_ownership` is pub and runs pre- AND
//!   post-connect so a same-uid attacker can't swap a planted socket
//!   mid-handshake.
//! - Stdin is a single pre-packaged byte buffer (or None). Interactive
//!   stdin pumping is out of scope for the crate — callers with
//!   interactive input wire their own pump against the Session.
//!   Simplifies the crate and side-steps the "stdin task must never
//!   return on the happy path" invariant that tripped shell_client.rs.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use iii_shell_proto::{
    FRAME_HEADER_SIZE, MAX_FRAME_SIZE, ShellMessage, decode_frame_body, encode_frame,
    flags::FLAG_TERMINAL,
};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// POSIX signal numbers. Hard-coded because the values are stable
/// across every Linux target we run on, so pulling them from libc
/// just adds a dependency hop.
const SIG_KILL: i32 = 9;

/// How long to wait for the `Exited` acknowledgement after we've
/// already decided to give up on a timed-out session. Budget for the
/// guest to observe SIGKILL, reap the child, and emit its terminal
/// frame.
const POST_KILL_GRACE: Duration = Duration::from_millis(1000);

/// How long to wait on the 4-byte `id_offset` handshake before we
/// conclude the relay is wedged. Small because a healthy relay writes
/// the handshake immediately after accept.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a single `write_all` is allowed to block. Protects
/// against a dead relay whose write side has filled and will never
/// drain.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// Specification for a single `ShellMessage::Request` frame.
///
/// Pipe-mode only: `tty`, `rows`, and `cols` are always `false`/0.
/// Callers that need TTY wire their own Request frame and don't use
/// this crate's `Session::run` helper.
#[derive(Debug, Clone, Default)]
pub struct RequestSpec {
    /// Program to execute inside the guest VM. Dispatcher does not
    /// PATH-search; pass an absolute path or rely on `/bin/sh -c`.
    pub cmd: String,
    /// argv tail (excluding `cmd` itself).
    pub args: Vec<String>,
    /// Working directory inside the guest. `None` inherits the
    /// dispatcher's cwd (typically `/workspace`).
    pub cwd: Option<String>,
    /// Environment variable overrides in `KEY=VALUE` form. Layered on
    /// top of the guest init's environment.
    pub env: Vec<String>,
    /// Pre-packaged stdin bytes sent as a single `Stdin` frame
    /// followed by an EOF `Stdin { data_b64: "" }` frame. `None`
    /// skips the pump entirely.
    pub stdin: Option<Vec<u8>>,
}

/// Terminal exit status of a successful `Session::run`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitStatus {
    /// Child's exit code, or `None` when the session timed out with
    /// no `Exited` frame received.
    pub code: Option<i32>,
    /// True when the outer timeout fired.
    pub timed_out: bool,
}

/// Flow control returned from an `OutputSink` callback.
///
/// `StopAppending` tells the session we've hit our local cap and
/// don't want more bytes in our buffer, but the session still keeps
/// reading frames so `Exited` arrives on time. Callers track how
/// many bytes they actually stored and the session reports the cap
/// hit via `stdout_truncated` / `stderr_truncated` on `ExitStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Keep forwarding bytes to the sink.
    Continue,
    /// Session should keep reading frames (so `Exited` arrives) but
    /// no more bytes need to land in this sink.
    StopAppending,
}

/// Where to deposit stdout/stderr bytes as they stream in.
///
/// The session passes each chunk from the wire to the sink. Implement
/// `Continue` for unbounded buffering, or `StopAppending` after your
/// cap is reached. The session tracks per-stream truncation and
/// surfaces it on the return value of `Session::run`.
pub trait OutputSink {
    /// Called once per Stdout frame with the decoded bytes.
    fn on_stdout(&mut self, bytes: &[u8]) -> Flow;
    /// Called once per Stderr frame with the decoded bytes.
    fn on_stderr(&mut self, bytes: &[u8]) -> Flow;
}

/// Result of `Session::run` including truncation flags set by the
/// `OutputSink`'s `StopAppending` decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOutcome {
    pub status: ExitStatus,
    /// True if `on_stdout` returned `StopAppending` at any point.
    pub stdout_truncated: bool,
    /// True if `on_stderr` returned `StopAppending` at any point.
    pub stderr_truncated: bool,
}

/// Typed failure modes. Every variant is either a client-observable
/// precondition failure (`WorkerMissing`, `Permission`) or a
/// protocol-level surprise (`ProtocolViolation`, `SessionTerminated`).
/// The dispatcher's own `Error { message }` frame is surfaced as
/// `DispatcherError` to keep the error handling symmetric.
#[derive(Debug, Error)]
pub enum VmClientError {
    /// The socket path did not exist, is not a socket, or belongs to
    /// a different uid than the caller.
    #[error("worker socket missing or wrong owner: {0}")]
    WorkerMissing(String),

    /// EACCES reading or connecting to the socket. Usually means
    /// caller and worker process run as different uids.
    #[error("permission denied on worker socket: {0}")]
    Permission(String),

    /// The socket file exists but nothing is listening. Relay panicked
    /// or the VM is restarting.
    #[error("worker relay not accepting connections: {0}")]
    RelayDown(String),

    /// Relay closed the connection immediately after accept.
    #[error("relay rejected connection (uid mismatch?): {0}")]
    AuthRejected(String),

    /// Socket swapped between pre-connect stat and post-connect stat.
    /// Catches a same-uid attacker planting a socket mid-handshake.
    #[error("socket inode changed between pre-check and connect: {0}")]
    SocketSwapped(String),

    /// Less than 4 bytes on the handshake (channel died mid-greeting).
    #[error("handshake truncated: expected 4 bytes, got {got}")]
    HandshakeTruncated { got: usize },

    /// 2s handshake timeout expired.
    #[error("handshake timed out after {0:?}")]
    HandshakeTimeout(Duration),

    /// Request frame would exceed `MAX_FRAME_SIZE` (4 MiB). Shrink
    /// env/cwd/args.
    #[error("request frame too large: {size} bytes (max {MAX_FRAME_SIZE})")]
    RequestTooLarge { size: usize },

    /// `write_all` took longer than `WRITE_TIMEOUT` — relay's write
    /// ring buffer is full and not draining.
    #[error("write to relay blocked; the command may still be running in the VM")]
    WriteBlocked,

    /// Any other write failure.
    #[error("write to relay failed: {0}")]
    WriteFailed(String),

    /// Frame codec rejected the bytes. Relay or guest is out of spec.
    #[error("protocol violation: {0}")]
    ProtocolViolation(String),

    /// Relay / VM closed the stream before `Exited` arrived.
    #[error("session terminated: vm disconnected mid-run")]
    SessionTerminated,

    /// The guest dispatcher reported a terminal error on its side
    /// (spawn failure, PTY allocation, etc.).
    #[error("dispatcher error: {0}")]
    DispatcherError(String),

    /// Any other I/O error while reading response frames.
    #[error("io: {0}")]
    Io(String),

    /// `encode_frame` failed. Unreachable for types we construct here,
    /// but forwarded in case callers stuff a variant that serde rejects.
    #[error("encode frame: {0}")]
    Encode(String),

    /// Base64 decode failed on a Stdout/Stderr frame's data_b64 field.
    #[error("base64 decode: {0}")]
    Base64(String),
}

/// Resolve the shell-channel socket path for a worker under
/// `$HOME/.iii/managed/<name>/shell.sock`. Validates that
/// `worker_name` is a single path segment with no `..`, no null
/// bytes, and no interior slashes so a caller-controlled name can't
/// redirect the connect to an arbitrary path.
pub fn shell_socket_path(worker_name: &str) -> Result<PathBuf, VmClientError> {
    if worker_name.is_empty() {
        return Err(VmClientError::WorkerMissing(
            "worker_name is empty".to_string(),
        ));
    }
    if worker_name.contains('\0') {
        return Err(VmClientError::WorkerMissing(format!(
            "worker_name must not contain NUL bytes: {worker_name:?}"
        )));
    }
    let p = Path::new(worker_name);
    if p.is_absolute() {
        return Err(VmClientError::WorkerMissing(format!(
            "worker_name must not be absolute: {worker_name:?}"
        )));
    }
    let mut comps = p.components();
    match (comps.next(), comps.next()) {
        (Some(std::path::Component::Normal(_)), None) => {}
        _ => {
            return Err(VmClientError::WorkerMissing(format!(
                "worker_name must be a single path segment: {worker_name:?}"
            )));
        }
    }
    let home = dirs::home_dir()
        .ok_or_else(|| VmClientError::WorkerMissing("HOME is not set".to_string()))?;
    Ok(home
        .join(".iii/managed")
        .join(worker_name)
        .join("shell.sock"))
}

/// Verify the shell socket belongs to us. Returns (dev, ino, mode)
/// so the caller can compare pre- and post-connect fingerprints.
/// Refuses non-sockets, non-euid owners, group/world-accessible modes.
#[cfg(unix)]
pub fn verify_shell_socket_ownership(sock: &Path) -> Result<(u64, u64, u32), VmClientError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    // symlink_metadata so a planted symlink isn't followed.
    let meta = std::fs::symlink_metadata(sock).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => VmClientError::WorkerMissing(format!(
            "shell socket {} not present — start the worker first",
            sock.display()
        )),
        std::io::ErrorKind::PermissionDenied => {
            VmClientError::Permission(format!("stat {}: {e}", sock.display()))
        }
        _ => VmClientError::WorkerMissing(format!("stat {}: {e}", sock.display())),
    })?;
    if !meta.file_type().is_socket() {
        return Err(VmClientError::WorkerMissing(format!(
            "refusing to connect to {}: not a Unix socket (type: {:?})",
            sock.display(),
            meta.file_type()
        )));
    }
    let our_uid = unsafe { libc::geteuid() };
    if meta.uid() != our_uid {
        return Err(VmClientError::Permission(format!(
            "refusing to connect to {}: socket is owned by uid {} (expected {})",
            sock.display(),
            meta.uid(),
            our_uid
        )));
    }
    let mode = meta.mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(VmClientError::Permission(format!(
            "refusing to connect to {}: mode {:o} is group/world-accessible \
             (expected 0o600 or stricter)",
            sock.display(),
            mode
        )));
    }
    Ok((meta.dev(), meta.ino(), mode))
}

#[cfg(not(unix))]
pub fn verify_shell_socket_ownership(_sock: &Path) -> Result<(u64, u64, u32), VmClientError> {
    // Non-Unix hosts don't support AF_UNIX. Return a sentinel so the
    // pre/post comparison is still a no-op equality check.
    Ok((0, 0, 0))
}

/// A connected, handshake-completed session. Drives exactly one
/// `Session::run` then the session is consumed — opening a fresh
/// session per call keeps the state machine trivial.
pub struct Session {
    stream: UnixStream,
    corr_id: u32,
}

impl Session {
    /// Open the socket, verify its pre/post fingerprint, read the
    /// 4-byte `id_offset`, and return a Session ready for one `run`.
    pub async fn connect(sock: &Path) -> Result<Self, VmClientError> {
        let pre_fp = verify_shell_socket_ownership(sock)?;
        let mut stream = UnixStream::connect(sock).await.map_err(|e| {
            // Map common connect errors to typed variants so callers
            // can branch without string-matching.
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    VmClientError::WorkerMissing(format!("connect({}): {e}", sock.display()))
                }
                std::io::ErrorKind::PermissionDenied => {
                    VmClientError::Permission(format!("connect({}): {e}", sock.display()))
                }
                std::io::ErrorKind::ConnectionRefused => {
                    VmClientError::RelayDown(format!("connect({}): {e}", sock.display()))
                }
                _ => VmClientError::Io(format!("connect({}): {e}", sock.display())),
            }
        })?;
        // Post-connect stat catches a same-uid attacker who swapped
        // the socket between `verify` and `connect`.
        let post_fp = verify_shell_socket_ownership(sock)?;
        if pre_fp != post_fp {
            return Err(VmClientError::SocketSwapped(format!(
                "socket {} fingerprint changed (pre={:?} post={:?})",
                sock.display(),
                pre_fp,
                post_fp
            )));
        }
        // Handshake: 4 big-endian bytes = id_offset. Clients pick ids
        // in `[id_offset+1, id_offset+ID_RANGE_STEP)`. We only issue
        // one corr_id per session so `+1` is enough.
        let mut handshake = [0u8; 4];
        let read = tokio::time::timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut handshake))
            .await
            .map_err(|_| VmClientError::HandshakeTimeout(HANDSHAKE_TIMEOUT))?;
        match read {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // Less than 4 bytes. Could be peer cred rejection
                // (same-uid check failed) or a relay panic.
                return Err(VmClientError::AuthRejected(format!(
                    "relay closed before handshake: {e}"
                )));
            }
            Err(e) => return Err(VmClientError::Io(format!("handshake read: {e}"))),
        }
        let id_offset = u32::from_be_bytes(handshake);
        let corr_id = id_offset + 1;
        Ok(Self { stream, corr_id })
    }

    /// Send `Request`, (optional) stdin + EOF, drive the frame loop
    /// until `Exited` or timeout. Consumes the session.
    pub async fn run(
        mut self,
        req: RequestSpec,
        sink: &mut dyn OutputSink,
        timeout: Option<Duration>,
    ) -> Result<RunOutcome, VmClientError> {
        let request = ShellMessage::Request {
            cmd: req.cmd,
            args: req.args,
            env: req.env,
            cwd: req.cwd,
            tty: false,
            rows: 0,
            cols: 0,
        };
        let frame = encode_frame(self.corr_id, 0, &request).map_err(|e| match e {
            iii_shell_proto::ShellCodecError::InvalidFrameLength(n) => {
                VmClientError::RequestTooLarge { size: n }
            }
            other => VmClientError::Encode(other.to_string()),
        })?;
        write_frame_bounded(&mut self.stream, &frame).await?;

        // Optional pre-packaged stdin: one Stdin frame with the bytes,
        // then one zero-byte Stdin frame as EOF.
        if let Some(data) = req.stdin.as_deref() {
            if !data.is_empty() {
                let stdin_frame = encode_frame(
                    self.corr_id,
                    0,
                    &ShellMessage::Stdin {
                        data_b64: B64.encode(data),
                    },
                )
                .map_err(|e| VmClientError::Encode(e.to_string()))?;
                write_frame_bounded(&mut self.stream, &stdin_frame).await?;
            }
            let eof = encode_frame(
                self.corr_id,
                0,
                &ShellMessage::Stdin {
                    data_b64: String::new(),
                },
            )
            .map_err(|e| VmClientError::Encode(e.to_string()))?;
            write_frame_bounded(&mut self.stream, &eof).await?;
        }

        let (mut reader, mut writer) = self.stream.into_split();
        let mut stdout_truncated = false;
        let mut stderr_truncated = false;
        let deadline = timeout.map(|d| tokio::time::Instant::now() + d);

        loop {
            let frame_opt = match deadline {
                Some(dl) => {
                    let remaining = dl.saturating_duration_since(tokio::time::Instant::now());
                    if remaining.is_zero() {
                        let _ = send_signal(&mut writer, self.corr_id, SIG_KILL).await;
                        let status = await_exited_with_grace(&mut reader, self.corr_id).await;
                        return Ok(RunOutcome {
                            status: ExitStatus {
                                code: status,
                                timed_out: true,
                            },
                            stdout_truncated,
                            stderr_truncated,
                        });
                    }
                    match tokio::time::timeout(remaining, read_one_frame(&mut reader)).await {
                        Ok(v) => v,
                        Err(_) => {
                            let _ = send_signal(&mut writer, self.corr_id, SIG_KILL).await;
                            let status = await_exited_with_grace(&mut reader, self.corr_id).await;
                            return Ok(RunOutcome {
                                status: ExitStatus {
                                    code: status,
                                    timed_out: true,
                                },
                                stdout_truncated,
                                stderr_truncated,
                            });
                        }
                    }
                }
                None => read_one_frame(&mut reader).await,
            };

            let (got_corr, flags, msg) = match frame_opt? {
                Some(f) => f,
                None => return Err(VmClientError::SessionTerminated),
            };
            if got_corr != self.corr_id {
                tracing::warn!(
                    "iii-shell-client: ignoring frame for corr_id={got_corr}, expected {}",
                    self.corr_id
                );
                continue;
            }
            match msg {
                ShellMessage::Started { pid: _ } => {
                    tracing::debug!("session started, corr_id={}", self.corr_id);
                }
                ShellMessage::Stdout { data_b64 } => {
                    let bytes = B64
                        .decode(data_b64.as_bytes())
                        .map_err(|e| VmClientError::Base64(e.to_string()))?;
                    if matches!(sink.on_stdout(&bytes), Flow::StopAppending) {
                        stdout_truncated = true;
                    }
                }
                ShellMessage::Stderr { data_b64 } => {
                    let bytes = B64
                        .decode(data_b64.as_bytes())
                        .map_err(|e| VmClientError::Base64(e.to_string()))?;
                    if matches!(sink.on_stderr(&bytes), Flow::StopAppending) {
                        stderr_truncated = true;
                    }
                }
                ShellMessage::Exited { code } => {
                    return Ok(RunOutcome {
                        status: ExitStatus {
                            code: Some(code),
                            timed_out: false,
                        },
                        stdout_truncated,
                        stderr_truncated,
                    });
                }
                ShellMessage::Error { message } => {
                    if flags & FLAG_TERMINAL != 0 {
                        return Err(VmClientError::DispatcherError(message));
                    }
                    tracing::warn!("dispatcher non-terminal error: {message}");
                }
                // Host-directed messages (Request/Stdin/Resize/Signal)
                // should never come back from the guest. Log and ignore.
                other => {
                    tracing::warn!("unexpected guest-originated variant: {other:?}");
                }
            }
        }
    }
}

async fn write_frame_bounded(stream: &mut UnixStream, frame: &[u8]) -> Result<(), VmClientError> {
    match tokio::time::timeout(WRITE_TIMEOUT, stream.write_all(frame)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(VmClientError::WriteFailed(e.to_string())),
        Err(_) => Err(VmClientError::WriteBlocked),
    }
}

async fn send_signal(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    corr_id: u32,
    signal: i32,
) -> Result<(), VmClientError> {
    let frame = encode_frame(corr_id, 0, &ShellMessage::Signal { signal })
        .map_err(|e| VmClientError::Encode(e.to_string()))?;
    match tokio::time::timeout(WRITE_TIMEOUT, writer.write_all(&frame)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(VmClientError::WriteFailed(e.to_string())),
        Err(_) => Err(VmClientError::WriteBlocked),
    }
}

/// After we've given up on a timed-out session and sent SIGKILL,
/// spend up to `POST_KILL_GRACE` trying to collect the Exited frame.
/// Best-effort — returns `Some(code)` if we got it, `None` otherwise.
async fn await_exited_with_grace(
    reader: &mut tokio::net::unix::OwnedReadHalf,
    expected_corr_id: u32,
) -> Option<i32> {
    let deadline = tokio::time::Instant::now() + POST_KILL_GRACE;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        let frame = match tokio::time::timeout(remaining, read_one_frame(reader)).await {
            Ok(Ok(Some(f))) => f,
            _ => return None,
        };
        if frame.0 != expected_corr_id {
            continue;
        }
        if let ShellMessage::Exited { code } = frame.2 {
            return Some(code);
        }
    }
}

/// Read one complete frame from any AsyncRead. Public so consumers
/// like iii-worker's CLI can share the same frame decoder instead of
/// carrying a duplicate copy. Returns `Ok(None)` on clean EOF at a
/// frame boundary.
pub async fn read_frame_async<R>(
    reader: &mut R,
) -> Result<Option<(u32, u8, ShellMessage)>, VmClientError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    match read_exact_or_eof_generic(reader, &mut len_buf).await? {
        None => return Ok(None),
        Some(()) => {}
    }
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if !(FRAME_HEADER_SIZE..=MAX_FRAME_SIZE).contains(&frame_len) {
        return Err(VmClientError::ProtocolViolation(format!(
            "frame length {frame_len} out of range"
        )));
    }
    let mut body: Vec<u8> = Vec::with_capacity(frame_len);
    unsafe { body.set_len(frame_len) };
    reader
        .read_exact(&mut body)
        .await
        .map_err(|e| VmClientError::Io(format!("short read on frame body: {e}")))?;
    decode_frame_body(&body)
        .map(Some)
        .map_err(|e| VmClientError::ProtocolViolation(e.to_string()))
}

async fn read_exact_or_eof_generic<R>(
    reader: &mut R,
    buf: &mut [u8],
) -> Result<Option<()>, VmClientError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut read = 0;
    while read < buf.len() {
        match reader
            .read(&mut buf[read..])
            .await
            .map_err(|e| VmClientError::Io(e.to_string()))?
        {
            0 => {
                if read == 0 {
                    return Ok(None);
                }
                return Err(VmClientError::ProtocolViolation(
                    "partial read on frame length prefix".into(),
                ));
            }
            n => read += n,
        }
    }
    Ok(Some(()))
}

/// Read one complete frame from the socket. Returns `Ok(None)` on
/// clean EOF at a frame boundary.
async fn read_one_frame(
    reader: &mut tokio::net::unix::OwnedReadHalf,
) -> Result<Option<(u32, u8, ShellMessage)>, VmClientError> {
    let mut len_buf = [0u8; 4];
    match read_exact_or_eof(reader, &mut len_buf).await? {
        None => return Ok(None),
        Some(()) => {}
    }
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if !(FRAME_HEADER_SIZE..=MAX_FRAME_SIZE).contains(&frame_len) {
        return Err(VmClientError::ProtocolViolation(format!(
            "frame length {frame_len} out of range"
        )));
    }
    // read_exact overwrites every byte, so uninit is fine — same
    // SAFETY rationale as shell_relay::read_frame.
    let mut body: Vec<u8> = Vec::with_capacity(frame_len);
    unsafe { body.set_len(frame_len) };
    reader
        .read_exact(&mut body)
        .await
        .map_err(|e| VmClientError::Io(format!("short read on frame body: {e}")))?;
    decode_frame_body(&body)
        .map(Some)
        .map_err(|e| VmClientError::ProtocolViolation(e.to_string()))
}

async fn read_exact_or_eof(
    reader: &mut tokio::net::unix::OwnedReadHalf,
    buf: &mut [u8],
) -> Result<Option<()>, VmClientError> {
    let mut read = 0;
    while read < buf.len() {
        match reader
            .read(&mut buf[read..])
            .await
            .map_err(|e| VmClientError::Io(e.to_string()))?
        {
            0 => {
                if read == 0 {
                    return Ok(None);
                }
                return Err(VmClientError::ProtocolViolation(
                    "partial read on frame length prefix".into(),
                ));
            }
            n => read += n,
        }
    }
    Ok(Some(()))
}

/// An `OutputSink` that buffers stdout/stderr up to a per-stream cap,
/// then returns `StopAppending` while staying cheap to compare (e.g.
/// in tests). Most callers (vm-worker, iii-worker CLI pipe mode) use
/// this directly.
pub struct VecSink {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    cap: usize,
}

impl VecSink {
    pub fn with_cap(cap: usize) -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
            cap,
        }
    }
}

impl OutputSink for VecSink {
    fn on_stdout(&mut self, bytes: &[u8]) -> Flow {
        if self.stdout.len() >= self.cap {
            return Flow::StopAppending;
        }
        let room = self.cap.saturating_sub(self.stdout.len());
        let take = bytes.len().min(room);
        self.stdout.extend_from_slice(&bytes[..take]);
        if take < bytes.len() {
            Flow::StopAppending
        } else {
            Flow::Continue
        }
    }
    fn on_stderr(&mut self, bytes: &[u8]) -> Flow {
        if self.stderr.len() >= self.cap {
            return Flow::StopAppending;
        }
        let room = self.cap.saturating_sub(self.stderr.len());
        let take = bytes.len().min(room);
        self.stderr.extend_from_slice(&bytes[..take]);
        if take < bytes.len() {
            Flow::StopAppending
        } else {
            Flow::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_socket_path_rejects_empty() {
        assert!(matches!(
            shell_socket_path(""),
            Err(VmClientError::WorkerMissing(_))
        ));
    }

    #[test]
    fn shell_socket_path_rejects_null() {
        assert!(matches!(
            shell_socket_path("foo\0bar"),
            Err(VmClientError::WorkerMissing(_))
        ));
    }

    #[test]
    fn shell_socket_path_rejects_absolute() {
        assert!(matches!(
            shell_socket_path("/etc/passwd"),
            Err(VmClientError::WorkerMissing(_))
        ));
    }

    #[test]
    fn shell_socket_path_rejects_traversal() {
        assert!(matches!(
            shell_socket_path("../evil"),
            Err(VmClientError::WorkerMissing(_))
        ));
        assert!(matches!(
            shell_socket_path("a/b"),
            Err(VmClientError::WorkerMissing(_))
        ));
    }

    #[test]
    fn shell_socket_path_accepts_single_segment() {
        let p = shell_socket_path("pdfkit").expect("valid");
        assert!(p.ends_with(".iii/managed/pdfkit/shell.sock"));
    }

    #[test]
    fn vec_sink_truncates_at_cap() {
        let mut s = VecSink::with_cap(4);
        assert_eq!(s.on_stdout(b"ab"), Flow::Continue);
        assert_eq!(s.on_stdout(b"cdef"), Flow::StopAppending);
        assert_eq!(s.stdout, b"abcd");
        assert_eq!(s.on_stdout(b"x"), Flow::StopAppending);
        assert_eq!(s.stdout, b"abcd");
    }

    #[test]
    fn vec_sink_stdout_and_stderr_independent() {
        let mut s = VecSink::with_cap(3);
        assert_eq!(s.on_stdout(b"out"), Flow::Continue);
        assert_eq!(s.on_stderr(b"err"), Flow::Continue);
        assert_eq!(s.stdout, b"out");
        assert_eq!(s.stderr, b"err");
        assert_eq!(s.on_stdout(b"x"), Flow::StopAppending);
        assert_eq!(s.on_stderr(b"y"), Flow::StopAppending);
    }
}
