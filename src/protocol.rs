use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub type ClientAddr = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    Register {
        name: String,
        description: String,
        methods: Vec<MethodDef>,
    },
    Call {
        id: Uuid,
        to: Option<ClientAddr>,
        method: String,
        params: Value,
    },
    Result {
        id: Uuid,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
    },
    Error {
        id: Uuid,
        code: String,
        message: String,
    },
    Notify {
        to: Option<ClientAddr>,
        method: String,
        params: Value,
    },
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodDef {
    pub name: String,
    pub params_schema: serde_json::Value,
    pub result_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}
