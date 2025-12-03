mod config;
mod logger;

use std::sync::Arc;

use async_trait::async_trait;
pub use config::LoggerModuleConfig;
use function_macros::{function, service};
pub use logger::Logger;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    modules::{configurable::Configurable, core_module::CoreModule},
    protocol::ErrorBody,
};

pub trait LoggerAdapter: Send + Sync + 'static {
    fn info(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &Option<Value>,
    );
    fn warn(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &Option<Value>,
    );
    fn error(
        &self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &Option<Value>,
    );
}

#[derive(Clone)]
pub struct LoggerCoreModule {
    logger: Arc<dyn LoggerAdapter>,
    #[allow(dead_code)]
    config: LoggerModuleConfig,
}

impl Configurable for LoggerCoreModule {
    type Config = LoggerModuleConfig;

    fn with_config(_engine: Arc<Engine>, config: Self::Config) -> Self {
        Self {
            logger: Arc::new(Logger {}),
            config,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct LoggerInput {
    trace_id: Option<String>,
    function_name: String,
    message: String,
    data: Option<Value>,
}

#[service(name = "logger")]
impl LoggerCoreModule {
    #[function(name = "logger.info", description = "Log an info message")]
    pub async fn info(&self, input: LoggerInput) -> Result<Option<Value>, ErrorBody> {
        self.logger.info(
            input.trace_id.as_deref(),
            &input.function_name.as_str(),
            &input.message.as_str(),
            &input.data,
        );

        Ok(Some(Value::Null))
    }

    #[function(name = "logger.warn", description = "Log a warn message")]
    pub async fn warn(&self, input: LoggerInput) -> Result<Option<Value>, ErrorBody> {
        self.logger.warn(
            input.trace_id.as_deref(),
            &input.function_name.as_str(),
            &input.message.as_str(),
            &input.data,
        );

        Ok(Some(Value::Null))
    }

    #[function(name = "logger.error", description = "Log an error message")]
    pub async fn error(&self, input: LoggerInput) -> Result<Option<Value>, ErrorBody> {
        self.logger.error(
            input.trace_id.as_deref(),
            &input.function_name.as_str(),
            &input.message.as_str(),
            &input.data,
        );

        Ok(Some(Value::Null))
    }
}

#[async_trait]
impl CoreModule for LoggerCoreModule {
    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> Result<(), anyhow::Error> {
        Ok(())
    }
}
