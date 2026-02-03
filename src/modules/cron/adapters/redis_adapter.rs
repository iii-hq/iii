// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use async_trait::async_trait;
use redis::{Client, aio::ConnectionManager};
use serde_json::Value;
use tokio::time::timeout;

use super::super::structs::CronSchedulerAdapter;
use crate::{
    engine::Engine,
    modules::{
        cron::registry::{CronAdapterFuture, CronAdapterRegistration},
        redis::DEFAULT_REDIS_CONNECTION_TIMEOUT,
    },
};

/// Default lock TTL for distributed cron locking (in milliseconds)
const CRON_LOCK_TTL_MS: u64 = 30_000; // 30 seconds

/// Prefix for cron lock keys in Redis
const CRON_LOCK_PREFIX: &str = "cron_lock:";

/// Redis-based distributed lock for cron jobs
pub struct RedisCronLock {
    connection: Arc<tokio::sync::Mutex<ConnectionManager>>,
    instance_id: String,
}

impl RedisCronLock {
    pub async fn new(redis_url: &str) -> anyhow::Result<Self> {
        let client = Client::open(redis_url)?;

        let manager = timeout(
            DEFAULT_REDIS_CONNECTION_TIMEOUT,
            client.get_connection_manager(),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Redis connection timed out after {:?}. Please ensure Redis is running at: {}",
                DEFAULT_REDIS_CONNECTION_TIMEOUT,
                redis_url
            )
        })?
        .map_err(|e| anyhow::anyhow!("Failed to connect to Redis at {}: {}", redis_url, e))?;

        // Generate a unique instance ID for this engine instance
        let instance_id = uuid::Uuid::new_v4().to_string();

        Ok(Self {
            connection: Arc::new(tokio::sync::Mutex::new(manager)),
            instance_id,
        })
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> CronAdapterFuture {
    Box::pin(async move {
        let redis_url = config
            .as_ref()
            .and_then(|c| c.get("redis_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("redis://localhost:6379")
            .to_string();
        Ok(Arc::new(RedisCronLock::new(&redis_url).await?) as Arc<dyn CronSchedulerAdapter>)
    })
}

crate::register_adapter!(<CronAdapterRegistration> "modules::cron::RedisCronAdapter", make_adapter);

#[async_trait]
impl CronSchedulerAdapter for RedisCronLock {
    async fn try_acquire_lock(&self, job_id: &str) -> bool {
        let lock_key = format!("{}{}", CRON_LOCK_PREFIX, job_id);
        let mut conn = self.connection.lock().await;

        // Use SET with NX (only if not exists) and PX (expiry in milliseconds)
        let result: redis::RedisResult<Option<String>> = redis::cmd("SET")
            .arg(&lock_key)
            .arg(&self.instance_id)
            .arg("NX")
            .arg("PX")
            .arg(CRON_LOCK_TTL_MS)
            .query_async(&mut *conn)
            .await;

        match result {
            Ok(Some(_)) => {
                tracing::debug!(job_id = %job_id, instance_id = %self.instance_id, "Acquired cron lock");
                true
            }
            Ok(None) => {
                tracing::debug!(job_id = %job_id, "Failed to acquire cron lock - another instance holds it");
                false
            }
            Err(e) => {
                tracing::error!(job_id = %job_id, error = %e, "Error acquiring cron lock");
                false
            }
        }
    }

    async fn release_lock(&self, job_id: &str) {
        let lock_key = format!("{}{}", CRON_LOCK_PREFIX, job_id);
        let mut conn = self.connection.lock().await;

        // Only release if we own the lock (using Lua script for atomicity)
        let script = r#"
            if redis.call("get", KEYS[1]) == ARGV[1] then
                return redis.call("del", KEYS[1])
            else
                return 0
            end
        "#;

        let result: redis::RedisResult<i32> = redis::Script::new(script)
            .key(&lock_key)
            .arg(&self.instance_id)
            .invoke_async(&mut *conn)
            .await;

        match result {
            Ok(1) => {
                tracing::debug!(job_id = %job_id, "Released cron lock");
            }
            Ok(_) => {
                tracing::debug!(job_id = %job_id, "Lock was not held by this instance");
            }
            Err(e) => {
                tracing::error!(job_id = %job_id, error = %e, "Error releasing cron lock");
            }
        }
    }
}
