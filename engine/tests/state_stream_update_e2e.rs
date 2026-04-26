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
use iii_sdk::{UpdateOp, types::MergePath};

const SCOPE: &str = "audio::transcripts";

async fn fresh_store() -> BuiltinKvStore {
    BuiltinKvStore::new(None)
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
