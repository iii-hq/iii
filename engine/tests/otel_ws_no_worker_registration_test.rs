//! Regression test for the OTEL WS routing fix.
//!
//! Pre-fix: every WebSocket connection hitting the engine — including
//! the secondary OTEL-only socket opened by the SDK for traces, metrics,
//! and logs — was handed to `handle_worker`, which unconditionally
//! called `worker_registry.register_worker`. The OTEL socket never sent
//! a `RegisterWorker` function call, so it sat in the registry with null
//! name/os/runtime/pid: a ghost worker alongside every real one. That
//! doubled `workers::list` output, inflated `workers_active` metrics,
//! and produced two `Worker registered` log lines per worker startup.
//!
//! Fix: the engine now exposes a dedicated `/otel` route. SDKs connect
//! their telemetry socket there. The new route runs the same RBAC
//! handshake as `/` but does NOT touch `worker_registry`, and only
//! accepts binary telemetry frames on the inbound side.
//!
//! This test boots the full `WorkerManager` axum router on a random
//! port, connects one WS client to `/` and one to `/otel`, and asserts:
//!   1. `/` produces exactly 1 registry entry.
//!   2. `/otel` produces ZERO additional registry entries.
//! Without the fix, `/otel` would 404 (pre-route) OR register the
//! telemetry socket (pre-path-routing), both of which this test catches.

use std::sync::Arc;
use std::time::Duration;

use futures_util::SinkExt;
use iii::engine::Engine;
use iii::workers::traits::Worker;
use iii::workers::worker::{WorkerManager, WorkerManagerConfig};
use serde_json::json;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

/// Boots a `WorkerManager` on a random port and returns the port + engine
/// handle so the test can later inspect `worker_registry`. The worker
/// manager runs in a background task for the duration of the test.
async fn spawn_engine() -> (u16, Arc<Engine>) {
    // Pre-bind to discover an available port so the test avoids racing
    // against CI parallelism on 49134.
    let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
    let port = probe.local_addr().expect("local_addr").port();
    drop(probe);

    let engine = Arc::new(Engine::new());
    let config = json!({ "port": port, "host": "127.0.0.1" });
    let worker = WorkerManager::create(engine.clone(), Some(config))
        .await
        .expect("create WorkerManager");

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    worker
        .start_background_tasks(shutdown_rx, shutdown_tx)
        .await
        .expect("start WorkerManager");

    // Give axum a moment to bind.
    tokio::time::sleep(Duration::from_millis(150)).await;

    (port, engine)
}

#[tokio::test]
async fn otel_path_skips_worker_registry_registration() {
    let (port, engine) = spawn_engine().await;

    // Baseline: registry starts empty.
    assert_eq!(
        engine.worker_registry.list_workers().len(),
        0,
        "registry should start empty"
    );

    // Step 1: connect a normal worker socket to `/`. This should register
    // exactly one WorkerConnection, proving the test harness is wired
    // correctly (and that the default route behavior is unchanged).
    let (mut normal_ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/", port))
        .await
        .expect("connect to / should succeed");

    // The engine's `handle_worker` sends a `WorkerRegistered` message
    // synchronously after `register_worker`, so once the client observes
    // any inbound message the registry insert is guaranteed to have
    // happened. Wait up to 500ms to stay well clear of CI jitter.
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if engine.worker_registry.list_workers().len() >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("`/` should register a worker within 500ms");

    let after_normal = engine.worker_registry.list_workers().len();
    assert_eq!(
        after_normal, 1,
        "`/` connection should add exactly one registry entry"
    );

    // Step 2: connect an OTEL-only socket to `/otel`. This must NOT add
    // a registry entry, even after sending a telemetry binary frame.
    let (mut otel_ws, _) =
        tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/otel", port))
            .await
            .expect("connect to /otel should succeed");

    // Send a synthetic OTLP frame. `OTLP` prefix + empty JSON object is
    // the minimum the engine's `handle_telemetry_frame` will accept as a
    // recognized telemetry frame.
    let mut frame = b"OTLP".to_vec();
    frame.extend_from_slice(b"{}");
    otel_ws
        .send(WsMessage::Binary(frame.into()))
        .await
        .expect("send OTLP frame");

    // Give the engine a generous window to process the frame. If a
    // regression reintroduced the bug, `register_worker` would have
    // fired on connect (before any frame was sent), so 200ms is plenty.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let after_otel = engine.worker_registry.list_workers().len();
    assert_eq!(
        after_otel, 1,
        "`/otel` connection must NOT add a worker registry entry \
         (got {after_otel}, expected 1 from the `/` connection above)"
    );

    // Cleanup: drop sockets so the engine background task doesn't leak.
    let _ = normal_ws.close(None).await;
    let _ = otel_ws.close(None).await;
}
