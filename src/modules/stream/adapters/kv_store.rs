// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use iii_sdk::{
    UpdateOp, UpdateResult,
    types::{DeleteResult, SetResult},
};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    builtins::{kv::BuiltinKvStore, pubsub_lite::BuiltInPubSubLite},
    engine::Engine,
    modules::stream::{
        StreamMetadata, StreamWrapperMessage,
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
    pub_sub: BuiltInPubSubLite,
}

impl BuiltinKvStoreAdapter {
    pub fn new(config: Option<Value>) -> Self {
        let storage = BuiltinKvStore::new(config.clone());
        let pub_sub = BuiltInPubSubLite::new(config);
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

    async fn delete(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
    ) -> anyhow::Result<DeleteResult> {
        let index = self.gen_key(stream_name, group_id);
        let old_value = self.storage.delete(index, item_id.to_string()).await;

        Ok(old_value)
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

    async fn list_all_stream(&self) -> anyhow::Result<Vec<StreamMetadata>> {
        use std::collections::{HashMap, HashSet};

        let all_keys = self
            .storage
            .list_keys_with_prefix("stream:".to_string())
            .await;
        let mut stream_map: HashMap<String, HashSet<String>> = HashMap::new();

        // Parse keys to extract stream names and groups
        for key in all_keys {
            let parts: Vec<&str> = key.split(':').collect();
            // Ensure key follows format "stream:<stream_name>:<group_id>"
            if parts.len() >= 3 && parts[0] == "stream" {
                let stream_name = parts[1].to_string();
                let group_id = parts[2].to_string();

                stream_map.entry(stream_name).or_default().insert(group_id);
            }
        }

        // Convert to StreamMetadata
        let mut stream: Vec<StreamMetadata> = stream_map
            .into_iter()
            .map(|(id, groups)| {
                let mut groups_vec: Vec<String> = groups.into_iter().collect();
                groups_vec.sort();
                StreamMetadata {
                    id,
                    groups: groups_vec,
                }
            })
            .collect();

        stream.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(stream)
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

crate::register_adapter!(<StreamAdapterRegistration> "modules::stream::adapters::KvStore", make_adapter);
