use std::sync::Arc;

use async_trait::async_trait;
use rkyv::rancor::Error;
use serde_json::Value;
use tokio::{io::AsyncWriteExt, sync::RwLock};

use super::{LogEntry, LoggerAdapter};

/// Logger implementation that uses tracing for output.
/// This maintains compatibility with the LoggerAdapter trait while
/// leveraging the centralized tracing formatter for consistent output.
///
/// The `function_name` parameter is passed as the `function` field to tracing,
/// which the formatter will use as the display name instead of the module path.
#[derive(Debug, Clone)]
pub struct Logger {
    logs: Arc<RwLock<Vec<LogEntry>>>,
}

impl Logger {
    pub fn new(save_interval: u64, file_path: &str) -> Self {
        let logs = Arc::new(RwLock::new(Vec::new()));
        let logs_for_task = logs.clone();

        let file_path = file_path.to_string();
        tokio::spawn(async move {
            let _ = Self::save_logs(logs_for_task, save_interval, &file_path).await;
        });

        Self { logs }
    }

    async fn serialize_logs(logs: &Arc<RwLock<Vec<LogEntry>>>) -> Vec<u8> {
        let logs_guard = logs.read().await;
        if logs_guard.is_empty() {
            return Vec::new();
        }
        let bytes = rkyv::to_bytes::<Error>(&*logs_guard).unwrap();
        bytes.to_vec()
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new(60, "logs.bin")
    }
}

fn get_args(args: &Option<Value>) -> String {
    match args {
        Some(v) => v
            .as_object()
            .map(|map| serde_json::to_string(map).unwrap_or_default())
            .unwrap_or_default(),
        None => "{}".to_string(),
    }
}

impl Logger {
    async fn include_logs(&self, entry: LogEntry) {
        self.logs.write().await.push(entry);
    }
}

#[async_trait]
impl LoggerAdapter for Logger {
    async fn save_logs(
        logs: Arc<RwLock<Vec<LogEntry>>>,
        polling_interval: u64,
        file_path: &str,
    ) -> anyhow::Result<()> {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(polling_interval));
        loop {
            interval.tick().await;
            let bytes = Self::serialize_logs(&logs).await;
            if !bytes.is_empty() {
                let mut file = tokio::fs::File::create(file_path).await?;
                file.write_all(&bytes).await?;
                file.flush().await?;
            } else {
                tracing::debug!("No logs to save.");
            }
        }
    }
    async fn load_logs(file_path: &str) -> Result<Vec<LogEntry>, std::io::Error> {
        let bytes = tokio::fs::read(file_path).await?;
        rkyv::from_bytes::<Vec<LogEntry>, Error>(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
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
        match (trace_id, &get_args(args)) {
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
        match (trace_id, &get_args(args)) {
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
        match (trace_id, &get_args(args)) {
            (Some(tid), data) => {
                tracing::error!(function = %function_name, trace_id = %tid, data = %data, "{}", message);
            }
            (None, data) => {
                tracing::error!(function = %function_name, data = %data, "{}", message);
            }
        }
    }
}
