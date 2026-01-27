// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

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
