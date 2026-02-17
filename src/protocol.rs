// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::invocation::{auth::HttpAuthRef, method::HttpMethod};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpInvocationRef {
    pub url: String,
    #[serde(default = "default_http_method")]
    pub method: HttpMethod,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub auth: Option<HttpAuthRef>,
}

fn default_http_method() -> HttpMethod {
    HttpMethod::Post
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    RegisterTriggerType {
        id: String,
        description: String,
    },
    RegisterTrigger {
        id: String,
        trigger_type: String,
        function_id: String,
        config: Value,
    },
    TriggerRegistrationResult {
        id: String,
        trigger_type: String,
        function_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
    },
    UnregisterTrigger {
        id: String,
        #[serde(default)]
        trigger_type: Option<String>,
    },
    RegisterFunction {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        request_format: Option<Value>,
        response_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        invocation: Option<HttpInvocationRef>,
    },
    UnregisterFunction {
        id: String,
    },
    InvokeFunction {
        invocation_id: Option<Uuid>,
        function_id: String,
        data: Value,
        /// W3C trace-context traceparent header for distributed tracing
        #[serde(skip_serializing_if = "Option::is_none")]
        traceparent: Option<String>,
        /// W3C baggage header for cross-cutting context propagation
        #[serde(skip_serializing_if = "Option::is_none")]
        baggage: Option<String>,
    },
    InvocationResult {
        invocation_id: Uuid,
        function_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
        /// W3C trace-context traceparent header for distributed tracing
        #[serde(skip_serializing_if = "Option::is_none")]
        traceparent: Option<String>,
        /// W3C baggage header for cross-cutting context propagation
        #[serde(skip_serializing_if = "Option::is_none")]
        baggage: Option<String>,
    },
    RegisterService {
        id: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Ping,
    Pong,
    WorkerRegistered {
        worker_id: String,
    },
}

/// Worker resource metrics for health monitoring.
///
/// # JavaScript Precision Note
///
/// The `u64` fields (`memory_*`, `cpu_*_micros`, `uptime_seconds`, `timestamp_ms`)
/// can theoretically exceed JavaScript's `Number.MAX_SAFE_INTEGER` (2^53 - 1).
/// In practice:
/// - Memory values would need to exceed ~9 PB to lose precision
/// - CPU microseconds would need ~285 years of continuous uptime
/// - Timestamps are safe until the year 287396
///
/// For most use cases this is not a concern, but if you need guaranteed precision
/// for very large values, consider parsing these as BigInt on the JavaScript side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMetrics {
    // Memory metrics (bytes)
    // Note: u64 values above 2^53-1 may lose precision in JavaScript
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_heap_used: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_heap_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_rss: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_external: Option<u64>,

    // CPU metrics (microseconds since process start)
    // Note: u64 values above 2^53-1 may lose precision in JavaScript
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_user_micros: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_system_micros: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<f64>,

    // Runtime metrics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_loop_lag_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_seconds: Option<u64>,

    // Metadata
    pub timestamp_ms: u64,
    pub runtime: String, // "node", "rust", "python", etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChannelDirection {
    #[default]
    Read,
    Write,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StreamChannelRef {
    pub channel_id: String,
    pub access_key: String,
    pub direction: ChannelDirection,
}

#[cfg(test)]
mod tests {
    use super::Message;
    use crate::{
        invocation::{auth::HttpAuthRef, method::HttpMethod},
        protocol::HttpInvocationRef,
    };

    #[test]
    fn deserialize_unregister_trigger_without_type() {
        let raw = r#"{"type":"unregistertrigger","id":"abc"}"#;
        let message: Message = serde_json::from_str(raw).expect("message should deserialize");

        match message {
            Message::UnregisterTrigger { id, trigger_type } => {
                assert_eq!(id, "abc");
                assert_eq!(trigger_type, None);
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn deserialize_unregister_trigger_with_type() {
        let raw = r#"{"type":"unregistertrigger","id":"abc","trigger_type":"http"}"#;
        let message: Message = serde_json::from_str(raw).expect("message should deserialize");

        match message {
            Message::UnregisterTrigger { id, trigger_type } => {
                assert_eq!(id, "abc");
                assert_eq!(trigger_type.as_deref(), Some("http"));
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn deserialize_register_function_with_http_invocation() {
        let raw = r#"{
            "type":"registerfunction",
            "id":"external.my_lambda",
            "description":"External Lambda function",
            "invocation":{
                "url":"https://example.com/lambda",
                "timeout_ms":30000,
                "headers":{"x-custom-header":"value"},
                "auth":{"type":"bearer","token_key":"LAMBDA_TOKEN"}
            }
        }"#;
        let message: Message = serde_json::from_str(raw).expect("message should deserialize");

        match message {
            Message::RegisterFunction {
                id,
                description,
                invocation,
                ..
            } => {
                assert_eq!(id, "external.my_lambda");
                assert_eq!(description.as_deref(), Some("External Lambda function"));

                let HttpInvocationRef {
                    url,
                    method,
                    timeout_ms,
                    headers,
                    auth,
                } = invocation.expect("invocation should be present");

                assert_eq!(url, "https://example.com/lambda");
                assert!(matches!(method, HttpMethod::Post));
                assert_eq!(timeout_ms, Some(30000));
                assert_eq!(
                    headers.get("x-custom-header").map(String::as_str),
                    Some("value")
                );
                match auth {
                    Some(HttpAuthRef::Bearer { token_key }) => {
                        assert_eq!(token_key, "LAMBDA_TOKEN");
                    }
                    _ => panic!("unexpected auth variant"),
                }
            }
            _ => panic!("unexpected message variant"),
        }
    }
}
