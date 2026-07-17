use iii_helpers::http::HttpInvocationConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// [`Message::RegistrationRejected`] code: another live worker already holds
/// this `(namespace, worker_name)`. The engine closes the connection; the SDK
/// must stop and not reconnect. Mirrors the engine constant of the same name.
pub const WORKER_NAMESPACE_CONFLICT: &str = "WORKER_NAMESPACE_CONFLICT";

/// [`Message::RegistrationRejected`] code: another live worker in this
/// namespace already exports this function id. Only that one registration is
/// refused; the connection stays open and the worker keeps serving its other
/// functions. Mirrors the engine constant of the same name.
pub const FUNCTION_NAMESPACE_CONFLICT: &str = "FUNCTION_NAMESPACE_CONFLICT";

/// Routing action for [`TriggerRequest`]. Determines how the engine handles
/// the invocation.
///
/// - `Enqueue`: Routes through a named queue for async processing.
/// - `Void`: Fire-and-forget, no response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TriggerAction {
    /// Routes the invocation through a named queue.
    Enqueue { queue: String },
    /// Fire-and-forget routing.
    Void,
}

/// Request object for `trigger()`.
///
/// ```rust
/// # use iii_sdk::protocol::{TriggerRequest, TriggerAction};
/// # use serde_json::json;
/// // Simple call
/// TriggerRequest {
///     function_id: "my::function".to_string(),
///     payload: json!({ "key": "value" }),
///     action: None,
///     timeout_ms: None,
/// };
///
/// // With action
/// TriggerRequest {
///     function_id: "my::function".to_string(),
///     payload: json!({}),
///     action: Some(TriggerAction::Enqueue { queue: "payments".to_string() }),
///     timeout_ms: None,
/// };
///
/// // With metadata
/// TriggerRequest {
///     function_id: "my::function".to_string(),
///     payload: json!({}),
///     action: None,
///     timeout_ms: None,
/// }
/// .metadata(json!({ "tenant": "acme" }));
/// ```
#[derive(Debug, Clone)]
pub struct TriggerRequest {
    /// ID of the function to invoke.
    pub function_id: String,
    /// Input data passed to the function.
    pub payload: Value,
    /// Sets how the trigger is routed. `None` for a synchronous request/response.
    /// Set a routing scheme otherwise (e.g. `TriggerAction::Enqueue { .. }`, `TriggerAction::Void`).
    pub action: Option<TriggerAction>,
    /// Override the default invocation timeout, in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl TriggerRequest {
    /// Attach per-invocation metadata without adding a required field to
    /// [`TriggerRequest`] struct literals.
    pub fn metadata(self, metadata: Value) -> TriggerRequestWithMetadata {
        TriggerRequestWithMetadata {
            request: self,
            metadata: Some(metadata),
            namespace: None,
        }
    }

    /// Target a specific namespace for this invocation without adding a
    /// required field to [`TriggerRequest`] struct literals. Serializes into
    /// [`Message::InvokeFunction`]'s `namespace`; omitted when unset (the
    /// engine then routes within its default namespace).
    pub fn namespace(self, namespace: impl Into<String>) -> TriggerRequestWithMetadata {
        TriggerRequestWithMetadata {
            request: self,
            metadata: None,
            namespace: Some(namespace.into()),
        }
    }
}

/// Trigger request plus optional per-invocation metadata and target namespace.
#[derive(Debug, Clone)]
pub struct TriggerRequestWithMetadata {
    pub(crate) request: TriggerRequest,
    pub(crate) metadata: Option<Value>,
    pub(crate) namespace: Option<String>,
}

impl TriggerRequestWithMetadata {
    /// Attach per-invocation metadata.
    pub fn metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Target a specific namespace for this invocation.
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }
}

impl<T> From<T> for TriggerRequestWithMetadata
where
    T: Into<TriggerRequest>,
{
    fn from(request: T) -> Self {
        Self {
            request: request.into(),
            metadata: None,
            namespace: None,
        }
    }
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
        trigger_type: String,
    },
    UnregisterTriggerType {
        id: String,
    },
    RegisterFunction {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        invocation: Option<HttpInvocationConfig>,
    },
    UnregisterFunction {
        id: String,
    },
    InvokeFunction {
        invocation_id: Option<Uuid>,
        function_id: String,
        data: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        traceparent: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        baggage: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<TriggerAction>,
        /// Per-invocation metadata sidecar, surfaced to the handler as a
        /// distinct argument alongside `data`. Optional and additive
        /// for wire compatibility with engines that don't send it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        /// Target namespace for routing. Optional and additive: absent means
        /// the engine's default namespace, so older peers that don't send it
        /// stay wire-compatible.
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
        #[serde(skip_serializing_if = "Option::is_none")]
        traceparent: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        baggage: Option<String>,
    },
    Ping,
    Pong,
    WorkerRegistered {
        worker_id: String,
    },
    /// Pushed by the engine when a registration collides with a live worker in
    /// the same namespace. The `code` distinguishes the two cases:
    /// [`WORKER_NAMESPACE_CONFLICT`] is fatal (the engine closes the connection;
    /// the SDK stops and does not reconnect), while [`FUNCTION_NAMESPACE_CONFLICT`]
    /// refuses a single function id, keeps the connection open, and here
    /// `worker_name` carries the rejected function id.
    RegistrationRejected {
        code: String,
        namespace: String,
        worker_name: String,
        owner_worker_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterTriggerTypeMessage {
    /// Unique identifier for the trigger type (e.g. `state`, `durable:subscriber`).
    pub id: String,
    /// Human-readable description of what this trigger type does.
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_request_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_request_format: Option<Value>,
}

impl RegisterTriggerTypeMessage {
    pub fn to_message(&self) -> Message {
        Message::RegisterTriggerType {
            id: self.id.clone(),
            description: self.description.clone(),
            trigger_request_format: self.trigger_request_format.clone(),
            call_request_format: self.call_request_format.clone(),
        }
    }
}

/// Input for [`IIIClient::register_trigger`](crate::IIIClient::register_trigger).
/// The `id` is auto-generated internally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterTriggerInput {
    /// Identifier of the registered trigger type this trigger uses (e.g. `storage::object-created`, `http`).
    pub trigger_type: String,
    /// ID of the function this trigger invokes when it fires.
    pub function_id: String,
    /// Trigger-type-specific configuration, matching the shape the trigger type expects.
    pub config: Value,
    /// Arbitrary user-specifiable metadata supplied to the triggered handler function on every invocation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterTriggerMessage {
    pub id: String,
    pub trigger_type: String,
    pub function_id: String,
    pub config: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl RegisterTriggerMessage {
    pub fn to_message(&self) -> Message {
        Message::RegisterTrigger {
            id: self.id.clone(),
            trigger_type: self.trigger_type.clone(),
            function_id: self.function_id.clone(),
            config: self.config.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnregisterTriggerMessage {
    pub id: String,
    pub trigger_type: String,
}

impl UnregisterTriggerMessage {
    pub fn to_message(&self) -> Message {
        Message::UnregisterTrigger {
            id: self.id.clone(),
            trigger_type: self.trigger_type.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnregisterTriggerTypeMessage {
    pub id: String,
}

impl UnregisterTriggerTypeMessage {
    pub fn to_message(&self) -> Message {
        Message::UnregisterTriggerType {
            id: self.id.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterFunctionMessage {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invocation: Option<HttpInvocationConfig>,
}

impl RegisterFunctionMessage {
    pub fn with_id(name: String) -> Self {
        RegisterFunctionMessage {
            id: name,
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        }
    }
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }
    pub fn to_message(&self) -> Message {
        Message::RegisterFunction {
            id: self.id.clone(),
            description: self.description.clone(),
            request_format: self.request_format.clone(),
            response_format: self.response_format.clone(),
            metadata: self.metadata.clone(),
            invocation: self.invocation.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionMessage {
    pub function_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stacktrace: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn register_function_to_message_and_serializes_type() {
        let msg = RegisterFunctionMessage {
            id: "functions.echo".to_string(),
            description: Some("Echo function".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        };

        let message = msg.to_message();
        match &message {
            Message::RegisterFunction {
                id, description, ..
            } => {
                assert_eq!(id, "functions.echo");
                assert_eq!(description.as_deref(), Some("Echo function"));
            }
            _ => panic!("unexpected message variant"),
        }

        let serialized = serde_json::to_value(&message).unwrap();
        assert_eq!(serialized["type"], "registerfunction");
        assert_eq!(serialized["id"], "functions.echo");
        assert_eq!(serialized["description"], "Echo function");
    }

    #[test]
    fn register_http_function_serializes_invocation() {
        use iii_helpers::http::{HttpInvocationConfig, HttpMethod};

        let msg = RegisterFunctionMessage {
            id: "external::my_lambda".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: Some(HttpInvocationConfig {
                url: "https://example.com/invoke".to_string(),
                method: HttpMethod::Post,
                timeout_ms: Some(30000),
                headers: HashMap::new(),
                auth: None,
            }),
        };

        let serialized = serde_json::to_value(msg.to_message()).unwrap();
        assert_eq!(serialized["type"], "registerfunction");
        assert_eq!(serialized["id"], "external::my_lambda");
        assert!(serialized["invocation"].is_object());
        assert_eq!(
            serialized["invocation"]["url"],
            "https://example.com/invoke"
        );
        assert_eq!(serialized["invocation"]["method"], "POST");
    }
}
