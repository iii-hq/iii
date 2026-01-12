use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::BridgeError;
use crate::protocol::{RegisterFunctionMessage, RegisterTriggerTypeMessage};
use crate::triggers::TriggerHandler;

pub type RemoteFunctionHandler = Arc<dyn Fn(Value) -> BoxFuture<'static, Result<Value, BridgeError>> + Send + Sync>;

#[derive(Clone)]
pub struct RemoteFunctionData {
    pub message: RegisterFunctionMessage,
    pub handler: RemoteFunctionHandler,
}

#[derive(Clone)]
pub struct RemoteTriggerTypeData {
    pub message: RegisterTriggerTypeMessage,
    pub handler: Arc<dyn TriggerHandler>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiRequest<T = Value> {
    #[serde(default)]
    pub query_params: HashMap<String, String>,
    #[serde(default)]
    pub path_params: HashMap<String, String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub body: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T = Value> {
    pub status_code: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub body: T,
}
