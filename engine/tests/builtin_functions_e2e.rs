use std::time::Duration;

use serde_json::json;

use iii::{EngineBuilder, engine::EngineTrait, workers::telemetry::is_iii_builtin_function_id};

/// The mandatory `configuration` worker persists entries on boot; point its
/// store at a tempdir so the tests write nothing into the repo-relative
/// `engine/config/` and leak nothing between runs. The returned `TempDir`
/// must stay alive for the test's duration.
async fn boot_bare_engine() -> (iii::EngineBuilder, tempfile::TempDir) {
    let config_dir = tempfile::tempdir().expect("create config tempdir");
    let builder = EngineBuilder::new()
        .add_worker(
            "configuration",
            Some(json!({
                "adapter": {
                    "name": "fs",
                    "config": { "directory": config_dir.path().to_string_lossy() }
                }
            })),
        )
        .add_worker("iii-stream", Some(json!({ "port": 0 })))
        .add_worker("iii-worker-manager", Some(json!({ "port": 0 })))
        .build()
        .await
        .expect("engine build should succeed");
    (builder, config_dir)
}

/// Boots the engine with all default modules (ephemeral ports to avoid
/// conflicts), calls `engine::functions::list` with `include_internal: true`,
/// and asserts every returned function_id is classified as an iii builtin.
#[tokio::test]
async fn all_functions_on_bare_engine_are_iii_builtins() {
    let (builder, _config_dir) = boot_bare_engine().await;
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

/// The trigger builtins are removed (decision 6): a booted engine registers
/// none of them, so `engine::functions::list` does not list them and a call of
/// one is `function_not_found`.
#[tokio::test]
async fn the_trigger_builtins_are_not_registered() {
    const REMOVED: [&str; 6] = [
        "engine::register_trigger",
        "engine::unregister_trigger",
        "engine::triggers::list",
        "engine::triggers::info",
        "engine::registered-triggers::list",
        "engine::registered-triggers::info",
    ];
    let (builder, _config_dir) = boot_bare_engine().await;
    let engine = builder.engine();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let listed = engine
        .call(
            "engine::functions::list",
            json!({ "include_internal": true }),
        )
        .await
        .expect("engine::functions::list should succeed")
        .expect("response should not be None");
    let ids: Vec<&str> = listed["functions"]
        .as_array()
        .expect("'functions' should be an array")
        .iter()
        .filter_map(|function| function["function_id"].as_str())
        .collect();
    assert!(
        ids.contains(&"engine::observers::list"),
        "the observer builtins replace them: {ids:?}"
    );
    for id in REMOVED {
        assert!(!ids.contains(&id), "{id} is listed");
        let error = engine
            .call(id, json!({}))
            .await
            .expect_err("no such function");
        assert_eq!(error.code, "function_not_found", "{id}: {error:?}");
    }
}

/// Smoke-test the new `engine::functions::info` builtin against a known engine
/// function. The response must include the schemas under their new
/// `request_schema` / `response_schema` keys.
#[tokio::test]
async fn functions_info_returns_schemas_for_engine_builtin() {
    let (builder, _config_dir) = boot_bare_engine().await;
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
            .get("observer_count")
            .and_then(|v| v.as_u64())
            .is_some(),
        "functions::info must include the observer_count"
    );
}

/// `engine::functions::info { function_ids: [...] }` fetches several
/// contracts in ONE call: full `FunctionDetail` rows for known ids, a
/// `{ function_id, error: "not_found" }` marker for unknown ones — in
/// request order, without failing the batch.
#[tokio::test]
async fn functions_info_batch_returns_details_and_markers() {
    let (builder, _config_dir) = boot_bare_engine().await;
    let engine = builder.engine();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let result = engine
        .call(
            "engine::functions::info",
            json!({ "function_ids": [
                "engine::functions::list",
                "does::not::exist",
                "engine::functions::info",
            ] }),
        )
        .await
        .expect("batch engine::functions::info should succeed")
        .expect("response should not be None");

    let functions = result
        .get("functions")
        .and_then(|v| v.as_array())
        .expect("batch response must carry a functions array");
    assert_eq!(functions.len(), 3);

    assert_eq!(
        functions[0].get("function_id").and_then(|v| v.as_str()),
        Some("engine::functions::list")
    );
    assert!(
        functions[0].get("request_schema").is_some(),
        "batch entries must carry the full FunctionDetail"
    );

    assert_eq!(
        functions[1].get("function_id").and_then(|v| v.as_str()),
        Some("does::not::exist")
    );
    assert_eq!(
        functions[1].get("error").and_then(|v| v.as_str()),
        Some("not_found")
    );

    assert_eq!(
        functions[2].get("function_id").and_then(|v| v.as_str()),
        Some("engine::functions::info")
    );
    assert!(functions[2].get("error").is_none());
}

/// `engine::workers::info` exposes the local-only `pid`, `internal`, and
/// (optional) `latest_metrics` extras alongside the shared core envelope.
/// Looking up a known in-process runtime worker by `name` must succeed.
#[tokio::test]
async fn workers_info_returns_full_surface_for_runtime_worker() {
    let (builder, _config_dir) = boot_bare_engine().await;
    let engine = builder.engine();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let result = engine
        .call("engine::workers::info", json!({ "name": "iii-stream" }))
        .await
        .expect("engine::workers::info should succeed")
        .expect("response should not be None");

    let worker = result.get("worker").expect("worker envelope");
    assert_eq!(
        worker.get("name").and_then(|v| v.as_str()),
        Some("iii-stream")
    );
    // description is shared core: builtin workers self-describe via their
    // WorkerRegistration.
    assert_eq!(
        worker.get("description").and_then(|v| v.as_str()),
        Some("Build durable streams for real-time data subscriptions.")
    );
    // internal is always present on the info envelope.
    assert!(worker.get("internal").and_then(|v| v.as_bool()).is_some());

    let functions = result
        .get("functions")
        .and_then(|v| v.as_array())
        .expect("functions array");
    assert!(!functions.is_empty(), "iii-stream should expose functions");
}

/// `engine::workers::info` lists a worker's functions and the observers it
/// created; the engine's own workers create none.
#[tokio::test]
async fn workers_info_lists_functions_and_observers() {
    let (builder, _config_dir) = boot_bare_engine().await;
    let engine = builder.engine();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let result = engine
        .call("engine::workers::info", json!({ "name": "iii-stream" }))
        .await
        .expect("engine::workers::info should succeed")
        .expect("response should not be None");

    let observers = result
        .get("observers")
        .and_then(|v| v.as_array())
        .expect("observers array");
    assert!(observers.is_empty());
}

/// Each builtin worker hosts the engine-hosted sources it registers
/// (decision 7): `engine::workers::info` lists them among its functions, and
/// `engine::functions::list` names it as their worker.
#[tokio::test]
async fn engine_hosted_sources_belong_to_their_builtin_worker() {
    let (builder, _config_dir) = boot_bare_engine().await;
    let engine = builder.engine();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let workers = engine
        .call("engine::workers::list", json!({ "include_internal": true }))
        .await
        .expect("engine::workers::list should succeed")
        .expect("response should not be None");
    let names: Vec<String> = workers["workers"]
        .as_array()
        .expect("'workers' should be an array")
        .iter()
        .filter_map(|worker| worker["name"].as_str().map(str::to_string))
        .collect();
    let listed = engine
        .call(
            "engine::functions::list",
            json!({ "include_internal": true }),
        )
        .await
        .expect("engine::functions::list should succeed")
        .expect("response should not be None");
    let worker_of = |id: &str| {
        listed["functions"]
            .as_array()
            .and_then(|functions| functions.iter().find(|f| f["function_id"] == id))
            .and_then(|f| f["worker_name"].as_str())
            .map(str::to_string)
    };

    let mut hosts = Vec::new();
    for source in [
        "engine::on-functions-changed",
        "engine::on-workers-changed",
        "engine::on-observers-changed",
        "configuration::on-updated",
        "observability::on-log",
        "observability::on-span",
        "stream::on-changed",
        "stream::on-join",
        "stream::on-leave",
    ] {
        let host = worker_of(source).unwrap_or_else(|| panic!("{source} is not listed"));
        assert!(
            names.contains(&host),
            "{source}: worker_name {host:?} is not a worker ({names:?})"
        );
        let info = engine
            .call("engine::workers::info", json!({ "name": host }))
            .await
            .expect("engine::workers::info should succeed")
            .expect("response should not be None");
        let functions: Vec<&str> = info["functions"]
            .as_array()
            .expect("'functions' should be an array")
            .iter()
            .filter_map(|f| f["function_id"].as_str())
            .collect();
        assert!(
            functions.contains(&source),
            "{host} does not list {source}: {functions:?}"
        );
        hosts.push((source, host));
    }
    let host_of = |source: &str| {
        hosts
            .iter()
            .find(|(s, _)| *s == source)
            .map(|(_, host)| host.as_str())
    };
    assert_eq!(
        host_of("engine::on-functions-changed"),
        host_of("engine::on-observers-changed")
    );
    assert_eq!(host_of("stream::on-changed"), host_of("stream::on-leave"));
    assert_ne!(
        host_of("configuration::on-updated"),
        host_of("stream::on-changed")
    );
}
