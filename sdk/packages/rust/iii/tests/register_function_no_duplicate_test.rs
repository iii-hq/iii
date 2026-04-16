//! Regression test for the Rust SDK double-register bug.
//!
//! Background: `register_function_inner` both inserts into `self.inner.functions`
//! AND pushes a `RegisterFunction` message onto the outbound mpsc. On the first
//! connect, `run_connection` called `collect_registrations()` (which rebuilds
//! messages from `self.inner.functions`) then entered the inner loop where
//! `rx.recv()` pulled the pre-connect-buffered copy and sent it again. The engine
//! saw the same RegisterFunction twice and logged:
//!
//! ```text
//! [REGISTERED] Function foo::bar
//! [WARN] Function already exists in service
//! [REGISTERED] Function foo::bar
//! [WARN] Function foo::bar is already registered. Overwriting.
//! ```
//!
//! Fix: drain the outbound mpsc into the local queue BEFORE
//! `dedupe_registrations` runs, so the dedupe pass can collapse the two copies.
//! This test wires a minimal in-process WS server, counts incoming
//! `RegisterFunction` frames per id, and asserts the count is exactly 1 for a
//! function registered before the connect handshake completes. Without the fix
//! the count is 2.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use iii_sdk::{InitOptions, OtelConfig, RegisterFunction, register_worker};
use serde::Deserialize;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::accept_async;

/// Spawns a tiny WebSocket server on 127.0.0.1:0 that records every
/// `RegisterFunction` frame it receives. Returns the server URL and a handle to
/// the counts map. The server accepts a single connection and reads until the
/// client disconnects.
async fn spawn_counting_server() -> (String, Arc<Mutex<HashMap<String, u32>>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("local_addr");
    let url = format!("ws://{}", addr);

    let counts: Arc<Mutex<HashMap<String, u32>>> = Arc::new(Mutex::new(HashMap::new()));
    let counts_for_task = counts.clone();

    tokio::spawn(async move {
        // Accept exactly one client (the SDK under test). Anything after that
        // we ignore — this keeps the server simple and deterministic.
        eprintln!("[test-server] listening, awaiting accept...");
        let (stream, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("[test-server] accept failed: {e}");
                return;
            }
        };
        eprintln!("[test-server] tcp accept from {peer}");
        let ws = match accept_async(stream).await {
            Ok(ws) => ws,
            Err(e) => {
                eprintln!("[test-server] accept_async failed: {e}");
                return;
            }
        };
        eprintln!("[test-server] ws handshake complete");
        let (_write, mut read) = ws.split();

        while let Some(msg) = read.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("[test-server] read error: {e}");
                    break;
                }
            };
            let text = match msg {
                tokio_tungstenite::tungstenite::Message::Text(t) => t.to_string(),
                tokio_tungstenite::tungstenite::Message::Binary(b) => {
                    String::from_utf8_lossy(&b).to_string()
                }
                tokio_tungstenite::tungstenite::Message::Close(_) => break,
                _ => continue,
            };
            eprintln!("[test-server] frame: {}", &text[..text.len().min(200)]);

            // Wire format serializes the message_type enum as lowercase
            // concatenated (no underscore): "registerfunction". See
            // sdk/packages/rust/iii/src/types.rs.
            if let Ok(parsed) = serde_json::from_str::<Value>(&text)
                && parsed.get("type").and_then(|v| v.as_str()) == Some("registerfunction")
                && let Some(id) = parsed.get("id").and_then(|v| v.as_str())
            {
                let mut c = counts_for_task.lock().await;
                *c.entry(id.to_string()).or_insert(0) += 1;
                eprintln!("[test-server] register_function observed: {id}");
            }
        }
        eprintln!("[test-server] loop exit");
    });

    (url, counts)
}

#[derive(Deserialize, schemars::JsonSchema)]
struct EchoInput {
    msg: String,
}

fn echo(input: EchoInput) -> Result<String, String> {
    Ok(input.msg)
}

/// Pre-connect registration should send exactly one RegisterFunction on the
/// wire, not two. This is the core regression guard for the double-send bug.
#[tokio::test]
async fn register_function_before_connect_is_not_duplicated() {
    let (url, counts) = spawn_counting_server().await;

    // Disable OTel so the telemetry-system doesn't open a second ws that the
    // tiny counting server can't service — it accepts one connection and quits.
    // The bug only affects the main iii connection anyway.
    let opts = InitOptions {
        otel: Some(OtelConfig {
            enabled: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };

    let iii = register_worker(&url, opts);

    // This is the typical user pattern: synchronously register a function
    // immediately after construction, before the connect handshake has had a
    // chance to complete. `register_function_inner` inserts into
    // `self.inner.functions` AND pushes onto the outbound mpsc. Pre-fix, both
    // copies survived dedupe and the server received two frames.
    let _fn_ref =
        iii.register_function(RegisterFunction::new("regression::echo", echo).description("ech"));

    // Poll the server's count for up to 3s so the SDK's independent thread
    // has time to init OTel, connect, and flush the register queue. OTel init
    // runs before `run_connection` even when disabled, so the wall-clock is
    // dominated by that rather than the handshake.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut got = 0u32;
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let c = counts.lock().await;
        got = c.get("regression::echo").copied().unwrap_or(0);
        // Stop as soon as we see >=1 so we can also catch a spurious second
        // send before the deadline (bug-era behaviour was 2 back-to-back sends
        // within ~1ms of the handshake, so a 100ms pause is plenty).
        if got >= 1 {
            drop(c);
            tokio::time::sleep(Duration::from_millis(100)).await;
            let c2 = counts.lock().await;
            got = c2.get("regression::echo").copied().unwrap_or(0);
            break;
        }
    }

    assert_eq!(
        got, 1,
        "expected exactly one RegisterFunction for regression::echo, got {}",
        got
    );

    iii.shutdown();
}
