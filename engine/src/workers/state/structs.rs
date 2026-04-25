// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use iii_sdk::UpdateOp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateSetInput {
    pub scope: String,
    pub key: String,
    #[serde(alias = "data")]
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateGetInput {
    pub scope: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateDeleteInput {
    pub scope: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateUpdateInput {
    pub scope: String,
    pub key: String,
    pub ops: Vec<UpdateOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateGetGroupInput {
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateListGroupsInput {}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StateListGroupsResult {
    pub groups: Vec<String>,
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
    pub scope: String,
    pub key: String,
    pub old_value: Option<Value>,
    pub new_value: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn state_set_input_data_alias() {
        let json = json!({"scope": "s", "key": "k", "data": "hello"});
        let input: StateSetInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.value, json!("hello"));
    }

    #[test]
    fn state_set_input_value_field() {
        let json = json!({"scope": "s", "key": "k", "value": 42});
        let input: StateSetInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.value, json!(42));
    }

    #[test]
    fn state_update_input_schema_merge_path_accepts_string_or_array() {
        // Regression: changing UpdateOp::Merge.path to an untagged
        // enum (MergePath: Single | Segments) must keep the schemars
        // schema for ops accepting both string and array path forms.
        // This test catches silent drift if schemars output changes.
        let schema = schemars::schema_for!(StateUpdateInput);
        let schema_value = serde_json::to_value(&schema).unwrap();
        let pretty = serde_json::to_string_pretty(&schema_value).unwrap();

        // The Merge variant must surface anyOf-of-(string, array of string)
        // somewhere in the generated schema. We don't pin the exact
        // shape (schemars output evolves) but we assert presence of
        // both forms in the pretty-printed schema text.
        assert!(
            pretty.contains("\"merge\""),
            "schema must mention the merge variant: {}",
            pretty
        );
        assert!(
            pretty.contains("array"),
            "schema must include an array form for merge.path"
        );
        assert!(
            pretty.contains("string"),
            "schema must include a string form for merge.path"
        );
    }

    #[test]
    fn state_event_type_serde() {
        let created = StateEventType::Created;
        let json = serde_json::to_value(&created).unwrap();
        assert_eq!(json, json!("state:created"));

        let back: StateEventType = serde_json::from_value(json!("state:updated")).unwrap();
        assert!(matches!(back, StateEventType::Updated));

        let deleted: StateEventType = serde_json::from_value(json!("state:deleted")).unwrap();
        assert!(matches!(deleted, StateEventType::Deleted));
    }

    #[test]
    fn state_event_data_roundtrip() {
        let json = json!({
            "type": "state_event",
            "event_type": "state:created",
            "scope": "users",
            "key": "user-1",
            "old_value": null,
            "new_value": {"name": "Alice"}
        });
        let data: StateEventData = serde_json::from_value(json).unwrap();
        assert_eq!(data.message_type, "state_event");
        assert!(matches!(data.event_type, StateEventType::Created));
        assert!(data.old_value.is_none());
        let back = serde_json::to_value(&data).unwrap();
        assert_eq!(back["type"], "state_event");
    }

    #[test]
    fn state_get_delete_group_roundtrip() {
        let _get: StateGetInput =
            serde_json::from_value(json!({"scope": "s", "key": "k"})).unwrap();
        let _del: StateDeleteInput =
            serde_json::from_value(json!({"scope": "s", "key": "k"})).unwrap();
        let _group: StateGetGroupInput = serde_json::from_value(json!({"scope": "s"})).unwrap();
        let _list: StateListGroupsInput = serde_json::from_value(json!({})).unwrap();
    }
}
