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

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use iii_sdk::channels::StreamChannelRef;
use iii_sdk::{IIIError, RegisterFunctionMessage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::sandbox_daemon::{
    errors::SandboxError, fs::adapter::FsRunner, registry::SandboxRegistry,
};

#[derive(Debug, Deserialize)]
pub struct ReadRequest {
    pub sandbox_id: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct ReadResponse {
    /// The caller reads file content from this channel ref.
    pub content: StreamChannelRef,
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

    let channel = iii
        .create_channel(Some(64))
        .await
        .map_err(|e| SandboxError::FsIo(format!("create_channel: {e}")))?;

    let reader_ref = channel.reader_ref.clone();
    let writer = channel.writer;

    // Pump bytes from the supervisor into the channel on a background task.
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
    let handler = move |payload: Value| {
        let registry = registry.clone();
        let runner = runner.clone();
        let iii = iii_clone.clone();
        Box::pin(async move {
            let req: ReadRequest = serde_json::from_value(payload)
                .map_err(|e| IIIError::Handler(format!("bad request: {e}")))?;
            match handle_read(req, &registry, &*runner, &iii).await {
                Ok(resp) => serde_json::to_value(resp)
                    .map_err(|e| IIIError::Handler(format!("serialize: {e}"))),
                Err(e) => Err(IIIError::Handler(
                    serde_json::to_string(&e.to_payload()).unwrap_or_else(|_| e.to_string()),
                )),
            }
        }) as Pin<Box<dyn Future<Output = Result<Value, IIIError>> + Send>>
    };
    let _ = iii.register_function_with(
        RegisterFunctionMessage {
            id: "sandbox::fs::read".to_string(),
            description: Some("Stream-download a file from a sandbox".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        handler,
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
