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
pub enum StreamIncomingMessage {
    Subscribe {
        #[serde(rename = "subscriptionId")]
        subscription_id: String,
        #[serde(rename = "streamName")]
        stream_name: String,
        #[serde(rename = "groupId")]
        group_id: String,
        id: Option<String>,
    },
    Unsubscribe {
        #[serde(rename = "subscriptionId")]
        subscription_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamOutboundMessage {
    Sync {
        data: Value,
    },
    Create {
        data: Value,
    },
    Update {
        data: Value,
    },
    Delete {
        data: Value,
    },
    Event {
        #[serde(rename = "type")]
        event_type: String,
        data: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamWrapperMessage {
    pub timestamp: i64,
    pub stream_name: String,
    pub group_id: String,
    pub item_id: Option<String>,
    pub message: StreamOutboundMessage,
}

#[derive(Debug)]
pub enum StreamOutbound {
    Stream(StreamWrapperMessage),
    Raw(WsMessage),
}
