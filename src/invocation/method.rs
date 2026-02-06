use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub struct HttpEndpointRef<'a> {
    pub url: &'a String,
    pub method: &'a HttpMethod,
    pub timeout_ms: &'a Option<u64>,
    pub headers: &'a HashMap<String, String>,
    pub auth: &'a Option<HttpAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum InvocationMethod {
    WebSocket {
        worker_id: Uuid,
    },
    Http {
        url: String,
        method: HttpMethod,
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

    pub fn as_http(&self) -> Option<HttpEndpointRef<'_>> {
        match self {
            InvocationMethod::Http {
                url,
                method,
                timeout_ms,
                headers,
                auth,
            } => Some(HttpEndpointRef {
                url,
                method,
                timeout_ms,
                headers,
                auth,
            }),
            _ => None,
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
