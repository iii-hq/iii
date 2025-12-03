use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct WebSocketConfig {
    pub port: u16,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self { port: 31112 }
    }
}
