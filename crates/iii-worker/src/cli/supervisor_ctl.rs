// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Host-side RPC to the in-VM `iii-supervisor`.
//!
//! Connects to the unix socket published by `__vm-boot`'s control proxy
//! at `~/.iii/managed/<name>/control.sock`, sends a newline-delimited
//! JSON [`Request`], reads the single-line [`Response`], disconnects.
//!
//! The proxy inside `__vm-boot` serializes access to the underlying
//! virtio-console port, so callers don't need to coordinate with each
//! other — each `UnixStream::connect` gets exclusive access to the
//! channel for the duration of its request/response round-trip.
//!
//! Every entry point has a strict 500ms timeout. If the supervisor is
//! unreachable (VM down, supervisor crashed, socket stale), the caller
//! falls back to a full `iii-worker start` which is slow but always
//! works.

use std::path::PathBuf;
use std::time::Duration;

use iii_supervisor::protocol::{self, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Default timeout for a single fast-path round-trip. A live supervisor
/// answers in <10ms; 500ms is generous enough to accommodate a real
/// request bouncing through the virtio-console port and back, tight
/// enough that a hung supervisor doesn't visibly delay the fallback.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(500);

/// Resolve the control-socket path for a named worker.
pub fn control_socket_path(worker_name: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".iii/managed")
        .join(worker_name)
        .join("control.sock")
}

/// Send a request, await a single-line response, return it.
///
/// Every RPC entry point funnels through here.
async fn round_trip(worker_name: &str, req: Request) -> anyhow::Result<Response> {
    let sock = control_socket_path(worker_name);
    let fut = async {
        let mut stream = UnixStream::connect(&sock).await?;
        let line = protocol::encode_request(&req) + "\n";
        stream.write_all(line.as_bytes()).await?;
        stream.flush().await?;

        let mut reader = BufReader::new(stream);
        let mut resp_line = String::new();
        let n = reader.read_line(&mut resp_line).await?;
        if n == 0 {
            anyhow::bail!("supervisor closed channel without responding");
        }
        let resp = protocol::decode_response(&resp_line)?;
        Ok::<_, anyhow::Error>(resp)
    };

    tokio::time::timeout(DEFAULT_TIMEOUT, fut)
        .await
        .map_err(|_| anyhow::anyhow!("supervisor timeout after {:?}", DEFAULT_TIMEOUT))?
}

/// Ask the supervisor to cycle the worker child. Returns when the
/// supervisor has killed the old process and spawned the new one.
/// Caller should NOT treat the new process as ready yet — the worker's
/// runtime still needs to register with the engine, which happens
/// asynchronously after this call returns.
pub async fn request_restart(worker_name: &str) -> anyhow::Result<()> {
    match round_trip(worker_name, Request::Restart).await? {
        Response::Ok => Ok(()),
        Response::Error { message } => {
            anyhow::bail!("supervisor restart error: {message}")
        }
        other => anyhow::bail!("unexpected supervisor response: {other:?}"),
    }
}

/// Ask the supervisor to kill its child and exit, powering down the VM.
/// Returns when the supervisor has acknowledged the shutdown request.
pub async fn request_shutdown(worker_name: &str) -> anyhow::Result<()> {
    match round_trip(worker_name, Request::Shutdown).await? {
        Response::Ok => Ok(()),
        Response::Error { message } => {
            anyhow::bail!("supervisor shutdown error: {message}")
        }
        other => anyhow::bail!("unexpected supervisor response: {other:?}"),
    }
}

/// Liveness probe. Returns `Ok(pid)` when the supervisor is reachable
/// and a child is running, `Ok(0)` when reachable with no child,
/// `Err(_)` when the channel is unreachable.
pub async fn ping(worker_name: &str) -> anyhow::Result<u32> {
    match round_trip(worker_name, Request::Ping).await? {
        Response::Alive { pid } => Ok(pid),
        Response::Error { message } => anyhow::bail!("supervisor ping error: {message}"),
        other => anyhow::bail!("unexpected supervisor response: {other:?}"),
    }
}

/// Query full status (pid + restart count).
pub async fn status(worker_name: &str) -> anyhow::Result<(Option<u32>, u32)> {
    match round_trip(worker_name, Request::Status).await? {
        Response::Status { pid, restarts } => Ok((pid, restarts)),
        Response::Error { message } => anyhow::bail!("supervisor status error: {message}"),
        other => anyhow::bail!("unexpected supervisor response: {other:?}"),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Blocking variant
// ──────────────────────────────────────────────────────────────────────────────
//
// The source watcher runs inside a tokio runtime (see main.rs
// __watch-source dispatch). Its file-change callback is a sync
// `FnMut(&str, ChangeKind)`, so it cannot `.await` anything. Spinning
// up a nested tokio runtime with `Runtime::new() + block_on` panics
// ("Cannot start a runtime from within a runtime"). The cleanest fix
// is to sidestep tokio on this path — use blocking std network calls.
// Same socket, same protocol, same wire format.

/// Blocking variant of [`round_trip`]. Safe to call from inside a
/// tokio context because it performs no async work itself.
fn round_trip_blocking(worker_name: &str, req: Request) -> anyhow::Result<Response> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let sock = control_socket_path(worker_name);
    let stream = UnixStream::connect(&sock)?;
    stream.set_read_timeout(Some(DEFAULT_TIMEOUT))?;
    stream.set_write_timeout(Some(DEFAULT_TIMEOUT))?;

    let mut writer = &stream;
    let line = protocol::encode_request(&req) + "\n";
    writer.write_all(line.as_bytes())?;
    writer.flush()?;

    let mut reader = BufReader::new(&stream);
    let mut resp_line = String::new();
    let n = reader.read_line(&mut resp_line)?;
    if n == 0 {
        anyhow::bail!("supervisor closed channel without responding");
    }
    Ok(protocol::decode_response(&resp_line)?)
}

/// Blocking `Request::Restart`. Used by the source watcher's sync
/// callback path.
pub fn request_restart_blocking(worker_name: &str) -> anyhow::Result<()> {
    match round_trip_blocking(worker_name, Request::Restart)? {
        Response::Ok => Ok(()),
        Response::Error { message } => {
            anyhow::bail!("supervisor restart error: {message}")
        }
        other => anyhow::bail!("unexpected supervisor response: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_socket_path_segments_by_worker_name() {
        let p = control_socket_path("my-worker");
        let s = p.to_string_lossy();
        assert!(s.ends_with("/.iii/managed/my-worker/control.sock"));
    }

    #[test]
    fn control_socket_path_does_not_collide_across_workers() {
        let a = control_socket_path("a");
        let b = control_socket_path("b");
        assert_ne!(a, b);
    }

    /// Spawn a minimal stub server at `sock_path` that accepts one
    /// connection, reads one line, and writes a canned response. Used
    /// by the RPC tests below to exercise the real unix-socket path
    /// without a VM.
    async fn stub_server(sock_path: PathBuf, response_line: &'static str) {
        let _ = tokio::fs::remove_file(&sock_path).await;
        if let Some(parent) = sock_path.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        let listener = tokio::net::UnixListener::bind(&sock_path).unwrap();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                let _ = reader.read_line(&mut line).await;
                let _ = stream.write_all(response_line.as_bytes()).await;
                let _ = stream.write_all(b"\n").await;
                let _ = stream.flush().await;
            }
        });
        // Tiny yield so the spawned task reaches the accept() before
        // the test's connect() races ahead.
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    fn sandbox_worker_name() -> String {
        format!("__iii_test_supervisor_ctl_{}__", std::process::id())
    }

    #[tokio::test]
    async fn request_restart_blocking_succeeds_from_within_tokio_context() {
        // The motivating bug: watcher's sync callback runs inside a
        // tokio runtime and cannot spin up a nested one. This test
        // proves that request_restart_blocking is safe to call from a
        // tokio context — no runtime nesting, no panic.
        let worker = format!("{}_blk", sandbox_worker_name());
        stub_server(control_socket_path(&worker), r#"{"result":"ok"}"#).await;

        // Call from inside the async test, mirroring how the watcher
        // calls it from its async-contextual sync callback.
        let w = worker.clone();
        tokio::task::spawn_blocking(move || request_restart_blocking(&w))
            .await
            .expect("join")
            .expect("restart ok");

        let _ =
            tokio::fs::remove_dir_all(dirs::home_dir().unwrap().join(".iii/managed").join(&worker))
                .await;
    }

    #[test]
    fn request_restart_blocking_errors_on_missing_socket() {
        // No stub running. Blocking connect should fail fast with
        // ENOENT or ECONNREFUSED. Caller maps this to "fall back to
        // full VM restart" and carries on.
        let worker = format!("__iii_test_supervisor_ctl_missing_{}__", std::process::id());
        let err = request_restart_blocking(&worker).expect_err("should error");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("no such file")
                || msg.contains("not found")
                || msg.contains("connection refused"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn request_restart_succeeds_on_ok_response() {
        let worker = sandbox_worker_name();
        stub_server(control_socket_path(&worker), r#"{"result":"ok"}"#).await;
        request_restart(&worker).await.expect("restart ok");
        let _ =
            tokio::fs::remove_dir_all(dirs::home_dir().unwrap().join(".iii/managed").join(&worker))
                .await;
    }

    #[tokio::test]
    async fn request_restart_propagates_error_response() {
        let worker = format!("{}_err", sandbox_worker_name());
        stub_server(
            control_socket_path(&worker),
            r#"{"result":"error","message":"boom"}"#,
        )
        .await;
        let err = request_restart(&worker).await.expect_err("should error");
        assert!(err.to_string().contains("boom"));
        let _ =
            tokio::fs::remove_dir_all(dirs::home_dir().unwrap().join(".iii/managed").join(&worker))
                .await;
    }

    #[tokio::test]
    async fn ping_reports_pid_from_alive_response() {
        let worker = format!("{}_ping", sandbox_worker_name());
        stub_server(
            control_socket_path(&worker),
            r#"{"result":"alive","pid":4242}"#,
        )
        .await;
        let pid = ping(&worker).await.expect("ping ok");
        assert_eq!(pid, 4242);
        let _ =
            tokio::fs::remove_dir_all(dirs::home_dir().unwrap().join(".iii/managed").join(&worker))
                .await;
    }

    #[tokio::test]
    async fn round_trip_fails_fast_when_socket_missing() {
        let worker = format!("{}_missing", sandbox_worker_name());
        // Don't start a server — the socket doesn't exist.
        let err = ping(&worker).await.expect_err("should fail");
        assert!(
            err.to_string().contains("No such file")
                || err.to_string().contains("Connection refused")
                || err.to_string().to_lowercase().contains("not found"),
            "unexpected error: {err}"
        );
    }
}
