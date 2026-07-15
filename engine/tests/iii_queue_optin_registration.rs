mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iii::{EngineBuilder, engine::EngineTrait, trigger::Trigger};

use common::queue_helpers::{enqueue_to_topic, register_payload_capturing_function};

async fn boot_optin_iii_queue() -> Arc<iii::engine::Engine> {
    let builder = EngineBuilder::new()
        .add_worker("iii-engine-functions", None)
        .add_worker("iii-observability", None)
        .add_worker("iii-queue", None)
        .build()
        .await
        .expect("engine build should succeed");
    builder.engine().clone()
}

/// An explicit `iii-queue` worker entry (what `iii worker add iii-queue`
/// writes into config.yaml) must boot the in-engine queue worker and register
/// the caller-facing queue/DLQ functions.
#[tokio::test]
async fn explicit_iii_queue_entry_registers_queue_functions() {
    let engine = boot_optin_iii_queue().await;

    let result = engine
        .call("engine::queue::list_topics", json!({}))
        .await
        .expect("engine::queue::list_topics should be registered and callable")
        .expect("list_topics should return a value");
    assert!(result.is_array(), "unexpected shape: {result:?}");

    let dlq = engine
        .call("engine::queue::dlq_topics", json!({}))
        .await
        .expect("engine::queue::dlq_topics should be registered and callable")
        .expect("dlq_topics should return a value");
    assert!(dlq.is_array(), "unexpected shape: {dlq:?}");
}

/// The opt-in builtin registers `durable:subscriber` in-process, so discovery
/// must attribute the trigger type to `iii-queue` — not to the standalone
/// `queue` worker that the static provider table names for install guidance.
/// This is what the release publish pipeline reads (`engine::workers::info`)
/// when building the iii-queue registry payload.
#[tokio::test]
async fn explicit_iii_queue_entry_attributes_durable_subscriber_to_itself() {
    let engine = boot_optin_iii_queue().await;

    let triggers = engine
        .call("engine::triggers::list", json!({}))
        .await
        .expect("engine::triggers::list should be callable")
        .expect("triggers::list should return a value");
    let durable = triggers["triggers"]
        .as_array()
        .expect("triggers should be an array")
        .iter()
        .find(|t| t["id"] == "durable:subscriber")
        .unwrap_or_else(|| panic!("durable:subscriber not registered: {triggers:?}"))
        .clone();
    assert_eq!(
        durable["worker_name"], "iii-queue",
        "durable:subscriber must attribute to the opt-in builtin"
    );

    let info = engine
        .call("engine::workers::info", json!({"name": "iii-queue"}))
        .await
        .expect("engine::workers::info should be callable")
        .expect("workers::info should return a value");
    let type_ids: Vec<&str> = info["trigger_types"]
        .as_array()
        .expect("trigger_types should be an array")
        .iter()
        .filter_map(|t| t["id"].as_str())
        .collect();
    assert!(
        type_ids.contains(&"durable:subscriber"),
        "workers::info for iii-queue must roll up durable:subscriber, got {type_ids:?}"
    );
}

/// Queue behavior must work as it did pre-retirement when the builtin is
/// explicitly configured: a `durable:subscriber` trigger binds and messages
/// published to its topic are delivered to the bound function.
#[tokio::test]
async fn explicit_iii_queue_entry_delivers_durable_subscriber_messages() {
    let engine = boot_optin_iii_queue().await;

    let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    register_payload_capturing_function(&engine, "test::optin_handler", captured.clone());

    let topic = format!("optin-e2e-{}", uuid::Uuid::new_v4());
    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: format!("trig-{}", uuid::Uuid::new_v4()),
            trigger_type: "durable:subscriber".to_string(),
            function_id: "test::optin_handler".to_string(),
            config: json!({
                "topic": &topic,
                "queue_config": { "poll_interval_ms": 50 }
            }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("durable:subscriber trigger should register against the opt-in builtin");

    enqueue_to_topic(&engine, &topic, json!({"msg": "hello from iii-queue"}))
        .await
        .expect("publish to topic should succeed");

    // Poll instead of a fixed sleep: consumer startup is asynchronous.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if !captured.lock().await.is_empty() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "message was not delivered to the durable:subscriber handler within 5s"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let payloads = captured.lock().await;
    assert_eq!(payloads.len(), 1, "expected exactly one delivery");
    assert_eq!(payloads[0]["msg"], "hello from iii-queue");
}
