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

        // Check existence
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

    async fn list_groups(&self, stream_name: &str) -> Vec<(String, usize)> {
        let pattern = format!("stream:{}:*", stream_name);
        let mut conn = self.publisher.lock().await;

        let keys: Vec<String> = match redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut *conn)
            .await
        {
            Ok(keys) => keys,
            Err(e) => {
                tracing::error!(error = %e, stream_name = %stream_name, "Failed to list stream groups from Redis");
                return Vec::new();
            }
        };

        if keys.is_empty() {
            return Vec::new();
        }

        let prefix = format!("stream:{}:", stream_name);
        let mut pipe = redis::pipe();
        let mut group_ids: Vec<String> = Vec::new();

        for key in &keys {
            if let Some(group_id) = key.strip_prefix(&prefix) {
                pipe.hlen(key);
                group_ids.push(group_id.to_string());
            }
        }

        let counts: Vec<usize> = match pipe.query_async(&mut *conn).await {
            Ok(counts) => counts,
            Err(_) => vec![0; group_ids.len()],
        };

        let mut groups: Vec<(String, usize)> = group_ids.into_iter().zip(counts).collect();

        groups.sort_by(|a, b| b.1.cmp(&a.1));
        groups
    }

    async fn list_streams(&self) -> Vec<String> {
        let mut conn = self.publisher.lock().await;
        let keys: Vec<String> = match redis::cmd("KEYS")
            .arg("stream:*")
            .query_async(&mut *conn)
            .await
        {
            Ok(keys) => keys,
            Err(e) => {
                tracing::error!(error = %e, "Failed to list streams from Redis");
                return Vec::new();
            }
        };

        let mut stream_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for key in keys {
            if let Some(rest) = key.strip_prefix("stream:") {
                if let Some(colon_pos) = rest.find(':') {
                    let stream_name = &rest[..colon_pos];
                    stream_names.insert(stream_name.to_string());
                }
            }
        }

        stream_names.into_iter().collect()
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
