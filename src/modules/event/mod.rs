pub mod adapters;
mod config;
mod event;

use axum::async_trait;
use serde_json::Value;

pub use self::event::EventCoreModule;

#[async_trait]
pub trait EventAdapter: Send + Sync + 'static {
    async fn emit(&self, topic: &str, event_data: Value);
    async fn subscribe(&self, topic: &str, id: &str, function_path: &str);
    async fn unsubscribe(&self, topic: &str, id: &str);
}
