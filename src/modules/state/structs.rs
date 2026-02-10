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
pub struct StateListGroupsInput {}

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_state_set_input_serde() {
        let input = StateSetInput {
            group_id: "users".to_string(),
            item_id: "123".to_string(),
            data: json!({"name": "test", "age": 30}),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["group_id"], "users");
        assert_eq!(json["item_id"], "123");
        assert_eq!(json["data"]["name"], "test");

        let deserialized: StateSetInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.group_id, "users");
        assert_eq!(deserialized.item_id, "123");
        assert_eq!(deserialized.data["name"], "test");
    }

    #[test]
    fn test_state_get_input_serde() {
        let input = StateGetInput {
            group_id: "users".to_string(),
            item_id: "123".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["group_id"], "users");
        assert_eq!(json["item_id"], "123");

        let deserialized: StateGetInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.group_id, "users");
        assert_eq!(deserialized.item_id, "123");
    }

    #[test]
    fn test_state_delete_input_serde() {
        let input = StateDeleteInput {
            group_id: "users".to_string(),
            item_id: "123".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["group_id"], "users");
        assert_eq!(json["item_id"], "123");

        let deserialized: StateDeleteInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.group_id, "users");
        assert_eq!(deserialized.item_id, "123");
    }

    #[test]
    fn test_state_update_input_serde() {
        let input = StateUpdateInput {
            group_id: "users".to_string(),
            item_id: "123".to_string(),
            ops: vec![],
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["group_id"], "users");
        assert_eq!(json["item_id"], "123");
        assert_eq!(json["ops"], json!([]));

        let deserialized: StateUpdateInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.group_id, "users");
        assert_eq!(deserialized.item_id, "123");
        assert_eq!(deserialized.ops.len(), 0);
    }

    #[test]
    fn test_state_get_group_input_serde() {
        let input = StateGetGroupInput {
            group_id: "users".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["group_id"], "users");

        let deserialized: StateGetGroupInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.group_id, "users");
    }

    #[test]
    fn test_state_list_groups_input_serde() {
        let input = StateListGroupsInput {};
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json, json!({}));

        let deserialized: StateListGroupsInput = serde_json::from_value(json).unwrap();
        assert_eq!(serde_json::to_value(&deserialized).unwrap(), json!({}));
    }

    #[test]
    fn test_state_event_type_created() {
        let event_type = StateEventType::Created;
        let json = serde_json::to_value(&event_type).unwrap();
        assert_eq!(json, "state:created");

        let deserialized: StateEventType = serde_json::from_value(json).unwrap();
        match deserialized {
            StateEventType::Created => {}
            _ => panic!("Expected Created"),
        }
    }

    #[test]
    fn test_state_event_type_updated() {
        let event_type = StateEventType::Updated;
        let json = serde_json::to_value(&event_type).unwrap();
        assert_eq!(json, "state:updated");

        let deserialized: StateEventType = serde_json::from_value(json).unwrap();
        match deserialized {
            StateEventType::Updated => {}
            _ => panic!("Expected Updated"),
        }
    }

    #[test]
    fn test_state_event_type_deleted() {
        let event_type = StateEventType::Deleted;
        let json = serde_json::to_value(&event_type).unwrap();
        assert_eq!(json, "state:deleted");

        let deserialized: StateEventType = serde_json::from_value(json).unwrap();
        match deserialized {
            StateEventType::Deleted => {}
            _ => panic!("Expected Deleted"),
        }
    }

    #[test]
    fn test_state_event_data_with_old_value() {
        let event = StateEventData {
            message_type: "state_event".to_string(),
            event_type: StateEventType::Updated,
            group_id: "users".to_string(),
            item_id: "123".to_string(),
            old_value: Some(json!({"name": "old"})),
            new_value: json!({"name": "new"}),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "state_event");
        assert_eq!(json["event_type"], "state:updated");
        assert_eq!(json["group_id"], "users");
        assert_eq!(json["item_id"], "123");
        assert_eq!(json["old_value"]["name"], "old");
        assert_eq!(json["new_value"]["name"], "new");

        let deserialized: StateEventData = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.message_type, "state_event");
        match deserialized.event_type {
            StateEventType::Updated => {}
            _ => panic!("Expected Updated"),
        }
        assert_eq!(deserialized.old_value.unwrap()["name"], "old");
        assert_eq!(deserialized.new_value["name"], "new");
    }

    #[test]
    fn test_state_event_data_without_old_value() {
        let event = StateEventData {
            message_type: "state_event".to_string(),
            event_type: StateEventType::Created,
            group_id: "users".to_string(),
            item_id: "123".to_string(),
            old_value: None,
            new_value: json!({"name": "new"}),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "state_event");
        assert_eq!(json["event_type"], "state:created");
        assert!(json["old_value"].is_null());
        assert_eq!(json["new_value"]["name"], "new");

        let deserialized: StateEventData = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.old_value, None);
        match deserialized.event_type {
            StateEventType::Created => {}
            _ => panic!("Expected Created"),
        }
    }
}
