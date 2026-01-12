use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::error::BridgeError;

#[derive(Debug, Clone)]
pub struct TriggerConfig {
    pub id: String,
    pub function_path: String,
    pub config: Value,
}

#[async_trait]
pub trait TriggerHandler: Send + Sync {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), BridgeError>;
    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), BridgeError>;
}

#[derive(Clone)]
pub struct Trigger {
    unregister_fn: Arc<dyn Fn() + Send + Sync>,
}

impl Trigger {
    pub fn new(unregister_fn: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { unregister_fn }
    }

    pub fn unregister(&self) {
        (self.unregister_fn)();
    }
}
