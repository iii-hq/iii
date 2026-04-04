mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iii::{
    engine::Engine,
    function::{Function, FunctionResult},
    modules::{module::Module, queue::QueueCoreModule},
    trigger::Trigger,
};

use common::queue_helpers::{builtin_queue_config, enqueue_to_topic};

fn register_capturing_function(engine: &Arc<Engine>, function_id: &str) -> Arc<Mutex<Vec<Value>>> {
    let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let cap = captured.clone();
    let function = Function {
        handler: Arc::new(move |_invocation_id, input, _session| {
            let rec = cap.clone();
            Box::pin(async move {
                rec.lock().await.push(input);
                FunctionResult::Success(Some(json!({ "ok": true })))
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("capturing test handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
    captured
}

fn register_counting_fn(engine: &Arc<Engine>, function_id: &str) -> Arc<AtomicU64> {
    let counter = Arc::new(AtomicU64::new(0));
    let cnt = counter.clone();
    let function = Function {
        handler: Arc::new(move |_invocation_id, _input, _session| {
            let c = cnt.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                FunctionResult::Success(Some(json!({ "ok": true })))
            })
        }),
        _function_id: function_id.to_string(),
        _description: Some("counting test handler".to_string()),
        request_format: None,
        response_format: None,
        metadata: None,
    };
    engine
        .functions
        .register_function(function_id.to_string(), function);
    counter
}

async fn setup_engine_with_topic_triggers(function_ids: &[&str], topic: &str) -> Arc<Engine> {
    iii::modules::observability::metrics::ensure_default_meter();
    let engine = Arc::new(Engine::new());

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");
    module.register_functions(engine.clone());
    module.initialize().await.expect("init should succeed");

    for fid in function_ids {
        engine
            .trigger_registry
            .register_trigger(Trigger {
                id: format!("trig-{}", uuid::Uuid::new_v4()),
                trigger_type: "queue".to_string(),
                function_id: fid.to_string(),
                config: json!({ "topic": topic }),
                worker_id: None,
                metadata: None,
            })
            .await
            .expect("register trigger should succeed");
    }

    engine
}

#[tokio::test]
async fn fanout_two_functions_same_topic_both_receive() {
    let topic = format!("fanout-{}", uuid::Uuid::new_v4());
    let engine = setup_engine_with_topic_triggers(&["test::fn_a", "test::fn_b"], &topic).await;

    let cap_a = register_capturing_function(&engine, "test::fn_a");
    let cap_b = register_capturing_function(&engine, "test::fn_b");

    enqueue_to_topic(&engine, &topic, json!({"msg": "hello"}))
        .await
        .expect("enqueue should succeed");

    tokio::time::sleep(Duration::from_secs(2)).await;

    let a = cap_a.lock().await;
    let b = cap_b.lock().await;
    assert_eq!(
        a.len(),
        1,
        "fn_a should receive exactly 1 message, got {}",
        a.len()
    );
    assert_eq!(
        b.len(),
        1,
        "fn_b should receive exactly 1 message, got {}",
        b.len()
    );
    assert_eq!(a[0]["msg"], "hello");
    assert_eq!(b[0]["msg"], "hello");
}

#[tokio::test]
async fn fanout_replicas_compete_within_function() {
    let topic = format!("replica-{}", uuid::Uuid::new_v4());
    iii::modules::observability::metrics::ensure_default_meter();
    let engine = Arc::new(Engine::new());

    let counter = register_counting_fn(&engine, "test::replica_fn");

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");
    module.register_functions(engine.clone());
    module.initialize().await.expect("init should succeed");

    for _ in 0..2 {
        engine
            .trigger_registry
            .register_trigger(Trigger {
                id: format!("trig-{}", uuid::Uuid::new_v4()),
                trigger_type: "queue".to_string(),
                function_id: "test::replica_fn".to_string(),
                config: json!({ "topic": &topic }),
                worker_id: None,
                metadata: None,
            })
            .await
            .expect("register trigger should succeed");
    }

    enqueue_to_topic(&engine, &topic, json!({"msg": "single"}))
        .await
        .expect("enqueue should succeed");

    tokio::time::sleep(Duration::from_secs(2)).await;

    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "only one replica should process the message"
    );
}

#[tokio::test]
async fn fanout_mixed_functions_and_replicas() {
    let topic = format!("mixed-{}", uuid::Uuid::new_v4());
    iii::modules::observability::metrics::ensure_default_meter();
    let engine = Arc::new(Engine::new());

    let counter_a = register_counting_fn(&engine, "test::mix_a");
    let counter_b = register_counting_fn(&engine, "test::mix_b");

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");
    module.register_functions(engine.clone());
    module.initialize().await.expect("init should succeed");

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: format!("trig-{}", uuid::Uuid::new_v4()),
            trigger_type: "queue".to_string(),
            function_id: "test::mix_a".to_string(),
            config: json!({ "topic": &topic }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("register trigger should succeed");

    for _ in 0..2 {
        engine
            .trigger_registry
            .register_trigger(Trigger {
                id: format!("trig-{}", uuid::Uuid::new_v4()),
                trigger_type: "queue".to_string(),
                function_id: "test::mix_b".to_string(),
                config: json!({ "topic": &topic }),
                worker_id: None,
                metadata: None,
            })
            .await
            .expect("register trigger should succeed");
    }

    for i in 0..5 {
        enqueue_to_topic(&engine, &topic, json!({"idx": i}))
            .await
            .expect("enqueue should succeed");
    }

    tokio::time::sleep(Duration::from_secs(3)).await;

    assert_eq!(
        counter_a.load(Ordering::SeqCst),
        5,
        "fn_a should receive all 5 messages"
    );
    assert_eq!(
        counter_b.load(Ordering::SeqCst),
        5,
        "fn_b should receive all 5 messages total across replicas"
    );
}

#[tokio::test]
async fn fanout_single_subscriber_unchanged() {
    let topic = format!("single-{}", uuid::Uuid::new_v4());
    let engine = setup_engine_with_topic_triggers(&["test::solo"], &topic).await;

    let cap = register_capturing_function(&engine, "test::solo");

    for i in 0..3 {
        enqueue_to_topic(&engine, &topic, json!({"n": i}))
            .await
            .expect("enqueue should succeed");
    }

    tokio::time::sleep(Duration::from_secs(2)).await;

    let received = cap.lock().await;
    assert_eq!(
        received.len(),
        3,
        "single subscriber should receive all 3 messages"
    );
}

#[tokio::test]
async fn fanout_unsubscribe_stops_delivery() {
    let topic = format!("unsub-{}", uuid::Uuid::new_v4());
    iii::modules::observability::metrics::ensure_default_meter();
    let engine = Arc::new(Engine::new());

    let cap_a = register_capturing_function(&engine, "test::unsub_a");
    let cap_b = register_capturing_function(&engine, "test::unsub_b");

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");
    module.register_functions(engine.clone());
    module.initialize().await.expect("init should succeed");

    let trigger_a_id = format!("trig-{}", uuid::Uuid::new_v4());
    let trigger_b_id = format!("trig-{}", uuid::Uuid::new_v4());

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: trigger_a_id.clone(),
            trigger_type: "queue".to_string(),
            function_id: "test::unsub_a".to_string(),
            config: json!({ "topic": &topic }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("register trigger A should succeed");

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: trigger_b_id.clone(),
            trigger_type: "queue".to_string(),
            function_id: "test::unsub_b".to_string(),
            config: json!({ "topic": &topic }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("register trigger B should succeed");

    enqueue_to_topic(&engine, &topic, json!({"phase": "before"}))
        .await
        .expect("enqueue should succeed");

    tokio::time::sleep(Duration::from_secs(2)).await;

    assert_eq!(cap_a.lock().await.len(), 1);
    assert_eq!(cap_b.lock().await.len(), 1);

    engine
        .trigger_registry
        .unregister_trigger(trigger_b_id, Some("queue".to_string()))
        .await
        .expect("unregister trigger B should succeed");

    tokio::time::sleep(Duration::from_millis(500)).await;

    enqueue_to_topic(&engine, &topic, json!({"phase": "after"}))
        .await
        .expect("enqueue should succeed");

    tokio::time::sleep(Duration::from_secs(2)).await;

    assert_eq!(
        cap_a.lock().await.len(),
        2,
        "fn_a should receive both messages"
    );
    assert_eq!(
        cap_b.lock().await.len(),
        1,
        "fn_b should only have the message from before unsubscribe"
    );
}

#[tokio::test]
async fn fanout_payload_integrity() {
    let topic = format!("payload-{}", uuid::Uuid::new_v4());
    let engine =
        setup_engine_with_topic_triggers(&["test::integrity_a", "test::integrity_b"], &topic).await;

    let cap_a = register_capturing_function(&engine, "test::integrity_a");
    let cap_b = register_capturing_function(&engine, "test::integrity_b");

    let complex_payload = json!({
        "order_id": 42,
        "items": [{"sku": "ABC", "qty": 3}],
        "nested": {"a": {"b": {"c": true}}},
        "tags": ["urgent", "vip"]
    });

    enqueue_to_topic(&engine, &topic, complex_payload.clone())
        .await
        .expect("enqueue should succeed");

    tokio::time::sleep(Duration::from_secs(2)).await;

    let a = cap_a.lock().await;
    let b = cap_b.lock().await;
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_eq!(a[0], complex_payload, "fn_a should receive exact payload");
    assert_eq!(b[0], complex_payload, "fn_b should receive exact payload");
}

#[tokio::test]
async fn fanout_with_condition_function() {
    let topic = format!("cond-fanout-{}", uuid::Uuid::new_v4());
    iii::modules::observability::metrics::ensure_default_meter();
    let engine = Arc::new(Engine::new());

    let cap_always = register_capturing_function(&engine, "test::cond_always");
    let cap_filtered = register_capturing_function(&engine, "test::cond_filtered");

    common::queue_helpers::register_condition_function(
        &engine,
        "test::only_important",
        "category",
        json!("important"),
    );

    let module = QueueCoreModule::create(engine.clone(), Some(builtin_queue_config()))
        .await
        .expect("QueueCoreModule::create should succeed");
    module.register_functions(engine.clone());
    module.initialize().await.expect("init should succeed");

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: format!("trig-{}", uuid::Uuid::new_v4()),
            trigger_type: "queue".to_string(),
            function_id: "test::cond_always".to_string(),
            config: json!({ "topic": &topic }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("register always trigger should succeed");

    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: format!("trig-{}", uuid::Uuid::new_v4()),
            trigger_type: "queue".to_string(),
            function_id: "test::cond_filtered".to_string(),
            config: json!({
                "topic": &topic,
                "condition_function_id": "test::only_important"
            }),
            worker_id: None,
            metadata: None,
        })
        .await
        .expect("register conditional trigger should succeed");

    enqueue_to_topic(
        &engine,
        &topic,
        json!({"category": "spam", "msg": "ignore"}),
    )
    .await
    .expect("enqueue spam should succeed");

    enqueue_to_topic(
        &engine,
        &topic,
        json!({"category": "important", "msg": "process"}),
    )
    .await
    .expect("enqueue important should succeed");

    tokio::time::sleep(Duration::from_secs(3)).await;

    let always = cap_always.lock().await;
    let filtered = cap_filtered.lock().await;

    assert_eq!(
        always.len(),
        2,
        "unconditional handler should receive both messages"
    );
    assert_eq!(
        filtered.len(),
        1,
        "conditional handler should receive only the important message"
    );
    assert_eq!(filtered[0]["category"], "important");
}
