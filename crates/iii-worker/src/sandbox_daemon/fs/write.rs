// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! sandbox::fs::write — streaming file upload trigger.
//!
//! The caller creates a channel and passes its `reader_ref` (a
//! `StreamChannelRef`) in the request JSON. This handler constructs a
//! `ChannelReader` from that ref, adapts it to `tokio::io::AsyncRead`
//! via `ChannelReaderAdapter`, then streams bytes into the supervisor
//! through `FsRunner::fs_write_stream`.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use iii_sdk::channels::{ChannelReader, StreamChannelRef};
use iii_sdk::{IIIError, RegisterFunctionMessage};
use iii_shell_proto::FsResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::sandbox_daemon::{
    errors::SandboxError, fs::adapter::FsRunner, registry::SandboxRegistry,
};

// ---------------------------------------------------------------------------
// AsyncRead adapter for ChannelReader
// ---------------------------------------------------------------------------

/// Wraps a `ChannelReader` and implements `tokio::io::AsyncRead` by driving
/// `next_binary()` on each poll. Because `ChannelReader::next_binary` is
/// async and `poll_read` is synchronous, we store a pending future when the
/// current chunk isn't ready yet. Leftover bytes from a chunk larger than
/// the read buffer are kept in `pending_buf`.
///
/// # Limitations
/// This adapter holds a `tokio::runtime::Handle` internally and spawns the
/// async `next_binary()` call as a task, then polls the JoinHandle. This
/// keeps the `AsyncRead` impl `Unpin` without unsafe and avoids storing a
/// `Pin<Box<dyn Future>>` inline (which would require `unsafe Unpin`).
pub struct ChannelReaderAdapter {
    reader: Arc<ChannelReader>,
    pending_buf: Vec<u8>,
    pending_task: Option<tokio::task::JoinHandle<Result<Option<Vec<u8>>, iii_sdk::IIIError>>>,
    eof: bool,
}

impl ChannelReaderAdapter {
    pub fn new(reader: ChannelReader) -> Self {
        Self {
            reader: Arc::new(reader),
            pending_buf: Vec::new(),
            pending_task: None,
            eof: false,
        }
    }
}

impl tokio::io::AsyncRead for ChannelReaderAdapter {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();

        if this.eof && this.pending_buf.is_empty() {
            return Poll::Ready(Ok(()));
        }

        // Drain any leftover bytes from a previous oversized chunk.
        if !this.pending_buf.is_empty() {
            let n = this.pending_buf.len().min(buf.remaining());
            buf.put_slice(&this.pending_buf[..n]);
            this.pending_buf.drain(..n);
            return Poll::Ready(Ok(()));
        }

        // Spawn a task if we don't have one in flight.
        if this.pending_task.is_none() {
            let reader = this.reader.clone();
            this.pending_task = Some(tokio::spawn(async move { reader.next_binary().await }));
        }

        // Poll the in-flight task.
        let task = this.pending_task.as_mut().unwrap();
        match Pin::new(task).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(join_result) => {
                this.pending_task = None;
                let chunk_result = join_result.map_err(|e| {
                    io::Error::new(io::ErrorKind::BrokenPipe, format!("channel task: {e}"))
                })?;
                match chunk_result {
                    Ok(None) => {
                        this.eof = true;
                        Poll::Ready(Ok(()))
                    }
                    Ok(Some(data)) => {
                        let n = data.len().min(buf.remaining());
                        buf.put_slice(&data[..n]);
                        if n < data.len() {
                            this.pending_buf.extend_from_slice(&data[n..]);
                        }
                        Poll::Ready(Ok(()))
                    }
                    Err(e) => Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        format!("channel read error: {e}"),
                    ))),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// File body for `sandbox::fs::write`. Untagged so the JSON shape
/// decides which variant runs:
///
/// - A bare JSON string (`"console.log('hi')"`) → [`WriteContent::Utf8`].
///   This is what LLM agents naturally pass and the recommended form
///   for source files / configs / small text.
/// - An object that matches [`StreamChannelRef`] → [`WriteContent::Stream`].
///   The existing channel-streaming path for large or binary payloads
///   coming from a programmatic caller that can construct a channel.
///
/// Serde tries variants in declaration order; `Utf8` matches first
/// because every JSON string deserialises into `String`, and
/// `StreamChannelRef` requires an object. Binary inline data uses the
/// separate `content_b64` field on [`WriteRequest`] (not a variant
/// here, so a caller can't accidentally pass base64 expecting it to be
/// decoded — they have to opt in by name).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum WriteContent {
    /// Inline UTF-8 string. Written to the file verbatim.
    Utf8(String),
    /// Streaming channel for large / binary uploads.
    Stream(StreamChannelRef),
}

#[derive(Debug, Deserialize)]
pub struct WriteRequest {
    pub sandbox_id: String,
    pub path: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub parents: bool,
    /// File body — pass a UTF-8 string for source/text, or a
    /// `StreamChannelRef` object for streaming. Mutually exclusive
    /// with `content_b64`; exactly one of the two must be set.
    #[serde(default)]
    pub content: Option<WriteContent>,
    /// Base64-encoded inline body for small binary payloads.
    /// Mutually exclusive with `content`.
    #[serde(default)]
    pub content_b64: Option<String>,
}

fn default_mode() -> String {
    "0644".to_string()
}

#[derive(Debug, Serialize)]
pub struct WriteResponse {
    pub bytes_written: u64,
    pub path: String,
}

// ---------------------------------------------------------------------------
// Handler — testable inner function
// ---------------------------------------------------------------------------

/// Inner handler that accepts a pre-constructed `AsyncRead`. Tests call
/// this directly with a `Cursor<Vec<u8>>`, bypassing the channel layer.
pub async fn handle_write_with_reader<R: FsRunner + ?Sized>(
    sandbox_id: String,
    path: String,
    mode: String,
    parents: bool,
    reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>,
    registry: &SandboxRegistry,
    runner: &R,
) -> Result<WriteResponse, SandboxError> {
    let id = Uuid::parse_str(&sandbox_id).map_err(|_| {
        SandboxError::InvalidRequest(format!("sandbox_id is not a valid UUID: {sandbox_id}"))
    })?;
    let state = registry.get(id).await?;
    if state.stopped {
        return Err(SandboxError::AlreadyStopped(id.to_string()));
    }
    registry.bump_last_exec(id).await;

    let result = runner
        .fs_write_stream(state.shell_sock, path.clone(), mode, parents, reader)
        .await?;

    match result {
        FsResult::Write {
            bytes_written,
            path: p,
        } => Ok(WriteResponse {
            bytes_written,
            path: p,
        }),
        other => Err(SandboxError::FsIo(format!(
            "expected Write result, got {other:?}"
        ))),
    }
}

/// Public trigger handler. Dispatches on the body shape and delegates
/// to [`handle_write_with_reader`].
///
/// Accepts three input shapes (exactly one must be present):
///
/// - `content: "<utf-8 string>"` — inline UTF-8 body, wrapped in a
///   `Cursor` and written verbatim. The path LLM agents naturally take
///   when they pass `content` as a string. No channel setup required.
/// - `content_b64: "<base64>"` — inline binary body. Decoded, then
///   wrapped in a `Cursor` like the UTF-8 form. Use for files with
///   bytes the JSON layer would mangle (NUL, invalid UTF-8, etc.).
/// - `content: { reader_ref: …, … }` — a `StreamChannelRef`. The
///   original streaming path, for large uploads from programmatic
///   callers that can construct a channel.
pub async fn handle_write<R: FsRunner + ?Sized>(
    req: WriteRequest,
    registry: &SandboxRegistry,
    runner: &R,
    engine_address: &str,
) -> Result<WriteResponse, SandboxError> {
    use base64::Engine as _;
    use std::io::Cursor;

    let reader: Box<dyn tokio::io::AsyncRead + Unpin + Send> = match (req.content, req.content_b64)
    {
        (Some(WriteContent::Utf8(s)), None) => Box::new(Cursor::new(s.into_bytes())),
        (Some(WriteContent::Stream(ref_)), None) => {
            let ch_reader = ChannelReader::new(engine_address, &ref_);
            Box::new(ChannelReaderAdapter::new(ch_reader))
        }
        (None, Some(b64)) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64.as_bytes())
                .map_err(|e| {
                    SandboxError::InvalidRequest(format!("content_b64 is not valid base64: {e}"))
                })?;
            Box::new(Cursor::new(bytes))
        }
        (Some(_), Some(_)) => {
            return Err(SandboxError::InvalidRequest(
                "set either `content` (string or StreamChannelRef) or `content_b64`, not both"
                    .into(),
            ));
        }
        (None, None) => {
            return Err(SandboxError::InvalidRequest(
                "missing file body: set `content` (string or StreamChannelRef) or `content_b64`"
                    .into(),
            ));
        }
    };

    handle_write_with_reader(
        req.sandbox_id,
        req.path,
        req.mode,
        req.parents,
        reader,
        registry,
        runner,
    )
    .await
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub(super) fn register(
    iii: &iii_sdk::III,
    registry: Arc<SandboxRegistry>,
    runner: Arc<dyn FsRunner>,
) {
    let engine_address = iii.address().to_string();
    let handler = move |payload: Value| {
        let registry = registry.clone();
        let runner = runner.clone();
        let engine_address = engine_address.clone();
        Box::pin(async move {
            let req: WriteRequest = serde_json::from_value(payload)
                .map_err(|e| IIIError::Handler(format!("bad request: {e}")))?;
            match handle_write(req, &registry, &*runner, &engine_address).await {
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
            id: "sandbox::fs::write".to_string(),
            description: Some("Stream-upload a file into a sandbox".to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox_daemon::{fs::adapter::FsRunner, registry::SandboxState};
    use iii_shell_proto::{FsOp, FsReadMeta, FsResult};
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::time::Instant;
    use tokio::sync::Mutex;

    struct FakeRunner {
        captured: Arc<Mutex<Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl FsRunner for FakeRunner {
        async fn fs_call(&self, _shell_sock: PathBuf, _op: FsOp) -> Result<FsResult, SandboxError> {
            unimplemented!()
        }

        async fn fs_write_stream(
            &self,
            _shell_sock: PathBuf,
            path: String,
            _mode: String,
            _parents: bool,
            mut reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>,
        ) -> Result<FsResult, SandboxError> {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            reader.read_to_end(&mut buf).await.unwrap();
            let bytes = buf.len() as u64;
            *self.captured.lock().await = buf;
            Ok(FsResult::Write {
                bytes_written: bytes,
                path,
            })
        }

        async fn fs_read_stream(
            &self,
            _shell_sock: PathBuf,
            _path: String,
        ) -> Result<(FsReadMeta, Box<dyn tokio::io::AsyncRead + Unpin + Send>), SandboxError>
        {
            unimplemented!()
        }
    }

    fn make_state(id: Uuid) -> SandboxState {
        SandboxState {
            id,
            name: None,
            image: "python".into(),
            rootfs: PathBuf::from("/tmp/r"),
            workdir: PathBuf::from("/tmp/w"),
            shell_sock: PathBuf::from("/tmp/s"),
            vm_pid: Some(1),
            created_at: Instant::now(),
            last_exec_at: Instant::now(),
            exec_in_progress: false,
            idle_timeout_secs: 300,
            stopped: false,
        }
    }

    #[tokio::test]
    async fn write_with_reader_captures_bytes() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let data = b"hello, sandbox!";
        let captured = Arc::new(Mutex::new(Vec::new()));
        let runner = FakeRunner {
            captured: captured.clone(),
        };
        let cursor = Box::new(Cursor::new(data.to_vec()));
        let resp = handle_write_with_reader(
            id.to_string(),
            "/workspace/test.txt".into(),
            "0644".into(),
            false,
            cursor,
            &reg,
            &runner,
        )
        .await
        .unwrap();
        assert_eq!(resp.bytes_written, data.len() as u64);
        assert_eq!(*captured.lock().await, data);
    }

    #[tokio::test]
    async fn bad_uuid_returns_s001() {
        let reg = SandboxRegistry::new();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let runner = FakeRunner { captured };
        let err = handle_write_with_reader(
            "not-a-uuid".into(),
            "/".into(),
            "0644".into(),
            false,
            Box::new(Cursor::new(vec![])),
            &reg,
            &runner,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
    }

    #[tokio::test]
    async fn missing_sandbox_returns_s002() {
        let reg = SandboxRegistry::new();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let runner = FakeRunner { captured };
        let err = handle_write_with_reader(
            Uuid::new_v4().to_string(),
            "/".into(),
            "0644".into(),
            false,
            Box::new(Cursor::new(vec![])),
            &reg,
            &runner,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code().as_str(), "S002");
    }

    // ── public handle_write — body-shape dispatch (option A) ────────────

    /// `content: "<utf-8 string>"` is the path agents naturally take.
    /// It must just work — no channel setup, no base64, write the bytes.
    #[tokio::test]
    async fn handle_write_accepts_inline_utf8_content() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let runner = FakeRunner {
            captured: captured.clone(),
        };
        let req = WriteRequest {
            sandbox_id: id.to_string(),
            path: "/workspace/app.js".into(),
            mode: "0644".into(),
            parents: false,
            content: Some(WriteContent::Utf8("console.log('hi')\n".into())),
            content_b64: None,
        };
        let resp = handle_write(req, &reg, &runner, "irrelevant")
            .await
            .unwrap();
        assert_eq!(resp.bytes_written, "console.log('hi')\n".len() as u64);
        assert_eq!(*captured.lock().await, b"console.log('hi')\n");
    }

    /// `content_b64` is the inline path for binary or shell-fragile bytes.
    #[tokio::test]
    async fn handle_write_accepts_inline_content_b64() {
        use base64::Engine as _;
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let runner = FakeRunner {
            captured: captured.clone(),
        };
        let payload = b"\x00\x01\x02binary\xffbytes";
        let b64 = base64::engine::general_purpose::STANDARD.encode(payload);
        let req = WriteRequest {
            sandbox_id: id.to_string(),
            path: "/workspace/blob.bin".into(),
            mode: "0644".into(),
            parents: false,
            content: None,
            content_b64: Some(b64),
        };
        let resp = handle_write(req, &reg, &runner, "irrelevant")
            .await
            .unwrap();
        assert_eq!(resp.bytes_written, payload.len() as u64);
        assert_eq!(&*captured.lock().await, payload);
    }

    #[tokio::test]
    async fn handle_write_rejects_invalid_base64() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let runner = FakeRunner { captured };
        let req = WriteRequest {
            sandbox_id: id.to_string(),
            path: "/x".into(),
            mode: "0644".into(),
            parents: false,
            content: None,
            content_b64: Some("not!valid!base64!".into()),
        };
        let err = handle_write(req, &reg, &runner, "irrelevant")
            .await
            .unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
        assert!(err.to_string().contains("not valid base64"), "got: {err}");
    }

    #[tokio::test]
    async fn handle_write_rejects_both_content_and_content_b64() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let runner = FakeRunner { captured };
        let req = WriteRequest {
            sandbox_id: id.to_string(),
            path: "/x".into(),
            mode: "0644".into(),
            parents: false,
            content: Some(WriteContent::Utf8("a".into())),
            content_b64: Some("YQ==".into()),
        };
        let err = handle_write(req, &reg, &runner, "irrelevant")
            .await
            .unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
        assert!(err.to_string().contains("not both"), "got: {err}");
    }

    #[tokio::test]
    async fn handle_write_rejects_neither_content_nor_content_b64() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let runner = FakeRunner { captured };
        let req = WriteRequest {
            sandbox_id: id.to_string(),
            path: "/x".into(),
            mode: "0644".into(),
            parents: false,
            content: None,
            content_b64: None,
        };
        let err = handle_write(req, &reg, &runner, "irrelevant")
            .await
            .unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
        assert!(err.to_string().contains("missing file body"), "got: {err}");
    }

    /// Serde dispatch: a bare JSON string lands as `Utf8`; an object
    /// matching `StreamChannelRef` lands as `Stream`. We don't test the
    /// `Stream` path end-to-end here (it requires a live engine for
    /// `ChannelReader`); the unit test covers the JSON → enum shape.
    #[tokio::test]
    async fn write_content_deserialises_string_as_utf8_variant() {
        let v: WriteContent = serde_json::from_value(serde_json::json!("hello"))
            .expect("string must deserialise as Utf8");
        match v {
            WriteContent::Utf8(s) => assert_eq!(s, "hello"),
            other => panic!("expected Utf8, got {other:?}"),
        }
    }
}
