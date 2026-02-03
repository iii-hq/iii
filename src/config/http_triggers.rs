use serde::Deserialize;
use serde_json::Value;

use crate::config::persistence::HttpAuthRef;

#[derive(Debug, Clone, Deserialize)]
pub struct HttpTriggerConfig {
    pub function_path: String,
    pub trigger_type: String,
    pub trigger_id: String,
    pub url: String,
    #[serde(default)]
    pub auth: Option<HttpAuthRef>,
    #[serde(default)]
    pub config: Value,
}
