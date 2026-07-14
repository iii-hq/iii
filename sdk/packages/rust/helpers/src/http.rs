//! iii http helpers.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// HTTP method accepted by [`HttpInvocationConfig`]. Distinct from the core
/// `builtin_triggers` HTTP method enum, which also covers HEAD/OPTIONS.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

/// Authentication configuration for HTTP-invoked functions.
///
/// - `Hmac`: HMAC signature verification using a shared secret.
/// - `Bearer`: Bearer token authentication.
/// - `ApiKey`: API key sent via a custom header.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HttpAuthConfig {
    Hmac {
        secret_key: String,
    },
    Bearer {
        token_key: String,
    },
    #[serde(rename = "api_key")]
    ApiKey {
        header: String,
        value_key: String,
    },
}

/// Configuration for registering an HTTP-invoked function (Lambda, Cloudflare
/// Workers, etc.) instead of a local handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpInvocationConfig {
    pub url: String,
    #[serde(default = "default_http_method")]
    pub method: HttpMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<HttpAuthConfig>,
}

fn default_http_method() -> HttpMethod {
    HttpMethod::Post
}

/// Buffered HTTP request received by a function handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest<T = Value> {
    #[serde(default)]
    pub query_params: HashMap<String, String>,
    #[serde(default)]
    pub path_params: HashMap<String, String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub body: T,
}

/// Buffered HTTP response returned from a function handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse<T = Value> {
    pub status_code: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub body: T,
}
