mod common;

use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::time::sleep;
use uuid::Uuid;

use iii::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::{module::Module, rest_api::RestApiCoreModule},
    trigger::Trigger,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Reserves an ephemeral port by binding and immediately releasing a listener.
/// This has a small TOCTOU race window but is acceptable for tests.
fn reserve_local_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("listener addr").port();
    drop(listener);
    port
}

/// Creates an Engine and a started `RestApiCoreModule` bound to `port`.
/// Returns `(engine, base_url)`.
async fn start_api_server(port: u16) -> (Arc<Engine>, String) {
    iii::modules::observability::metrics::ensure_default_meter();
    let engine = Arc::new(Engine::new());

    let module = RestApiCoreModule::create(
        engine.clone(),
        Some(json!({
            "host": "127.0.0.1",
            "port": port,
            "default_timeout": 5000,
        })),
    )
    .await
    .expect("RestApiCoreModule::create should succeed");

    module.initialize().await.expect("initialize should succeed");

    let base_url = format!("http://127.0.0.1:{port}");
    (engine, base_url)
}

/// Creates an Engine and a started `RestApiCoreModule` bound to `port` with
/// global middleware configured at the RestApi level.
async fn start_api_server_with_global_middleware(
    port: u16,
    middleware_function_id: &str,
) -> (Arc<Engine>, String) {
    iii::modules::observability::metrics::ensure_default_meter();
    let engine = Arc::new(Engine::new());

    let module = RestApiCoreModule::create(
        engine.clone(),
        Some(json!({
            "host": "127.0.0.1",
            "port": port,
            "default_timeout": 5000,
            "middleware": [
                {
                    "function_id": middleware_function_id,
                    "phase": "preHandler",
                    "priority": 0
                }
            ]
        })),
    )
    .await
    .expect("RestApiCoreModule::create should succeed");

    module.initialize().await.expect("initialize should succeed");

    let base_url = format!("http://127.0.0.1:{port}");
    (engine, base_url)
}

/// Registers an HTTP trigger with per-route `middleware_function_ids`.
async fn register_http_trigger(
    engine: &Arc<Engine>,
    trigger_id: &str,
    function_id: &str,
    path: &str,
    method: &str,
    middleware_function_ids: Vec<&str>,
) {
    let middleware_ids: Vec<Value> = middleware_function_ids
        .iter()
        .map(|id| json!(id))
        .collect();

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: trigger_id.to_string(),
            trigger_type: "http".to_string(),
            function_id: function_id.to_string(),
            config: json!({
                "api_path": path,
                "http_method": method,
                "middleware_function_ids": middleware_ids,
            }),
            worker_id: None,
        })
        .await
        .expect("register_trigger should succeed");
}

/// Waits until the given route starts responding (up to 3 seconds).
async fn wait_for_route(client: &reqwest::Client, url: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if client.get(url).send().await.is_ok() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
}

// ---------------------------------------------------------------------------
// Test 1: per-route middleware returns `continue` — handler runs normally
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_per_route_middleware_continue() {
    let port = reserve_local_port();
    let (engine, base_url) = start_api_server(port).await;

    // Middleware function that always continues.
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "mw_e2e::continue".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| async move {
            FunctionResult::Success(Some(json!({ "action": "continue" })))
        }),
    );

    // Handler function that returns 200.
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "handler_e2e::hello".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| async move {
            FunctionResult::Success(Some(json!({
                "status_code": 200,
                "body": { "message": "hello from handler" }
            })))
        }),
    );

    register_http_trigger(
        &engine,
        &format!("trig-{}", Uuid::new_v4()),
        "handler_e2e::hello",
        "/mw-continue",
        "GET",
        vec!["mw_e2e::continue"],
    )
    .await;

    let client = reqwest::Client::new();
    let url = format!("{base_url}/mw-continue");
    wait_for_route(&client, &url).await;

    let response = client
        .get(&url)
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(
        response.status().as_u16(),
        200,
        "middleware returned continue — handler should produce 200"
    );

    let body: Value = response.json().await.expect("body should be valid JSON");
    assert_eq!(
        body.get("message").and_then(|v| v.as_str()),
        Some("hello from handler"),
        "handler response body should be returned"
    );
}

// ---------------------------------------------------------------------------
// Test 2: per-route middleware short-circuits with `respond` — handler skipped
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_per_route_middleware_short_circuit() {
    let port = reserve_local_port();
    let (engine, base_url) = start_api_server(port).await;

    // Middleware function that rejects with 403.
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "mw_e2e::forbidden".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| async move {
            FunctionResult::Success(Some(json!({
                "action": "respond",
                "response": {
                    "status_code": 403,
                    "body": { "error": "Forbidden" }
                }
            })))
        }),
    );

    // Handler function — should never be invoked.
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "handler_e2e::secret".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| async move {
            FunctionResult::Success(Some(json!({
                "status_code": 200,
                "body": { "secret": "you should not see this" }
            })))
        }),
    );

    register_http_trigger(
        &engine,
        &format!("trig-{}", Uuid::new_v4()),
        "handler_e2e::secret",
        "/mw-forbidden",
        "GET",
        vec!["mw_e2e::forbidden"],
    )
    .await;

    let client = reqwest::Client::new();
    let url = format!("{base_url}/mw-forbidden");
    wait_for_route(&client, &url).await;

    let response = client
        .get(&url)
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(
        response.status().as_u16(),
        403,
        "middleware short-circuit should return 403"
    );

    let body: Value = response.json().await.expect("body should be valid JSON");
    assert_eq!(
        body.get("error").and_then(|v| v.as_str()),
        Some("Forbidden"),
        "middleware response body should be returned"
    );
}

// ---------------------------------------------------------------------------
// Test 3: multiple middleware — ordering preserved, second one short-circuits
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_multiple_middleware_ordering() {
    let port = reserve_local_port();
    let (engine, base_url) = start_api_server(port).await;

    // mw_0: always continues.
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "mw_e2e::mw_0_continue".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| async move {
            FunctionResult::Success(Some(json!({ "action": "continue" })))
        }),
    );

    // mw_1: responds with 429.
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "mw_e2e::mw_1_rate_limit".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| async move {
            FunctionResult::Success(Some(json!({
                "action": "respond",
                "response": {
                    "status_code": 429,
                    "body": { "error": "Too Many Requests" }
                }
            })))
        }),
    );

    // Handler — should never run because mw_1 short-circuits.
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "handler_e2e::guarded".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| async move {
            FunctionResult::Success(Some(json!({
                "status_code": 200,
                "body": { "ok": true }
            })))
        }),
    );

    register_http_trigger(
        &engine,
        &format!("trig-{}", Uuid::new_v4()),
        "handler_e2e::guarded",
        "/mw-ordered",
        "GET",
        vec!["mw_e2e::mw_0_continue", "mw_e2e::mw_1_rate_limit"],
    )
    .await;

    let client = reqwest::Client::new();
    let url = format!("{base_url}/mw-ordered");
    wait_for_route(&client, &url).await;

    let response = client
        .get(&url)
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(
        response.status().as_u16(),
        429,
        "second middleware should short-circuit after first continues"
    );

    let body: Value = response.json().await.expect("body should be valid JSON");
    assert_eq!(
        body.get("error").and_then(|v| v.as_str()),
        Some("Too Many Requests"),
    );
}

// ---------------------------------------------------------------------------
// Test 4: no middleware — handler runs directly (regression test)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_no_middleware_handler_runs_directly() {
    let port = reserve_local_port();
    let (engine, base_url) = start_api_server(port).await;

    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "handler_e2e::direct".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| async move {
            FunctionResult::Success(Some(json!({
                "status_code": 200,
                "body": { "result": "direct" }
            })))
        }),
    );

    register_http_trigger(
        &engine,
        &format!("trig-{}", Uuid::new_v4()),
        "handler_e2e::direct",
        "/no-middleware",
        "GET",
        vec![],
    )
    .await;

    let client = reqwest::Client::new();
    let url = format!("{base_url}/no-middleware");
    wait_for_route(&client, &url).await;

    let response = client
        .get(&url)
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(
        response.status().as_u16(),
        200,
        "handler without middleware should return 200"
    );

    let body: Value = response.json().await.expect("body should be valid JSON");
    assert_eq!(
        body.get("result").and_then(|v| v.as_str()),
        Some("direct"),
        "handler response body should be returned unchanged"
    );
}

// ---------------------------------------------------------------------------
// Test 5: global middleware from config runs before per-route middleware
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_global_middleware_from_config() {
    let port = reserve_local_port();

    // Global middleware that short-circuits all requests with 401.
    let (engine, base_url) =
        start_api_server_with_global_middleware(port, "mw_e2e::global_auth").await;

    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "mw_e2e::global_auth".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| async move {
            FunctionResult::Success(Some(json!({
                "action": "respond",
                "response": {
                    "status_code": 401,
                    "body": { "error": "Unauthorized" }
                }
            })))
        }),
    );

    // Per-route middleware that would continue (should never be reached).
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "mw_e2e::route_continue".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| async move {
            FunctionResult::Success(Some(json!({ "action": "continue" })))
        }),
    );

    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "handler_e2e::protected".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|_input: Value| async move {
            FunctionResult::Success(Some(json!({
                "status_code": 200,
                "body": { "data": "protected resource" }
            })))
        }),
    );

    register_http_trigger(
        &engine,
        &format!("trig-{}", Uuid::new_v4()),
        "handler_e2e::protected",
        "/global-mw",
        "GET",
        vec!["mw_e2e::route_continue"],
    )
    .await;

    let client = reqwest::Client::new();
    let url = format!("{base_url}/global-mw");
    wait_for_route(&client, &url).await;

    let response = client
        .get(&url)
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(
        response.status().as_u16(),
        401,
        "global middleware should run first and short-circuit before per-route middleware"
    );

    let body: Value = response.json().await.expect("body should be valid JSON");
    assert_eq!(
        body.get("error").and_then(|v| v.as_str()),
        Some("Unauthorized"),
        "global middleware response body should be returned"
    );
}
