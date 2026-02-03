mod http_functions;
mod http_triggers;
pub mod persistence;

use serde::Deserialize;

pub use http_functions::HttpFunctionConfig;
pub use persistence::{HttpAuthRef, resolve_auth_ref};
pub use http_triggers::HttpTriggerConfig;

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub url_allowlist: Vec<String>,
    #[serde(default = "default_block_private_ips")]
    pub block_private_ips: bool,
    #[serde(default = "default_require_https")]
    pub require_https: bool,
}

fn default_block_private_ips() -> bool {
    true
}

fn default_require_https() -> bool {
    true
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            url_allowlist: vec!["*".to_string()],
            block_private_ips: true,
            require_https: true,
        }
    }
}
