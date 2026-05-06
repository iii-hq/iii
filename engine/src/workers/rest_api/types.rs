// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

use axum::{
    body::Body,
    http::{HeaderName, HeaderValue, Response, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::protocol::StreamChannelRef;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerMetadata {
    #[serde(rename = "type")]
    pub trigger_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub query_params: HashMap<String, String>,
    pub path_params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub path: String,
    pub method: String,
    pub body: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<TriggerMetadata>,

    pub request_body: StreamChannelRef,
    pub response: StreamChannelRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status_code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

impl HttpResponse {
    pub fn from_function_return(value: Value) -> Self {
        let status_code = value
            .get("status_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(200) as u16;
        let headers = match value.get("headers") {
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .filter_map(parse_header_line)
                .collect(),
            Some(Value::Object(map)) => map
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect(),
            _ => Vec::new(),
        };
        let body = value.get("body").cloned().unwrap_or(json!({}));
        HttpResponse {
            status_code,
            headers,
            body,
        }
    }

    /// Returns the value of the first header matching `name` case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Build an axum response, honoring user-set headers and choosing a body
    /// serialization based on Content-Type (string/bytes for non-JSON types,
    /// JSON otherwise).
    pub fn into_axum_response(self) -> Response<Body> {
        let status = StatusCode::from_u16(self.status_code).unwrap_or(StatusCode::OK);
        let user_content_type = self.header("content-type").map(|s| s.to_string());

        let (body_bytes, default_content_type) = match (&user_content_type, &self.body) {
            (Some(_), Value::String(s)) => (s.clone().into_bytes(), None),
            (Some(_), Value::Null) => (Vec::new(), None),
            (Some(ct), other) if !ct.to_ascii_lowercase().contains("application/json") => {
                (other.to_string().into_bytes(), None)
            }
            (_, body) => (
                serde_json::to_vec(body).unwrap_or_else(|_| b"null".to_vec()),
                Some("application/json"),
            ),
        };

        let mut builder = Response::builder().status(status);
        let mut content_type_set = false;
        for (k, v) in &self.headers {
            match (
                HeaderName::try_from(k.as_str()),
                HeaderValue::try_from(v.as_str()),
            ) {
                (Ok(name), Ok(value)) => {
                    if name.as_str().eq_ignore_ascii_case("content-type") {
                        content_type_set = true;
                    }
                    builder = builder.header(name, value);
                }
                _ => {
                    tracing::warn!(header_name = %k, "Skipping invalid response header");
                }
            }
        }
        if !content_type_set {
            if let Some(ct) = default_content_type {
                builder = builder.header("content-type", ct);
            }
        }

        builder
            .body(Body::from(body_bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
    }
}

fn parse_header_line(line: &str) -> Option<(String, String)> {
    let (name, value) = line.split_once(':')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some((name.to_string(), value.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // =========================================================================
    // TriggerMetadata serialization/deserialization
    // =========================================================================

    #[test]
    fn trigger_metadata_serialize_full() {
        let meta = TriggerMetadata {
            trigger_type: "http".to_string(),
            path: Some("/api/test".to_string()),
            method: Some("GET".to_string()),
        };
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(json["type"], "http");
        assert_eq!(json["path"], "/api/test");
        assert_eq!(json["method"], "GET");
    }

    #[test]
    fn trigger_metadata_serialize_skips_none() {
        let meta = TriggerMetadata {
            trigger_type: "cron".to_string(),
            path: None,
            method: None,
        };
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(json["type"], "cron");
        assert!(json.get("path").is_none());
        assert!(json.get("method").is_none());
    }

    #[test]
    fn trigger_metadata_deserialize() {
        let json = json!({"type": "durable:subscriber", "path": "/events", "method": "POST"});
        let meta: TriggerMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(meta.trigger_type, "durable:subscriber");
        assert_eq!(meta.path, Some("/events".to_string()));
        assert_eq!(meta.method, Some("POST".to_string()));
    }

    #[test]
    fn trigger_metadata_deserialize_minimal() {
        let json = json!({"type": "cron"});
        let meta: TriggerMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(meta.trigger_type, "cron");
        assert!(meta.path.is_none());
        assert!(meta.method.is_none());
    }

    // =========================================================================
    // HttpResponse::from_function_return
    // =========================================================================

    #[test]
    fn http_response_from_full_value() {
        let value = json!({
            "status_code": 201,
            "headers": ["Content-Type: application/json", "X-Custom: value"],
            "body": {"key": "value"}
        });
        let resp = HttpResponse::from_function_return(value);
        assert_eq!(resp.status_code, 201);
        assert_eq!(
            resp.headers,
            vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("X-Custom".to_string(), "value".to_string()),
            ]
        );
        assert_eq!(resp.body, json!({"key": "value"}));
    }

    #[test]
    fn http_response_headers_from_object() {
        let value = json!({
            "status_code": 200,
            "headers": { "Content-Type": "text/xml" },
            "body": "<Response/>"
        });
        let resp = HttpResponse::from_function_return(value);
        assert_eq!(
            resp.header("content-type"),
            Some("text/xml"),
            "headers given as object form should parse case-insensitively"
        );
    }

    #[test]
    fn http_response_defaults_status_200() {
        let value = json!({});
        let resp = HttpResponse::from_function_return(value);
        assert_eq!(resp.status_code, 200);
    }

    #[test]
    fn http_response_defaults_empty_headers() {
        let value = json!({"status_code": 404});
        let resp = HttpResponse::from_function_return(value);
        assert!(resp.headers.is_empty());
    }

    #[test]
    fn http_response_defaults_empty_body() {
        let value = json!({"status_code": 204});
        let resp = HttpResponse::from_function_return(value);
        assert_eq!(resp.body, json!({}));
    }

    #[test]
    fn http_response_headers_filters_invalid() {
        let value = json!({
            "headers": ["X-A: one", 123, null, "no-colon", "X-B: two"]
        });
        let resp = HttpResponse::from_function_return(value);
        // Non-strings and entries without a colon are dropped.
        assert_eq!(
            resp.headers,
            vec![
                ("X-A".to_string(), "one".to_string()),
                ("X-B".to_string(), "two".to_string()),
            ]
        );
    }

    #[test]
    fn http_response_headers_null_becomes_empty() {
        let value = json!({"headers": null});
        let resp = HttpResponse::from_function_return(value);
        assert!(resp.headers.is_empty());
    }

    #[test]
    fn http_response_status_code_from_u64() {
        let value = json!({"status_code": 500});
        let resp = HttpResponse::from_function_return(value);
        assert_eq!(resp.status_code, 500);
    }

    #[test]
    fn http_response_status_code_non_number_defaults() {
        let value = json!({"status_code": "not_a_number"});
        let resp = HttpResponse::from_function_return(value);
        assert_eq!(resp.status_code, 200);
    }

    #[test]
    fn http_response_body_string_value() {
        let value = json!({"body": "plain text"});
        let resp = HttpResponse::from_function_return(value);
        assert_eq!(resp.body, json!("plain text"));
    }

    #[test]
    fn http_response_body_array_value() {
        let value = json!({"body": [1, 2, 3]});
        let resp = HttpResponse::from_function_return(value);
        assert_eq!(resp.body, json!([1, 2, 3]));
    }

    // =========================================================================
    // HttpResponse serialization roundtrip
    // =========================================================================

    #[test]
    fn http_response_serialize_deserialize() {
        let resp = HttpResponse {
            status_code: 200,
            headers: vec![("Content-Type".to_string(), "text/html".to_string())],
            body: json!({"message": "ok"}),
        };
        let json_str = serde_json::to_string(&resp).unwrap();
        let deserialized: HttpResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.status_code, 200);
        assert_eq!(
            deserialized.headers,
            vec![("Content-Type".to_string(), "text/html".to_string())]
        );
        assert_eq!(deserialized.body, json!({"message": "ok"}));
    }

    #[test]
    fn into_axum_response_applies_user_content_type_for_string_body() {
        let value = json!({
            "status_code": 200,
            "headers": ["Content-Type: text/xml"],
            "body": "<Response><Say>hi</Say></Response>",
        });
        let resp = HttpResponse::from_function_return(value).into_axum_response();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("text/xml"),
        );
    }

    #[test]
    fn into_axum_response_defaults_to_json_when_no_content_type() {
        let value = json!({
            "status_code": 201,
            "body": { "ok": true },
        });
        let resp = HttpResponse::from_function_return(value).into_axum_response();
        assert_eq!(resp.status(), StatusCode::CREATED);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("application/json"),
        );
    }

    #[test]
    fn into_axum_response_preserves_custom_headers() {
        let value = json!({
            "status_code": 200,
            "headers": { "X-Custom": "abc", "Content-Type": "text/plain" },
            "body": "hello",
        });
        let resp = HttpResponse::from_function_return(value).into_axum_response();
        assert_eq!(
            resp.headers().get("x-custom").and_then(|v| v.to_str().ok()),
            Some("abc"),
        );
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("text/plain"),
        );
    }

    // =========================================================================
    // HttpRequest serialization
    // =========================================================================

    #[test]
    fn http_request_serialize_deserialize() {
        let req = HttpRequest {
            query_params: {
                let mut m = HashMap::new();
                m.insert("page".to_string(), "1".to_string());
                m
            },
            path_params: HashMap::new(),
            headers: {
                let mut m = HashMap::new();
                m.insert("content-type".to_string(), "application/json".to_string());
                m
            },
            path: "/api/test".to_string(),
            method: "GET".to_string(),
            body: json!(null),
            trigger: None,
            request_body: StreamChannelRef {
                channel_id: "ch1".to_string(),
                access_key: "key1".to_string(),
                direction: crate::protocol::ChannelDirection::Read,
            },
            response: StreamChannelRef {
                channel_id: "ch2".to_string(),
                access_key: "key2".to_string(),
                direction: crate::protocol::ChannelDirection::Write,
            },
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let deserialized: HttpRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.path, "/api/test");
        assert_eq!(deserialized.method, "GET");
        assert_eq!(
            deserialized.query_params.get("page"),
            Some(&"1".to_string())
        );
        assert!(deserialized.trigger.is_none());
    }

    #[test]
    fn http_request_with_trigger() {
        let req = HttpRequest {
            query_params: HashMap::new(),
            path_params: HashMap::new(),
            headers: HashMap::new(),
            path: "/webhook".to_string(),
            method: "POST".to_string(),
            body: json!({"data": true}),
            trigger: Some(TriggerMetadata {
                trigger_type: "http".to_string(),
                path: Some("/webhook".to_string()),
                method: Some("POST".to_string()),
            }),
            request_body: StreamChannelRef {
                channel_id: "ch1".to_string(),
                access_key: "key1".to_string(),
                direction: crate::protocol::ChannelDirection::Read,
            },
            response: StreamChannelRef {
                channel_id: "ch2".to_string(),
                access_key: "key2".to_string(),
                direction: crate::protocol::ChannelDirection::Write,
            },
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("trigger").is_some());
        assert_eq!(json["trigger"]["type"], "http");
    }
}
