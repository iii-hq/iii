//! Regression tests for MOT-3967: the engine used to accept a TCP socket
//! on the worker WS port and wait indefinitely for the HTTP upgrade
//! request. A client that stalled pre-upgrade (observed: a TSI data stall)
//! held its fd forever — an invisible leak (fd 16 held for 8+ minutes in
//! the MOT-3857/MOT-3931 evidence chain).
//!
//! Fix: the worker listener serves each accepted connection through
//! hyper's http1 builder with a `header_read_timeout` handshake deadline
//! (`WorkerManagerConfig::handshake_timeout_ms`). Sockets that don't
//! complete the upgrade in time are closed and logged; established WS
//! sessions are unaffected (the deadline disarms once headers are read).

use std::sync::Arc;
use std::time::Duration;

use iii::engine::Engine;
use iii::workers::traits::Worker;
use iii::workers::worker::WorkerManager;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Handshake deadline used by these tests. Short so the suite stays fast,
/// long enough that a healthy local WS upgrade (sub-millisecond) never
/// trips it.
const HANDSHAKE_TIMEOUT_MS: u64 = 300;

/// Boots a `WorkerManager` on a random port with a sub-second handshake
/// deadline and returns the port + engine handle. Mirrors the pattern in
/// `otel_ws_no_worker_registration_test.rs`.
async fn spawn_engine() -> (u16, Arc<Engine>) {
    // Pre-bind to discover an available port so the test avoids racing
    // against CI parallelism on 49134.
    let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
    let port = probe.local_addr().expect("local_addr").port();
    drop(probe);

    let engine = Arc::new(Engine::new());
    let config = json!({
        "port": port,
        "host": "127.0.0.1",
        "handshake_timeout_ms": HANDSHAKE_TIMEOUT_MS,
    });
    let worker = WorkerManager::create(engine.clone(), Some(config))
        .await
        .expect("create WorkerManager");

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    worker
        .start_background_tasks(shutdown_rx, shutdown_tx)
        .await
        .expect("start WorkerManager");

    // Give the listener task a moment to start serving.
    tokio::time::sleep(Duration::from_millis(150)).await;

    (port, engine)
}

/// Asserts the server closes `stream` (read returns EOF) within 2s —
/// i.e. well within a few multiples of the 300ms handshake deadline.
/// Pre-fix, the read never completes and the outer timeout elapses.
async fn assert_server_closes(mut stream: TcpStream, what: &str) {
    let mut buf = [0u8; 512];
    let deadline = Duration::from_secs(2);
    tokio::time::timeout(deadline, async {
        loop {
            match stream.read(&mut buf).await {
                // EOF: server closed the connection. Reset also counts as
                // closed — either way the fd is reclaimed server-side.
                Ok(0) | Err(_) => break,
                // Ignore any bytes the server writes (e.g. an HTTP 408).
                Ok(_) => continue,
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!("server did not close {what} within {deadline:?} (fd leak: MOT-3967 regression)")
    });
}

#[tokio::test]
async fn stalled_socket_is_closed_at_handshake_deadline() {
    let (port, _engine) = spawn_engine().await;
    let addr = format!("127.0.0.1:{port}");

    // Case 1: TCP connect, then total silence — the observed MOT-3967
    // failure mode (client never sends the upgrade request).
    let silent = TcpStream::connect(&addr).await.expect("connect silent");
    assert_server_closes(silent, "a zero-byte stalled socket").await;

    // Case 2: partial request headers, then stall.
    let mut partial = TcpStream::connect(&addr).await.expect("connect partial");
    partial
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n")
        .await
        .expect("write partial headers");
    assert_server_closes(partial, "a partial-header stalled socket").await;

    // The listener must stay healthy after reaping stalled sockets: a
    // well-behaved WS client still connects.
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/"))
        .await
        .expect("WS connect should still succeed after stalled sockets were reaped");
    drop(ws);
}

#[tokio::test]
async fn established_ws_session_survives_handshake_timeout() {
    let (port, engine) = spawn_engine().await;

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/"))
        .await
        .expect("connect to /");

    // Wait for the registry entry that proves handle_worker registered
    // the session (same idiom as otel_ws_no_worker_registration_test).
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if engine.worker_registry.list_workers().len() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("`/` should register a worker within 500ms");

    // Sit well past the handshake deadline. The deadline only bounds the
    // pre-upgrade phase; an established session must not be reaped.
    tokio::time::sleep(Duration::from_millis(HANDSHAKE_TIMEOUT_MS * 3)).await;

    assert_eq!(
        engine.worker_registry.list_workers().len(),
        1,
        "established WS session must survive past the handshake deadline"
    );

    let _ = ws.close(None).await;
}
