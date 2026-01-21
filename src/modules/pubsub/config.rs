use serde::Deserialize;

use crate::modules::module::AdapterEntry;

#[allow(dead_code)] // this is used as default value
fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PubSubModuleConfig {
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}
