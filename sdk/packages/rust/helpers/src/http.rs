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

/// Configuration for an HTTP-invoked function (Lambda, Cloudflare Workers, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpInvocationConfig {
    /// URL to invoke.
    pub url: String,
    /// HTTP method. Defaults to `POST`.
    #[serde(default = "default_http_method")]
    pub method: HttpMethod,
    /// Timeout in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Custom headers to send with the request.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Authentication configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<HttpAuthConfig>,
}

fn default_http_method() -> HttpMethod {
    HttpMethod::Post
}

/// Buffered HTTP request received by a function handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest<T = Value> {
    /// Query-string parameters from the request URL.
    #[serde(default)]
    pub query_params: HashMap<String, String>,
    /// Path parameters extracted from the matched route.
    #[serde(default)]
    pub path_params: HashMap<String, String>,
    /// Request headers.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Request path.
    #[serde(default)]
    pub path: String,
    /// HTTP method of the request (e.g. `GET`, `POST`).
    #[serde(default)]
    pub method: String,
    /// Parsed request body.
    #[serde(default)]
    pub body: T,
}

/// Buffered HTTP response returned from a function handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse<T = Value> {
    /// HTTP status code.
    pub status_code: u16,
    /// Response headers.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Response body.
    pub body: T,
}
