use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{RwLock, broadcast};

use crate::{
    engine::Engine,
    modules::streams::{
        StreamOutboundMessage, StreamWrapperMessage,
        adapters::{StreamAdapter, StreamConnection},
        registry::{StreamAdapterFuture, StreamAdapterRegistration},
    },
};

type Subscribers = Vec<Arc<dyn StreamConnection>>;
type TopicName = String;
type GroupId = String;
type ItemId = String;
type ItemsData = HashMap<ItemId, Value>;

pub struct KvStore {
    store: RwLock<HashMap<(TopicName, GroupId), ItemsData>>,
    subscribers: RwLock<HashMap<TopicName, Subscribers>>,
    events_tx: tokio::sync::broadcast::Sender<StreamWrapperMessage>,
}

impl KvStore {
    pub fn new() -> Self {
        let (events_tx, _events_rx) = tokio::sync::broadcast::channel(256);
        Self {
            store: RwLock::new(HashMap::new()),
            subscribers: RwLock::new(HashMap::new()),
            events_tx,
        }
    }
}

#[async_trait]
impl StreamAdapter for KvStore {
    async fn emit_event(&self, message: StreamWrapperMessage) {
        let _ = self.events_tx.send(message);
    }
    async fn set(&self, stream_name: &str, group_id: &str, item_id: &str, data: Value) {
        let key = (stream_name.to_string(), group_id.to_string());
        let mut store = self.store.write().await;
        if let Some(topic) = store.get_mut(&key) {
            let existed = topic.contains_key(item_id);
            topic.insert(item_id.to_string(), data.clone());
            let event = if existed {
                StreamOutboundMessage::Update { data }
            } else {
                StreamOutboundMessage::Create { data }
            };
            let message = StreamWrapperMessage {
                timestamp: chrono::Utc::now().timestamp_millis(),
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                id: Some(item_id.to_string()),
                event,
            };
            self.emit_event(message).await;
            return;
        } else {
            let mut topic = HashMap::new();
            topic.insert(item_id.to_string(), data.clone());
            store.insert(key, topic);
            let message = StreamWrapperMessage {
                timestamp: chrono::Utc::now().timestamp_millis(),
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                id: Some(item_id.to_string()),
                event: StreamOutboundMessage::Create { data },
            };
            self.emit_event(message).await;
            return;
        }
    }

    async fn get(&self, stream_name: &str, group_id: &str, item_id: &str) -> Option<Value> {
        let key = (stream_name.to_string(), group_id.to_string());
        let store = self.store.read().await;
        let topic = store.get(&key);
        if let Some(group) = topic {
            group.get(item_id).cloned()
        } else {
            None
        }
    }
    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str) {
        let key = (stream_name.to_string(), group_id.to_string());
        let mut store = self.store.write().await;
        if let Some(group) = store.get_mut(&key)
            && let Some(data) = group.remove(item_id)
        {
            let message = StreamWrapperMessage {
                timestamp: chrono::Utc::now().timestamp_millis(),
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                id: Some(item_id.to_string()),
                event: StreamOutboundMessage::Delete { data: data.clone() },
            };
            self.emit_event(message).await;
            return;
        }
        tracing::warn!(stream_name = %stream_name, group_id = %group_id, item_id = %item_id, "Item to delete not found");
    }

    async fn get_group(&self, stream_name: &str, group_id: &str) -> Vec<Value> {
        let key = (stream_name.to_string(), group_id.to_string());
        let store = self.store.read().await;
        if let Some(topic) = store.get(&key) {
            topic.values().cloned().collect()
        } else {
            Vec::new()
        }
    }

    async fn subscribe(&self, id: String, connection: Arc<dyn StreamConnection>) {
        let mut subscribers = self.subscribers.write().await;
        let entry = subscribers.entry(id).or_insert_with(Vec::new);
        entry.push(connection.clone());
    }
    async fn unsubscribe(&self, id: String) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.remove(&id);
    }

    async fn watch_events(&self) {
        let mut rx = self.events_tx.subscribe();

        loop {
            match rx.recv().await {
                Ok(msg) => {
                    tracing::debug!("Received stream event: {:?}", msg);
                    let group_id = msg.group_id.clone();
                    let stream_name = msg.stream_name.clone();
                    let subscribers = self.subscribers.read().await;
                    // For simplicity, broadcasting to all subscribers of the stream
                    // In a real implementation, you might want to filter based on group_id or
                    // other criteria
                    for connections in subscribers.values() {
                        for connection in connections.iter() {
                            tracing::debug!(stream_name = %stream_name, group_id = %group_id, "Handling stream message for subscriber");
                            if let Err(e) = connection.handle_stream_message(&msg).await {
                                tracing::error!(error = %e, stream_name = %stream_name, group_id = %group_id, "Failed to handle stream message");
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    tracing::warn!("Lagged in receiving stream events");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::error!("Stream events channel closed");
                    break;
                }
            }
        }
    }
}

fn make_adapter(_engine: Arc<Engine>, _config: Option<Value>) -> StreamAdapterFuture {
    Box::pin(async move {
        let kv_store = KvStore::new();
        Ok(Arc::new(kv_store) as Arc<dyn StreamAdapter>)
    })
}

crate::register_adapter!(<StreamAdapterRegistration> "modules::streams::adapters::KvStore", make_adapter);
