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

// ============================================================================
// Stream Update Types
// ============================================================================

/// Represents a path to a field in a JSON object
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldPath(pub String);

impl FieldPath {
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    pub fn root() -> Self {
        Self(String::new())
    }
}

impl From<&str> for FieldPath {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for FieldPath {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Operations that can be performed atomically on a stream value
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum UpdateOp {
    /// Set a value at path (overwrite)
    Set {
        path: FieldPath,
        value: Option<Value>,
    },

    /// Merge object into existing value (object-only)
    Merge {
        path: Option<FieldPath>,
        value: Value,
    },

    /// Increment numeric value
    Increment { path: FieldPath, by: i64 },

    /// Decrement numeric value
    Decrement { path: FieldPath, by: i64 },

    /// Remove a field
    Remove { path: FieldPath },
}

impl UpdateOp {
    /// Create a Set operation
    pub fn set(path: impl Into<FieldPath>, value: impl Into<Option<Value>>) -> Self {
        Self::Set {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create an Increment operation
    pub fn increment(path: impl Into<FieldPath>, by: i64) -> Self {
        Self::Increment {
            path: path.into(),
            by,
        }
    }

    /// Create a Decrement operation
    pub fn decrement(path: impl Into<FieldPath>, by: i64) -> Self {
        Self::Decrement {
            path: path.into(),
            by,
        }
    }

    /// Create a Remove operation
    pub fn remove(path: impl Into<FieldPath>) -> Self {
        Self::Remove { path: path.into() }
    }

    /// Create a Merge operation at root level
    pub fn merge(value: impl Into<Value>) -> Self {
        Self::Merge {
            path: None,
            value: value.into(),
        }
    }

    /// Create a Merge operation at a specific path
    pub fn merge_at(path: impl Into<FieldPath>, value: impl Into<Value>) -> Self {
        Self::Merge {
            path: Some(path.into()),
            value: value.into(),
        }
    }
}

/// Result of an atomic update operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    /// The value before the update (None if key didn't exist)
    pub old_value: Option<Value>,
    /// The value after the update
    pub new_value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetResult {
    /// The value before the update (None if key didn't exist)
    pub old_value: Option<Value>,
    /// The value after the update
    pub new_value: Value,
}

/// Input for the stream update function
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamUpdateInput {
    pub key: String,
    pub ops: Vec<UpdateOp>,
}

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
