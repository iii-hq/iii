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
            StreamMetadata, StreamWrapperMessage,
            adapters::{StreamAdapter, StreamConnection},
            registry::{StreamAdapterFuture, StreamAdapterRegistration},
        },
    },
};
use iii_sdk::{
    UpdateOp, UpdateResult,
    types::{DeleteResult, SetResult},
};

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
    ) -> anyhow::Result<UpdateResult> {
        let mut conn = self.publisher.lock().await;
        let key = format!("stream:{}:{}", stream_name, group_id);

        // Serialize operations to JSON
        let ops_json = serde_json::to_string(&ops)
            .map_err(|e| anyhow::anyhow!("Failed to serialize update operations: {}", e))?;

        // Use a single Lua script that atomically gets, applies operations, and sets
        // Try using cjson as a global (available in most Redis installations)
        // If cjson is not available, fall back to Rust-based approach
        let script = redis::Script::new(
            r#"
                -- Try to use cjson if available (as global, not via require)
                local json_decode, json_encode
                if cjson then
                    json_decode = cjson.decode
                    json_encode = cjson.encode
                else
                    -- Fallback: return error so Rust can handle it
                    return {false, 'cjson not available'}
                end
                
                local key = KEYS[1]
                local field = ARGV[1]
                local ops_json = ARGV[2]
                
                -- Get old value
                local old_value_str = redis.call('HGET', key, field)
                local old_value = {}
                if old_value_str then
                    local ok, decoded = pcall(json_decode, old_value_str)
                    if ok then
                        old_value = decoded
                    end
                end
                
                -- Parse operations
                local ops = json_decode(ops_json)
                local current = json_decode(json_encode(old_value)) -- Deep copy
                
                -- Helper to extract path string
                local function get_path(path)
                    if path == nil then
                        return nil
                    end
                    if type(path) == 'string' then
                        return path
                    end
                    if type(path) == 'table' then
                        if path[1] then
                            return path[1]
                        end
                        if path['0'] then
                            return path['0']
                        end
                    end
                    return path
                end
                
                -- Apply all operations
                for i, op in ipairs(ops) do
                    if op.type == 'set' then
                        local path = get_path(op.path)
                        if (path == '' or path == nil) and op.value ~= nil then
                            current = op.value
                        else
                            if type(current) ~= 'table' or current == nil then
                                current = {}
                            end
                            current[path] = op.value or cjson.null
                        end
                    elseif op.type == 'merge' then
                        local path = get_path(op.path)
                        if (path == nil or path == '') and type(current) == 'table' and type(op.value) == 'table' then
                            for k, v in pairs(op.value) do
                                current[k] = v
                            end
                        end
                    elseif op.type == 'increment' then
                        local path = get_path(op.path)
                        if type(current) ~= 'table' or current == nil then
                            current = {}
                        end
                        local val = current[path]
                        if type(val) == 'number' then
                            current[path] = val + op.by
                        else
                            current[path] = op.by
                        end
                    elseif op.type == 'decrement' then
                        local path = get_path(op.path)
                        if type(current) ~= 'table' or current == nil then
                            current = {}
                        end
                        local val = current[path]
                        if type(val) == 'number' then
                            current[path] = val - op.by
                        else
                            current[path] = -op.by
                        end
                    elseif op.type == 'remove' then
                        local path = get_path(op.path)
                        if type(current) == 'table' and current ~= nil then
                            current[path] = nil
                        end
                    end
                end
                
                -- Set new value
                local new_value_str = json_encode(current)
                redis.call('HSET', key, field, new_value_str)
                
                -- Return [success, old_value_json, new_value_json]
                return {true, old_value_str or '', new_value_str}
            "#,
        );

        let result: redis::RedisResult<Vec<String>> = script
            .key(&key)
            .arg(item_id)
            .arg(&ops_json)
            .invoke_async(&mut *conn)
            .await;

        match result {
            Ok(values) if values.len() >= 2 => {
                // Check if cjson was available
                if values[0] == "false" {
                    tracing::warn!(values = ?values, "cjson not available, falling back to Rust-based update");

                    // Fall back to Rust-based approach
                    return self
                        .update_rust_based(stream_name, group_id, item_id, ops)
                        .await;
                }

                if values.len() == 3 {
                    let old_value = if values[1].is_empty() {
                        None
                    } else {
                        Some(serde_json::from_str::<Value>(&values[1]).map_err(|e| {
                            anyhow::anyhow!("Failed to deserialize old value: {}", e)
                        })?)
                    };

                    let new_value = serde_json::from_str(&values[2])
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize new value: {}", e))?;

                    Ok(UpdateResult {
                        old_value,
                        new_value,
                    })
                } else {
                    Err(anyhow::anyhow!(
                        "Unexpected return value from update script: expected 3 values, got {}",
                        values.len()
                    ))
                }
            }
            Err(e) => {
                // If script fails, try Rust-based fallback
                tracing::debug!(error = %e, "Lua script failed, falling back to Rust-based update");
                self.update_rust_based(stream_name, group_id, item_id, ops)
                    .await
            }
            _ => Err(anyhow::anyhow!(
                "Unexpected return value from update script"
            )),
        }
    }

    async fn emit_event(&self, message: StreamWrapperMessage) -> anyhow::Result<()> {
        let mut conn = self.publisher.lock().await;
        tracing::debug!(msg = ?message, "Emitting event to Redis");

        let event_json = serde_json::to_string(&message)
            .map_err(|e| anyhow::anyhow!("Failed to serialize event data: {}", e))?;

        conn.publish::<_, _, ()>(&STREAM_TOPIC, &event_json)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to publish event to Redis: {}", e))?;

        tracing::debug!("Event published to Redis");
        Ok(())
    }

    async fn set(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        data: Value,
    ) -> anyhow::Result<SetResult> {
        let key: String = format!("stream:{}:{}", stream_name, group_id);
        let mut conn = self.publisher.lock().await;
        let value = serde_json::to_string(&data).unwrap_or_default();

        // Use Lua script for atomic get-and-set operation
        // This script atomically gets the old value and sets the new value
        let script = redis::Script::new(
            r#"
                local old_value = redis.call('HGET', KEYS[1], ARGV[1])
                redis.call('HSET', KEYS[1], ARGV[1], ARGV[2])
                return old_value
            "#,
        );

        let result: redis::RedisResult<Option<String>> = script
            .key(&key)
            .arg(item_id)
            .arg(&value)
            .invoke_async(&mut *conn)
            .await;

        let old_value = match result {
            Ok(old) => old
                .map(|s| {
                    serde_json::from_str(&s)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize old value: {}", e))
                })
                .transpose()?,
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Failed to atomically set value in Redis: {}",
                    e
                ));
            }
        };
        let new_value = data.clone();

        tracing::debug!(stream_name = %stream_name, group_id = %group_id, item_id = %item_id, "Value set in Redis");

        Ok(SetResult {
            old_value,
            new_value,
        })
    }

    async fn get(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
    ) -> anyhow::Result<Option<Value>> {
        let key = format!("stream:{}:{}", stream_name, group_id);
        let mut conn = self.publisher.lock().await;

        match conn.hget::<_, _, Option<String>>(&key, &item_id).await {
            Ok(Some(s)) => serde_json::from_str(&s)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize value: {}", e))
                .map(Some),
            Ok(None) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("Failed to get value from Redis: {}", e)),
        }
    }

    async fn delete(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
    ) -> anyhow::Result<DeleteResult> {
        let stream_name = stream_name.to_string();
        let group_id = group_id.to_string();
        let item_id = item_id.to_string();

        let key = format!("stream:{}:{}", stream_name, group_id);

        // Use Lua script for atomic get-and-delete operation
        // This script atomically gets the old value and deletes the field
        let script = redis::Script::new(
            r#"
                local old_value = redis.call('HGET', KEYS[1], ARGV[1])
                redis.call('HDEL', KEYS[1], ARGV[1])
                return old_value
            "#,
        );

        let result: redis::RedisResult<Option<String>> = script
            .key(&key)
            .arg(&item_id)
            .invoke_async(&mut *self.publisher.lock().await)
            .await;

        let old_value = match result {
            Ok(old) => old
                .map(|s| {
                    serde_json::from_str(&s)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize old value: {}", e))
                })
                .transpose()?,
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Failed to atomically delete value from Redis: {}",
                    e
                ));
            }
        };

        Ok(DeleteResult { old_value })
    }

    async fn get_group(&self, stream_name: &str, group_id: &str) -> anyhow::Result<Vec<Value>> {
        let key = format!("stream:{}:{}", stream_name, group_id);
        let mut conn = self.publisher.lock().await;

        match conn.hgetall::<String, HashMap<String, String>>(key).await {
            Ok(values) => {
                let mut result = Vec::new();
                for v in values.into_values() {
                    match serde_json::from_str(&v) {
                        Ok(value) => result.push(value),
                        Err(e) => {
                            return Err(anyhow::anyhow!(
                                "Failed to deserialize value in group: {}",
                                e
                            ));
                        }
                    }
                }
                Ok(result)
            }
            Err(e) => Err(anyhow::anyhow!("Failed to get group from Redis: {}", e)),
        }
    }

    async fn list_groups(&self, stream_name: &str) -> anyhow::Result<Vec<String>> {
        let mut conn = self.publisher.lock().await;
        let pattern = format!("stream:{}:*", stream_name);
        let prefix = format!("stream:{}:", stream_name);

        match conn.keys::<_, Vec<String>>(pattern).await {
            Ok(keys) => Ok(keys
                .into_iter()
                .filter_map(|key| key.strip_prefix(&prefix).map(|s| s.to_string()))
                .collect()),
            Err(e) => Err(anyhow::anyhow!("Failed to list groups from Redis: {}", e)),
        }
    }

    async fn list_all_streams(&self) -> anyhow::Result<Vec<StreamMetadata>> {
        use std::collections::{HashMap, HashSet};

        let mut conn = self.publisher.lock().await;
        let pattern = "stream:*";
        let mut stream_map: HashMap<String, HashSet<String>> = HashMap::new();

        // Use SCAN instead of KEYS to avoid blocking Redis
        let mut cursor = 0u64;
        loop {
            let result: (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(pattern)
                .query_async(&mut *conn)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to scan streams from Redis: {}", e))?;

            let (next_cursor, keys) = result;

            // Parse keys: stream:<stream_name>:<group_id>
            for key in keys {
                let parts: Vec<&str> = key.split(':').collect();
                if parts.len() >= 3 && parts[0] == "stream" {
                    let stream_name = parts[1].to_string();
                    let group_id = parts[2].to_string();

                    stream_map.entry(stream_name).or_default().insert(group_id);
                }
            }

            cursor = next_cursor;
            if cursor == 0 {
                break;
            }
        }

        // Convert to StreamMetadata
        let mut streams: Vec<StreamMetadata> = stream_map
            .into_iter()
            .map(|(id, groups)| {
                let mut groups_vec: Vec<String> = groups.into_iter().collect();
                groups_vec.sort();
                StreamMetadata {
                    id,
                    groups: groups_vec,
                }
            })
            .collect();

        streams.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(streams)
    }

    async fn subscribe(
        &self,
        id: String,
        connection: Arc<dyn StreamConnection>,
    ) -> anyhow::Result<()> {
        let mut connections = self.connections.write().await;
        connections.insert(id, connection.clone());
        Ok(())
    }

    async fn unsubscribe(&self, id: String) -> anyhow::Result<()> {
        let mut connections = self.connections.write().await;
        connections.remove(&id);
        Ok(())
    }

    async fn watch_events(&self) -> anyhow::Result<()> {
        tracing::debug!("Watching events");

        let mut pubsub = self
            .subscriber
            .get_async_pubsub()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get async pubsub connection: {}", e))?;

        pubsub
            .subscribe(&STREAM_TOPIC)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to subscribe to Redis channel: {}", e))?;

        let mut msg = pubsub.into_on_message();

        loop {
            let msg = match msg.next().await {
                Some(msg) => msg,
                None => break,
            };
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
        Ok(())
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

impl RedisAdapter {
    // Fallback method that does JSON manipulation in Rust
    async fn update_rust_based(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        ops: Vec<UpdateOp>,
    ) -> anyhow::Result<UpdateResult> {
        let mut conn = self.publisher.lock().await;
        let key = format!("stream:{}:{}", stream_name, group_id);

        // Simple atomic get-and-set approach
        // Get old value
        let old_value_str: Option<String> = conn
            .hget::<_, _, Option<String>>(&key, item_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get old value: {}", e))?;

        // Parse and apply operations
        let old_value: Option<Value> = old_value_str
            .as_ref()
            .and_then(|s| serde_json::from_str(s).ok());

        let mut updated_value = old_value
            .clone()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

        // Apply operations (same logic as before)
        for op in &ops {
            match op {
                UpdateOp::Set { path, value } => {
                    if path.0.is_empty() && value.is_some() {
                        updated_value = value.clone().unwrap();
                    } else if let Value::Object(ref mut map) = updated_value {
                        map.insert(path.0.clone(), value.clone().unwrap_or(Value::Null));
                    }
                }
                UpdateOp::Merge { path, value } => {
                    if (path.is_none() || path.as_ref().unwrap().0.is_empty())
                        && let (Value::Object(existing_map), Value::Object(new_map)) =
                            (&mut updated_value, value)
                    {
                        for (k, v) in new_map {
                            existing_map.insert(k.clone(), v.clone());
                        }
                    }
                }
                UpdateOp::Increment { path, by } => {
                    if let Value::Object(ref mut map) = updated_value {
                        let next = match map.get(&path.0).and_then(|v| v.as_i64()) {
                            Some(num) => num + by,
                            None => *by, // non-numeric or missing => set to by
                        };
                        map.insert(
                            path.0.clone(),
                            Value::Number(serde_json::Number::from(next)),
                        );
                    }
                }
                UpdateOp::Decrement { path, by } => {
                    if let Value::Object(ref mut map) = updated_value {
                        if let Some(existing_val) = map.get_mut(&path.0) {
                            if let Some(num) = existing_val.as_i64() {
                                *existing_val = Value::Number(serde_json::Number::from(num - by));
                            }
                        } else {
                            map.insert(
                                path.0.clone(),
                                Value::Number(serde_json::Number::from(-*by)),
                            );
                        }
                    }
                }
                UpdateOp::Remove { path } => {
                    if let Value::Object(ref mut map) = updated_value {
                        map.remove(&path.0);
                    }
                }
            }
        }

        // Serialize and atomically set
        let new_value_str = serde_json::to_string(&updated_value)
            .map_err(|e| anyhow::anyhow!("Failed to serialize: {}", e))?;

        let script = redis::Script::new(
            r#"
                local old_value = redis.call('HGET', KEYS[1], ARGV[1])
                redis.call('HSET', KEYS[1], ARGV[1], ARGV[2])
                return old_value or ''
            "#,
        );

        let _: redis::RedisResult<String> = script
            .key(&key)
            .arg(item_id)
            .arg(&new_value_str)
            .invoke_async(&mut *conn)
            .await;

        Ok(UpdateResult {
            old_value,
            new_value: updated_value,
        })
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
