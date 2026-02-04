use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    config::persistence::HttpAuthRef,
    invocation::method::HttpMethod,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpFunctionConfig {
    #[serde(alias = "path")]
    pub function_path: String,
    pub url: String,
    #[serde(default = "default_method")]
    pub method: HttpMethod,
    /// Request timeout in milliseconds. If not specified, the invoker's default timeout will be used.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub auth: Option<HttpAuthRef>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub request_format: Option<Value>,
    #[serde(default)]
    pub response_format: Option<Value>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub registered_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

fn default_method() -> HttpMethod {
    HttpMethod::Post
}
