use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

/// Authentication configuration for HTTP-invoked functions.
///
/// - `Hmac` -- HMAC signature verification using a shared secret.
/// - `Bearer` -- Bearer token authentication.
/// - `ApiKey` -- API key sent via a custom header.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HttpAuthConfig {
    Hmac {
        secret_key: String,
    },
    Bearer {
        token_key: String,
    },
    #[serde(rename = "api_key")]
    ApiKey {
        header: String,
        value_key: String,
    },
}

/// Configuration for registering an HTTP-invoked function (Lambda, Cloudflare
/// Workers, etc.) instead of a local handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpInvocationConfig {
    pub url: String,
    #[serde(default = "default_http_method")]
    pub method: HttpMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<HttpAuthConfig>,
}

fn default_http_method() -> HttpMethod {
    HttpMethod::Post
}

/// Routing action for [`TriggerRequest`]. Determines how the engine handles
/// the invocation.
///
/// - `Enqueue` -- Routes through a named queue for async processing.
/// - `Void` -- Fire-and-forget, no response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TriggerAction {
    /// Routes the invocation through a named queue.
    Enqueue { queue: String },
    /// Fire-and-forget routing.
    Void,
}

/// Result returned by the engine when a message is successfully enqueued.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EnqueueResult {
    #[serde(rename = "messageReceiptId")]
    pub message_receipt_id: String,
}

/// Request object for `trigger()`. Matches the Node/Python SDK signature:
/// `trigger({ function_id, payload, action?, timeout_ms? })`
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
/// ```
#[derive(Debug, Clone)]
pub struct TriggerRequest {
    pub function_id: String,
    pub payload: Value,
    pub action: Option<TriggerAction>,
    pub timeout_ms: Option<u64>,
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
    WorkerRegistered {
        worker_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterTriggerTypeMessage {
    pub id: String,
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

/// Input for [`III::register_trigger`](crate::III::register_trigger).
/// The `id` is auto-generated internally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterTriggerInput {
    pub trigger_type: String,
    pub function_id: String,
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterTriggerMessage {
    pub id: String,
    pub trigger_type: String,
    pub function_id: String,
    pub config: Value,
}

impl RegisterTriggerMessage {
    pub fn to_message(&self) -> Message {
        Message::RegisterTrigger {
            id: self.id.clone(),
            trigger_type: self.trigger_type.clone(),
            function_id: self.function_id.clone(),
            config: self.config.clone(),
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
pub struct RegisterServiceMessage {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_service_id: Option<String>,
}

impl RegisterServiceMessage {
    pub fn to_message(&self) -> Message {
        Message::RegisterService {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            parent_service_id: self.parent_service_id.clone(),
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
        use super::{HttpInvocationConfig, HttpMethod};

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
