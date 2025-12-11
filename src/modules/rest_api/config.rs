use serde::{Deserialize, Serialize};

fn default_port() -> u16 {
    3111
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_timeout() -> u64 {
    30000
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestApiConfig {
    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_timeout")]
    pub default_timeout: u64,

    #[serde(default)]
    pub default_path: Option<String>,

    #[serde(default)]
    pub cors: Option<CorsConfig>,
}

impl Default for RestApiConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            default_timeout: default_timeout(),
            default_path: None,
            cors: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CorsConfig {
    #[serde(default)]
    pub allowed_origins: Vec<String>,

    #[serde(default)]
    pub allowed_methods: Vec<String>,
}
