//! Integration tests for the queue system via SDK.
//!
//! Requires a running III engine with queue module configured.
//! Set III_URL or use ws://localhost:49134 default.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iii_sdk::{IIIError, RegisterFunctionMessage, TriggerAction, TriggerRequest};

#[tokio::test]
async fn enqueue_returns_acknowledgement() {
    let iii = common::shared_iii();

    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.queue.echo.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let received = received_clone.clone();
            async move {
                received.lock().await.push(input.clone());
                Ok(json!({ "processed": true }))
            }
        },
    );
    common::settle().await;

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test.queue.echo.rs".to_string(),
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
            function_id: "test.queue.unknown.rs".to_string(),
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
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.queue.fifo.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let received = received_clone.clone();
            async move {
                received.lock().await.push(input.clone());
                Ok(json!({ "processed": true }))
            }
        },
    );
    common::settle().await;

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test.queue.fifo.rs".to_string(),
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
            function_id: "test.queue.fifo.nofield.rs".to_string(),
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
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.queue.void.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |_input: Value| {
            let count = count_clone.clone();
            async move {
                *count.lock().await += 1;
                Ok(json!({ "done": true }))
            }
        },
    );
    common::settle().await;

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test.queue.void.rs".to_string(),
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
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.queue.multi.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let received = received_clone.clone();
            async move {
                received.lock().await.push(input.clone());
                Ok(json!({ "processed": true }))
            }
        },
    );
    common::settle().await;

    let message_count = 5;
    for i in 0..message_count {
        let result = iii
            .trigger(TriggerRequest {
                function_id: "test.queue.multi.rs".to_string(),
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
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.queue.chain.b.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let b_received = b_received_clone.clone();
            async move {
                b_received.lock().await.push(input.clone());
                Ok(json!({ "step": "b_done" }))
            }
        },
    );

    let a_received = Arc::new(Mutex::new(Vec::new()));
    let a_received_clone = a_received.clone();
    let iii_for_a = iii.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "test.queue.chain.a.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input: Value| {
            let a_received = a_received_clone.clone();
            let iii = iii_for_a.clone();
            async move {
                a_received.lock().await.push(input.clone());

                let label = input["label"].as_str().unwrap_or("unknown").to_string();
                iii.trigger(TriggerRequest {
                    function_id: "test.queue.chain.b.rs".to_string(),
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
    );
    common::settle().await;

    let result = iii
        .trigger(TriggerRequest {
            function_id: "test.queue.chain.a.rs".to_string(),
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
