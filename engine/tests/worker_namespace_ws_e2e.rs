//! End-to-end proof that the registration buffer is wired into the real
//! WebSocket connection path.
//!
//! A worker's namespace does not arrive as a protocol message: it rides on the
//! `engine::workers::register` engine call. Every SDK that predates the
//! send-order change flushes its queued `RegisterFunction` messages *before*
//! making that call, so a worker declaring `namespace: "orders"` would have its
//! functions registered while the engine still believed it was in `default`.
//!
//! `Engine::handle_worker` arms the buffer (`begin_namespace_resolution`) for
//! every accepted connection. The unit tests around the state machine all call
//! that themselves, so deleting the single call site in `handle_worker` would
//! leave every one of them green while silently restoring the bug. This test
//! closes that gap by driving a real WebSocket client through the real
//! `WorkerManager` router in the SDK's message order, and asserting the
//! function lands in the declared namespace.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use iii::engine::Engine;
use iii::protocol::DEFAULT_NAMESPACE;
use iii::workers::engine_fn::EngineFunctionsWorker;
use iii::workers::traits::Worker;
use iii::workers::worker::WorkerManager;
use serde_json::json;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

/// Boots the engine-functions module (which owns `engine::workers::register`)
/// plus the `WorkerManager` axum router on a free port, and hands back the port
/// and the engine so the test can inspect the function registry.
async fn spawn_engine() -> (u16, Arc<Engine>) {
    iii::workers::observability::metrics::ensure_default_meter();

    // Pre-bind to discover a free port so this test does not race CI parallelism.
    let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
    let port = probe.local_addr().expect("local_addr").port();
    drop(probe);

    let engine = Arc::new(Engine::new());

    // `engine::workers::register` is the message that reveals the namespace;
    // without this module the drain would never be triggered.
    let engine_fn = EngineFunctionsWorker::create(engine.clone(), None)
        .await
        .expect("create EngineFunctionsWorker");
    engine_fn
        .initialize()
        .await
        .expect("initialize EngineFunctionsWorker");
    engine_fn.register_functions(engine.clone());

    let manager = WorkerManager::create(
        engine.clone(),
        Some(json!({ "port": port, "host": "127.0.0.1" })),
    )
    .await
    .expect("create WorkerManager");

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    // No sleep needed: `start_background_tasks` awaits `TcpListener::bind` to
    // completion before spawning `axum::serve`, so the socket is already
    // listening when this returns and the OS backlog holds our connect.
    manager
        .start_background_tasks(shutdown_rx, shutdown_tx)
        .await
        .expect("start WorkerManager");

    (port, engine)
}

/// Polls until `f` holds or the deadline passes. Returns whether it held, so
/// callers can assert with a useful message instead of a bare timeout panic.
///
/// Deliberately shorter than `REGISTRATION_NAMESPACE_GRACE` (5s). The drain
/// these tests wait on happens in microseconds; keeping the two equal meant a
/// CI stall past the grace could turn a pass into a confusing failure whose
/// message blamed the namespace rather than the stall.
async fn eventually(mut f: impl FnMut() -> bool) -> bool {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok()
}

/// Reproduces the exact send order of an SDK that predates the reorder:
/// `RegisterFunction` first, `engine::workers::register` (carrying the
/// namespace) second. The function must still land in `orders`.
///
/// Delete `begin_namespace_resolution` from `handle_worker` and this test fails:
/// the function registers immediately into `default`.
#[tokio::test]
async fn register_function_sent_before_workers_register_lands_in_declared_namespace() {
    let (port, engine) = spawn_engine().await;

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/"))
        .await
        .expect("connect to / should succeed");

    // `handle_worker` sends `WorkerRegistered` right after it registers the
    // connection and arms the buffer, so observing it means the engine is ready
    // for our registration messages.
    let registered = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("WorkerRegistered should arrive")
        .expect("stream should yield")
        .expect("frame should decode");
    let registered: serde_json::Value =
        serde_json::from_str(registered.to_text().expect("text frame")).expect("json");
    assert_eq!(registered["type"], "workerregistered");

    // Step 1 — the SDK flushes its queued function registrations.
    ws.send(WsMessage::Text(
        json!({
            "type": "registerfunction",
            "id": "orders::create",
            "request_format": null,
            "response_format": null,
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send RegisterFunction");

    // Step 2 — only now does it announce itself, revealing the namespace. The
    // engine injects `_caller_worker_id` from the connection itself, so this is
    // the genuine end-to-end path.
    ws.send(WsMessage::Text(
        json!({
            "type": "invokefunction",
            "invocation_id": uuid::Uuid::new_v4(),
            "function_id": "engine::workers::register",
            "data": { "runtime": "node", "name": "orders-worker", "namespace": "orders" },
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send engine::workers::register");

    let landed = eventually(|| engine.functions.get("orders", "orders::create").is_some()).await;
    assert!(
        landed,
        "function must land in the namespace the worker declared; \
         in `default`: {}",
        engine
            .functions
            .get(DEFAULT_NAMESPACE, "orders::create")
            .is_some()
    );
    assert!(
        engine
            .functions
            .get(DEFAULT_NAMESPACE, "orders::create")
            .is_none(),
        "function must not leak into the default namespace"
    );

    let _ = ws.close(None).await;
}

/// The wire-compatibility half: a worker that declares no namespace must still
/// end up in `default`. Guards against the buffer stranding registrations from
/// SDKs that never send a namespace.
#[tokio::test]
async fn register_function_without_a_declared_namespace_lands_in_default() {
    let (port, engine) = spawn_engine().await;

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/"))
        .await
        .expect("connect to / should succeed");

    let _ = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("WorkerRegistered should arrive");

    ws.send(WsMessage::Text(
        json!({
            "type": "registerfunction",
            "id": "legacy::ping",
            "request_format": null,
            "response_format": null,
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send RegisterFunction");

    ws.send(WsMessage::Text(
        json!({
            "type": "invokefunction",
            "invocation_id": uuid::Uuid::new_v4(),
            "function_id": "engine::workers::register",
            "data": { "runtime": "node", "name": "legacy-worker" },
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send engine::workers::register");

    let landed = eventually(|| {
        engine
            .functions
            .get(DEFAULT_NAMESPACE, "legacy::ping")
            .is_some()
    })
    .await;
    assert!(
        landed,
        "a worker that declares no namespace must register into `default`"
    );

    let _ = ws.close(None).await;
}
