//! End-to-end proof that a fired trigger reaches the function in the namespace
//! of the worker that REGISTERED the trigger — not a same-named function that
//! happens to live in `default`.
//!
//! The bug this guards: `Engine::fire_triggers` dispatched every trigger through
//! a path that resolved the target function unconditionally in `default`. A
//! worker in namespace `orders` that registered `state::set` and a trigger for
//! it would, at fire time, silently execute the `state::set` of a DIFFERENT
//! worker in `default`. Not "not found" — the wrong function, no error.
//!
//! Everything here drives the real `WorkerManager` axum router over live
//! WebSockets, in the message order the SDKs use. Nothing hand-builds a
//! `Trigger` and pokes `.namespace`: the trigger's namespace is captured from
//! the connection the way it really is, and the fire is driven through the real
//! `Engine::fire_triggers`. Reverting the fix turns the decisive test RED with
//! the `default` worker receiving the invocation.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use iii::engine::Engine;
use iii::function::FunctionResult;
use iii::protocol::DEFAULT_NAMESPACE;
use iii::workers::engine_fn::{EngineFunctionsWorker, TRIGGER_FUNCTIONS_AVAILABLE};
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

async fn connect(port: u16) -> Client {
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
    ws
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

async fn send_register_function(ws: &mut Client, id: &str, description: &str) {
    ws.send(WsMessage::Text(
        json!({
            "type": "registerfunction",
            "id": id,
            "description": description,
            "request_format": null,
            "response_format": null,
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send RegisterFunction");
}

async fn send_register_trigger(ws: &mut Client, id: &str, trigger_type: &str, function_id: &str) {
    ws.send(WsMessage::Text(
        json!({
            "type": "registertrigger",
            "id": id,
            "trigger_type": trigger_type,
            "function_id": function_id,
            "config": {},
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send RegisterTrigger");
}

/// Reads frames until an `InvokeFunction` for `function_id` arrives, or the
/// deadline passes. `true` means this connection was asked to run the function.
async fn received_invoke(ws: &mut Client, function_id: &str) -> bool {
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(Ok(frame)) = ws.next().await {
            let WsMessage::Text(text) = &frame else {
                continue;
            };
            let Ok(msg): Result<Value, _> = serde_json::from_str(text) else {
                continue;
            };
            if msg["type"] == "invokefunction" && msg["function_id"] == function_id {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false)
}

/// Calls an engine introspection function in-process. Content still comes from
/// the registries the real WebSocket path populated.
async fn call_engine_fn(engine: &Arc<Engine>, function_id: &str, input: Value) -> Value {
    let function = engine
        .functions
        .get(DEFAULT_NAMESPACE, function_id)
        .unwrap_or_else(|| panic!("{function_id} should be registered"));
    match function.call_handler(None, input, None).await {
        FunctionResult::Success(Some(value)) => value,
        _ => panic!("{function_id} should return a result"),
    }
}

/// A registered trigger's introspection detail must resolve its target function
/// in the trigger's own namespace — not `default`. Before the fix,
/// `engine::registered-triggers::info` resolved the target in `default`
/// (the `TRIGGER_TARGET_NAMESPACE` const), so a trigger whose target lives in
/// `orders` reported a null `function` detail.
#[tokio::test]
async fn registered_trigger_info_resolves_its_target_in_the_triggers_namespace() {
    let (port, engine) = spawn_engine().await;

    let mut orders = connect(port).await;
    send_register_worker(&mut orders, "orders-worker", Some("orders")).await;
    send_register_function(&mut orders, "orders::handler", "the orders handler").await;
    send_register_trigger(
        &mut orders,
        "t-info",
        TRIGGER_FUNCTIONS_AVAILABLE,
        "orders::handler",
    )
    .await;

    assert!(
        eventually(
            || engine.functions.get("orders", "orders::handler").is_some()
                && engine.trigger_registry.triggers.contains_key("t-info")
        )
        .await,
        "the orders function and trigger must register"
    );

    let result = call_engine_fn(
        &engine,
        "engine::registered-triggers::info",
        json!({ "id": "t-info" }),
    )
    .await;

    assert_eq!(
        result["function"]["namespace"],
        json!("orders"),
        "the target function detail must resolve in the trigger's namespace; got: {result}"
    );
    assert_eq!(
        result["function"]["worker_name"],
        json!("orders-worker"),
        "the target function must be attributed to its real owner; got: {result}"
    );

    let _ = orders.close(None).await;
}

/// The decisive case. Two workers register the SAME function id `state::set` —
/// one in `orders`, one in `default`. A trigger registered by the `orders`
/// worker must, when fired, invoke the `orders` copy — the worker that
/// registered the trigger — never the `default` one.
#[tokio::test]
async fn fired_trigger_invokes_the_function_in_the_registering_workers_namespace() {
    let (port, engine) = spawn_engine().await;

    // orders worker: state::set + a functions-available trigger for it.
    let mut orders = connect(port).await;
    send_register_worker(&mut orders, "orders-worker", Some("orders")).await;
    send_register_function(&mut orders, "state::set", "from-orders").await;
    send_register_trigger(
        &mut orders,
        "t-orders",
        TRIGGER_FUNCTIONS_AVAILABLE,
        "state::set",
    )
    .await;

    // default worker: the same id, in default.
    let mut default = connect(port).await;
    send_register_worker(&mut default, "default-worker", None).await;
    send_register_function(&mut default, "state::set", "from-default").await;

    assert!(
        eventually(|| engine.functions.get("orders", "state::set").is_some()
            && engine.functions.get("default", "state::set").is_some()
            && engine.trigger_registry.triggers.contains_key("t-orders"))
        .await,
        "both functions and the trigger must register"
    );

    engine
        .fire_triggers(TRIGGER_FUNCTIONS_AVAILABLE, json!({ "event": "test" }))
        .await;

    // The orders worker — the one that registered the trigger — must be the one
    // asked to run state::set.
    assert!(
        received_invoke(&mut orders, "state::set").await,
        "the trigger's own namespace (orders) must receive the invocation"
    );
    // And the default worker's identically-named function must NOT be invoked.
    assert!(
        !received_invoke(&mut default, "state::set").await,
        "the default worker's same-named function must NOT be fired by an orders trigger"
    );

    let _ = orders.close(None).await;
    let _ = default.close(None).await;
}

/// The load-bearing coupling: a `RegisterTrigger` that arrives BEFORE
/// `engine::workers::register` must still resolve in the worker's real
/// namespace once the buffer drains — not `default`. If `RegisterTrigger` were
/// left out of the registration buffer, its namespace would be captured as
/// `default` (unknown at arrival) and the bug would return.
#[tokio::test]
async fn trigger_registered_before_workers_register_resolves_in_the_real_namespace() {
    let (port, engine) = spawn_engine().await;

    let mut orders = connect(port).await;
    // Function + trigger flushed BEFORE the namespace-bearing workers::register,
    // exactly as an SDK that predates the reorder does.
    send_register_function(&mut orders, "state::set", "from-orders").await;
    send_register_trigger(
        &mut orders,
        "t-early",
        TRIGGER_FUNCTIONS_AVAILABLE,
        "state::set",
    )
    .await;
    send_register_worker(&mut orders, "orders-worker", Some("orders")).await;

    let mut default = connect(port).await;
    send_register_worker(&mut default, "default-worker", None).await;
    send_register_function(&mut default, "state::set", "from-default").await;

    assert!(
        eventually(|| engine.trigger_registry.triggers.contains_key("t-early")
            && engine.functions.get("orders", "state::set").is_some()
            && engine.functions.get("default", "state::set").is_some())
        .await,
        "the buffered function and trigger must drain and register"
    );

    // The trigger the buffer drained must carry the real namespace, captured at
    // the point it was actually applied — not the `default` that was in force
    // when the message arrived.
    let ns = engine
        .trigger_registry
        .triggers
        .get("t-early")
        .expect("t-early registered")
        .namespace
        .clone();
    assert_eq!(
        ns, "orders",
        "a trigger buffered before workers::register must resolve in the drained namespace"
    );

    engine
        .fire_triggers(TRIGGER_FUNCTIONS_AVAILABLE, json!({ "event": "test" }))
        .await;

    assert!(
        received_invoke(&mut orders, "state::set").await,
        "the early-registered trigger must still fire the orders function"
    );
    assert!(
        !received_invoke(&mut default, "state::set").await,
        "the default worker's same-named function must NOT be fired"
    );

    let _ = orders.close(None).await;
    let _ = default.close(None).await;
}
