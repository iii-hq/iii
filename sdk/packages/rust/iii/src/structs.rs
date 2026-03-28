use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::TriggerAction;

/// Input passed to the RBAC auth function during WebSocket upgrade.
///
/// Contains the HTTP headers, query parameters, and client IP from the
/// connecting worker's upgrade request.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthInput {
    /// HTTP headers from the WebSocket upgrade request.
    pub headers: HashMap<String, String>,
    /// Query parameters from the upgrade URL. Each key maps to a vec of values
    /// to support repeated keys (e.g. `?a=1&a=2`).
    pub query_params: HashMap<String, Vec<String>>,
    /// IP address of the connecting client.
    pub ip_address: String,
}

/// Return value from the RBAC auth function.
///
/// Controls which functions the authenticated worker can invoke and what
/// context is forwarded to the middleware.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthResult {
    /// Additional function IDs to allow beyond the `expose_functions` config.
    #[serde(default)]
    pub allowed_functions: Vec<String>,
    /// Function IDs to deny even if they match `expose_functions`.
    /// Takes precedence over allowed.
    #[serde(default)]
    pub forbidden_functions: Vec<String>,
    /// Trigger type IDs the worker may register triggers for.
    /// When `None`, all types are allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_trigger_types: Option<Vec<String>>,
    /// Whether the worker may register new trigger types.
    #[serde(default = "default_true")]
    pub allow_trigger_type_registration: bool,
    /// Whether the worker may register new functions. Defaults to `true`.
    #[serde(default = "default_true")]
    pub allow_function_registration: bool,
    /// Arbitrary context forwarded to the middleware function on every
    /// invocation.
    #[serde(default = "default_context")]
    pub context: Value,
}

fn default_true() -> bool {
    true
}

fn default_context() -> Value {
    Value::Object(Default::default())
}

/// Input passed to the RBAC middleware function on every function invocation
/// through the RBAC port.
///
/// The middleware can inspect, modify, or reject the call before it reaches
/// the target function.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MiddlewareFunctionInput {
    /// ID of the function being invoked.
    pub function_id: String,
    /// Payload sent by the caller.
    pub payload: Value,
    /// Routing action, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<TriggerAction>,
    /// Auth context returned by the auth function for this session.
    pub context: Value,
}

/// Input passed to the `on_trigger_type_registration_function_id` hook
/// when a worker attempts to register a new trigger type through the RBAC port.
/// Return `true` to allow the registration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OnTriggerTypeRegistrationInput {
    /// ID of the trigger type being registered.
    pub trigger_type_id: String,
    /// Human-readable description of the trigger type.
    pub description: String,
    /// Auth context from `AuthResult.context` for this session.
    pub context: Value,
}

/// Input passed to the `on_trigger_registration_function_id` hook
/// when a worker attempts to register a trigger through the RBAC port.
/// Return `true` to allow the registration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OnTriggerRegistrationInput {
    /// ID of the trigger being registered.
    pub trigger_id: String,
    /// Trigger type identifier.
    pub trigger_type: String,
    /// ID of the function this trigger is bound to.
    pub function_id: String,
    /// Trigger-specific configuration.
    pub config: Value,
    /// Auth context from `AuthResult.context` for this session.
    pub context: Value,
}

/// Input passed to the `on_function_registration_function_id` hook
/// when a worker attempts to register a function through the RBAC port.
/// Return `true` to allow the registration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OnFunctionRegistrationInput {
    /// ID of the function being registered.
    pub function_id: String,
    /// Human-readable description of the function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Arbitrary metadata attached to the function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    /// Auth context from `AuthResult.context` for this session.
    pub context: Value,
}
