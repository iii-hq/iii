//! The compose daemon against a real engine.
//!
//! Everything here goes over a real WebSocket into a real `WorkerManager`: the
//! daemon registers `compose::*` in its own namespace, and the test reaches it
//! the same way an operator would — a namespaced trigger.
//!
//! What is *not* covered here: a container that actually becomes ready. That
//! needs a child process which speaks the SDK, and there is no such fixture
//! binary yet. The readiness path is therefore exercised through its failure
//! side (a child that never registers), which is the side that has to roll back.

use std::sync::Arc;
use std::time::Duration;

use iii::engine::Engine;
use iii::workers::engine_fn::EngineFunctionsWorker;
use iii::workers::traits::Worker;
use iii::workers::worker::WorkerManager;
use iii_compose::{ComposeFile, daemon::Daemon, remote};
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::{InitOptions, register_worker};
use serde_json::{Value, json};
use tokio::net::TcpListener;

/// Boots an engine with the modules the daemon needs: `engine::workers::*` for
/// readiness and registration.
async fn spawn_engine() -> u16 {
    iii::workers::observability::metrics::ensure_default_meter();

    let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
    let port = probe.local_addr().expect("local_addr").port();
    drop(probe);

    let engine = Arc::new(Engine::new());
    let engine_fn = EngineFunctionsWorker::create(engine.clone(), None)
        .await
        .expect("create EngineFunctionsWorker");
    engine_fn
        .initialize()
        .await
        .expect("initialize EngineFunctionsWorker");
    engine_fn.register_functions(engine.clone());

    let manager = WorkerManager::create(
        engine.clone(),
        Some(json!({ "port": port, "host": "127.0.0.1" })),
    )
    .await
    .expect("create WorkerManager");

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    manager
        .start_background_tasks(shutdown_rx, shutdown_tx)
        .await
        .expect("start WorkerManager");

    port
}

/// Writes a compose project into `dir` and returns the parsed file.
fn project(dir: &std::path::Path, compose: &str, workers: &[&str]) -> ComposeFile {
    for worker in workers {
        std::fs::create_dir_all(dir.join("workers").join(worker)).expect("worker dir");
    }
    let path = dir.join("worker-compose.yaml");
    std::fs::write(&path, compose).expect("write compose");
    ComposeFile::load(&path).expect("compose should parse")
}

/// Calls a `compose::*` function the way an operator does: by namespace.
async fn call(port: u16, namespace: &str, function: &str, payload: Value) -> Result<Value, String> {
    let client = register_worker(
        &format!("ws://127.0.0.1:{port}"),
        InitOptions {
            metadata: Some(iii_sdk::iii::WorkerMetadata {
                name: format!("test-caller-{}", uuid::Uuid::new_v4()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    let request = TriggerRequest {
        function_id: function.to_string(),
        payload,
        action: None,
        timeout_ms: Some(30_000),
    };
    let result = client.trigger(request.namespace(namespace)).await;
    client.shutdown_async().await;

    result.map_err(|err| err.to_string())
}

/// Keeps daemon state out of the developer's home directory.
///
/// The variable is process-wide and cargo runs tests in threads, so it is set
/// exactly once for the whole binary; tests stay isolated by using distinct
/// daemon ids, which are the subdirectory under this root.
fn isolate_state() {
    static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let root = ROOT.get_or_init(|| tempfile::tempdir().expect("state root"));
    unsafe { std::env::set_var("III_COMPOSE_STATE_DIR", root.path()) };
}

async fn start_daemon(port: u16, id: &str, file: ComposeFile) -> Arc<Daemon> {
    let daemon = Daemon::start(id.to_string(), file, format!("ws://127.0.0.1:{port}"), None)
        .await
        .expect("daemon should start");
    remote::register(&daemon);
    // The SDK flushes registrations after the namespace announce; give the
    // round trip a moment before the first call.
    tokio::time::sleep(Duration::from_millis(600)).await;
    daemon
}

const TWO_WORKERS: &str = r#"
name: orders
startup_timeout: 2s
stop_timeout: 1s
containers:
  database:
    worker: path://./workers/database
    scripts:
      run: "sleep 30"
  api:
    worker: path://./workers/api
    depends_on: [database]
    scripts:
      run: "sleep 30"
"#;

#[tokio::test(flavor = "multi_thread")]
async fn the_daemon_serves_compose_functions_in_its_own_namespace() {
    let tmp = tempfile::tempdir().unwrap();
    isolate_state();
    let port = spawn_engine().await;
    let file = project(tmp.path(), TWO_WORKERS, &["database", "api"]);
    let daemon = start_daemon(port, "serves", file).await;

    let listed = call(port, "serves", "compose::list", json!({}))
        .await
        .expect("compose::list should answer in the daemon's namespace");

    assert_eq!(listed["project"], "orders");
    assert_eq!(listed["daemon_id"], "serves");
    assert_eq!(
        listed["containers"],
        json!(["database", "api"]),
        "list reports the declared containers"
    );

    let status = call(port, "serves", "compose::status", json!({}))
        .await
        .expect("compose::status should answer");
    assert_eq!(status["containers"].as_array().unwrap().len(), 2);
    assert_eq!(status["containers"][0]["state"], "stopped");

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn compose_functions_are_unreachable_without_the_namespace() {
    let tmp = tempfile::tempdir().unwrap();
    isolate_state();
    let port = spawn_engine().await;
    let file = project(tmp.path(), TWO_WORKERS, &["database", "api"]);
    let daemon = start_daemon(port, "no-leak", file).await;

    // Routing is strict: without the namespace the invoke resolves in
    // `default`, where no daemon registered anything.
    let error = call(port, "default", "compose::list", json!({}))
        .await
        .expect_err("compose::* must not leak into the default namespace");
    assert!(
        error.contains("not found") || error.contains("NOT_FOUND"),
        "unexpected error: {error}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_aimed_at_another_daemon_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    isolate_state();
    let port = spawn_engine().await;
    let file = project(tmp.path(), TWO_WORKERS, &["database", "api"]);
    let daemon = start_daemon(port, "wrong-id", file).await;

    let error = call(port, "wrong-id", "compose::up", json!({ "id": "host-b" }))
        .await
        .expect_err("a mismatched id must not execute");

    assert!(error.contains("WRONG_DAEMON"), "unexpected error: {error}");
    // The message names the invocation that would have worked.
    assert!(
        error.contains("--namespace host-b"),
        "unexpected error: {error}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_child_that_never_registers_times_out_and_rolls_back() {
    let tmp = tempfile::tempdir().unwrap();
    isolate_state();
    let port = spawn_engine().await;
    // `sleep` is a process, not a worker: it never registers, so readiness can
    // never be satisfied. This is the failure the rollback exists for.
    let file = project(tmp.path(), TWO_WORKERS, &["database", "api"]);
    let daemon = start_daemon(port, "timeout", file).await;

    let result = call(port, "timeout", "compose::up", json!({}))
        .await
        .expect("compose::up answers even when it fails");

    assert_eq!(result["status"], "failed");
    let database = &result["containers"][0];
    assert_eq!(database["container"], "database");
    assert_eq!(database["error"]["code"], "STARTUP_TIMEOUT");

    // Nothing was left running: the timed-out child was stopped, and `api`
    // never started because its dependency failed.
    let status = call(port, "timeout", "compose::status", json!({}))
        .await
        .expect("status after a failed up");
    for container in status["containers"].as_array().unwrap() {
        assert_ne!(
            container["state"], "ready",
            "no container may report ready after a failed up: {container}"
        );
    }

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_daemon_with_the_same_id_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    isolate_state();
    let port = spawn_engine().await;

    let file = project(tmp.path(), TWO_WORKERS, &["database", "api"]);
    let first = start_daemon(port, "duplicate", file.clone()).await;

    // A second daemon claiming the same --id collides on
    // `(namespace, worker_name)`; the engine rejects it fatally rather than
    // letting two daemons answer for one project.
    let second = start_daemon(port, "duplicate", file).await;

    let mut rejected = false;
    for _ in 0..40 {
        if second.fatal_error().is_some() {
            rejected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        rejected,
        "the second daemon must be told it lost the id, not fail silently"
    );

    second.shutdown().await;
    first.shutdown().await;
}
