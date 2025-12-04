use serde::Deserialize;
use serde_json::Value;

#[allow(dead_code)] // this is used as default value
fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EventModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdapterEntry {
    pub class: String,
    #[serde(default)]
    pub config: Option<Value>,
}
