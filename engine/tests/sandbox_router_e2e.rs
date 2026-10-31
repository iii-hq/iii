//! End-to-end proof that the engine's `sandbox::*` router forwards to the bound
//! adapter worker in its own namespace, answers S301/S302/S303/S304 itself, and
//! re-runs the `adapter::info` handshake after the adapter drops.
//!
//! The adapter is a raw `tokio_tungstenite` worker named `fake` in namespace
//! `fake-ns`, connected through the real `WorkerManager` router as in
//! `namespace_conflict_e2e.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use iii::engine::{Engine, EngineTrait};
use iii::function::{Function, FunctionResult};
use iii::protocol::ErrorBody;
use iii::workers::engine_fn::EngineFunctionsWorker;
use iii::workers::sandbox::SandboxRouter;
use iii::workers::traits::Worker;
use iii::workers::worker::WorkerManager;
use iii_sandbox_contract::{OPERATIONS, ROUTER_INFO_ID, SandboxErrorCode, code_of, error_payload};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message as WsMessage;

async fn spawn_engine(binding: Option<Value>) -> (u16, Arc<Engine>) {
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

    let router = SandboxRouter::create(engine.clone(), binding)
        .await
        .expect("create SandboxRouter");
    router.initialize().await.expect("initialize SandboxRouter");
    router.register_functions(engine.clone());

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

fn bound() -> Option<Value> {
    Some(json!({ "adapter": "fake", "namespace": "fake-ns" }))
}

struct FakeAdapter {
    /// Ids of invocations the fake received and will never answer.
    in_flight: mpsc::UnboundedReceiver<String>,
    /// How many times `fake::adapter::info` was called.
    info_calls: Arc<AtomicUsize>,
    /// Aborting it drops the socket.
    task: JoinHandle<()>,
}

const FAKE_IDS: [&str; 5] = [
    "fake::adapter::info",
    "fake::exec",
    "fake::fs::ls",
    "fake::fs::stat",
    "fake::create",
];

fn text(v: Value) -> WsMessage {
    WsMessage::Text(v.to_string().into())
}

/// Connects the `fake` adapter in `fake-ns`, reporting contract `major`, and
/// returns once its last id is in the engine's registry.
async fn fake_adapter(port: u16, major: u32, engine: &Engine) -> FakeAdapter {
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

    for id in FAKE_IDS {
        ws.send(text(json!({
            "type": "registerfunction",
            "id": id,
            "request_format": null,
            "response_format": null,
        })))
        .await
        .expect("send RegisterFunction");
    }
    ws.send(text(json!({
        "type": "invokefunction",
        "invocation_id": uuid::Uuid::new_v4(),
        "function_id": "engine::workers::register",
        "data": { "name": "fake", "namespace": "fake-ns" },
    })))
    .await
    .expect("send engine::workers::register");

    let (tx, in_flight) = mpsc::unbounded_channel();
    let info_calls = Arc::new(AtomicUsize::new(0));
    let calls = info_calls.clone();
    let task = tokio::spawn(async move {
        while let Some(Ok(frame)) = ws.next().await {
            let Ok(txt) = frame.to_text() else { continue };
            let Ok(msg) = serde_json::from_str::<Value>(txt) else {
                continue;
            };
            if msg["type"] != "invokefunction" {
                continue;
            }
            let function_id = msg["function_id"].as_str().unwrap_or_default().to_string();
            let mut reply = json!({
                "type": "invocationresult",
                "invocation_id": msg["invocation_id"],
                "function_id": function_id,
            });
            match function_id.as_str() {
                "fake::adapter::info" => {
                    calls.fetch_add(1, Ordering::Relaxed);
                    reply["result"] = json!({
                        "adapter": "fake",
                        "adapter_version": "0.0.0",
                        "contract": { "major": major, "minor": 0 },
                    });
                }
                "fake::exec" => {
                    reply["result"] = json!({ "input": msg["data"], "metadata": msg["metadata"] });
                }
                "fake::fs::ls" => {
                    reply["error"] = json!({
                        "code": "invocation_failed",
                        "message": error_payload(SandboxErrorCode::S211, "file not found: /nope", None, None).to_string(),
                    });
                }
                "fake::fs::stat" => {
                    reply["error"] = json!({
                        "code": "function_not_found",
                        "message": "nested trigger missed",
                    });
                }
                "fake::create" => {
                    let _ = tx.send(function_id);
                    continue;
                }
                _ => continue,
            }
            let _ = ws.send(text(reply)).await;
        }
    });

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while engine.functions.get("fake-ns", "fake::create").is_none() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "fake adapter ids never reached fake-ns"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    FakeAdapter {
        in_flight,
        info_calls,
        task,
    }
}

async fn call(
    engine: &Engine,
    id: &str,
    input: Value,
    metadata: Option<Value>,
) -> Result<Option<Value>, ErrorBody> {
    engine
        .call_with_metadata_ns("default", id, input, metadata)
        .await
}

fn payload(err: &ErrorBody) -> Value {
    serde_json::from_str::<Value>(&err.message).expect("S-code payload")
}

#[tokio::test]
async fn router_registers_every_public_id_even_when_unbound() {
    let (_port, engine) = spawn_engine(None).await;

    assert_eq!(OPERATIONS.len(), 16);
    for op in OPERATIONS {
        let f = engine
            .functions
            .get("default", &op.public_id())
            .unwrap_or_else(|| panic!("{} is not registered", op.public_id()));
        assert_eq!(f.request_format, Some((op.request_schema)()));
        assert_eq!(f.response_format, Some((op.response_schema)()));
    }
    assert_eq!(
        engine
            .functions
            .get("default", ROUTER_INFO_ID)
            .expect("router info is registered")
            .metadata,
        Some(json!({"internal": true}))
    );
}

#[tokio::test]
async fn unbound_router_answers_s303_but_info_never_errors() {
    let (_port, engine) = spawn_engine(None).await;

    let err = call(&engine, "sandbox::exec", json!({}), None)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err.message), Some(SandboxErrorCode::S303));
    let p = payload(&err);
    assert_eq!(p["retryable"], false);
    assert_eq!(
        p["message"],
        "no sandbox adapter configured: this engine is not managed by a Compose file; set sandbox.adapter in its engine config (Compose-managed engines default to sandbox-libkrun)"
    );

    let info = call(&engine, ROUTER_INFO_ID, json!({}), None).await;
    assert_eq!(
        info.unwrap(),
        Some(
            json!({"adapter": null, "namespace": null, "contract": {"major": 1, "minor": 1}, "info": null})
        )
    );
}

#[tokio::test]
async fn create_rejects_a_binding_to_the_router_itself() {
    let err = SandboxRouter::create(Arc::new(Engine::new()), Some(json!({"adapter": "sandbox"})))
        .await
        .err()
        .expect("adapter 'sandbox' is rejected");
    assert_eq!(
        err.to_string(),
        "[INVALID_SANDBOX_CONFIG] sandbox.adapter 'sandbox' must match [a-z0-9-]+ and must not be 'sandbox'"
    );
}

#[tokio::test]
async fn bound_router_without_adapter_answers_s301() {
    let (_port, engine) = spawn_engine(bound()).await;

    let err = call(&engine, "sandbox::exec", json!({}), None)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err.message), Some(SandboxErrorCode::S301));
    let p = payload(&err);
    assert_eq!(p["retryable"], true);
    assert_eq!(
        p["message"],
        "adapter fake not connected in namespace fake-ns; compose::add iii-sandbox installs it in the Compose project there, or, if none runs, declare a fake container in a worker-compose.yaml for fake-ns"
    );

    let info = call(&engine, ROUTER_INFO_ID, json!({}), None).await;
    assert_eq!(
        info.unwrap(),
        Some(
            json!({"adapter": "fake", "namespace": "fake-ns", "contract": {"major": 1, "minor": 1}, "info": null})
        )
    );
}

#[tokio::test]
async fn forwards_across_namespaces_with_payload_and_metadata_untouched() {
    let (port, engine) = spawn_engine(bound()).await;
    let _fake = fake_adapter(port, 1, &engine).await;

    let input = json!({"sandbox_id": "0b6c2f0e-1f7e-4a53-9a43-5d1f8f2b9c11", "cmd": "echo hi", "env": ["A=1"],
        "channel": {"channel_id": "c1", "access_key": "k1", "direction": "write"}, "not_in_schema": true});
    let metadata = json!({"trace": "m1"});

    let out = call(
        &engine,
        "sandbox::exec",
        input.clone(),
        Some(metadata.clone()),
    )
    .await;
    assert_eq!(
        out.unwrap(),
        Some(json!({"input": input, "metadata": metadata}))
    );

    let info = call(&engine, ROUTER_INFO_ID, json!({}), None)
        .await
        .unwrap()
        .expect("router info reply");
    assert_eq!(
        info["info"],
        json!({"adapter": "fake", "adapter_version": "0.0.0", "contract": {"major": 1, "minor": 0}})
    );
}

#[tokio::test]
async fn adapter_errors_pass_through_untouched() {
    let (port, engine) = spawn_engine(bound()).await;
    let _fake = fake_adapter(port, 1, &engine).await;

    let err = call(&engine, "sandbox::fs::ls", json!({}), None)
        .await
        .unwrap_err();
    assert_eq!(err.code, "invocation_failed");
    assert_eq!(
        err.message,
        error_payload(SandboxErrorCode::S211, "file not found: /nope", None, None).to_string()
    );
}

#[tokio::test]
async fn s302_when_adapter_major_differs() {
    let (port, engine) = spawn_engine(bound()).await;
    let _fake = fake_adapter(port, 2, &engine).await;

    let err = call(&engine, "sandbox::exec", json!({}), None)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err.message), Some(SandboxErrorCode::S302));
    let p = payload(&err);
    assert_eq!(p["retryable"], false);
    assert_eq!(
        p["message"],
        "adapter fake implements sandbox contract 2.0, but this engine needs major 1; install a fake release that implements sandbox contract 1.x"
    );
}

#[tokio::test]
async fn invocation_stopped_on_adapter_disconnect_clears_handshake() {
    let (port, engine) = spawn_engine(bound()).await;
    let mut fake = fake_adapter(port, 1, &engine).await;

    call(&engine, "sandbox::exec", json!({}), None)
        .await
        .expect("exec succeeds and caches the handshake");

    let caller = engine.clone();
    let create = tokio::spawn(async move {
        call(&caller, "sandbox::create", json!({"image": "python"}), None).await
    });
    let received = tokio::time::timeout(Duration::from_secs(5), fake.in_flight.recv())
        .await
        .expect("fake::create should reach the adapter");
    assert_eq!(received.as_deref(), Some("fake::create"));
    fake.task.abort();

    let err = tokio::time::timeout(Duration::from_secs(5), create)
        .await
        .expect("create should return within 5 s of the disconnect")
        .expect("create task should not panic")
        .unwrap_err();
    assert_eq!(err.code, "invocation_stopped");

    let _fake = fake_adapter(port, 2, &engine).await;
    let err = call(&engine, "sandbox::exec", json!({}), None)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err.message), Some(SandboxErrorCode::S302));
}

#[tokio::test]
async fn idle_adapter_disconnect_answers_s301_and_clears_handshake() {
    let (port, engine) = spawn_engine(bound()).await;
    let fake = fake_adapter(port, 1, &engine).await;

    call(&engine, "sandbox::exec", json!({}), None)
        .await
        .expect("exec succeeds and caches the handshake");

    fake.task.abort();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while FAKE_IDS
        .iter()
        .any(|id| engine.functions.get("fake-ns", id).is_some())
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "fake adapter ids were never removed from fake-ns"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // `adapter::info` is gone, so the handshake answers S301 before any forward.
    let err = call(&engine, "sandbox::exec", json!({}), None)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err.message), Some(SandboxErrorCode::S301));
    let p = payload(&err);
    assert_eq!(p["retryable"], true);
    assert_eq!(
        p["message"],
        "adapter fake not connected in namespace fake-ns; compose::add iii-sandbox installs it in the Compose project there, or, if none runs, declare a fake container in a worker-compose.yaml for fake-ns"
    );

    let _fake = fake_adapter(port, 2, &engine).await;
    let err = call(&engine, "sandbox::exec", json!({}), None)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err.message), Some(SandboxErrorCode::S302));
}

#[tokio::test]
async fn op_missing_on_connected_adapter_answers_s304_and_keeps_handshake() {
    let (port, engine) = spawn_engine(bound()).await;
    let fake = fake_adapter(port, 1, &engine).await;

    call(&engine, "sandbox::exec", json!({}), None)
        .await
        .expect("exec succeeds and caches the handshake");

    // The fake registers adapter::info but not fake::list.
    for _ in 0..2 {
        let err = call(&engine, "sandbox::list", json!({}), None)
            .await
            .unwrap_err();
        assert_eq!(code_of(&err.message), Some(SandboxErrorCode::S304));
        let p = payload(&err);
        assert_eq!(p["retryable"], false);
        assert_eq!(
            p["message"],
            "adapter fake does not implement sandbox::list"
        );
    }
    assert_eq!(
        fake.info_calls.load(Ordering::Relaxed),
        1,
        "S304 must not reset the handshake"
    );
}

#[tokio::test]
async fn adapter_swapped_while_idle_answers_s302() {
    let (port, engine) = spawn_engine(bound()).await;
    let fake = fake_adapter(port, 1, &engine).await;

    call(&engine, "sandbox::exec", json!({}), None)
        .await
        .expect("exec succeeds and caches the handshake");

    // No call lands while the adapter is gone.
    fake.task.abort();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while engine
        .functions
        .get("fake-ns", "fake::adapter::info")
        .is_some()
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "fake::adapter::info was never removed from fake-ns"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let _fake = fake_adapter(port, 2, &engine).await;
    let err = call(&engine, "sandbox::exec", json!({}), None)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err.message), Some(SandboxErrorCode::S302));
}

#[tokio::test]
async fn adapter_returned_function_not_found_passes_through() {
    let (port, engine) = spawn_engine(bound()).await;
    let _fake = fake_adapter(port, 1, &engine).await;

    let err = call(&engine, "sandbox::fs::stat", json!({}), None)
        .await
        .unwrap_err();
    assert_eq!(err.code, "function_not_found");
    assert_eq!(err.message, "nested trigger missed");
}

/// Caches the handshake, drops `fake::exec` and returns it with a pending
/// `sandbox::exec` that is still inside the router's missing-op grace.
async fn exec_in_grace(
    engine: &Arc<Engine>,
) -> (Function, JoinHandle<Result<Option<Value>, ErrorBody>>) {
    call(engine, "sandbox::exec", json!({}), None)
        .await
        .expect("exec succeeds and caches the handshake");
    let exec = engine
        .functions
        .get("fake-ns", "fake::exec")
        .expect("fake::exec is registered");
    engine.functions.remove("fake-ns", "fake::exec");
    let caller = engine.clone();
    let pending =
        tokio::spawn(
            async move { call(&caller, "sandbox::exec", json!({"cmd": "x"}), None).await },
        );
    tokio::time::sleep(Duration::from_millis(50)).await;
    (exec, pending)
}

#[tokio::test]
async fn op_registered_just_after_adapter_info_is_forwarded() {
    let (port, engine) = spawn_engine(bound()).await;
    let _fake = fake_adapter(port, 1, &engine).await;

    // A reconnecting adapter can replay `adapter::info` before its ops.
    let (exec, pending) = exec_in_grace(&engine).await;
    engine
        .functions
        .register_function_ns("fake-ns", "fake::exec".into(), exec);

    let out = pending.await.expect("call task should not panic");
    assert_eq!(
        out.expect("exec is forwarded once the op registers"),
        Some(json!({"input": {"cmd": "x"}, "metadata": null}))
    );
}

#[tokio::test]
async fn adapter_gone_during_missing_op_grace_answers_s301() {
    let (port, engine) = spawn_engine(bound()).await;
    let _fake = fake_adapter(port, 1, &engine).await;

    // A disconnecting adapter can drop an op before `adapter::info`.
    let (_exec, pending) = exec_in_grace(&engine).await;
    engine.functions.remove("fake-ns", "fake::adapter::info");

    let err = pending
        .await
        .expect("call task should not panic")
        .unwrap_err();
    assert_eq!(code_of(&err.message), Some(SandboxErrorCode::S301));
    assert_eq!(payload(&err)["retryable"], true);
}

#[tokio::test]
async fn adapter_swapped_during_missing_op_grace_answers_s302() {
    let (port, engine) = spawn_engine(bound()).await;
    let _fake = fake_adapter(port, 1, &engine).await;

    // A major-2 adapter replaces the old one before the grace ends.
    let (exec, pending) = exec_in_grace(&engine).await;
    let info = Function {
        handler: Arc::new(|_invocation_id, _input, _session, _metadata| {
            Box::pin(async {
                FunctionResult::Success(Some(json!({
                    "adapter": "fake",
                    "adapter_version": "0.0.0",
                    "contract": { "major": 2, "minor": 0 },
                })))
            })
        }),
        _function_id: "fake::adapter::info".into(),
        _description: None,
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function_ns("fake-ns", "fake::adapter::info".into(), info);
    engine
        .functions
        .register_function_ns("fake-ns", "fake::exec".into(), exec);

    let err = pending
        .await
        .expect("call task should not panic")
        .unwrap_err();
    assert_eq!(code_of(&err.message), Some(SandboxErrorCode::S302));
}
