use std::time::Duration;

use serde_json::json;

use iii::{
    engine::EngineTrait,
    workers::telemetry::is_iii_builtin_function_id,
    EngineBuilder,
};

/// Boots the engine with all default modules (ephemeral ports to avoid
/// conflicts), calls `engine::functions::list` with `include_internal: true`,
/// and asserts every returned function_id is classified as an iii builtin.
#[tokio::test]
async fn all_functions_on_bare_engine_are_iii_builtins() {
    let builder = EngineBuilder::new()
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
        .expect("engine build should succeed");

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
}
