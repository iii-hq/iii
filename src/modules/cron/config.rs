use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{CronScheduler, RedisCronLock};

fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

/// Default adapter class when none is specified
const DEFAULT_ADAPTER_CLASS: &str = "modules::cron::RedisCronAdapter";

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CronModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}

impl CronModuleConfig {
    /// Creates the appropriate scheduler based on the config
    pub async fn create_scheduler(&self) -> anyhow::Result<Arc<dyn CronScheduler>> {
        let adapter_entry = self.adapter.as_ref();

        let class = adapter_entry
            .map(|a| a.class.as_str())
            .unwrap_or(DEFAULT_ADAPTER_CLASS);

        let config = adapter_entry.and_then(|a| a.config.clone());

        match class {
            "modules::cron::RedisCronAdapter" => {
                let redis_config: RedisAdapterConfig = config
                    .map(|v| serde_json::from_value(v).unwrap_or_default())
                    .unwrap_or_default();

                tracing::info!(
                    "Creating RedisCronLock for CronModule at {}",
                    redis_config.redis_url
                );

                let scheduler = RedisCronLock::new(&redis_config.redis_url).await?;

                Ok(Arc::new(scheduler))
            }

            unknown => Err(anyhow::anyhow!("Unknown cron adapter class: {}", unknown)),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
