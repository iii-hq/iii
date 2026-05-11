use std::sync::Arc;

use iii::engine::Engine;
use iii::function::{Function, FunctionResult};
use iii::workers::reload::WorkerRegistrations;
use serial_test::serial;

/// Suppress the `iii-worker-ops` auto-injection. `EngineBuilder::build()`
/// otherwise drops a daemon child into `serve()`, and that child only
/// listens for SIGINT (`tokio::signal::ctrl_c`); the engine's worker
/// teardown then waits past this test's 2s SIGTERM deadline. We're
/// asserting engine-builtin shutdown speed, not daemon lifecycle.
fn disable_builtin_daemons() {
    static SET: std::sync::Once = std::sync::Once::new();
    SET.call_once(|| {
        // Safety: called once before any code that may read the env;
        // tests are #[serial] so there is no concurrent reader at this
        // point.
        unsafe {
            std::env::set_var("IIIWORKER_DISABLE_BUILTIN_DAEMONS", "1");
        }
    });
}

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

    assert_eq!(regs.function_ids, vec!["test::Worker::handler".to_string()]);
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

// Serial so the env-var flipped by `serve_returns_on_sigterm`'s
// `disable_builtin_daemons()` helper can't race with this test's
// back-to-back `default_config()` calls (a flip between them produces
// asymmetric `expected_names` vs `running` sets and a spurious failure).
#[tokio::test]
#[serial]
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

// Helper: minimal config whose only explicit worker is iii-worker-manager
// bound to an ephemeral port. Mandatory workers (telemetry, observability,
// engine-functions) will be injected by build() but don't bind fixed ports.
// This lets builder-plumbing tests coexist with the other integration tests
// in this binary that also exercise build(), without fighting over the real
// default_config()'s fixed ports (iii-http, iii-stream, iii-worker-manager).
fn minimal_config_for_builder_tests() -> iii::workers::config::EngineConfig {
    iii::workers::config::EngineConfig {
        modules: Vec::new(),
        workers: vec![iii::workers::config::WorkerEntry {
            name: "iii-worker-manager".to_string(),
            image: None,
            config: Some(serde_json::json!({
                "host": "127.0.0.1",
                "port": 0,
            })),
        }],
    }
}

#[tokio::test]
async fn config_path_is_stored_when_set() {
    use iii::EngineBuilder;

    let builder = EngineBuilder::new()
        .with_config(minimal_config_for_builder_tests())
        .with_config_path("/tmp/fake-config.yaml")
        .build()
        .await
        .unwrap();

    assert_eq!(builder.config_path(), Some("/tmp/fake-config.yaml"));
}

#[tokio::test]
async fn config_path_is_none_when_not_set() {
    use iii::EngineBuilder;

    let builder = EngineBuilder::new()
        .with_config(minimal_config_for_builder_tests())
        .build()
        .await
        .unwrap();

    assert!(builder.config_path().is_none());
}

/// Regression test for a shutdown regression introduced when each worker
/// was given its own per-worker shutdown channel: serve() was subscribed
/// to a private channel with no publisher, so SIGTERM/Ctrl+C no longer
/// unwound the server. serve() must return within a short deadline after
/// SIGTERM is delivered to the current process.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn serve_returns_on_sigterm() {
    use iii::EngineBuilder;
    use iii::workers::config::EngineConfig;
    use std::time::Duration;
    use tokio::signal::unix::{SignalKind, signal};

    disable_builtin_daemons();

    // Register a SIGTERM handler in the test process BEFORE anything else so
    // the default disposition (process termination) is replaced. The worker's
    // own shutdown_signal() will also see the signal because tokio signal
    // handlers are process-global.
    let mut sigterm_guard = signal(SignalKind::terminate()).expect("install SIGTERM handler");

    // Use a minimal config: only declare iii-worker-manager (bound to an
    // ephemeral port to avoid collisions with parallel tests and any local
    // iii dev server). Other mandatory workers (telemetry, observability,
    // engine-functions) will be injected by build() automatically and do
    // not bind ports. We intentionally avoid EngineConfig::default_config()
    // here because its enabled-by-default workers (iii-stream, iii-http,
    // etc.) bind fixed ports and would conflict with other integration tests
    // running in parallel.
    let config = EngineConfig {
        modules: Vec::new(),
        workers: vec![iii::workers::config::WorkerEntry {
            name: "iii-worker-manager".to_string(),
            image: None,
            config: Some(serde_json::json!({
                "host": "127.0.0.1",
                "port": 0,
            })),
        }],
    };

    let builder = EngineBuilder::new()
        .with_config(config)
        .build()
        .await
        .expect("build should succeed for default config");

    let handle = tokio::spawn(async move { builder.serve().await });

    // Give serve() time to spawn its workers and hit the graceful-shutdown
    // future (which installs its own tokio signal handler).
    tokio::time::sleep(Duration::from_millis(500)).await;

    // SAFETY: kill(2) with SIGTERM to our own pid is well-defined.
    unsafe {
        libc::kill(libc::getpid(), libc::SIGTERM);
    }

    // Drain the test's own SIGTERM receiver so the signal is acknowledged.
    // (Not strictly required, but keeps things tidy.)
    let _ = tokio::time::timeout(Duration::from_millis(100), sigterm_guard.recv()).await;

    // serve() must return within 2 seconds.
    let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
    assert!(
        result.is_ok(),
        "serve() did not return within 2s of SIGTERM -- shutdown regression"
    );
    result
        .expect("timeout")
        .expect("join")
        .expect("serve error");
}
