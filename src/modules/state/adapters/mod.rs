pub mod bridge;
pub mod kv_store;
pub mod redis_adapter;

use async_trait::async_trait;
use iii_sdk::{UpdateOp, UpdateResult, types::SetResult};
use serde_json::Value;

#[async_trait]
pub trait StateAdapter: Send + Sync {
    async fn set(&self, group_id: &str, item_id: &str, data: Value) -> SetResult;
    async fn get(&self, group_id: &str, item_id: &str) -> Option<Value>;
    async fn delete(&self, group_id: &str, item_id: &str);
    async fn update(
        &self,
        group_id: &str,
        item_id: &str,
        ops: Vec<UpdateOp>,
    ) -> Option<UpdateResult>;
    async fn list(&self, group_id: &str) -> Vec<Value>;
    async fn destroy(&self) -> anyhow::Result<()>;
}
