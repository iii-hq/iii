// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use iii_sdk::{UpdateOp, UpdateResult, types::SetResult};
use redis::{AsyncCommands, Client, aio::ConnectionManager};
use serde_json::Value;
use tokio::{sync::Mutex, time::timeout};

use crate::{
    engine::Engine,
    modules::{
        redis::DEFAULT_REDIS_CONNECTION_TIMEOUT,
        state::{
            adapters::StateAdapter,
            registry::{StateAdapterFuture, StateAdapterRegistration},
        },
    },
};

pub struct StateRedisAdapter {
    publisher: Arc<Mutex<ConnectionManager>>,
}

impl StateRedisAdapter {
    pub async fn new(redis_url: String) -> anyhow::Result<Self> {
        let client = Client::open(redis_url.as_str())?;
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

        let publisher = Arc::new(Mutex::new(manager));
        Ok(Self { publisher })
    }
}

#[async_trait]
impl StateAdapter for StateRedisAdapter {
    async fn set(&self, group_id: &str, item_id: &str, data: Value) -> SetResult {
        let key = format!("state:{}:{}", group_id, item_id);
        let mut conn = self.publisher.lock().await;
        let value = serde_json::to_string(&data).unwrap_or_default();

        let old_value = redis::cmd("HSET")
            .arg(&key)
            .arg(&value)
            .arg("GET")
            .query_async::<Option<String>>(&mut *conn)
            .await;

        if let Err(e) = old_value {
            tracing::error!(error = %e, group_id = %group_id, item_id = %item_id, "Failed to set value in Redis");

            return SetResult {
                old_value: None,
                new_value: Value::Null,
            };
        }

        let old_value = old_value
            .unwrap_or(None)
            .map(|s| serde_json::from_str(&s).unwrap_or(Value::Null));
        let new_value = data.clone();

        SetResult {
            old_value,
            new_value,
        }
    }

    async fn get(&self, group_id: &str, item_id: &str) -> Option<Value> {
        let mut conn = self.publisher.lock().await;

        match conn.hget::<_, _, Option<String>>(&group_id, &item_id).await {
            Ok(Some(s)) => serde_json::from_str(&s).ok(),
            Ok(None) => None,
            Err(e) => {
                tracing::error!(error = %e, group_id = %group_id, item_id = %item_id, "Failed to get value from Redis");
                None
            }
        }
    }

    async fn update(
        &self,
        group_id: &str,
        item_id: &str,
        ops: Vec<UpdateOp>,
    ) -> Option<UpdateResult> {
        // Use RedisJSON commands for atomic, server-side operations
        // Each JSON.* command is atomic, and we use MULTI/EXEC to make all ops atomic together
        let mut conn = self.publisher.lock().await;
        let key = format!("state:{}:{}", group_id, item_id);

        // Get old value first using JSON.GET
        let old_value: Option<Value> = match redis::cmd("JSON.GET")
            .arg(key.clone())
            .arg("$")
            .query_async::<Option<String>>(&mut *conn)
            .await
        {
            Ok(Some(json_array)) => {
                // JSON.GET with $ returns an array, parse and get first element
                serde_json::from_str::<Vec<Value>>(&json_array)
                    .ok()
                    .and_then(|arr| arr.into_iter().next())
            }
            Ok(None) => None,
            Err(e) => {
                // Key doesn't exist or other error - try to initialize it
                tracing::debug!(error = %e, key = %key.clone(), "JSON.GET failed, key may not exist");
                None
            }
        };

        // If key doesn't exist, initialize it with empty object
        if old_value.is_none()
            && let Err(e) = redis::cmd("JSON.SET")
                .arg(key.clone())
                .arg("$")
                .arg("{}")
                .query_async::<()>(&mut *conn)
                .await
        {
            tracing::error!(error = %e, key = %key, "Failed to initialize JSON key");
            return None;
        }

        // Build a pipeline with all RedisJSON operations
        let mut pipe = redis::pipe();
        pipe.atomic(); // Use MULTI/EXEC for atomicity

        for op in &ops {
            match op {
                UpdateOp::Set { path, value } => {
                    let json_path = if path.0.is_empty() {
                        "$".to_string()
                    } else {
                        format!("$.{}", path.0)
                    };
                    let json_value =
                        serde_json::to_string(value).expect("Failed to serialize value");
                    pipe.cmd("JSON.SET")
                        .arg(key.clone())
                        .arg(&json_path)
                        .arg(&json_value)
                        .ignore();
                }
                UpdateOp::Merge { path: _, value } => {
                    // For merge, set each field individually
                    if let Value::Object(map) = value {
                        for (field, val) in map {
                            let json_path = format!("$.{}", field);
                            let json_value =
                                serde_json::to_string(&val).expect("Failed to serialize value");
                            pipe.cmd("JSON.SET")
                                .arg(key.clone())
                                .arg(&json_path)
                                .arg(&json_value)
                                .ignore();
                        }
                    }
                }
                UpdateOp::Increment { path, by } => {
                    let json_path = format!("$.{}", path.0);
                    pipe.cmd("JSON.NUMINCRBY")
                        .arg(key.clone())
                        .arg(&json_path)
                        .arg(*by)
                        .ignore();
                }
                UpdateOp::Decrement { path, by } => {
                    let json_path = format!("$.{}", path.0);
                    pipe.cmd("JSON.NUMINCRBY")
                        .arg(key.clone())
                        .arg(&json_path)
                        .arg(-*by)
                        .ignore();
                }
                UpdateOp::Remove { path } => {
                    let json_path = format!("$.{}", path.0);
                    pipe.cmd("JSON.DEL")
                        .arg(key.clone())
                        .arg(&json_path)
                        .ignore();
                }
            }
        }

        // Execute all operations atomically
        if let Err(e) = pipe.query_async::<()>(&mut *conn).await {
            tracing::error!(error = %e, key = %key, "Failed to execute RedisJSON operations");

            return None;
        }

        // Get new value after operations
        let new_value: Value = match redis::cmd("JSON.GET")
            .arg(key.clone())
            .arg("$")
            .query_async::<Option<String>>(&mut *conn)
            .await
        {
            Ok(Some(json_array)) => serde_json::from_str::<Vec<Value>>(&json_array)
                .ok()
                .and_then(|arr| arr.into_iter().next())
                .unwrap_or(Value::Null),
            Ok(None) => Value::Null,
            Err(e) => {
                tracing::error!(error = %e, key = %key, "Failed to get new value after update");
                Value::Null
            }
        };

        Some(UpdateResult {
            old_value,
            new_value,
        })
    }

    async fn delete(&self, group_id: &str, item_id: &str) {
        let mut conn = self.publisher.lock().await;

        if let Err(e) = conn
            .hdel::<String, String, ()>(group_id.into(), item_id.into())
            .await
        {
            tracing::error!(error = %e, group_id = %group_id, item_id = %item_id, "Failed to delete value from Redis");
        }
    }

    async fn list(&self, group_id: &str) -> Vec<Value> {
        let mut conn = self.publisher.lock().await;

        match conn
            .hgetall::<String, HashMap<String, String>>(group_id.into())
            .await
        {
            Ok(values) => values
                .into_values()
                .map(|v| serde_json::from_str(&v).unwrap())
                .collect(),
            Err(e) => {
                tracing::error!(error = %e, group_id = %group_id, "Failed to get group from Redis");
                Vec::new()
            }
        }
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::debug!("Destroying StateRedisAdapter");
        Ok(())
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> StateAdapterFuture {
    Box::pin(async move {
        let redis_url = config
            .as_ref()
            .and_then(|c| c.get("redis_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("redis://localhost:6379")
            .to_string();
        Ok(Arc::new(StateRedisAdapter::new(redis_url).await?) as Arc<dyn StateAdapter>)
    })
}

crate::register_adapter!(<StateAdapterRegistration> "modules::state::adapters::RedisAdapter", make_adapter);
