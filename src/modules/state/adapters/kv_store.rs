use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    builtins::BuiltinKvStore,
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

    async fn set(&self, group_id: &str, item_id: &str, data: Value) {
        let key: String = group_id.into();

        if let Some(value) = self.storage.get(key.clone()).await {
            let mut topic: HashMap<String, Value> =
                serde_json::from_value(value).unwrap_or_default();
            topic.insert(item_id.to_string(), data.clone());
            let data = serde_json::to_value(&topic).unwrap();
            self.storage.set(key, data).await;
        } else {
            let mut topic = HashMap::new();
            topic.insert(item_id.to_string(), data.clone());
            let data = serde_json::to_value(&topic).unwrap();
            self.storage.set(key, data).await;
        }
    }

    async fn get(&self, group_id: &str, item_id: &str) -> Option<Value> {
        let value = self.storage.get(group_id.into()).await;
        match value {
            Some(v) => {
                let topic: HashMap<String, Value> = serde_json::from_value(v).unwrap_or_default();
                topic.get(item_id).cloned()
            }
            None => None,
        }
    }

    async fn delete(&self, group_id: &str, item_id: &str) {
        let value = self.storage.get(group_id.into()).await;
        if let Some(v) = value {
            let mut topic: HashMap<String, Value> = serde_json::from_value(v).unwrap_or_default();

            if topic.remove(item_id).is_some() {
                let data = serde_json::to_value(&topic).unwrap();
                self.storage.set(group_id.into(), data).await;
            }
        }
    }

    async fn list(&self, group_id: &str) -> Vec<Value> {
        self.storage.get(group_id.into()).await.map_or(vec![], |v| {
            let topic: HashMap<String, Value> = serde_json::from_value(v).unwrap_or_default();
            topic.values().cloned().collect()
        })
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
        builtin_adapter.set(group_id, item_id, data.clone()).await;

        // Test get
        let saved_data = builtin_adapter
            .get(group_id, item_id)
            .await
            .expect("Data should exist");

        assert_eq!(saved_data, data);

        // Test delete
        let deleted_data = builtin_adapter.get(group_id, item_id).await;
        assert!(deleted_data.is_some());

        builtin_adapter.delete(group_id, item_id).await;

        let deleted_data = builtin_adapter.get(group_id, item_id).await;
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
        builtin_adapter.set(group_id, item1_id, data1.clone()).await;
        builtin_adapter.set(group_id, item2_id, data2.clone()).await;

        let list = builtin_adapter.list(group_id).await;
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
        builtin_adapter.set(group_id, item_id, data1.clone()).await;
        // Update item
        builtin_adapter.set(group_id, item_id, data2.clone()).await;

        let saved_data = builtin_adapter
            .get(group_id, item_id)
            .await
            .expect("Data should exist");
        assert_eq!(saved_data, data2);
    }
}
