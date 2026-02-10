// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;

use axum::extract::ws::Message as WsMessage;
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
    Event { data: EventData },
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
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
    pub ops: Vec<UpdateOp>,
}

/// Input for streams.listAll (empty struct)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamListAllInput {}

/// Metadata for a stream (used by streams.listAll)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamMetadata {
    pub id: String,
    pub groups: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_stream_incoming_message_data_serde() {
        let data = StreamIncomingMessageData {
            subscription_id: "sub-1".to_string(),
            stream_name: "test-stream".to_string(),
            group_id: "group-1".to_string(),
            id: Some("item-1".to_string()),
        };
        let json = serde_json::to_value(&data).unwrap();
        assert_eq!(json["subscriptionId"], "sub-1");
        assert_eq!(json["streamName"], "test-stream");
        assert_eq!(json["groupId"], "group-1");
        assert_eq!(json["id"], "item-1");

        let deserialized: StreamIncomingMessageData = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.subscription_id, "sub-1");
        assert_eq!(deserialized.stream_name, "test-stream");
        assert_eq!(deserialized.group_id, "group-1");
        assert_eq!(deserialized.id, Some("item-1".to_string()));
    }

    #[test]
    fn test_stream_incoming_message_data_without_id() {
        let data = StreamIncomingMessageData {
            subscription_id: "sub-1".to_string(),
            stream_name: "test-stream".to_string(),
            group_id: "group-1".to_string(),
            id: None,
        };
        let json = serde_json::to_value(&data).unwrap();
        assert!(json["id"].is_null());

        let deserialized: StreamIncomingMessageData = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.id, None);
    }

    #[test]
    fn test_stream_incoming_message_join() {
        let message = StreamIncomingMessage::Join {
            data: StreamIncomingMessageData {
                subscription_id: "sub-1".to_string(),
                stream_name: "test-stream".to_string(),
                group_id: "group-1".to_string(),
                id: None,
            },
        };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "join");
        assert_eq!(json["data"]["subscriptionId"], "sub-1");

        let deserialized: StreamIncomingMessage = serde_json::from_value(json).unwrap();
        match deserialized {
            StreamIncomingMessage::Join { data } => {
                assert_eq!(data.subscription_id, "sub-1");
            }
            _ => panic!("Expected Join"),
        }
    }

    #[test]
    fn test_stream_incoming_message_leave() {
        let message = StreamIncomingMessage::Leave {
            data: StreamIncomingMessageData {
                subscription_id: "sub-1".to_string(),
                stream_name: "test-stream".to_string(),
                group_id: "group-1".to_string(),
                id: None,
            },
        };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "leave");

        let deserialized: StreamIncomingMessage = serde_json::from_value(json).unwrap();
        match deserialized {
            StreamIncomingMessage::Leave { data } => {
                assert_eq!(data.subscription_id, "sub-1");
            }
            _ => panic!("Expected Leave"),
        }
    }

    #[test]
    fn test_event_data_serde() {
        let event = EventData {
            event_type: "test_event".to_string(),
            data: json!({"key": "value"}),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "test_event");
        assert_eq!(json["data"]["key"], "value");

        let deserialized: EventData = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.event_type, "test_event");
        assert_eq!(deserialized.data["key"], "value");
    }

    #[test]
    fn test_stream_outbound_message_unauthorized() {
        let message = StreamOutboundMessage::Unauthorized {};
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "unauthorized");

        let deserialized: StreamOutboundMessage = serde_json::from_value(json).unwrap();
        match deserialized {
            StreamOutboundMessage::Unauthorized {} => {}
            _ => panic!("Expected Unauthorized"),
        }
    }

    #[test]
    fn test_stream_outbound_message_sync() {
        let message = StreamOutboundMessage::Sync {
            data: json!({"items": [1, 2, 3]}),
        };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "sync");
        assert_eq!(json["data"]["items"], json!([1, 2, 3]));

        let deserialized: StreamOutboundMessage = serde_json::from_value(json).unwrap();
        match deserialized {
            StreamOutboundMessage::Sync { data } => {
                assert_eq!(data["items"], json!([1, 2, 3]));
            }
            _ => panic!("Expected Sync"),
        }
    }

    #[test]
    fn test_stream_outbound_message_create() {
        let message = StreamOutboundMessage::Create {
            data: json!({"id": "123", "name": "test"}),
        };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "create");

        let deserialized: StreamOutboundMessage = serde_json::from_value(json).unwrap();
        match deserialized {
            StreamOutboundMessage::Create { data } => {
                assert_eq!(data["id"], "123");
            }
            _ => panic!("Expected Create"),
        }
    }

    #[test]
    fn test_stream_outbound_message_update() {
        let message = StreamOutboundMessage::Update {
            data: json!({"id": "123"}),
        };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "update");

        let deserialized: StreamOutboundMessage = serde_json::from_value(json).unwrap();
        match deserialized {
            StreamOutboundMessage::Update { data } => {
                assert_eq!(data["id"], "123");
            }
            _ => panic!("Expected Update"),
        }
    }

    #[test]
    fn test_stream_outbound_message_delete() {
        let message = StreamOutboundMessage::Delete {
            data: json!({"id": "123"}),
        };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "delete");

        let deserialized: StreamOutboundMessage = serde_json::from_value(json).unwrap();
        match deserialized {
            StreamOutboundMessage::Delete { data } => {
                assert_eq!(data["id"], "123");
            }
            _ => panic!("Expected Delete"),
        }
    }

    #[test]
    fn test_stream_outbound_message_event() {
        let message = StreamOutboundMessage::Event {
            data: EventData {
                event_type: "custom_event".to_string(),
                data: json!({"payload": "data"}),
            },
        };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "event");
        assert_eq!(json["data"]["type"], "custom_event");

        let deserialized: StreamOutboundMessage = serde_json::from_value(json).unwrap();
        match deserialized {
            StreamOutboundMessage::Event { data } => {
                assert_eq!(data.event_type, "custom_event");
            }
            _ => panic!("Expected Event"),
        }
    }

    #[test]
    fn test_stream_wrapper_message_serde() {
        let wrapper = StreamWrapperMessage {
            event_type: "stream_event".to_string(),
            timestamp: 1234567890,
            stream_name: "test-stream".to_string(),
            group_id: "group-1".to_string(),
            id: Some("item-1".to_string()),
            event: StreamOutboundMessage::Sync {
                data: json!({"data": "test"}),
            },
        };
        let json = serde_json::to_value(&wrapper).unwrap();
        assert_eq!(json["type"], "stream_event");
        assert_eq!(json["timestamp"], 1234567890);
        assert_eq!(json["streamName"], "test-stream");
        assert_eq!(json["groupId"], "group-1");
        assert_eq!(json["id"], "item-1");
        assert_eq!(json["event"]["type"], "sync");

        let deserialized: StreamWrapperMessage = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.event_type, "stream_event");
        assert_eq!(deserialized.timestamp, 1234567890);
        assert_eq!(deserialized.stream_name, "test-stream");
        assert_eq!(deserialized.group_id, "group-1");
        assert_eq!(deserialized.id, Some("item-1".to_string()));
    }

    #[test]
    fn test_stream_set_input_serde() {
        let input = StreamSetInput {
            stream_name: "test-stream".to_string(),
            group_id: "group-1".to_string(),
            item_id: "item-1".to_string(),
            data: json!({"name": "test"}),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["stream_name"], "test-stream");
        assert_eq!(json["group_id"], "group-1");
        assert_eq!(json["item_id"], "item-1");

        let deserialized: StreamSetInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.stream_name, "test-stream");
        assert_eq!(deserialized.group_id, "group-1");
        assert_eq!(deserialized.item_id, "item-1");
    }

    #[test]
    fn test_stream_get_input_serde() {
        let input = StreamGetInput {
            stream_name: "test-stream".to_string(),
            group_id: "group-1".to_string(),
            item_id: "item-1".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["stream_name"], "test-stream");

        let deserialized: StreamGetInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.stream_name, "test-stream");
    }

    #[test]
    fn test_stream_delete_input_serde() {
        let input = StreamDeleteInput {
            stream_name: "test-stream".to_string(),
            group_id: "group-1".to_string(),
            item_id: "item-1".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["stream_name"], "test-stream");

        let deserialized: StreamDeleteInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.stream_name, "test-stream");
    }

    #[test]
    fn test_stream_get_group_input_serde() {
        let input = StreamGetGroupInput {
            stream_name: "test-stream".to_string(),
            group_id: "group-1".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["stream_name"], "test-stream");
        assert_eq!(json["group_id"], "group-1");

        let deserialized: StreamGetGroupInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.stream_name, "test-stream");
        assert_eq!(deserialized.group_id, "group-1");
    }

    #[test]
    fn test_stream_list_groups_input_serde() {
        let input = StreamListGroupsInput {
            stream_name: "test-stream".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["stream_name"], "test-stream");

        let deserialized: StreamListGroupsInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.stream_name, "test-stream");
    }

    #[test]
    fn test_stream_auth_input_serde() {
        let mut headers = HashMap::new();
        headers.insert("authorization".to_string(), "Bearer token".to_string());
        let mut query_params = HashMap::new();
        query_params.insert("key".to_string(), vec!["value".to_string()]);

        let input = StreamAuthInput {
            headers,
            path: "/stream/test".to_string(),
            query_params,
            addr: "127.0.0.1:8080".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["path"], "/stream/test");
        assert_eq!(json["addr"], "127.0.0.1:8080");

        let deserialized: StreamAuthInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.path, "/stream/test");
        assert_eq!(deserialized.addr, "127.0.0.1:8080");
    }

    #[test]
    fn test_stream_auth_context_with_context() {
        let context = StreamAuthContext {
            context: Some(json!({"user_id": "123"})),
        };
        let json = serde_json::to_value(&context).unwrap();
        assert_eq!(json["context"]["user_id"], "123");

        let deserialized: StreamAuthContext = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.context.unwrap()["user_id"], "123");
    }

    #[test]
    fn test_stream_auth_context_without_context() {
        let context = StreamAuthContext { context: None };
        let json = serde_json::to_value(&context).unwrap();
        assert!(json["context"].is_null());

        let deserialized: StreamAuthContext = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.context, None);
    }

    #[test]
    fn test_stream_join_leave_event_serde() {
        let event = StreamJoinLeaveEvent {
            subscription_id: "sub-1".to_string(),
            stream_name: "test-stream".to_string(),
            group_id: "group-1".to_string(),
            id: Some("item-1".to_string()),
            context: Some(json!({"key": "value"})),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["subscription_id"], "sub-1");
        assert_eq!(json["stream_name"], "test-stream");

        let deserialized: StreamJoinLeaveEvent = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.subscription_id, "sub-1");
        assert_eq!(deserialized.stream_name, "test-stream");
    }

    #[test]
    fn test_stream_join_result_serde() {
        let result = StreamJoinResult { unauthorized: true };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["unauthorized"], true);

        let deserialized: StreamJoinResult = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.unauthorized, true);
    }

    #[test]
    fn test_stream_update_input_serde() {
        let input = StreamUpdateInput {
            stream_name: "test-stream".to_string(),
            group_id: "group-1".to_string(),
            item_id: "item-1".to_string(),
            ops: vec![],
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["stream_name"], "test-stream");
        assert_eq!(json["ops"], json!([]));

        let deserialized: StreamUpdateInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.stream_name, "test-stream");
        assert_eq!(deserialized.ops.len(), 0);
    }

    #[test]
    fn test_stream_list_all_input_serde() {
        let input = StreamListAllInput {};
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json, json!({}));

        let deserialized: StreamListAllInput = serde_json::from_value(json).unwrap();
        assert_eq!(serde_json::to_value(&deserialized).unwrap(), json!({}));
    }

    #[test]
    fn test_stream_metadata_serde() {
        let metadata = StreamMetadata {
            id: "stream-1".to_string(),
            groups: vec!["group-1".to_string(), "group-2".to_string()],
        };
        let json = serde_json::to_value(&metadata).unwrap();
        assert_eq!(json["id"], "stream-1");
        assert_eq!(json["groups"], json!(["group-1", "group-2"]));

        let deserialized: StreamMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.id, "stream-1");
        assert_eq!(deserialized.groups.len(), 2);
    }
}
