//! End-to-end proof that `engine::workers::list` and `engine::functions::list`
//! report the namespace each worker/function actually lives in.
//!
//! With namespaces, the same function id (`state::get`) can legitimately exist
//! twice. A listing that renders two identical rows is useless — the namespace
//! is the only thing that tells them apart.
//!
//! These tests deliberately do NOT hand-build a `WorkerConnection` and poke
//! `.namespace` on it: that field is a plain (non-shared) field on a `Clone`
//! type, so a doctored local proves nothing about production. The namespace
//! here arrives the way it really arrives — riding on the
//! `engine::workers::register` engine call over a real WebSocket — and the
//! listings are then read out of the real registries.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use iii::engine::Engine;
use iii::function::FunctionResult;
use iii::protocol::DEFAULT_NAMESPACE;
use iii::workers::engine_fn::EngineFunctionsWorker;
use iii::workers::traits::Worker;
use iii::workers::worker::rbac_config::{FunctionFilter, MetadataValue, RbacConfig};
use iii::workers::worker::rbac_session::Session;
use iii::workers::worker::{WorkerManager, WorkerManagerConfig};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

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

/// Connects a worker that registers `function_id` and then announces itself in
/// `namespace` via `engine::workers::register`. Returns the socket so the caller
/// keeps the connection (and therefore the registration) alive.
///
/// `pid` is sent because `engine::workers::list` only reports workers that
/// declare one.
async fn connect_worker(
    port: u16,
    name: &str,
    namespace: Option<&str>,
    function_id: &str,
    pid: u32,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    connect_worker_with_metadata(port, name, namespace, function_id, pid, None).await
}

/// As [`connect_worker`], but the `RegisterFunction` carries `metadata` — the
/// input an RBAC `Metadata` filter matches against.
async fn connect_worker_with_metadata(
    port: u16,
    name: &str,
    namespace: Option<&str>,
    function_id: &str,
    pid: u32,
    metadata: Option<Value>,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
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

    let mut register_function = json!({
        "type": "registerfunction",
        "id": function_id,
        "request_format": null,
        "response_format": null,
    });
    if let Some(metadata) = metadata {
        register_function["metadata"] = metadata;
    }
    ws.send(WsMessage::Text(register_function.to_string().into()))
        .await
        .expect("send RegisterFunction");

    let mut data = json!({ "runtime": "node", "name": name, "pid": pid });
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

    ws
}

/// Registers a trigger type and one trigger instance over an already-open,
/// namespace-resolved worker socket. The trigger's namespace is captured by the
/// engine from the connection at apply time (`connection_namespace`), so this is
/// the real path a trigger gets its namespace — not a doctored field. Returns
/// the trigger instance id.
///
/// The caller must have driven `engine::workers::register` and waited until the
/// worker's function landed, so the connection namespace is already resolved and
/// the `RegisterTrigger` applies immediately rather than buffering.
async fn register_trigger_over_ws(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    trigger_type: &str,
    function_id: &str,
    config: Value,
) -> String {
    ws.send(WsMessage::Text(
        json!({
            "type": "registertriggertype",
            "id": trigger_type,
            "description": "test trigger type",
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send RegisterTriggerType");

    let trigger_id = uuid::Uuid::new_v4().to_string();
    ws.send(WsMessage::Text(
        json!({
            "type": "registertrigger",
            "id": trigger_id,
            "trigger_type": trigger_type,
            "function_id": function_id,
            "config": config,
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send RegisterTrigger");

    trigger_id
}

/// Calls an engine introspection function in-process. The listing's *content*
/// still comes from the registries the real WebSocket path populated.
async fn call_engine_fn(engine: &Arc<Engine>, function_id: &str, input: Value) -> Value {
    match call_engine_fn_raw(engine, function_id, input, None).await {
        FunctionResult::Success(Some(value)) => value,
        _ => panic!("{function_id} should return a result"),
    }
}

async fn call_engine_fn_raw(
    engine: &Arc<Engine>,
    function_id: &str,
    input: Value,
    session: Option<Arc<Session>>,
) -> FunctionResult<Option<Value>, iii::protocol::ErrorBody> {
    let function = engine
        .functions
        .get(DEFAULT_NAMESPACE, function_id)
        .unwrap_or_else(|| panic!("{function_id} should be registered"));
    function.call_handler(None, input, session).await
}

/// A session whose RBAC allow-list is a single `Metadata` filter.
///
/// This is the shape that exposed the bug: `is_function_allowed` evaluates
/// `expose_functions` as an allow-list via `.any(|filter| filter.matches(id,
/// metadata))`, and a `Metadata` filter returns `false` outright when `metadata`
/// is `None`. So a caller that re-fetches the function from the wrong namespace
/// hands it `None` and the function is silently DENIED — fail-closed, but wrong.
fn session_exposing_metadata(engine: Arc<Engine>, key: &str, value: &str) -> Arc<Session> {
    let mut expected = HashMap::new();
    expected.insert(
        key.to_string(),
        MetadataValue::Exact(Value::String(value.to_string())),
    );

    let config = WorkerManagerConfig {
        rbac: Some(RbacConfig {
            auth_function_id: None,
            expose_functions: vec![FunctionFilter::Metadata(expected)],
            on_trigger_registration_function_id: None,
            on_trigger_type_registration_function_id: None,
            on_function_registration_function_id: None,
        }),
        ..Default::default()
    };

    Arc::new(Session {
        engine,
        config: Arc::new(config),
        ip_address: "127.0.0.1".to_string(),
        session_id: uuid::Uuid::new_v4(),
        allowed_functions: vec![],
        forbidden_functions: vec![],
        allowed_trigger_types: None,
        allow_function_registration: true,
        allow_trigger_type_registration: true,
        context: json!({}),
        function_registration_prefix: None,
    })
}

/// The headline case: two workers, same function id, different namespaces.
/// Both rows must appear in `engine::functions::list` and be told apart by
/// `namespace`.
#[tokio::test]
async fn functions_list_reports_the_namespace_each_function_is_registered_in() {
    let (port, engine) = spawn_engine().await;

    let _orders = connect_worker(port, "orders-worker", Some("orders"), "state::get", 4001).await;
    let _billing =
        connect_worker(port, "billing-worker", Some("billing"), "state::get", 4002).await;

    let landed = eventually(|| {
        engine.functions.get("orders", "state::get").is_some()
            && engine.functions.get("billing", "state::get").is_some()
    })
    .await;
    assert!(landed, "both workers' functions must register");

    let result = call_engine_fn(&engine, "engine::functions::list", json!({})).await;
    let functions = result["functions"].as_array().expect("functions array");

    let mut namespaces: Vec<&str> = functions
        .iter()
        .filter(|f| f["function_id"] == "state::get")
        .map(|f| {
            f["namespace"]
                .as_str()
                .expect("every function row must carry a namespace")
        })
        .collect();
    namespaces.sort();

    assert_eq!(
        namespaces,
        vec!["billing", "orders"],
        "the two `state::get` rows must be distinguishable by namespace; got listing: {result}"
    );
}

/// Wire compatibility: a function registered by a worker that declared no
/// namespace is reported as `default`, not omitted or blank.
#[tokio::test]
async fn functions_list_reports_default_namespace_for_undeclared_workers() {
    let (port, engine) = spawn_engine().await;

    let _legacy = connect_worker(port, "legacy-worker", None, "legacy::ping", 4003).await;

    let landed = eventually(|| {
        engine
            .functions
            .get(DEFAULT_NAMESPACE, "legacy::ping")
            .is_some()
    })
    .await;
    assert!(landed, "the legacy worker's function must register");

    let result = call_engine_fn(&engine, "engine::functions::list", json!({})).await;
    let row = result["functions"]
        .as_array()
        .expect("functions array")
        .iter()
        .find(|f| f["function_id"] == "legacy::ping")
        .expect("legacy::ping must be listed");

    assert_eq!(row["namespace"], json!(DEFAULT_NAMESPACE));
}

/// `engine::workers::list` must report each worker's declared namespace, read
/// from the registry entry `engine::workers::register` actually wrote to.
#[tokio::test]
async fn workers_list_reports_the_namespace_each_worker_declared() {
    let (port, engine) = spawn_engine().await;

    let _orders = connect_worker(
        port,
        "orders-worker",
        Some("orders"),
        "orders::create",
        4101,
    )
    .await;
    let _billing = connect_worker(
        port,
        "billing-worker",
        Some("billing"),
        "billing::charge",
        4102,
    )
    .await;
    let _legacy = connect_worker(port, "legacy-worker", None, "legacy::ping", 4103).await;

    // The metadata write and the function registration are driven by the same
    // `engine::workers::register` call, so the functions landing means the
    // namespace reached the registry.
    let landed = eventually(|| {
        engine.functions.get("orders", "orders::create").is_some()
            && engine.functions.get("billing", "billing::charge").is_some()
            && engine
                .functions
                .get(DEFAULT_NAMESPACE, "legacy::ping")
                .is_some()
    })
    .await;
    assert!(landed, "all three workers must finish registering");

    let result = call_engine_fn(&engine, "engine::workers::list", json!({})).await;
    let workers = result["workers"].as_array().expect("workers array");

    let ns_of = |name: &str| -> String {
        workers
            .iter()
            .find(|w| w["name"] == name)
            .unwrap_or_else(|| panic!("{name} must be listed; got: {result}"))["namespace"]
            .as_str()
            .expect("every worker row must carry a namespace")
            .to_string()
    };

    assert_eq!(ns_of("orders-worker"), "orders");
    assert_eq!(ns_of("billing-worker"), "billing");
    assert_eq!(
        ns_of("legacy-worker"),
        DEFAULT_NAMESPACE,
        "a worker that declared no namespace is reported as `default`"
    );
}

// ── The Task-2 re-key fallout ───────────────────────────────────────────────
//
// The four tests below cover lookups that were correct by construction while
// the engine had exactly one namespace, and that Task 2's `(ns, id)` re-key
// silently turned into default-only lookups. They are regressions of this
// plan, not pre-existing bugs: each one only misbehaves for a function that
// lives outside `default`, which was not expressible before Task 2.

/// `engine::workers::info` denormalizes a worker's functions by looking each id
/// up in the registry. Looking them up in `default` finds nothing for a
/// namespaced worker, so the worker is reported as having ZERO functions — the
/// exact opposite of this task's point.
#[tokio::test]
async fn workers_info_lists_the_functions_of_a_namespaced_worker() {
    let (port, engine) = spawn_engine().await;

    let _orders = connect_worker(
        port,
        "orders-worker",
        Some("orders"),
        "orders::create",
        4201,
    )
    .await;

    let landed = eventually(|| engine.functions.get("orders", "orders::create").is_some()).await;
    assert!(landed, "the worker's function must register");

    let result = call_engine_fn(
        &engine,
        "engine::workers::info",
        json!({ "name": "orders-worker" }),
    )
    .await;

    let functions = result["functions"].as_array().expect("functions array");
    let row = functions
        .iter()
        .find(|f| f["function_id"] == "orders::create")
        .unwrap_or_else(|| {
            panic!(
                "a namespaced worker must not be reported as having zero functions; got: {result}"
            )
        });
    assert_eq!(row["namespace"], json!("orders"));
}

/// `engine::functions::info` resolves the id in `default` only, so a function
/// that lives solely in `orders` reports NOT_FOUND — an engine whose every
/// worker is namespaced is blind to its own contracts.
#[tokio::test]
async fn functions_info_finds_a_function_registered_only_in_a_non_default_namespace() {
    let (port, engine) = spawn_engine().await;

    let _orders = connect_worker(
        port,
        "orders-worker",
        Some("orders"),
        "orders::create",
        4202,
    )
    .await;

    let landed = eventually(|| engine.functions.get("orders", "orders::create").is_some()).await;
    assert!(landed, "the worker's function must register");

    let result = call_engine_fn(
        &engine,
        "engine::functions::info",
        json!({ "function_id": "orders::create" }),
    )
    .await;

    assert_eq!(result["function_id"], json!("orders::create"));
    assert_eq!(
        result["namespace"],
        json!("orders"),
        "the detail must say which namespace it describes; got: {result}"
    );
}

/// RBAC re-fetch, listing path (`engine::functions::list`).
///
/// The session's allow-list is a single `Metadata` filter. `is_function_allowed`
/// only sees metadata if the caller re-fetches the right registry entry; a
/// `default` re-fetch of a namespaced function yields `None`, the filter cannot
/// match, and the function is wrongly hidden.
///
/// This test sets a real session — Task 5's RBAC lookup stayed green when
/// neutered precisely because no test did.
#[tokio::test]
async fn functions_list_rbac_allows_a_namespaced_function_matched_by_metadata_filter() {
    let (port, engine) = spawn_engine().await;

    let _orders = connect_worker_with_metadata(
        port,
        "orders-worker",
        Some("orders"),
        "orders::create",
        4203,
        Some(json!({ "scope": "public" })),
    )
    .await;

    let landed = eventually(|| engine.functions.get("orders", "orders::create").is_some()).await;
    assert!(landed, "the worker's function must register");

    let session = session_exposing_metadata(engine.clone(), "scope", "public");
    let result = match call_engine_fn_raw(
        &engine,
        "engine::functions::list",
        json!({}),
        Some(session),
    )
    .await
    {
        FunctionResult::Success(Some(value)) => value,
        _ => panic!("functions::list should return a result"),
    };

    let listed = result["functions"]
        .as_array()
        .expect("functions array")
        .iter()
        .any(|f| f["function_id"] == "orders::create");
    assert!(
        listed,
        "a namespaced function whose metadata matches the session's `expose_functions` \
         filter must be visible; got: {result}"
    );
}

/// RBAC re-fetch, detail path (`engine::functions::info`). Same bug as the
/// listing path: the namespaced function is wrongly rejected as FORBIDDEN.
#[tokio::test]
async fn functions_info_rbac_allows_a_namespaced_function_matched_by_metadata_filter() {
    let (port, engine) = spawn_engine().await;

    let _orders = connect_worker_with_metadata(
        port,
        "orders-worker",
        Some("orders"),
        "orders::create",
        4204,
        Some(json!({ "scope": "public" })),
    )
    .await;

    let landed = eventually(|| engine.functions.get("orders", "orders::create").is_some()).await;
    assert!(landed, "the worker's function must register");

    let session = session_exposing_metadata(engine.clone(), "scope", "public");
    match call_engine_fn_raw(
        &engine,
        "engine::functions::info",
        json!({ "function_id": "orders::create" }),
        Some(session),
    )
    .await
    {
        FunctionResult::Success(Some(value)) => {
            assert_eq!(value["function_id"], json!("orders::create"));
            assert_eq!(value["namespace"], json!("orders"));
        }
        FunctionResult::Failure(err) => panic!(
            "a namespaced function matching the session's metadata filter must not be \
             {}: {}",
            err.code, err.message
        ),
        _ => panic!("functions::info should return a result"),
    }
}

/// Concern 3: `worker_name` must name the worker that actually owns each row.
///
/// The owner index was keyed by bare function id, so with `state::get` in two
/// namespaces both rows were attributed to whichever worker the index happened
/// to reach first — a WRONG answer, not a missing one, and nondeterministic
/// (DashMap iteration order).
#[tokio::test]
async fn functions_list_attributes_each_namespace_row_to_its_own_worker() {
    let (port, engine) = spawn_engine().await;

    let _orders = connect_worker(port, "orders-worker", Some("orders"), "state::get", 4205).await;
    let _billing =
        connect_worker(port, "billing-worker", Some("billing"), "state::get", 4206).await;

    let landed = eventually(|| {
        engine.functions.get("orders", "state::get").is_some()
            && engine.functions.get("billing", "state::get").is_some()
    })
    .await;
    assert!(landed, "both workers' functions must register");

    let result = call_engine_fn(&engine, "engine::functions::list", json!({})).await;
    let functions = result["functions"].as_array().expect("functions array");

    let owner_of = |namespace: &str| -> String {
        functions
            .iter()
            .find(|f| f["function_id"] == "state::get" && f["namespace"] == namespace)
            .unwrap_or_else(|| panic!("the {namespace} row must exist; got: {result}"))
            ["worker_name"]
            .as_str()
            .expect("worker_name")
            .to_string()
    };

    assert_eq!(owner_of("orders"), "orders-worker");
    assert_eq!(owner_of("billing"), "billing-worker");
}

/// The one case `engine::functions::info` genuinely cannot answer: a bare id
/// registered in several non-default namespaces at once. There is no namespace
/// input on the wire, so picking one would be a guess. It resolves nothing and
/// names the candidates instead — the same shape as `Engine::resolve_function`'s
/// hint — rather than claiming the function is not registered, which would be a
/// lie.
#[tokio::test]
async fn functions_info_reports_an_ambiguous_bare_id_with_its_candidate_namespaces() {
    let (port, engine) = spawn_engine().await;

    let _orders = connect_worker(port, "orders-worker", Some("orders"), "state::get", 4207).await;
    let _billing =
        connect_worker(port, "billing-worker", Some("billing"), "state::get", 4208).await;

    let landed = eventually(|| {
        engine.functions.get("orders", "state::get").is_some()
            && engine.functions.get("billing", "state::get").is_some()
    })
    .await;
    assert!(landed, "both workers' functions must register");

    match call_engine_fn_raw(
        &engine,
        "engine::functions::info",
        json!({ "function_id": "state::get" }),
        None,
    )
    .await
    {
        FunctionResult::Failure(err) => {
            assert_eq!(err.code, "NOT_FOUND");
            assert!(
                err.message.contains("billing") && err.message.contains("orders"),
                "the error must name both candidate namespaces, not claim the function \
                 is unregistered; got: {}",
                err.message
            );
        }
        _ => panic!("an ambiguous bare id must not resolve to an arbitrary namespace"),
    }
}

/// BUG 2: an id duplicated across non-default namespaces (none in `default`) is
/// unaddressable without a namespace input. With an explicit `namespace`,
/// `engine::functions::info` resolves strictly in that namespace — the same
/// strict semantics as `Engine::resolve_function`'s explicit-ns path.
#[tokio::test]
async fn functions_info_addresses_an_ambiguous_id_with_an_explicit_namespace() {
    let (port, engine) = spawn_engine().await;

    let _orders = connect_worker(port, "orders-worker", Some("orders"), "state::get", 4209).await;
    let _billing =
        connect_worker(port, "billing-worker", Some("billing"), "state::get", 4210).await;

    let landed = eventually(|| {
        engine.functions.get("orders", "state::get").is_some()
            && engine.functions.get("billing", "state::get").is_some()
    })
    .await;
    assert!(landed, "both workers' functions must register");

    // Explicit `orders` resolves the `orders` copy, not `billing`, not NOT_FOUND.
    let result = call_engine_fn(
        &engine,
        "engine::functions::info",
        json!({ "function_id": "state::get", "namespace": "orders" }),
    )
    .await;
    assert_eq!(result["function_id"], json!("state::get"));
    assert_eq!(
        result["namespace"],
        json!("orders"),
        "an explicit namespace must pin the detail to that namespace; got: {result}"
    );

    // The sibling namespace is equally addressable.
    let billing = call_engine_fn(
        &engine,
        "engine::functions::info",
        json!({ "function_id": "state::get", "namespace": "billing" }),
    )
    .await;
    assert_eq!(billing["namespace"], json!("billing"));
}

/// An explicit `namespace` that the id does not live in is a miss, reported as
/// NOT_FOUND that names where the id actually exists — never a resolution in
/// some other namespace.
#[tokio::test]
async fn functions_info_explicit_namespace_miss_reports_not_found_naming_where_it_exists() {
    let (port, engine) = spawn_engine().await;

    let _orders = connect_worker(port, "orders-worker", Some("orders"), "state::get", 4211).await;

    let landed = eventually(|| engine.functions.get("orders", "state::get").is_some()).await;
    assert!(landed, "the worker's function must register");

    match call_engine_fn_raw(
        &engine,
        "engine::functions::info",
        json!({ "function_id": "state::get", "namespace": "billing" }),
        None,
    )
    .await
    {
        FunctionResult::Failure(err) => {
            assert_eq!(err.code, "NOT_FOUND");
            assert!(
                err.message.contains("orders"),
                "a miss in the requested namespace must name where the id does exist; got: {}",
                err.message
            );
        }
        _ => panic!("an explicit-namespace miss must not resolve elsewhere"),
    }
}

// ── BUG: `engine::workers::info` name is ambiguous across namespaces ─────────
//
// A worker name is unique only *within* a namespace, so once two workers named
// `state` register into `orders` and `analytics`, `engine::workers::info { name:
// "state" }` cannot tell them apart. It previously returned the first worker it
// found (registry iteration order) — a wrong, nondeterministic answer.

/// An explicit `namespace` pins `engine::workers::info` to the worker of that
/// name in that namespace. The two same-named workers are distinguished by the
/// functions they own.
#[tokio::test]
async fn workers_info_disambiguates_two_same_named_workers_by_namespace() {
    let (port, engine) = spawn_engine().await;

    // Same worker NAME, different namespaces, different owned functions.
    let _orders = connect_worker(port, "state", Some("orders"), "orders::create", 4301).await;
    let _analytics =
        connect_worker(port, "state", Some("analytics"), "analytics::track", 4302).await;

    let landed = eventually(|| {
        engine.functions.get("orders", "orders::create").is_some()
            && engine
                .functions
                .get("analytics", "analytics::track")
                .is_some()
    })
    .await;
    assert!(landed, "both same-named workers must finish registering");

    // Explicit `orders` resolves the orders worker: it owns `orders::create`,
    // never `analytics::track`.
    let result = call_engine_fn(
        &engine,
        "engine::workers::info",
        json!({ "name": "state", "namespace": "orders" }),
    )
    .await;
    let fn_ids: Vec<&str> = result["functions"]
        .as_array()
        .expect("functions array")
        .iter()
        .map(|f| f["function_id"].as_str().expect("function_id"))
        .collect();
    assert!(
        fn_ids.contains(&"orders::create"),
        "the orders worker must own orders::create; got: {result}"
    );
    assert!(
        !fn_ids.contains(&"analytics::track"),
        "the orders worker must NOT list the analytics worker's function; got: {result}"
    );
}

/// A bare `name` that is ambiguous across two non-default namespaces resolves
/// nothing and names the candidate namespaces — never silently picks one.
#[tokio::test]
async fn workers_info_bare_ambiguous_name_reports_candidate_namespaces() {
    let (port, engine) = spawn_engine().await;

    let _orders = connect_worker(port, "state", Some("orders"), "orders::create", 4303).await;
    let _analytics =
        connect_worker(port, "state", Some("analytics"), "analytics::track", 4304).await;

    let landed = eventually(|| {
        engine.functions.get("orders", "orders::create").is_some()
            && engine
                .functions
                .get("analytics", "analytics::track")
                .is_some()
    })
    .await;
    assert!(landed, "both same-named workers must finish registering");

    match call_engine_fn_raw(
        &engine,
        "engine::workers::info",
        json!({ "name": "state" }),
        None,
    )
    .await
    {
        FunctionResult::Failure(err) => {
            assert_eq!(err.code, "NOT_FOUND");
            assert!(
                err.message.contains("orders") && err.message.contains("analytics"),
                "the error must name both candidate namespaces; got: {}",
                err.message
            );
        }
        _ => panic!("an ambiguous bare worker name must not resolve to an arbitrary worker"),
    }
}

// ── BUG: trigger association ignored namespace ──────────────────────────────
//
// A function detail listed its triggers by bare `function_id`, so the same id in
// two namespaces spliced each namespace's triggers onto the other's detail.

/// `svc::f` exists in both `orders` and `analytics`, each with its own trigger.
/// The `orders` function detail must list ONLY the orders trigger.
#[tokio::test]
async fn functions_info_lists_only_the_triggers_in_its_own_namespace() {
    let (port, engine) = spawn_engine().await;

    let mut orders = connect_worker(port, "orders-worker", Some("orders"), "svc::f", 4401).await;
    let mut analytics =
        connect_worker(port, "analytics-worker", Some("analytics"), "svc::f", 4402).await;

    // Wait for both `svc::f` registrations to land in their namespaces, which
    // also proves each connection's namespace is resolved before we bind
    // triggers (so the RegisterTrigger applies with the right namespace).
    let landed = eventually(|| {
        engine.functions.get("orders", "svc::f").is_some()
            && engine.functions.get("analytics", "svc::f").is_some()
    })
    .await;
    assert!(landed, "both svc::f registrations must land");

    let _orders_trig = register_trigger_over_ws(
        &mut orders,
        "orders-tt",
        "svc::f",
        json!({ "tag": "orders" }),
    )
    .await;
    let _analytics_trig = register_trigger_over_ws(
        &mut analytics,
        "analytics-tt",
        "svc::f",
        json!({ "tag": "analytics" }),
    )
    .await;

    // Both triggers must land, each stamped with its connection's namespace.
    let bound = eventually(|| {
        let triggers = &engine.trigger_registry.triggers;
        triggers
            .iter()
            .any(|e| e.value().namespace == "orders" && e.value().function_id == "svc::f")
            && triggers
                .iter()
                .any(|e| e.value().namespace == "analytics" && e.value().function_id == "svc::f")
    })
    .await;
    assert!(bound, "both triggers must register in their own namespace");

    let result = call_engine_fn(
        &engine,
        "engine::functions::info",
        json!({ "function_id": "svc::f", "namespace": "orders" }),
    )
    .await;

    assert_eq!(result["namespace"], json!("orders"));
    let trigger_types: Vec<&str> = result["registered_triggers"]
        .as_array()
        .expect("registered_triggers array")
        .iter()
        .map(|t| t["trigger_type"].as_str().expect("trigger_type"))
        .collect();
    assert_eq!(
        trigger_types,
        vec!["orders-tt"],
        "the orders detail must list ONLY the orders trigger, not the analytics \
         one bound to the same id in another namespace; got: {result}"
    );
}
