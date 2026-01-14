use std::{collections::HashMap, sync::Arc};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    error::BridgeError,
    protocol::{RegisterFunctionMessage, RegisterTriggerTypeMessage},
    triggers::TriggerHandler,
};

pub type RemoteFunctionHandler =
    Arc<dyn Fn(Value) -> BoxFuture<'static, Result<Value, BridgeError>> + Send + Sync>;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_request_defaults_when_missing_fields() {
        let request: ApiRequest = serde_json::from_str("{}").unwrap();

        assert!(request.query_params.is_empty());
        assert!(request.path_params.is_empty());
        assert!(request.headers.is_empty());
        assert_eq!(request.path, "");
        assert_eq!(request.method, "");
        assert!(request.body.is_null());
    }
}
