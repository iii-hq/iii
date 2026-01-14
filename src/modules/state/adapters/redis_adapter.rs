use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
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
    async fn set(&self, group_id: &str, item_id: &str, data: Value) {
        let mut conn = self.publisher.lock().await;
        let value = serde_json::to_string(&data).unwrap_or_default();

        if let Err(e) = conn
            .hset::<_, _, String, ()>(group_id, item_id, value)
            .await
        {
            tracing::error!(error = %e, group_id = %group_id, item_id = %item_id, "Failed to set value in Redis");
            return;
        }

        tracing::debug!(group_id = %group_id, item_id = %item_id, "Value set in Redis");
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
