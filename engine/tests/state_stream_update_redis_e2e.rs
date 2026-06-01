// Redis-backed parity lane for `state::update` / `stream::update`.
//
// Sister to `state_stream_update_e2e.rs`, which exercises the
// in-memory `BuiltinKvStore` (Rust `apply_update_ops` path). This file
// drives the same scenarios through `StateRedisAdapter`, exercising
// the embedded Lua script in `engine/src/workers/redis.rs`. The two
// paths must produce identical results — the empty-table cjson
// roundtrip ambiguity, array-as-intermediate handling, and root-shape
// rules are the load-bearing parity points reviewed in PR #1612.
//
// Gating: tests skip cleanly when no Redis is reachable. Set
// `TEST_REDIS_URL` (default `redis://localhost:6379`) to opt in.
//
//   docker run -d --rm -p 6379:6379 redis:7-alpine
//   TEST_REDIS_URL=redis://localhost:6379 \
//     cargo test -p iii --test state_stream_update_redis_e2e \
//     -- --include-ignored

use serde_json::json;

use iii::workers::state::adapters::{StateAdapter, redis_adapter::StateRedisAdapter};
use iii_sdk::UpdateOp;

const SCOPE: &str = "audio::transcripts";

fn redis_url() -> String {
    std::env::var("TEST_REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string())
}

async fn try_adapter() -> Option<StateRedisAdapter> {
    StateRedisAdapter::new(redis_url()).await.ok()
}

async fn fresh_key(adapter: &StateRedisAdapter, key: &str) {
    let _ = adapter.delete(SCOPE, key).await;
}

// Thin wrapper that mimics the `set_op` helper from the in-memory
// e2e file. Building the op via the SDK keeps the wire shape under
// test (root issue from #1612 was a wire-format divergence).
fn set_op(path: &str, value: serde_json::Value) -> UpdateOp {
    UpdateOp::Set {
        path: iii_sdk::FieldPath::from(path),
        value: Some(value),
    }
}

#[tokio::test]
#[ignore = "redis-parity: requires reachable Redis at TEST_REDIS_URL (default redis://localhost:6379)"]
async fn redis_append_existing_empty_array_at_single_segment_pushes_value() {
    // Scenario A: `{"buffer": []}` + `append("buffer", "x")`.
    let Some(adapter) = try_adapter().await else {
        eprintln!("[skip] redis-parity: cannot reach {}", redis_url());
        return;
    };
    let key = "redis-empty-leaf";
    fresh_key(&adapter, key).await;

    adapter
        .update(SCOPE, key, vec![set_op("buffer", json!([]))])
        .await
        .unwrap();

    let result = adapter
        .update(SCOPE, key, vec![UpdateOp::append("buffer", json!("x"))])
        .await
        .unwrap();

    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.new_value, json!({ "buffer": ["x"] }));

    fresh_key(&adapter, key).await;
}

#[tokio::test]
#[ignore = "redis-parity: requires reachable Redis at TEST_REDIS_URL"]
async fn redis_append_root_when_state_is_empty_array_pushes_value() {
    // Scenario B: state at the key is `[]`, root append `"x"` → `["x"]`.
    let Some(adapter) = try_adapter().await else {
        eprintln!("[skip] redis-parity: cannot reach {}", redis_url());
        return;
    };
    let key = "redis-empty-root-array";
    fresh_key(&adapter, key).await;

    adapter.set(SCOPE, key, json!([])).await.unwrap();

    let result = adapter
        .update(SCOPE, key, vec![UpdateOp::append_root(json!("x"))])
        .await
        .unwrap();

    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.new_value, json!(["x"]));

    fresh_key(&adapter, key).await;
}

#[tokio::test]
#[ignore = "redis-parity: requires reachable Redis at TEST_REDIS_URL"]
async fn redis_nested_append_replaces_array_intermediate_with_object() {
    // Scenario C: `{"a": [1,2,3]}` + nested append `["a","b"]` 42.
    // The Lua walk_or_create must replace the array intermediate with
    // a fresh object — without this fix, Lua walks into the array and
    // produces the corrupted mixed-key form `{"a": {"1":1,"2":2,"3":3,"b":[42]}}`
    // that ytallo specifically forbade.
    let Some(adapter) = try_adapter().await else {
        eprintln!("[skip] redis-parity: cannot reach {}", redis_url());
        return;
    };
    let key = "redis-array-intermediate";
    fresh_key(&adapter, key).await;

    adapter
        .update(SCOPE, key, vec![set_op("a", json!([1, 2, 3]))])
        .await
        .unwrap();

    let result = adapter
        .update(
            SCOPE,
            key,
            vec![UpdateOp::append_at_path(["a", "b"], json!(42))],
        )
        .await
        .unwrap();

    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.new_value, json!({ "a": { "b": [42] } }));
    // Negative assertion the gherkin specifically calls out.
    assert_ne!(
        result.new_value,
        json!({ "a": { "1": 1, "2": 2, "3": 3, "b": [42] } })
    );

    fresh_key(&adapter, key).await;
}

#[tokio::test]
#[ignore = "redis-parity: requires reachable Redis at TEST_REDIS_URL"]
async fn redis_nested_append_happy_path_through_lua_script() {
    // Scenario D: no state at the key, nested append at
    // `["session","abc","events"]` value `"first"`. Validates the
    // empty-document path through the Lua IMP-003 root gate
    // (`is_object_shape` admits empty Lua tables) and the
    // walk_or_create + missing-leaf-as-array tier.
    let Some(adapter) = try_adapter().await else {
        eprintln!("[skip] redis-parity: cannot reach {}", redis_url());
        return;
    };
    let key = "redis-nested-create";
    fresh_key(&adapter, key).await;

    let result = adapter
        .update(
            SCOPE,
            key,
            vec![UpdateOp::append_at_path(
                ["session", "abc", "events"],
                json!("first"),
            )],
        )
        .await
        .unwrap();

    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(
        result.new_value,
        json!({ "session": { "abc": { "events": ["first"] } } })
    );

    fresh_key(&adapter, key).await;
}
