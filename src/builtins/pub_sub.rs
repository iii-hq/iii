use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;

use crate::{
    engine::Engine,
    modules::streams::{StreamWrapperMessage, adapters::StreamConnection},
};

type StreamSubscribers = Vec<Arc<dyn StreamConnection>>;
type SubscriberId = String;

pub struct BuiltInPubSubAdapter {
    stream_subscribers: RwLock<HashMap<SubscriberId, StreamSubscribers>>,
}

impl BuiltInPubSubAdapter {
    pub fn new(_engine: Arc<Engine>) -> Self {
        Self {
            stream_subscribers: RwLock::new(HashMap::new()),
        }
    }

    pub async fn subscribe_connection(&self, id: String, connection: Arc<dyn StreamConnection>) {
        let mut subscribers = self.stream_subscribers.write().await;
        let entry = subscribers.entry(id).or_insert_with(Vec::new);
        entry.push(connection);
    }

    pub async fn unsubscribe_connection(&self, id: String) {
        let mut subscribers = self.stream_subscribers.write().await;
        subscribers.remove(&id);
    }

    pub async fn send_msg(&self, message: StreamWrapperMessage) {
        let connections: Vec<Arc<dyn StreamConnection>> = {
            let subscribers = self.stream_subscribers.read().await;
            subscribers
                .values()
                .flat_map(|connections| connections.iter().cloned())
                .collect()
        };
        if connections.is_empty() {
            return;
        }
        let message = Arc::new(message);
        for connection in connections {
            let message = Arc::clone(&message);
            tokio::spawn(async move {
                if let Err(e) = connection.handle_stream_message(message.as_ref()).await {
                    tracing::error!(error = ?e, "Failed to handle stream message");
                }
            });
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use tokio::sync::Notify;

    use super::*;
    use crate::modules::streams::{
        StreamOutboundMessage, adapters::StreamConnection as StreamConnectionTrait,
    };

    struct TestConnection {
        message_count: Arc<AtomicUsize>,
        notify: Arc<Notify>,
    }

    impl TestConnection {
        fn new() -> (Self, Arc<AtomicUsize>, Arc<Notify>) {
            let count = Arc::new(AtomicUsize::new(0));
            let notify = Arc::new(Notify::new());
            (
                Self {
                    message_count: Arc::clone(&count),
                    notify: Arc::clone(&notify),
                },
                count,
                notify,
            )
        }
    }

    #[async_trait]
    impl StreamConnectionTrait for TestConnection {
        async fn handle_stream_message(&self, _msg: &StreamWrapperMessage) -> anyhow::Result<()> {
            self.message_count.fetch_add(1, Ordering::SeqCst);
            self.notify.notify_one();
            Ok(())
        }

        async fn cleanup(&self) {}
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_subscribe_unsubscribe() {
        let engine = Arc::new(Engine::new());
        let adapter = BuiltInPubSubAdapter::new(engine);
        let (connection, _, _) = TestConnection::new();
        let connection: Arc<dyn StreamConnection> = Arc::new(connection);
        let subscriber_id = "test_subscriber".to_string();

        // Subscribe
        adapter
            .subscribe_connection(subscriber_id.clone(), connection.clone())
            .await;
        {
            let subscribers = adapter.stream_subscribers.read().await;
            assert!(subscribers.contains_key(&subscriber_id));
            assert_eq!(subscribers.get(&subscriber_id).unwrap().len(), 1);
        }

        // Unsubscribe
        adapter.unsubscribe_connection(subscriber_id.clone()).await;
        {
            let subscribers = adapter.stream_subscribers.read().await;
            assert!(!subscribers.contains_key(&subscriber_id));
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_send_msg_delivers_to_subscribers() {
        let engine = Arc::new(Engine::new());
        let adapter = BuiltInPubSubAdapter::new(engine);

        let (connection, count, notify) = TestConnection::new();
        let connection: Arc<dyn StreamConnection> = Arc::new(connection);

        adapter
            .subscribe_connection("subscriber1".to_string(), connection)
            .await;

        let message = StreamWrapperMessage {
            stream_name: "test_stream".to_string(),
            group_id: "test_group".to_string(),
            id: Some("item1".to_string()),
            timestamp: chrono::Utc::now().timestamp_millis(),
            event: StreamOutboundMessage::Create {
                data: serde_json::json!({"key": "value"}),
            },
        };

        adapter.send_msg(message).await;

        // Wait for the message to be delivered
        tokio::time::timeout(std::time::Duration::from_secs(1), notify.notified())
            .await
            .expect("Timed out waiting for message delivery");

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
