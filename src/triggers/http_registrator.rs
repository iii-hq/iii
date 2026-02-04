use std::{pin::Pin, sync::Arc};

use futures::Future;
use serde_json::Value;

use crate::{
    function::FunctionsRegistry,
    invocation::http_invoker::HttpInvoker,
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator},
};

#[derive(Clone)]
pub struct HttpTriggerRegistrator {
    http_invoker: Arc<HttpInvoker>,
    functions: Arc<FunctionsRegistry>,
}

impl HttpTriggerRegistrator {
    pub fn new(http_invoker: Arc<HttpInvoker>, functions: Arc<FunctionsRegistry>) -> Self {
        Self {
            http_invoker,
            functions,
        }
    }

    pub async fn deliver(&self, trigger: &Trigger, payload: Value) -> Result<(), ErrorBody> {
        let function = self.functions.get(&trigger.function_path)
            .ok_or_else(|| ErrorBody {
                code: "function_not_found".into(),
                message: format!("Function '{}' not found", trigger.function_path),
            })?;

        self.http_invoker
            .deliver_webhook(&function, &trigger.trigger_type, &trigger.id, payload)
            .await
    }
}

impl TriggerRegistrator for HttpTriggerRegistrator {
    fn register_trigger(
        &self,
        _trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn unregister_trigger(
        &self,
        _trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }
}
