use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementConfig {
    pub port: u16,
    pub host: String,
    pub enabled: bool,
    #[serde(default)]
    pub cors: Option<CorsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
}

impl Default for ManagementConfig {
    fn default() -> Self {
        Self {
            port: 9001,
            host: "0.0.0.0".to_string(),
            enabled: true,
            cors: None,
        }
    }
}
