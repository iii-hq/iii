mod config;
mod logger;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
pub use config::LoggerModuleConfig;
use function_macros::{function, service};
pub use logger::Logger;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::core_module::CoreModule,
    protocol::ErrorBody,
};

#[derive(Clone, Archive, RkyvSerialize, RkyvDeserialize, Debug)]
pub struct LogEntry {
    trace_id: Option<String>,
    message: String,
    args: Option<String>,
    level: String,
    function_name: String,
    date: String,
}

#[async_trait]
pub trait LoggerAdapter: Send + Sync + 'static {
    async fn save_logs(logs: Arc<Mutex<Vec<LogEntry>>>, polling_interval: u64)
    where
        Self: Sized;
    fn info(
        &mut self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &Option<Value>,
    );
    fn warn(
        &mut self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &Option<Value>,
    );
    fn error(
        &mut self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &Option<Value>,
    );
}

#[derive(Clone)]
pub struct LoggerCoreModule {
    logger: Arc<Mutex<dyn LoggerAdapter>>,
    #[allow(dead_code)]
    config: LoggerModuleConfig,
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
    pub async fn info(&self, input: LoggerInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.logger.lock().unwrap().info(
            input.trace_id.as_deref(),
            input.function_name.as_str(),
            input.message.as_str(),
            &input.data,
        );

        FunctionResult::NoResult
    }

    #[function(name = "logger.warn", description = "Log a warn message")]
    pub async fn warn(&self, input: LoggerInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.logger.lock().unwrap().warn(
            input.trace_id.as_deref(),
            input.function_name.as_str(),
            input.message.as_str(),
            &input.data,
        );

        FunctionResult::NoResult
    }

    #[function(name = "logger.error", description = "Log an error message")]
    pub async fn error(&self, input: LoggerInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.logger.lock().unwrap().error(
            input.trace_id.as_deref(),
            input.function_name.as_str(),
            input.message.as_str(),
            &input.data,
        );

        FunctionResult::NoResult
    }
}

#[async_trait]
impl CoreModule for LoggerCoreModule {
    async fn create(
        _engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        let config: LoggerModuleConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let logger = Arc::new(Mutex::new(Logger::new(60)));
        Ok(Box::new(Self { config, logger }))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

crate::register_module!(
    "modules::observability::LoggingModule",
    <LoggerCoreModule as CoreModule>::make_module,
    enabled_by_default = true
);
