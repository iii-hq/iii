use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum InvocationMethod {
    WebSocket {
        worker_id: Uuid,
    },
    Http {
        url: String,
        method: HttpMethod,
        /// Request timeout in milliseconds. None means use the default timeout.
        timeout_ms: Option<u64>,
        headers: HashMap<String, String>,
        auth: Option<HttpAuth>,
    },
}

impl InvocationMethod {
    pub fn method_type(&self) -> &'static str {
        match self {
            InvocationMethod::WebSocket { .. } => "websocket",
            InvocationMethod::Http { .. } => "http",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Post,
    Put,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HttpAuth {
    Hmac { secret: String },
    Bearer { token: String },
    ApiKey { header: String, value: String },
}
