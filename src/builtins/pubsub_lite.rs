use std::{collections::HashMap, sync::Arc};

use serde_json::Value;
use tokio::sync::{RwLock, broadcast};

#[async_trait::async_trait]
pub trait Subscriber: Send + Sync {
    async fn handle_message(&self, message: Arc<Value>) -> anyhow::Result<()>;
}

type TopicName = String;
type SubscriptionId = String;

pub struct BuiltInPubSubLite {
    subscribers: RwLock<HashMap<SubscriptionId, Arc<dyn Subscriber>>>,
    events_tx: tokio::sync::broadcast::Sender<Value>,
}

impl BuiltInPubSubLite {
    pub fn new(config: Option<Value>) -> Self {
        let channel_size = config
            .clone()
            .and_then(|cfg| cfg.get("channel_size").and_then(|v| v.as_u64()))
            .unwrap_or(256) as usize;

        let (events_tx, _events_rx) = tokio::sync::broadcast::channel(channel_size);
        Self {
            subscribers: RwLock::new(HashMap::new()),
            events_tx,
        }
    }
    pub async fn subscribe(&self, subscription_id: TopicName, connection: Arc<dyn Subscriber>) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.insert(subscription_id.clone(), connection);
    }

    pub async fn unsubscribe(&self, subscription_id: SubscriptionId) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.remove(&subscription_id);
    }

    pub fn send_msg<T: serde::Serialize>(&self, message: T) {
        match serde_json::to_value(message) {
            Ok(value) => {
                let _ = self.events_tx.send(value);
            }
            Err(err) => {
                tracing::error!(error = %err, "Failed to serialize message for send_msg");
            }
        }
    }

    pub async fn watch_events(&self) {
        let mut rx = self.events_tx.subscribe();

        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let msg = Arc::new(msg);
                    tracing::debug!("Received message event: {:?}", msg);
                    let subscribers = self.subscribers.read().await;
                    let subscribers = subscribers.values().collect::<Vec<_>>();
                    let mut promises = Vec::new();

                    for connection in subscribers {
                        promises.push(connection.handle_message(msg.clone()));
                    }

                    futures::future::join_all(promises).await;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    tracing::warn!("Lagged in receiving messages");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::error!("Messages channel closed");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use async_trait::async_trait;
    use mockall::mock;
    use uuid::Uuid;

    use super::*;

    mock! {
        pub Subscriber {}
        #[async_trait]
        impl Subscriber for Subscriber {
            async fn handle_message(&self, msg: Arc<Value>) -> anyhow::Result<()>;
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_unsubscribe() {
        let adapter = BuiltInPubSubLite::new(None);
        let connection: Arc<dyn Subscriber> = Arc::new(MockSubscriber::new());
        let subscription_id = Uuid::new_v4().to_string();

        adapter
            .subscribe(subscription_id.clone(), connection.clone())
            .await;

        {
            let subscribers = adapter.subscribers.read().await;
            assert!(subscribers.contains_key(&subscription_id));
        }

        adapter.unsubscribe(subscription_id.clone()).await;

        {
            let subscribers = adapter.subscribers.read().await;
            assert!(!subscribers.contains_key(&subscription_id));
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_normal_flow() {
        use std::sync::Arc as StdArc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let call_count = StdArc::new(AtomicU32::new(0));

        struct TestSubscriber {
            call_count: StdArc<AtomicU32>,
        }

        #[async_trait::async_trait]
        impl Subscriber for TestSubscriber {
            async fn handle_message(&self, _msg: Arc<Value>) -> anyhow::Result<()> {
                self.call_count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let adapter = BuiltInPubSubLite::new(None);
        let subscription_id1 = Uuid::new_v4().to_string();
        let subscription_id2 = Uuid::new_v4().to_string();

        // Subscribe two subscribers in normal (non-exclusive) mode
        let subscriber1: Arc<dyn Subscriber> = Arc::new(TestSubscriber {
            call_count: call_count.clone(),
        });
        let subscriber2: Arc<dyn Subscriber> = Arc::new(TestSubscriber {
            call_count: call_count.clone(),
        });

        adapter
            .subscribe(subscription_id1.clone(), subscriber1)
            .await;
        adapter
            .subscribe(subscription_id2.clone(), subscriber2)
            .await;

        // Start watching events in background
        let adapter = Arc::new(adapter);
        let adapter_clone = Arc::clone(&adapter);
        tokio::spawn(async move {
            adapter_clone.watch_events().await;
        });

        // Give it a moment to start watching
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Send a message
        adapter.send_msg(serde_json::json!({"test": "data"}));

        // Wait for message to be processed
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // In normal mode, both subscribers should have been called
        assert_eq!(call_count.load(Ordering::SeqCst), 2);

        adapter.unsubscribe(subscription_id1).await;
        // Send a message
        adapter.send_msg(serde_json::json!({"test": "data"}));

        // Wait for message to be processed
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        assert_eq!(call_count.load(Ordering::SeqCst), 3);

        adapter.unsubscribe(subscription_id2).await;
        // Send a message
        adapter.send_msg(serde_json::json!({"test": "data"}));

        // Wait for message to be processed
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }
}
