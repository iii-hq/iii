// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use async_trait::async_trait;
use redis::{AsyncCommands, Client, aio::ConnectionManager};
use serde_json::Value;
use tokio::{
    sync::{Mutex, RwLock},
    time::timeout,
};

use crate::{
    engine::Engine,
    modules::{
        observability::{
            LogEntry, LoggerAdapter, LoggerAdapterRegistration, registry::LoggerAdapterFuture,
        },
        redis::DEFAULT_REDIS_CONNECTION_TIMEOUT,
    },
};

pub struct RedisLogger {
    connection_manager: Arc<Mutex<ConnectionManager>>,
    logs: Arc<RwLock<Vec<LogEntry>>>,
}

impl RedisLogger {
    pub async fn new(url: &str) -> anyhow::Result<Self> {
        let connection_manager = Self::get_connection(url).await?;
        let logs = Arc::new(RwLock::new(Vec::new()));
        Ok(Self {
            connection_manager,
            logs,
        })
    }

    async fn get_connection(url: &str) -> anyhow::Result<Arc<Mutex<ConnectionManager>>> {
        let client = Client::open(url)?;
        let manager = timeout(
            DEFAULT_REDIS_CONNECTION_TIMEOUT,
            client.get_connection_manager(),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Redis connection timed out after {:?}. Please ensure Redis is running at: {}",
                DEFAULT_REDIS_CONNECTION_TIMEOUT,
                url
            )
        })?
        .map_err(|e| anyhow::anyhow!("Failed to connect to Redis at {}: {}", url, e))?;

        Ok(Arc::new(Mutex::new(manager)))
    }
}

#[async_trait]
impl LoggerAdapter for RedisLogger {
    async fn load_logs_object(&self, _file_path: &str) -> Result<Vec<LogEntry>, std::io::Error> {
        let mut conn = self.connection_manager.lock().await;
        let log_strings: Vec<String> = conn.lrange("logs", 0, -1).await.unwrap_or_default();
        let mut logs = Vec::new();
        for log_str in log_strings {
            if let Ok(entry) = serde_json::from_str::<LogEntry>(&log_str) {
                logs.push(entry);
            } else {
                tracing::warn!("Failed to deserialize log entry : {}", log_str);
            }
        }
        Ok(logs)
    }

    async fn include_logs(&self, entry: LogEntry) {
        let mut conn = self.connection_manager.lock().await;
        let log_data = serde_json::to_string(&entry).unwrap_or_default();
        let _: () = conn.rpush("logs", log_data).await.unwrap_or(());
        self.logs.write().await.push(entry);
    }

    async fn save_logs(self, polling_interval: u64, _file_path: &str) -> anyhow::Result<()> {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(polling_interval));
        loop {
            interval.tick().await;
            let logs_guard = self.logs.read().await;
            if logs_guard.is_empty() {
                continue;
            }
            let mut conn = self.connection_manager.lock().await;
            for log in logs_guard.iter() {
                let log_data = serde_json::to_string(log).unwrap_or_default();
                let _: () = conn.rpush("logs", log_data).await?;
            }
            drop(logs_guard);
            self.logs.write().await.clear();
        }
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> LoggerAdapterFuture {
    Box::pin(async move {
        let redis_url = config
            .as_ref()
            .and_then(|c| c.get("redis_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("redis://localhost:6379");
        Ok(Arc::new(RedisLogger::new(redis_url).await?) as Arc<dyn LoggerAdapter>)
    })
}

crate::register_adapter!(<LoggerAdapterRegistration> "modules::observability::adapters::RedisLogger", make_adapter);
