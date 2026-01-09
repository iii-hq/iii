use async_trait::async_trait;
use serde_json::Value;

use crate::builtins::BuiltinKvStore;

#[async_trait]
pub trait KVStoreAdapter: Send + Sync {
    async fn set(&self, key: String, value: Value, config: Option<Value>);
    async fn get(&self, key: String, config: Option<Value>) -> Option<Value>;
    async fn delete(&self, key: String) -> Option<Value>;
    async fn exists(&self, key: String) -> bool;

    async fn destroy(&self) -> anyhow::Result<()>;
}

#[async_trait]
impl KVStoreAdapter for BuiltinKvStore {
    async fn set(&self, key: String, value: Value, _config: Option<Value>) {
        self.set(key, value).await;
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

    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
