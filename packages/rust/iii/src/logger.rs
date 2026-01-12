use serde_json::{Value, json};
use std::sync::Arc;

pub type LoggerInvoker = Arc<dyn Fn(&str, Value) + Send + Sync>;

#[derive(Clone, Default)]
pub struct Logger {
    invoker: Option<LoggerInvoker>,
    trace_id: String,
    function_name: String,
}

impl Logger {
    pub fn new(invoker: Option<LoggerInvoker>, trace_id: Option<String>, function_name: Option<String>) -> Self {
        Self {
            invoker,
            trace_id: trace_id.unwrap_or_default(),
            function_name: function_name.unwrap_or_default(),
        }
    }

    fn build_params(&self, message: &str, data: Option<Value>) -> Value {
        json!({
            "message": message,
            "trace_id": self.trace_id,
            "function_name": self.function_name,
            "data": data,
        })
    }

    pub fn info(&self, message: &str, data: Option<Value>) {
        if let Some(invoker) = &self.invoker {
            invoker("logger.info", self.build_params(message, data));
        }
        tracing::info!(function = %self.function_name, message = %message);
    }

    pub fn warn(&self, message: &str, data: Option<Value>) {
        if let Some(invoker) = &self.invoker {
            invoker("logger.warn", self.build_params(message, data));
        }
        tracing::warn!(function = %self.function_name, message = %message);
    }

    pub fn error(&self, message: &str, data: Option<Value>) {
        if let Some(invoker) = &self.invoker {
            invoker("logger.error", self.build_params(message, data));
        }
        tracing::error!(function = %self.function_name, message = %message);
    }
}
