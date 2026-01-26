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
        let key: String = format!("state:{}", group_id);
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
            Ok(old) => old.map(|s| serde_json::from_str(&s).unwrap_or(Value::Null)),
            Err(e) => {
                tracing::error!(error = %e, group_id = %group_id, item_id = %item_id, "Failed to atomically set value in Redis");
                return SetResult {
                    old_value: None,
                    new_value: Value::Null,
                };
            }
        };
        let new_value = data.clone();

        SetResult {
            old_value,
            new_value,
        }
    }

    async fn get(&self, group_id: &str, item_id: &str) -> Option<Value> {
        let key = format!("state:{}", group_id);
        let mut conn = self.publisher.lock().await;

        match conn.hget::<_, _, Option<String>>(&key, &item_id).await {
            Ok(Some(s)) => serde_json::from_str(&s).ok(),
            Ok(None) => None,
            Err(e) => {
                tracing::error!(error = %e, group_id = %group_id, item_id = %item_id, "Failed to get value from Redis");
                None
            }
        }
    }

    async fn update(&self, group_id: &str, item_id: &str, ops: Vec<UpdateOp>) -> UpdateResult {
        let mut conn = self.publisher.lock().await;
        let key = format!("state:{}", group_id);

        // Serialize operations to JSON
        let ops_json = match serde_json::to_string(&ops) {
            Ok(json) => json,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize update operations");
                return UpdateResult {
                    old_value: None,
                    new_value: Value::Null,
                };
            }
        };

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
                    return self.update_rust_based(group_id, item_id, ops).await;
                }

                if values.len() == 3 {
                    let old_value = if values[1].is_empty() {
                        None
                    } else {
                        serde_json::from_str(&values[1]).ok()
                    };

                    let new_value = serde_json::from_str(&values[2]).unwrap_or(Value::Null);

                    UpdateResult {
                        old_value,
                        new_value,
                    }
                } else {
                    tracing::error!("Unexpected return value from update script");
                    UpdateResult {
                        old_value: None,
                        new_value: Value::Null,
                    }
                }
            }
            Err(e) => {
                // If script fails, try Rust-based fallback
                tracing::debug!(error = %e, "Lua script failed, falling back to Rust-based update");
                self.update_rust_based(group_id, item_id, ops).await
            }
            _ => {
                tracing::error!("Unexpected return value from update script");
                UpdateResult {
                    old_value: None,
                    new_value: Value::Null,
                }
            }
        }
    }

    async fn delete(&self, group_id: &str, item_id: &str) {
        let key = format!("state:{}", group_id);
        let mut conn = self.publisher.lock().await;

        if let Err(e) = conn.hdel::<_, String, ()>(&key, item_id.to_string()).await {
            tracing::error!(error = %e, group_id = %group_id, item_id = %item_id, "Failed to delete value from Redis");
        }
    }

    async fn list(&self, group_id: &str) -> Vec<Value> {
        let key = format!("state:{}", group_id);
        let mut conn = self.publisher.lock().await;

        match conn.hgetall::<String, HashMap<String, String>>(key).await {
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

impl StateRedisAdapter {
    // Fallback method that does JSON manipulation in Rust
    async fn update_rust_based(
        &self,
        group_id: &str,
        item_id: &str,
        ops: Vec<UpdateOp>,
    ) -> UpdateResult {
        let mut conn = self.publisher.lock().await;
        let key = format!("state:{}", group_id);

        // Simple atomic get-and-set approach
        // Get old value
        let old_value_str: Option<String> =
            match conn.hget::<_, _, Option<String>>(&key, item_id).await {
                Ok(val) => val,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get old value");
                    return UpdateResult {
                        old_value: None,
                        new_value: Value::Null,
                    };
                }
            };

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
                        if let Some(existing_val) = map.get_mut(&path.0) {
                            if let Some(num) = existing_val.as_i64() {
                                *existing_val = Value::Number(serde_json::Number::from(num + by));
                            }
                        } else {
                            map.insert(
                                path.0.clone(),
                                Value::Number(serde_json::Number::from(*by)),
                            );
                        }
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
        let new_value_str = match serde_json::to_string(&updated_value) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize");
                return UpdateResult {
                    old_value: None,
                    new_value: Value::Null,
                };
            }
        };

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

        UpdateResult {
            old_value,
            new_value: updated_value,
        }
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
