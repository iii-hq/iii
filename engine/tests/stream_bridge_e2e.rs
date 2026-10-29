//! The stream `bridge` adapter against a real remote engine: every bridged
//! engine publishes its stream events through the remote `publish` function
//! and observes that function's calls on `stream.events` to receive them.

mod observe_support;

use std::sync::Arc;
use std::time::Duration;

use iii::builtins::pubsub_lite::Subscriber;
use iii::observer::ObserversConfig;
use iii::workers::stream::adapters::bridge::{BridgeAdapter, STREAM_EVENTS_TOPIC};
use iii::workers::stream::adapters::{StreamAdapter, StreamConnection};
use iii::workers::stream::{StreamOutboundMessage, StreamWrapperMessage};
use observe_support::{connect, eventually, registered, spawn_engine};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// A local stream subscriber that records the messages it is handed.
struct Recorder(mpsc::UnboundedSender<Value>);

#[async_trait::async_trait]
impl Subscriber for Recorder {
    async fn handle_message(&self, message: Arc<Value>) -> anyhow::Result<()> {
        let _ = self.0.send((*message).clone());
        Ok(())
    }
}

#[async_trait::async_trait]
impl StreamConnection for Recorder {
    async fn cleanup(&self) {}

    async fn handle_stream_message(&self, _message: &StreamWrapperMessage) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn published_stream_events_reach_the_bridged_subscribers() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut pubsub = connect(&t, "pubsub", true).await;
    pubsub.register_function("publish").await;
    registered(&t.engine, "default", "publish").await;

    let bridge = Arc::new(
        BridgeAdapter::new(format!("ws://127.0.0.1:{}", t.port))
            .await
            .unwrap(),
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    bridge
        .subscribe("recorder".to_string(), Arc::new(Recorder(tx)))
        .await
        .unwrap();
    let watcher = tokio::spawn({
        let bridge = bridge.clone();
        async move { bridge.watch_events().await }
    });
    assert!(
        eventually(|| t.engine.observers.is_observed("default", "publish")).await,
        "the bridge observes publish"
    );

    let message = |stream_name: &str| StreamWrapperMessage {
        event_type: "stream".to_string(),
        timestamp: 1,
        stream_name: stream_name.to_string(),
        group_id: "g1".to_string(),
        id: Some("o1".to_string()),
        event: StreamOutboundMessage::Create {
            data: json!({ "total": 5 }),
        },
    };

    // A publish on another topic does not match the bridge's observer.
    let other = pubsub
        .call(
            "publish",
            json!({ "topic": "elsewhere", "data": message("elsewhere") }),
        )
        .await;
    pubsub.serve("publish", Value::Null).await;
    pubsub.result(other).await;

    let message = message("orders");
    let emit = tokio::spawn({
        let bridge = bridge.clone();
        async move { bridge.emit_event(message).await }
    });
    let published = pubsub.serve("publish", Value::Null).await;
    assert_eq!(published["data"]["topic"], STREAM_EVENTS_TOPIC);
    emit.await.unwrap().unwrap();

    let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("the published stream event reaches the subscriber")
        .unwrap();
    assert_eq!(received["streamName"], "orders");
    assert_eq!(received["groupId"], "g1");
    assert_eq!(
        received["event"],
        json!({ "type": "create", "data": { "total": 5 } })
    );
    assert!(rx.try_recv().is_err(), "one event, the stream one");

    watcher.abort();
    bridge.destroy().await.unwrap();
}
