mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iii::{
    engine::Engine,
    modules::{module::Module, queue::QueueCoreModule},
    trigger::Trigger,
};

use common::queue_helpers::{
    builtin_queue_config, enqueue, enqueue_to_topic, register_condition_function,
    register_payload_capturing_function,
};

// ---------------------------------------------------------------------------
// QBLT-01: Enqueue -> Process -> Ack preserves payload integrity
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enqueue_process_ack_preserves_payload() {
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let captured_payloads: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    register_payload_capturing_function(&engine, "test::payload_check", captured_payloads.clone());

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");
    module.initialize().await.expect("init should succeed");

    let sent_payload = json!({
        "order_id": 42,
        "items": [{"sku": "ABC-123", "qty": 3}],
        "metadata": {"source": "test", "timestamp": 1234567890},
        "tags": ["urgent", "vip"],
        "nested": {"a": {"b": {"c": true}}}
    });

    enqueue(
        &engine,
        "default",
        "test::payload_check",
        sent_payload.clone(),
    )
    .await
    .expect("enqueue should succeed");

    tokio::time::sleep(Duration::from_millis(500)).await;

    let received = captured_payloads.lock().await;
    assert_eq!(
        received.len(),
        1,
        "handler should have been invoked exactly once"
    );
    assert_eq!(
        received[0], sent_payload,
        "received payload must match sent payload structurally"
    );
}

// ---------------------------------------------------------------------------
// QBLT-07: Condition-based filtering routes only matching messages
// ---------------------------------------------------------------------------

#[tokio::test]
async fn condition_based_filtering_routes_matching_messages_only() {
    let engine = {
        iii::modules::observability::metrics::ensure_default_meter();
        Arc::new(Engine::new())
    };

    let captured_payloads: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));

    // Register handler that captures payloads (gated by condition)
    register_payload_capturing_function(
        &engine,
        "test::filtered_handler",
        captured_payloads.clone(),
    );

    // Register condition function: only passes when category == "important"
    register_condition_function(
        &engine,
        "test::importance_condition",
        "category",
        json!("important"),
    );

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");
    // register_functions must be called to make the "enqueue" service function
    // available on the engine (topic-based enqueue path uses engine.call("enqueue", ...))
    module.register_functions(engine.clone());
    module.initialize().await.expect("init should succeed");

    let topic = format!("cond-test-{}", uuid::Uuid::new_v4());

    // Register trigger WITH condition_function_id AFTER initialize
    // (the "queue" trigger type is registered during initialize)
    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: format!("trig-{}", uuid::Uuid::new_v4()),
            trigger_type: "queue".to_string(),
            function_id: "test::filtered_handler".to_string(),
            config: json!({
                "topic": &topic,
                "condition_function_id": "test::importance_condition",
                "queue_config": {
                    "max_retries": 2,
                    "backoff_ms": 100,
                    "poll_interval_ms": 50
                }
            }),
            worker_id: None,
        })
        .await
        .expect("register conditional trigger should succeed");

    // Enqueue 3 messages via topic-based path: 2 non-matching, 1 matching
    enqueue_to_topic(
        &engine,
        &topic,
        json!({"category": "spam", "msg": "ignore me 1"}),
    )
    .await
    .expect("enqueue spam 1 should succeed");

    enqueue_to_topic(
        &engine,
        &topic,
        json!({"category": "important", "msg": "process me"}),
    )
    .await
    .expect("enqueue important should succeed");

    enqueue_to_topic(
        &engine,
        &topic,
        json!({"category": "spam", "msg": "ignore me 2"}),
    )
    .await
    .expect("enqueue spam 2 should succeed");

    // Wait for all messages to be processed
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // The conditional handler should only be called for the "important" message.
    // The 2 "spam" messages are silently acked by the condition check (returning false).
    let payloads = captured_payloads.lock().await;
    assert_eq!(
        payloads.len(),
        1,
        "conditional handler should receive exactly 1 message (the 'important' one), got {}",
        payloads.len()
    );
    assert_eq!(
        payloads[0]["category"], "important",
        "the received message should be the 'important' one"
    );
    assert_eq!(
        payloads[0]["msg"], "process me",
        "the received message payload should match what was sent"
    );
}
