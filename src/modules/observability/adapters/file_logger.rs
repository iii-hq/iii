// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use async_trait::async_trait;
use rkyv::rancor::Error;
use serde_json::Value;
use tokio::{
    fs::OpenOptions,
    io::{AsyncReadExt, AsyncWriteExt},
    sync::RwLock,
};

use crate::{
    engine::Engine,
    modules::observability::{
        LogEntry, LoggerAdapter, LoggerAdapterRegistration, registry::LoggerAdapterFuture,
    },
};

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
    pub fn new(config: Option<Value>) -> Self {
        let five_seconds_ms = 5 * 1000;
        let save_interval_ms = config
            .as_ref()
            .and_then(|c| c.get("save_interval_ms").and_then(|v| v.as_u64()))
            .unwrap_or(five_seconds_ms);
        let file_path = config
            .as_ref()
            .and_then(|c| c.get("file_path").and_then(|v| v.as_str()))
            .unwrap_or("app.log");

        let logs = Arc::new(RwLock::new(Vec::new()));
        let logs_for_task = logs.clone();

        let clonned_self = Self {
            logs: logs_for_task,
        };
        let file_path = file_path.to_string();
        tokio::spawn(async move {
            let _ = clonned_self.save_logs(save_interval_ms, &file_path).await;
        });
        Self { logs }
    }

    async fn drain_logs(&self) -> Vec<LogEntry> {
        let mut logs_guard = self.logs.write().await;
        std::mem::take(&mut *logs_guard)
    }

    async fn restore_logs_front(&self, drained: Vec<LogEntry>) {
        if drained.is_empty() {
            return;
        }

        let mut logs_guard = self.logs.write().await;
        if logs_guard.is_empty() {
            *logs_guard = drained;
            return;
        }

        let current = std::mem::take(&mut *logs_guard);
        let mut merged = drained;
        merged.extend(current);
        *logs_guard = merged;
    }

    fn encode_chunk(logs: &Vec<LogEntry>) -> Vec<u8> {
        let payload = rkyv::to_bytes::<Error>(logs).unwrap();
        let payload = payload.as_ref();

        let len: u32 = payload
            .len()
            .try_into()
            .expect("Serialized log chunk too large");

        let mut out = Vec::with_capacity(4 + payload.len());
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    async fn append_chunk(file_path: &str, chunk: &[u8]) -> anyhow::Result<()> {
        let exists = tokio::fs::metadata(file_path).await.is_ok();

        let mut file = if exists {
            OpenOptions::new().append(true).open(file_path).await?
        } else {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(file_path)
                .await?
        };

        file.write_all(chunk).await?;
        file.flush().await?;
        Ok(())
    }

    fn decode_chunks(bytes: &[u8]) -> Result<Vec<LogEntry>, std::io::Error> {
        let mut offset = 0usize;
        let mut out = Vec::new();

        while offset < bytes.len() {
            if bytes.len() - offset < 4 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid log file: truncated chunk header",
                ));
            }

            let len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;

            if bytes.len() - offset < len {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid log file: truncated chunk payload",
                ));
            }

            let payload = &bytes[offset..offset + len];
            offset += len;

            let mut chunk_logs = rkyv::from_bytes::<Vec<LogEntry>, Error>(payload)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            out.append(&mut chunk_logs);
        }

        Ok(out)
    }
}

#[async_trait]
impl LoggerAdapter for FileLogger {
    async fn save_logs(self, polling_interval: u64, file_path: &str) -> anyhow::Result<()> {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(polling_interval));
        loop {
            interval.tick().await;
            let drained = self.drain_logs().await;
            if drained.is_empty() {
                continue;
            }

            let chunk = Self::encode_chunk(&drained);
            tracing::debug!("Saving {} bytes of logs to {}", chunk.len(), file_path);

            if let Err(e) = Self::append_chunk(file_path, &chunk).await {
                tracing::error!("Failed to save logs to {}: {}", file_path, e);
                self.restore_logs_front(drained).await;
            }
        }
    }

    async fn include_logs(&self, entry: LogEntry) {
        self.logs.write().await.push(entry);
    }
    async fn load_logs_object(&self, file_path: &str) -> Result<Vec<LogEntry>, std::io::Error> {
        let mut file = tokio::fs::File::open(file_path).await?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).await?;
        Self::decode_chunks(&bytes)
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> LoggerAdapterFuture {
    Box::pin(async move { Ok(Arc::new(FileLogger::new(config)) as Arc<dyn LoggerAdapter>) })
}

crate::register_adapter!(<LoggerAdapterRegistration> "modules::observability::adapters::FileLogger", make_adapter);
