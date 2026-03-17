//! Integration tests for PubSub operations.
//!
//! Requires a running III engine. Set III_URL or use ws://localhost:49134 default.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iii_sdk::{RegisterFunctionMessage, RegisterTriggerInput, TriggerRequest};

fn unique_topic(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{prefix}_{ts}")
}

#[tokio::test]
async fn subscribe_and_receive_published_messages() {
    let iii = common::shared_iii();

    let topic = unique_topic("test_topic");
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx = Arc::new(Mutex::new(Some(tx)));

    let fn_id = format!("test.pubsub.rs.subscriber.{topic}");
    let fn_ref = iii.register_function(
        RegisterFunctionMessage {
            id: fn_id.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |data: Value| {
            let received = received_clone.clone();
            let tx = tx.clone();
            async move {
                received.lock().await.push(data);
                if let Some(sender) = tx.lock().await.take() {
                    let _ = sender.send(());
                }
                Ok(json!({}))
            }
        },
    );

    let trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "subscribe".to_string(),
            function_id: fn_id.clone(),
            config: json!({"topic": topic}),
        })
        .expect("register trigger");

    common::settle().await;

    iii.trigger(TriggerRequest {
        function_id: "publish".to_string(),
        payload: json!({"topic": topic, "data": {"message": "Hello PubSub!"}}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("publish");

    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("timeout waiting for pubsub message")
        .expect("channel error");

    let msgs = received.lock().await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["message"], "Hello PubSub!");

    fn_ref.unregister();
    trigger.unregister();
}

#[tokio::test]
async fn topic_isolation() {
    let iii = common::shared_iii();

    let topic_a = unique_topic("topic_a");
    let topic_b = unique_topic("topic_b");

    let received_a = Arc::new(Mutex::new(Vec::<Value>::new()));
    let received_b = Arc::new(Mutex::new(Vec::<Value>::new()));
    let received_a_clone = received_a.clone();
    let received_b_clone = received_b.clone();

    let (tx_a, rx_a) = tokio::sync::oneshot::channel::<()>();
    let tx_a = Arc::new(Mutex::new(Some(tx_a)));

    let fn_id_a = format!("test.pubsub.rs.topic_a.{topic_a}");
    let fn_id_b = format!("test.pubsub.rs.topic_b.{topic_b}");

    let fn_a = iii.register_function(
        RegisterFunctionMessage {
            id: fn_id_a.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |data: Value| {
            let received = received_a_clone.clone();
            let tx = tx_a.clone();
            async move {
                received.lock().await.push(data);
                if let Some(sender) = tx.lock().await.take() {
                    let _ = sender.send(());
                }
                Ok(json!({}))
            }
        },
    );

    let fn_b = iii.register_function(
        RegisterFunctionMessage {
            id: fn_id_b.clone(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |data: Value| {
            let received = received_b_clone.clone();
            async move {
                received.lock().await.push(data);
                Ok(json!({}))
            }
        },
    );

    let trigger_a = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "subscribe".to_string(),
            function_id: fn_id_a.clone(),
            config: json!({"topic": topic_a}),
        })
        .expect("register trigger a");
    let trigger_b = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "subscribe".to_string(),
            function_id: fn_id_b.clone(),
            config: json!({"topic": topic_b}),
        })
        .expect("register trigger b");

    common::settle().await;

    iii.trigger(TriggerRequest {
        function_id: "publish".to_string(),
        payload: json!({"topic": topic_a, "data": {"for": "a"}}),
        action: None,
        timeout_ms: None,
    })
    .await
    .expect("publish to topic_a");

    tokio::time::timeout(Duration::from_secs(5), rx_a)
        .await
        .expect("timeout waiting for topic A message")
        .expect("channel error");

    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(received_a.lock().await.len(), 1);
    assert_eq!(received_b.lock().await.len(), 0);

    fn_a.unregister();
    fn_b.unregister();
    trigger_a.unregister();
    trigger_b.unregister();
}
