use serde::{Deserialize, Serialize};

const DEFAULT_ADMIN_PORT: u16 = 49135;
const DEFAULT_ADMIN_HOST: &str = "127.0.0.1";

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AdminApiConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_host")]
    pub host: String,
}

fn default_port() -> u16 {
    DEFAULT_ADMIN_PORT
}

fn default_host() -> String {
    DEFAULT_ADMIN_HOST.to_string()
}

impl Default for AdminApiConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_ADMIN_PORT,
            host: DEFAULT_ADMIN_HOST.to_string(),
        }
    }
}
