use serde::Deserialize;
use serde_json::Value;

fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CronModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
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
