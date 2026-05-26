// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! sandbox::fs::read — streaming file download trigger.
//!
//! 1. Calls `runner.fs_read_stream()` to get `(meta, Box<dyn AsyncRead>)`.
//! 2. Calls `iii.create_channel()` to allocate a fresh engine channel.
//! 3. Returns the channel's `reader_ref` (as `StreamChannelRef`) to the
//!    caller in the response JSON, plus the file metadata.
//! 4. Spawns a background task that pumps bytes from the `AsyncRead` into
//!    `channel.writer`. On read error the task sends a JSON error message
//!    on the channel before closing.

use std::sync::Arc;

use iii_sdk::RegisterFunction;
use iii_sdk::channels::StreamChannelRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::sandbox_daemon::{
    errors::{SandboxError, SandboxErrorWire},
    fs::adapter::FsRunner,
    registry::SandboxRegistry,
};

/// Files reported by the supervisor as smaller than this threshold are
/// fully buffered in-process and inspected for valid UTF-8. If decode
/// succeeds, the response carries `ReadContent::Utf8(String)`. If decode
/// fails (binary) or the file is reported as larger than this cap, the
/// response carries `ReadContent::Stream(StreamChannelRef)` instead and
/// the bytes flow through an engine channel.
///
/// The cap also bounds memory pressure: peak buffer per concurrent
/// `sandbox::fs::read` call is at most `INLINE_BUFFER_CAP` bytes. With
/// `sandbox.max_concurrent` (see `iii.config.yaml`) concurrent calls, peak
/// is `max_concurrent * INLINE_BUFFER_CAP`.
const INLINE_BUFFER_CAP: usize = 1024 * 1024; // 1 MiB

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(example = "read_request_example")]
pub struct ReadRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Absolute path to read inside the sandbox guest.
    pub path: String,
}

fn read_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app/index.js"
    })
}

/// File body returned by `sandbox::fs::read`. Untagged: serializes as a
/// JSON string for small UTF-8 files (the agent-natural shape) or as a
/// `StreamChannelRef` object for large or binary files.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ReadContent {
    /// Inline UTF-8 content. File was small enough to fit in
    /// [`INLINE_BUFFER_CAP`] and decoded cleanly as UTF-8.
    Utf8(String),
    /// Streaming channel ref. File was either larger than the inline
    /// cap or contained invalid UTF-8 bytes. The caller reads from this
    /// channel to receive the full file contents.
    Stream(StreamChannelRef),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadResponse {
    /// File body. UTF-8 string for small text files, channel ref otherwise.
    pub content: ReadContent,
    pub size: u64,
    pub mode: String,
    pub mtime: i64,
}

pub async fn handle_read<R: FsRunner + ?Sized>(
    req: ReadRequest,
    registry: &SandboxRegistry,
    runner: &R,
    iii: &iii_sdk::III,
) -> Result<ReadResponse, SandboxError> {
    let id = Uuid::parse_str(&req.sandbox_id).map_err(|_| {
        SandboxError::InvalidRequest(format!(
            "sandbox_id is not a valid UUID: {}",
            req.sandbox_id
        ))
    })?;
    let state = registry.get(id).await?;
    if state.stopped {
        return Err(SandboxError::AlreadyStopped(id.to_string()));
    }
    registry.bump_last_exec(id).await;

    let path = req.path;
    let (meta, mut reader): (
        iii_shell_proto::FsReadMeta,
        Box<dyn tokio::io::AsyncRead + Unpin + Send>,
    ) = runner
        .fs_read_stream(state.shell_sock, path.clone())
        .await?;

    // Fast path: file is small per supervisor metadata. Buffer up to
    // INLINE_BUFFER_CAP bytes and try UTF-8 decode. Three outcomes:
    //   - decode succeeds                  -> ReadContent::Utf8(String)
    //   - decode fails (invalid UTF-8)     -> Stream, prepending the buffered
    //                                         bytes via Chain<Cursor, reader>
    //   - buffer fills before EOF (file
    //     larger than meta said)           -> Stream, same chain trick
    //
    // The chain ensures no bytes are dropped at the buffer boundary.
    if (meta.size as usize) < INLINE_BUFFER_CAP {
        let mut buf: Vec<u8> = Vec::with_capacity((meta.size as usize).min(INLINE_BUFFER_CAP));
        // take(N+1) so we can detect when the file is actually larger than
        // meta claimed (we'd read past meta.size and hit the cap).
        let read_cap = INLINE_BUFFER_CAP as u64;
        let bytes_read = (&mut reader)
            .take(read_cap)
            .read_to_end(&mut buf)
            .await
            .map_err(|e| SandboxError::FsIo(format!("read buffer: {e}")))?;

        if bytes_read < INLINE_BUFFER_CAP {
            // We have the entire file. Try UTF-8.
            match String::from_utf8(buf) {
                Ok(s) => {
                    return Ok(ReadResponse {
                        content: ReadContent::Utf8(s),
                        size: meta.size,
                        mode: meta.mode,
                        mtime: meta.mtime,
                    });
                }
                Err(err) => {
                    // Invalid UTF-8. Stream the buffered bytes back through a
                    // channel. The reader is already drained (we hit EOF), so
                    // we only need to emit `buf`.
                    let buf = err.into_bytes();
                    let cursor = std::io::Cursor::new(buf);
                    let chained: Box<dyn tokio::io::AsyncRead + Unpin + Send> = Box::new(cursor);
                    return stream_via_channel(iii, chained, meta, path).await;
                }
            }
        } else {
            // File is larger than meta claimed (or exactly INLINE_BUFFER_CAP).
            // We have the first INLINE_BUFFER_CAP bytes in `buf` and `reader`
            // still holds the remainder. Chain them so no bytes are lost.
            let cursor = std::io::Cursor::new(buf);
            let chained: Box<dyn tokio::io::AsyncRead + Unpin + Send> =
                Box::new(cursor.chain(reader));
            return stream_via_channel(iii, chained, meta, path).await;
        }
    }

    // Large file: stream directly without buffering.
    stream_via_channel(iii, reader, meta, path).await
}

/// Spawn the channel-pump task and return a `ReadResponse` carrying a
/// `StreamChannelRef`. Shared by every code path that produces streaming
/// output (large file, invalid-UTF-8 small file, oversized file).
async fn stream_via_channel(
    iii: &iii_sdk::III,
    mut reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>,
    meta: iii_shell_proto::FsReadMeta,
    _path: String,
) -> Result<ReadResponse, SandboxError> {
    let channel = iii
        .create_channel(Some(64))
        .await
        .map_err(|e| SandboxError::FsIo(format!("create_channel: {e}")))?;

    let reader_ref = channel.reader_ref.clone();
    let writer = channel.writer;

    // Pump bytes from the source AsyncRead into the channel on a background task.
    tokio::spawn(async move {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => {
                    // Clean EOF — close the channel.
                    let _ = writer.close().await;
                    break;
                }
                Ok(n) => {
                    if let Err(e) = writer.write(&buf[..n]).await {
                        let _ = writer
                            .send_message(
                                &serde_json::json!({
                                    "error": format!("write to channel failed: {e}")
                                })
                                .to_string(),
                            )
                            .await;
                        let _ = writer.close().await;
                        break;
                    }
                }
                Err(e) => {
                    let _ = writer
                        .send_message(
                            &serde_json::json!({
                                "error": format!("read from supervisor failed: {e}")
                            })
                            .to_string(),
                        )
                        .await;
                    let _ = writer.close().await;
                    break;
                }
            }
        }
    });

    Ok(ReadResponse {
        content: ReadContent::Stream(reader_ref),
        size: meta.size,
        mode: meta.mode,
        mtime: meta.mtime,
    })
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub(super) fn register(
    iii: &iii_sdk::III,
    registry: Arc<SandboxRegistry>,
    runner: Arc<dyn FsRunner>,
) {
    let iii_clone = iii.clone();
    let _ = iii.register_function(
        "sandbox::fs::read",
        RegisterFunction::new_async(move |req: ReadRequest| {
            let registry = registry.clone();
            let runner = runner.clone();
            let iii = iii_clone.clone();
            async move {
                let sid = req.sandbox_id.clone();
                let start = std::time::Instant::now();
                let result = handle_read(req, &registry, &*runner, &iii).await;
                crate::sandbox_daemon::log_handler_result(
                    "sandbox::fs::read",
                    Some(&sid),
                    &result,
                    start.elapsed().as_millis() as u64,
                );
                result.map_err(|e| SandboxErrorWire(e).into())
            }
        })
        .description(
            "Read a file from a sandbox. Returns content as a UTF-8 string for small text files \
             (under 1 MiB) or a StreamChannelRef for large/binary files. Example: { sandbox_id: \"...\", path: \"/home/app/index.js\" }",
        ),
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// Unit tests for `handle_read` require a real `iii_sdk::III` that connects
// to a live engine (for `create_channel`). Without an engine, the call
// fails at channel allocation. End-to-end coverage is deferred to Phase 6
// (external_known_sandbox_fs.rs). The S001/S002 guard tests below pass a
// dummy `&iii_sdk::III` value from `register_worker` so they don't need
// a live engine — they assert early-exit before the channel call.
//
// NOTE: S001/S002 tests are omitted here because constructing even a
// disconnected `III` handle requires starting the background runtime thread
// and a valid engine URL. The guard logic (UUID parse and registry lookup)
// is identical to every other fs trigger and is covered by those test suites.
// The background-task lifecycle (pump loop) is covered by Phase 6 e2e tests.
//
// #[ignore] marker is placed below as documentation that the full test is
// intentionally skipped at unit-test time.

#[cfg(test)]
mod tests {
    /// Full `handle_read` unit test skipped: requires a live engine for
    /// `iii.create_channel()`. Covered by Phase 6 e2e tests instead.
    #[tokio::test]
    #[ignore]
    async fn handle_read_e2e_deferred_to_phase6() {}
}
