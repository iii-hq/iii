// Integration tests for nested merge in `state::update` (and the
// shared `apply_update_ops` machinery used by `stream::update`).
//
// Closes iii-hq/iii#1546. Each test here exercises behavior that
// requires going through `BuiltinKvStore` (persistence across calls,
// op-batch interleaving, error plumbing into `UpdateResult`). The
// in-batch nested-merge mechanics — auto-create intermediates, replace
// non-object intermediates, validation rejections — are covered by
// the unit tests in `engine/src/update_ops.rs`.

use serde_json::json;

use iii::builtins::kv::BuiltinKvStore;
use iii_sdk::{UpdateOp, UpdateOpError, types::MergePath};

const SCOPE: &str = "audio::transcripts";

async fn fresh_store() -> BuiltinKvStore {
    BuiltinKvStore::new(None)
}

fn set_op(path: impl Into<iii_sdk::FieldPath>, value: serde_json::Value) -> UpdateOp {
    UpdateOp::Set {
        path: path.into(),
        value: Some(value),
    }
}

fn assert_structured_error(errors: &[UpdateOpError], op_index: usize, code: &str, path_text: &str) {
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, code);
    assert_eq!(errors[0].op_index, op_index);
    assert!(errors[0].message.contains(path_text));
    assert!(errors[0].doc_url.is_some());
}

#[tokio::test]
async fn merge_first_level_path_accumulates_siblings_across_calls() {
    // Issue #1546 case 1: two merges into the same first-level field
    // accumulate timestamps without nuking siblings.
    let store = fresh_store().await;
    let key = "session-abc".to_string();

    let r1 = store
        .update(
            SCOPE.to_string(),
            key.clone(),
            vec![UpdateOp::merge_at(
                "session-1",
                json!({ "ts:0001": "first chunk" }),
            )],
        )
        .await;
    assert!(r1.errors.is_empty(), "first merge errors: {:?}", r1.errors);

    let r2 = store
        .update(
            SCOPE.to_string(),
            key.clone(),
            vec![UpdateOp::merge_at(
                "session-1",
                json!({ "ts:0002": "second chunk" }),
            )],
        )
        .await;
    assert!(r2.errors.is_empty(), "second merge errors: {:?}", r2.errors);

    assert_eq!(
        r2.new_value,
        json!({
            "session-1": {
                "ts:0001": "first chunk",
                "ts:0002": "second chunk",
            }
        })
    );
}

#[tokio::test]
async fn merge_replaces_null_intermediate_along_nested_path() {
    // Hostile-target case: an existing `null` intermediate must be
    // replaced by `{}` and the merge proceed. Previously this would
    // silently no-op (Codex's "non-object blocks merge forever"
    // finding).
    let store = fresh_store().await;
    let key = "key".to_string();

    // Seed `sessions: null` via a set op.
    store
        .update(
            SCOPE.to_string(),
            key.clone(),
            vec![UpdateOp::Set {
                path: "sessions".into(),
                value: Some(serde_json::Value::Null),
            }],
        )
        .await;

    let result = store
        .update(
            SCOPE.to_string(),
            key.clone(),
            vec![UpdateOp::merge_at(
                MergePath::Segments(vec!["sessions".into(), "abc".into()]),
                json!({ "author": "alice" }),
            )],
        )
        .await;

    assert!(result.errors.is_empty());
    assert_eq!(
        result.new_value,
        json!({
            "sessions": {
                "abc": { "author": "alice" }
            }
        })
    );
}

#[tokio::test]
async fn merge_then_remove_then_merge_recreates_field_cleanly() {
    let store = fresh_store().await;
    let key = "key".to_string();

    let r = store
        .update(
            SCOPE.to_string(),
            key.clone(),
            vec![
                UpdateOp::merge_at("session-1", json!({ "a": 1 })),
                UpdateOp::Remove {
                    path: "session-1".into(),
                },
                UpdateOp::merge_at("session-1", json!({ "b": 2 })),
            ],
        )
        .await;

    assert!(r.errors.is_empty());
    assert_eq!(r.new_value, json!({ "session-1": { "b": 2 } }));
}

#[tokio::test]
async fn merge_with_proto_polluted_segment_returns_structured_error() {
    let store = fresh_store().await;
    let key = "key".to_string();

    let r = store
        .update(
            SCOPE.to_string(),
            key.clone(),
            vec![UpdateOp::merge_at(
                MergePath::Segments(vec!["__proto__".into(), "polluted".into()]),
                json!({ "x": 1 }),
            )],
        )
        .await;

    assert_eq!(r.errors.len(), 1);
    assert_eq!(r.errors[0].code, "merge.path.proto_polluted");
    assert_eq!(r.errors[0].op_index, 0);
    assert!(r.errors[0].doc_url.is_some());
    // The op did not apply.
    assert_eq!(r.new_value, json!({}));
}

#[tokio::test]
async fn non_merge_ops_with_proto_polluted_path_return_structured_errors() {
    let cases: Vec<(&str, UpdateOp)> = vec![
        ("set", set_op("__proto__", json!(1))),
        ("append", UpdateOp::append("__proto__", json!(1))),
        ("increment", UpdateOp::increment("__proto__", 1)),
        ("decrement", UpdateOp::decrement("__proto__", 1)),
        (
            "remove",
            UpdateOp::Remove {
                path: "__proto__".into(),
            },
        ),
    ];

    for (op_name, op) in cases {
        let store = fresh_store().await;
        let result = store
            .update(SCOPE.to_string(), format!("proto-{op_name}"), vec![op])
            .await;

        assert_structured_error(
            &result.errors,
            0,
            &format!("{op_name}.path.proto_polluted"),
            "__proto__",
        );
        assert_eq!(result.new_value, json!({}), "op {op_name} should not apply");
    }
}

#[tokio::test]
async fn set_on_non_object_target_returns_structured_error_and_skips() {
    let store = fresh_store().await;
    let key = "set-target".to_string();

    store
        .update(
            SCOPE.to_string(),
            key.clone(),
            vec![set_op("", json!("leaf"))],
        )
        .await;

    let result = store
        .update(SCOPE.to_string(), key, vec![set_op("field", json!(1))])
        .await;

    assert_structured_error(&result.errors, 0, "set.target_not_object", "field");
    assert_eq!(result.new_value, json!("leaf"));
}

#[tokio::test]
async fn append_type_mismatch_returns_structured_error_and_skips() {
    let store = fresh_store().await;
    let key = "append-type".to_string();

    store
        .update(
            SCOPE.to_string(),
            key.clone(),
            vec![set_op("count", json!(1))],
        )
        .await;

    let result = store
        .update(
            SCOPE.to_string(),
            key,
            vec![UpdateOp::append("count", json!("chunk"))],
        )
        .await;

    assert_structured_error(&result.errors, 0, "append.type_mismatch", "count");
    assert_eq!(result.new_value, json!({ "count": 1 }));
}

#[tokio::test]
async fn append_on_non_object_target_returns_structured_error_and_skips() {
    let store = fresh_store().await;
    let key = "append-target".to_string();

    store
        .update(
            SCOPE.to_string(),
            key.clone(),
            vec![set_op("", json!("leaf"))],
        )
        .await;

    let result = store
        .update(
            SCOPE.to_string(),
            key,
            vec![UpdateOp::append("events", json!("chunk"))],
        )
        .await;

    assert_structured_error(&result.errors, 0, "append.target_not_object", "events");
    assert_eq!(result.new_value, json!("leaf"));
}

#[tokio::test]
async fn failed_update_ops_continue_and_report_original_indexes() {
    let store = fresh_store().await;
    let key = "partial-errors".to_string();

    store
        .update(
            SCOPE.to_string(),
            key.clone(),
            vec![set_op("", json!({ "bad": "value", "events": {} }))],
        )
        .await;

    let result = store
        .update(
            SCOPE.to_string(),
            key,
            vec![
                UpdateOp::increment("bad", 1),
                set_op("__proto__", json!(true)),
                UpdateOp::append("events", json!("chunk")),
                set_op("ok", json!(true)),
            ],
        )
        .await;

    assert_eq!(
        result.new_value,
        json!({ "bad": "value", "events": {}, "ok": true })
    );
    assert_eq!(result.errors.len(), 3);
    assert_eq!(result.errors[0].op_index, 0);
    assert_eq!(result.errors[0].code, "increment.not_number");
    assert_eq!(result.errors[1].op_index, 1);
    assert_eq!(result.errors[1].code, "set.path.proto_polluted");
    assert_eq!(result.errors[2].op_index, 2);
    assert_eq!(result.errors[2].code, "append.type_mismatch");
}

#[tokio::test]
async fn increment_non_number_returns_structured_error_and_skips() {
    let store = fresh_store().await;
    let key = "increment-number".to_string();

    store
        .update(
            SCOPE.to_string(),
            key.clone(),
            vec![set_op("name", json!("Ada"))],
        )
        .await;

    let result = store
        .update(SCOPE.to_string(), key, vec![UpdateOp::increment("name", 1)])
        .await;

    assert_structured_error(&result.errors, 0, "increment.not_number", "name");
    assert_eq!(result.new_value, json!({ "name": "Ada" }));
}

#[tokio::test]
async fn decrement_non_number_returns_structured_error_and_skips() {
    let store = fresh_store().await;
    let key = "decrement-number".to_string();

    store
        .update(
            SCOPE.to_string(),
            key.clone(),
            vec![set_op("name", json!("Ada"))],
        )
        .await;

    let result = store
        .update(SCOPE.to_string(), key, vec![UpdateOp::decrement("name", 1)])
        .await;

    assert_structured_error(&result.errors, 0, "decrement.not_number", "name");
    assert_eq!(result.new_value, json!({ "name": "Ada" }));
}

#[tokio::test]
async fn numeric_ops_and_remove_on_non_object_target_return_structured_errors() {
    let cases: Vec<(&str, UpdateOp)> = vec![
        ("increment", UpdateOp::increment("count", 1)),
        ("decrement", UpdateOp::decrement("count", 1)),
        (
            "remove",
            UpdateOp::Remove {
                path: "count".into(),
            },
        ),
    ];

    for (op_name, op) in cases {
        let store = fresh_store().await;
        let key = format!("{op_name}-target");

        store
            .update(
                SCOPE.to_string(),
                key.clone(),
                vec![set_op("", json!("leaf"))],
            )
            .await;

        let result = store.update(SCOPE.to_string(), key, vec![op]).await;

        assert_structured_error(
            &result.errors,
            0,
            &format!("{op_name}.target_not_object"),
            "count",
        );
        assert_eq!(result.new_value, json!("leaf"));
    }
}

#[tokio::test]
async fn remove_missing_path_remains_idempotent_and_silent() {
    let store = fresh_store().await;
    let key = "remove-missing".to_string();

    let result = store
        .update(
            SCOPE.to_string(),
            key,
            vec![UpdateOp::Remove {
                path: "missing".into(),
            }],
        )
        .await;

    assert!(result.errors.is_empty());
    assert_eq!(result.new_value, json!({}));
}
