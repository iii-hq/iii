pub mod emit;
pub mod redis_adapter;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

pub use self::redis_adapter::RedisAdapter;
use crate::modules::streams::StreamWrapperMessage;

#[async_trait]
pub trait StreamAdapter: Send + Sync {
    async fn set(&self, stream_name: &str, group_id: &str, item_id: &str, data: Value);
    async fn get(&self, stream_name: &str, group_id: &str, item_id: &str) -> Option<Value>;
    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str);
    async fn get_group(&self, stream_name: &str, group_id: &str) -> Vec<Value>;

    async fn subscribe(&self, id: String, connection: Arc<dyn StreamConnection>);
    async fn unsubscribe(&self, id: String);
}

#[async_trait]
pub trait StreamConnection: Send + Sync {
    async fn handle_stream_message(&self, msg: &StreamWrapperMessage) -> anyhow::Result<()>;
}
