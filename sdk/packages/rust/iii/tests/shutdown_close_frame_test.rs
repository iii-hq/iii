//! Regression test for the Rust SDK shutdown bug.
//!
//! Background: pre-fix, `run_connection`'s `Outbound::Shutdown` branch
//! just returned without sending a WebSocket Close frame. The engine
//! had to detect the disconnect via TCP-level EOF, which on a fast
//! restart raced the next worker's registration arrival — leaving the
//! engine logging spurious `Function ... is already registered.
//! Overwriting.` warnings and (worse) deleting the new worker's fresh
//! registrations when the late cleanup_worker fired.
//!
//! Fix: in the inner-loop Shutdown branch, send `WsMessage::Close(None)`
//! and call `ws_tx.close()` before returning. This test wires a minimal
//! in-process WS server, asserts the server observes a Close frame
//! after `iii.shutdown()` is called. Without the fix, the server only
//! sees the TCP drop (no Close frame) and the assertion fails.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use iii_sdk::{InitOptions, OtelConfig, register_worker};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::accept_async;

/// Spawns a tiny WebSocket server on 127.0.0.1:0 that records whether
/// it received a WebSocket Close frame from its single client. Returns
/// the server URL and a flag handle. The server accepts exactly one
/// connection.
async fn spawn_close_observing_server() -> (String, Arc<Mutex<bool>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("local_addr");
    let url = format!("ws://{}", addr);

    let saw_close: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let saw_close_task = saw_close.clone();

    tokio::spawn(async move {
        // Accept exactly one client. Anything after is ignored.
        let (stream, _peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(_) => return,
        };
        let ws = match accept_async(stream).await {
            Ok(ws) => ws,
            Err(_) => return,
        };
        let (_write, mut read) = ws.split();

        // Drain frames until we observe a Close OR the stream errors.
        // The pre-fix codepath dropped TCP without a Close frame, so
        // `read.next()` returns either `Some(Err(...))` or `None`
        // depending on the lib's interpretation of the abrupt drop.
        // Either way, we DON'T see `Ok(Message::Close(_))`.
        while let Some(msg) = read.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                    *saw_close_task.lock().await = true;
                    break;
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    });

    (url, saw_close)
}

/// `iii.shutdown()` should trigger `run_connection` to send a clean
/// WebSocket Close frame to the engine before the connection drops.
/// Pre-fix the Shutdown branch just `return`-ed and the engine had to
/// infer the disconnect from a TCP-level EOF — the slow path that
/// races fast-restart re-registration on the engine side.
#[tokio::test]
async fn shutdown_sends_websocket_close_frame() {
    let (url, saw_close) = spawn_close_observing_server().await;

    // OTel disabled — the telemetry system tries to open a SECOND ws
    // connection that the single-accept server can't service, and any
    // failure noise there isn't relevant to this test. auto_shutdown
    // also disabled because we want to drive shutdown() explicitly
    // from the test rather than via a real signal (which would
    // process::exit and kill the test runner).
    let opts = InitOptions {
        otel: Some(OtelConfig {
            enabled: Some(false),
            ..Default::default()
        }),
        auto_shutdown: false,
        ..Default::default()
    };

    let iii = register_worker(&url, opts);

    // Wait for the connection to be live before triggering shutdown,
    // otherwise the Shutdown branch we want to exercise is the
    // pre-connect one (which has no `ws_tx` to send Close on and is
    // intentionally a bare `return`). 1.5s is comfortably above the
    // OTel init + handshake budget seen in the sibling
    // register_function_no_duplicate test.
    tokio::time::sleep(Duration::from_millis(1_500)).await;

    iii.shutdown();

    // Give the close handshake a moment to round-trip from the SDK to
    // our test server. The Shutdown branch sends the frame
    // synchronously (`.send(...).await`) before returning, so the
    // server-side `read.next()` should observe it within tens of ms;
    // 500ms is generous.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let observed = *saw_close.lock().await;
    assert!(
        observed,
        "expected a WebSocket Close frame after shutdown(), got none — \
         pre-fix the SDK dropped TCP without sending a close frame, \
         which makes engine-side cleanup_worker run after the next \
         worker has already registered"
    );
}
