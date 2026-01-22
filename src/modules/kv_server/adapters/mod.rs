use async_trait::async_trait;
use serde_json::Value;

use crate::builtins::kv::{BuiltinKvStore, SetResult};

use super::structs::{UpdateOp, UpdateResult};

#[async_trait]
pub trait KVStoreAdapter: Send + Sync {
    async fn set(&self, key: String, value: Value, config: Option<Value>) -> SetResult;
    async fn get(&self, key: String, config: Option<Value>) -> Option<Value>;
    async fn delete(&self, key: String) -> Option<Value>;
    async fn exists(&self, key: String) -> bool;
    async fn list_keys_with_prefix(&self, prefix: String) -> Vec<String>;
    async fn list(&self, key: String) -> Vec<Value>;

    async fn update(&self, stream_name: &str, ops: Vec<UpdateOp>) -> UpdateResult;

    async fn destroy(&self) -> anyhow::Result<()>;
}

#[async_trait]
impl KVStoreAdapter for BuiltinKvStore {
    async fn set(&self, key: String, value: Value, _config: Option<Value>) -> SetResult {
        self.set(key, value).await
    }

    async fn get(&self, key: String, _config: Option<Value>) -> Option<Value> {
        self.get(key).await
    }

    async fn delete(&self, key: String) -> Option<Value> {
        self.delete(key).await
    }

    async fn exists(&self, key: String) -> bool {
        self.exists(key).await
    }

    async fn update(&self, key: &str, ops: Vec<UpdateOp>) -> UpdateResult {
        match self.update(key.to_string(), ops).await {
            Some(result) => result,
            None => UpdateResult {
                old_value: None,
                new_value: serde_json::Value::Null,
            },
        }
    }

    async fn list_keys_with_prefix(&self, prefix: String) -> Vec<String> {
        self.list_keys_with_prefix(prefix).await
    }

    async fn list(&self, key: String) -> Vec<Value> {
        self.list(key).await
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
