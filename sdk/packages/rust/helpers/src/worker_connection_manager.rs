//! iii worker connection manager helpers.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// Grants scoped to one namespace each: `{ "orders": ["svc::*"] }`. A value
    /// is an exact function ID or a wildcard, in either the bare (`svc::*`) or
    /// the `match("svc::*")` spelling.
    ///
    /// The keys are also the namespaces the session may declare on
    /// `engine::workers::register`. Leave it empty to add no namespace-scoped
    /// grants: the session may declare any namespace, and only
    /// `allowed_functions` and `expose_functions` apply.
    #[serde(default)]
    pub namespaces: HashMap<String, Vec<String>>,
    /// Additional function IDs to allow beyond the `expose_functions` config,
    /// in the `default` namespace only. Put per-namespace grants in
    /// [`AuthResult::namespaces`].
    #[serde(default)]
    pub allowed_functions: Vec<String>,
    /// Function IDs to deny even if they match `expose_functions`.
    /// Takes precedence over allowed.
    #[serde(default)]
    pub forbidden_functions: Vec<String>,
    /// Whether the worker may register new functions. Defaults to `true`.
    #[serde(default = "default_true")]
    pub allow_function_registration: bool,
    /// Arbitrary context forwarded to the middleware function on every
    /// invocation.
    #[serde(default = "default_context")]
    pub context: Value,
    /// Optional prefix applied to all function IDs registered by this worker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_registration_prefix: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_context() -> Value {
    Value::Object(Default::default())
}

/// Wire-compat default for [`OnFunctionRegistrationInput::namespace`] when an
/// older engine omits the field.
fn default_namespace_field() -> String {
    "default".to_string()
}

/// Input passed to the `on_function_registration_function_id` hook
/// when a worker attempts to register a function through the RBAC port.
/// Return an [`OnFunctionRegistrationResult`] with the (possibly mapped)
/// fields, or return an error to deny the registration.
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
    /// Namespace the function registers in. The same id can exist in several
    /// namespaces, so the hook needs this to authorize per namespace.
    #[serde(default = "default_namespace_field")]
    pub namespace: String,
    /// Only the registering worker may call the function.
    #[serde(default)]
    pub private: bool,
    /// Whether the function may be observed. An older engine omits it:
    /// `true`.
    #[serde(default = "default_true")]
    pub observable: bool,
    /// JSON Schema of the function's observer args, when it takes any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_format: Option<Value>,
    /// Auth context from `AuthResult.context` for this session.
    pub context: Value,
}

/// Result returned from the `on_function_registration_function_id` hook.
/// Omitted fields keep the original value from the registration request.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct OnFunctionRegistrationResult {
    /// Mapped function ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_id: Option<String>,
    /// Mapped description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Mapped metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    /// Replaces the registration's `private`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,
    /// Replaces the registration's `observable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observable: Option<bool>,
    /// Replaces the registration's `args_format`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_format: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Defaults must match the engine's canonical AuthResult
    // (engine/src/workers/worker/rbac_session.rs).
    #[test]
    fn auth_result_defaults_match_engine() {
        let result: AuthResult = serde_json::from_str("{}").unwrap();
        assert!(result.namespaces.is_empty());
        assert_eq!(result.allowed_functions, Vec::<String>::new());
        assert_eq!(result.forbidden_functions, Vec::<String>::new());
        assert!(result.allow_function_registration);
        assert_eq!(result.context, Value::Object(Default::default()));
        assert_eq!(result.function_registration_prefix, None);
    }

    #[test]
    fn function_registration_input_carries_the_v2_fields() {
        let input: OnFunctionRegistrationInput = serde_json::from_value(serde_json::json!({
            "function_id": "cron::on-tick",
            "namespace": "default",
            "private": true,
            "observable": true,
            "args_format": { "type": "object" },
            "context": {},
        }))
        .unwrap();
        assert!(input.private && input.observable);
        assert_eq!(
            input.args_format,
            Some(serde_json::json!({ "type": "object" }))
        );

        // an older engine sends none of them
        let input: OnFunctionRegistrationInput = serde_json::from_value(serde_json::json!({
            "function_id": "orders::create",
            "context": {},
        }))
        .unwrap();
        assert!(!input.private && input.observable && input.args_format.is_none());

        let result = OnFunctionRegistrationResult {
            observable: Some(false),
            ..OnFunctionRegistrationResult::default()
        };
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            serde_json::json!({ "observable": false })
        );
    }
}
