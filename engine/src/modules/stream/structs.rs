// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

use axum::extract::ws::Message as WsMessage;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use iii_sdk::UpdateOp;

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
    Event { event: EventData },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamWrapperMessage {
    #[serde(rename = "type")]
    pub event_type: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamSetInput {
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamGetInput {
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamDeleteInput {
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamListInput {
    pub stream_name: String,
    pub group_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamUpdateInput {
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
    pub ops: Vec<UpdateOp>,
}

/// Input for stream.listAll (empty struct)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamListAllInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamSendInput {
    pub stream_name: String,
    pub group_id: String,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: Value,
}

/// Metadata for a stream (used by stream.listAll)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamMetadata {
    pub id: String,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StreamListAllResult {
    pub stream: Vec<StreamMetadata>,
    pub count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stream_incoming_message_join_roundtrip() {
        let json = json!({
            "type": "join",
            "data": {
                "subscriptionId": "sub-1",
                "streamName": "stream-1",
                "groupId": "group-1",
                "id": "item-1"
            }
        });
        let msg: StreamIncomingMessage = serde_json::from_value(json).unwrap();
        match &msg {
            StreamIncomingMessage::Join { data } => {
                assert_eq!(data.subscription_id, "sub-1");
                assert_eq!(data.stream_name, "stream-1");
                assert_eq!(data.group_id, "group-1");
                assert_eq!(data.id.as_deref(), Some("item-1"));
            }
            _ => panic!("Expected Join"),
        }
        let back = serde_json::to_value(&msg).unwrap();
        assert_eq!(back["type"], "join");
    }

    #[test]
    fn stream_incoming_message_leave_roundtrip() {
        let json = json!({
            "type": "leave",
            "data": {
                "subscriptionId": "sub-2",
                "streamName": "s",
                "groupId": "g"
            }
        });
        let msg: StreamIncomingMessage = serde_json::from_value(json).unwrap();
        assert!(matches!(msg, StreamIncomingMessage::Leave { .. }));
    }

    #[test]
    fn stream_outbound_message_variants() {
        let cases = vec![
            json!({"type": "unauthorized"}),
            json!({"type": "sync", "data": 1}),
            json!({"type": "create", "data": "x"}),
            json!({"type": "update", "data": null}),
            json!({"type": "delete", "data": []}),
            json!({"type": "event", "event": {"type": "click", "data": {}}}),
        ];
        for case in cases {
            let msg: StreamOutboundMessage = serde_json::from_value(case.clone()).unwrap();
            let back = serde_json::to_value(&msg).unwrap();
            assert_eq!(back["type"], case["type"]);
        }
    }

    #[test]
    fn stream_wrapper_message_roundtrip() {
        let json = json!({
            "type": "wrapper",
            "timestamp": 12345,
            "streamName": "s",
            "groupId": "g",
            "id": null,
            "event": {"type": "sync", "data": {}}
        });
        let msg: StreamWrapperMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg.event_type, "wrapper");
        assert_eq!(msg.timestamp, 12345);
        assert_eq!(msg.stream_name, "s");
    }

    #[test]
    fn stream_set_input_roundtrip() {
        let json = json!({
            "stream_name": "s",
            "group_id": "g",
            "item_id": "i",
            "data": {"key": "value"}
        });
        let input: StreamSetInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.stream_name, "s");
        let back = serde_json::to_value(&input).unwrap();
        assert_eq!(back["item_id"], "i");
    }

    #[test]
    fn stream_get_delete_input_roundtrip() {
        let json = json!({"stream_name": "s", "group_id": "g", "item_id": "i"});
        let _get: StreamGetInput = serde_json::from_value(json.clone()).unwrap();
        let _del: StreamDeleteInput = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn stream_list_and_list_groups_roundtrip() {
        let list: StreamListInput =
            serde_json::from_value(json!({"stream_name": "s", "group_id": "g"})).unwrap();
        assert_eq!(list.stream_name, "s");

        let lg: StreamListGroupsInput =
            serde_json::from_value(json!({"stream_name": "s"})).unwrap();
        assert_eq!(lg.stream_name, "s");
    }

    #[test]
    fn stream_auth_input_roundtrip() {
        let json = json!({
            "headers": {"auth": "bearer x"},
            "path": "/ws",
            "query_params": {"key": ["val"]},
            "addr": "127.0.0.1"
        });
        let input: StreamAuthInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.path, "/ws");
        assert_eq!(input.addr, "127.0.0.1");
    }

    #[test]
    fn stream_send_input_roundtrip() {
        let json = json!({
            "stream_name": "s",
            "group_id": "g",
            "id": "i",
            "type": "custom",
            "data": {"x": 1}
        });
        let input: StreamSendInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.event_type, "custom");
        let back = serde_json::to_value(&input).unwrap();
        assert_eq!(back["type"], "custom");
    }

    #[test]
    fn stream_metadata_roundtrip() {
        let json = json!({"id": "stream-1", "groups": ["a", "b"]});
        let meta: StreamMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(meta.id, "stream-1");
        assert_eq!(meta.groups, vec!["a", "b"]);
    }
}
