// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::invocation::{auth::HttpAuthConfig, method::HttpMethod};

/// Namespace used when a message carries no explicit `namespace`.
pub const DEFAULT_NAMESPACE: &str = "default";

/// Returns the effective namespace for an optional wire-level namespace,
/// falling back to [`DEFAULT_NAMESPACE`] when absent.
pub fn effective_namespace(ns: &Option<String>) -> &str {
    ns.as_deref().unwrap_or(DEFAULT_NAMESPACE)
}

/// [`DEFAULT_NAMESPACE`] as an owned `String`. Used as a `#[serde(default)]`
/// for owned namespace fields so payloads that predate the field decode to
/// `default` rather than failing.
pub fn default_namespace() -> String {
    DEFAULT_NAMESPACE.to_string()
}

/// [`Message::RegistrationRejected`] code: another live worker already runs
/// under this name in this namespace.
///
/// Defined, but **not emitted yet** — deferred to Task 9.5, after the SDKs learn
/// to handle `RegistrationRejected`. Rejecting a worker means closing its
/// connection, and today's SDKs would simply reconnect and be rejected again,
/// forever. See `.superpowers/sdd/task-4-report.md` for the `hostname:pid`
/// naming problem this code has to solve before it can ship.
pub const WORKER_NAMESPACE_CONFLICT: &str = "WORKER_NAMESPACE_CONFLICT";

/// [`Message::RegistrationRejected`] code: another live worker in this
/// namespace already exports this function id. Only that one registration is
/// refused; the connection stays open.
pub const FUNCTION_NAMESPACE_CONFLICT: &str = "FUNCTION_NAMESPACE_CONFLICT";

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
    pub auth: Option<HttpAuthConfig>,
}

fn default_http_method() -> HttpMethod {
    HttpMethod::Post
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TriggerAction {
    Enqueue { queue: String },
    Void,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    RegisterTriggerType {
        id: String,
        description: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        trigger_request_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_request_format: Option<Value>,
    },
    RegisterTrigger {
        id: String,
        trigger_type: String,
        function_id: String,
        config: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<TriggerAction>,
        /// Per-invocation metadata sidecar, delivered to the target handler as a
        /// distinct argument (not folded into `data`). Optional and additive:
        /// omitted when absent, so older peers that don't send/expect it stay
        /// wire-compatible.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        /// Target namespace for routing. Optional and additive: absent means
        /// [`DEFAULT_NAMESPACE`], so older peers that don't send it stay
        /// wire-compatible.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
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
        #[serde(default)]
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_service_id: Option<String>,
    },
    Ping,
    Pong,
    /// Worker→engine, sent as the first message of a reconnect, before the
    /// registration replay: `previous_worker_id` and `reattach_token` are
    /// the values the engine handed this worker via `WorkerRegistered` on
    /// its previous connection. The engine retires that previous connection
    /// (same teardown as a disconnect) so the replay lands on a clean slate
    /// instead of racing the old connection's cleanup. The token is
    /// required: worker ids are publicly discoverable, the token was only
    /// ever sent over the previous connection's own socket. Old engines
    /// warn-log and ignore unknown message types, so this is version-skew
    /// safe.
    Reattach {
        previous_worker_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reattach_token: Option<String>,
    },
    WorkerRegistered {
        worker_id: String,
        /// Secret the worker must present in `Reattach` to retire this
        /// connection after a reconnect. Optional on the wire so older
        /// SDKs/engines interop cleanly.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reattach_token: Option<String>,
    },
    RegistrationRejected {
        code: String,
        namespace: String,
        worker_name: String,
        owner_worker_id: String,
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stacktrace: Option<String>,
}

impl ErrorBody {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            stacktrace: None,
        }
    }
}

impl std::fmt::Display for ErrorBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChannelDirection {
    #[default]
    Read,
    Write,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct StreamChannelRef {
    pub channel_id: String,
    pub access_key: String,
    pub direction: ChannelDirection,
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_NAMESPACE, Message, TriggerAction, effective_namespace};
    use crate::{
        invocation::{auth::HttpAuthConfig, method::HttpMethod},
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
    fn reattach_round_trips() {
        // Token present.
        let raw = r#"{"type":"reattach","previous_worker_id":"abc-123","reattach_token":"tok-1"}"#;
        let message: Message = serde_json::from_str(raw).expect("reattach should deserialize");
        match &message {
            Message::Reattach {
                previous_worker_id,
                reattach_token,
            } => {
                assert_eq!(previous_worker_id, "abc-123");
                assert_eq!(reattach_token.as_deref(), Some("tok-1"));
            }
            _ => panic!("unexpected message variant"),
        }
        let serialized = serde_json::to_string(&message).expect("serialize");
        assert_eq!(serialized, raw);

        // Token absent (older SDK): still deserializes, token None.
        let raw = r#"{"type":"reattach","previous_worker_id":"abc-123"}"#;
        let message: Message = serde_json::from_str(raw).expect("tokenless reattach deserializes");
        assert!(matches!(
            message,
            Message::Reattach {
                reattach_token: None,
                ..
            }
        ));
    }

    #[test]
    fn worker_registered_token_is_optional_on_the_wire() {
        // Older engines omit the token; SDKs must still parse the frame.
        let raw = r#"{"type":"workerregistered","worker_id":"w-1"}"#;
        let message: Message = serde_json::from_str(raw).expect("should deserialize");
        assert!(matches!(
            message,
            Message::WorkerRegistered {
                reattach_token: None,
                ..
            }
        ));
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
                    Some(HttpAuthConfig::Bearer { token_key }) => {
                        assert_eq!(token_key, "LAMBDA_TOKEN");
                    }
                    _ => panic!("unexpected auth variant"),
                }
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn deserialize_invoke_function_with_enqueue_action() {
        let raw = r#"{
            "type": "invokefunction",
            "invocation_id": null,
            "function_id": "payment.process",
            "data": {"amount": 100},
            "action": {"type": "enqueue", "queue": "payment"}
        }"#;
        let message: Message = serde_json::from_str(raw).expect("message should deserialize");

        match message {
            Message::InvokeFunction {
                function_id,
                action,
                ..
            } => {
                assert_eq!(function_id, "payment.process");
                match action {
                    Some(TriggerAction::Enqueue { queue }) => {
                        assert_eq!(queue, "payment");
                    }
                    other => panic!("expected Enqueue action, got {other:?}"),
                }
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn deserialize_invoke_function_with_void_action() {
        let raw = r#"{
            "type": "invokefunction",
            "invocation_id": null,
            "function_id": "audit.log",
            "data": {"event": "login"},
            "action": {"type": "void"}
        }"#;
        let message: Message = serde_json::from_str(raw).expect("message should deserialize");

        match message {
            Message::InvokeFunction {
                function_id,
                action,
                ..
            } => {
                assert_eq!(function_id, "audit.log");
                assert!(
                    matches!(action, Some(TriggerAction::Void)),
                    "expected Void action, got {action:?}"
                );
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn deserialize_invoke_function_without_action() {
        let raw = r#"{
            "type": "invokefunction",
            "invocation_id": null,
            "function_id": "sync.call",
            "data": {}
        }"#;
        let message: Message = serde_json::from_str(raw).expect("message should deserialize");

        match message {
            Message::InvokeFunction {
                function_id,
                action,
                ..
            } => {
                assert_eq!(function_id, "sync.call");
                assert!(action.is_none(), "expected no action, got {action:?}");
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn serialize_invoke_function_without_action_omits_field() {
        let msg = Message::InvokeFunction {
            invocation_id: None,
            function_id: "test.fn".to_string(),
            data: serde_json::json!({}),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
        };
        let json = serde_json::to_string(&msg).expect("message should serialize");

        assert!(
            !json.contains("\"action\""),
            "action field should be omitted when None, got: {json}"
        );
        assert!(
            !json.contains("\"traceparent\""),
            "traceparent field should be omitted when None, got: {json}"
        );
        assert!(
            !json.contains("\"baggage\""),
            "baggage field should be omitted when None, got: {json}"
        );
        assert!(
            !json.contains("\"metadata\""),
            "metadata field should be omitted when None, got: {json}"
        );
    }

    #[test]
    fn invoke_function_metadata_roundtrips_and_is_backward_compatible() {
        // Present: metadata survives a serialize → deserialize round-trip.
        let msg = Message::InvokeFunction {
            invocation_id: None,
            function_id: "test.fn".to_string(),
            data: serde_json::json!({ "k": 1 }),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: Some(serde_json::json!({ "session_id": "s_1" })),
            namespace: None,
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(json.contains("\"metadata\""));
        match serde_json::from_str::<Message>(&json).expect("deserialize") {
            Message::InvokeFunction { metadata, data, .. } => {
                assert_eq!(metadata, Some(serde_json::json!({ "session_id": "s_1" })));
                // Payload is untouched — metadata is not folded into `data`.
                assert_eq!(data, serde_json::json!({ "k": 1 }));
            }
            _ => panic!("expected InvokeFunction"),
        }

        // Backward compatible: a payload from an older peer that omits the
        // field deserializes to `metadata: None`.
        let legacy =
            r#"{"type":"invokefunction","invocation_id":null,"function_id":"f","data":{}}"#;
        match serde_json::from_str::<Message>(legacy).expect("deserialize legacy") {
            Message::InvokeFunction { metadata, .. } => assert_eq!(metadata, None),
            _ => panic!("expected InvokeFunction"),
        }
    }

    #[test]
    fn effective_namespace_defaults_when_absent() {
        assert_eq!(effective_namespace(&None), DEFAULT_NAMESPACE);
        assert_eq!(effective_namespace(&Some("orders".to_string())), "orders");
    }

    #[test]
    fn invoke_function_serializes_namespace_when_set() {
        let msg = Message::InvokeFunction {
            invocation_id: None,
            function_id: "state::get".into(),
            data: serde_json::json!({}),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: Some("orders".into()),
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains(r#""namespace":"orders""#));
    }

    #[test]
    fn invoke_function_omits_namespace_when_absent_and_parses_legacy() {
        // legacy wire (sem namespace) continua parseando
        let json = r#"{"type":"invokefunction","invocation_id":null,"function_id":"state::get","data":{}}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        match msg {
            Message::InvokeFunction { namespace, .. } => assert!(namespace.is_none()),
            _ => panic!("expected InvokeFunction"),
        }
    }

    #[test]
    fn registration_rejected_roundtrip() {
        let msg = Message::RegistrationRejected {
            code: "WORKER_NAMESPACE_CONFLICT".into(),
            namespace: "orders".into(),
            worker_name: "state".into(),
            owner_worker_id: "abc".into(),
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains(r#""code":"WORKER_NAMESPACE_CONFLICT""#));
        let back: Message = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, Message::RegistrationRejected { .. }));
    }
}
