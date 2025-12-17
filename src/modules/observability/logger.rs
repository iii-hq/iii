use std::sync::{Arc, Mutex};

use rkyv::{Archive, Deserialize, Serialize, rancor::Error};
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use super::LoggerAdapter;

/// Logger implementation that uses tracing for output.
/// This maintains compatibility with the LoggerAdapter trait while
/// leveraging the centralized tracing formatter for consistent output.
///
/// The `function_name` parameter is passed as the `function` field to tracing,
/// which the formatter will use as the display name instead of the module path.
#[derive(Debug, Clone)]
pub struct Logger {
    logs: Arc<Mutex<Vec<LogEntry>>>,
}

#[derive(Clone, Archive, Serialize, Deserialize, Debug)]
struct LogEntry {
    trace_id: Option<String>,
    message: String,
    args: Option<String>,
    level: String,
    function_name: String,
    date: String,
}

impl Logger {
    pub fn new(save_interval: u64) -> Self {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let logs_for_task = Arc::clone(&logs);

        tokio::spawn(async move {
            Self::save_at_disk(logs_for_task, save_interval).await;
        });

        Self { logs }
    }

    async fn save_at_disk(logs: Arc<Mutex<Vec<LogEntry>>>, polling_interval: u64) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(polling_interval));
        loop {
            interval.tick().await;
            let bytes = Self::serialize_logs(&logs);
            if !bytes.is_empty() {
                tracing::debug!("Saving logs to disk...");
                let path = "logs.bin";
                let mut file = tokio::fs::File::create(path).await.unwrap();
                file.write_all(&bytes).await.unwrap();
                file.flush().await.unwrap();
            } else {
                tracing::debug!("No logs to save.");
            }
        }
    }

    fn serialize_logs(logs: &Arc<Mutex<Vec<LogEntry>>>) -> Vec<u8> {
        let logs_guard = logs.lock().unwrap();
        let bytes = rkyv::to_bytes::<Error>(&*logs_guard).unwrap();
        bytes.to_vec()
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new(60)
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
    fn save_log(&self, entry: LogEntry) {
        self.logs.lock().unwrap().push(entry);
    }
}

impl LoggerAdapter for Logger {
    fn info(
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
        self.save_log(log_entry);
        match (trace_id, &get_args(args)) {
            (Some(tid), data) => {
                tracing::info!(function = %function_name, trace_id = %tid, data = %data, "{}", message);
            }
            (None, data) => {
                tracing::info!(function = %function_name, data = %data, "{}", message);
            }
        }
    }

    fn warn(
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
        self.save_log(log_entry);
        match (trace_id, &get_args(args)) {
            (Some(tid), data) => {
                tracing::warn!(function = %function_name, trace_id = %tid, data = %data, "{}", message);
            }
            (None, data) => {
                tracing::warn!(function = %function_name, data = %data, "{}", message);
            }
        }
    }

    fn error(
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
        self.save_log(log_entry);
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
