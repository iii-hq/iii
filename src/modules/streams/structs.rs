use std::collections::HashMap;

use axum::extract::ws::Message as WsMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::modules::kv_server::structs::UpdateOp;

pub struct Subscription {
    pub subscription_id: String,
    pub stream_name: String,
    pub group_id: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamIncomingMessageData {
    #[serde(rename = "subscriptionId")]
    pub subscription_id: String,
    #[serde(rename = "streamName")]
    pub stream_name: String,
    #[serde(rename = "groupId")]
    pub group_id: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StreamIncomingMessage {
    Join { data: StreamIncomingMessageData },
    Leave { data: StreamIncomingMessageData },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventData {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StreamOutboundMessage {
    Unauthorized {},
    Sync { data: Value },
    Create { data: Value },
    Update { data: Value },
    Delete { data: Value },
    Event { data: EventData },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamWrapperMessage {
    pub timestamp: i64,
    #[serde(rename = "streamName")]
    pub stream_name: String,
    #[serde(rename = "groupId")]
    pub group_id: String,
    pub id: Option<String>,
    pub event: StreamOutboundMessage,
}

#[derive(Debug)]
pub enum StreamOutbound {
    Stream(StreamWrapperMessage),
    Raw(WsMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSetInput {
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamGetInput {
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDeleteInput {
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamGetGroupInput {
    pub stream_name: String,
    pub group_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamListGroupsInput {
    pub stream_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamAuthInput {
    pub headers: HashMap<String, String>,
    pub path: String,
    pub query_params: HashMap<String, Vec<String>>,
    pub addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamAuthContext {
    pub context: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamJoinLeaveEvent {
    pub subscription_id: String,
    pub stream_name: String,
    pub group_id: String,
    pub id: Option<String>,
    pub context: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamJoinResult {
    pub unauthorized: bool,
}

/// Input for atomic stream update operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamUpdateInput {
    /// The key to update (format: "stream_name::group_id::item_id" or custom key)
    pub key: String,
    /// List of operations to apply atomically
    pub ops: Vec<UpdateOp>,
}
