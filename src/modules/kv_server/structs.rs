// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_kv_set_input_serde() {
        let input = KvSetInput {
            index: "users".to_string(),
            key: "user-123".to_string(),
            value: json!({"name": "test", "age": 30}),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["index"], "users");
        assert_eq!(json["key"], "user-123");
        assert_eq!(json["value"]["name"], "test");

        let deserialized: KvSetInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.index, "users");
        assert_eq!(deserialized.key, "user-123");
        assert_eq!(deserialized.value["name"], "test");
    }

    #[test]
    fn test_kv_delete_input_serde() {
        let input = KvDeleteInput {
            index: "users".to_string(),
            key: "user-123".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["index"], "users");
        assert_eq!(json["key"], "user-123");

        let deserialized: KvDeleteInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.index, "users");
        assert_eq!(deserialized.key, "user-123");
    }

    #[test]
    fn test_kv_get_input_serde() {
        let input = KvGetInput {
            index: "users".to_string(),
            key: "user-123".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["index"], "users");
        assert_eq!(json["key"], "user-123");

        let deserialized: KvGetInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.index, "users");
        assert_eq!(deserialized.key, "user-123");
    }

    #[test]
    fn test_kv_list_keys_input_serde() {
        let input = KvListKeysInput {
            prefix: "user-".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["prefix"], "user-");

        let deserialized: KvListKeysInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.prefix, "user-");
    }

    #[test]
    fn test_kv_list_keys_with_prefix_input_serde() {
        let input = KvListKeysWithPrefixInput {
            prefix: "user-".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["prefix"], "user-");

        let deserialized: KvListKeysWithPrefixInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.prefix, "user-");
    }

    #[test]
    fn test_kv_list_input_serde() {
        let input = KvListInput {
            index: "users".to_string(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["index"], "users");

        let deserialized: KvListInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.index, "users");
    }

    #[test]
    fn test_kv_update_input_serde() {
        let input = KvUpdateInput {
            index: "users".to_string(),
            key: "user-123".to_string(),
            ops: vec![],
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["index"], "users");
        assert_eq!(json["key"], "user-123");
        assert_eq!(json["ops"], json!([]));

        let deserialized: KvUpdateInput = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.index, "users");
        assert_eq!(deserialized.key, "user-123");
        assert_eq!(deserialized.ops.len(), 0);
    }

    #[test]
    fn test_update_result_with_old_value() {
        let result = UpdateResult {
            old_value: Some(json!({"name": "old"})),
            new_value: json!({"name": "new"}),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["old_value"]["name"], "old");
        assert_eq!(json["new_value"]["name"], "new");

        let deserialized: UpdateResult = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.old_value.unwrap()["name"], "old");
        assert_eq!(deserialized.new_value["name"], "new");
    }

    #[test]
    fn test_update_result_without_old_value() {
        let result = UpdateResult {
            old_value: None,
            new_value: json!({"name": "new"}),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert!(json["old_value"].is_null());
        assert_eq!(json["new_value"]["name"], "new");

        let deserialized: UpdateResult = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.old_value, None);
        assert_eq!(deserialized.new_value["name"], "new");
    }
}
