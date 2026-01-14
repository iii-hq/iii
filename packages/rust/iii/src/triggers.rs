use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

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

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use super::*;

    #[test]
    fn trigger_unregister_calls_closure() {
        let called = Arc::new(AtomicBool::new(false));
        let called_ref = called.clone();
        let trigger = Trigger::new(Arc::new(move || {
            called_ref.store(true, Ordering::SeqCst);
        }));

        trigger.unregister();

        assert!(called.load(Ordering::SeqCst));
    }
}
