//! Integration tests for PubSub operations: a subscriber observes
//! `publish` with a filter on `input.topic`.
//!
//! Requires a running III engine. Set III_URL or use ws://localhost:49134 default.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iii_sdk::{CallRequest, Deliver, IIIClient, ObserverHandle, RegisterFunction, Source};

fn unique_topic(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{prefix}_{ts}")
}

/// Observes the messages published to `topic`: the target gets each call's
/// input, `{ topic, data }`.
async fn subscribe(iii: &IIIClient, topic: &str, target: &str) -> ObserverHandle {
    let observer = iii.observe(
        Source::function("publish")
            .filters(json!({ "input.topic": { "$eq": topic } }))
            .deliver(Deliver::Input),
        target,
    );
    // An observer of a function nobody hosts stays pending: bound the wait so a
    // missing pubsub worker fails the test instead of hanging it.
    tokio::time::timeout(Duration::from_secs(10), observer.ready())
        .await
        .expect("`publish` is not hosted: is the pubsub worker running?")
        .expect("subscribe");
    observer
}

#[tokio::test]
async fn subscribe_and_receive_published_messages() {
    let iii = common::shared_iii();

    let topic = unique_topic("test_topic");
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx = Arc::new(Mutex::new(Some(tx)));

    let fn_id = format!("test::pubsub::rs::subscriber::{topic}");
    let fn_ref = iii.register_function(
        fn_id.clone(),
        RegisterFunction::new_async(move |data: Value| {
            let received = received_clone.clone();
            let tx = tx.clone();
            async move {
                received.lock().await.push(data);
                if let Some(sender) = tx.lock().await.take() {
                    let _ = sender.send(());
                }
                Ok(json!({}))
            }
        }),
    );

    let subscription = subscribe(iii, &topic, &fn_id).await;

    common::settle().await;

    iii.call(CallRequest {
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
    assert_eq!(msgs[0]["data"]["message"], "Hello PubSub!");

    subscription.dispose().await.expect("dispose");
    fn_ref.unregister();
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

    let fn_id_a = format!("test::pubsub::rs::topic_a::{topic_a}");
    let fn_id_b = format!("test::pubsub::rs::topic_b::{topic_b}");

    let fn_a = iii.register_function(
        fn_id_a.clone(),
        RegisterFunction::new_async(move |data: Value| {
            let received = received_a_clone.clone();
            let tx = tx_a.clone();
            async move {
                received.lock().await.push(data);
                if let Some(sender) = tx.lock().await.take() {
                    let _ = sender.send(());
                }
                Ok(json!({}))
            }
        }),
    );

    let fn_b = iii.register_function(
        fn_id_b.clone(),
        RegisterFunction::new_async(move |data: Value| {
            let received = received_b_clone.clone();
            async move {
                received.lock().await.push(data);
                Ok(json!({}))
            }
        }),
    );

    let subscription_a = subscribe(iii, &topic_a, &fn_id_a).await;
    let subscription_b = subscribe(iii, &topic_b, &fn_id_b).await;

    common::settle().await;

    iii.call(CallRequest {
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

    subscription_a.dispose().await.expect("dispose a");
    subscription_b.dispose().await.expect("dispose b");
    fn_a.unregister();
    fn_b.unregister();
}
