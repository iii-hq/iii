mod adapters;
mod config;
#[allow(clippy::module_inception)]
mod event;
pub mod registry;

use serde_json::Value;

pub use self::event::EventCoreModule;

#[async_trait::async_trait]
pub trait EventAdapter: Send + Sync + 'static {
    async fn emit(&self, topic: &str, event_data: Value);
    async fn subscribe(
        &self,
        topic: &str,
        id: &str,
        function_path: &str,
        condition_function_path: Option<String>,
    );
    async fn unsubscribe(&self, topic: &str, id: &str);
}
