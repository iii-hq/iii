use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct StreamModuleConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub adapter: Option<AdapterEntry>,
}

fn default_port() -> u16 {
    31112
}

impl Default for StreamModuleConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            adapter: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterEntry {
    pub class: String,
    #[serde(default)]
    pub config: Option<Value>,
}
