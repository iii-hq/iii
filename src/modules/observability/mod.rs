mod adapters;

mod registry;
use std::sync::RwLock;
mod config;

use std::{collections::HashMap, sync::Arc};

pub use adapters::{FileLogger, RedisLogger};
use async_trait::async_trait;
pub use config::LoggerModuleConfig;
use function_macros::{function, service};
use once_cell::sync::Lazy;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::{
        core_module::{AdapterFactory, ConfigurableModule, CoreModule},
        observability::registry::LoggerAdapterRegistration,
    },
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

    async fn load_logs_object(&self, file_path: &str) -> Result<Vec<LogEntry>, std::io::Error>;

    async fn load_logs(&self, file_path: &str) -> Result<Vec<LogEntry>, std::io::Error>
    where
        Self: Sized,
    {
        self.load_logs_object(file_path).await
    }

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
        &self,
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
        &self,
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
        &self,
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
    adapter: Arc<dyn LoggerAdapter>,
    engine: Arc<Engine>,
    _config: LoggerModuleConfig,
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
    #[function(name = "logger.load_logs", description = "Load logs from file")]
    pub async fn load_logs(&self, file_path: String) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.adapter.load_logs_object(&file_path).await {
            Ok(logs) => {
                let serialized = serde_json::to_value(&logs).unwrap_or(Value::Null);
                FunctionResult::Success(Some(serialized))
            }
            Err(_e) => FunctionResult::NoResult,
        }
    }

    #[function(name = "logger.info", description = "Log an info message")]
    pub async fn info(&self, input: LoggerInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.adapter
            .info(
                input.trace_id.as_deref(),
                input.function_name.as_str(),
                input.message.as_str(),
                &input.data,
            )
            .await;

        self.emit_log_to_stream(&input, "info").await;
        FunctionResult::NoResult
    }

    #[function(name = "logger.warn", description = "Log a warn message")]
    pub async fn warn(&self, input: LoggerInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.adapter
            .warn(
                input.trace_id.as_deref(),
                input.function_name.as_str(),
                input.message.as_str(),
                &input.data,
            )
            .await;

        self.emit_log_to_stream(&input, "warn").await;
        FunctionResult::NoResult
    }

    #[function(name = "logger.error", description = "Log an error message")]
    pub async fn error(&self, input: LoggerInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.adapter
            .error(
                input.trace_id.as_deref(),
                input.function_name.as_str(),
                input.message.as_str(),
                &input.data,
            )
            .await;

        self.emit_log_to_stream(&input, "error").await;
        FunctionResult::NoResult
    }

    async fn emit_log_to_stream(&self, input: &LoggerInput, level: &str) {
        let log_id = Uuid::new_v4().to_string();
        let timestamp = chrono::Utc::now().timestamp_millis();
        let log_data = serde_json::json!({
            "id": log_id,
            "traceId": input.trace_id,
            "step": input.function_name,
            "input": input.data,
            "level": level,
            "time": timestamp,
            "msg": input.message,
        });
        let _ = self
            .engine
            .invoke_function(
                "streams.set",
                serde_json::json!({
                    "stream_name": "iii.logs",
                    "group_id": "all",
                    "item_id": log_id,
                    "data": log_data
                }),
            )
            .await;
    }
}

#[async_trait]
impl CoreModule for LoggerCoreModule {
    fn name(&self) -> &'static str {
        "LoggerModule"
    }
    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl ConfigurableModule for LoggerCoreModule {
    type Config = LoggerModuleConfig;
    type Adapter = dyn LoggerAdapter;
    type AdapterRegistration = LoggerAdapterRegistration;
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::observability::adapters::FileLogger";

    fn adapter_class_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.class.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn LoggerAdapter>>>> =
            Lazy::new(|| RwLock::new(LoggerCoreModule::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self {
            adapter,
            engine,
            _config: config,
        }
    }
}

crate::register_module!(
    "modules::observability::LoggingModule",
    LoggerCoreModule,
    enabled_by_default = true
);
