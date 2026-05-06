// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Fake-relay integration tests for the filesystem helpers in iii-shell-client.
//!
//! Mirrors the shape of `fake_relay.rs`. Each test binds a Unix socket,
//! spawns a relay task that drives a canned frame sequence, and asserts
//! the client-side return value.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use base64::Engine;
use iii_shell_client::{FsStreamReader, Session, VmClientError};
use iii_shell_proto::{
    FRAME_HEADER_SIZE, FsEntry, FsOp, FsReadMeta, FsResult, MAX_FRAME_SIZE, ShellMessage,
    decode_frame_body, encode_frame, flags::FLAG_TERMINAL,
};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

// ---------------------------------------------------------------------------
// Shared relay helpers
// ---------------------------------------------------------------------------

/// Bind a UnixListener on `<tempdir>/shell.sock` chmoded to 0o600.
fn bind_socket() -> (TempDir, PathBuf, UnixListener) {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("shell.sock");
    let listener = UnixListener::bind(&sock).expect("bind");
    let mut perms = std::fs::metadata(&sock).expect("meta").permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(&sock, perms).expect("chmod");
    (dir, sock, listener)
}

/// Read one length-prefixed frame from a `UnixStream`.
async fn read_one_frame(
    stream: &mut UnixStream,
) -> std::io::Result<Option<(u32, u8, ShellMessage)>> {
    let mut len_buf = [0u8; 4];
    let mut read = 0;
    while read < 4 {
        let n = stream.read(&mut len_buf[read..]).await?;
        if n == 0 {
            if read == 0 {
                return Ok(None);
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "partial length prefix",
            ));
        }
        read += n;
    }
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if !(FRAME_HEADER_SIZE..=MAX_FRAME_SIZE).contains(&frame_len) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame_len {frame_len} out of range"),
        ));
    }
    let mut body = vec![0u8; frame_len];
    stream.read_exact(&mut body).await?;
    let parsed = decode_frame_body(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(Some(parsed))
}

/// Write a length-prefixed frame to a `UnixStream`.
async fn write_framed(
    stream: &mut UnixStream,
    corr_id: u32,
    flags: u8,
    msg: &ShellMessage,
) -> std::io::Result<()> {
    let frame = encode_frame(corr_id, flags, msg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    stream.write_all(&frame).await
}

/// Write the 4-byte `id_offset` handshake.
async fn write_handshake(stream: &mut UnixStream, id_offset: u32) -> std::io::Result<()> {
    stream.write_all(&id_offset.to_be_bytes()).await
}

/// Spawn a relay task that accepts one connection.
fn spawn_relay<F, Fut>(listener: UnixListener, body: F) -> JoinHandle<()>
where
    F: FnOnce(UnixStream) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    tokio::spawn(async move {
        let (stream, _addr) = listener.accept().await.expect("accept");
        body(stream).await;
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Relay receives `FsRequest(Ls)`, replies with `FsResponse(Ls{entries})`.
/// `Session::fs_call` must return the matching `FsResult::Ls`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fs_call_ls_happy_path() {
    let (_dir, sock, listener) = bind_socket();

    let relay = spawn_relay(listener, |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");

        let (corr, _flags, msg) = read_one_frame(&mut s)
            .await
            .expect("read frame")
            .expect("frame present");
        assert_eq!(corr, 1, "corr_id should be id_offset(0)+1");
        assert!(
            matches!(msg, ShellMessage::FsRequest(FsOp::Ls { .. })),
            "expected FsRequest(Ls), got {msg:?}"
        );

        let entry = FsEntry {
            name: "foo.txt".into(),
            is_dir: false,
            size: 42,
            mode: "0644".into(),
            mtime: 1_700_000_000,
            is_symlink: false,
        };
        write_framed(
            &mut s,
            1,
            FLAG_TERMINAL,
            &ShellMessage::FsResponse(FsResult::Ls {
                entries: vec![entry.clone()],
            }),
        )
        .await
        .expect("write response");
    });

    let session = Session::connect(&sock).await.expect("connect");
    let result = session
        .fs_call(FsOp::Ls {
            path: "/workspace".into(),
        })
        .await
        .expect("fs_call succeeded");

    match result {
        FsResult::Ls { entries } => {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].name, "foo.txt");
            assert_eq!(entries[0].size, 42);
        }
        other => panic!("expected FsResult::Ls, got {other:?}"),
    }

    relay.await.expect("relay join");
}

/// Relay replies with `FsError { code, message }`.
/// `Session::fs_call` must return `Err(VmClientError::FsError { code, message })`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fs_call_surfaces_fs_error_code_and_message() {
    let (_dir, sock, listener) = bind_socket();

    let relay = spawn_relay(listener, |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");

        // Consume the request frame.
        let _ = read_one_frame(&mut s).await.expect("read request");

        write_framed(
            &mut s,
            1,
            FLAG_TERMINAL,
            &ShellMessage::FsError {
                code: "S211".into(),
                message: "no such file or directory".into(),
            },
        )
        .await
        .expect("write error");
    });

    let session = Session::connect(&sock).await.expect("connect");
    let err = session
        .fs_call(FsOp::Ls {
            path: "/nonexistent".into(),
        })
        .await
        .expect_err("expected Err from fs_call");

    match err {
        VmClientError::FsError { code, message } => {
            assert_eq!(code, "S211");
            assert_eq!(message, "no such file or directory");
        }
        other => panic!("expected VmClientError::FsError, got {other:?}"),
    }

    relay.await.expect("relay join");
}

/// Relay reads `FsRequest(WriteStart)`, then loops reading `FsChunk`/`FsEnd`,
/// counts total bytes received, replies with `FsResponse(Write { bytes_written, path })`.
/// Host calls `fs_write_stream` with a 200-byte payload via `Cursor`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fs_write_stream_sends_chunks_and_end() {
    let (_dir, sock, listener) = bind_socket();

    let (tx, rx) = tokio::sync::oneshot::channel::<u64>();

    let relay = spawn_relay(listener, |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");

        // First frame must be FsRequest(WriteStart).
        let (corr, _flags, msg) = read_one_frame(&mut s)
            .await
            .expect("read write-start")
            .expect("frame present");
        assert_eq!(corr, 1);
        assert!(
            matches!(msg, ShellMessage::FsRequest(FsOp::WriteStart { .. })),
            "expected WriteStart, got {msg:?}"
        );

        // Read FsChunk / FsEnd frames until FsEnd.
        let mut total_bytes: u64 = 0;
        loop {
            let (_c, _f, frame) = read_one_frame(&mut s)
                .await
                .expect("read chunk/end")
                .expect("frame present");
            match frame {
                ShellMessage::FsChunk { data_b64 } => {
                    let bytes = B64.decode(data_b64.as_bytes()).expect("b64");
                    total_bytes += bytes.len() as u64;
                }
                ShellMessage::FsEnd => break,
                other => panic!("unexpected frame: {other:?}"),
            }
        }

        tx.send(total_bytes).expect("send total");

        write_framed(
            &mut s,
            1,
            FLAG_TERMINAL,
            &ShellMessage::FsResponse(FsResult::Write {
                bytes_written: total_bytes,
                path: "/workspace/out.bin".into(),
            }),
        )
        .await
        .expect("write response");
    });

    let payload = vec![0xABu8; 200];
    let reader = std::io::Cursor::new(payload.clone());

    let session = Session::connect(&sock).await.expect("connect");
    let result = session
        .fs_write_stream("/workspace/out.bin".into(), "0644".into(), false, reader)
        .await
        .expect("fs_write_stream succeeded");

    match result {
        FsResult::Write {
            bytes_written,
            path,
        } => {
            assert_eq!(
                bytes_written, 200,
                "bytes_written must match payload length"
            );
            assert_eq!(path, "/workspace/out.bin");
        }
        other => panic!("expected FsResult::Write, got {other:?}"),
    }

    let relay_total = rx.await.expect("relay total");
    assert_eq!(
        relay_total, 200,
        "relay counted {relay_total} bytes, expected 200"
    );

    relay.await.expect("relay join");
}

/// Relay sends `FsMeta` first, then chunks the 200-byte payload across
/// multiple `FsChunk` frames, then `FsEnd` with FLAG_TERMINAL.
/// Host calls `fs_read_stream`, reads the returned reader to end,
/// asserts bytes match.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fs_read_stream_yields_meta_then_bytes() {
    let (_dir, sock, listener) = bind_socket();

    let payload = vec![0xCDu8; 200];
    let payload_clone = payload.clone();

    let relay = spawn_relay(listener, move |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");

        // Consume the ReadStart request.
        let (corr, _flags, msg) = read_one_frame(&mut s)
            .await
            .expect("read read-start")
            .expect("frame present");
        assert_eq!(corr, 1);
        assert!(
            matches!(msg, ShellMessage::FsRequest(FsOp::ReadStart { .. })),
            "expected ReadStart, got {msg:?}"
        );

        // Send metadata first.
        write_framed(
            &mut s,
            1,
            0,
            &ShellMessage::FsMeta(FsReadMeta {
                size: payload_clone.len() as u64,
                mode: "0644".into(),
                mtime: 1_700_000_000,
            }),
        )
        .await
        .expect("write meta");

        // Send payload in two chunks of 100 bytes each.
        write_framed(
            &mut s,
            1,
            0,
            &ShellMessage::FsChunk {
                data_b64: B64.encode(&payload_clone[..100]),
            },
        )
        .await
        .expect("write chunk 1");

        write_framed(
            &mut s,
            1,
            0,
            &ShellMessage::FsChunk {
                data_b64: B64.encode(&payload_clone[100..]),
            },
        )
        .await
        .expect("write chunk 2");

        write_framed(&mut s, 1, FLAG_TERMINAL, &ShellMessage::FsEnd)
            .await
            .expect("write end");
    });

    let session = Session::connect(&sock).await.expect("connect");
    let (meta, mut reader) = session
        .fs_read_stream("/workspace/data.bin".into())
        .await
        .expect("fs_read_stream succeeded");

    assert_eq!(meta.size, 200);
    assert_eq!(meta.mode, "0644");

    let mut received = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut received)
        .await
        .expect("read to end");

    assert_eq!(received, payload, "received bytes must match sent payload");

    relay.await.expect("relay join");
}

/// Regression test for the FsStreamReader truncation bug.
///
/// Symptom: when a single `FsChunk` frame's body doesn't arrive in one
/// kernel read syscall, `FsStreamReader::poll_read` used to allocate a
/// fresh `Box::pin(read_frame_async(...))` per poll. The inner future
/// stores partial state across syscalls (4-byte length prefix + body
/// progress); dropping it on `Poll::Pending` discards bytes already
/// consumed from the kernel into the future's local buffer. The next
/// poll then re-parses mid-body garbage as a fresh frame length, hits
/// a protocol violation or EOF, and the caller sees 0 bytes.
///
/// Reproduction: write a single FsChunk with a body large enough that
/// the kernel tx buffer can't deliver it atomically, splitting the
/// `write_all` calls with `tokio::time::sleep` so the receiver's
/// `read_frame_async` is guaranteed to suspend mid-frame at least once.
/// Under the buggy impl the host receives 0 bytes; under the fixed
/// impl (one task owns the stream and drives `read_frame_async` to
/// completion in a stable loop) the host receives the full payload.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fs_read_stream_handles_partial_frame_reads() {
    use std::time::Duration;

    let (_dir, sock, listener) = bind_socket();

    // 256 KiB payload of pseudo-random bytes — well over the 64 KiB
    // tokio default kernel socket buffer on most platforms, so the
    // receiver MUST issue multiple read syscalls to drain one frame.
    let payload: Vec<u8> = (0..256u32 * 1024).map(|i| (i & 0xff) as u8).collect();
    let payload_clone = payload.clone();

    let relay = spawn_relay(listener, move |mut s| async move {
        use tokio::io::AsyncWriteExt;

        write_handshake(&mut s, 0).await.expect("handshake");

        // Consume the ReadStart request.
        let (corr, _flags, msg) = read_one_frame(&mut s)
            .await
            .expect("read read-start")
            .expect("frame present");
        assert_eq!(corr, 1);
        assert!(matches!(
            msg,
            ShellMessage::FsRequest(FsOp::ReadStart { .. })
        ));

        // Send FsMeta cleanly (small frame, single syscall — fine).
        write_framed(
            &mut s,
            1,
            0,
            &ShellMessage::FsMeta(FsReadMeta {
                size: payload_clone.len() as u64,
                mode: "0644".into(),
                mtime: 1_700_000_000,
            }),
        )
        .await
        .expect("write meta");

        // Build the single FsChunk frame containing the entire 256 KiB
        // payload (base64-encoded, ~342 KiB total wire size).
        let chunk_msg = ShellMessage::FsChunk {
            data_b64: B64.encode(&payload_clone),
        };
        let frame_bytes = encode_frame(1, 0, &chunk_msg).expect("encode chunk");

        // Hand-deliver the frame in 4 KiB pieces with a tiny sleep
        // between writes. This guarantees the receiving end's first
        // `read` returns less than the full frame, forcing
        // `read_frame_async` to suspend partway through.
        const PIECE: usize = 4 * 1024;
        let mut offset = 0;
        while offset < frame_bytes.len() {
            let end = (offset + PIECE).min(frame_bytes.len());
            s.write_all(&frame_bytes[offset..end])
                .await
                .expect("piece write");
            s.flush().await.expect("flush");
            // Yield + tiny sleep so the receiver actually polls before
            // we ship the next piece. Without this the kernel may
            // coalesce small writes and hide the bug.
            tokio::time::sleep(Duration::from_millis(2)).await;
            offset = end;
        }

        write_framed(&mut s, 1, FLAG_TERMINAL, &ShellMessage::FsEnd)
            .await
            .expect("write end");
    });

    let session = Session::connect(&sock).await.expect("connect");
    let (meta, mut reader) = session
        .fs_read_stream("/workspace/big.bin".into())
        .await
        .expect("fs_read_stream succeeded");

    assert_eq!(meta.size, payload.len() as u64);

    let mut received = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut received)
        .await
        .expect("read to end");

    assert_eq!(
        received.len(),
        payload.len(),
        "expected {} bytes, got {} — partial-frame regression",
        payload.len(),
        received.len()
    );
    assert_eq!(received, payload, "byte-by-byte mismatch");

    relay.await.expect("relay join");
}

/// REGRESSION: `Session::fs_write_stream` must not hang forever
/// when the caller-supplied `AsyncRead` stalls indefinitely
/// (mirrors a hard-terminated channel WS where `next_binary()`
/// never resolves). Without the per-read idle timeout, the trigger
/// handler sits in `reader.read().await` forever, the SDK's outer
/// timeout fires as an opaque "Invocation timeout", and the
/// supervisor's temp file leaks for up to 30s before its
/// safety-valve recv_timeout fires.
///
/// Fix (in iii-shell-client/src/lib.rs): each `reader.read(&mut buf)`
/// is bounded by `FS_WRITE_READ_IDLE_TIMEOUT` (5s). On idle timeout
/// the call returns `Err(VmClientError::FsError { code: "S218" })`.
///
/// This test injects a `StalledReader` whose `poll_read` returns
/// `Pending` forever and asserts the call returns S218 in well under
/// the SDK's outer 30s default.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fs_write_stream_aborts_on_stalled_reader() {
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, ReadBuf};

    /// AsyncRead that never makes progress — models a
    /// hard-terminated data-channel reader where the SDK's
    /// `next_binary()` neither errors nor returns Some/None.
    struct StalledReader;
    impl AsyncRead for StalledReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Pending
        }
    }

    let (_dir, sock, listener) = bind_socket();

    let relay = spawn_relay(listener, |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");
        // Expect WriteStart only — host should give up on its idle
        // timeout and drop the Session before sending FsChunk/FsEnd.
        let (_, _, msg) = read_one_frame(&mut s).await.expect("read").expect("frame");
        assert!(matches!(
            msg,
            ShellMessage::FsRequest(FsOp::WriteStart { .. })
        ));
        // Hold the relay open; the host will drop us first.
        let _ = tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    });

    let session = Session::connect(&sock).await.expect("connect");
    let started = std::time::Instant::now();
    let err = session
        .fs_write_stream(
            "/workspace/should-fail.bin".into(),
            "0644".into(),
            false,
            StalledReader,
        )
        .await
        .expect_err("expected Err from stalled reader");
    let elapsed = started.elapsed();

    match err {
        VmClientError::FsError {
            ref code,
            ref message,
        } => {
            assert_eq!(code, "S218", "expected S218, got {err:?}");
            assert!(
                message.contains("stalled") || message.contains("aborted"),
                "expected idle-timeout-shaped message, got {message:?}",
            );
        }
        other => panic!("expected FsError(S218), got {other:?}"),
    }
    // 5s timeout + scheduling slack. Anything well under 10s
    // proves the host gives up before any reasonable outer
    // SDK timeout fires.
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "fs_write_stream blocked for {elapsed:?} — should give up near 5s",
    );

    // Don't await the relay — it's intentionally still sleeping.
    relay.abort();
}
