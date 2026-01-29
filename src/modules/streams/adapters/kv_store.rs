// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use iii_sdk::{UpdateOp, UpdateResult, types::SetResult};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    builtins::{kv::BuiltinKvStore, pubsub::BuiltInPubSubAdapter},
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
    ) -> anyhow::Result<UpdateResult> {
        Ok(self
            .storage
            .update(
                self.gen_key(stream_name, group_id),
                item_id.to_string(),
                ops,
            )
            .await)
    }

    async fn emit_event(&self, message: StreamWrapperMessage) -> anyhow::Result<()> {
        self.pub_sub.send_msg(message);
        Ok(())
    }

    async fn set(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        data: Value,
    ) -> anyhow::Result<SetResult> {
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
        self.emit_event(message).await?;

        Ok(result)
    }

    async fn get(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
    ) -> anyhow::Result<Option<Value>> {
        let index = self.gen_key(stream_name, group_id);
        Ok(self.storage.get(index, item_id.to_string()).await)
    }

    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str) -> anyhow::Result<()> {
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
        .await?;
        Ok(())
    }

    async fn get_group(&self, stream_name: &str, group_id: &str) -> anyhow::Result<Vec<Value>> {
        let index = self.gen_key(stream_name, group_id);
        Ok(self.storage.list(index).await)
    }

    async fn list_groups(&self, stream_name: &str) -> anyhow::Result<Vec<String>> {
        let prefix = self.gen_key(stream_name, "");

        Ok(self
            .storage
            .list_keys_with_prefix(prefix.to_string())
            .await
            .into_iter()
            .filter_map(|key| key.strip_prefix(&prefix.clone()).map(|s| s.to_string()))
            .collect())
    }

    async fn subscribe(
        &self,
        id: String,
        connection: Arc<dyn StreamConnection>,
    ) -> anyhow::Result<()> {
        self.pub_sub.subscribe(id, connection).await;
        Ok(())
    }
    async fn unsubscribe(&self, id: String) -> anyhow::Result<()> {
        self.pub_sub.unsubscribe(id).await;
        Ok(())
    }

    async fn watch_events(&self) -> anyhow::Result<()> {
        self.pub_sub.watch_events().await;
        Ok(())
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
            .await
            .expect("Should subscribe successfully");
        let watcher_adapter = Arc::clone(&builtin_adapter);
        let watcher = tokio::spawn(async move {
            let _ = watcher_adapter.watch_events().await;
        });
        tokio::task::yield_now().await;

        // Set initial item
        builtin_adapter
            .set(stream_name, group_id, item_id, data1.clone())
            .await
            .expect("Should set value successfully");
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for create event")
            .expect("Should receive create event");

        assert!(matches!(msg.event, StreamOutboundMessage::Create { .. }));

        // Update item
        builtin_adapter
            .set(stream_name, group_id, item_id, data2.clone())
            .await
            .expect("Should set value successfully");
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for update event")
            .expect("Should receive update event");
        assert!(matches!(msg.event, StreamOutboundMessage::Update { .. }));

        let saved_data = builtin_adapter
            .get(stream_name, group_id, item_id)
            .await
            .expect("Should get value successfully")
            .expect("Data should exist");
        assert_eq!(saved_data, data2);

        builtin_adapter
            .delete(stream_name, group_id, item_id)
            .await
            .expect("Should delete value successfully");

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for delete event")
            .expect("Should receive delete event");
        assert!(matches!(msg.event, StreamOutboundMessage::Delete { .. }));

        let saved_data = builtin_adapter
            .get(stream_name, group_id, item_id)
            .await
            .expect("Should get value successfully");
        assert!(saved_data.is_none());

        watcher.abort();
    }
}
