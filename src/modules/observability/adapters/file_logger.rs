use std::sync::Arc;

use async_trait::async_trait;
use rkyv::rancor::Error;
use tokio::{io::AsyncWriteExt, sync::RwLock};

use crate::modules::observability::{LogEntry, LoggerAdapter};

/// Logger implementation that uses tracing for output.
/// This maintains compatibility with the LoggerAdapter trait while
/// leveraging the centralized tracing formatter for consistent output.
///
/// The `function_name` parameter is passed as the `function` field to tracing,
/// which the formatter will use as the display name instead of the module path.
#[derive(Debug, Clone)]
pub struct FileLogger {
    logs: Arc<RwLock<Vec<LogEntry>>>,
}

impl FileLogger {
    pub fn new(save_interval: u64, file_path: &str) -> Self {
        let logs = Arc::new(RwLock::new(Vec::new()));
        let logs_for_task = logs.clone();

        let clonned_self = Self {
            logs: logs_for_task,
        };
        let file_path = file_path.to_string();
        tokio::spawn(async move {
            let _ = clonned_self.save_logs(save_interval, &file_path).await;
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

impl Default for FileLogger {
    fn default() -> Self {
        Self::new(60, "logs.bin")
    }
}

#[async_trait]
impl LoggerAdapter for FileLogger {
    async fn save_logs(self, polling_interval: u64, file_path: &str) -> anyhow::Result<()> {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(polling_interval));
        loop {
            interval.tick().await;
            let bytes = Self::serialize_logs(&self.logs).await;
            if !bytes.is_empty() {
                tracing::debug!("Saving {} bytes of logs to {}", bytes.len(), file_path);
                let file = tokio::fs::File::create(file_path).await;
                if let Err(e) = file {
                    tracing::error!("Failed to create log file {}: {}", file_path, e);
                    continue;
                }
                let mut file = file.expect("Failed to create log file");
                file.write_all(&bytes).await?;
                file.flush().await?;
            } else {
                tracing::debug!("No logs to save.");
            }
        }
    }

    async fn include_logs(&self, entry: LogEntry) {
        self.logs.write().await.push(entry);
    }
    async fn load_logs(&self, file_path: &str) -> Result<Vec<LogEntry>, std::io::Error> {
        let bytes = tokio::fs::read(file_path).await?;
        rkyv::from_bytes::<Vec<LogEntry>, Error>(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}
