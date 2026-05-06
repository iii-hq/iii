// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Fake-relay integration tests for the iii-shell-client crate.
//!
//! Each test binds a UnixListener inside a tempdir, chmods the socket
//! to 0o600 so `verify_shell_socket_ownership` accepts it, and spawns
//! a relay task that drives the client through a canned frame sequence.
//! The client side calls `Session::connect` + `Session::run` with a
//! `VecSink` and asserts the outcome.
//!
//! Frame protocol (matches crate docs):
//! ```text
//! ┌──────────┬──────────┬───────┬──────────────────┐
//! │ frame_len│ corr_id  │ flags │  JSON payload    │
//! │  u32     │  u32     │  u8   │  frame_len-5 B   │
//! └──────────┴──────────┴───────┴──────────────────┘
//! ```
//! 4-byte id_offset handshake is written by the relay first, before any
//! framed traffic.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use iii_shell_client::{Flow, OutputSink, RequestSpec, Session, VecSink, VmClientError};
use iii_shell_proto::{
    FRAME_HEADER_SIZE, MAX_FRAME_SIZE, ShellMessage, decode_frame_body, encode_frame,
    flags::FLAG_TERMINAL,
};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// Bind a UnixListener on `<tempdir>/shell.sock` and chmod to 0o600 so
/// `verify_shell_socket_ownership` accepts it.
fn bind_socket() -> (TempDir, PathBuf, UnixListener) {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("shell.sock");
    let listener = UnixListener::bind(&sock).expect("bind");
    let mut perms = std::fs::metadata(&sock).expect("meta").permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(&sock, perms).expect("chmod");
    (dir, sock, listener)
}

fn nonexistent_path() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("does-not-exist.sock");
    (dir, sock)
}

/// Read one length-prefixed frame from a UnixStream and parse it via
/// `decode_frame_body`. Returns Ok(None) on clean EOF at a frame
/// boundary (peer closed between frames).
async fn read_one_frame(
    stream: &mut UnixStream,
) -> std::io::Result<Option<(u32, u8, ShellMessage)>> {
    let mut len_buf = [0u8; 4];
    // Manual loop so we can distinguish clean EOF from partial read.
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

/// Write the 4-byte id_offset handshake (big-endian).
async fn write_handshake(stream: &mut UnixStream, id_offset: u32) -> std::io::Result<()> {
    stream.write_all(&id_offset.to_be_bytes()).await
}

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

/// Spawn a relay task that accepts a single connection, runs `body`,
/// and returns. The returned JoinHandle must be awaited by the test so
/// assertions inside the relay fire.
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

fn sample_req() -> RequestSpec {
    RequestSpec {
        cmd: "/bin/true".into(),
        args: vec![],
        env: vec![],
        cwd: None,
        stdin: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn happy_path() {
    let (_dir, sock, listener) = bind_socket();

    let relay = spawn_relay(listener, |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");
        let (corr, _flags, msg) = read_one_frame(&mut s)
            .await
            .expect("read request")
            .expect("frame present");
        assert_eq!(corr, 1, "corr_id should be id_offset(0)+1");
        assert!(matches!(msg, ShellMessage::Request { .. }));
        write_framed(&mut s, 1, 0, &ShellMessage::Started { pid: 123 })
            .await
            .expect("started");
        write_framed(
            &mut s,
            1,
            0,
            &ShellMessage::Stdout {
                data_b64: B64.encode(b"hello\n"),
            },
        )
        .await
        .expect("stdout");
        write_framed(
            &mut s,
            1,
            0,
            &ShellMessage::Stderr {
                data_b64: B64.encode(b"warn\n"),
            },
        )
        .await
        .expect("stderr");
        write_framed(&mut s, 1, FLAG_TERMINAL, &ShellMessage::Exited { code: 0 })
            .await
            .expect("exited");
    });

    let session = Session::connect(&sock).await.expect("connect");
    let mut sink = VecSink::with_cap(4096);
    let outcome = session
        .run(sample_req(), &mut sink, Some(Duration::from_secs(5)))
        .await
        .expect("run");

    assert_eq!(outcome.status.code, Some(0));
    assert!(!outcome.status.timed_out);
    assert_eq!(sink.stdout, b"hello\n");
    assert_eq!(sink.stderr, b"warn\n");
    assert!(!outcome.stdout_truncated);
    assert!(!outcome.stderr_truncated);

    relay.await.expect("relay join");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timeout_sends_kill() {
    let (_dir, sock, listener) = bind_socket();

    // Use a channel so we can observe which frames the relay received.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ShellMessage>();

    let relay = spawn_relay(listener, move |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");
        let (_corr, _flags, req) = read_one_frame(&mut s)
            .await
            .expect("read request")
            .expect("frame present");
        tx.send(req).expect("send req");
        // Read subsequent frames (we expect Signal{KILL}). Keep reading
        // until the stream closes so the client's post-kill grace loop
        // can run to completion without the relay blocking writes.
        while let Ok(Some((_, _, msg))) = read_one_frame(&mut s).await {
            let _ = tx.send(msg);
        }
        // Never send Started/Exited — simulate a wedged child.
    });

    let session = Session::connect(&sock).await.expect("connect");
    let mut sink = VecSink::with_cap(1024);
    let outcome = session
        .run(sample_req(), &mut sink, Some(Duration::from_millis(100)))
        .await
        .expect("run returns Ok with timed_out=true");

    assert!(outcome.status.timed_out, "expected timed_out=true");
    assert_eq!(outcome.status.code, None, "no Exited frame, code is None");

    let first = rx.recv().await.expect("at least one frame");
    assert!(
        matches!(first, ShellMessage::Request { .. }),
        "first frame is Request"
    );

    // Bound the wait for the Signal frame so we don't hang a bad test.
    let signal = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("relay received a second frame before timeout")
        .expect("channel open");
    match signal {
        ShellMessage::Signal { signal } => assert_eq!(signal, 9, "SIGKILL"),
        other => panic!("expected Signal{{9}}, got {other:?}"),
    }

    relay.await.expect("relay join");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn output_truncation_via_sink_drains_until_exited() {
    let (_dir, sock, listener) = bind_socket();

    let relay = spawn_relay(listener, |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");
        let _req = read_one_frame(&mut s).await.expect("read").expect("some");
        // 10 Stdout frames of 1 KB each = 10 KB total.
        let chunk = vec![b'x'; 1024];
        for _ in 0..10 {
            write_framed(
                &mut s,
                1,
                0,
                &ShellMessage::Stdout {
                    data_b64: B64.encode(&chunk),
                },
            )
            .await
            .expect("stdout chunk");
        }
        // Five more Stdout frames after cap is hit to verify draining.
        for _ in 0..5 {
            write_framed(
                &mut s,
                1,
                0,
                &ShellMessage::Stdout {
                    data_b64: B64.encode(&chunk),
                },
            )
            .await
            .expect("stdout chunk post-cap");
        }
        write_framed(&mut s, 1, FLAG_TERMINAL, &ShellMessage::Exited { code: 0 })
            .await
            .expect("exited");
    });

    let session = Session::connect(&sock).await.expect("connect");
    let mut sink = VecSink::with_cap(4096);
    let outcome = session
        .run(sample_req(), &mut sink, Some(Duration::from_secs(10)))
        .await
        .expect("run");

    assert_eq!(
        outcome.status.code,
        Some(0),
        "Exited received after cap hit"
    );
    assert!(!outcome.status.timed_out);
    assert_eq!(sink.stdout.len(), 4096, "stdout capped at 4 KB");
    assert!(outcome.stdout_truncated, "truncation flag set");
    assert!(!outcome.stderr_truncated);

    relay.await.expect("relay join");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn eof_mid_stream_returns_session_terminated() {
    let (_dir, sock, listener) = bind_socket();

    let relay = spawn_relay(listener, |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");
        let _req = read_one_frame(&mut s).await.expect("read").expect("some");
        write_framed(&mut s, 1, 0, &ShellMessage::Started { pid: 42 })
            .await
            .expect("started");
        write_framed(
            &mut s,
            1,
            0,
            &ShellMessage::Stdout {
                data_b64: B64.encode(b"partial"),
            },
        )
        .await
        .expect("stdout");
        // Abruptly drop the stream. Flushing on shutdown is implicit
        // via Drop but shut down the write side explicitly for clarity.
        let _ = s.shutdown().await;
        drop(s);
    });

    let session = Session::connect(&sock).await.expect("connect");
    let mut sink = VecSink::with_cap(1024);
    let err = session
        .run(sample_req(), &mut sink, Some(Duration::from_secs(5)))
        .await
        .expect_err("EOF before Exited must error");

    assert!(
        matches!(err, VmClientError::SessionTerminated),
        "expected SessionTerminated, got {err:?}"
    );

    relay.await.expect("relay join");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_violation_bad_frame_len() {
    let (_dir, sock, listener) = bind_socket();

    let relay = spawn_relay(listener, |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");
        let _req = read_one_frame(&mut s).await.expect("read").expect("some");
        // Write MAX_FRAME_SIZE + 1 as the length prefix, then nothing —
        // client should reject at length-validation before trying to
        // read the body.
        let bad = ((MAX_FRAME_SIZE as u32) + 1).to_be_bytes();
        s.write_all(&bad).await.expect("write bad len");
        // Give the client time to read and fail before we close.
        tokio::time::sleep(Duration::from_millis(50)).await;
    });

    let session = Session::connect(&sock).await.expect("connect");
    let mut sink = VecSink::with_cap(1024);
    let err = session
        .run(sample_req(), &mut sink, Some(Duration::from_secs(5)))
        .await
        .expect_err("bad frame_len must error");

    assert!(
        matches!(err, VmClientError::ProtocolViolation(_)),
        "expected ProtocolViolation, got {err:?}"
    );

    relay.await.expect("relay join");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_missing_enoent() {
    let (_dir, sock) = nonexistent_path();
    let err = match Session::connect(&sock).await {
        Ok(_) => panic!("connect to missing path must error"),
        Err(e) => e,
    };
    assert!(
        matches!(err, VmClientError::WorkerMissing(_)),
        "expected WorkerMissing, got {err:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handshake_truncated() {
    let (_dir, sock, listener) = bind_socket();

    let relay = spawn_relay(listener, |mut s| async move {
        let _ = s.shutdown().await;
        drop(s);
    });

    let err = match Session::connect(&sock).await {
        Ok(_) => panic!("immediate close should error"),
        Err(e) => e,
    };
    // 0 bytes before handshake maps to AuthRejected (UnexpectedEof path).
    assert!(
        matches!(err, VmClientError::AuthRejected(_)),
        "expected AuthRejected, got {err:?}"
    );

    relay.await.expect("relay join");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handshake_partial_timeout() {
    let (_dir, sock, listener) = bind_socket();

    let relay = spawn_relay(listener, |mut s| async move {
        // Write 2 of 4 handshake bytes, then sleep past the client's
        // 2-second HANDSHAKE_TIMEOUT.
        s.write_all(&[0u8, 0u8]).await.expect("partial write");
        tokio::time::sleep(Duration::from_secs(4)).await;
    });

    let err = match Session::connect(&sock).await {
        Ok(_) => panic!("partial handshake should error"),
        Err(e) => e,
    };
    // The client wraps the read_exact in a tokio::time::timeout, so we
    // expect HandshakeTimeout; accept HandshakeTruncated as a secondary
    // outcome if the stream ever signals EOF instead.
    assert!(
        matches!(
            err,
            VmClientError::HandshakeTimeout(_) | VmClientError::HandshakeTruncated { .. }
        ),
        "expected HandshakeTimeout or HandshakeTruncated, got {err:?}"
    );

    // Don't join the relay — it's sleeping. Detach.
    relay.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dispatcher_error_terminal() {
    let (_dir, sock, listener) = bind_socket();

    let relay = spawn_relay(listener, |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");
        let _req = read_one_frame(&mut s).await.expect("read").expect("some");
        write_framed(
            &mut s,
            1,
            FLAG_TERMINAL,
            &ShellMessage::Error {
                message: "spawn failed: ENOENT".into(),
            },
        )
        .await
        .expect("error frame");
    });

    let session = Session::connect(&sock).await.expect("connect");
    let mut sink = VecSink::with_cap(1024);
    let err = session
        .run(sample_req(), &mut sink, Some(Duration::from_secs(5)))
        .await
        .expect_err("terminal Error frame must surface");

    match err {
        VmClientError::DispatcherError(msg) => {
            assert!(
                msg.contains("spawn failed"),
                "expected message to contain 'spawn failed', got {msg:?}"
            );
        }
        other => panic!("expected DispatcherError, got {other:?}"),
    }

    relay.await.expect("relay join");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdin_pre_packaged_sends_frame_then_eof() {
    let (_dir, sock, listener) = bind_socket();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ShellMessage>();

    let relay = spawn_relay(listener, move |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");
        // Expect exactly Request, Stdin(data), Stdin(eof).
        for _ in 0..3 {
            let (_, _, msg) = read_one_frame(&mut s).await.expect("read").expect("some");
            tx.send(msg).expect("send");
        }
        write_framed(&mut s, 1, FLAG_TERMINAL, &ShellMessage::Exited { code: 0 })
            .await
            .expect("exited");
    });

    let mut req = sample_req();
    req.stdin = Some(b"hello".to_vec());

    let session = Session::connect(&sock).await.expect("connect");
    let mut sink = VecSink::with_cap(1024);
    let outcome = session
        .run(req, &mut sink, Some(Duration::from_secs(5)))
        .await
        .expect("run");

    assert_eq!(outcome.status.code, Some(0));
    assert!(!outcome.status.timed_out);

    let f1 = rx.recv().await.expect("request");
    assert!(
        matches!(f1, ShellMessage::Request { .. }),
        "first is Request"
    );

    // Frame 2: Stdin with base64("hello") = "aGVsbG8=".
    let f2 = rx.recv().await.expect("stdin data");
    match f2 {
        ShellMessage::Stdin { data_b64 } => {
            assert_eq!(data_b64, "aGVsbG8=", "stdin data_b64");
        }
        other => panic!("expected Stdin(data), got {other:?}"),
    }

    let f3 = rx.recv().await.expect("stdin eof");
    match f3 {
        ShellMessage::Stdin { data_b64 } => {
            assert!(data_b64.is_empty(), "eof frame has empty data_b64");
        }
        other => panic!("expected Stdin(eof), got {other:?}"),
    }

    relay.await.expect("relay join");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn readers_drain_before_exited_ordering_contract() {
    let (_dir, sock, listener) = bind_socket();

    let relay = spawn_relay(listener, |mut s| async move {
        write_handshake(&mut s, 0).await.expect("handshake");
        let _req = read_one_frame(&mut s).await.expect("read").expect("some");
        // Encode all three frames into a single write so they hit the
        // client's read buffer in one shot. This catches any
        // implementation that bails out on Exited before draining the
        // preceding Stdout frames.
        let mut batch = Vec::new();
        let a = encode_frame(
            1,
            0,
            &ShellMessage::Stdout {
                data_b64: B64.encode(b"A"),
            },
        )
        .expect("encode A");
        let b = encode_frame(
            1,
            0,
            &ShellMessage::Stdout {
                data_b64: B64.encode(b"B"),
            },
        )
        .expect("encode B");
        let e =
            encode_frame(1, FLAG_TERMINAL, &ShellMessage::Exited { code: 0 }).expect("encode E");
        batch.extend_from_slice(&a);
        batch.extend_from_slice(&b);
        batch.extend_from_slice(&e);
        s.write_all(&batch).await.expect("batch write");
    });

    let session = Session::connect(&sock).await.expect("connect");
    let mut sink = VecSink::with_cap(4096);
    let outcome = session
        .run(sample_req(), &mut sink, Some(Duration::from_secs(5)))
        .await
        .expect("run");

    assert_eq!(outcome.status.code, Some(0));
    assert!(!outcome.status.timed_out);
    assert_eq!(
        sink.stdout, b"AB",
        "both Stdout frames drained before run returned"
    );

    relay.await.expect("relay join");
}

// Helpers below exist only to keep trait/type imports linked when
// individual tests get feature-gated off.

#[allow(dead_code)]
fn _assert_send_sync<T: Send + Sync>() {}

#[allow(dead_code)]
fn _touch_path(_p: &Path) {}

#[allow(dead_code)]
fn _touch_sink<S: OutputSink>(_s: &mut S) {
    // Ensure the OutputSink trait and Flow enum stay linked into the
    // binary even if tests above stop naming them.
    let _ = Flow::Continue;
}
