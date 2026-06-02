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
/// succeeds, the response carries `body: Some(String)` alongside the
/// always-present `content: StreamChannelRef`. If decode fails (binary)
/// or the file is reported as larger than this cap, `body` stays `None`
/// and consumers read the bytes through `content` instead.
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

// `ReadContent` (an untagged Utf8/Stream enum) was previously introduced
// here as the response shape, but it broke peers (notably the `workers/shell`
// crate) that statically typed `content` as `StreamChannelRef`. We now keep
// `ReadResponse.content` as `StreamChannelRef` for wire compatibility and
// expose the inline-UTF-8 fast path through an additive `body: Option<String>`
// field instead. See `ReadResponse` below.

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadResponse {
    /// Channel ref for the file body. Always set; callers can subscribe
    /// to receive the full file contents as bytes. Preserved for wire
    /// compatibility with peers that statically type this field as
    /// `StreamChannelRef`.
    pub content: StreamChannelRef,
    /// Inline UTF-8 body. Populated for text files under
    /// [`INLINE_BUFFER_CAP`] (1 MiB) that decode cleanly as UTF-8.
    /// `None` for large or binary files; subscribe to `content` instead.
    /// When `Some`, the same bytes are also delivered through `content`
    /// so legacy callers keep working — new callers can use `body`
    /// directly and skip the channel subscription.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub body: Option<String>,
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
    //   - decode succeeds                  -> inline `body: Some(String)`,
    //                                         channel still serves the same
    //                                         bytes for legacy subscribers.
    //   - decode fails (invalid UTF-8)     -> stream the buffered bytes via
    //                                         Cursor; `body: None`.
    //   - buffer fills before EOF (file
    //     larger than meta said)           -> stream Cursor chained with the
    //                                         remaining reader so no bytes are
    //                                         dropped; `body: None`.
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
                    // Inline-UTF-8 fast path. Emit the same bytes through the
                    // channel so peers that always subscribe to `content`
                    // (e.g. workers/shell `ReadResponse` mirror) keep working
                    // unchanged. New callers see `body: Some(_)` and skip the
                    // subscription. Cost: up to INLINE_BUFFER_CAP bytes
                    // buffered in-channel until the writer closes or the
                    // subscriber drains them.
                    let bytes_for_channel = s.as_bytes().to_vec();
                    let cursor = std::io::Cursor::new(bytes_for_channel);
                    let chained: Box<dyn tokio::io::AsyncRead + Unpin + Send> = Box::new(cursor);
                    return stream_via_channel(iii, chained, meta, Some(s), path).await;
                }
                Err(err) => {
                    // Invalid UTF-8. Stream the buffered bytes back through a
                    // channel. The reader is already drained (we hit EOF), so
                    // we only need to emit `buf`.
                    let buf = err.into_bytes();
                    let cursor = std::io::Cursor::new(buf);
                    let chained: Box<dyn tokio::io::AsyncRead + Unpin + Send> = Box::new(cursor);
                    return stream_via_channel(iii, chained, meta, None, path).await;
                }
            }
        } else {
            // File is larger than meta claimed (or exactly INLINE_BUFFER_CAP).
            // We have the first INLINE_BUFFER_CAP bytes in `buf` and `reader`
            // still holds the remainder. Chain them so no bytes are lost.
            let cursor = std::io::Cursor::new(buf);
            let chained: Box<dyn tokio::io::AsyncRead + Unpin + Send> =
                Box::new(cursor.chain(reader));
            return stream_via_channel(iii, chained, meta, None, path).await;
        }
    }

    // Large file: stream directly without buffering.
    stream_via_channel(iii, reader, meta, None, path).await
}

/// Spawn the channel-pump task and return a `ReadResponse` carrying a
/// `StreamChannelRef`. Shared by every code path that produces a response:
/// large file, invalid-UTF-8 small file, oversized file, and the
/// inline-UTF-8 fast path (which passes `body: Some(s)` and a Cursor over
/// the buffered bytes so the channel still serves the same payload to
/// legacy subscribers).
async fn stream_via_channel(
    iii: &iii_sdk::III,
    mut reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>,
    meta: iii_shell_proto::FsReadMeta,
    body: Option<String>,
    _path: String,
) -> Result<ReadResponse, SandboxError> {
    let channel = iii_sdk::helpers::create_channel(iii, Some(64))
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
        content: reader_ref,
        body,
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
            "Read a file from a sandbox. Always returns `content`: a StreamChannelRef \
             callers can subscribe to for the full file bytes. For UTF-8 text files \
             under 1 MiB, the response also carries an inline `body` string so callers \
             can short-circuit the subscription. Example: { sandbox_id: \"...\", path: \"/home/app/index.js\" }",
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
