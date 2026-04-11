use std::sync::Arc;

use iii::engine::Engine;
use iii::function::{Function, FunctionResult};
use iii::workers::reload::WorkerRegistrations;

fn make_dummy_function(id: &str) -> Function {
    Function {
        handler: Arc::new(|_invocation_id, _input, _session| {
            Box::pin(async { FunctionResult::Success(None) })
        }),
        _function_id: id.to_string(),
        _description: None,
        request_format: None,
        response_format: None,
        metadata: None,
    }
}

#[test]
fn scope_begin_end_lifecycle() {
    let engine = Arc::new(Engine::new());
    engine.begin_worker_scope("test::Worker");
    let regs = engine.end_worker_scope();
    assert!(regs.function_ids.is_empty());
}

#[test]
fn remove_worker_registrations_clears_functions() {
    let engine = Arc::new(Engine::new());
    // Manually seed a function, then construct WorkerRegistrations by hand
    // (Task 1 has no scope interception yet; that's Task 2).
    let function_id = "test::Worker::handler".to_string();
    engine
        .functions
        .register_function(function_id.clone(), make_dummy_function(&function_id));
    assert!(engine.functions.get(&function_id).is_some());

    let regs = WorkerRegistrations {
        function_ids: vec![function_id.clone()],
    };
    engine.remove_worker_registrations(&regs);

    assert!(engine.functions.get(&function_id).is_none());
}

#[test]
fn register_function_records_into_active_scope() {
    let engine = Arc::new(Engine::new());

    engine.begin_worker_scope("test::Worker");
    engine.functions.register_function(
        "test::Worker::handler".to_string(),
        make_dummy_function("test::Worker::handler"),
    );
    let regs = engine.end_worker_scope();

    assert_eq!(
        regs.function_ids,
        vec!["test::Worker::handler".to_string()]
    );
}

#[test]
fn register_function_outside_scope_does_not_track() {
    let engine = Arc::new(Engine::new());
    // No scope active: registry still stores the function, but nothing is captured.
    engine.functions.register_function(
        "test::Worker::handler".to_string(),
        make_dummy_function("test::Worker::handler"),
    );
    assert!(engine.functions.get("test::Worker::handler").is_some());

    // Open a fresh scope afterwards — it should be empty because the registration
    // happened before begin_worker_scope.
    engine.begin_worker_scope("test::Worker");
    let regs = engine.end_worker_scope();
    assert!(regs.function_ids.is_empty());
}

#[tokio::test]
async fn builder_produces_running_workers_with_matching_entries() {
    use iii::EngineBuilder;
    use iii::workers::config::EngineConfig;

    let default_config = EngineConfig::default_config();
    let expected_names: Vec<String> = default_config
        .modules
        .iter()
        .chain(default_config.workers.iter())
        .map(|e| e.name.clone())
        .collect();
    assert!(
        !expected_names.is_empty(),
        "default config must define at least one worker entry"
    );

    let builder = EngineBuilder::new()
        .with_config(EngineConfig::default_config())
        .build()
        .await
        .expect("build should succeed for default config");

    let running = builder.running();
    assert!(
        !running.is_empty(),
        "default config must produce at least one running worker"
    );

    // Every RunningWorker must carry a non-empty entry name.
    for rw in running {
        assert!(
            !rw.entry.name.is_empty(),
            "RunningWorker.entry.name must be populated from the source WorkerEntry"
        );
    }

    // Every entry from the original default config must appear in the running
    // set (mandatory workers may add more on top, which is fine).
    let running_names: std::collections::HashSet<&str> =
        running.iter().map(|rw| rw.entry.name.as_str()).collect();
    for expected in &expected_names {
        assert!(
            running_names.contains(expected.as_str()),
            "expected worker '{}' missing from running()",
            expected
        );
    }
}
