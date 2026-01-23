use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
pub struct KvSetInput {
    pub index: String,
    pub key: String,
    pub value: Value,
}

#[derive(Serialize, Deserialize)]
pub struct KvDeleteInput {
    pub index: String,
    pub key: String,
}

#[derive(Serialize, Deserialize)]
pub struct KvGetInput {
    pub index: String,
    pub key: String,
}

#[derive(Serialize, Deserialize)]
pub struct KvListKeysInput {
    pub prefix: String,
}

#[derive(Serialize, Deserialize)]
pub struct KvListKeysWithPrefixInput {
    pub prefix: String,
}

#[derive(Serialize, Deserialize)]
pub struct KvListInput {
    pub index: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldPath(pub String);

impl From<&str> for FieldPath {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateResult {
    pub old_value: Option<Value>,
    pub new_value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UpdateOp {
    /// Set a value at path (overwrite)
    Set { path: FieldPath, value: Value },

    /// Merge object into existing value
    /// (object-only; backend should reject non-objects)
    Merge {
        path: Option<FieldPath>, // usually root: ""
        value: Value,
    },

    /// Increment numeric value
    Increment { path: FieldPath, by: i64 },

    /// Decrement numeric value
    Decrement { path: FieldPath, by: i64 },

    /// Remove a field
    Remove { path: FieldPath },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvUpdateInput {
    pub index: String,
    pub key: String,
    pub ops: Vec<UpdateOp>,
}
