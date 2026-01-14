use std::sync::Arc;

use serde_json::{Value, json};

pub type LoggerInvoker = Arc<dyn Fn(&str, Value) + Send + Sync>;

#[derive(Clone, Default)]
pub struct Logger {
    invoker: Option<LoggerInvoker>,
    trace_id: String,
    function_name: String,
}

impl Logger {
    pub fn new(
        invoker: Option<LoggerInvoker>,
        trace_id: Option<String>,
        function_name: Option<String>,
    ) -> Self {
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn logger_invokes_with_expected_payload() {
        let calls: Arc<Mutex<Vec<(String, Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_ref = calls.clone();

        let invoker: LoggerInvoker = Arc::new(move |path, params| {
            calls_ref.lock().unwrap().push((path.to_string(), params));
        });

        let logger = Logger::new(
            Some(invoker),
            Some("trace-1".to_string()),
            Some("function-a".to_string()),
        );

        logger.info("hello", Some(json!({ "key": "value" })));
        logger.warn("warn", None);
        logger.error("oops", Some(json!(123)));

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0, "logger.info");
        assert_eq!(calls[1].0, "logger.warn");
        assert_eq!(calls[2].0, "logger.error");

        assert_eq!(calls[0].1["message"], "hello");
        assert_eq!(calls[0].1["trace_id"], "trace-1");
        assert_eq!(calls[0].1["function_name"], "function-a");
        assert_eq!(calls[0].1["data"]["key"], "value");

        assert!(calls[1].1["data"].is_null());
        assert_eq!(calls[2].1["data"], 123);
    }
}
