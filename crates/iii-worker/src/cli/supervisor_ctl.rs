// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
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

use std::path::{Component, Path, PathBuf};
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
///
/// Validates `worker_name` is a single-segment non-empty basename —
/// not absolute, no parent components, no interior slashes — because
/// `Path::join` with an absolute path silently replaces the base, and
/// `..` components would escape the managed dir. Both would let a
/// malformed config or CLI arg redirect the socket connect to an
/// arbitrary path. The trust model today treats config.yaml / CLI
/// args as trusted, but the cost of defense-in-depth here is one
/// component walk per RPC and the failure mode (clear error) is
/// strictly better than the silent misdirection it prevents.
///
/// Also fails if `HOME` is unset — the previous `unwrap_or_default`
/// produced a relative path starting with `.iii/managed/...`, which
/// would resolve against `$CWD` and connect to the wrong socket
/// entirely when invoked from different directories.
pub fn control_socket_path(worker_name: &str) -> anyhow::Result<PathBuf> {
    if worker_name.is_empty() {
        anyhow::bail!("worker_name is empty");
    }
    let p = Path::new(worker_name);
    if p.is_absolute() {
        anyhow::bail!("worker_name must not be absolute: {worker_name:?}");
    }
    // Reject any `..` or `/` in the name. Accept only a single Normal
    // component — `a/b` or `../x` both fail here even though only one
    // of them is technically traversal, because a legitimate worker
    // name is always a single directory basename.
    let mut comps = p.components();
    match (comps.next(), comps.next()) {
        (Some(Component::Normal(_)), None) => {}
        _ => anyhow::bail!("worker_name must be a single path segment: {worker_name:?}"),
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    Ok(home
        .join(".iii/managed")
        .join(worker_name)
        .join("control.sock"))
}

/// Send a request, await a single-line response, return it.
///
/// Every RPC entry point funnels through here.
async fn round_trip(worker_name: &str, req: Request) -> anyhow::Result<Response> {
    let sock = control_socket_path(worker_name)?;
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
///
/// Wraps the connect+send+recv cycle in a total deadline equal to
/// `DEFAULT_TIMEOUT`. `UnixStream::connect` has no native timeout in
/// std, and a wedged listener could theoretically stall it — running
/// the whole round-trip on a worker thread with `recv_timeout` bounds
/// the entire sync RPC, not just the per-syscall read/write pieces.
fn round_trip_blocking(worker_name: &str, req: Request) -> anyhow::Result<Response> {
    use std::sync::mpsc;
    use std::thread;

    let (tx, rx) = mpsc::channel::<anyhow::Result<Response>>();
    let worker_name = worker_name.to_string();
    thread::spawn(move || {
        let result = round_trip_blocking_inner(&worker_name, req);
        let _ = tx.send(result);
    });

    match rx.recv_timeout(DEFAULT_TIMEOUT) {
        Ok(r) => r,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            anyhow::bail!(
                "supervisor blocking round-trip timeout after {:?}",
                DEFAULT_TIMEOUT
            )
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            anyhow::bail!("supervisor blocking round-trip thread died without result")
        }
    }
}

fn round_trip_blocking_inner(worker_name: &str, req: Request) -> anyhow::Result<Response> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let sock = control_socket_path(worker_name)?;
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
        let p = control_socket_path("my-worker").unwrap();
        let s = p.to_string_lossy();
        assert!(s.ends_with("/.iii/managed/my-worker/control.sock"));
    }

    #[test]
    fn control_socket_path_does_not_collide_across_workers() {
        let a = control_socket_path("a").unwrap();
        let b = control_socket_path("b").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn control_socket_path_rejects_empty_name() {
        assert!(control_socket_path("").is_err());
    }

    #[test]
    fn control_socket_path_rejects_absolute_path() {
        // Path::join with an absolute path silently replaces the base,
        // which would route the socket connect to /etc/... — exactly
        // the redirection we're defending against.
        let err = control_socket_path("/etc/passwd").unwrap_err();
        assert!(
            err.to_string().contains("absolute"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn control_socket_path_rejects_parent_traversal() {
        for bad in ["..", "../escape", "foo/../bar", "a/b"] {
            let err = control_socket_path(bad).unwrap_err();
            assert!(
                err.to_string().contains("single path segment")
                    || err.to_string().contains("absolute"),
                "expected rejection for {bad:?}, got: {err}"
            );
        }
    }

    #[test]
    fn control_socket_path_accepts_single_segment_names() {
        for ok in ["worker", "w1", "a-b", "a_b", "worker.v2"] {
            assert!(
                control_socket_path(ok).is_ok(),
                "expected {ok:?} to be accepted"
            );
        }
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

    /// Guard that locks the process-global HOME so other tests in the
    /// crate (notably `status::tests` which temporarily points HOME at
    /// a long `/var/folders/...` tempdir) can't make our resolved
    /// socket path breach macOS's 104-byte SUN_LEN limit while this
    /// test is mid-run. Thin wrapper over
    /// [`crate::cli::test_support::lock_home`] — the shared helper
    /// keeps the invariant documented in one place and gives any new
    /// test module an obvious entry point.
    fn lock_home() -> std::sync::MutexGuard<'static, ()> {
        super::super::test_support::lock_home()
    }

    #[tokio::test]
    async fn request_restart_blocking_succeeds_from_within_tokio_context() {
        let _h = lock_home();
        // The motivating bug: watcher's sync callback runs inside a
        // tokio runtime and cannot spin up a nested one. This test
        // proves that request_restart_blocking is safe to call from a
        // tokio context — no runtime nesting, no panic.
        let worker = format!("{}_blk", sandbox_worker_name());
        stub_server(control_socket_path(&worker).unwrap(), r#"{"result":"ok"}"#).await;

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
        let _h = lock_home();
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
        let _h = lock_home();
        let worker = sandbox_worker_name();
        stub_server(control_socket_path(&worker).unwrap(), r#"{"result":"ok"}"#).await;
        request_restart(&worker).await.expect("restart ok");
        let _ =
            tokio::fs::remove_dir_all(dirs::home_dir().unwrap().join(".iii/managed").join(&worker))
                .await;
    }

    #[tokio::test]
    async fn request_restart_propagates_error_response() {
        let _h = lock_home();
        let worker = format!("{}_err", sandbox_worker_name());
        stub_server(
            control_socket_path(&worker).unwrap(),
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
        let _h = lock_home();
        let worker = format!("{}_ping", sandbox_worker_name());
        stub_server(
            control_socket_path(&worker).unwrap(),
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
        let _h = lock_home();
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
