use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    builtins::{
        kv::{BuiltinKvStore, SetResult},
        pubsub::BuiltInPubSubAdapter,
    },
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
        format!("stream:{}:{}", stream_name, group_id)
    }
}

#[async_trait]
impl StreamAdapter for BuiltinKvStoreAdapter {
    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn update(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        ops: Vec<UpdateOp>,
    ) -> UpdateResult {
        match self
            .storage
            .update(
                self.gen_key(stream_name, group_id),
                item_id.to_string(),
                ops,
            )
            .await
        {
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

    async fn set(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        data: Value,
    ) -> SetResult {
        let index = self.gen_key(stream_name, group_id);
        let result = self
            .storage
            .set(index, item_id.to_string(), data.clone())
            .await;

        let message = StreamWrapperMessage {
            timestamp: chrono::Utc::now().timestamp_millis(),
            stream_name: stream_name.to_string(),
            group_id: group_id.to_string(),
            id: Some(item_id.to_string()),
            event: if result.old_value.is_some() {
                StreamOutboundMessage::Update { data }
            } else {
                StreamOutboundMessage::Create { data }
            },
        };
        self.emit_event(message).await;

        result
    }

    async fn get(&self, stream_name: &str, group_id: &str, item_id: &str) -> Option<Value> {
        let index = self.gen_key(stream_name, group_id);
        let value = self.storage.get(index.clone(), item_id.to_string()).await;

        match value {
            Some(v) => {
                let topic: HashMap<String, Value> = serde_json::from_value(v).unwrap_or_default();
                topic.get(item_id).cloned()
            }
            None => None,
        }
    }

    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str) {
        let index = self.gen_key(stream_name, group_id);

        self.storage.delete(index, item_id.to_string()).await;
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

    async fn get_group(&self, stream_name: &str, group_id: &str) -> Vec<Value> {
        let index = self.gen_key(stream_name, group_id);
        self.storage.list(index).await
    }

    async fn list_groups(&self, stream_name: &str) -> Vec<String> {
        let prefix = self.gen_key(stream_name, "");

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
