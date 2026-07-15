use serde_json::json;

use iii::{EngineBuilder, engine::EngineTrait};

/// An explicit `iii-queue` worker entry (what `iii worker add iii-queue`
/// writes into config.yaml) must boot the in-engine queue worker and register
/// the caller-facing queue/DLQ functions.
#[tokio::test]
async fn explicit_iii_queue_entry_registers_queue_functions() {
    let builder = EngineBuilder::new()
        .add_worker("iii-engine-functions", None)
        .add_worker("iii-observability", None)
        .add_worker("iii-queue", None)
        .build()
        .await
        .expect("engine build should succeed");
    let engine = builder.engine();

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
