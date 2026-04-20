mod common;

use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use serial_test::serial;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};

use iii::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    trigger::Trigger,
    workers::{queue::QueueWorker, rest_api::HttpWorker, state::StateWorker, traits::Worker},
};

use common::flow_helpers::{HttpQueueStatePathSpec, ensure_flow_test_tracing};
use common::queue_helpers::builtin_queue_config;

fn reserve_local_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("listener addr").port();
    drop(listener);
    port
}

async fn start_http_worker(engine: Arc<Engine>, port: u16) -> String {
    let module = HttpWorker::create(
        engine.clone(),
        Some(json!({
            "host": "127.0.0.1",
            "port": port,
            "default_timeout": 5000,
        })),
    )
    .await
    .expect("HttpWorker::create should succeed");

    module.register_functions(engine);
    module
        .initialize()
        .await
        .expect("HttpWorker::initialize should succeed");

    format!("http://127.0.0.1:{port}")
}

async fn wait_for_route(client: &reqwest::Client, url: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if client.get(url).send().await.is_ok() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }

    panic!("HTTP route did not become reachable before timeout: {url}");
}

#[tokio::test]
#[serial]
async fn validated_flow_helper_builds_graph_asset_from_real_http_queue_state_path() {
    ensure_flow_test_tracing();

    let engine = Arc::new(Engine::new());

    let queue_module = QueueWorker::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueWorker::create should succeed");
    queue_module.register_functions(engine.clone());
    queue_module
        .initialize()
        .await
        .expect("QueueWorker::initialize should succeed");

    let state_module = StateWorker::create(engine.clone(), None)
        .await
        .expect("StateWorker::create should succeed");
    state_module.register_functions(engine.clone());
    state_module
        .initialize()
        .await
        .expect("StateWorker::initialize should succeed");

    let port = reserve_local_port();
    let base_url = start_http_worker(engine.clone(), port).await;

    let queue_topic = "validated-flow.events";
    let state_scope = "validated-flow.orders";
    let expected_order_id = "order-123";

    let (state_event_tx, mut state_event_rx) = mpsc::unbounded_channel::<Value>();

    let engine_for_http = engine.clone();
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "flow_poc::http_entry".to_string(),
            description: Some("Entry function for validated flow POC".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(move |input: Value| {
            let engine = engine_for_http.clone();
            async move {
                let order_id = input
                    .get("body")
                    .and_then(|body| body.get("order_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("missing-order-id")
                    .to_string();

                match engine
                    .call(
                        "enqueue",
                        json!({
                            "topic": queue_topic,
                            "data": {
                                "order_id": order_id,
                                "status": "processed"
                            }
                        }),
                    )
                    .await
                {
                    Ok(_) => FunctionResult::Success(Some(json!({
                        "status_code": 202,
                        "body": {
                            "accepted": true,
                            "queue_topic": queue_topic
                        }
                    }))),
                    Err(err) => FunctionResult::Failure(err),
                }
            }
        }),
    );

    let engine_for_consumer = engine.clone();
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "flow_poc::queue_consumer".to_string(),
            description: Some("Queue consumer for validated flow POC".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(move |input: Value| {
            let engine = engine_for_consumer.clone();
            async move {
                let order_id = input
                    .get("order_id")
                    .and_then(Value::as_str)
                    .unwrap_or("missing-order-id")
                    .to_string();

                match engine
                    .call(
                        "state::set",
                        json!({
                            "scope": state_scope,
                            "key": order_id,
                            "value": {
                                "status": input
                                    .get("status")
                                    .and_then(Value::as_str)
                                    .unwrap_or("unknown")
                            }
                        }),
                    )
                    .await
                {
                    Ok(_) => FunctionResult::Success(Some(json!({ "ok": true }))),
                    Err(err) => FunctionResult::Failure(err),
                }
            }
        }),
    );

    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "flow_poc::state_observer".to_string(),
            description: Some("State observer for validated flow POC".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(move |input: Value| {
            let tx = state_event_tx.clone();
            async move {
                tx.send(input)
                    .expect("state event receiver should remain available");
                FunctionResult::Success(Some(json!({ "observed": true })))
            }
        }),
    );

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: "validated-flow-http".to_string(),
            trigger_type: "http".to_string(),
            function_id: "flow_poc::http_entry".to_string(),
            config: json!({
                "api_path": "/validated-flow",
                "http_method": "POST"
            }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("HTTP trigger should register");

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: "validated-flow-queue".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "flow_poc::queue_consumer".to_string(),
            config: json!({
                "topic": queue_topic,
                "queue_config": {
                    "max_retries": 2,
                    "backoff_ms": 50,
                    "poll_interval_ms": 25
                }
            }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("queue trigger should register");

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: "validated-flow-state".to_string(),
            trigger_type: "state".to_string(),
            function_id: "flow_poc::state_observer".to_string(),
            config: json!({
                "scope": state_scope,
            }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("state trigger should register");

    let client = reqwest::Client::new();
    let url = format!("{base_url}/validated-flow");
    wait_for_route(&client, &url).await;

    let response = client
        .post(&url)
        .json(&json!({ "order_id": expected_order_id }))
        .send()
        .await
        .expect("HTTP request should succeed");

    assert_eq!(response.status().as_u16(), 202);

    let response_body: Value = response
        .json()
        .await
        .expect("response should be valid JSON");
    assert_eq!(response_body["accepted"], true);
    assert_eq!(response_body["queue_topic"], queue_topic);

    let state_event = timeout(Duration::from_secs(5), state_event_rx.recv())
        .await
        .expect("timed out waiting for state observer")
        .expect("state observer channel should remain open");

    assert_eq!(state_event["scope"], state_scope);
    assert_eq!(state_event["key"], expected_order_id);
    assert_eq!(state_event["new_value"]["status"], "processed");

    let spec = HttpQueueStatePathSpec {
        id: "validated-http-queue-state".to_string(),
        name: "Validated HTTP Queue State Path".to_string(),
        method: "POST".to_string(),
        path: "/validated-flow".to_string(),
        entry_function_id: "flow_poc::http_entry".to_string(),
        queue_topic: queue_topic.to_string(),
        queue_consumer_function_id: "flow_poc::queue_consumer".to_string(),
        state_scope: state_scope.to_string(),
        state_key: expected_order_id.to_string(),
        state_observer_function_id: "flow_poc::state_observer".to_string(),
    };

    let asset = spec
        .wait_for_validated_asset(&state_event, Duration::from_secs(5))
        .await
        .expect("validated flow asset should be generated");

    assert_eq!(
        serde_json::to_value(&asset).expect("asset should serialize"),
        json!({
            "id": "validated-http-queue-state",
            "name": "Validated HTTP Queue State Path",
            "nodes": [
                {
                    "id": "http:POST:/validated-flow",
                    "kind": "http_trigger",
                    "label": "POST /validated-flow"
                },
                {
                    "id": "flow_poc::http_entry",
                    "kind": "function",
                    "label": "flow_poc::http_entry"
                },
                {
                    "id": "queue:validated-flow.events",
                    "kind": "queue_topic",
                    "label": "validated-flow.events"
                },
                {
                    "id": "flow_poc::queue_consumer",
                    "kind": "function",
                    "label": "flow_poc::queue_consumer"
                },
                {
                    "id": "state:validated-flow.orders:order-123",
                    "kind": "state_slot",
                    "label": "validated-flow.orders/order-123"
                },
                {
                    "id": "flow_poc::state_observer",
                    "kind": "function",
                    "label": "flow_poc::state_observer"
                }
            ],
            "edges": [
                {
                    "id": "http:POST:/validated-flow->flow_poc::http_entry",
                    "kind": "http_trigger",
                    "source": "http:POST:/validated-flow",
                    "target": "flow_poc::http_entry",
                    "label": "POST /validated-flow"
                },
                {
                    "id": "flow_poc::http_entry->queue:validated-flow.events",
                    "kind": "enqueue",
                    "source": "flow_poc::http_entry",
                    "target": "queue:validated-flow.events",
                    "label": "validated-flow.events"
                },
                {
                    "id": "queue:validated-flow.events->flow_poc::queue_consumer",
                    "kind": "queue_trigger",
                    "source": "queue:validated-flow.events",
                    "target": "flow_poc::queue_consumer",
                    "label": "validated-flow.events"
                },
                {
                    "id": "flow_poc::queue_consumer->state:validated-flow.orders:order-123",
                    "kind": "state_write",
                    "source": "flow_poc::queue_consumer",
                    "target": "state:validated-flow.orders:order-123",
                    "label": "validated-flow.orders/order-123"
                },
                {
                    "id": "state:validated-flow.orders:order-123->flow_poc::state_observer",
                    "kind": "state_trigger",
                    "source": "state:validated-flow.orders:order-123",
                    "target": "flow_poc::state_observer",
                    "label": "validated-flow.orders"
                }
            ]
        })
    );
}
