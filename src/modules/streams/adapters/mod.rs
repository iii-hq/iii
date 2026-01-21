pub mod bridge;
pub mod kv_store;
pub mod redis_adapter;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::modules::streams::StreamWrapperMessage;

#[async_trait]
pub trait StreamAdapter: Send + Sync {
    async fn set(&self, stream_name: &str, group_id: &str, item_id: &str, data: Value);
    async fn get(&self, stream_name: &str, group_id: &str, item_id: &str) -> Option<Value>;
    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str);
    async fn get_group(&self, stream_name: &str, group_id: &str) -> Vec<Value>;
    async fn list_groups(&self, stream_name: &str) -> Vec<String>;

    async fn emit_event(&self, message: StreamWrapperMessage);

    async fn subscribe(&self, id: String, connection: Arc<dyn StreamConnection>);
    async fn unsubscribe(&self, id: String);
    async fn watch_events(&self);

    async fn destroy(&self) -> anyhow::Result<()>;
}

#[async_trait]
pub trait StreamConnection: Send + Sync {
    async fn handle_stream_message(&self, msg: &StreamWrapperMessage) -> anyhow::Result<()>;
    async fn cleanup(&self);
}
