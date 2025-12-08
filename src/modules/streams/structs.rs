use axum::extract::ws::Message as WsMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// pub struct StreamConfig {
//     pub stream_name: String,
//     pub schema: Value,
//     pub can_access_function_path: String,
// }

pub struct Subscription {
    pub stream_name: String,
    pub group_id: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinData {
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
    Join { data: JoinData },
    Leave { data: JoinData },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventData {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StreamOutboundMessage {
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
    #[serde(rename = "itemId")]
    pub item_id: Option<String>,
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
