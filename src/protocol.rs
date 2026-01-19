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
        trigger_type: String,
        function_path: String,
        config: Value,
    },
    TriggerRegistrationResult {
        id: String,
        trigger_type: String,
        function_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
    },
    UnregisterTrigger {
        id: String,
        trigger_type: String,
    },
    RegisterFunction {
        function_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        request_format: Option<Value>,
        response_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    InvokeFunction {
        invocation_id: Option<Uuid>,
        function_path: String,
        data: Value,
    },
    InvocationResult {
        invocation_id: Uuid,
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
        functions: Vec<FunctionMessage>,
    },
    ListFunctions,
    RegisterWorker {
        runtime: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        os: Option<String>,
    },
    ListWorkers,
    WorkersAvailable {
        workers: Vec<WorkerInfo>,
    },
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionMessage {
    pub function_path: String,
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
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    pub status: String,
    pub connected_at_ms: u64,
    pub function_count: usize,
    pub functions: Vec<String>,
    pub active_invocations: usize,
}
