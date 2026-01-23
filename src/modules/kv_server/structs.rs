use iii_sdk::UpdateOp;
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateResult {
    pub old_value: Option<Value>,
    pub new_value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvUpdateInput {
    pub index: String,
    pub key: String,
    pub ops: Vec<UpdateOp>,
}
