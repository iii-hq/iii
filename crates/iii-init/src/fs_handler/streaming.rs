// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Streaming handlers for `WriteStart` (upload) and `ReadStart` (download).
//!
//! Both handlers are **sync** — they use `std::sync::mpsc` channels,
//! `std::fs`, `std::io::Write` / `std::io::Read`. No tokio anywhere.
//!
//! The `handle_write_start` function receives a `Receiver<ShellMessage>`
//! that the dispatcher populates by forwarding `FsChunk` / `FsEnd`
//! frames for the same correlation id. Once it observes `FsEnd` it
//! fsyncs + renames the temp file atomically.
//!
//! The `handle_read_start` function opens the file, sends `FsMeta` then
//! streams 64 KiB chunks as `FsChunk` frames, and closes with `FsEnd`,
//! all through the shared outbound `Writer` (a `SyncSender<Vec<u8>>`
//! that feeds the wire writer thread).

use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use base64::Engine;
use iii_shell_proto::{FsReadMeta, FsResult, ShellMessage};

use super::ops::temp_sibling;
use super::{FsCallResult, FsError, parse_mode};
use crate::shell_dispatcher::Writer;

/// Size of each chunk sent during `ReadStart` streaming.
const READ_CHUNK_SIZE: usize = 64 * 1024;

/// Maximum idle time between `FsChunk` / `FsEnd` frames during a
/// `WriteStart` session. The dispatcher's `fs_writes` registry
/// holds the per-corr_id sender for the lifetime of the session,
/// so a blocking `recv()` would wait forever if the host trigger
/// handler stalls (caller-side data channel hard-terminated, the
/// worker's `ChannelReader::next_binary()` never resolves, etc.).
/// On expiry we treat the session as aborted: unlink the temp
/// file, return `S218`. The worker's outer 30s SDK timeout still
/// fires, but the supervisor releases its temp first so callers
/// don't accumulate `.iii-tmp-*` leaks across aborted uploads.
/// 30s is well above realistic per-chunk inter-arrival latency
/// for a healthy 64 KiB-frame channel — a real upload sees a
/// chunk every few milliseconds.
const WRITE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

// ──────────────────────────────────────────────────────────────────────────────
// Upload  (host → guest)
// ──────────────────────────────────────────────────────────────────────────────

/// Handle a `WriteStart` upload session.
///
/// Pulls `FsChunk` / `FsEnd` frames from `inbound` (populated by the
/// dispatcher routing loop), writes chunks to a temp sibling, fsyncs,
/// sets `mode`, then atomically renames to `path`.
///
/// On early channel close (sender dropped before `FsEnd`) the temp file
/// is unlinked and `S218` is returned.
///
/// Errors:
/// - `S210` — bad octal `mode` or bad base64 in a chunk.
/// - `S211` — parent dir missing and `parents=false`.
/// - `S218` — channel closed before `FsEnd`.
/// - `S216` — I/O error writing or renaming.
pub fn handle_write_start(
    path: String,
    mode: String,
    parents: bool,
    inbound: Receiver<ShellMessage>,
) -> FsCallResult {
    handle_write_start_with_timeout(path, mode, parents, inbound, WRITE_IDLE_TIMEOUT)
}

/// Internal entry point exposed to tests so they can drive the
/// timeout path without waiting `WRITE_IDLE_TIMEOUT`. Production
/// always calls `handle_write_start` (which passes the const).
pub(crate) fn handle_write_start_with_timeout(
    path: String,
    mode: String,
    parents: bool,
    inbound: Receiver<ShellMessage>,
    idle_timeout: Duration,
) -> FsCallResult {
    let bits = parse_mode(&mode)?;
    let target = std::path::PathBuf::from(&path);

    // Resolve / create parent.
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            if parents {
                std::fs::create_dir_all(parent).map_err(|e| FsError::from_io(&path, e))?;
            } else if !parent.exists() {
                return Err(FsError::new(
                    "S211",
                    format!("parent not found: {}", parent.display()),
                ));
            }
        }
    }

    let tmp = temp_sibling(&target);
    let mut f = match std::fs::File::create(&tmp) {
        Ok(f) => f,
        Err(e) => return Err(FsError::from_io(&path, e)),
    };

    let mut written: u64 = 0;
    let mut ended_cleanly = false;
    let mut timed_out = false;

    // Drain the inbound channel until FsEnd, the sender drops, or
    // the per-recv idle timeout fires. recv_timeout is the only way
    // the supervisor side can notice a stalled upload — there's no
    // host-disconnect signal that flows down to this thread otherwise
    // (the dispatcher's per-corr_id sender lives in fs_writes for
    // the session lifetime).
    loop {
        let msg = match inbound.recv_timeout(idle_timeout) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => {
                timed_out = true;
                break;
            }
            Err(RecvTimeoutError::Disconnected) => break, // sender dropped
        };
        match msg {
            ShellMessage::FsChunk { data_b64 } => {
                let bytes = match base64::engine::general_purpose::STANDARD.decode(&data_b64) {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp);
                        return Err(FsError::new("S210", format!("bad base64 in chunk: {e}")));
                    }
                };
                if let Err(e) = f.write_all(&bytes) {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(FsError::from_io(&path, e));
                }
                written += bytes.len() as u64;
            }
            ShellMessage::FsEnd => {
                ended_cleanly = true;
                break;
            }
            other => {
                let _ = std::fs::remove_file(&tmp);
                return Err(FsError::new(
                    "S210",
                    format!("unexpected frame in write stream: {other:?}"),
                ));
            }
        }
    }

    if !ended_cleanly {
        let _ = std::fs::remove_file(&tmp);
        let why = if timed_out {
            format!(
                "no chunks for {}s — caller channel stalled or aborted",
                WRITE_IDLE_TIMEOUT.as_secs()
            )
        } else {
            "channel closed before FsEnd".to_string()
        };
        return Err(FsError::new("S218", why));
    }

    // fsync, set mode, rename atomically.
    if let Err(e) = f.flush() {
        let _ = std::fs::remove_file(&tmp);
        return Err(FsError::from_io(&path, e));
    }
    if let Err(e) = f.sync_all() {
        let _ = std::fs::remove_file(&tmp);
        return Err(FsError::from_io(&path, e));
    }
    drop(f);

    if let Err(e) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(bits)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(FsError::from_io(&path, e));
    }
    if let Err(e) = std::fs::rename(&tmp, &target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(FsError::from_io(&path, e));
    }

    Ok(FsResult::Write {
        bytes_written: written,
        path,
    })
}

// ──────────────────────────────────────────────────────────────────────────────
// Download  (guest → host)
// ──────────────────────────────────────────────────────────────────────────────

/// Handle a `ReadStart` download session.
///
/// Opens `path`, emits `FsMeta` then N `FsChunk` frames (64 KiB each),
/// and closes with a terminal `FsEnd` — all via the shared `writer`.
///
/// If any I/O error occurs **after** `FsMeta` has been sent, the
/// function emits an `FsError` frame with `FLAG_TERMINAL` through
/// `writer` (the caller sees it as a terminal error frame on the wire).
///
/// Errors returned from this function indicate pre-`FsMeta` failures
/// (e.g. path not found, is a directory), so the caller can emit the
/// `FsError` frame itself without risk of sending two terminal frames.
///
/// Errors:
/// - `S211` — path does not exist.
/// - `S212` — path is a directory.
/// - `S216` — I/O error opening the file.
pub fn handle_read_start(path: String, writer: &Writer, corr_id: u32) -> Result<(), FsError> {
    use iii_shell_proto::flags::FLAG_TERMINAL;

    let p = Path::new(&path);
    // `metadata()` follows symlinks so the size/mode/mtime we ship in
    // FsMeta describe the same bytes File::open will stream below. Using
    // `symlink_metadata()` here would report the link itself (size ≈
    // path length, mode 0o777) while the stream body comes from the
    // target — caller's "expected vs received" check trips on every
    // symlink download, and a symlink-to-dir would emit FsMeta then
    // fail with EISDIR after the meta was already accepted.
    let md = match std::fs::metadata(p) {
        Ok(m) => m,
        Err(e) => return Err(FsError::from_io(&path, e)),
    };
    if md.is_dir() {
        return Err(FsError::new("S212", format!("is a directory: {path}")));
    }

    let mode_bits = md.mode() & 0o7777;
    let meta = FsReadMeta {
        size: md.len(),
        mode: format!("{mode_bits:04o}"),
        mtime: md.mtime(),
    };
    // Send FsMeta — after this any error must go as a wire FsError frame.
    crate::shell_dispatcher::send_frame(writer, corr_id, 0, &ShellMessage::FsMeta(meta));

    let mut file = match std::fs::File::open(p) {
        Ok(f) => f,
        Err(e) => {
            let fe = FsError::from_io(&path, e);
            crate::shell_dispatcher::send_frame(
                writer,
                corr_id,
                FLAG_TERMINAL,
                &ShellMessage::FsError {
                    code: fe.code.to_string(),
                    message: fe.message.clone(),
                },
            );
            return Ok(()); // FsMeta already sent; caller must NOT send another frame
        }
    };

    let mut buf = vec![0u8; READ_CHUNK_SIZE];
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                let fe = FsError::from_io(&path, e);
                crate::shell_dispatcher::send_frame(
                    writer,
                    corr_id,
                    FLAG_TERMINAL,
                    &ShellMessage::FsError {
                        code: fe.code.to_string(),
                        message: fe.message,
                    },
                );
                return Ok(());
            }
        };
        let encoded = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
        crate::shell_dispatcher::send_frame(
            writer,
            corr_id,
            0,
            &ShellMessage::FsChunk { data_b64: encoded },
        );
    }

    // Terminal FsEnd.
    crate::shell_dispatcher::send_frame(writer, corr_id, FLAG_TERMINAL, &ShellMessage::FsEnd);
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests helper re-export for tests.rs
// ──────────────────────────────────────────────────────────────────────────────

/// Encode bytes as base64 (STANDARD alphabet). Exposed for tests.
#[cfg(test)]
pub fn b64_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}
