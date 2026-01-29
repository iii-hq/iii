use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpTriggerConfig {
    pub function_path: String,
    pub trigger_type: String,
    pub trigger_id: String,
    #[serde(default)]
    pub config: Value,
}
