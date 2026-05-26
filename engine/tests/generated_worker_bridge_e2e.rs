use std::sync::{Arc, Mutex};

use axum::{Json, Router, body::Bytes, extract::State, http::StatusCode, routing::post};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use uuid::Uuid;

use iii::{
    engine::{Engine, EngineTrait, Outbound},
    invocation::method::HttpMethod,
    protocol::{HttpInvocationRef, Message},
    worker_connections::WorkerConnection,
    workers::{
        engine_fn::EngineFunctionsWorker,
        http_functions::{HttpFunctionsWorker, config::HttpFunctionsConfig},
        observability::metrics::ensure_default_meter,
        traits::Worker,
        worker::{WorkerManagerConfig, rbac_session::Session},
    },
};

#[derive(Clone, Default)]
struct RequestCapture {
    body: Arc<Mutex<Option<Value>>>,
}

async fn spawn_success_server(capture: RequestCapture) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr = listener.local_addr().expect("resolve local addr");

    async fn success_handler(
        State(capture): State<RequestCapture>,
        body: Bytes,
    ) -> (StatusCode, Json<Value>) {
        *capture.body.lock().expect("lock body") = Some(if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).expect("request body should be valid json")
        });
        (StatusCode::OK, Json(json!({ "ok": true })))
    }

    let app = Router::new()
        .route("/success", post(success_handler))
        .with_state(capture);
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test app");
    });

    format!("http://{addr}")
}

async fn engine_with_discovery_and_http_functions() -> Arc<Engine> {
    ensure_default_meter();
    let engine = Arc::new(Engine::new());

    let engine_functions = EngineFunctionsWorker::create(engine.clone(), None)
        .await
        .expect("create engine functions worker");
    engine_functions
        .initialize()
        .await
        .expect("initialize engine functions worker");
    engine_functions.register_functions(engine.clone());

    let http_functions = HttpFunctionsWorker::create(
        engine.clone(),
        Some(serde_json::to_value(HttpFunctionsConfig::default()).expect("serialize config")),
    )
    .await
    .expect("create http functions worker");
    http_functions
        .initialize()
        .await
        .expect("initialize http functions worker");

    engine
}

fn worker_with_session(engine: Arc<Engine>, ip_address: &str) -> WorkerConnection {
    let (tx, _rx) = mpsc::channel::<Outbound>(8);
    WorkerConnection::with_session(
        tx,
        Session {
            engine,
            config: Arc::new(WorkerManagerConfig::default()),
            ip_address: ip_address.to_string(),
            session_id: Uuid::new_v4(),
            allowed_functions: vec![],
            forbidden_functions: vec![],
            allowed_trigger_types: None,
            allow_function_registration: true,
            allow_trigger_type_registration: true,
            trusted_internal: false,
            context: json!({}),
            function_registration_prefix: None,
        },
    )
}

fn register_generated_http_function(url: String) -> Message {
    Message::RegisterFunction {
        id: "generated_docs::search".to_string(),
        description: Some("Search generated docs".to_string()),
        request_format: None,
        response_format: None,
        metadata: Some(json!({
            "spec": {
                "source": "http://127.0.0.1/openapi.json",
                "sourceType": "openapi",
                "workerName": "generated-docs-worker"
            },
            "iii": {
                "generatedWorker": {
                    "name": "generated-docs-worker"
                }
            }
        })),
        invocation: Some(HttpInvocationRef {
            url,
            method: HttpMethod::Post,
            timeout_ms: Some(30_000),
            headers: Default::default(),
            auth: None,
        }),
    }
}

fn workers_array(value: &Value) -> &[Value] {
    value
        .get("workers")
        .and_then(Value::as_array)
        .expect("workers array")
}

fn functions_array(value: &Value) -> &[Value] {
    value
        .get("functions")
        .and_then(Value::as_array)
        .expect("functions array")
}

#[tokio::test]
async fn generated_bridge_registers_triggerable_function_and_normal_worker_group() {
    let engine = engine_with_discovery_and_http_functions().await;
    let capture = RequestCapture::default();
    let base_url = spawn_success_server(capture.clone()).await;
    let worker = worker_with_session(engine.clone(), "127.0.0.1");
    let register_message = register_generated_http_function(format!("{base_url}/success"));

    engine
        .router_msg(&worker, &register_message)
        .await
        .expect("register generated function");

    let result = engine
        .call(
            "generated_docs::search",
            json!({
                "query": "rust"
            }),
        )
        .await
        .expect("generated function call succeeds")
        .expect("generated function returns a result");

    assert_eq!(result, json!({ "ok": true }));
    assert_eq!(
        *capture.body.lock().expect("lock body"),
        Some(json!({ "query": "rust" }))
    );

    let workers = engine
        .call("engine::workers::list", json!({}))
        .await
        .expect("workers list succeeds")
        .expect("workers list result");
    let generated_worker = workers_array(&workers)
        .iter()
        .find(|worker| worker.get("name").and_then(Value::as_str) == Some("generated-docs-worker"))
        .expect("generated worker group is listed");

    assert_eq!(
        generated_worker.get("id"),
        Some(&json!("generated-docs-worker"))
    );
    assert_eq!(generated_worker.get("runtime"), Some(&json!("engine")));
    assert_eq!(generated_worker.get("function_count"), Some(&json!(1)));
    assert!(generated_worker.get("internal").is_none());
    assert!(generated_worker.get("functions").is_none());
    assert!(generated_worker.get("generated_worker").is_none());
    assert!(generated_worker.get("generatedWorker").is_none());
    assert!(generated_worker.get("virtual_worker").is_none());
    assert!(generated_worker.get("virtualWorker").is_none());

    let worker_info = engine
        .call(
            "engine::workers::info",
            json!({
                "name": "generated-docs-worker"
            }),
        )
        .await
        .expect("workers info succeeds")
        .expect("workers info result");
    let worker_detail = worker_info.get("worker").expect("worker detail is present");
    assert_eq!(
        worker_detail.get("id"),
        Some(&json!("generated-docs-worker"))
    );
    assert_eq!(worker_detail.get("runtime"), Some(&json!("engine")));
    assert_eq!(worker_detail.get("internal"), Some(&json!(false)));
    assert_eq!(worker_detail.get("function_count"), Some(&json!(1)));
    assert!(
        worker_info
            .get("functions")
            .and_then(Value::as_array)
            .expect("worker functions")
            .iter()
            .any(
                |function| function.get("function_id").and_then(Value::as_str)
                    == Some("generated_docs::search")
            )
    );

    let function_list = engine
        .call(
            "engine::functions::list",
            json!({
                "include_internal": true
            }),
        )
        .await
        .expect("functions list succeeds")
        .expect("functions list result");
    let generated_function = functions_array(&function_list)
        .iter()
        .find(|function| {
            function.get("function_id").and_then(Value::as_str) == Some("generated_docs::search")
        })
        .expect("generated function is listed");
    assert!(generated_function.get("metadata").is_none());

    let generated_function = engine
        .call(
            "engine::functions::info",
            json!({
                "function_id": "generated_docs::search"
            }),
        )
        .await
        .expect("functions info succeeds")
        .expect("functions info result");
    assert_eq!(
        generated_function.get("worker_name"),
        Some(&json!("generated-docs-worker"))
    );
    let metadata = generated_function
        .get("metadata")
        .expect("metadata is present");

    assert_eq!(
        metadata.pointer("/spec/workerName"),
        Some(&json!("generated-docs-worker"))
    );
    assert!(metadata.get("iii").is_none());

    engine
        .router_msg(
            &worker,
            &Message::UnregisterFunction {
                id: "generated_docs::search".to_string(),
            },
        )
        .await
        .expect("unregister generated function");

    assert!(engine.functions.get("generated_docs::search").is_none());

    let workers = engine
        .call("engine::workers::list", json!({}))
        .await
        .expect("workers list succeeds")
        .expect("workers list result");
    assert!(
        workers_array(&workers)
            .iter()
            .all(|worker| worker.get("name").and_then(Value::as_str)
                != Some("generated-docs-worker"))
    );

    let worker_info = engine
        .call(
            "engine::workers::info",
            json!({
                "name": "generated-docs-worker"
            }),
        )
        .await;
    assert!(worker_info.is_err());
}

#[tokio::test]
async fn remote_worker_cannot_register_loopback_generated_bridge() {
    let engine = engine_with_discovery_and_http_functions().await;
    let worker = worker_with_session(engine.clone(), "203.0.113.10");
    let register_message =
        register_generated_http_function("http://127.0.0.1/generated/search".to_string());

    engine
        .router_msg(&worker, &register_message)
        .await
        .expect("registration failure is contained");

    assert!(engine.functions.get("generated_docs::search").is_none());
    let http_functions = engine
        .service_registry
        .get_service::<HttpFunctionsWorker>("http_functions")
        .expect("http functions service");
    assert!(
        !http_functions
            .http_functions()
            .contains_key("generated_docs::search")
    );

    let workers = engine
        .call("engine::workers::list", json!({}))
        .await
        .expect("workers list succeeds")
        .expect("workers list result");

    assert!(
        workers_array(&workers)
            .iter()
            .all(|worker| worker.get("name").and_then(Value::as_str)
                != Some("generated-docs-worker"))
    );
}
