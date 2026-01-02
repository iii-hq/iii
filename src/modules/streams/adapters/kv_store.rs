use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    builtins::{BuiltInPubSubAdapter, BuiltinKvStore},
    engine::Engine,
    modules::streams::{
        StreamOutboundMessage, StreamWrapperMessage,
        adapters::{StreamAdapter, StreamConnection},
        registry::{StreamAdapterFuture, StreamAdapterRegistration},
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
}

#[async_trait]
impl StreamAdapter for BuiltinKvStoreAdapter {
    async fn emit_event(&self, message: StreamWrapperMessage) {
        self.pub_sub.send_msg(message);
    }
    async fn set(&self, stream_name: &str, group_id: &str, item_id: &str, data: Value) {
        self.storage
            .set(stream_name, group_id, item_id, data.clone())
            .await;
        let exist = self
            .storage
            .get(stream_name, group_id, item_id)
            .await
            .is_some();
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
        self.storage.get(stream_name, group_id, item_id).await
    }

    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str) {
        self.storage.delete(stream_name, group_id, item_id).await;
    }

    async fn get_group(&self, stream_name: &str, group_id: &str) -> Vec<Value> {
        self.storage.get_group(stream_name, group_id).await
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
