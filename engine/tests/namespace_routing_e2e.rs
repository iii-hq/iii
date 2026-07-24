//! End-to-end proof that `InvokeFunction` routes strictly by the namespace the
//! caller asked for.
//!
//! Two workers export the same function id, `state::get`: one declares no
//! namespace (so it lands in `default`), the other declares `analytics`. The
//! only thing that may decide which handler answers is the `namespace` field on
//! the invoke message — never the namespace of the connection that sent it.
//!
//! These tests deliberately drive a live WebSocket client through the real
//! `WorkerManager` router. The lookup that this task flips lives behind
//! `Engine::connection_namespace`, which reads engine-owned state rather than
//! the (stale, cloned) `WorkerConnection.namespace` field. A test that hand-built
//! a `WorkerConnection` and called `router_msg` directly would bypass that state
//! entirely and stay green even if the production wiring were deleted.

use std::sync::Arc;
use std::time::Duration;

use axum::{Json, Router, routing::post};
use futures_util::{SinkExt, StreamExt};
use iii::config::SecurityConfig;
use iii::engine::Engine;
use iii::protocol::DEFAULT_NAMESPACE;
use iii::workers::engine_fn::EngineFunctionsWorker;
use iii::workers::http_functions::{HttpFunctionsWorker, config::HttpFunctionsConfig};
use iii::workers::traits::Worker;
use iii::workers::worker::WorkerManager;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

/// Boots the engine-functions module (which owns `engine::workers::register`,
/// the call that reveals a connection's namespace) plus the `WorkerManager`
/// axum router on a free port.
async fn spawn_engine() -> (u16, Arc<Engine>) {
    iii::workers::observability::metrics::ensure_default_meter();

    // Pre-bind to discover a free port so this test does not race CI parallelism.
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

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn text(v: Value) -> WsMessage {
    WsMessage::Text(v.to_string().into())
}

/// Connects and consumes the `WorkerRegistered` frame, which the engine sends
/// once the connection is registered and its namespace buffer is armed.
async fn connect(port: u16) -> Ws {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/"))
        .await
        .expect("connect to / should succeed");
    let registered = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("WorkerRegistered should arrive")
        .expect("stream should yield")
        .expect("frame should decode");
    let registered: Value =
        serde_json::from_str(registered.to_text().expect("text frame")).expect("json");
    assert_eq!(registered["type"], "workerregistered");
    ws
}

/// Sends `engine::workers::register`, declaring `namespace` when present. This
/// is the only way a connection's namespace ever reaches the engine.
async fn declare_worker(ws: &mut Ws, name: &str, namespace: Option<&str>) {
    let mut data = json!({ "runtime": "node", "name": name });
    if let Some(ns) = namespace {
        data["namespace"] = json!(ns);
    }
    ws.send(text(json!({
        "type": "invokefunction",
        "invocation_id": uuid::Uuid::new_v4(),
        "function_id": "engine::workers::register",
        "data": data,
    })))
    .await
    .expect("send engine::workers::register");
}

/// Registers `state::get` on a fresh connection, declares `namespace`, and then
/// serves every `state::get` invocation with `reply` until the socket closes.
async fn spawn_executor(
    port: u16,
    name: &'static str,
    namespace: Option<&'static str>,
    reply: &'static str,
) {
    let mut ws = connect(port).await;

    ws.send(text(json!({
        "type": "registerfunction",
        "id": "state::get",
        "request_format": null,
        "response_format": null,
    })))
    .await
    .expect("send RegisterFunction");

    declare_worker(&mut ws, name, namespace).await;

    tokio::spawn(async move {
        while let Some(Ok(frame)) = ws.next().await {
            let Ok(txt) = frame.to_text() else { continue };
            let Ok(msg) = serde_json::from_str::<Value>(txt) else {
                continue;
            };
            if msg["type"] == "invokefunction" && msg["function_id"] == "state::get" {
                let _ = ws
                    .send(text(json!({
                        "type": "invocationresult",
                        "invocation_id": msg["invocation_id"],
                        "function_id": "state::get",
                        "result": reply,
                    })))
                    .await;
            }
        }
    });
}

/// Polls until `f` holds or the deadline passes.
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

/// Boots an engine with a `default` executor answering `"from-default"` and an
/// `analytics` executor answering `"from-analytics"`, both exporting
/// `state::get`.
async fn spawn_engine_with_both_executors() -> (u16, Arc<Engine>) {
    let (port, engine) = spawn_engine().await;

    spawn_executor(port, "default-worker", None, "from-default").await;
    spawn_executor(
        port,
        "analytics-worker",
        Some("analytics"),
        "from-analytics",
    )
    .await;

    assert!(
        eventually(|| engine
            .functions
            .get(DEFAULT_NAMESPACE, "state::get")
            .is_some()
            && engine.functions.get("analytics", "state::get").is_some())
        .await,
        "both executors must have registered before the invoke assertions run"
    );

    (port, engine)
}

/// Invokes `state::get` from a connection whose own namespace is
/// `caller_namespace`, targeting `target_namespace`, and returns the
/// `InvocationResult` frame.
async fn invoke_state_get(
    port: u16,
    caller_name: &str,
    caller_namespace: Option<&str>,
    target_namespace: Option<&str>,
) -> Value {
    let mut ws = connect(port).await;

    if caller_namespace.is_some() {
        declare_worker(&mut ws, caller_name, caller_namespace).await;
        // Wait for the register call's own result, which only comes back after
        // `resolve_connection_namespace` has pinned the connection. Without this
        // the invoke below could race the namespace resolution.
        wait_for_result(&mut ws, "engine::workers::register").await;
    }

    let invocation_id = uuid::Uuid::new_v4();
    let mut msg = json!({
        "type": "invokefunction",
        "invocation_id": invocation_id,
        "function_id": "state::get",
        "data": {},
    });
    if let Some(ns) = target_namespace {
        msg["namespace"] = json!(ns);
    }
    ws.send(text(msg)).await.expect("send InvokeFunction");

    let result = wait_for_result(&mut ws, "state::get").await;
    let _ = ws.close(None).await;
    result
}

/// Reads until an `invocationresult` for `function_id` arrives.
async fn wait_for_result(ws: &mut Ws, function_id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(frame)) = ws.next().await {
            let Ok(txt) = frame.to_text() else { continue };
            let Ok(msg) = serde_json::from_str::<Value>(txt) else {
                continue;
            };
            if msg["type"] == "invocationresult" && msg["function_id"] == function_id {
                return msg;
            }
        }
        panic!("socket closed before an InvocationResult for {function_id} arrived");
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for an InvocationResult for {function_id}"))
}

#[tokio::test]
async fn invoke_without_a_namespace_resolves_in_default() {
    let (port, _engine) = spawn_engine_with_both_executors().await;

    let result = invoke_state_get(port, "caller", None, None).await;

    assert_eq!(
        result["result"], "from-default",
        "an invoke carrying no namespace must resolve only in `default`; got {result}"
    );
}

#[tokio::test]
async fn invoke_with_an_explicit_namespace_resolves_only_in_that_namespace() {
    let (port, _engine) = spawn_engine_with_both_executors().await;

    let result = invoke_state_get(port, "caller", None, Some("analytics")).await;

    assert_eq!(
        result["result"], "from-analytics",
        "an invoke naming `analytics` must reach the analytics worker; got {result}"
    );
}

#[tokio::test]
async fn invoke_in_an_unknown_namespace_fails_with_a_hint_listing_the_known_namespaces() {
    let (port, _engine) = spawn_engine_with_both_executors().await;

    let result = invoke_state_get(port, "caller", None, Some("nope")).await;

    assert!(
        result["result"].is_null(),
        "an unknown namespace must not fall back to any other namespace; got {result}"
    );
    let message = result["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("expected an error body; got {result}"))
        .to_string();
    assert!(
        message.contains("analytics") && message.contains("default"),
        "the miss must hint at the namespaces where `state::get` does exist; got {message}"
    );
}

/// The load-bearing assertion of strict routing: the caller's own namespace is
/// not part of resolution. An `analytics` worker that omits the namespace gets
/// `default`, not its own.
#[tokio::test]
async fn the_callers_namespace_does_not_influence_resolution() {
    let (port, _engine) = spawn_engine_with_both_executors().await;

    let result = invoke_state_get(port, "analytics-caller", Some("analytics"), None).await;

    assert_eq!(
        result["result"], "from-default",
        "a caller in `analytics` that omits the namespace must still resolve in `default`; got {result}"
    );
}

// ---------------------------------------------------------------------------
// RBAC's view of the resolved function
//
// RBAC stays keyed by function id, but `FunctionFilter::Metadata` matches on the
// *resolved function's* metadata. So the pre-RBAC lookup must resolve in the
// requested namespace too: resolve it in `default` and an `analytics` invoke is
// judged against `default`'s metadata, which can flip allow into deny (or, worse,
// deny into allow).
//
// This one drives `router_msg` directly rather than a socket, and that is
// correct here rather than a shortcut: resolution reads the `namespace` field of
// the invoke *message* and never `connection_namespace`, so the connection
// carries no state this path could read. The registry entries below are written
// through the real `register_function_ns`, so nothing about the lookup is faked.
// ---------------------------------------------------------------------------

/// The pre-RBAC lookup must resolve in the invoke's namespace, because RBAC's
/// metadata filters read the resolved function.
///
/// Point that lookup at `default` and this test fails: the `analytics` invoke is
/// checked against `default`'s `tier: public` metadata and is wrongly allowed.
#[tokio::test]
async fn rbac_metadata_filters_see_the_function_from_the_requested_namespace() {
    use iii::engine::{EngineTrait, Handler, Outbound, RegisterFunctionRequest};
    use iii::function::FunctionResult;
    use iii::protocol::Message;
    use iii::worker_connections::WorkerConnection;
    use iii::workers::worker::{
        WorkerManagerConfig,
        rbac_config::{FunctionFilter, MetadataValue, RbacConfig},
        rbac_session::Session,
    };
    use std::collections::HashMap;

    iii::workers::observability::metrics::ensure_default_meter();
    let engine = Arc::new(Engine::new());

    // Same id, two namespaces, different metadata tiers.
    for (namespace, tier) in [(DEFAULT_NAMESPACE, "public"), ("analytics", "internal")] {
        engine.register_function_handler_ns(
            namespace,
            RegisterFunctionRequest {
                function_id: "reports::render".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: Some(json!({ "tier": tier })),
            },
            Handler::new(|_input| async move { FunctionResult::Success(Some(json!("ok"))) }),
        );
    }

    // Expose only functions whose metadata says `tier: public` — i.e. the
    // `default` copy, never the `analytics` one.
    let rbac = RbacConfig {
        auth_function_id: None,
        expose_functions: vec![FunctionFilter::metadata(HashMap::from([(
            "tier".to_string(),
            MetadataValue::Exact(json!("public")),
        )]))],
        on_trigger_registration_function_id: None,
        on_trigger_type_registration_function_id: None,
        on_function_registration_function_id: None,
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Outbound>(8);
    let mut worker = WorkerConnection::new(tx);
    worker.session = Some(Arc::new(Session {
        engine: engine.clone(),
        config: Arc::new(WorkerManagerConfig {
            port: 0,
            host: "127.0.0.1".to_string(),
            middleware_function_id: None,
            rbac: Some(rbac),
            handshake_timeout_ms: 10_000,
            registration_namespace_grace_ms: 5000,
        }),
        ip_address: "127.0.0.1".to_string(),
        session_id: uuid::Uuid::new_v4(),
        allowed_functions: vec![],
        forbidden_functions: vec![],
        allowed_trigger_types: None,
        allow_function_registration: true,
        allow_trigger_type_registration: true,
        context: json!({}),
        function_registration_prefix: None,
    }));

    let invoke = |namespace: Option<&str>| Message::InvokeFunction {
        invocation_id: Some(uuid::Uuid::new_v4()),
        function_id: "reports::render".to_string(),
        data: json!({}),
        traceparent: None,
        baggage: None,
        action: None,
        metadata: None,
        namespace: namespace.map(str::to_string),
    };

    async fn next_result(rx: &mut tokio::sync::mpsc::Receiver<Outbound>) -> Message {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("timed out waiting for InvocationResult")
            .expect("worker channel closed")
        {
            Outbound::Protocol(msg) => msg,
            other => panic!("expected a protocol message, got {other:?}"),
        }
    }

    // `default` carries `tier: public` → allowed.
    engine
        .router_msg(&worker, &invoke(None))
        .await
        .expect("router_msg");
    match next_result(&mut rx).await {
        Message::InvocationResult { error, .. } => {
            assert!(
                error.is_none(),
                "the `default` copy is `tier: public` and must be allowed; got {error:?}"
            );
        }
        other => panic!("expected InvocationResult, got {other:?}"),
    }

    // `analytics` carries `tier: internal` → forbidden. If the lookup resolved
    // in `default`, RBAC would read `tier: public` here and wrongly allow it.
    engine
        .router_msg(&worker, &invoke(Some("analytics")))
        .await
        .expect("router_msg");
    match next_result(&mut rx).await {
        Message::InvocationResult { error, .. } => {
            let error = error.expect(
                "the `analytics` copy is `tier: internal` and must be FORBIDDEN — \
                 RBAC read the wrong namespace's metadata",
            );
            assert_eq!(error.code, "FORBIDDEN");
        }
        other => panic!("expected InvocationResult, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// External (HTTP-invocation) registrations
//
// A `RegisterFunction` carrying `invocation` does not install a worker-routed
// handler; it installs an HTTP-calling one via `HttpFunctionsWorker`. That half
// used to write into `default` unconditionally while the *service* half already
// used the connection's namespace. Strict routing turns that split into a hard
// bug: a namespaced worker's HTTP function becomes unreachable from its own
// namespace. These tests pin both halves to the connection's namespace.
// ---------------------------------------------------------------------------

/// Boots the same stack as `spawn_engine`, plus the `http_functions` service
/// (permissive security, since the target is a local test server) and a local
/// echo server. Returns the ws port, the engine, and the echo URL.
async fn spawn_engine_with_http_functions() -> (u16, Arc<Engine>, String) {
    let (port, engine) = spawn_engine().await;

    let http_module = HttpFunctionsWorker::create(
        engine.clone(),
        Some(
            serde_json::to_value(HttpFunctionsConfig {
                security: SecurityConfig {
                    require_https: false,
                    block_private_ips: false,
                    url_allowlist: vec!["*".to_string()],
                },
            })
            .expect("serialize http functions config"),
        ),
    )
    .await
    .expect("create HttpFunctionsWorker");
    http_module
        .initialize()
        .await
        .expect("initialize HttpFunctionsWorker");

    let app = Router::new().route(
        "/echo",
        post(|body: Json<Value>| async move { Json(json!({ "echoed": body.0 })) }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind echo server");
    let addr = listener.local_addr().expect("echo local_addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (port, engine, format!("http://{addr}/echo"))
}

/// A namespaced worker's HTTP-invocation function must be reachable from that
/// worker's namespace — and must not be silently parked in `default`.
///
/// Neuter the namespace argument in `HttpFunctionsWorker::register_http_function`
/// (back to `DEFAULT_NAMESPACE`) and this test fails: the invoke in `billing`
/// misses and comes back `function_not_found`.
#[tokio::test]
async fn an_external_function_registers_in_the_workers_namespace() {
    let (port, engine, echo_url) = spawn_engine_with_http_functions().await;

    let mut ws = connect(port).await;
    ws.send(text(json!({
        "type": "registerfunction",
        "id": "billing::charge",
        "request_format": null,
        "response_format": null,
        "invocation": { "url": echo_url, "method": "POST" },
    })))
    .await
    .expect("send RegisterFunction with invocation");
    declare_worker(&mut ws, "billing-worker", Some("billing")).await;

    assert!(
        eventually(|| engine.functions.get("billing", "billing::charge").is_some()).await,
        "the external registration must land in the worker's namespace, not `default` \
         (in `default`: {})",
        engine
            .functions
            .get(DEFAULT_NAMESPACE, "billing::charge")
            .is_some()
    );
    assert!(
        engine
            .functions
            .get(DEFAULT_NAMESPACE, "billing::charge")
            .is_none(),
        "the external registration must not leak into `default`"
    );

    // And it must actually route: a caller naming `billing` reaches the HTTP
    // endpoint. This is what the registry assertion alone cannot prove.
    let mut caller = connect(port).await;
    caller
        .send(text(json!({
            "type": "invokefunction",
            "invocation_id": uuid::Uuid::new_v4(),
            "function_id": "billing::charge",
            "namespace": "billing",
            "data": { "amount": 42 },
        })))
        .await
        .expect("send InvokeFunction");
    // The echo server mirrors the payload back under `echoed`; the engine adds
    // `_caller_worker_id` to every invoke payload, so assert on the field we
    // sent rather than on the whole object.
    let result = wait_for_result(&mut caller, "billing::charge").await;
    assert_eq!(
        result["result"]["echoed"]["amount"],
        json!(42),
        "the invoke must reach the HTTP endpoint; got {result}"
    );

    let _ = ws.close(None).await;
    let _ = caller.close(None).await;
}

/// The removal half. `UnregisterFunction` resolves the connection's namespace
/// and must tear down the registration that lives there — before this task the
/// removal targeted the connection's namespace while the write had gone to
/// `default`, so it matched nothing and the function stayed alive forever.
#[tokio::test]
async fn unregistering_an_external_function_removes_it_from_the_workers_namespace() {
    let (port, engine, echo_url) = spawn_engine_with_http_functions().await;

    let mut ws = connect(port).await;
    ws.send(text(json!({
        "type": "registerfunction",
        "id": "billing::refund",
        "request_format": null,
        "response_format": null,
        "invocation": { "url": echo_url, "method": "POST" },
    })))
    .await
    .expect("send RegisterFunction with invocation");
    declare_worker(&mut ws, "billing-worker", Some("billing")).await;

    assert!(
        eventually(|| engine.functions.get("billing", "billing::refund").is_some()).await,
        "precondition: the external function must be registered in `billing`"
    );

    ws.send(text(json!({
        "type": "unregisterfunction",
        "id": "billing::refund",
    })))
    .await
    .expect("send UnregisterFunction");

    assert!(
        eventually(|| engine.functions.get("billing", "billing::refund").is_none()).await,
        "UnregisterFunction must actually remove the external registration"
    );

    let _ = ws.close(None).await;
}

/// The other removal path: a namespaced worker that simply drops its socket.
/// `cleanup_worker` resolves the connection's namespace and must tear the
/// external registration out of it — otherwise a disconnected worker's HTTP
/// function stays callable forever.
#[tokio::test]
async fn disconnecting_removes_an_external_function_from_the_workers_namespace() {
    let (port, engine, echo_url) = spawn_engine_with_http_functions().await;

    let mut ws = connect(port).await;
    ws.send(text(json!({
        "type": "registerfunction",
        "id": "billing::void",
        "request_format": null,
        "response_format": null,
        "invocation": { "url": echo_url, "method": "POST" },
    })))
    .await
    .expect("send RegisterFunction with invocation");
    declare_worker(&mut ws, "billing-worker", Some("billing")).await;

    assert!(
        eventually(|| engine.functions.get("billing", "billing::void").is_some()).await,
        "precondition: the external function must be registered in `billing`"
    );

    ws.close(None).await.expect("close the worker socket");

    assert!(
        eventually(|| engine.functions.get("billing", "billing::void").is_none()).await,
        "cleanup_worker must remove the external registration from the worker's namespace"
    );
}
