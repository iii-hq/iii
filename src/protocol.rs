use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    RegisterFunction {
        #[serde(rename = "functionPath")]
        function_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    InvokeFunction {
        #[serde(rename = "invocationId")]
        invocation_id: Uuid,
        #[serde(rename = "functionPath")]
        function_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<Value>,
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
    // RegisterService {
    //     id: String,
    //     #[serde(skip_serializing_if = "Option::is_none")]
    //     description: Option<String>,
    //     #[serde(skip_serializing_if = "Option::is_none", rename = "parentServiceId")]
    //     parent_service_id: Option<String>,
    // },
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}
