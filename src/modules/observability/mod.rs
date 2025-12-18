mod adapters;
mod config;

use std::sync::Arc;

pub use adapters::{FileLogger, RedisLogger};
use async_trait::async_trait;
pub use config::LoggerModuleConfig;
use function_macros::{function, service};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::core_module::CoreModule,
    protocol::ErrorBody,
};

#[derive(Clone, Archive, RkyvSerialize, RkyvDeserialize, Debug, Serialize, Deserialize)]
#[rkyv(compare(PartialEq), derive(Debug))]
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
    async fn save_logs(self, polling_interval: u64, file_path: &str) -> anyhow::Result<()>
    where
        Self: Sized;

    async fn load_logs(&self, file_path: &str) -> Result<Vec<LogEntry>, std::io::Error>
    where
        Self: Sized;

    async fn include_logs(&self, entry: LogEntry);

    fn get_args(&self, args: &Option<Value>) -> String {
        match args {
            Some(v) => v
                .as_object()
                .map(|map| serde_json::to_string(map).unwrap_or_default())
                .unwrap_or_default(),
            None => "{}".to_string(),
        }
    }
    async fn info(
        &mut self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &Option<Value>,
    ) {
        let args_entry = args.as_ref().map(|v| v.clone().to_string());
        let log_entry = LogEntry {
            trace_id: trace_id.map(|s| s.to_string()),
            message: message.to_string(),
            args: args_entry,
            level: "info".to_string(),
            function_name: function_name.to_string(),
            date: chrono::Utc::now().to_rfc3339(),
        };
        self.include_logs(log_entry).await;
        match (trace_id, self.get_args(args)) {
            (Some(tid), data) => {
                tracing::info!(function = %function_name, trace_id = %tid, data = %data, "{}", message);
            }
            (None, data) => {
                tracing::info!(function = %function_name, data = %data, "{}", message);
            }
        }
    }
    async fn warn(
        &mut self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &Option<Value>,
    ) {
        let args_entry = args.as_ref().map(|v| v.clone().to_string());
        let log_entry = LogEntry {
            trace_id: trace_id.map(|s| s.to_string()),
            message: message.to_string(),
            args: args_entry,
            level: "warn".to_string(),
            function_name: function_name.to_string(),
            date: chrono::Utc::now().to_rfc3339(),
        };
        self.include_logs(log_entry).await;
        match (trace_id, self.get_args(args)) {
            (Some(tid), data) => {
                tracing::warn!(function = %function_name, trace_id = %tid, data = %data, "{}", message);
            }
            (None, data) => {
                tracing::warn!(function = %function_name, data = %data, "{}", message);
            }
        }
    }

    async fn error(
        &mut self,
        trace_id: Option<&str>,
        function_name: &str,
        message: &str,
        args: &Option<Value>,
    ) {
        let args_entry = args.as_ref().map(|v| v.clone().to_string());
        let log_entry = LogEntry {
            trace_id: trace_id.map(|s| s.to_string()),
            message: message.to_string(),
            args: args_entry,
            level: "error".to_string(),
            function_name: function_name.to_string(),
            date: chrono::Utc::now().to_rfc3339(),
        };
        self.include_logs(log_entry).await;
        match (trace_id, self.get_args(args)) {
            (Some(tid), data) => {
                tracing::error!(function = %function_name, trace_id = %tid, data = %data, "{}", message);
            }
            (None, data) => {
                tracing::error!(function = %function_name, data = %data, "{}", message);
            }
        }
    }
}

#[derive(Clone)]
pub struct LoggerCoreModule {
    logger: Arc<RwLock<dyn LoggerAdapter>>,
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
        self.logger
            .write()
            .await
            .info(
                input.trace_id.as_deref(),
                input.function_name.as_str(),
                input.message.as_str(),
                &input.data,
            )
            .await;

        FunctionResult::NoResult
    }

    #[function(name = "logger.warn", description = "Log a warn message")]
    pub async fn warn(&self, input: LoggerInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.logger
            .write()
            .await
            .warn(
                input.trace_id.as_deref(),
                input.function_name.as_str(),
                input.message.as_str(),
                &input.data,
            )
            .await;

        FunctionResult::NoResult
    }

    #[function(name = "logger.error", description = "Log an error message")]
    pub async fn error(&self, input: LoggerInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.logger
            .write()
            .await
            .error(
                input.trace_id.as_deref(),
                input.function_name.as_str(),
                input.message.as_str(),
                &input.data,
            )
            .await;

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

        // let logger = Arc::new(RwLock::new(
        //     RedisLogger::new("redis://localhost:6379").await?,
        // ));
        let logger = Arc::new(RwLock::new(FileLogger::new(5, "logs.bin")));
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
