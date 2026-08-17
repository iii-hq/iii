use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::TriggerAction;

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
    /// Target namespace the invoke addressed; forward the call here to stay in
    /// the caller's namespace. Absent → the engine's default namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserializes_namespace_from_engine_middleware_input() {
        // Mirrors the wire object the engine builds (engine/src/engine/mod.rs).
        let input: MiddlewareFunctionInput = serde_json::from_value(json!({
            "function_id": "orders::create",
            "payload": {},
            "context": {},
            "namespace": "orders",
        }))
        .unwrap();
        assert_eq!(input.namespace, Some("orders".to_string()));
    }

    #[test]
    fn namespace_is_optional() {
        let input: MiddlewareFunctionInput = serde_json::from_value(json!({
            "function_id": "orders::create",
            "payload": {},
            "context": {},
        }))
        .unwrap();
        assert_eq!(input.namespace, None);
    }
}
