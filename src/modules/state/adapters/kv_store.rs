// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use iii_sdk::{UpdateOp, UpdateResult, types::SetResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    builtins::kv::BuiltinKvStore,
    engine::Engine,
    modules::state::{
        adapters::StateAdapter,
        registry::{StateAdapterFuture, StateAdapterRegistration},
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Storage(HashMap<String, Value>);

pub struct BuiltinKvStoreAdapter {
    storage: BuiltinKvStore,
}

impl BuiltinKvStoreAdapter {
    pub fn new(config: Option<Value>) -> Self {
        let storage = BuiltinKvStore::new(config.clone());
        Self { storage }
    }
}

#[async_trait]
impl StateAdapter for BuiltinKvStoreAdapter {
    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn set(&self, group_id: &str, item_id: &str, data: Value) -> anyhow::Result<SetResult> {
        Ok(self
            .storage
            .set(group_id.to_string(), item_id.to_string(), data.clone())
            .await)
    }

    async fn get(&self, group_id: &str, item_id: &str) -> anyhow::Result<Option<Value>> {
        Ok(self
            .storage
            .get(group_id.to_string(), item_id.to_string())
            .await)
    }

    async fn delete(&self, group_id: &str, item_id: &str) -> anyhow::Result<()> {
        self.storage
            .delete(group_id.to_string(), item_id.to_string())
            .await;
        Ok(())
    }

    async fn update(
        &self,
        group_id: &str,
        item_id: &str,
        ops: Vec<UpdateOp>,
    ) -> anyhow::Result<UpdateResult> {
        Ok(self
            .storage
            .update(group_id.to_string(), item_id.to_string(), ops)
            .await)
    }

    async fn list(&self, group_id: &str) -> anyhow::Result<Vec<Value>> {
        Ok(self.storage.list(group_id.to_string()).await)
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> StateAdapterFuture {
    Box::pin(
        async move { Ok(Arc::new(BuiltinKvStoreAdapter::new(config)) as Arc<dyn StateAdapter>) },
    )
}

crate::register_adapter!(<StateAdapterRegistration> "modules::state::adapters::KvStore", make_adapter);

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_kv_store_adapter_set_get_delete() {
        let builtin_adapter = BuiltinKvStoreAdapter::new(None);

        let group_id = "test_group";
        let item_id = "item1";
        let data = serde_json::json!({"key": "value"});

        // Test set
        builtin_adapter
            .set(group_id, item_id, data.clone())
            .await
            .expect("Set should succeed");

        // Test get
        let saved_data = builtin_adapter
            .get(group_id, item_id)
            .await
            .expect("Get should succeed")
            .expect("Data should exist");

        assert_eq!(saved_data, data);

        // Test delete
        let deleted_data = builtin_adapter
            .get(group_id, item_id)
            .await
            .expect("Get should succeed");
        assert!(deleted_data.is_some());

        builtin_adapter
            .delete(group_id, item_id)
            .await
            .expect("Delete should succeed");

        let deleted_data = builtin_adapter
            .get(group_id, item_id)
            .await
            .expect("Get should succeed");
        assert!(deleted_data.is_none());
    }

    #[tokio::test]
    async fn test_kv_store_adapter_get_group() {
        let builtin_adapter = BuiltinKvStoreAdapter::new(None);
        let group_id = "test_group";
        let item1_id = "item1";
        let item2_id = "item2";
        let data1 = serde_json::json!({"key1": "value1"});
        let data2 = serde_json::json!({"key2": "value2"});
        // Set items
        builtin_adapter
            .set(group_id, item1_id, data1.clone())
            .await
            .expect("Set should succeed");
        builtin_adapter
            .set(group_id, item2_id, data2.clone())
            .await
            .expect("Set should succeed");

        let list = builtin_adapter
            .list(group_id)
            .await
            .expect("List should succeed");
        assert_eq!(list.len(), 2);
        assert!(list.contains(&data1));
        assert!(list.contains(&data2));
    }

    #[tokio::test]
    async fn test_kv_store_adapter_update_item() {
        let builtin_adapter = Arc::new(BuiltinKvStoreAdapter::new(None));
        let group_id = "test_group";
        let item_id = "item1";
        let data1 = serde_json::json!({"key": "value1"});
        let data2 = serde_json::json!({"key": "value2"});

        // Set initial item
        builtin_adapter
            .set(group_id, item_id, data1.clone())
            .await
            .expect("Set should succeed");
        // Update item
        builtin_adapter
            .set(group_id, item_id, data2.clone())
            .await
            .expect("Set should succeed");

        let saved_data = builtin_adapter
            .get(group_id, item_id)
            .await
            .expect("Get should succeed")
            .expect("Data should exist");
        assert_eq!(saved_data, data2);
    }
}
