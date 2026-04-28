// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use iii_sdk::{UpdateOp, UpdateOpError, types::MergePath};
use serde_json::{Map, Number, Value};

// Validation bounds for merge ops. Mirrored in
// `engine/src/workers/redis.rs` Lua so Redis-backed adapters share
// identical semantics. Update both sides together.
pub(crate) const MAX_PATH_DEPTH: usize = 32;
pub(crate) const MAX_SEGMENT_BYTES: usize = 256;
pub(crate) const MAX_VALUE_DEPTH: usize = 16;
pub(crate) const MAX_VALUE_KEYS: usize = 1024;

// Segments and merge-value top-level keys matching any of these are
// rejected. They are JS prototype-pollution sinks reachable through
// any consumer that does `Object.assign(obj, mergedValue)` or similar
// unsafe spreading on the merged JSON.
pub(crate) const PROTO_POLLUTION_KEYS: &[&str] = &["__proto__", "constructor", "prototype"];

pub(crate) const ERR_PATH_TOO_DEEP: &str = "merge.path.too_deep";
pub(crate) const ERR_SEGMENT_TOO_LONG: &str = "merge.path.segment_too_long";
pub(crate) const ERR_EMPTY_SEGMENT: &str = "merge.path.empty_segment";
pub(crate) const ERR_PATH_PROTO: &str = "merge.path.proto_polluted";
pub(crate) const ERR_VALUE_TOO_DEEP: &str = "merge.value.too_deep";
pub(crate) const ERR_VALUE_TOO_MANY_KEYS: &str = "merge.value.too_many_keys";
pub(crate) const ERR_VALUE_PROTO: &str = "merge.value.proto_polluted";
pub(crate) const ERR_VALUE_NOT_OBJECT: &str = "merge.value.not_an_object";

const DOC_URL_BASE: &str = "https://iii.dev/docs/workers/iii-state#merge-bounds";

fn err(op_index: usize, code: &str, message: String) -> UpdateOpError {
    UpdateOpError {
        op_index,
        code: code.to_string(),
        message,
        doc_url: Some(DOC_URL_BASE.to_string()),
    }
}

/// Normalize an `Option<MergePath>` into a borrowed slice of segments.
/// `None`, `Single("")`, and `Segments(vec![])` all collapse to an
/// empty slice meaning "root merge".
fn merge_path_segments<'a>(path: &'a Option<MergePath>) -> &'a [String] {
    match path {
        None => &[],
        Some(MergePath::Single(s)) => {
            if s.is_empty() {
                &[]
            } else {
                std::slice::from_ref(s)
            }
        }
        Some(MergePath::Segments(v)) => v.as_slice(),
    }
}

fn validate_merge_path(
    op_index: usize,
    segments: &[String],
    errors: &mut Vec<UpdateOpError>,
) -> bool {
    if segments.len() > MAX_PATH_DEPTH {
        errors.push(err(
            op_index,
            ERR_PATH_TOO_DEEP,
            format!(
                "Path depth {} exceeds maximum of {}",
                segments.len(),
                MAX_PATH_DEPTH
            ),
        ));
        return false;
    }
    for seg in segments {
        if seg.is_empty() {
            errors.push(err(
                op_index,
                ERR_EMPTY_SEGMENT,
                "Path contains an empty segment".to_string(),
            ));
            return false;
        }
        if seg.len() > MAX_SEGMENT_BYTES {
            errors.push(err(
                op_index,
                ERR_SEGMENT_TOO_LONG,
                format!(
                    "Path segment of {} bytes exceeds maximum of {}",
                    seg.len(),
                    MAX_SEGMENT_BYTES
                ),
            ));
            return false;
        }
        if PROTO_POLLUTION_KEYS.contains(&seg.as_str()) {
            errors.push(err(
                op_index,
                ERR_PATH_PROTO,
                format!("Path segment {:?} is a prototype-pollution sink", seg),
            ));
            return false;
        }
    }
    true
}

fn json_depth(value: &Value) -> usize {
    match value {
        Value::Object(map) => 1 + map.values().map(json_depth).max().unwrap_or(0),
        Value::Array(items) => 1 + items.iter().map(json_depth).max().unwrap_or(0),
        _ => 0,
    }
}

fn validate_merge_value(op_index: usize, value: &Value, errors: &mut Vec<UpdateOpError>) -> bool {
    let map = match value {
        Value::Object(map) => map,
        _ => {
            errors.push(err(
                op_index,
                ERR_VALUE_NOT_OBJECT,
                "Merge value must be a JSON object".to_string(),
            ));
            return false;
        }
    };
    if map.len() > MAX_VALUE_KEYS {
        errors.push(err(
            op_index,
            ERR_VALUE_TOO_MANY_KEYS,
            format!(
                "Merge value has {} top-level keys, exceeds maximum of {}",
                map.len(),
                MAX_VALUE_KEYS
            ),
        ));
        return false;
    }
    for k in map.keys() {
        if PROTO_POLLUTION_KEYS.contains(&k.as_str()) {
            errors.push(err(
                op_index,
                ERR_VALUE_PROTO,
                format!(
                    "Merge value top-level key {:?} is a prototype-pollution sink",
                    k
                ),
            ));
            return false;
        }
    }
    let depth = json_depth(value);
    if depth > MAX_VALUE_DEPTH {
        errors.push(err(
            op_index,
            ERR_VALUE_TOO_DEEP,
            format!(
                "Merge value JSON nesting depth {} exceeds maximum of {}",
                depth, MAX_VALUE_DEPTH
            ),
        ));
        return false;
    }
    true
}

/// Walk the segment path within `current`, replacing or auto-creating
/// non-object intermediates along the way (RFC 7396-style). Returns a
/// mutable reference to the target object's map, ready for shallow
/// merging. Caller must ensure `current` is itself an object before
/// calling (root case is the only place where this matters).
fn walk_or_create<'a>(
    current: &'a mut Value,
    segments: &[String],
) -> Option<&'a mut Map<String, Value>> {
    // Make sure the root is an object before we descend.
    if !matches!(current, Value::Object(_)) {
        *current = Value::Object(Map::new());
    }
    let mut node = current;
    for seg in segments {
        let map = match node {
            Value::Object(m) => m,
            // Should not happen — we replace non-object nodes below
            // before recursing into them. Defensive bail-out.
            _ => return None,
        };
        // Replace any non-object intermediate with a fresh object.
        let entry = map
            .entry(seg.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        if !matches!(entry, Value::Object(_)) {
            *entry = Value::Object(Map::new());
        }
        node = entry;
    }
    match node {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

pub(crate) fn apply_update_ops(
    old_value: Option<Value>,
    ops: &[UpdateOp],
) -> (Value, Vec<UpdateOpError>) {
    let mut using_missing_default = old_value.is_none();
    let mut current = old_value.unwrap_or_else(|| Value::Object(Map::new()));
    let mut errors: Vec<UpdateOpError> = Vec::new();

    for (op_index, op) in ops.iter().enumerate() {
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
                let segments = merge_path_segments(path);
                if !validate_merge_path(op_index, segments, &mut errors) {
                    continue;
                }
                if !validate_merge_value(op_index, value, &mut errors) {
                    continue;
                }
                let new_map = match value {
                    Value::Object(m) => m,
                    // Already rejected by validate_merge_value above.
                    _ => continue,
                };
                if segments.is_empty() {
                    // Root merge — preserve existing semantics.
                    if let Value::Object(existing_map) = &mut current {
                        for (k, v) in new_map {
                            existing_map.insert(k.clone(), v.clone());
                        }
                        using_missing_default = false;
                    } else {
                        tracing::warn!(
                            "Merge operation requires existing root to be a JSON object"
                        );
                    }
                } else if let Some(target) = walk_or_create(&mut current, segments) {
                    for (k, v) in new_map {
                        target.insert(k.clone(), v.clone());
                    }
                    using_missing_default = false;
                } else {
                    tracing::warn!(
                        path = ?segments,
                        "Merge operation could not resolve target path"
                    );
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

    let _ = using_missing_default;
    (current, errors)
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
    use iii_sdk::{FieldPath, UpdateOp, types::MergePath};
    use serde_json::json;

    use super::apply_update_ops;

    /// Asserts no errors and returns the resulting value. Most tests
    /// don't expect errors; the ones that do call `apply_update_ops`
    /// directly and inspect the second tuple element.
    fn run(old_value: Option<serde_json::Value>, ops: &[UpdateOp]) -> serde_json::Value {
        let (value, errors) = apply_update_ops(old_value, ops);
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
        value
    }

    #[test]
    fn appends_to_array_field_as_single_element() {
        let updated = run(
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
        let updated = run(
            Some(json!({ "transcript": "hello" })),
            &[UpdateOp::append("transcript", json!(" world"))],
        );

        assert_eq!(updated, json!({ "transcript": "hello world" }));
    }

    #[test]
    fn initializes_missing_fields_by_value_kind() {
        let updated = run(
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
        let string_root = run(None, &[UpdateOp::append("", json!("hello"))]);
        let array_root = run(None, &[UpdateOp::append("", json!({"kind": "chunk"}))]);

        assert_eq!(string_root, json!("hello"));
        assert_eq!(array_root, json!([{"kind": "chunk"}]));
    }

    #[test]
    fn skips_incompatible_string_append_value() {
        let updated = run(
            Some(json!({ "transcript": "hello" })),
            &[UpdateOp::append("transcript", json!({"not": "string"}))],
        );

        assert_eq!(updated, json!({ "transcript": "hello" }));
    }

    #[test]
    fn skips_incompatible_object_append_target() {
        let updated = run(
            Some(json!({ "events": {} })),
            &[UpdateOp::append("events", json!("chunk"))],
        );

        assert_eq!(updated, json!({ "events": {} }));
    }

    #[test]
    fn increments_existing_non_number_and_missing_fields() {
        let updated = run(
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
        let updated = run(
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
        let updated = run(
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
        let updated = run(
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
        let updated = run(
            Some(json!({ "events": [] })),
            &[
                UpdateOp::append("events", json!("first")),
                UpdateOp::append("events", json!("second")),
            ],
        );

        assert_eq!(updated, json!({ "events": ["first", "second"] }));
    }

    // ----- Nested merge tests (issue #1546) -----

    fn merge_at(path: impl Into<MergePath>, value: serde_json::Value) -> UpdateOp {
        UpdateOp::merge_at(path, value)
    }

    #[test]
    fn merges_into_existing_first_level_object_field() {
        let updated = run(
            Some(json!({ "session-1": { "ts1": "a" } })),
            &[merge_at("session-1", json!({ "ts2": "b" }))],
        );

        assert_eq!(updated, json!({ "session-1": { "ts1": "a", "ts2": "b" } }));
    }

    #[test]
    fn merges_into_missing_first_level_field_creates_object() {
        let updated = run(
            Some(json!({})),
            &[merge_at("session-1", json!({ "author": "alice" }))],
        );

        assert_eq!(updated, json!({ "session-1": { "author": "alice" } }));
    }

    #[test]
    fn merges_at_nested_path_replacing_non_object_intermediate() {
        // Intermediate "abc" is a non-object string; merge at
        // ["sessions", "abc"] must replace it with {} and merge in.
        let updated = run(
            Some(json!({ "sessions": { "abc": "garbage" } })),
            &[merge_at(
                vec!["sessions", "abc"],
                json!({ "author": "alice" }),
            )],
        );

        assert_eq!(
            updated,
            json!({ "sessions": { "abc": { "author": "alice" } } })
        );
    }

    #[test]
    fn merges_at_nested_path_creating_missing_intermediates() {
        let updated = run(
            Some(json!({})),
            &[merge_at(vec!["a", "b", "c"], json!({ "x": 1 }))],
        );

        assert_eq!(updated, json!({ "a": { "b": { "c": { "x": 1 } } } }));
    }

    #[test]
    fn merges_at_nested_target_shallowly_overwrites_keys() {
        let updated = run(
            Some(json!({
                "sessions": {
                    "abc": { "author": "old", "topic": "preserved" }
                }
            })),
            &[merge_at(
                vec!["sessions", "abc"],
                json!({ "author": "new" }),
            )],
        );

        // "topic" preserved; "author" replaced.
        assert_eq!(
            updated,
            json!({
                "sessions": {
                    "abc": { "author": "new", "topic": "preserved" }
                }
            })
        );
    }

    #[test]
    fn merge_with_non_object_value_returns_error_and_no_ops() {
        let (value, errors) = apply_update_ops(
            Some(json!({ "a": 1 })),
            &[merge_at("foo", json!("not-an-object"))],
        );

        assert_eq!(value, json!({ "a": 1 }));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, super::ERR_VALUE_NOT_OBJECT);
        assert_eq!(errors[0].op_index, 0);
    }

    #[test]
    fn root_level_merge_unchanged_for_none_empty_string_and_empty_array() {
        // path: None
        let none_path = run(
            Some(json!({ "a": 1 })),
            &[UpdateOp::merge(json!({ "b": 2 }))],
        );
        // path: Single("")
        let empty_string = run(Some(json!({ "a": 1 })), &[merge_at("", json!({ "b": 2 }))]);
        // path: Segments(vec![])
        let empty_array = run(
            Some(json!({ "a": 1 })),
            &[merge_at(Vec::<&str>::new(), json!({ "b": 2 }))],
        );

        let expected = json!({ "a": 1, "b": 2 });
        assert_eq!(none_path, expected);
        assert_eq!(empty_string, expected);
        assert_eq!(empty_array, expected);
    }

    #[test]
    fn single_string_path_equivalent_to_single_segment_array() {
        let from_string = run(Some(json!({})), &[merge_at("foo", json!({ "x": 1 }))]);
        let from_array = run(Some(json!({})), &[merge_at(vec!["foo"], json!({ "x": 1 }))]);

        assert_eq!(from_string, from_array);
        assert_eq!(from_string, json!({ "foo": { "x": 1 } }));
    }

    #[test]
    fn literal_dotted_segment_treated_as_one_key() {
        // ["a.b"] is a single literal key, not a→b traversal.
        let updated = run(
            Some(json!({ "a": { "b": { "preserved": true } } })),
            &[merge_at(vec!["a.b"], json!({ "x": 1 }))],
        );

        assert_eq!(
            updated,
            json!({
                "a": { "b": { "preserved": true } },
                "a.b": { "x": 1 }
            })
        );
    }

    // ----- Validation rejection tests -----

    #[test]
    fn rejects_path_too_deep() {
        let path: Vec<String> = (0..super::MAX_PATH_DEPTH + 1)
            .map(|i| format!("k{i}"))
            .collect();
        let (_value, errors) =
            apply_update_ops(Some(json!({})), &[merge_at(path, json!({ "x": 1 }))]);

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, super::ERR_PATH_TOO_DEEP);
    }

    #[test]
    fn rejects_oversized_segment() {
        let big = "a".repeat(super::MAX_SEGMENT_BYTES + 1);
        let (_value, errors) =
            apply_update_ops(Some(json!({})), &[merge_at(vec![big], json!({ "x": 1 }))]);

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, super::ERR_SEGMENT_TOO_LONG);
    }

    #[test]
    fn rejects_empty_segment() {
        let (_value, errors) = apply_update_ops(
            Some(json!({})),
            &[merge_at(vec!["a", ""], json!({ "x": 1 }))],
        );

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, super::ERR_EMPTY_SEGMENT);
    }

    #[test]
    fn rejects_proto_polluted_path_segment() {
        for sink in super::PROTO_POLLUTION_KEYS {
            let (_value, errors) =
                apply_update_ops(Some(json!({})), &[merge_at(vec![*sink], json!({ "x": 1 }))]);

            assert_eq!(errors.len(), 1, "expected proto rejection for {sink}");
            assert_eq!(errors[0].code, super::ERR_PATH_PROTO);
        }
    }

    #[test]
    fn rejects_proto_polluted_value_top_level_key() {
        let (_value, errors) = apply_update_ops(
            Some(json!({})),
            &[merge_at(
                "foo",
                json!({ "__proto__": { "polluted": true } }),
            )],
        );

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, super::ERR_VALUE_PROTO);
    }

    #[test]
    fn rejects_value_too_deep() {
        // Build nested object of depth MAX_VALUE_DEPTH + 1.
        let mut v = json!({});
        for _ in 0..=super::MAX_VALUE_DEPTH {
            v = json!({ "n": v });
        }
        let (_value, errors) = apply_update_ops(Some(json!({})), &[merge_at("foo", v)]);

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, super::ERR_VALUE_TOO_DEEP);
    }
}
