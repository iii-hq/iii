use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::Error;
use crate::protocol::ErrorBody;

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
    /// Resolved namespace the trigger's target `function_id` uses. Current SDKs
    /// fill an omitted registration value from the registering worker's
    /// namespace. A provider that stores this config and later calls
    /// `trigger()` must pass it through; `None` is the legacy/default case.
    pub namespace: Option<String>,
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
    registration_error_fn: Option<Arc<dyn Fn() -> Option<ErrorBody> + Send + Sync>>,
}

impl Trigger {
    pub fn new(unregister_fn: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            unregister_fn,
            registration_error_fn: None,
        }
    }

    /// Attach the source this handle reads its rejection from. Separate from
    /// [`new`](Trigger::new) so the existing constructor keeps its signature.
    pub fn with_registration_error(
        mut self,
        registration_error_fn: Arc<dyn Fn() -> Option<ErrorBody> + Send + Sync>,
    ) -> Self {
        self.registration_error_fn = Some(registration_error_fn);
        self
    }

    /// Remove this trigger from the engine.
    pub fn unregister(&self) {
        (self.unregister_fn)();
    }

    /// The engine's rejection of this binding, if one arrived; `None`
    /// otherwise. Registration is asynchronous and only failures are acked, so
    /// `None` means "no failure reported yet", not "confirmed live".
    ///
    /// The common cause is `trigger_type_not_found` from a boot-order race:
    /// the binding was requested before the provider registered the trigger
    /// type. A reconnect re-sends the registration and clears this.
    ///
    /// To confirm a binding IS live, call `engine::registered-triggers::list`
    /// with `trigger_type`, `function_id`, and `namespace`.
    pub fn registration_error(&self) -> Option<ErrorBody> {
        self.registration_error_fn.as_ref().and_then(|f| f())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
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

    /// A handle built without a source reports nothing rather than panicking:
    /// `Trigger::new` stays usable on its own, which is what keeps its
    /// signature unchanged for existing callers.
    #[test]
    fn registration_error_is_none_without_a_source() {
        let trigger = Trigger::new(Arc::new(|| {}));

        assert!(trigger.registration_error().is_none());
    }

    /// The handle must read through to the live map. A snapshot taken at
    /// construction would stay `None` forever, since the ack always arrives
    /// after `register_trigger` has returned.
    #[test]
    fn registration_error_reads_through_to_its_source() {
        let slot: Arc<Mutex<Option<ErrorBody>>> = Arc::new(Mutex::new(None));
        let reader = slot.clone();
        let trigger = Trigger::new(Arc::new(|| {}))
            .with_registration_error(Arc::new(move || reader.lock().unwrap().clone()));

        assert!(trigger.registration_error().is_none());

        *slot.lock().unwrap() = Some(ErrorBody {
            code: "trigger_type_not_found".to_string(),
            message: "Trigger type not found".to_string(),
            stacktrace: None,
        });

        let err = trigger.registration_error().expect("error after the ack");
        assert_eq!(err.code, "trigger_type_not_found");
    }
}
