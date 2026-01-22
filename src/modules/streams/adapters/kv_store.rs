use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    builtins::{kv::BuiltinKvStore, pubsub::BuiltInPubSubAdapter},
    engine::Engine,
    modules::{
        kv_server::structs::{UpdateOp, UpdateResult},
        streams::{
            StreamOutboundMessage, StreamWrapperMessage,
            adapters::{StreamAdapter, StreamConnection},
            registry::{StreamAdapterFuture, StreamAdapterRegistration},
        },
    },
};

type TopicName = String;
type GroupId = String;
type ItemId = String;
type ItemsDataAsString = HashMap<ItemId, String>;
type StoreKey = (TopicName, GroupId);

#[derive(Clone, Debug, Archive, RkyvSerialize, RkyvDeserialize, Serialize, Deserialize)]
pub struct Storage(HashMap<StoreKey, ItemsDataAsString>);

pub struct BuiltinKvStoreAdapter {
    storage: BuiltinKvStore,
    pub_sub: BuiltInPubSubAdapter,
}

impl BuiltinKvStoreAdapter {
    pub fn new(config: Option<Value>) -> Self {
        let storage = BuiltinKvStore::new(config.clone());
        let pub_sub = BuiltInPubSubAdapter::new(config);
        Self { storage, pub_sub }
    }

    fn gen_key(&self, stream_name: &str, group_id: &str) -> String {
        format!("{}::{}", stream_name, group_id)
    }
}

#[async_trait]
impl StreamAdapter for BuiltinKvStoreAdapter {
    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn update(&self, key: &str, ops: Vec<UpdateOp>) -> UpdateResult {
        match self.storage.update(key.to_string(), ops).await {
            Some(result) => result,
            None => UpdateResult {
                old_value: None,
                new_value: Value::Null,
            },
        }
    }

    async fn emit_event(&self, message: StreamWrapperMessage) {
        self.pub_sub.send_msg(message);
    }

    async fn set(&self, stream_name: &str, group_id: &str, item_id: &str, data: Value) {
        //let key = (stream_name.to_string(), group_id.to_string());
        let key = self.gen_key(stream_name, group_id);
        let mut exist = false;
        if let Some(value) = self.storage.get(key.clone()).await {
            let mut topic: HashMap<String, Value> =
                serde_json::from_value(value).unwrap_or_default();
            exist = topic.contains_key(item_id);
            topic.insert(item_id.to_string(), data.clone());

            match serde_json::to_value(&topic) {
                Ok(value) => {
                    self.storage.set(key, value).await;
                }
                Err(err) => {
                    tracing::error!(error = %err, "Failed to serialize topic");
                    return;
                }
            };
        } else {
            let mut topic = HashMap::new();
            topic.insert(item_id.to_string(), data.clone());

            match serde_json::to_value(&topic) {
                Ok(value) => {
                    self.storage.set(key, value).await;
                }
                Err(err) => {
                    tracing::error!(error = %err, "Failed to serialize topic");
                    return;
                }
            };
        }

        let event = if exist {
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
    }

    async fn get(&self, stream_name: &str, group_id: &str, item_id: &str) -> Option<Value> {
        let key = self.gen_key(stream_name, group_id);
        let value = self.storage.get(key).await;
        match value {
            Some(v) => {
                let topic: HashMap<String, Value> = serde_json::from_value(v).unwrap_or_default();
                topic.get(item_id).cloned()
            }
            None => None,
        }
    }

    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str) {
        let key = self.gen_key(stream_name, group_id);
        let value = self.storage.get(key.clone()).await;
        if let Some(v) = value {
            let mut topic: HashMap<String, Value> = serde_json::from_value(v).unwrap_or_default();
            if topic.remove(item_id).is_some() {
                let data = serde_json::to_value(&topic).unwrap();
                self.storage.set(key, data).await;
                self.emit_event(StreamWrapperMessage {
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    stream_name: stream_name.to_string(),
                    group_id: group_id.to_string(),
                    id: Some(item_id.to_string()),
                    event: StreamOutboundMessage::Delete {
                        data: serde_json::json!({ "id": item_id }),
                    },
                })
                .await;
            }
        }
    }

    async fn get_group(&self, stream_name: &str, group_id: &str) -> Vec<Value> {
        let key = self.gen_key(stream_name, group_id);

        self.storage.list(key).await
    }

    async fn list_groups(&self, stream_name: &str) -> Vec<String> {
        let prefix = &format!("{}::", stream_name);
        self.storage
            .list_keys_with_prefix(prefix.to_string())
            .await
            .into_iter()
            .filter_map(|key| key.strip_prefix(&prefix.clone()).map(|s| s.to_string()))
            .collect()
    }

    async fn subscribe(&self, id: String, connection: Arc<dyn StreamConnection>) {
        self.pub_sub.subscribe(id, connection).await;
    }
    async fn unsubscribe(&self, id: String) {
        self.pub_sub.unsubscribe(id).await;
    }

    async fn watch_events(&self) {
        self.pub_sub.watch_events().await;
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> StreamAdapterFuture {
    Box::pin(
        async move { Ok(Arc::new(BuiltinKvStoreAdapter::new(config)) as Arc<dyn StreamAdapter>) },
    )
}

crate::register_adapter!(<StreamAdapterRegistration> "modules::streams::adapters::KvStore", make_adapter);

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use tokio::sync::mpsc;

    use crate::builtins::pubsub::Subscriber;

    use super::*;

    struct RecordingConnection {
        tx: mpsc::UnboundedSender<StreamWrapperMessage>,
    }

    #[async_trait]
    impl StreamConnection for RecordingConnection {
        async fn cleanup(&self) {}

        async fn handle_stream_message(&self, msg: &StreamWrapperMessage) -> anyhow::Result<()> {
            let _ = self.tx.send(msg.clone());
            Ok(())
        }
    }

    #[async_trait]
    impl Subscriber for RecordingConnection {
        async fn handle_message(&self, message: Arc<Value>) -> anyhow::Result<()> {
            let msg = match serde_json::from_value::<StreamWrapperMessage>((*message).clone()) {
                Ok(msg) => msg,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to deserialize stream message");
                    return Err(anyhow::anyhow!("Failed to deserialize stream message"));
                }
            };
            let _ = self.tx.send(msg);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_kv_store_adapter_set_get_delete() {
        let builtin_adapter = BuiltinKvStoreAdapter::new(None);

        let stream_name = "test_stream";
        let group_id = "test_group";
        let item_id = "item1";
        let data = serde_json::json!({"key": "value"});

        // Test set
        builtin_adapter
            .set(stream_name, group_id, item_id, data.clone())
            .await;

        // Test get
        let saved_data = builtin_adapter
            .get(stream_name, group_id, item_id)
            .await
            .expect("Data should exist");

        assert_eq!(saved_data, data);

        // Test delete
        let deleted_data = builtin_adapter.get(stream_name, group_id, item_id).await;
        assert!(deleted_data.is_some());

        builtin_adapter.delete(stream_name, group_id, item_id).await;

        let deleted_data = builtin_adapter.get(stream_name, group_id, item_id).await;
        assert!(deleted_data.is_none());
    }

    #[tokio::test]
    async fn test_kv_store_adapter_get_group() {
        let builtin_adapter = BuiltinKvStoreAdapter::new(None);
        let stream_name = "test_stream";
        let group_id = "test_group";
        let item1_id = "item1";
        let item2_id = "item2";
        let data1 = serde_json::json!({"key1": "value1"});
        let data2 = serde_json::json!({"key2": "value2"});
        // Set items
        builtin_adapter
            .set(stream_name, group_id, item1_id, data1.clone())
            .await;
        builtin_adapter
            .set(stream_name, group_id, item2_id, data2.clone())
            .await;

        let group_items = builtin_adapter.get_group(stream_name, group_id).await;
        assert_eq!(group_items.len(), 2);
        assert!(group_items.contains(&data1));
        assert!(group_items.contains(&data2));
    }

    #[tokio::test]
    async fn test_kv_store_adapter_update_item() {
        let builtin_adapter = Arc::new(BuiltinKvStoreAdapter::new(None));
        let stream_name = "test_stream";
        let group_id = "test_group";
        let item_id = "item1";
        let data1 = serde_json::json!({"key": "value1"});
        let data2 = serde_json::json!({"key": "value2"});

        let (tx, mut rx) = mpsc::unbounded_channel();
        builtin_adapter
            .subscribe(
                "test-subscriber".to_string(),
                Arc::new(RecordingConnection { tx }),
            )
            .await;
        let watcher_adapter = Arc::clone(&builtin_adapter);
        let watcher = tokio::spawn(async move {
            watcher_adapter.watch_events().await;
        });
        tokio::task::yield_now().await;

        // Set initial item
        builtin_adapter
            .set(stream_name, group_id, item_id, data1.clone())
            .await;
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for create event")
            .expect("Should receive create event");

        assert!(matches!(msg.event, StreamOutboundMessage::Create { .. }));

        // Update item
        builtin_adapter
            .set(stream_name, group_id, item_id, data2.clone())
            .await;
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for update event")
            .expect("Should receive update event");
        assert!(matches!(msg.event, StreamOutboundMessage::Update { .. }));

        let saved_data = builtin_adapter
            .get(stream_name, group_id, item_id)
            .await
            .expect("Data should exist");
        assert_eq!(saved_data, data2);

        watcher.abort();
    }
}
