use std::time::Duration;

use serde_json::json;

use iii::{EngineBuilder, engine::EngineTrait, workers::telemetry::is_iii_builtin_function_id};

async fn boot_bare_engine() -> iii::EngineBuilder {
    EngineBuilder::new()
        .add_worker("iii-engine-functions", None)
        .add_worker("iii-state", None)
        .add_worker("iii-stream", Some(json!({ "port": 0 })))
        .add_worker("iii-queue", None)
        .add_worker("iii-pubsub", None)
        .add_worker("iii-observability", None)
        .add_worker("iii-http", Some(json!({ "port": 0 })))
        .add_worker("iii-worker-manager", Some(json!({ "port": 0 })))
        .build()
        .await
        .expect("engine build should succeed")
}

/// Boots the engine with all default modules (ephemeral ports to avoid
/// conflicts), calls `engine::functions::list` with `include_internal: true`,
/// and asserts every returned function_id is classified as an iii builtin.
#[tokio::test]
async fn all_functions_on_bare_engine_are_iii_builtins() {
    let builder = boot_bare_engine().await;
    let engine = builder.engine();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let result = engine
        .call(
            "engine::functions::list",
            json!({ "include_internal": true }),
        )
        .await
        .expect("engine::functions::list should succeed");

    let functions = result
        .expect("response should not be None")
        .get("functions")
        .expect("response should have 'functions' key")
        .as_array()
        .expect("'functions' should be an array")
        .clone();

    assert!(
        !functions.is_empty(),
        "engine should have at least one registered function"
    );

    let mut non_builtins = Vec::new();
    for func in &functions {
        let id = func
            .get("function_id")
            .and_then(|v| v.as_str())
            .expect("each function should have a function_id string");

        if !is_iii_builtin_function_id(id) {
            non_builtins.push(id.to_string());
        }
    }

    assert!(
        non_builtins.is_empty(),
        "bare engine should have zero non-builtin functions, but found: {:?}",
        non_builtins
    );

    // Every row must also carry a resolved worker_name (or a fallback to the
    // first `::` segment of the function_id).
    for func in &functions {
        let worker_name = func
            .get("worker_name")
            .and_then(|v| v.as_str())
            .expect("each function summary should expose worker_name");
        assert!(
            !worker_name.is_empty(),
            "worker_name must be non-empty for {}",
            func.get("function_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
        );
    }
}

/// Smoke-test the new `engine::functions::info` builtin against a known engine
/// function. The response must include the schemas under their new
/// `request_schema` / `response_schema` keys.
#[tokio::test]
async fn functions_info_returns_schemas_for_engine_builtin() {
    let builder = boot_bare_engine().await;
    let engine = builder.engine();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let result = engine
        .call(
            "engine::functions::info",
            json!({ "function_id": "engine::functions::list" }),
        )
        .await
        .expect("engine::functions::info should succeed")
        .expect("response should not be None");

    assert_eq!(
        result.get("function_id").and_then(|v| v.as_str()),
        Some("engine::functions::list")
    );
    assert!(
        result.get("worker_name").and_then(|v| v.as_str()).is_some(),
        "functions::info must include worker_name"
    );
    assert!(
        result.get("request_schema").is_some(),
        "functions::info must use the renamed `request_schema` field"
    );
    assert!(
        result.get("response_schema").is_some(),
        "functions::info must use the renamed `response_schema` field"
    );
    assert!(
        result
            .get("registered_triggers")
            .and_then(|v| v.as_array())
            .is_some(),
        "functions::info must include the registered_triggers array"
    );
}

/// `engine::triggers::list` now returns trigger TYPES (templates), not
/// instances. Every row must carry an `id`, `worker_name`, and `description`.
#[tokio::test]
async fn triggers_list_returns_trigger_types() {
    let builder = boot_bare_engine().await;
    let engine = builder.engine();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let result = engine
        .call(
            "engine::triggers::list",
            json!({ "include_internal": true }),
        )
        .await
        .expect("engine::triggers::list should succeed")
        .expect("response should not be None");

    let triggers = result
        .get("triggers")
        .and_then(|v| v.as_array())
        .expect("triggers array");

    assert!(!triggers.is_empty(), "expected at least one trigger type");

    for trigger in triggers {
        let id = trigger
            .get("id")
            .and_then(|v| v.as_str())
            .expect("trigger row must have id");
        assert!(!id.is_empty(), "trigger id must not be empty");
        trigger
            .get("worker_name")
            .and_then(|v| v.as_str())
            .expect("trigger row must have worker_name");
        trigger
            .get("description")
            .and_then(|v| v.as_str())
            .expect("trigger row must have description");
        // Schemas live on info, never on list.
        assert!(trigger.get("configuration_schema").is_none());
        assert!(trigger.get("request_schema").is_none());
    }
}

/// `engine::registered-triggers::list` lists subscriber rows. On a bare
/// engine without any user subscriptions there are no internal rows either
/// (the engine's own triggers run through the in-process registrator, not
/// through the trigger registry), but the call must succeed and return the
/// canonical `{ registered_triggers: [] }` envelope.
#[tokio::test]
async fn registered_triggers_list_returns_canonical_envelope() {
    let builder = boot_bare_engine().await;
    let engine = builder.engine();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let result = engine
        .call(
            "engine::registered-triggers::list",
            json!({ "include_internal": true }),
        )
        .await
        .expect("engine::registered-triggers::list should succeed")
        .expect("response should not be None");

    let rows = result
        .get("registered_triggers")
        .and_then(|v| v.as_array())
        .expect("registered_triggers array");

    for row in rows {
        row.get("id").and_then(|v| v.as_str()).expect("row id");
        row.get("trigger_type")
            .and_then(|v| v.as_str())
            .expect("row trigger_type");
        row.get("function_id")
            .and_then(|v| v.as_str())
            .expect("row function_id");
        row.get("worker_name")
            .and_then(|v| v.as_str())
            .expect("row worker_name");
        row.get("config_summary")
            .and_then(|v| v.as_str())
            .expect("row config_summary");
    }
}

/// `engine::workers::info` exposes the local-only `pid`, `internal`, and
/// (optional) `latest_metrics` extras alongside the shared core envelope.
/// Looking up a known in-process runtime worker by `name` must succeed.
#[tokio::test]
async fn workers_info_returns_full_surface_for_runtime_worker() {
    let builder = boot_bare_engine().await;
    let engine = builder.engine();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let result = engine
        .call("engine::workers::info", json!({ "name": "iii-state" }))
        .await
        .expect("engine::workers::info should succeed")
        .expect("response should not be None");

    let worker = result.get("worker").expect("worker envelope");
    assert_eq!(
        worker.get("name").and_then(|v| v.as_str()),
        Some("iii-state")
    );
    // description is shared core: builtin workers self-describe via their
    // WorkerRegistration.
    assert_eq!(
        worker.get("description").and_then(|v| v.as_str()),
        Some("Distributed key-value state management with reactive change triggers.")
    );
    // internal is always present on the info envelope.
    assert!(worker.get("internal").and_then(|v| v.as_bool()).is_some());

    let functions = result
        .get("functions")
        .and_then(|v| v.as_array())
        .expect("functions array");
    assert!(!functions.is_empty(), "iii-state should expose functions");
}

/// In-process workers register `TriggerType` with `worker_id: None`, so the
/// `worker_registry` Uuid lookup used for WebSocket workers cannot attribute
/// them. `engine::workers::info` must still roll trigger types up into the
/// owning runtime worker via the static `BUILTIN_TRIGGER_TYPES` map —
/// otherwise the publish workflow ships an empty `triggers: []` array to the
/// registry for iii-http, iii-cron, iii-state, etc.
#[tokio::test]
async fn workers_info_attributes_trigger_types_to_in_process_worker() {
    let builder = boot_bare_engine().await;
    let engine = builder.engine();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let result = engine
        .call("engine::workers::info", json!({ "name": "iii-http" }))
        .await
        .expect("engine::workers::info should succeed")
        .expect("response should not be None");

    let trigger_types = result
        .get("trigger_types")
        .and_then(|v| v.as_array())
        .expect("trigger_types array");

    let ids: Vec<&str> = trigger_types
        .iter()
        .filter_map(|tt| tt.get("id").and_then(|v| v.as_str()))
        .collect();
    assert!(
        ids.contains(&"http"),
        "iii-http workers::info should include the `http` trigger type, got {:?}",
        ids
    );

    for tt in trigger_types {
        assert_eq!(
            tt.get("worker_name").and_then(|v| v.as_str()),
            Some("iii-http"),
            "trigger_type {:?} should be attributed to iii-http",
            tt.get("id")
        );
    }
}
