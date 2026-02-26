// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

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
    pub headers: Vec<String>,
    pub body: Value,
}

impl HttpResponse {
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
        HttpResponse {
            status_code,
            headers,
            body,
        }
    }
}
