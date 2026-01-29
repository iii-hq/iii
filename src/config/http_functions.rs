use std::collections::HashMap;

use anyhow::anyhow;
use serde::Deserialize;
use serde_json::Value;

use crate::invocation::method::{HttpAuth, HttpMethod};

#[derive(Debug, Clone, Deserialize)]
pub struct HttpFunctionConfig {
    pub path: String,
    pub url: String,
    #[serde(default = "default_method")]
    pub method: HttpMethod,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub auth: Option<HttpAuthConfig>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub request_format: Option<Value>,
    #[serde(default)]
    pub response_format: Option<Value>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HttpAuthConfig {
    Hmac { secret_env: String },
    Bearer { token_env: String },
    ApiKey { header: String, value_env: String },
}

pub fn resolve_http_auth(auth: &HttpAuthConfig) -> anyhow::Result<HttpAuth> {
    match auth {
        HttpAuthConfig::Hmac { secret_env } => {
            let secret = std::env::var(secret_env)
                .map_err(|_| anyhow!("Missing env var: {}", secret_env))?;
            Ok(HttpAuth::Hmac { secret })
        }
        HttpAuthConfig::Bearer { token_env } => {
            let token = std::env::var(token_env)
                .map_err(|_| anyhow!("Missing env var: {}", token_env))?;
            Ok(HttpAuth::Bearer { token })
        }
        HttpAuthConfig::ApiKey { header, value_env } => {
            let value = std::env::var(value_env)
                .map_err(|_| anyhow!("Missing env var: {}", value_env))?;
            Ok(HttpAuth::ApiKey {
                header: header.clone(),
                value,
            })
        }
    }
}

fn default_method() -> HttpMethod {
    HttpMethod::Post
}

fn default_timeout_ms() -> u64 {
    30000
}
