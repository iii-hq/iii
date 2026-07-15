//! End-to-end proof that function-id collisions within a namespace are rejected
//! on the real WebSocket registration path.
//!
//! Everything here drives a live `tokio_tungstenite` client against the real
//! `WorkerManager` axum router, in the message order the SDKs actually use
//! (`RegisterFunction` first, `engine::workers::register` second). Nothing is
//! hand-constructed: the `WorkerConnection` these tests exercise is the one
//! `handle_worker` builds, whose `namespace` field is `None` for life — so a
//! fix that only works on a doctored connection cannot pass this file.
//!
//! Two behaviors are covered:
//!   * `FUNCTION_NAMESPACE_CONFLICT` — a second live worker in the same
//!     namespace is refused that one function id, and keeps its connection.
//!   * the `(namespace, function_id)` owner re-key — the same id in *different*
//!     namespaces is legitimate, and dropping one owner must not orphan it.
//!
//! Worker-name collisions (`WORKER_NAMESPACE_CONFLICT`) are deferred to Task
//! 9.5; those tests are parked at the bottom of this file with the reasoning.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use iii::engine::Engine;
use iii::workers::engine_fn::EngineFunctionsWorker;
use iii::workers::traits::Worker;
use iii::workers::worker::WorkerManager;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message as WsMessage;

type Client = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn spawn_engine() -> (u16, Arc<Engine>) {
    iii::workers::observability::metrics::ensure_default_meter();

    let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
    let port = probe.local_addr().expect("local_addr").port();
    drop(probe);

    let engine = Arc::new(Engine::new());

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
    manager
        .start_background_tasks(shutdown_rx, shutdown_tx)
        .await
        .expect("start WorkerManager");

    (port, engine)
}

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

/// Connects and consumes the `WorkerRegistered` handshake, returning the
/// engine-assigned worker id. Observing that frame means `handle_worker` has
/// registered the connection and armed the namespace buffer.
async fn connect(port: u16) -> (Client, String) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/"))
        .await
        .expect("connect to / should succeed");

    let frame = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("WorkerRegistered should arrive")
        .expect("stream should yield")
        .expect("frame should decode");
    let msg: Value = serde_json::from_str(frame.to_text().expect("text frame")).expect("json");
    assert_eq!(msg["type"], "workerregistered");
    let id = msg["worker_id"].as_str().expect("worker_id").to_string();
    (ws, id)
}

async fn send_register_worker(ws: &mut Client, name: &str, namespace: Option<&str>) {
    let mut data = json!({ "runtime": "node", "name": name });
    if let Some(ns) = namespace {
        data["namespace"] = json!(ns);
    }
    ws.send(WsMessage::Text(
        json!({
            "type": "invokefunction",
            "invocation_id": uuid::Uuid::new_v4(),
            "function_id": "engine::workers::register",
            "data": data,
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send engine::workers::register");
}

async fn send_register_function(ws: &mut Client, id: &str) {
    send_described_function(ws, id, None).await;
}

/// `description` rides through to `Function::_description`, which is the only
/// field of the registered handler a test can read back — it stands in for
/// "whose handler is installed under this id".
async fn send_described_function(ws: &mut Client, id: &str, description: Option<&str>) {
    let mut msg = json!({
        "type": "registerfunction",
        "id": id,
        "request_format": null,
        "response_format": null,
    });
    if let Some(d) = description {
        msg["description"] = json!(d);
    }
    ws.send(WsMessage::Text(msg.to_string().into()))
        .await
        .expect("send RegisterFunction");
}

/// Reads frames until a `registrationrejected` arrives, or the stream closes /
/// the deadline passes. Other traffic (invocation results, pongs) is skipped.
async fn await_rejection(ws: &mut Client) -> Option<Value> {
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(frame)) = ws.next().await {
            let WsMessage::Text(text) = &frame else {
                continue;
            };
            let msg: Value = serde_json::from_str(text).ok()?;
            if msg["type"] == "registrationrejected" {
                return Some(msg);
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
}

/// Two distinct, live workers in one namespace registering the same function
/// id: the second registration is refused, but the worker keeps its connection
/// and the incumbent's handler is untouched.
#[tokio::test]
async fn duplicate_function_id_in_same_namespace_is_rejected_without_closing_the_connection() {
    let (port, engine) = spawn_engine().await;

    let (mut first, first_id) = connect(port).await;
    send_register_worker(&mut first, "alpha", Some("orders")).await;
    send_described_function(&mut first, "svc::f", Some("from-alpha")).await;
    assert!(
        eventually(|| engine.functions.get("orders", "svc::f").is_some()).await,
        "first worker's function must register"
    );

    let (mut second, _) = connect(port).await;
    send_register_worker(&mut second, "beta", Some("orders")).await;
    send_described_function(&mut second, "svc::f", Some("from-beta")).await;

    let rejection = await_rejection(&mut second)
        .await
        .expect("second worker must receive RegistrationRejected");
    assert_eq!(rejection["code"], "FUNCTION_NAMESPACE_CONFLICT");
    assert_eq!(rejection["namespace"], "orders");
    assert_eq!(rejection["worker_name"], "svc::f");
    assert_eq!(rejection["owner_worker_id"], first_id);

    // The connection must survive: a refused function is not a fatal error.
    // Ping/Pong round-trips only if the read loop is still running.
    second
        .send(WsMessage::Text(
            json!({ "type": "ping" }).to_string().into(),
        ))
        .await
        .expect("connection must still accept frames");
    let pong = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(frame)) = second.next().await {
            if let WsMessage::Text(text) = &frame
                && serde_json::from_str::<Value>(text).is_ok_and(|m| m["type"] == "pong")
            {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(pong, "the rejected worker's connection must stay open");

    // The refusal must be total, not cosmetic. Sending the rejection while
    // still writing the registration would leave the incumbent's owner entry
    // intact but its *handler* silently replaced by the rejected worker's —
    // the exact hijack this task exists to prevent, and invisible to any
    // assertion that only counts registry entries.
    assert_eq!(
        engine
            .functions
            .get("orders", "svc::f")
            .expect("incumbent's registration must still be there")
            ._description
            .as_deref(),
        Some("from-alpha"),
        "the incumbent's handler must be untouched — a rejected registration \
         must not reach any registry"
    );

    // And the incumbent must still hold the lease: dropping it releases the id.
    first.close(None).await.expect("close first worker");
    let first_uuid = uuid::Uuid::parse_str(&first_id).expect("uuid");
    assert!(
        eventually(|| engine.worker_registry.get_worker(&first_uuid).is_none()).await,
        "engine must reap the disconnected worker"
    );
    assert!(
        eventually(|| engine.functions.get("orders", "svc::f").is_none()).await,
        "the incumbent must still have owned the lease it took first"
    );

    let _ = second.close(None).await;
}

/// The `function_owners` re-key, proven end to end.
///
/// With owners keyed by bare function id, worker B's claim clobbers worker A's
/// entry. When A then disconnects, its CAS release fails (the owner is B), so
/// A's `("orders", "svc::f")` registration is orphaned forever — a dead handler
/// with no owner — while B's own entry is the one that looks correct.
#[tokio::test]
async fn dropping_one_of_two_cross_namespace_owners_orphans_nothing() {
    let (port, engine) = spawn_engine().await;

    let (mut orders, orders_id) = connect(port).await;
    send_register_worker(&mut orders, "alpha", Some("orders")).await;
    send_register_function(&mut orders, "svc::f").await;
    assert!(
        eventually(|| engine.functions.get("orders", "svc::f").is_some()).await,
        "orders worker's function must register"
    );

    let (mut analytics, _) = connect(port).await;
    send_register_worker(&mut analytics, "beta", Some("analytics")).await;
    send_register_function(&mut analytics, "svc::f").await;
    assert!(
        eventually(|| engine.functions.get("analytics", "svc::f").is_some()).await,
        "the same id in another namespace is legitimate and must register"
    );

    orders.close(None).await.expect("close orders worker");
    let orders_uuid = uuid::Uuid::parse_str(&orders_id).expect("uuid");
    assert!(
        eventually(|| engine.worker_registry.get_worker(&orders_uuid).is_none()).await,
        "engine must reap the disconnected worker"
    );

    assert!(
        eventually(|| engine.functions.get("orders", "svc::f").is_none()).await,
        "the dead worker's registration must be released from its own namespace, \
         not orphaned because another namespace's worker holds the bare-id lease"
    );
    assert!(
        engine.functions.get("analytics", "svc::f").is_some(),
        "the surviving worker's registration must be untouched"
    );

    let _ = analytics.close(None).await;
}

// ---------------------------------------------------------------------------
// PARKED FOR TASK 9.5 — worker-name collisions (`WORKER_NAMESPACE_CONFLICT`)
// ---------------------------------------------------------------------------
//
// These three tests passed against a working implementation of worker-name
// rejection. They are parked, not deleted, so 9.5 does not rewrite them from
// scratch: un-comment, re-add `claim_worker_name` + the close, and they should
// go green as-is (modulo whichever naming fix 9.5 lands).
//
// WHY THIS WAS DEFERRED — read before re-enabling. Rejecting a worker means
// closing its connection, and no SDK understands `RegistrationRejected` yet
// (Tasks 7-9). A closed client just reconnects and is rejected again: a live
// run showed 20+ reject/reconnect cycles in 60s. Rejection cannot ship before
// something exists that knows how to receive it.
//
// AND THE REJECTION RULE ITSELF IS NOT YET SOUND. Both SDKs name a worker
// `III_WORKER_NAME ?? hostname:pid` (rust: `sdk/packages/rust/iii/src/iii.rs`
// :286-289; node: `sdk/packages/node/iii/src/iii.ts:78`) — scoped to the
// PROCESS, not the connection. Two SDK clients in one process therefore share
// a name and are indistinguishable, to the engine, from a real collision.
// `engine/tests/worker_trigger_e2e.rs` is exactly this shape (two
// `InitOptions::default()` clients at :318 and :328) and the worker-name
// rejection killed it. Task 9.5 must first pick one:
//
//   (a) Only enforce for EXPLICIT names. Needs a new wire signal (e.g.
//       `RegisterWorkerInput.name_is_explicit`) — the engine cannot tell
//       `state` from a `hostname:pid` on its own.
//   (b) Make the SDK default unique per connection. Note the III_WORKER_NAME
//       comment in iii.rs:282-285: engine truth (`iii worker status`/`list`)
//       matches connections BY NAME, so changing the default affects that
//       matching.
//
// Related, same root cause: a worker that reconnects before the engine reaps
// its old connection is now rejected rather than taking over its lease
// (liveness == presence in `worker_registry.workers`). Whatever 9.5 picks
// should account for that window too.
//
// /// Drains until the server closes the stream. `true` means the engine hung up.
// async fn closed_by_engine(ws: &mut Client) -> bool {
//     tokio::time::timeout(Duration::from_secs(5), async {
//         loop {
//             match ws.next().await {
//                 Some(Ok(WsMessage::Close(_))) | None => return true,
//                 Some(Err(_)) => return true,
//                 Some(Ok(_)) => continue,
//             }
//         }
//     })
//     .await
//     .unwrap_or(false)
// }
//
// /// Two live workers claiming the same name in the same namespace: the second
// /// one is told why and hung up on.
// #[tokio::test]
// async fn second_worker_with_same_name_in_same_namespace_is_rejected_and_closed() {
//     let (port, engine) = spawn_engine().await;
//
//     let (mut winner, winner_id) = connect(port).await;
//     send_register_worker(&mut winner, "state", Some("orders")).await;
//     assert!(
//         eventually(|| engine
//             .worker_registry
//             .list_workers()
//             .iter()
//             .any(|w| w.name.as_deref() == Some("state")))
//         .await,
//         "first worker must register"
//     );
//
//     let (mut loser, _) = connect(port).await;
//     send_register_worker(&mut loser, "state", Some("orders")).await;
//
//     let rejection = await_rejection(&mut loser)
//         .await
//         .expect("loser must receive RegistrationRejected");
//     assert_eq!(rejection["code"], "WORKER_NAMESPACE_CONFLICT");
//     assert_eq!(rejection["namespace"], "orders");
//     assert_eq!(rejection["worker_name"], "state");
//     assert_eq!(rejection["owner_worker_id"], winner_id);
//
//     assert!(
//         closed_by_engine(&mut loser).await,
//         "the engine must close the rejected worker's connection"
//     );
//
//     let _ = winner.close(None).await;
// }
//
// /// The same worker name in a different namespace is a legitimate deployment,
// /// not a collision.
// #[tokio::test]
// async fn same_worker_name_in_a_different_namespace_coexists() {
//     let (port, engine) = spawn_engine().await;
//
//     let (mut orders, _) = connect(port).await;
//     send_register_worker(&mut orders, "state", Some("orders")).await;
//     assert!(
//         eventually(|| engine
//             .worker_registry
//             .list_workers()
//             .iter()
//             .any(|w| w.namespace.as_deref() == Some("orders")))
//         .await,
//         "orders worker must register"
//     );
//
//     let (mut analytics, _) = connect(port).await;
//     send_register_worker(&mut analytics, "state", Some("analytics")).await;
//
//     assert!(
//         eventually(|| engine
//             .worker_registry
//             .list_workers()
//             .iter()
//             .any(|w| w.namespace.as_deref() == Some("analytics")
//                 && w.name.as_deref() == Some("state")))
//         .await,
//         "the same name in another namespace must be accepted"
//     );
//
//     let _ = orders.close(None).await;
//     let _ = analytics.close(None).await;
// }
//
// /// The name lease is held by the connection, not the namespace: once the owner
// /// disconnects, the name is claimable again.
// #[tokio::test]
// async fn worker_name_is_claimable_again_after_the_owner_disconnects() {
//     let (port, engine) = spawn_engine().await;
//
//     let (mut first, first_id) = connect(port).await;
//     send_register_worker(&mut first, "state", Some("orders")).await;
//     assert!(
//         eventually(|| engine
//             .worker_registry
//             .list_workers()
//             .iter()
//             .any(|w| w.name.as_deref() == Some("state")))
//         .await,
//         "first worker must register"
//     );
//
//     first.close(None).await.expect("close first worker");
//     let first_uuid = uuid::Uuid::parse_str(&first_id).expect("uuid");
//     assert!(
//         eventually(|| engine.worker_registry.get_worker(&first_uuid).is_none()).await,
//         "engine must reap the disconnected worker"
//     );
//
//     let (mut second, second_id) = connect(port).await;
//     send_register_worker(&mut second, "state", Some("orders")).await;
//
//     let second_uuid = uuid::Uuid::parse_str(&second_id).expect("uuid");
//     assert!(
//         eventually(|| engine
//             .worker_registry
//             .get_worker(&second_uuid)
//             .is_some_and(|w| w.name.as_deref() == Some("state")))
//         .await,
//         "the freed name must be claimable by a new connection"
//     );
//
//     let _ = second.close(None).await;
// }
//
