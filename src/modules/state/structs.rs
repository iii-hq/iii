// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use iii_sdk::UpdateOp;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSetInput {
    pub group_id: String,
    pub item_id: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateGetInput {
    pub group_id: String,
    pub item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDeleteInput {
    pub group_id: String,
    pub item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateUpdateInput {
    pub group_id: String,
    pub item_id: String,
    pub ops: Vec<UpdateOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateGetGroupInput {
    pub group_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StateEventType {
    #[serde(rename = "state:created")]
    Created,
    #[serde(rename = "state:updated")]
    Updated,
    #[serde(rename = "state:deleted")]
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateEventData {
    #[serde(rename = "type")]
    pub message_type: String,
    pub event_type: StateEventType,
    pub group_id: String,
    pub item_id: String,
    pub old_value: Option<Value>,
    pub new_value: Value,
}
