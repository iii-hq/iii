use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;

use super::{EventAdapter, adapters::RedisAdapter};
use crate::engine::Engine;

fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

/// Default adapter class when none is specified
const DEFAULT_ADAPTER_CLASS: &str = "modules::event::RedisAdapter";

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EventModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}

impl EventModuleConfig {
    /// Creates the appropriate adapter based on the config
    pub async fn create_adapter(
        &self,
        engine: Arc<Engine>,
    ) -> anyhow::Result<Arc<dyn EventAdapter>> {
        let adapter_entry = self.adapter.as_ref();

        let class = adapter_entry
            .map(|a| a.class.as_str())
            .unwrap_or(DEFAULT_ADAPTER_CLASS);

        let config = adapter_entry.and_then(|a| a.config.clone());

        match class {
            "modules::event::RedisAdapter" => {
                let redis_config: RedisAdapterConfig = config
                    .map(|v| serde_json::from_value(v).unwrap_or_default())
                    .unwrap_or_default();

                tracing::info!(
                    "Creating RedisAdapter for EventModule at {}",
                    redis_config.redis_url
                );

                let adapter = tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    RedisAdapter::new(redis_config.redis_url, engine),
                )
                .await
                .map_err(|_| anyhow::anyhow!("Timed out while connecting to Redis"))?
                .map_err(|e| anyhow::anyhow!("Failed to connect to Redis: {}", e))?;

                Ok(Arc::new(adapter))
            }

            unknown => Err(anyhow::anyhow!("Unknown event adapter class: {}", unknown)),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdapterEntry {
    pub class: String,
    #[serde(default)]
    pub config: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisAdapterConfig {
    #[serde(default = "default_redis_url")]
    pub redis_url: String,
}

impl Default for RedisAdapterConfig {
    fn default() -> Self {
        Self {
            redis_url: default_redis_url(),
        }
    }
}
