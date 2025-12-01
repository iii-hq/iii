use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WebSocketConfig {
    pub port: u16,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self { port: 31112 }
    }
}
