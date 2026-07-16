use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::Error;

/// Configuration passed to a [`TriggerHandler`] when a trigger instance is
/// registered or unregistered.
#[derive(Debug, Clone)]
pub struct TriggerConfig {
    /// Trigger instance ID.
    pub id: String,
    /// Function to invoke when the trigger fires.
    pub function_id: String,
    /// Trigger-specific configuration.
    pub config: Value,
    /// Arbitrary user-specifiable metadata supplied to the triggered handler function on every invocation.
    pub metadata: Option<Value>,
}

/// Handler trait for custom trigger types. Implement this and pass to
/// [`IIIClient::register_trigger_type`](crate::IIIClient::register_trigger_type).
#[async_trait]
pub trait TriggerHandler: Send + Sync {
    /// Called when a trigger instance is registered.
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), Error>;
    /// Called when a trigger instance is unregistered.
    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), Error>;
}

/// Handle returned by [`IIIClient::register_trigger`](crate::IIIClient::register_trigger).
/// Call [`unregister`](Trigger::unregister) to remove the trigger from the engine.
#[derive(Clone)]
pub struct Trigger {
    unregister_fn: Arc<dyn Fn() + Send + Sync>,
}

impl Trigger {
    pub fn new(unregister_fn: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { unregister_fn }
    }

    /// Remove this trigger from the engine.
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
