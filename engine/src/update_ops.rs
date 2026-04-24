// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use iii_sdk::UpdateOp;
use serde_json::{Map, Number, Value};

pub(crate) fn apply_update_ops(old_value: Option<Value>, ops: &[UpdateOp]) -> Value {
    let mut using_missing_default = old_value.is_none();
    let mut current = old_value.unwrap_or_else(|| Value::Object(Map::new()));

    for op in ops {
        match op {
            UpdateOp::Set { path, value } => {
                if path.0.is_empty()
                    && let Some(value) = value
                {
                    current = value.clone();
                    using_missing_default = false;
                } else if let Value::Object(ref mut map) = current {
                    map.insert(path.0.clone(), value.clone().unwrap_or(Value::Null));
                    using_missing_default = false;
                } else {
                    tracing::warn!(
                        path = %path.0,
                        "Set operation with path requires existing value to be a JSON object"
                    );
                }
            }
            UpdateOp::Merge { path, value } => {
                if path.is_none() || path.as_ref().is_some_and(|path| path.0.is_empty()) {
                    if let (Value::Object(existing_map), Value::Object(new_map)) =
                        (&mut current, value)
                    {
                        for (k, v) in new_map {
                            existing_map.insert(k.clone(), v.clone());
                        }
                        using_missing_default = false;
                    } else {
                        tracing::warn!(
                            "Merge operation requires both existing and new values to be JSON objects"
                        );
                    }
                } else if let Some(path) = path {
                    tracing::warn!(path = %path.0, "Only root-level merge is supported");
                }
            }
            UpdateOp::Increment { path, by } => {
                if let Value::Object(ref mut map) = current {
                    if let Some(existing_val) = map.get_mut(&path.0) {
                        if let Some(num) = existing_val.as_i64() {
                            *existing_val = Value::Number(Number::from(num + *by));
                        } else {
                            *existing_val = Value::Number(Number::from(*by));
                        }
                    } else {
                        map.insert(path.0.clone(), Value::Number(Number::from(*by)));
                    }
                    using_missing_default = false;
                } else {
                    tracing::warn!(
                        path = %path.0,
                        "Increment operation requires existing value to be a JSON object"
                    );
                }
            }
            UpdateOp::Decrement { path, by } => {
                if let Value::Object(ref mut map) = current {
                    if let Some(existing_val) = map.get_mut(&path.0) {
                        if let Some(num) = existing_val.as_i64() {
                            *existing_val = Value::Number(Number::from(num - *by));
                        } else {
                            *existing_val = Value::Number(Number::from(-*by));
                        }
                    } else {
                        map.insert(path.0.clone(), Value::Number(Number::from(-*by)));
                    }
                    using_missing_default = false;
                } else {
                    tracing::warn!(
                        path = %path.0,
                        "Decrement operation requires existing value to be a JSON object"
                    );
                }
            }
            UpdateOp::Append { path, value } => {
                if path.0.is_empty() {
                    if using_missing_default {
                        current = Value::Null;
                    }
                    if append_to_target(&mut current, value, "root") {
                        using_missing_default = false;
                    }
                } else if let Value::Object(ref mut map) = current {
                    if let Some(existing_val) = map.get_mut(&path.0) {
                        append_to_target(existing_val, value, &path.0);
                    } else {
                        map.insert(path.0.clone(), initial_append_value(value));
                    }
                    using_missing_default = false;
                } else {
                    tracing::warn!(
                        path = %path.0,
                        "Append operation with path requires existing value to be a JSON object"
                    );
                }
            }
            UpdateOp::Remove { path } => {
                if let Value::Object(ref mut map) = current {
                    map.remove(&path.0);
                    using_missing_default = false;
                } else {
                    tracing::warn!(
                        path = %path.0,
                        "Remove operation requires existing value to be a JSON object"
                    );
                }
            }
        }
    }

    current
}

fn append_to_target(target: &mut Value, value: &Value, path: &str) -> bool {
    match target {
        Value::Array(items) => {
            items.push(value.clone());
            true
        }
        Value::String(existing) => {
            if let Some(chunk) = value.as_str() {
                existing.push_str(chunk);
                true
            } else {
                tracing::warn!(
                    path,
                    "Append operation on a string target requires a string value"
                );
                false
            }
        }
        Value::Null => {
            *target = initial_append_value(value);
            true
        }
        _ => {
            tracing::warn!(
                path,
                "Append operation requires target to be an array, string, null, or missing field"
            );
            false
        }
    }
}

fn initial_append_value(value: &Value) -> Value {
    if value.is_string() {
        value.clone()
    } else {
        Value::Array(vec![value.clone()])
    }
}

#[cfg(test)]
mod tests {
    use iii_sdk::{FieldPath, UpdateOp};
    use serde_json::json;

    use super::apply_update_ops;

    #[test]
    fn appends_to_array_field_as_single_element() {
        let updated = apply_update_ops(
            Some(json!({ "events": [{"kind": "start"}] })),
            &[UpdateOp::append("events", json!({"kind": "chunk"}))],
        );

        assert_eq!(
            updated,
            json!({ "events": [{"kind": "start"}, {"kind": "chunk"}] })
        );
    }

    #[test]
    fn concatenates_string_fields() {
        let updated = apply_update_ops(
            Some(json!({ "transcript": "hello" })),
            &[UpdateOp::append("transcript", json!(" world"))],
        );

        assert_eq!(updated, json!({ "transcript": "hello world" }));
    }

    #[test]
    fn initializes_missing_fields_by_value_kind() {
        let updated = apply_update_ops(
            Some(json!({})),
            &[
                UpdateOp::append("transcript", json!("hello")),
                UpdateOp::append("events", json!({"kind": "chunk"})),
            ],
        );

        assert_eq!(
            updated,
            json!({
                "transcript": "hello",
                "events": [{"kind": "chunk"}],
            })
        );
    }

    #[test]
    fn initializes_missing_root_by_value_kind() {
        let string_root = apply_update_ops(None, &[UpdateOp::append("", json!("hello"))]);
        let array_root = apply_update_ops(None, &[UpdateOp::append("", json!({"kind": "chunk"}))]);

        assert_eq!(string_root, json!("hello"));
        assert_eq!(array_root, json!([{"kind": "chunk"}]));
    }

    #[test]
    fn skips_incompatible_string_append_value() {
        let updated = apply_update_ops(
            Some(json!({ "transcript": "hello" })),
            &[UpdateOp::append("transcript", json!({"not": "string"}))],
        );

        assert_eq!(updated, json!({ "transcript": "hello" }));
    }

    #[test]
    fn skips_incompatible_object_append_target() {
        let updated = apply_update_ops(
            Some(json!({ "events": {} })),
            &[UpdateOp::append("events", json!("chunk"))],
        );

        assert_eq!(updated, json!({ "events": {} }));
    }

    #[test]
    fn increments_existing_non_number_and_missing_fields() {
        let updated = apply_update_ops(
            Some(json!({ "count": 2, "bad": "value" })),
            &[
                UpdateOp::increment("count", 3),
                UpdateOp::increment("bad", 3),
                UpdateOp::increment("missing", 3),
            ],
        );

        assert_eq!(updated, json!({ "count": 5, "bad": 3, "missing": 3 }));
    }

    #[test]
    fn decrements_existing_non_number_and_missing_fields() {
        let updated = apply_update_ops(
            Some(json!({ "count": 5, "bad": "value" })),
            &[
                UpdateOp::decrement("count", 3),
                UpdateOp::decrement("bad", 3),
                UpdateOp::decrement("missing", 3),
            ],
        );

        assert_eq!(updated, json!({ "count": 2, "bad": -3, "missing": -3 }));
    }

    #[test]
    fn preserves_order_across_multiple_numeric_ops() {
        let updated = apply_update_ops(
            Some(json!({ "count": 10 })),
            &[
                UpdateOp::increment("count", 5),
                UpdateOp::decrement("count", 3),
            ],
        );

        assert_eq!(updated, json!({ "count": 12 }));
    }

    #[test]
    fn dotted_paths_are_first_level_fields() {
        let updated = apply_update_ops(
            Some(json!({ "user.name": ["A"], "user": { "name": ["B"] } })),
            &[UpdateOp::Append {
                path: FieldPath("user.name".to_string()),
                value: json!("C"),
            }],
        );

        assert_eq!(
            updated,
            json!({ "user.name": ["A", "C"], "user": { "name": ["B"] } })
        );
    }

    #[test]
    fn preserves_order_across_multiple_append_ops() {
        let updated = apply_update_ops(
            Some(json!({ "events": [] })),
            &[
                UpdateOp::append("events", json!("first")),
                UpdateOp::append("events", json!("second")),
            ],
        );

        assert_eq!(updated, json!({ "events": ["first", "second"] }));
    }
}
