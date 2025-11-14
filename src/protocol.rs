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
        #[serde(rename = "triggerType")]
        trigger_type: String,
        #[serde(rename = "functionPath")]
        function_path: String,
        config: Value,
    },
    TriggerRegistrationResult {
        id: String,
        #[serde(rename = "triggerType")]
        trigger_type: String,
        #[serde(rename = "functionPath")]
        function_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
    },
    UnregisterTrigger {
        id: String,
        #[serde(rename = "triggerType")]
        trigger_type: String,
    },
    RegisterFunction {
        #[serde(rename = "functionPath")]
        function_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    InvokeFunction {
        #[serde(rename = "invocationId")]
        invocation_id: Option<Uuid>,
        #[serde(rename = "functionPath")]
        function_path: String,
        data: Value,
    },
    InvocationResult {
        #[serde(rename = "invocationId")]
        invocation_id: Uuid,
        #[serde(rename = "functionPath")]
        function_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
    },
    RegisterService {
        id: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    FunctionsAvailable {
        functions: Vec<String>,
    },
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}
