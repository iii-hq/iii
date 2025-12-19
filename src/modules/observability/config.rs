use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LoggerModuleConfig {
    #[serde(default)]
    pub level: Option<String>,

    #[serde(default)]
    pub format: Option<String>,
    pub adapter: Option<AdapterEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdapterEntry {
    pub class: String,
    #[serde(default)]
    pub config: Option<Value>,
}
