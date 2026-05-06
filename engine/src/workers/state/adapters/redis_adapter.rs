// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use iii_sdk::{UpdateOp, UpdateResult, types::SetResult};
use redis::{AsyncCommands, Client, aio::ConnectionManager};
use serde_json::Value;
use tokio::{sync::Mutex, time::timeout};

use crate::{
    engine::Engine,
    workers::{
        redis::{DEFAULT_REDIS_CONNECTION_TIMEOUT, JSON_UPDATE_SCRIPT},
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
    async fn set(&self, scope: &str, key: &str, value: Value) -> anyhow::Result<SetResult> {
        let scope_key: String = format!("state:{}", scope);
        let mut conn = self.publisher.lock().await;
        let serialized = serde_json::to_string(&value)
            .map_err(|e| anyhow::anyhow!("Failed to serialize value: {}", e))?;

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
            .key(&scope_key)
            .arg(key)
            .arg(&serialized)
            .invoke_async(&mut *conn)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to atomically set value in Redis: {}", e))?;

        match result {
            Ok(s) => {
                let old_value = s.map(|s| serde_json::from_str(&s).unwrap_or(Value::Null));
                let new_value = value.clone();

                Ok(SetResult {
                    old_value,
                    new_value,
                })
            }
            Err(e) => Err(anyhow::anyhow!(
                "Failed to atomically set value in Redis: {}",
                e
            )),
        }
    }

    async fn get(&self, scope: &str, key: &str) -> anyhow::Result<Option<Value>> {
        let scope_key = format!("state:{}", scope);
        let mut conn = self.publisher.lock().await;

        match conn.hget::<_, _, Option<String>>(&scope_key, &key).await {
            Ok(Some(s)) => serde_json::from_str(&s)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize value from Redis: {}", e))
                .map(Some),
            Ok(None) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("Failed to get value from Redis: {}", e)),
        }
    }

    async fn update(
        &self,
        scope: &str,
        key: &str,
        ops: Vec<UpdateOp>,
    ) -> anyhow::Result<UpdateResult> {
        let mut conn = self.publisher.lock().await;
        let scope_key = format!("state:{}", scope);

        // Serialize operations to JSON
        let ops_json = serde_json::to_string(&ops)
            .map_err(|e| anyhow::anyhow!("Failed to serialize update operations: {}", e))?;

        // Use a single Lua script that atomically gets, applies operations, and sets.
        let script = redis::Script::new(JSON_UPDATE_SCRIPT);

        let result: redis::RedisResult<Vec<String>> = script
            .key(&scope_key)
            .arg(key)
            .arg(&ops_json)
            .invoke_async(&mut *conn)
            .await;

        match result {
            Ok(values) if values.len() >= 2 => {
                // Check if the Lua update script reported a failure.
                if values[0] == "false" {
                    return Err(anyhow::anyhow!(
                        "Redis atomic update script failed: {}",
                        values.get(1).map_or("unknown error", String::as_str)
                    ));
                }

                if values.len() == 3 || values.len() == 4 {
                    let old_value = if values[1].is_empty() {
                        None
                    } else {
                        serde_json::from_str(&values[1]).map_err(|e| {
                            anyhow::anyhow!("Failed to deserialize old value: {}", e)
                        })?
                    };

                    let new_value = serde_json::from_str(&values[2])
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize new value: {}", e))?;

                    let errors = if values.len() == 4 && !values[3].is_empty() {
                        serde_json::from_str(&values[3]).map_err(|e| {
                            anyhow::anyhow!("Failed to deserialize update errors: {}", e)
                        })?
                    } else {
                        Vec::new()
                    };

                    Ok(UpdateResult {
                        old_value,
                        new_value,
                        errors,
                    })
                } else {
                    Err(anyhow::anyhow!(
                        "Unexpected return value from update script: expected 3 or 4 values, got {}",
                        values.len()
                    ))
                }
            }
            Err(e) => Err(anyhow::anyhow!("Redis atomic update script failed: {}", e)),
            _ => Err(anyhow::anyhow!(
                "Unexpected return value from update script"
            )),
        }
    }

    async fn delete(&self, scope: &str, key: &str) -> anyhow::Result<()> {
        let scope_key = format!("state:{}", scope);
        let mut conn = self.publisher.lock().await;

        conn.hdel::<_, String, ()>(&scope_key, key.to_string())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete value from Redis: {}", e))?;
        Ok(())
    }

    async fn list(&self, scope: &str) -> anyhow::Result<Vec<Value>> {
        let scope_key = format!("state:{}", scope);
        let mut conn = self.publisher.lock().await;

        let values = conn
            .hgetall::<String, HashMap<String, String>>(scope_key)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get group from Redis: {}", e))?;

        let mut result = Vec::new();
        for v in values.into_values() {
            result.push(
                serde_json::from_str(&v)
                    .map_err(|e| anyhow::anyhow!("Failed to deserialize value: {}", e))?,
            );
        }
        Ok(result)
    }

    async fn list_groups(&self) -> anyhow::Result<Vec<String>> {
        let mut conn = self.publisher.lock().await;
        let mut cursor = 0u64;
        let mut groups = Vec::new();

        loop {
            let (new_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg("state:*")
                .arg("COUNT")
                .arg(100)
                .query_async(&mut *conn)
                .await?;

            for key in keys {
                if let Some(scope) = key.strip_prefix("state:") {
                    groups.push(scope.to_string());
                }
            }

            cursor = new_cursor;
            if cursor == 0 {
                break;
            }
        }

        Ok(groups)
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

crate::register_adapter!(<StateAdapterRegistration> name: "redis", make_adapter);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn setup_test_adapter() -> StateRedisAdapter {
        let redis_url = "redis://localhost:6379".to_string();
        StateRedisAdapter::new(redis_url).await.unwrap()
    }

    #[tokio::test]
    #[ignore] // Requires Redis running
    async fn test_list_groups_redis() {
        let adapter = setup_test_adapter().await;

        // Clean up known test keys before running
        let _ = adapter.delete("test_group1", "item1").await;
        let _ = adapter.delete("test_group2", "item1").await;

        // Add test data
        adapter
            .set("test_group1", "item1", json!({"value": 1}))
            .await
            .unwrap();
        adapter
            .set("test_group2", "item1", json!({"value": 2}))
            .await
            .unwrap();

        let mut groups = adapter.list_groups().await.unwrap();
        groups.sort();

        assert!(groups.contains(&"test_group1".to_string()));
        assert!(groups.contains(&"test_group2".to_string()));
    }

    #[tokio::test]
    #[ignore] // Requires Redis running
    async fn test_update_append_redis() {
        let adapter = setup_test_adapter().await;
        let scope = "test_append_scope";
        let key = "append_item";

        let _ = adapter.delete(scope, key).await;
        adapter
            .set(scope, key, json!({"events": [], "transcript": "hello"}))
            .await
            .unwrap();

        let result = adapter
            .update(
                scope,
                key,
                vec![
                    UpdateOp::append("events", json!({"kind": "chunk"})),
                    UpdateOp::append("transcript", json!(" world")),
                ],
            )
            .await
            .unwrap();

        assert_eq!(result.new_value["events"], json!([{"kind": "chunk"}]));
        assert_eq!(result.new_value["transcript"], "hello world");

        let _ = adapter.delete(scope, key).await;
    }

    #[tokio::test]
    #[ignore] // Requires Redis running
    async fn test_update_structured_errors_redis() {
        let adapter = setup_test_adapter().await;
        let scope = "test_error_scope";
        let key = "error_item";

        let _ = adapter.delete(scope, key).await;
        adapter
            .set(
                scope,
                key,
                json!({"bad": "value", "events": {"nested": true}}),
            )
            .await
            .unwrap();

        let result = adapter
            .update(
                scope,
                key,
                vec![
                    UpdateOp::increment("bad", 1),
                    UpdateOp::Set {
                        path: "__proto__".into(),
                        value: Some(json!(true)),
                    },
                    UpdateOp::append("events", json!("chunk")),
                    UpdateOp::Set {
                        path: "ok".into(),
                        value: Some(json!(true)),
                    },
                ],
            )
            .await
            .unwrap();

        assert_eq!(
            result.new_value,
            json!({"bad": "value", "events": {"nested": true}, "ok": true})
        );
        assert_eq!(result.errors.len(), 3);
        assert_eq!(result.errors[0].op_index, 0);
        assert_eq!(result.errors[0].code, "increment.not_number");
        assert_eq!(result.errors[1].op_index, 1);
        assert_eq!(result.errors[1].code, "set.path.proto_polluted");
        assert_eq!(result.errors[2].op_index, 2);
        assert_eq!(result.errors[2].code, "append.type_mismatch");

        let _ = adapter.delete(scope, key).await;
    }

    #[tokio::test]
    #[ignore] // Requires Redis running
    async fn test_update_root_null_and_numeric_null_errors_redis() {
        let adapter = setup_test_adapter().await;
        let scope = "test_null_scope";
        let key = "null_item";

        let _ = adapter.delete(scope, key).await;
        adapter
            .set(scope, key, json!({"": "empty-field", "keep": true}))
            .await
            .unwrap();

        let result = adapter
            .update(
                scope,
                key,
                vec![UpdateOp::Set {
                    path: "".into(),
                    value: None,
                }],
            )
            .await
            .unwrap();

        assert_eq!(result.new_value, json!(null));
        assert!(result.errors.is_empty());

        adapter
            .set(scope, key, json!({"inc": null, "dec": null}))
            .await
            .unwrap();

        let result = adapter
            .update(
                scope,
                key,
                vec![UpdateOp::increment("inc", 1), UpdateOp::decrement("dec", 1)],
            )
            .await
            .unwrap();

        assert_eq!(result.new_value, json!({"inc": null, "dec": null}));
        assert_eq!(result.errors.len(), 2);
        assert_eq!(result.errors[0].op_index, 0);
        assert_eq!(result.errors[0].code, "increment.not_number");
        assert_eq!(result.errors[1].op_index, 1);
        assert_eq!(result.errors[1].code, "decrement.not_number");

        let _ = adapter.delete(scope, key).await;
    }

    #[tokio::test]
    #[ignore] // Requires Redis running
    async fn test_update_empty_path_numeric_and_remove_ops_target_root_redis() {
        let adapter = setup_test_adapter().await;
        let scope = "test_root_ops_scope";
        let key = "root_ops_item";

        let _ = adapter.delete(scope, key).await;
        adapter.set(scope, key, json!(2)).await.unwrap();

        let result = adapter
            .update(scope, key, vec![UpdateOp::increment("", 3)])
            .await
            .unwrap();

        assert_eq!(result.new_value, json!(5));
        assert!(result.errors.is_empty());

        let result = adapter
            .update(scope, key, vec![UpdateOp::decrement("", 2)])
            .await
            .unwrap();

        assert_eq!(result.new_value, json!(3));
        assert!(result.errors.is_empty());

        let result = adapter
            .update(scope, key, vec![UpdateOp::remove("")])
            .await
            .unwrap();

        assert_eq!(result.new_value, json!(null));
        assert!(result.errors.is_empty());

        let _ = adapter.delete(scope, key).await;
    }
}
