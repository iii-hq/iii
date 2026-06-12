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
}
