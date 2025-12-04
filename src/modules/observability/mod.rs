mod config;
mod logger;

use std::{pin::Pin, sync::Arc};

use async_trait::async_trait;
pub use config::LoggerModuleConfig;
use futures::Future;
pub use logger::Logger;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    engine::{Engine, EngineTrait, RegisterFunctionRequest},
    function::FunctionHandler,
    modules::core_module::CoreModule,
    protocol::ErrorBody,
};

pub trait LoggerAdapter: Send + Sync + 'static {
    fn info(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &[(&str, &Value)],
    );
    fn warn(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &[(&str, &Value)],
    );
    fn error(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &[(&str, &Value)],
    );
}

#[derive(Clone)]
pub struct LoggerCoreModule {
    engine: Arc<Engine>,
    logger: Arc<dyn LoggerAdapter>,
    #[allow(dead_code)]
    config: LoggerModuleConfig,
}

impl FunctionHandler for LoggerCoreModule {
    fn handle_function<'a>(
        &'a self,
        _invocation_id: Option<Uuid>,
        function_path: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, ErrorBody>> + Send + 'a>> {
        Box::pin(async move {
            let logger = self.logger.clone();
            let function_path = function_path.clone();
            let input = input;

            let trace_id = input.get("trace_id").and_then(|v| v.as_str());
            let function_name = input
                .get("function_name")
                .and_then(|v: &Value| v.as_str())
                .unwrap_or_default();
            let message = input
                .get("message")
                .and_then(|v: &Value| v.as_str())
                .unwrap_or_default();
            // The args is coming as an object, not an array of arrays.
            let data: Vec<(&str, &Value)> = input
                .get("data")
                .and_then(|v: &Value| v.as_object())
                .map(|map| map.iter().map(|(k, v)| (k.as_str(), v)).collect())
                .unwrap_or_default();

            if function_path == "logger.info" {
                logger.info(trace_id, function_name, message, &data);
            } else if function_path == "logger.warn" {
                logger.warn(trace_id, function_name, message, &data);
            } else if function_path == "logger.error" {
                logger.error(trace_id, function_name, message, &data);
            }

            Ok(Some(serde_json::Value::Null))
        })
    }
}

#[async_trait]
impl CoreModule for LoggerCoreModule {
    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        let config: LoggerModuleConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let logger = Arc::new(Logger {});
        Ok(Box::new(Self {
            engine,
            config,
            logger,
        }))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        let _ = self.engine.register_function(
            RegisterFunctionRequest {
                function_path: "logger.info".to_string(),
                description: Some("Log an info message".to_string()),
                request_format: None,
                response_format: None,
            },
            Box::new(self.clone()),
        );
        let _ = self.engine.register_function(
            RegisterFunctionRequest {
                function_path: "logger.warn".to_string(),
                description: Some("Log a warn message".to_string()),
                request_format: None,
                response_format: None,
            },
            Box::new(self.clone()),
        );
        let _ = self.engine.register_function(
            RegisterFunctionRequest {
                function_path: "logger.error".to_string(),
                description: Some("Log an error message".to_string()),
                request_format: None,
                response_format: None,
            },
            Box::new(self.clone()),
        );

        Ok(())
    }
}
