// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use futures::StreamExt;
use redis::{AsyncCommands, Client, aio::ConnectionManager};
use serde_json::Value;
use tokio::{
    sync::{Mutex, RwLock},
    time::timeout,
};

use crate::{
    engine::Engine,
    modules::{
        redis::DEFAULT_REDIS_CONNECTION_TIMEOUT,
        streams::{
            StreamOutboundMessage, StreamWrapperMessage,
            adapters::{StreamAdapter, StreamConnection},
            registry::{StreamAdapterFuture, StreamAdapterRegistration},
        },
    },
};
use iii_sdk::{UpdateOp, UpdateResult, types::SetResult};

const STREAM_TOPIC: &str = "stream::events";

pub struct RedisAdapter {
    publisher: Arc<Mutex<ConnectionManager>>,
    subscriber: Arc<Client>,
    connections: Arc<RwLock<HashMap<String, Arc<dyn StreamConnection>>>>,
}

impl RedisAdapter {
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
        let subscriber = Arc::new(client);

        Ok(Self {
            publisher,
            subscriber,
            connections: Arc::new(RwLock::new(HashMap::new())),
        })
    }
}

#[async_trait]
impl StreamAdapter for RedisAdapter {
    async fn update(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        ops: Vec<UpdateOp>,
    ) -> Option<UpdateResult> {
        // Use RedisJSON commands for atomic, server-side operations
        // Each JSON.* command is atomic, and we use MULTI/EXEC to make all ops atomic together
        let mut conn = self.publisher.lock().await;
        let key = format!("stream:{}:{}", stream_name, group_id);

        // Get old value first using JSON.GET
        let old_value: Option<Value> = match redis::cmd("JSON.GET")
            .arg(key.clone())
            .arg(item_id)
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

    async fn emit_event(&self, message: StreamWrapperMessage) {
        let mut conn = self.publisher.lock().await;
        tracing::debug!(msg = ?message, "Emitting event to Redis");

        let event_json = match serde_json::to_string(&message) {
            Ok(json) => json,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize event data");
                return;
            }
        };

        if let Err(e) = conn.publish::<_, _, ()>(&STREAM_TOPIC, &event_json).await {
            tracing::error!(error = %e, "Failed to publish event to Redis");
        } else {
            tracing::debug!("Event published to Redis");
        }
    }

    async fn set(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        data: Value,
    ) -> SetResult {
        let key: String = format!("stream:{}:{}", stream_name, group_id);
        let mut conn = self.publisher.lock().await;
        let value = serde_json::to_string(&data).unwrap_or_default();

        let old_value = redis::cmd("HSET")
            .arg(&key)
            .arg(item_id)
            .arg(&value)
            .arg("GET")
            .query_async::<Option<String>>(&mut *conn)
            .await;

        if let Err(e) = old_value {
            tracing::error!(error = %e, stream_name = %stream_name, group_id = %group_id, item_id = %item_id, "Failed to set value in Redis");

            return SetResult {
                old_value: None,
                new_value: Value::Null,
            };
        }

        let old_value = old_value
            .unwrap_or(None)
            .map(|s| serde_json::from_str(&s).unwrap_or(Value::Null));
        let new_value = data.clone();
        let timestamp = chrono::Utc::now().timestamp_millis();

        drop(conn); // Release the lock

        self.emit_event(StreamWrapperMessage {
            timestamp,
            stream_name: stream_name.to_string(),
            group_id: group_id.to_string(),
            id: Some(item_id.to_string()),
            event: if old_value.is_some() {
                StreamOutboundMessage::Update { data }
            } else {
                StreamOutboundMessage::Create { data }
            },
        })
        .await;

        tracing::debug!(stream_name = %stream_name, group_id = %group_id, item_id = %item_id, "Value set in Redis");

        SetResult {
            old_value,
            new_value,
        }
    }

    async fn get(&self, stream_name: &str, group_id: &str, item_id: &str) -> Option<Value> {
        let key = format!("stream:{}:{}", stream_name, group_id);
        let mut conn = self.publisher.lock().await;

        match conn.hget::<_, _, Option<String>>(&key, &item_id).await {
            Ok(Some(s)) => serde_json::from_str(&s).ok(),
            Ok(None) => None,
            Err(e) => {
                tracing::error!(error = %e, stream_name = %stream_name, group_id = %group_id, item_id = %item_id, "Failed to get value from Redis");
                None
            }
        }
    }

    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str) {
        let stream_name = stream_name.to_string();
        let group_id = group_id.to_string();
        let item_id = item_id.to_string();

        let key = format!("stream:{}:{}", stream_name, group_id);
        let mut conn = self.publisher.lock().await;
        let timestamp = chrono::Utc::now().timestamp_millis();

        if let Err(e) = conn.hdel::<String, String, ()>(key, item_id.clone()).await {
            tracing::error!(error = %e, stream_name = %stream_name, group_id = %group_id, item_id = %item_id, "Failed to delete value from Redis");
        }

        drop(conn); // Release the lock

        self.emit_event(StreamWrapperMessage {
            timestamp,
            stream_name: stream_name.to_string(),
            group_id: group_id.to_string(),
            id: Some(item_id.to_string()),
            event: StreamOutboundMessage::Delete {
                data: serde_json::json!({ "id": item_id }),
            },
        })
        .await;
    }

    async fn get_group(&self, stream_name: &str, group_id: &str) -> Vec<Value> {
        let key = format!("stream:{}:{}", stream_name, group_id);
        let mut conn = self.publisher.lock().await;

        match conn.hgetall::<String, HashMap<String, String>>(key).await {
            Ok(values) => values
                .into_values()
                .map(|v| serde_json::from_str(&v).unwrap())
                .collect(),
            Err(e) => {
                tracing::error!(error = %e, stream_name = %stream_name, group_id = %group_id, "Failed to get group from Redis");
                Vec::new()
            }
        }
    }

    async fn list_groups(&self, stream_name: &str) -> Vec<String> {
        let mut conn = self.publisher.lock().await;
        let pattern = format!("stream:{}:*", stream_name);
        let prefix = format!("stream:{}:", stream_name);

        match conn.keys::<_, Vec<String>>(pattern).await {
            Ok(keys) => keys
                .into_iter()
                .filter_map(|key| key.strip_prefix(&prefix).map(|s| s.to_string()))
                .collect(),
            Err(e) => {
                tracing::error!(error = %e, stream_name = %stream_name, "Failed to list groups from Redis");
                Vec::new()
            }
        }
    }

    async fn subscribe(&self, id: String, connection: Arc<dyn StreamConnection>) {
        let mut connections = self.connections.write().await;
        connections.insert(id, connection.clone());
    }

    async fn unsubscribe(&self, id: String) {
        let mut connections = self.connections.write().await;
        connections.remove(&id);
    }

    async fn watch_events(&self) {
        tracing::debug!("Watching events");

        let mut pubsub = match self.subscriber.get_async_pubsub().await {
            Ok(pubsub) => pubsub,
            Err(e) => {
                tracing::error!(error = %e, "Failed to get async pubsub connection");
                return;
            }
        };

        if let Err(e) = pubsub.subscribe(&STREAM_TOPIC).await {
            tracing::error!(error = %e, "Failed to subscribe to Redis channel");
            return;
        }

        let mut msg = pubsub.into_on_message();

        while let Some(msg) = msg.next().await {
            let payload: String = match msg.get_payload() {
                Ok(payload) => payload,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get message payload");
                    continue;
                }
            };

            let connections = self.connections.read().await;
            // Deserialize once, reuse for all connections (optimization)
            let msg: StreamWrapperMessage = match serde_json::from_str(&payload) {
                Ok(msg) => msg,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to parse message as JSON");
                    continue;
                }
            };

            for connection in connections.values() {
                match connection.handle_stream_message(&msg).await {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!(error = ?e, "Failed to handle stream message");
                    }
                }
            }
        }
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::debug!("Destroying RedisAdapter");

        let mut writer = self.connections.write().await;
        let connections = writer.values().collect::<Vec<_>>();

        for connection in connections {
            tracing::info!("Cleaning up connection");
            connection.cleanup().await;
        }

        writer.clear();

        Ok(())
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> StreamAdapterFuture {
    Box::pin(async move {
        let redis_url = config
            .as_ref()
            .and_then(|c| c.get("redis_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("redis://localhost:6379")
            .to_string();
        Ok(Arc::new(RedisAdapter::new(redis_url).await?) as Arc<dyn StreamAdapter>)
    })
}

crate::register_adapter!(<StreamAdapterRegistration> "modules::streams::adapters::RedisAdapter", make_adapter);
