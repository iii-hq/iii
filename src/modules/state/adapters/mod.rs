pub mod kv_store;
pub mod redis_adapter;

use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait StateAdapter: Send + Sync {
    async fn set(&self, group_id: &str, item_id: &str, data: Value);
    async fn get(&self, group_id: &str, item_id: &str) -> Option<Value>;
    async fn delete(&self, group_id: &str, item_id: &str);
    async fn list(&self, group_id: &str) -> Vec<Value>;
    async fn destroy(&self) -> anyhow::Result<()>;
}
