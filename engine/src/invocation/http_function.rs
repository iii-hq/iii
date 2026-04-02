// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::invocation::method::HttpMethod;

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
    pub auth: Option<crate::invocation::auth::HttpAuthConfig>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_minimal_config_has_defaults() {
        let json = json!({
            "function_path": "/fn",
            "url": "http://localhost:3000"
        });
        let config: HttpFunctionConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.function_path, "/fn");
        assert_eq!(config.url, "http://localhost:3000");
        assert!(matches!(config.method, HttpMethod::Post));
        assert!(config.timeout_ms.is_none());
        assert!(config.headers.is_empty());
        assert!(config.auth.is_none());
        assert!(config.description.is_none());
    }

    #[test]
    fn deserialize_full_config() {
        let json = json!({
            "function_path": "/fn",
            "url": "http://localhost:3000",
            "method": "GET",
            "timeout_ms": 5000,
            "headers": {"X-Custom": "val"},
            "description": "test function"
        });
        let config: HttpFunctionConfig = serde_json::from_value(json).unwrap();
        assert!(matches!(config.method, HttpMethod::Get));
        assert_eq!(config.timeout_ms, Some(5000));
        assert_eq!(config.headers.get("X-Custom").unwrap(), "val");
        assert_eq!(config.description.as_deref(), Some("test function"));
    }

    #[test]
    fn deserialize_path_alias() {
        let json = json!({
            "path": "/aliased",
            "url": "http://localhost:3000"
        });
        let config: HttpFunctionConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.function_path, "/aliased");
    }

    #[test]
    fn serialize_roundtrip() {
        let json = json!({
            "function_path": "/fn",
            "url": "http://localhost:3000"
        });
        let config: HttpFunctionConfig = serde_json::from_value(json).unwrap();
        let serialized = serde_json::to_value(&config).unwrap();
        let back: HttpFunctionConfig = serde_json::from_value(serialized).unwrap();
        assert_eq!(back.function_path, "/fn");
        assert_eq!(back.url, "http://localhost:3000");
    }
}
