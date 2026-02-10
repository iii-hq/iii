// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

use axum::{Json, http::header::HeaderMap};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
pub struct APIrequest {
    pub query_params: HashMap<String, String>,
    pub path_params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub path: String,
    pub method: String,
    pub body: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<TriggerMetadata>,
}

impl APIrequest {
    pub fn new(
        query_params: HashMap<String, String>,
        path_params: HashMap<String, String>,
        headers: HeaderMap,
        path: String,
        method: String,
        body: Option<Json<Value>>,
    ) -> Self {
        let body_value = body.map(|Json(v)| v).unwrap_or(serde_json::json!({}));
        APIrequest {
            query_params,
            path_params,
            headers: headers
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
                .collect(),
            path: path.clone(),
            method: method.clone(),
            body: body_value,
            trigger: Some(TriggerMetadata {
                trigger_type: "api".to_string(),
                path: Some(path),
                method: Some(method),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct APIresponse {
    pub status_code: u16,
    pub headers: Vec<String>,
    pub body: Value,
}

impl APIresponse {
    pub fn from_function_return(value: Value) -> Self {
        let status_code = value
            .get("status_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(200) as u16;
        let headers = value
            .get("headers")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        let body = value.get("body").cloned().unwrap_or(json!({}));
        APIresponse {
            status_code,
            headers,
            body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderName, HeaderValue};

    #[test]
    fn test_apiresponse_from_function_return_full() {
        let value = json!({
            "status_code": 201,
            "headers": ["Content-Type: application/json", "X-Custom: value"],
            "body": {"message": "created", "id": 123}
        });
        let response = APIresponse::from_function_return(value);
        assert_eq!(response.status_code, 201);
        assert_eq!(response.headers.len(), 2);
        assert_eq!(response.headers[0], "Content-Type: application/json");
        assert_eq!(response.headers[1], "X-Custom: value");
        assert_eq!(response.body["message"], "created");
        assert_eq!(response.body["id"], 123);
    }

    #[test]
    fn test_apiresponse_from_function_return_missing_fields() {
        let value = json!({});
        let response = APIresponse::from_function_return(value);
        assert_eq!(response.status_code, 200);
        assert_eq!(response.headers.len(), 0);
        assert_eq!(response.body, json!({}));
    }

    #[test]
    fn test_apiresponse_from_function_return_missing_status_code() {
        let value = json!({
            "headers": ["Content-Type: text/plain"],
            "body": {"data": "test"}
        });
        let response = APIresponse::from_function_return(value);
        assert_eq!(response.status_code, 200);
        assert_eq!(response.headers.len(), 1);
        assert_eq!(response.body["data"], "test");
    }

    #[test]
    fn test_apiresponse_from_function_return_missing_headers() {
        let value = json!({
            "status_code": 404,
            "body": {"error": "not found"}
        });
        let response = APIresponse::from_function_return(value);
        assert_eq!(response.status_code, 404);
        assert_eq!(response.headers.len(), 0);
        assert_eq!(response.body["error"], "not found");
    }

    #[test]
    fn test_apiresponse_from_function_return_missing_body() {
        let value = json!({
            "status_code": 204,
            "headers": []
        });
        let response = APIresponse::from_function_return(value);
        assert_eq!(response.status_code, 204);
        assert_eq!(response.headers.len(), 0);
        assert_eq!(response.body, json!({}));
    }

    #[test]
    fn test_apiresponse_from_function_return_empty_headers_array() {
        let value = json!({
            "status_code": 200,
            "headers": [],
            "body": {"data": "test"}
        });
        let response = APIresponse::from_function_return(value);
        assert_eq!(response.status_code, 200);
        assert_eq!(response.headers.len(), 0);
        assert_eq!(response.body["data"], "test");
    }

    #[test]
    fn test_apiresponse_from_function_return_non_string_headers() {
        let value = json!({
            "status_code": 200,
            "headers": ["valid-header", 123, null, "another-header"],
            "body": {}
        });
        let response = APIresponse::from_function_return(value);
        assert_eq!(response.headers.len(), 2);
        assert_eq!(response.headers[0], "valid-header");
        assert_eq!(response.headers[1], "another-header");
    }

    #[test]
    fn test_apiresponse_from_function_return_nested_body() {
        let value = json!({
            "status_code": 200,
            "body": {
                "user": {
                    "id": 1,
                    "name": "test",
                    "metadata": {
                        "role": "admin"
                    }
                }
            }
        });
        let response = APIresponse::from_function_return(value);
        assert_eq!(response.body["user"]["id"], 1);
        assert_eq!(response.body["user"]["name"], "test");
        assert_eq!(response.body["user"]["metadata"]["role"], "admin");
    }

    #[test]
    fn test_apirequest_new_all_fields() {
        let mut query_params = HashMap::new();
        query_params.insert("key1".to_string(), "value1".to_string());
        let mut path_params = HashMap::new();
        path_params.insert("id".to_string(), "123".to_string());
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        let body = Some(Json(json!({"data": "test"})));

        let request = APIrequest::new(
            query_params.clone(),
            path_params.clone(),
            headers,
            "/api/users/:id".to_string(),
            "POST".to_string(),
            body,
        );

        assert_eq!(request.query_params, query_params);
        assert_eq!(request.path_params, path_params);
        assert_eq!(request.path, "/api/users/:id");
        assert_eq!(request.method, "POST");
        assert_eq!(request.body["data"], "test");
        assert!(request.trigger.is_some());
        assert_eq!(request.trigger.as_ref().unwrap().trigger_type, "api");
        assert_eq!(request.trigger.as_ref().unwrap().path, Some("/api/users/:id".to_string()));
        assert_eq!(request.trigger.as_ref().unwrap().method, Some("POST".to_string()));
    }

    #[test]
    fn test_apirequest_new_empty_body() {
        let query_params = HashMap::new();
        let path_params = HashMap::new();
        let headers = HeaderMap::new();

        let request = APIrequest::new(
            query_params,
            path_params,
            headers,
            "/api/test".to_string(),
            "GET".to_string(),
            None,
        );

        assert_eq!(request.body, json!({}));
        assert_eq!(request.path, "/api/test");
        assert_eq!(request.method, "GET");
    }

    #[test]
    fn test_apirequest_new_empty_query_params() {
        let query_params = HashMap::new();
        let path_params = HashMap::new();
        let headers = HeaderMap::new();

        let request = APIrequest::new(
            query_params,
            path_params,
            headers,
            "/api/test".to_string(),
            "GET".to_string(),
            None,
        );

        assert_eq!(request.query_params.len(), 0);
    }

    #[test]
    fn test_apirequest_new_headers_conversion() {
        let query_params = HashMap::new();
        let path_params = HashMap::new();
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("authorization", HeaderValue::from_static("Bearer token"));

        let request = APIrequest::new(
            query_params,
            path_params,
            headers,
            "/api/test".to_string(),
            "GET".to_string(),
            None,
        );

        assert_eq!(request.headers.len(), 2);
        assert_eq!(request.headers.get("content-type"), Some(&"application/json".to_string()));
        assert_eq!(request.headers.get("authorization"), Some(&"Bearer token".to_string()));
    }

    #[test]
    fn test_trigger_metadata_serde_with_all_fields() {
        let metadata = TriggerMetadata {
            trigger_type: "api".to_string(),
            path: Some("/api/users".to_string()),
            method: Some("GET".to_string()),
        };
        let json = serde_json::to_value(&metadata).unwrap();
        assert_eq!(json["type"], "api");
        assert_eq!(json["path"], "/api/users");
        assert_eq!(json["method"], "GET");

        let deserialized: TriggerMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.trigger_type, "api");
        assert_eq!(deserialized.path, Some("/api/users".to_string()));
        assert_eq!(deserialized.method, Some("GET".to_string()));
    }

    #[test]
    fn test_trigger_metadata_serde_without_optional_fields() {
        let metadata = TriggerMetadata {
            trigger_type: "api".to_string(),
            path: None,
            method: None,
        };
        let json = serde_json::to_value(&metadata).unwrap();
        assert_eq!(json["type"], "api");
        assert!(!json.as_object().unwrap().contains_key("path"));
        assert!(!json.as_object().unwrap().contains_key("method"));

        let deserialized: TriggerMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.trigger_type, "api");
        assert_eq!(deserialized.path, None);
        assert_eq!(deserialized.method, None);
    }
}
