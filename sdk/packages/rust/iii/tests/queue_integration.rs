//! Integration tests for the queue system via SDK.
//!
//! Requires a running III engine with queue module configured.
//! Set III_URL or use ws://localhost:49134 default.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iii_sdk::{
    IIIError, RegisterFunctionMessage, RegisterTriggerInput, TriggerAction, TriggerRequest,
};

fn unique_topic(prefix: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{prefix}-{ts}")
}

#[tokio::test]
async fn enqueue_returns_acknowledgement() {
    let iii = common::shared_iii();

    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    iii.register_function((
        RegisterFunctionMessage::with_id("test::queue::echo::rs".to_string()),
        move |input: Value| {
            let received = received_clone.clone();
            async move {
                received.lock().await.push(input.clone());
                Ok(json!({ "processed": true }))
            }
        },
    ));
    common::settle().await;

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test::queue::echo::rs".to_string(),
            payload: json!({"msg": "hello"}),
            action: Some(TriggerAction::Enqueue {
                queue: "default".to_string(),
            }),
            timeout_ms: None,
        })
        .await
        .expect("enqueue should succeed");

    assert!(
        result["messageReceiptId"].is_string(),
        "enqueue should return a messageReceiptId"
    );

    tokio::time::sleep(Duration::from_secs(2)).await;

    let msgs = received.lock().await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["msg"], "hello");
}

#[tokio::test]
async fn enqueue_to_unknown_queue_returns_error() {
    let iii = common::shared_iii();

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test::queue::unknown::rs".to_string(),
            payload: json!({"msg": "hello"}),
            action: Some(TriggerAction::Enqueue {
                queue: "nonexistent_queue".to_string(),
            }),
            timeout_ms: None,
        })
        .await;

    match result {
        Err(IIIError::Remote { code, message, .. }) => {
            assert_eq!(
                code, "enqueue_error",
                "expected enqueue_error code, got: {code}"
            );
            assert!(!message.is_empty(), "error message should not be empty");
        }
        Err(other) => panic!("expected IIIError::Remote with enqueue_error code, got: {other:?}"),
        Ok(val) => panic!("expected error, got success: {val}"),
    }
}

#[tokio::test]
async fn enqueue_fifo_with_valid_group_field() {
    let iii = common::shared_iii();

    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    iii.register_function((
        RegisterFunctionMessage::with_id("test::queue::fifo::rs".to_string()),
        move |input: Value| {
            let received = received_clone.clone();
            async move {
                received.lock().await.push(input.clone());
                Ok(json!({ "processed": true }))
            }
        },
    ));
    common::settle().await;

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test::queue::fifo::rs".to_string(),
            payload: json!({
                "transaction_id": "txn-001",
                "amount": 99.99
            }),
            action: Some(TriggerAction::Enqueue {
                queue: "payment".to_string(),
            }),
            timeout_ms: None,
        })
        .await
        .expect("enqueue to fifo should succeed");

    assert!(
        result["messageReceiptId"].is_string(),
        "enqueue should return a messageReceiptId"
    );

    tokio::time::sleep(Duration::from_secs(2)).await;

    let msgs = received.lock().await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["transaction_id"], "txn-001");
    assert_eq!(msgs[0]["amount"], 99.99);
}

#[tokio::test]
async fn enqueue_fifo_missing_group_field_returns_error() {
    let iii = common::shared_iii();

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test::queue::fifo::nofield::rs".to_string(),
            payload: json!({
                "amount": 50.00
            }),
            action: Some(TriggerAction::Enqueue {
                queue: "payment".to_string(),
            }),
            timeout_ms: None,
        })
        .await;

    match result {
        Err(IIIError::Remote { code, message, .. }) => {
            assert_eq!(
                code, "enqueue_error",
                "expected enqueue_error code, got: {code}"
            );
            assert!(
                message.contains("transaction_id"),
                "error message should mention the missing field 'transaction_id', got: {message}"
            );
        }
        Err(other) => panic!("expected IIIError::Remote with enqueue_error code, got: {other:?}"),
        Ok(val) => panic!("expected error for missing group field, got success: {val}"),
    }
}

#[tokio::test]
async fn void_returns_null_immediately() {
    let iii = common::shared_iii();

    let call_count = Arc::new(Mutex::new(0u32));
    let count_clone = call_count.clone();
    iii.register_function((
        RegisterFunctionMessage::with_id("test::queue::void::rs".to_string()),
        move |_input: Value| {
            let count = count_clone.clone();
            async move {
                *count.lock().await += 1;
                Ok(json!({ "done": true }))
            }
        },
    ));
    common::settle().await;

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test::queue::void::rs".to_string(),
            payload: json!({"fire": "forget"}),
            action: Some(TriggerAction::Void),
            timeout_ms: None,
        })
        .await
        .expect("void should succeed");

    assert_eq!(result, Value::Null, "void should return null immediately");

    tokio::time::sleep(Duration::from_secs(2)).await;

    let count = *call_count.lock().await;
    assert_eq!(count, 1, "function should have been called exactly once");
}

#[tokio::test]
async fn enqueue_multiple_messages_all_processed() {
    let iii = common::shared_iii();

    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    iii.register_function((
        RegisterFunctionMessage::with_id("test::queue::multi::rs".to_string()),
        move |input: Value| {
            let received = received_clone.clone();
            async move {
                received.lock().await.push(input.clone());
                Ok(json!({ "processed": true }))
            }
        },
    ));
    common::settle().await;

    let message_count = 5;
    for i in 0..message_count {
        let result = iii
            .trigger(TriggerRequest {
                function_id: "test::queue::multi::rs".to_string(),
                payload: json!({ "index": i }),
                action: Some(TriggerAction::Enqueue {
                    queue: "default".to_string(),
                }),
                timeout_ms: None,
            })
            .await
            .unwrap_or_else(|_| panic!("enqueue message {i} should succeed"));

        assert!(
            result["messageReceiptId"].is_string(),
            "enqueue should return a messageReceiptId"
        );
    }

    tokio::time::sleep(Duration::from_secs(3)).await;

    let msgs = received.lock().await;
    assert_eq!(
        msgs.len(),
        message_count,
        "all {message_count} messages should be processed, got {}",
        msgs.len()
    );

    let mut indices: Vec<i64> = msgs.iter().filter_map(|m| m["index"].as_i64()).collect();
    indices.sort();
    let expected: Vec<i64> = (0..message_count as i64).collect();
    assert_eq!(indices, expected, "all message indices should be present");
}

#[tokio::test]
async fn chained_enqueue() {
    let iii = common::shared_iii();

    let b_received = Arc::new(Mutex::new(Vec::new()));
    let b_received_clone = b_received.clone();
    iii.register_function((
        RegisterFunctionMessage::with_id("test::queue::chain::b::rs".to_string()),
        move |input: Value| {
            let b_received = b_received_clone.clone();
            async move {
                b_received.lock().await.push(input.clone());
                Ok(json!({ "step": "b_done" }))
            }
        },
    ));

    let a_received = Arc::new(Mutex::new(Vec::new()));
    let a_received_clone = a_received.clone();
    let iii_for_a = iii.clone();
    iii.register_function((
        RegisterFunctionMessage::with_id("test::queue::chain::a::rs".to_string()),
        move |input: Value| {
            let a_received = a_received_clone.clone();
            let iii = iii_for_a.clone();
            async move {
                a_received.lock().await.push(input.clone());

                let label = input["label"].as_str().unwrap_or("unknown").to_string();
                iii.trigger(TriggerRequest {
                    function_id: "test::queue::chain::b::rs".to_string(),
                    payload: json!({ "from_a": true, "label": label }),
                    action: Some(TriggerAction::Enqueue {
                        queue: "default".to_string(),
                    }),
                    timeout_ms: None,
                })
                .await
                .map_err(|e| IIIError::Handler(e.to_string()))?;

                Ok(json!({ "step": "a_done" }))
            }
        },
    ));
    common::settle().await;

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test::queue::chain::a::rs".to_string(),
            payload: json!({ "label": "chained-work" }),
            action: Some(TriggerAction::Enqueue {
                queue: "default".to_string(),
            }),
            timeout_ms: None,
        })
        .await
        .expect("enqueue to chain A should succeed");

    assert!(
        result["messageReceiptId"].is_string(),
        "enqueue should return a messageReceiptId"
    );

    tokio::time::sleep(Duration::from_secs(4)).await;

    let a_msgs = a_received.lock().await;
    assert_eq!(a_msgs.len(), 1, "function A should have been called once");
    assert_eq!(a_msgs[0]["label"], "chained-work");

    let b_msgs = b_received.lock().await;
    assert_eq!(b_msgs.len(), 1, "function B should have been called once");
    assert_eq!(b_msgs[0]["from_a"], true);
    assert_eq!(b_msgs[0]["label"], "chained-work");
}

// ---------------------------------------------------------------------------
// Durable subscriber scenarios (ported from motia queue integration suite).
// See sdk/packages/node/iii/tests/queue.test.ts and
// sdk/packages/python/iii/tests/test_queue_integration.py for the JS/Python
// counterparts. These exercise the `durable:subscriber` trigger type +
// `iii::durable::publish` fan-out pattern, distinct from the
// `TriggerAction::Enqueue` coverage above.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn durable_subscriber_receives_published_message() {
    let iii = common::shared_iii();
    let topic = unique_topic("test-durable-basic-rs");
    let function_id = format!("test.queue.durable.basic.rs.{}", topic);

    let received: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
    let received_clone = received.clone();
    let fn_ref = iii.register_function((
        RegisterFunctionMessage::with_id(function_id.clone()),
        move |data: Value| {
            let received = received_clone.clone();
            async move {
                *received.lock().await = Some(data);
                Ok(json!({ "ok": true }))
            }
        },
    ));
    let trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "durable:subscriber".to_string(),
            function_id: fn_ref.id.clone(),
            config: json!({ "topic": topic }),
            metadata: None,
        })
        .expect("register durable:subscriber");

    common::settle().await;

    iii.trigger(TriggerRequest {
        function_id: "iii::durable::publish".to_string(),
        payload: json!({ "topic": topic, "data": { "order": "abc" } }),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("iii::durable::publish");

    let expected = json!({ "order": "abc" });
    let mut got: Option<Value> = None;
    for _ in 0..50 {
        got = received.lock().await.clone();
        if got.as_ref() == Some(&expected) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    trigger.unregister();
    fn_ref.unregister();

    assert_eq!(got, Some(expected));
}

#[tokio::test]
async fn durable_subscriber_receives_exact_nested_payload() {
    let iii = common::shared_iii();
    let topic = unique_topic("test-durable-payload-rs");
    let function_id = format!("test.queue.durable.payload.rs.{}", topic);
    let payload = json!({ "id": "x1", "count": 42, "nested": { "a": 1 } });

    let received: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
    let received_clone = received.clone();
    let fn_ref = iii.register_function((
        RegisterFunctionMessage::with_id(function_id.clone()),
        move |data: Value| {
            let received = received_clone.clone();
            async move {
                *received.lock().await = Some(data);
                Ok(json!({ "ok": true }))
            }
        },
    ));
    let trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "durable:subscriber".to_string(),
            function_id: fn_ref.id.clone(),
            config: json!({ "topic": topic }),
            metadata: None,
        })
        .expect("register durable:subscriber");

    common::settle().await;

    iii.trigger(TriggerRequest {
        function_id: "iii::durable::publish".to_string(),
        payload: json!({ "topic": topic, "data": payload }),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("iii::durable::publish");

    let mut got: Option<Value> = None;
    for _ in 0..50 {
        got = received.lock().await.clone();
        if got.as_ref() == Some(&payload) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    trigger.unregister();
    fn_ref.unregister();

    assert_eq!(got, Some(payload));
}

#[tokio::test]
async fn durable_subscriber_with_queue_config_receives_messages() {
    let iii = common::shared_iii();
    let topic = unique_topic("test-durable-infra-rs");
    let function_id = format!("test.queue.durable.infra.rs.{}", topic);

    let received: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
    let received_clone = received.clone();
    let fn_ref = iii.register_function((
        RegisterFunctionMessage::with_id(function_id.clone()),
        move |data: Value| {
            let received = received_clone.clone();
            async move {
                *received.lock().await = Some(data);
                Ok(json!({ "ok": true }))
            }
        },
    ));
    let trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "durable:subscriber".to_string(),
            function_id: fn_ref.id.clone(),
            config: json!({
                "topic": topic,
                "queue_config": {
                    "maxRetries": 5,
                    "type": "standard",
                    "concurrency": 2,
                },
            }),
            metadata: None,
        })
        .expect("register durable:subscriber with queue_config");

    common::settle().await;

    iii.trigger(TriggerRequest {
        function_id: "iii::durable::publish".to_string(),
        payload: json!({ "topic": topic, "data": { "infra": true } }),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("iii::durable::publish");

    let expected = json!({ "infra": true });
    let mut got: Option<Value> = None;
    for _ in 0..50 {
        got = received.lock().await.clone();
        if got.as_ref() == Some(&expected) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    trigger.unregister();
    fn_ref.unregister();

    assert_eq!(got, Some(expected));
}

#[tokio::test]
async fn durable_subscriber_fanout_to_multiple_subscribers() {
    let iii = common::shared_iii();
    let topic = unique_topic("test-durable-fanout-rs");
    let function_id_1 = format!("test.queue.durable.multi1.rs.{}", topic);
    let function_id_2 = format!("test.queue.durable.multi2.rs.{}", topic);

    let received_1: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let received_2: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));

    let received_1_clone = received_1.clone();
    let fn_1 = iii.register_function((
        RegisterFunctionMessage::with_id(function_id_1.clone()),
        move |data: Value| {
            let received = received_1_clone.clone();
            async move {
                received.lock().await.push(data);
                Ok(json!({ "ok": true }))
            }
        },
    ));
    let received_2_clone = received_2.clone();
    let fn_2 = iii.register_function((
        RegisterFunctionMessage::with_id(function_id_2.clone()),
        move |data: Value| {
            let received = received_2_clone.clone();
            async move {
                received.lock().await.push(data);
                Ok(json!({ "ok": true }))
            }
        },
    ));
    let trigger_1 = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "durable:subscriber".to_string(),
            function_id: fn_1.id.clone(),
            config: json!({ "topic": topic }),
            metadata: None,
        })
        .expect("register durable:subscriber #1");
    let trigger_2 = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "durable:subscriber".to_string(),
            function_id: fn_2.id.clone(),
            config: json!({ "topic": topic }),
            metadata: None,
        })
        .expect("register durable:subscriber #2");

    tokio::time::sleep(Duration::from_millis(500)).await;

    iii.trigger(TriggerRequest {
        function_id: "iii::durable::publish".to_string(),
        payload: json!({ "topic": topic, "data": { "msg": 1 } }),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("publish msg 1");
    iii.trigger(TriggerRequest {
        function_id: "iii::durable::publish".to_string(),
        payload: json!({ "topic": topic, "data": { "msg": 2 } }),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("publish msg 2");

    for _ in 0..50 {
        let got_1 = received_1.lock().await.len();
        let got_2 = received_2.lock().await.len();
        if got_1 >= 2 && got_2 >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let msgs_1 = received_1.lock().await.clone();
    let msgs_2 = received_2.lock().await.clone();

    trigger_1.unregister();
    trigger_2.unregister();
    fn_1.unregister();
    fn_2.unregister();

    assert_eq!(msgs_1.len(), 2, "fn1 should receive both messages");
    assert_eq!(msgs_2.len(), 2, "fn2 should receive both messages");
    assert!(msgs_1.contains(&json!({ "msg": 1 })));
    assert!(msgs_1.contains(&json!({ "msg": 2 })));
    assert!(msgs_2.contains(&json!({ "msg": 1 })));
    assert!(msgs_2.contains(&json!({ "msg": 2 })));
}

#[tokio::test]
async fn durable_subscriber_condition_function_filters_messages() {
    let iii = common::shared_iii();
    let topic = unique_topic("test-durable-cond-rs");
    let function_id = format!("test.queue.durable.cond.rs.{}", topic);
    let condition_function_id = format!("{function_id}::conditions::0");

    let handler_calls: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let handler_calls_clone = handler_calls.clone();
    let fn_ref = iii.register_function((
        RegisterFunctionMessage::with_id(function_id.clone()),
        move |_data: Value| {
            let handler_calls = handler_calls_clone.clone();
            async move {
                *handler_calls.lock().await += 1;
                Ok(json!({ "ok": true }))
            }
        },
    ));
    let cond_fn = iii.register_function((
        RegisterFunctionMessage::with_id(condition_function_id.clone()),
        move |input: Value| async move {
            let accept = input
                .get("accept")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Ok(Value::Bool(accept))
        },
    ));
    let trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "durable:subscriber".to_string(),
            function_id: fn_ref.id.clone(),
            config: json!({
                "topic": topic,
                "condition_function_id": cond_fn.id.clone(),
            }),
            metadata: None,
        })
        .expect("register durable:subscriber with condition");

    tokio::time::sleep(Duration::from_millis(500)).await;

    iii.trigger(TriggerRequest {
        function_id: "iii::durable::publish".to_string(),
        payload: json!({ "topic": topic, "data": { "accept": false } }),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("publish rejected msg");
    iii.trigger(TriggerRequest {
        function_id: "iii::durable::publish".to_string(),
        payload: json!({ "topic": topic, "data": { "accept": true } }),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("publish accepted msg");

    // Poll until we see the accepted message, then wait a bit longer to make
    // sure the rejected one is not still in flight.
    for _ in 0..50 {
        if *handler_calls.lock().await >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    let calls = *handler_calls.lock().await;

    trigger.unregister();
    fn_ref.unregister();
    cond_fn.unregister();

    assert_eq!(
        calls, 1,
        "only the message satisfying the condition should be delivered"
    );
}
