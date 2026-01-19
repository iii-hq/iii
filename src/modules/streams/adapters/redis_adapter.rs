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
    builtins::filters::UpdateOp,
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

#[cfg(test)]
use tokio::sync::Barrier;

const STREAM_TOPIC: &str = "stream::events";

#[cfg(test)]
#[derive(Clone)]
struct UpdateTestHook {
    reached: Arc<Barrier>,
    proceed: Arc<Barrier>,
}

#[cfg(test)]
impl UpdateTestHook {
    fn new() -> Self {
        Self {
            reached: Arc::new(Barrier::new(2)),
            proceed: Arc::new(Barrier::new(2)),
        }
    }
}

#[cfg(test)]
struct UpdateHookGuard<'a> {
    hook: &'a std::sync::Mutex<Option<Arc<UpdateTestHook>>>,
}

#[cfg(test)]
impl Drop for UpdateHookGuard<'_> {
    fn drop(&mut self) {
        *self.hook.lock().unwrap() = None;
    }
}

pub struct RedisAdapter {
    publisher: Arc<Mutex<ConnectionManager>>,
    subscriber: Arc<Client>,
    connections: Arc<RwLock<HashMap<String, Arc<dyn StreamConnection>>>>,
    #[cfg(test)]
    update_hook: std::sync::Mutex<Option<Arc<UpdateTestHook>>>,
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
            #[cfg(test)]
            update_hook: std::sync::Mutex::new(None),
        })
    }
}

#[cfg(test)]
impl RedisAdapter {
    fn install_update_hook<'a>(&'a self, hook: Arc<UpdateTestHook>) -> UpdateHookGuard<'a> {
        *self.update_hook.lock().unwrap() = Some(hook);
        UpdateHookGuard {
            hook: &self.update_hook,
        }
    }

    async fn maybe_wait_update_hook(&self) {
        let hook = self.update_hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook.reached.wait().await;
            hook.proceed.wait().await;
        }
    }
}

#[cfg(not(test))]
impl RedisAdapter {
    async fn maybe_wait_update_hook(&self) {}
}

#[async_trait]
impl StreamAdapter for RedisAdapter {
    async fn update(&self, stream_name: &str, group_id: &str, ops: Vec<UpdateOp>) -> Option<Value> {
        let key = format!("stream:{}:{}", stream_name, group_id);
        let mut conn = self.publisher.lock().await;

        const MAX_RETRIES: usize = 5;
        for _ in 0..MAX_RETRIES {
            let watch_result: Result<(), _> =
                redis::cmd("WATCH").arg(&key).query_async(&mut *conn).await;
            if let Err(e) = watch_result {
                tracing::error!(error = %e, stream_name = %stream_name, group_id = %group_id, "Failed to watch Redis key");
                return None;
            }

            let exists = match conn.exists::<_, bool>(&key).await {
                Ok(exists) => exists,
                Err(e) => {
                    tracing::error!(error = %e, stream_name = %stream_name, group_id = %group_id, "Failed to check Redis key existence");
                    let _: Result<(), _> = redis::cmd("UNWATCH").query_async(&mut *conn).await;
                    return None;
                }
            };
            if !exists {
                let _: Result<(), _> = redis::cmd("UNWATCH").query_async(&mut *conn).await;
                return None;
            }

            self.maybe_wait_update_hook().await;

            let mut pipe = redis::pipe();
            pipe.atomic();

            for op in ops.iter().cloned() {
                match op {
                    UpdateOp::Set { path, value } => {
                        if path.0.is_empty() {
                            match value {
                                Value::Object(map) => {
                                    pipe.del(&key);
                                    for (field, v) in map {
                                        let payload = match serde_json::to_string(&v) {
                                            Ok(payload) => payload,
                                            Err(err) => {
                                                tracing::warn!(
                                                    error = ?err,
                                                    stream_name = %stream_name,
                                                    group_id = %group_id,
                                                    field = %field,
                                                    "Failed to serialize value for HSET"
                                                );
                                                continue;
                                            }
                                        };
                                        pipe.hset(&key, field, payload);
                                    }
                                }
                                _ => {
                                    tracing::warn!(
                                        "Set operation with empty path requires value to be a JSON object"
                                    );
                                }
                            }
                        } else {
                            let payload = match serde_json::to_string(&value) {
                                Ok(payload) => payload,
                                Err(err) => {
                                    tracing::warn!(
                                        error = ?err,
                                        stream_name = %stream_name,
                                        group_id = %group_id,
                                        "Failed to serialize value for HSET"
                                    );
                                    continue;
                                }
                            };
                            pipe.hset(&key, path.0, payload);
                        }
                    }
                    UpdateOp::Merge { path, value } => {
                        if path.is_none() || path.as_ref().unwrap().0.is_empty() {
                            match value {
                                Value::Object(map) => {
                                    for (field, v) in map {
                                        let payload = match serde_json::to_string(&v) {
                                            Ok(payload) => payload,
                                            Err(err) => {
                                                tracing::warn!(
                                                    error = ?err,
                                                    stream_name = %stream_name,
                                                    group_id = %group_id,
                                                    field = %field,
                                                    "Failed to serialize value for HSET"
                                                );
                                                continue;
                                            }
                                        };
                                        pipe.hset(&key, field, payload);
                                    }
                                }
                                _ => {
                                    tracing::warn!(
                                        "Merge operation requires new values to be a JSON object"
                                    );
                                }
                            }
                        } else {
                            tracing::warn!("Only root-level merge is supported");
                        }
                    }
                    UpdateOp::Increment { path, by } => {
                        pipe.hincr(&key, path.0, by);
                    }
                    UpdateOp::Decrement { path, by } => {
                        pipe.hincr(&key, path.0, -by);
                    }
                    UpdateOp::Remove { path } => {
                        pipe.hdel(&key, path.0);
                    }
                }
            }

            let result: Result<Option<Vec<redis::Value>>, _> = pipe.query_async(&mut *conn).await;
            match result {
                Ok(Some(_)) => {
                    let values = match conn.hgetall::<_, HashMap<String, String>>(&key).await {
                        Ok(values) => values,
                        Err(e) => {
                            tracing::error!(error = %e, stream_name = %stream_name, group_id = %group_id, "Failed to get group from Redis");
                            return None;
                        }
                    };

                    let mut map = serde_json::Map::new();
                    for (field, value) in values {
                        match serde_json::from_str::<Value>(&value) {
                            Ok(parsed) => {
                                map.insert(field, parsed);
                            }
                            Err(err) => {
                                tracing::warn!(
                                    error = ?err,
                                    stream_name = %stream_name,
                                    group_id = %group_id,
                                    field = %field,
                                    "Failed to parse Redis hash value as JSON"
                                );
                            }
                        }
                    }

                    return Some(Value::Object(map));
                }
                Ok(None) => {
                    let exists = match conn.exists::<_, bool>(&key).await {
                        Ok(exists) => exists,
                        Err(err) => {
                            tracing::error!(error = %err, stream_name = %stream_name, group_id = %group_id, "Failed to re-check Redis key existence");
                            return None;
                        }
                    };
                    if !exists {
                        return None;
                    }
                    continue;
                }
                Err(e)
                    if e.kind() == redis::ErrorKind::Server(redis::ServerErrorKind::ExecAbort) =>
                {
                    let exists = match conn.exists::<_, bool>(&key).await {
                        Ok(exists) => exists,
                        Err(err) => {
                            tracing::error!(error = %err, stream_name = %stream_name, group_id = %group_id, "Failed to re-check Redis key existence");
                            return None;
                        }
                    };
                    if !exists {
                        return None;
                    }
                    continue;
                }
                Err(e) => {
                    let _: Result<(), _> = redis::cmd("UNWATCH").query_async(&mut *conn).await;
                    tracing::error!(error = %e, stream_name = %stream_name, group_id = %group_id, "Failed to update Redis hash");
                    return None;
                }
            }
        }

        None
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

    async fn set(&self, stream_name: &str, group_id: &str, item_id: &str, data: Value) {
        let key: String = format!("stream:{}:{}", stream_name, group_id);
        let mut conn = self.publisher.lock().await;
        let value = serde_json::to_string(&data).unwrap_or_default();

        let existed = match conn.hexists::<_, _, bool>(&key, item_id).await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(error = %e, key = %key, item_id = %item_id, "Failed to check existence with hexists");
                false
            }
        };

        if let Err(e) = conn.hset::<_, _, String, ()>(key, item_id, value).await {
            tracing::error!(error = %e, stream_name = %stream_name, group_id = %group_id, item_id = %item_id, "Failed to set value in Redis");
            return;
        }

        let event_type = if existed { "update" } else { "create" };
        let timestamp = chrono::Utc::now().timestamp_millis();

        drop(conn); // Release the lock

        self.emit_event(StreamWrapperMessage {
            timestamp,
            stream_name: stream_name.to_string(),
            group_id: group_id.to_string(),
            id: Some(item_id.to_string()),
            event: match event_type {
                "update" => StreamOutboundMessage::Update { data },
                "create" => StreamOutboundMessage::Create { data },
                _ => StreamOutboundMessage::Create { data },
            },
        })
        .await;

        tracing::debug!(stream_name = %stream_name, group_id = %group_id, item_id = %item_id, "Value set in Redis");
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
            let msg: StreamWrapperMessage =
                match serde_json::from_str::<StreamWrapperMessage>(&payload) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::filters::{FieldPath, UpdateOp};
    use serde_json::json;
    use uuid::Uuid;

    async fn create_adapter() -> Option<RedisAdapter> {
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

        match RedisAdapter::new(redis_url).await {
            Ok(adapter) => Some(adapter),
            Err(err) => {
                eprintln!("Skipping RedisAdapter tests: {err}");
                None
            }
        }
    }

    async fn create_connection() -> Option<ConnectionManager> {
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
        let client = match Client::open(redis_url.as_str()) {
            Ok(client) => client,
            Err(err) => {
                eprintln!("Skipping RedisAdapter tests: {err}");
                return None;
            }
        };

        match client.get_connection_manager().await {
            Ok(conn) => Some(conn),
            Err(err) => {
                eprintln!("Skipping RedisAdapter tests: {err}");
                None
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_redis_adapter_set_get_delete() {
        let Some(adapter) = create_adapter().await else {
            return;
        };

        let stream_name = format!("test_stream_{}", Uuid::new_v4());
        let group_id = format!("group_{}", Uuid::new_v4());
        let item_id = "item1";
        let data = json!({"key": "value"});

        adapter
            .set(&stream_name, &group_id, item_id, data.clone())
            .await;

        let retrieved = adapter
            .get(&stream_name, &group_id, item_id)
            .await
            .expect("Item should exist");
        assert_eq!(retrieved, data);

        adapter.delete(&stream_name, &group_id, item_id).await;
        let deleted = adapter.get(&stream_name, &group_id, item_id).await;
        assert!(deleted.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_redis_adapter_update_missing_group_returns_none() {
        let Some(adapter) = create_adapter().await else {
            return;
        };

        let stream_name = format!("test_stream_{}", Uuid::new_v4());
        let group_id = format!("group_{}", Uuid::new_v4());

        let result = adapter
            .update(
                &stream_name,
                &group_id,
                vec![UpdateOp::Set {
                    path: FieldPath::from("a"),
                    value: json!(1),
                }],
            )
            .await;

        assert!(result.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_redis_adapter_update_applies_ops() {
        let Some(adapter) = create_adapter().await else {
            return;
        };

        let stream_name = format!("test_stream_{}", Uuid::new_v4());
        let group_id = format!("group_{}", Uuid::new_v4());

        adapter.set(&stream_name, &group_id, "a", json!(1)).await;
        adapter.set(&stream_name, &group_id, "b", json!(2)).await;

        let result = adapter
            .update(
                &stream_name,
                &group_id,
                vec![
                    UpdateOp::Increment {
                        path: FieldPath::from("a"),
                        by: 2,
                    },
                    UpdateOp::Decrement {
                        path: FieldPath::from("b"),
                        by: 1,
                    },
                    UpdateOp::Set {
                        path: FieldPath::from("c"),
                        value: json!(5),
                    },
                    UpdateOp::Merge {
                        path: None,
                        value: json!({"d": 6}),
                    },
                    UpdateOp::Remove {
                        path: FieldPath::from("b"),
                    },
                ],
            )
            .await
            .expect("Update should return value");

        assert_eq!(result, json!({"a": 3, "c": 5, "d": 6}));

        let a = adapter.get(&stream_name, &group_id, "a").await;
        let b = adapter.get(&stream_name, &group_id, "b").await;
        let c = adapter.get(&stream_name, &group_id, "c").await;
        let d = adapter.get(&stream_name, &group_id, "d").await;

        assert_eq!(a, Some(json!(3)));
        assert!(b.is_none());
        assert_eq!(c, Some(json!(5)));
        assert_eq!(d, Some(json!(6)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_redis_adapter_concurrent_updates_increment() {
        let Some(adapter) = create_adapter().await else {
            return;
        };

        let adapter = Arc::new(adapter);
        let stream_name = format!("test_stream_{}", Uuid::new_v4());
        let group_id = format!("group_{}", Uuid::new_v4());
        let counter_key = "counter";

        adapter
            .set(&stream_name, &group_id, counter_key, json!(0))
            .await;

        let mut handles = Vec::with_capacity(500);
        for _ in 0..500 {
            let adapter = Arc::clone(&adapter);
            let stream_name = stream_name.clone();
            let group_id = group_id.clone();
            handles.push(tokio::spawn(async move {
                let _ = adapter
                    .update(
                        &stream_name,
                        &group_id,
                        vec![UpdateOp::Increment {
                            path: FieldPath::from(counter_key),
                            by: 1,
                        }],
                    )
                    .await;
            }));
        }

        futures::future::join_all(handles).await;

        let result = adapter
            .get(&stream_name, &group_id, counter_key)
            .await
            .expect("Counter should exist");

        assert_eq!(result, json!(500));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_redis_adapter_update_does_not_resurrect_after_delete() {
        let Some(adapter) = create_adapter().await else {
            return;
        };
        let Some(mut delete_conn) = create_connection().await else {
            return;
        };

        let adapter = Arc::new(adapter);
        let hook = Arc::new(UpdateTestHook::new());
        let _hook_guard = adapter.install_update_hook(hook.clone());

        let stream_name = format!("test_stream_{}", Uuid::new_v4());
        let group_id = format!("group_{}", Uuid::new_v4());
        let counter_key = "counter";
        let key = format!("stream:{}:{}", stream_name, group_id);

        adapter
            .set(&stream_name, &group_id, counter_key, json!(0))
            .await;

        let adapter_for_update = Arc::clone(&adapter);
        let stream_for_update = stream_name.clone();
        let group_for_update = group_id.clone();

        let update_task = tokio::spawn(async move {
            adapter_for_update
                .update(
                    &stream_for_update,
                    &group_for_update,
                    vec![UpdateOp::Increment {
                        path: FieldPath::from(counter_key),
                        by: 1,
                    }],
                )
                .await
        });

        tokio::time::timeout(std::time::Duration::from_secs(2), hook.reached.wait())
            .await
            .expect("Timed out waiting for update hook");

        let result: Result<(), _> = redis::cmd("DEL")
            .arg(&key)
            .query_async(&mut delete_conn)
            .await;
        assert!(result.is_ok());

        tokio::time::timeout(std::time::Duration::from_secs(2), hook.proceed.wait())
            .await
            .expect("Timed out waiting for update hook proceed");

        let update_result = update_task.await.expect("Update task should complete");
        assert!(update_result.is_none());

        let exists: bool = delete_conn.exists(&key).await.unwrap_or(true);
        assert!(!exists);
    }
}
