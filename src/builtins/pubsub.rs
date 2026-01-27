// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use serde_json::Value;
use tokio::sync::{RwLock, broadcast};

#[async_trait::async_trait]
pub trait Subscriber: Send + Sync {
    async fn handle_message(&self, message: Arc<Value>) -> anyhow::Result<()>;
}

type Subscribers = Vec<Arc<dyn Subscriber>>;
type TopicName = String;

pub struct BuiltInPubSubAdapter {
    subscribers: RwLock<HashMap<TopicName, Subscribers>>,
    events_tx: tokio::sync::broadcast::Sender<Value>,
}

impl BuiltInPubSubAdapter {
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
    pub async fn subscribe(&self, id: String, connection: Arc<dyn Subscriber>) {
        let mut subscribers = self.subscribers.write().await;
        let entry = subscribers.entry(id).or_insert_with(Vec::new);
        entry.push(connection.clone());
    }
    pub async fn unsubscribe(&self, id: String) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.remove(&id);
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
                    for connections in subscribers.values() {
                        for connection in connections.iter() {
                            if let Err(e) = connection.handle_message(msg.clone()).await {
                                tracing::error!(error = %e, "Failed to handle message");
                            }
                        }
                    }
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
        let adapter = BuiltInPubSubAdapter::new(None);
        let connection: Arc<dyn Subscriber> = Arc::new(MockSubscriber::new());
        let topic_id = "test_topic".to_string();

        adapter
            .subscribe(topic_id.clone(), connection.clone())
            .await;
        {
            let subscribers = adapter.subscribers.read().await;
            assert!(subscribers.contains_key(&topic_id));
            assert_eq!(subscribers.get(&topic_id).unwrap().len(), 1);
        }

        adapter.unsubscribe(topic_id.clone()).await;
        {
            let subscribers = adapter.subscribers.read().await;
            assert!(!subscribers.contains_key(&topic_id));
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_pub_sub() {
        let adapter = BuiltInPubSubAdapter::new(None);
        let message = serde_json::json!({"key": "value"});

        let mut rx = adapter.events_tx.subscribe();
        adapter.send_msg(message.clone());

        let received = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for message")
            .expect("Should receive message");

        assert_eq!(received, message);
    }
}
