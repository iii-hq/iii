use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
pub struct KvSetInput {
    pub key: String,
    pub value: Value,
}

#[derive(Serialize, Deserialize)]
pub struct KvDeleteInput {
    pub key: String,
}

#[derive(Serialize, Deserialize)]
pub struct KvGetInput {
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
    pub key: String,
}
