// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
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
pub(crate) const ERR_SET_TARGET_NOT_OBJECT: &str = "set.target_not_object";
pub(crate) const ERR_APPEND_TARGET_NOT_OBJECT: &str = "append.target_not_object";
pub(crate) const ERR_APPEND_TYPE_MISMATCH: &str = "append.type_mismatch";
pub(crate) const ERR_INCREMENT_TARGET_NOT_OBJECT: &str = "increment.target_not_object";
pub(crate) const ERR_INCREMENT_NOT_NUMBER: &str = "increment.not_number";
pub(crate) const ERR_DECREMENT_TARGET_NOT_OBJECT: &str = "decrement.target_not_object";
pub(crate) const ERR_DECREMENT_NOT_NUMBER: &str = "decrement.not_number";
pub(crate) const ERR_REMOVE_TARGET_NOT_OBJECT: &str = "remove.target_not_object";

const DOC_URL_BASE: &str = "https://iii.dev/docs/workers/iii-state#error-codes";

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
fn merge_path_segments(path: &Option<MergePath>) -> &[String] {
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

fn field_path_segments(path: &String) -> &[String] {
    if path.is_empty() {
        &[]
    } else {
        std::slice::from_ref(path)
    }
}

fn path_error_code(op_name: &str, reason: &str) -> String {
    match (op_name, reason) {
        ("merge", "too_deep") => ERR_PATH_TOO_DEEP.to_string(),
        ("merge", "segment_too_long") => ERR_SEGMENT_TOO_LONG.to_string(),
        ("merge", "empty_segment") => ERR_EMPTY_SEGMENT.to_string(),
        ("merge", "proto_polluted") => ERR_PATH_PROTO.to_string(),
        _ => format!("{op_name}.path.{reason}"),
    }
}

fn validate_op_path(
    op_name: &str,
    op_index: usize,
    segments: &[String],
    errors: &mut Vec<UpdateOpError>,
) -> bool {
    if segments.len() > MAX_PATH_DEPTH {
        errors.push(err(
            op_index,
            &path_error_code(op_name, "too_deep"),
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
                &path_error_code(op_name, "empty_segment"),
                "Path contains an empty segment".to_string(),
            ));
            return false;
        }
        if seg.len() > MAX_SEGMENT_BYTES {
            errors.push(err(
                op_index,
                &path_error_code(op_name, "segment_too_long"),
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
                &path_error_code(op_name, "proto_polluted"),
                format!("Path segment '{seg}' is not allowed (prototype pollution)."),
            ));
            return false;
        }
    }
    true
}

fn validate_merge_path(
    op_index: usize,
    segments: &[String],
    errors: &mut Vec<UpdateOpError>,
) -> bool {
    validate_op_path("merge", op_index, segments, errors)
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

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn path_label(path: &str) -> &str {
    if path.is_empty() { "root" } else { path }
}

fn path_label_segments(segments: &[String]) -> String {
    if segments.is_empty() {
        "root".to_string()
    } else {
        // Bracketed form to keep the literal-segment contract visible —
        // never a dot-joined form, since dots are not separators in iii.
        format!("[{}]", segments.join(", "))
    }
}

fn increment_number(value: &Value, by: i64) -> Option<Value> {
    if let Some(num) = value.as_i64()
        && let Some(sum) = num.checked_add(by)
    {
        return Some(Value::Number(Number::from(sum)));
    }

    if let Some(num) = value.as_u64() {
        let sum = if by >= 0 {
            num.checked_add(by as u64)?
        } else {
            num.checked_sub(by.unsigned_abs())?
        };
        return Some(Value::Number(Number::from(sum)));
    }

    if let Value::Number(number) = value
        && number.is_f64()
    {
        return Number::from_f64(number.as_f64()? + by as f64).map(Value::Number);
    }

    None
}

fn decrement_number(value: &Value, by: i64) -> Option<Value> {
    if let Some(num) = value.as_i64()
        && let Some(diff) = num.checked_sub(by)
    {
        return Some(Value::Number(Number::from(diff)));
    }

    if let Some(num) = value.as_u64() {
        let diff = if by >= 0 {
            num.checked_sub(by as u64)?
        } else {
            num.checked_add(by.unsigned_abs())?
        };
        return Some(Value::Number(Number::from(diff)));
    }

    if let Value::Number(number) = value
        && number.is_f64()
    {
        return Number::from_f64(number.as_f64()? - by as f64).map(Value::Number);
    }

    None
}

fn missing_decrement_value(by: i64) -> Value {
    if let Some(negated) = by.checked_neg() {
        Value::Number(Number::from(negated))
    } else {
        Value::Number(Number::from(by.unsigned_abs()))
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
                let segments = field_path_segments(&path.0);
                if !validate_op_path("set", op_index, segments, &mut errors) {
                    continue;
                }
                if path.0.is_empty() {
                    current = value.clone().unwrap_or(Value::Null);
                    using_missing_default = false;
                } else if let Value::Object(ref mut map) = current {
                    map.insert(path.0.clone(), value.clone().unwrap_or(Value::Null));
                    using_missing_default = false;
                } else {
                    errors.push(err(
                        op_index,
                        ERR_SET_TARGET_NOT_OBJECT,
                        format!(
                            "Cannot set at path '{}': target is {}, expected object.",
                            path_label(&path.0),
                            json_type_name(&current)
                        ),
                    ));
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
                let segments = field_path_segments(&path.0);
                if !validate_op_path("increment", op_index, segments, &mut errors) {
                    continue;
                }
                if path.0.is_empty() {
                    if using_missing_default {
                        current = Value::Number(Number::from(*by));
                    } else if let Some(updated) = increment_number(&current, *by) {
                        current = updated;
                    } else {
                        errors.push(err(
                            op_index,
                            ERR_INCREMENT_NOT_NUMBER,
                            format!(
                                "Expected number at path '{}', got {}.",
                                path_label(&path.0),
                                json_type_name(&current)
                            ),
                        ));
                        continue;
                    }
                    using_missing_default = false;
                } else if let Value::Object(ref mut map) = current {
                    if let Some(existing_val) = map.get_mut(&path.0) {
                        if let Some(updated) = increment_number(existing_val, *by) {
                            *existing_val = updated;
                        } else {
                            errors.push(err(
                                op_index,
                                ERR_INCREMENT_NOT_NUMBER,
                                format!(
                                    "Expected number at path '{}', got {}.",
                                    path_label(&path.0),
                                    json_type_name(existing_val)
                                ),
                            ));
                            continue;
                        }
                    } else {
                        map.insert(path.0.clone(), Value::Number(Number::from(*by)));
                    }
                    using_missing_default = false;
                } else {
                    errors.push(err(
                        op_index,
                        ERR_INCREMENT_TARGET_NOT_OBJECT,
                        format!(
                            "Cannot increment at path '{}': target is {}, expected object.",
                            path_label(&path.0),
                            json_type_name(&current)
                        ),
                    ));
                }
            }
            UpdateOp::Decrement { path, by } => {
                let segments = field_path_segments(&path.0);
                if !validate_op_path("decrement", op_index, segments, &mut errors) {
                    continue;
                }
                if path.0.is_empty() {
                    if using_missing_default {
                        current = missing_decrement_value(*by);
                    } else if let Some(updated) = decrement_number(&current, *by) {
                        current = updated;
                    } else {
                        errors.push(err(
                            op_index,
                            ERR_DECREMENT_NOT_NUMBER,
                            format!(
                                "Expected number at path '{}', got {}.",
                                path_label(&path.0),
                                json_type_name(&current)
                            ),
                        ));
                        continue;
                    }
                    using_missing_default = false;
                } else if let Value::Object(ref mut map) = current {
                    if let Some(existing_val) = map.get_mut(&path.0) {
                        if let Some(updated) = decrement_number(existing_val, *by) {
                            *existing_val = updated;
                        } else {
                            errors.push(err(
                                op_index,
                                ERR_DECREMENT_NOT_NUMBER,
                                format!(
                                    "Expected number at path '{}', got {}.",
                                    path_label(&path.0),
                                    json_type_name(existing_val)
                                ),
                            ));
                            continue;
                        }
                    } else {
                        map.insert(path.0.clone(), missing_decrement_value(*by));
                    }
                    using_missing_default = false;
                } else {
                    errors.push(err(
                        op_index,
                        ERR_DECREMENT_TARGET_NOT_OBJECT,
                        format!(
                            "Cannot decrement at path '{}': target is {}, expected object.",
                            path_label(&path.0),
                            json_type_name(&current)
                        ),
                    ));
                }
            }
            UpdateOp::Append { path, value } => {
                // Validation order is load-bearing (SDD §10):
                //   1. validate_op_path  (bounds + proto-pollution rejection)
                //   2. root-is-object    (before walk_or_create can mutate)
                //   3. walk_or_create    (nested only — auto-creates intermediates)
                //   4. leaf-type matrix  (FR-11)
                let segments = merge_path_segments(path);
                if !validate_op_path("append", op_index, segments, &mut errors) {
                    continue;
                }
                if segments.is_empty() {
                    // Root append (path absent / Single("") / Segments([]))
                    // — legacy semantics preserved (initial_append_value
                    // wraps non-strings into a one-element array, keeps
                    // strings as strings for the string-concat tier).
                    if using_missing_default {
                        current = Value::Null;
                    }
                    if append_to_target(&mut current, value, "root", op_index, &mut errors) {
                        using_missing_default = false;
                    }
                } else if !matches!(current, Value::Object(_)) {
                    // Step 2: non-empty path requires object root.
                    errors.push(err(
                        op_index,
                        ERR_APPEND_TARGET_NOT_OBJECT,
                        format!(
                            "Cannot append at path '{}': target is {}, expected object.",
                            path_label_segments(segments),
                            json_type_name(&current)
                        ),
                    ));
                } else if segments.len() == 1 {
                    // Single-segment path: back-compat with the old
                    // `FieldPath` shape — `initial_append_value` keeps
                    // string-concat tier for missing leaves.
                    let leaf_key = &segments[0];
                    if let Value::Object(ref mut map) = current {
                        if let Some(existing_val) = map.get_mut(leaf_key) {
                            append_to_target(existing_val, value, leaf_key, op_index, &mut errors);
                        } else {
                            map.insert(leaf_key.clone(), initial_append_value(value));
                        }
                        using_missing_default = false;
                    }
                } else {
                    // Nested path: walk parent (creating intermediates),
                    // then operate on the leaf key. FR-11 nested-path rule:
                    // missing leaf → ALWAYS array (no string-concat tier).
                    let (leaf_key, parent_segments) = segments.split_last().unwrap();
                    if let Some(parent_map) = walk_or_create(&mut current, parent_segments) {
                        if let Some(existing_val) = parent_map.get_mut(leaf_key) {
                            append_to_target(existing_val, value, leaf_key, op_index, &mut errors);
                        } else {
                            // FR-11: nested-path missing leaf is always an array.
                            parent_map.insert(leaf_key.clone(), Value::Array(vec![value.clone()]));
                        }
                        using_missing_default = false;
                    } else {
                        tracing::warn!(
                            path = ?segments,
                            "Append operation could not resolve parent path"
                        );
                    }
                }
            }
            UpdateOp::Remove { path } => {
                let segments = field_path_segments(&path.0);
                if !validate_op_path("remove", op_index, segments, &mut errors) {
                    continue;
                }
                if path.0.is_empty() {
                    current = Value::Null;
                    using_missing_default = false;
                } else if let Value::Object(ref mut map) = current {
                    map.remove(&path.0);
                    using_missing_default = false;
                } else {
                    errors.push(err(
                        op_index,
                        ERR_REMOVE_TARGET_NOT_OBJECT,
                        format!(
                            "Cannot remove at path '{}': target is {}, expected object.",
                            path_label(&path.0),
                            json_type_name(&current)
                        ),
                    ));
                }
            }
        }
    }

    let _ = using_missing_default;
    (current, errors)
}

fn append_to_target(
    target: &mut Value,
    value: &Value,
    path: &str,
    op_index: usize,
    errors: &mut Vec<UpdateOpError>,
) -> bool {
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
                errors.push(err(
                    op_index,
                    ERR_APPEND_TYPE_MISMATCH,
                    format!(
                        "Expected string append value at path '{}', got {}.",
                        path_label(path),
                        json_type_name(value)
                    ),
                ));
                false
            }
        }
        Value::Null => {
            *target = initial_append_value(value);
            true
        }
        _ => {
            errors.push(err(
                op_index,
                ERR_APPEND_TYPE_MISMATCH,
                format!(
                    "Cannot append at path '{}': target is {}, expected array, string, null, or missing field.",
                    path_label(path),
                    json_type_name(target)
                ),
            ));
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
    use serde_json::{Number, json};

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
    fn set_empty_path_replaces_root_even_with_null_value() {
        let updated = run(
            Some(json!({ "": "empty-field", "keep": true })),
            &[UpdateOp::Set {
                path: "".into(),
                value: None,
            }],
        );

        assert_eq!(updated, json!(null));
    }

    #[test]
    fn skips_incompatible_string_append_value() {
        let (updated, errors) = apply_update_ops(
            Some(json!({ "transcript": "hello" })),
            &[UpdateOp::append("transcript", json!({"not": "string"}))],
        );

        assert_eq!(updated, json!({ "transcript": "hello" }));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, super::ERR_APPEND_TYPE_MISMATCH);
    }

    #[test]
    fn skips_incompatible_object_append_target() {
        let (updated, errors) = apply_update_ops(
            Some(json!({ "events": {} })),
            &[UpdateOp::append("events", json!("chunk"))],
        );

        assert_eq!(updated, json!({ "events": {} }));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, super::ERR_APPEND_TYPE_MISMATCH);
    }

    #[test]
    fn increments_existing_number_and_missing_fields() {
        let updated = run(
            Some(json!({ "count": 2 })),
            &[
                UpdateOp::increment("count", 3),
                UpdateOp::increment("missing", 3),
            ],
        );

        assert_eq!(updated, json!({ "count": 5, "missing": 3 }));
    }

    #[test]
    fn increment_rejects_existing_non_number() {
        let (updated, errors) = apply_update_ops(
            Some(json!({ "count": 2, "bad": "value" })),
            &[
                UpdateOp::increment("count", 3),
                UpdateOp::increment("bad", 3),
                UpdateOp::increment("missing", 3),
            ],
        );

        assert_eq!(updated, json!({ "count": 5, "bad": "value", "missing": 3 }));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, super::ERR_INCREMENT_NOT_NUMBER);
        assert_eq!(errors[0].op_index, 1);
    }

    #[test]
    fn decrements_existing_number_and_missing_fields() {
        let updated = run(
            Some(json!({ "count": 5 })),
            &[
                UpdateOp::decrement("count", 3),
                UpdateOp::decrement("missing", 3),
            ],
        );

        assert_eq!(updated, json!({ "count": 2, "missing": -3 }));
    }

    #[test]
    fn decrement_rejects_existing_non_number() {
        let (updated, errors) = apply_update_ops(
            Some(json!({ "count": 5, "bad": "value" })),
            &[
                UpdateOp::decrement("count", 3),
                UpdateOp::decrement("bad", 3),
                UpdateOp::decrement("missing", 3),
            ],
        );

        assert_eq!(
            updated,
            json!({ "count": 2, "bad": "value", "missing": -3 })
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, super::ERR_DECREMENT_NOT_NUMBER);
        assert_eq!(errors[0].op_index, 1);
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
    fn numeric_ops_accept_json_float_fields() {
        let updated = run(
            Some(json!({ "count": 1.5 })),
            &[
                UpdateOp::increment("count", 2),
                UpdateOp::decrement("count", 1),
            ],
        );

        assert_eq!(updated, json!({ "count": 2.5 }));
    }

    #[test]
    fn numeric_ops_reject_null_fields_instead_of_treating_them_as_missing() {
        let (updated, errors) = apply_update_ops(
            Some(json!({ "inc": null, "dec": null })),
            &[UpdateOp::increment("inc", 1), UpdateOp::decrement("dec", 1)],
        );

        assert_eq!(updated, json!({ "inc": null, "dec": null }));
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].op_index, 0);
        assert_eq!(errors[0].code, super::ERR_INCREMENT_NOT_NUMBER);
        assert_eq!(errors[1].op_index, 1);
        assert_eq!(errors[1].code, super::ERR_DECREMENT_NOT_NUMBER);
    }

    #[test]
    fn numeric_ops_with_zero_still_validate_existing_type() {
        let (updated, errors) = apply_update_ops(
            Some(json!({ "bad": "value", "count": 2 })),
            &[
                UpdateOp::increment("bad", 0),
                UpdateOp::decrement("count", 0),
            ],
        );

        assert_eq!(updated, json!({ "bad": "value", "count": 2 }));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].op_index, 0);
        assert_eq!(errors[0].code, super::ERR_INCREMENT_NOT_NUMBER);
    }

    #[test]
    fn numeric_overflow_promotes_to_exact_unsigned_integer() {
        let updated = run(
            Some(json!({ "count": i64::MAX })),
            &[UpdateOp::increment("count", 1)],
        );

        assert_eq!(updated["count"].as_u64(), Some(i64::MAX as u64 + 1));
    }

    #[test]
    fn numeric_ops_preserve_large_unsigned_integer_precision() {
        let large = Number::from(i64::MAX as u64 + 10);
        let updated = run(
            Some(json!({ "inc": large.clone(), "dec": large })),
            &[UpdateOp::increment("inc", 1), UpdateOp::decrement("dec", 1)],
        );

        assert_eq!(updated["inc"].as_u64(), Some(i64::MAX as u64 + 11));
        assert_eq!(updated["dec"].as_u64(), Some(i64::MAX as u64 + 9));
    }

    #[test]
    fn missing_decrement_by_i64_min_initializes_exact_unsigned_value() {
        let updated = run(Some(json!({})), &[UpdateOp::decrement("count", i64::MIN)]);

        assert_eq!(updated["count"].as_u64(), Some(i64::MIN.unsigned_abs()));
    }

    #[test]
    fn empty_path_numeric_ops_target_root_value() {
        let incremented = run(Some(json!(2)), &[UpdateOp::increment("", 3)]);
        let decremented = run(Some(json!(5)), &[UpdateOp::decrement("", 3)]);

        assert_eq!(incremented, json!(5));
        assert_eq!(decremented, json!(2));
    }

    #[test]
    fn empty_path_remove_clears_root_value() {
        let updated = run(
            Some(json!({ "": "empty-field", "keep": true })),
            &[UpdateOp::remove("")],
        );

        assert_eq!(updated, json!(null));
    }

    #[test]
    fn failed_ops_continue_and_preserve_error_indexes() {
        let (updated, errors) = apply_update_ops(
            Some(json!({ "bad": "value", "events": {} })),
            &[
                UpdateOp::increment("bad", 1),
                UpdateOp::Set {
                    path: "__proto__".into(),
                    value: Some(json!(true)),
                },
                UpdateOp::append("events", json!("chunk")),
                UpdateOp::Set {
                    path: "ok".into(),
                    value: Some(json!(true)),
                },
            ],
        );

        assert_eq!(updated, json!({ "bad": "value", "events": {}, "ok": true }));
        assert_eq!(errors.len(), 3);
        assert_eq!(errors[0].op_index, 0);
        assert_eq!(errors[0].code, super::ERR_INCREMENT_NOT_NUMBER);
        assert_eq!(errors[1].op_index, 1);
        assert_eq!(errors[1].code, "set.path.proto_polluted");
        assert_eq!(errors[2].op_index, 2);
        assert_eq!(errors[2].code, super::ERR_APPEND_TYPE_MISMATCH);
    }

    #[test]
    fn dotted_paths_are_first_level_fields() {
        // Dotted strings are ALWAYS literal segments — never traversed
        // as `user.name`. This regression test locks the legacy
        // FieldPath-style literal-key contract through the
        // FieldPath → MergePath compatibility shim.
        let updated = run(
            Some(json!({ "user.name": ["A"], "user": { "name": ["B"] } })),
            &[UpdateOp::Append {
                path: Some(FieldPath("user.name".to_string()).into()),
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

    #[test]
    fn append_array_value_to_array_target_as_single_element() {
        let updated = run(
            Some(json!({ "events": [] })),
            &[UpdateOp::append("events", json!(["nested", "array"]))],
        );

        assert_eq!(updated, json!({ "events": [["nested", "array"]] }));
    }

    #[test]
    fn append_to_null_field_initializes_by_value_kind() {
        let updated = run(
            Some(json!({ "string": null, "array": null })),
            &[
                UpdateOp::append("string", json!("chunk")),
                UpdateOp::append("array", json!({ "kind": "chunk" })),
            ],
        );

        assert_eq!(
            updated,
            json!({
                "string": "chunk",
                "array": [{ "kind": "chunk" }],
            })
        );
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
    fn root_merge_on_non_object_current_value_remains_silent_noop() {
        let (updated, errors) =
            apply_update_ops(Some(json!("leaf")), &[UpdateOp::merge(json!({ "a": 1 }))]);

        assert_eq!(updated, json!("leaf"));
        assert!(errors.is_empty());
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
    fn rejects_all_proto_pollution_sinks_for_each_non_merge_op() {
        for sink in super::PROTO_POLLUTION_KEYS {
            let cases = [
                (
                    "set",
                    UpdateOp::Set {
                        path: (*sink).into(),
                        value: Some(json!(true)),
                    },
                    "set.path.proto_polluted",
                ),
                (
                    "append",
                    UpdateOp::append(*sink, json!("chunk")),
                    "append.path.proto_polluted",
                ),
                (
                    "increment",
                    UpdateOp::increment(*sink, 1),
                    "increment.path.proto_polluted",
                ),
                (
                    "decrement",
                    UpdateOp::decrement(*sink, 1),
                    "decrement.path.proto_polluted",
                ),
                (
                    "remove",
                    UpdateOp::Remove {
                        path: (*sink).into(),
                    },
                    "remove.path.proto_polluted",
                ),
            ];

            for (op_name, op, code) in cases {
                let (_value, errors) = apply_update_ops(Some(json!({})), &[op]);
                assert_eq!(
                    errors.len(),
                    1,
                    "expected proto rejection for {op_name} path sink {sink}"
                );
                assert_eq!(errors[0].code, code);
            }
        }
    }

    #[test]
    fn validates_set_path_segments_with_op_specific_error_code() {
        let (_value, errors) = apply_update_ops(
            Some(json!({})),
            &[UpdateOp::Set {
                path: "__proto__".into(),
                value: Some(json!(true)),
            }],
        );

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "set.path.proto_polluted");
    }

    #[test]
    fn validates_append_path_segments_with_op_specific_error_code() {
        let oversized = "a".repeat(super::MAX_SEGMENT_BYTES + 1);
        let (_value, errors) =
            apply_update_ops(Some(json!({})), &[UpdateOp::append(oversized, json!("x"))]);

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "append.path.segment_too_long");
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

    // ───────────────────────── nested-path append (issue #1552) ─────────────────────────

    #[test]
    fn append_segments_path_walks_and_pushes_to_existing_array() {
        // UC-1: the new happy path. `Segments(["entityId","buffer"])`
        // walks the parent and pushes onto the existing array.
        let updated = run(
            Some(json!({ "entityId": { "buffer": ["a"] } })),
            &[UpdateOp::append_at_path(["entityId", "buffer"], json!("b"))],
        );

        assert_eq!(updated, json!({ "entityId": { "buffer": ["a", "b"] } }));
    }

    #[test]
    fn append_segments_path_creates_intermediate_object() {
        // FR-3: walk_or_create auto-creates missing intermediate objects.
        let updated = run(
            Some(json!({})),
            &[UpdateOp::append_at_path(["entityId", "buffer"], json!("a"))],
        );

        assert_eq!(updated, json!({ "entityId": { "buffer": ["a"] } }));
    }

    #[test]
    fn append_segments_missing_leaf_string_value_becomes_array() {
        // FR-11 + B1 carve-out + UC-5: nested-path missing leaf is
        // ALWAYS an array, even when the value is a string. This
        // diverges from single-path's `initial_append_value` which
        // would keep the string as a string for the concat tier.
        let updated = run(
            Some(json!({ "entityId": {} })),
            &[UpdateOp::append_at_path(["entityId", "buffer"], json!("x"))],
        );

        // Compare with the single-path equivalent which keeps "x" as a string.
        let single = run(Some(json!({})), &[UpdateOp::append("buffer", json!("x"))]);

        assert_eq!(updated, json!({ "entityId": { "buffer": ["x"] } }));
        assert_eq!(single, json!({ "buffer": "x" })); // back-compat preserved
    }

    #[test]
    fn append_dotted_string_path_keeps_literal_segment_semantics() {
        // UC-3: `Single("entityId.buffer")` is ONE literal key, never
        // dot-traversed. Mirrors iii's existing FieldPath contract.
        let updated = run(
            Some(json!({ "entityId": { "buffer": ["nested"] } })),
            &[UpdateOp::append("entityId.buffer", json!("flat"))],
        );

        // The dotted-string append targets the literal "entityId.buffer"
        // key at root, NOT the nested array. The nested array is
        // untouched; a new top-level key is created.
        assert_eq!(
            updated,
            json!({
                "entityId": { "buffer": ["nested"] },
                "entityId.buffer": "flat",
            })
        );
    }

    #[test]
    fn append_segments_path_object_leaf_returns_type_mismatch() {
        // UC-6 + Case-2 transition: existing object at the leaf can't
        // accept an append. Previously this silently no-op'd at the
        // single-path level; now nested append produces a structured
        // error and leaves state unchanged.
        let (updated, errors) = apply_update_ops(
            Some(json!({ "entityId": { "buffer": { "x": "y" } } })),
            &[UpdateOp::append_at_path(["entityId", "buffer"], json!("z"))],
        );

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, super::ERR_APPEND_TYPE_MISMATCH);
        // State unchanged: the object at the leaf was not mutated.
        assert_eq!(updated, json!({ "entityId": { "buffer": { "x": "y" } } }));
    }

    #[test]
    fn append_segments_path_non_object_root_returns_target_not_object() {
        // IMP-003 root-not-object: validation order must check root
        // is an object BEFORE walk_or_create can mutate it. With a
        // top-level array as state, append["a","b"] should reject.
        let (updated, errors) = apply_update_ops(
            Some(json!([1, 2, 3])),
            &[UpdateOp::append_at_path(["a", "b"], json!("x"))],
        );

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, super::ERR_APPEND_TARGET_NOT_OBJECT);
        // State unchanged: walk_or_create did NOT replace the array root.
        assert_eq!(updated, json!([1, 2, 3]));
    }

    // ───────────────────── proto-pollution at root/intermediate/leaf (B3) ─────────────────────

    #[test]
    fn append_proto_polluted_at_root_segment_rejected() {
        let (updated, errors) = apply_update_ops(
            Some(json!({})),
            &[UpdateOp::append_at_path(["__proto__"], json!("x"))],
        );

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "append.path.proto_polluted");
        assert_eq!(updated, json!({}));
    }

    #[test]
    fn append_proto_polluted_at_intermediate_segment_rejected() {
        let (updated, errors) = apply_update_ops(
            Some(json!({})),
            &[UpdateOp::append_at_path(
                ["a", "constructor", "b"],
                json!("x"),
            )],
        );

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "append.path.proto_polluted");
        // State must be unchanged — validation runs BEFORE walk_or_create
        // can create intermediate objects (validate-before-mutate audit).
        assert_eq!(updated, json!({}));
    }

    #[test]
    fn append_proto_polluted_at_leaf_segment_rejected() {
        let (updated, errors) = apply_update_ops(
            Some(json!({ "safe": {} })),
            &[UpdateOp::append_at_path(["safe", "prototype"], json!("x"))],
        );

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "append.path.proto_polluted");
        assert_eq!(updated, json!({ "safe": {} }));
    }

    // ───────────────────────── multi-op batch semantics (FR-13) ─────────────────────────

    #[test]
    fn multi_op_partial_failure_retains_prior_successes() {
        // FR-13: skip-failed semantics. op-0 succeeds, op-1 fails on
        // proto-pollution, op-2 succeeds. Both successful ops are
        // reflected in new_value; op-1's error is in errors[] with the
        // correct original-array op_index (1, not the post-skip 0).
        let (updated, errors) = apply_update_ops(
            Some(json!({ "buffer": [] })),
            &[
                UpdateOp::append("buffer", json!("first")),
                UpdateOp::append_at_path(["__proto__", "polluted"], json!(true)),
                UpdateOp::append("buffer", json!("third")),
            ],
        );

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].op_index, 1);
        assert_eq!(errors[0].code, "append.path.proto_polluted");
        // op-0 and op-2 effects retained, op-1 skipped.
        assert_eq!(updated, json!({ "buffer": ["first", "third"] }));
    }
}
