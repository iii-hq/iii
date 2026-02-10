// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

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
        trigger_type: String,
    },
    RegisterFunction {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        request_format: Option<Value>,
        response_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_message_register_trigger_type_round_trip() {
        let msg = Message::RegisterTriggerType {
            id: "trigger-type-1".to_string(),
            description: "Test trigger type".to_string(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "registertriggertype");
        assert_eq!(json["id"], "trigger-type-1");
        assert_eq!(json["description"], "Test trigger type");

        let deserialized: Message = serde_json::from_value(json).unwrap();
        match deserialized {
            Message::RegisterTriggerType { id, description } => {
                assert_eq!(id, "trigger-type-1");
                assert_eq!(description, "Test trigger type");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_message_register_trigger_round_trip() {
        let msg = Message::RegisterTrigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            config: json!({"schedule": "0 0 * * *"}),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "registertrigger");
        assert_eq!(json["id"], "trigger-1");
        assert_eq!(json["trigger_type"], "cron");
        assert_eq!(json["function_id"], "test.function");

        let deserialized: Message = serde_json::from_value(json).unwrap();
        match deserialized {
            Message::RegisterTrigger {
                id,
                trigger_type,
                function_id,
                config: _,
            } => {
                assert_eq!(id, "trigger-1");
                assert_eq!(trigger_type, "cron");
                assert_eq!(function_id, "test.function");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_message_trigger_registration_result_with_error() {
        let msg = Message::TriggerRegistrationResult {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            error: Some(ErrorBody {
                code: "INVALID_CONFIG".to_string(),
                message: "Invalid schedule".to_string(),
            }),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "triggerregistrationresult");
        assert!(json["error"].is_object());
        assert_eq!(json["error"]["code"], "INVALID_CONFIG");
    }

    #[test]
    fn test_message_trigger_registration_result_without_error() {
        let msg = Message::TriggerRegistrationResult {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
            function_id: "test.function".to_string(),
            error: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "triggerregistrationresult");
        assert!(!json.as_object().unwrap().contains_key("error"));
    }

    #[test]
    fn test_message_unregister_trigger_round_trip() {
        let msg = Message::UnregisterTrigger {
            id: "trigger-1".to_string(),
            trigger_type: "cron".to_string(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "unregistertrigger");
        assert_eq!(json["id"], "trigger-1");
        assert_eq!(json["trigger_type"], "cron");

        let deserialized: Message = serde_json::from_value(json).unwrap();
        match deserialized {
            Message::UnregisterTrigger { id, trigger_type } => {
                assert_eq!(id, "trigger-1");
                assert_eq!(trigger_type, "cron");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_message_register_function_with_all_fields() {
        let msg = Message::RegisterFunction {
            id: "test.function".to_string(),
            description: Some("Test function".to_string()),
            request_format: Some(json!({"type": "object"})),
            response_format: Some(json!({"type": "string"})),
            metadata: Some(json!({"version": "1.0"})),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "registerfunction");
        assert_eq!(json["id"], "test.function");
        assert_eq!(json["description"], "Test function");
        assert!(json["request_format"].is_object());
        assert!(json["response_format"].is_object());
        assert!(json["metadata"].is_object());
    }

    #[test]
    fn test_message_register_function_without_optional_fields() {
        let msg = Message::RegisterFunction {
            id: "test.function".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "registerfunction");
        assert_eq!(json["id"], "test.function");
        assert!(!json.as_object().unwrap().contains_key("description"));
        assert!(json["request_format"].is_null());
        assert!(json["response_format"].is_null());
        assert!(!json.as_object().unwrap().contains_key("metadata"));
    }

    #[test]
    fn test_message_invoke_function_round_trip() {
        let invocation_id = Uuid::new_v4();
        let msg = Message::InvokeFunction {
            invocation_id: Some(invocation_id),
            function_id: "test.function".to_string(),
            data: json!({"key": "value"}),
            traceparent: Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_string()),
            baggage: Some("key=value".to_string()),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "invokefunction");
        assert_eq!(json["function_id"], "test.function");
        assert_eq!(json["data"]["key"], "value");
        assert_eq!(json["traceparent"], "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01");
        assert_eq!(json["baggage"], "key=value");

        let deserialized: Message = serde_json::from_value(json).unwrap();
        match deserialized {
            Message::InvokeFunction {
                invocation_id: id,
                function_id,
                data: _,
                traceparent: _,
                baggage: _,
            } => {
                assert_eq!(id, Some(invocation_id));
                assert_eq!(function_id, "test.function");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_message_invoke_function_without_optional_fields() {
        let msg = Message::InvokeFunction {
            invocation_id: None,
            function_id: "test.function".to_string(),
            data: json!({"key": "value"}),
            traceparent: None,
            baggage: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "invokefunction");
        assert_eq!(json["function_id"], "test.function");
        assert!(json["invocation_id"].is_null());
        assert!(!json.as_object().unwrap().contains_key("traceparent"));
        assert!(!json.as_object().unwrap().contains_key("baggage"));
    }

    #[test]
    fn test_message_invocation_result_with_result() {
        let invocation_id = Uuid::new_v4();
        let msg = Message::InvocationResult {
            invocation_id,
            function_id: "test.function".to_string(),
            result: Some(json!({"output": "success"})),
            error: None,
            traceparent: None,
            baggage: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "invocationresult");
        assert_eq!(json["function_id"], "test.function");
        assert_eq!(json["result"]["output"], "success");
        assert!(!json.as_object().unwrap().contains_key("error"));
    }

    #[test]
    fn test_message_invocation_result_with_error() {
        let invocation_id = Uuid::new_v4();
        let msg = Message::InvocationResult {
            invocation_id,
            function_id: "test.function".to_string(),
            result: None,
            error: Some(ErrorBody {
                code: "EXECUTION_ERROR".to_string(),
                message: "Function failed".to_string(),
            }),
            traceparent: None,
            baggage: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "invocationresult");
        assert_eq!(json["error"]["code"], "EXECUTION_ERROR");
        assert_eq!(json["error"]["message"], "Function failed");
        assert!(!json.as_object().unwrap().contains_key("result"));
    }

    #[test]
    fn test_message_register_service_round_trip() {
        let msg = Message::RegisterService {
            id: "service-1".to_string(),
            name: "test-service".to_string(),
            description: Some("Test service".to_string()),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "registerservice");
        assert_eq!(json["id"], "service-1");
        assert_eq!(json["name"], "test-service");
        assert_eq!(json["description"], "Test service");

        let deserialized: Message = serde_json::from_value(json).unwrap();
        match deserialized {
            Message::RegisterService { id, name, description } => {
                assert_eq!(id, "service-1");
                assert_eq!(name, "test-service");
                assert_eq!(description, Some("Test service".to_string()));
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_message_register_service_without_description() {
        let msg = Message::RegisterService {
            id: "service-1".to_string(),
            name: "test-service".to_string(),
            description: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert!(!json.as_object().unwrap().contains_key("description"));
    }

    #[test]
    fn test_message_ping_pong() {
        let ping = Message::Ping;
        let json = serde_json::to_value(&ping).unwrap();
        assert_eq!(json["type"], "ping");

        let deserialized: Message = serde_json::from_value(json).unwrap();
        match deserialized {
            Message::Ping => {}
            _ => panic!("Wrong variant"),
        }

        let pong = Message::Pong;
        let json = serde_json::to_value(&pong).unwrap();
        assert_eq!(json["type"], "pong");

        let deserialized: Message = serde_json::from_value(json).unwrap();
        match deserialized {
            Message::Pong => {}
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_message_worker_registered_round_trip() {
        let msg = Message::WorkerRegistered {
            worker_id: "worker-123".to_string(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "workerregistered");
        assert_eq!(json["worker_id"], "worker-123");

        let deserialized: Message = serde_json::from_value(json).unwrap();
        match deserialized {
            Message::WorkerRegistered { worker_id } => {
                assert_eq!(worker_id, "worker-123");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_worker_metrics_full_serialization() {
        let metrics = WorkerMetrics {
            memory_heap_used: Some(1024 * 1024),
            memory_heap_total: Some(2 * 1024 * 1024),
            memory_rss: Some(3 * 1024 * 1024),
            memory_external: Some(512 * 1024),
            cpu_user_micros: Some(1000000),
            cpu_system_micros: Some(500000),
            cpu_percent: Some(25.5),
            event_loop_lag_ms: Some(1.5),
            uptime_seconds: Some(3600),
            timestamp_ms: 1234567890,
            runtime: "node".to_string(),
        };
        let json = serde_json::to_value(&metrics).unwrap();
        assert_eq!(json["memory_heap_used"], 1024 * 1024);
        assert_eq!(json["memory_heap_total"], 2 * 1024 * 1024);
        assert_eq!(json["cpu_percent"], 25.5);
        assert_eq!(json["timestamp_ms"], 1234567890);
        assert_eq!(json["runtime"], "node");

        let deserialized: WorkerMetrics = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.memory_heap_used, Some(1024 * 1024));
        assert_eq!(deserialized.runtime, "node");
    }

    #[test]
    fn test_worker_metrics_minimal_serialization() {
        let metrics = WorkerMetrics {
            memory_heap_used: None,
            memory_heap_total: None,
            memory_rss: None,
            memory_external: None,
            cpu_user_micros: None,
            cpu_system_micros: None,
            cpu_percent: None,
            event_loop_lag_ms: None,
            uptime_seconds: None,
            timestamp_ms: 1234567890,
            runtime: "rust".to_string(),
        };
        let json = serde_json::to_value(&metrics).unwrap();
        assert!(!json.as_object().unwrap().contains_key("memory_heap_used"));
        assert!(!json.as_object().unwrap().contains_key("cpu_percent"));
        assert_eq!(json["timestamp_ms"], 1234567890);
        assert_eq!(json["runtime"], "rust");

        let deserialized: WorkerMetrics = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.memory_heap_used, None);
        assert_eq!(deserialized.timestamp_ms, 1234567890);
    }

    #[test]
    fn test_error_body_round_trip() {
        let error = ErrorBody {
            code: "TEST_ERROR".to_string(),
            message: "Test error message".to_string(),
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], "TEST_ERROR");
        assert_eq!(json["message"], "Test error message");

        let deserialized: ErrorBody = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.code, "TEST_ERROR");
        assert_eq!(deserialized.message, "Test error message");
    }
}
