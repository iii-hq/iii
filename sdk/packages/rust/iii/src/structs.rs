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
    #[serde(default)]
    pub allow_trigger_type_registration: bool,
    /// Arbitrary context forwarded to the middleware function on every
    /// invocation.
    #[serde(default = "default_context")]
    pub context: Value,
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
