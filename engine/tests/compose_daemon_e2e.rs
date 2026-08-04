//! The compose daemon against a real engine.
//!
//! Everything here goes over a real WebSocket into a real `WorkerManager`: one
//! daemon registers `compose::*` in `default`, and the test reaches it the same
//! way an operator does — `iii trigger compose::up id=…`.
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

/// Writes a compose project into `dir` and returns its path.
fn project(dir: &std::path::Path, compose: &str, workers: &[&str]) -> std::path::PathBuf {
    for worker in workers {
        std::fs::create_dir_all(dir.join("workers").join(worker)).expect("worker dir");
    }
    let path = dir.join("worker-compose.yaml");
    std::fs::write(&path, compose).expect("write compose");
    ComposeFile::load(&path).expect("compose should parse");
    path
}

/// Calls a `compose::*` function the way an operator does: in `default`, with
/// the project named in the payload.
async fn call(port: u16, function: &str, payload: Value) -> Result<Value, String> {
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

    let result = client
        .trigger(TriggerRequest {
            function_id: function.to_string(),
            payload,
            action: None,
            timeout_ms: Some(30_000),
        })
        .await;
    client.shutdown_async().await;

    result.map_err(|err| err.to_string())
}

/// Keeps daemon state out of the developer's home directory.
///
/// The variable is process-wide and cargo runs tests in threads, so it is set
/// exactly once for the whole binary; tests stay isolated by using distinct
/// project ids, which are the subdirectory under this root.
fn isolate_state() {
    static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let root = ROOT.get_or_init(|| tempfile::tempdir().expect("state root"));
    unsafe { std::env::set_var("III_COMPOSE_STATE_DIR", root.path()) };
}

async fn start_daemon(port: u16) -> Arc<Daemon> {
    let daemon = Daemon::start(format!("ws://127.0.0.1:{port}"));
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

/// The same project under another name, so two of them can be held at once
/// without their workers competing for one namespace.
const ONE_WORKER: &str = r#"
name: billing
startup_timeout: 2s
stop_timeout: 1s
containers:
  ledger:
    worker: path://./workers/ledger
    scripts:
      run: "sleep 30"
"#;

#[tokio::test(flavor = "multi_thread")]
async fn the_daemon_serves_compose_functions_in_the_default_namespace() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    // `default` is where an operator's `iii trigger` lands with no namespace
    // flag, which is the whole point of moving the control surface here: the
    // namespace goes back to being the workers' address.
    let listed = call(port, "compose::list", json!({}))
        .await
        .expect("compose::list should answer in default");

    assert_eq!(listed["daemon"], "compose");
    assert_eq!(
        listed["projects"],
        json!([]),
        "a daemon that has just started holds nothing"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_project_scoped_call_names_the_argument_it_wanted() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    let error = call(port, "compose::down", json!({}))
        .await
        .expect_err("down without an id cannot mean anything");

    assert!(error.contains("MISSING_ID"), "unexpected error: {error}");
    // The message is the invocation that would have worked, not a description
    // of the one that did not.
    assert!(
        error.contains("compose::down id="),
        "unexpected error: {error}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_id_is_a_question_rather_than_a_new_project() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    // A mistyped id must not quietly become an empty project reporting
    // "nothing to stop" — that reads as success for a command that did nothing.
    let error = call(port, "compose::down", json!({ "id": "ghost" }))
        .await
        .expect_err("an id nobody brought up is not a project");

    assert!(error.contains("UNKNOWN_PROJECT"), "unexpected: {error}");
    assert!(error.contains("file="), "the way out is named: {error}");

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn one_daemon_holds_several_projects_at_once() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let orders = project(first.path(), TWO_WORKERS, &["database", "api"]);
    let billing = project(second.path(), ONE_WORKER, &["ledger"]);

    for (id, file) in [("shop", &orders), ("books", &billing)] {
        call(
            port,
            "compose::status",
            json!({ "id": id, "file": file.to_str().unwrap() }),
        )
        .await
        .unwrap_or_else(|err| panic!("status should load {id}: {err}"));
    }

    let listed = call(port, "compose::list", json!({}))
        .await
        .expect("compose::list should answer");
    let projects = listed["projects"].as_array().expect("projects");

    assert_eq!(projects.len(), 2, "both projects are held: {listed}");
    // Each keeps its own namespace, taken from `name:` and never from the id:
    // the id addresses the project, the namespace addresses its workers.
    let namespaces: Vec<&str> = projects
        .iter()
        .map(|p| p["namespace"].as_str().unwrap())
        .collect();
    assert!(namespaces.contains(&"orders"), "{listed}");
    assert!(namespaces.contains(&"billing"), "{listed}");

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn validating_a_file_does_not_take_the_project_on() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    let tmp = tempfile::tempdir().unwrap();
    let file = project(tmp.path(), TWO_WORKERS, &["database", "api"]);

    let report = call(
        port,
        "compose::validate",
        json!({ "id": "checked", "file": file.to_str().unwrap() }),
    )
    .await
    .expect("validate should answer for a file it was handed");
    assert_eq!(report["namespace"], "orders");
    assert_eq!(report["start_order"], json!(["database", "api"]));

    // Validation is a question, not a decision. Holding the project would bind
    // the id to this path and put durable state behind it — so a CI job that
    // only ever validates would leave a daemon owning what it checked.
    let listed = call(port, "compose::list", json!({}))
        .await
        .expect("compose::list should answer");
    assert_eq!(
        listed["projects"],
        json!([]),
        "validate must hold nothing: {listed}"
    );

    // And the id it named is still free to be bound to something else.
    let other = tempfile::tempdir().unwrap();
    let billing = project(other.path(), ONE_WORKER, &["ledger"]);
    call(
        port,
        "compose::status",
        json!({ "id": "checked", "file": billing.to_str().unwrap() }),
    )
    .await
    .expect("the id was never bound, so it can still be claimed");

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_id_cannot_be_pointed_at_a_different_file() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let orders = project(first.path(), TWO_WORKERS, &["database", "api"]);
    let billing = project(second.path(), ONE_WORKER, &["ledger"]);

    call(
        port,
        "compose::status",
        json!({ "id": "bound", "file": orders.to_str().unwrap() }),
    )
    .await
    .expect("the first call binds the id");

    // Rebinding would adopt children the new file never started, while the
    // recorded state still points at the old project.
    let error = call(
        port,
        "compose::status",
        json!({ "id": "bound", "file": billing.to_str().unwrap() }),
    )
    .await
    .expect_err("an id belongs to one compose file");

    assert!(
        error.contains("STATE_BINDING_MISMATCH"),
        "unexpected: {error}"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_child_that_never_registers_times_out_and_rolls_back() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    // `sleep` is a process, not a worker: it never registers, so readiness can
    // never be satisfied. This is the failure the rollback exists for.
    let tmp = tempfile::tempdir().unwrap();
    let file = project(tmp.path(), TWO_WORKERS, &["database", "api"]);

    let result = call(
        port,
        "compose::up",
        json!({ "id": "timeout", "file": file.to_str().unwrap() }),
    )
    .await
    .expect("compose::up answers even when it fails");

    assert_eq!(result["status"], "failed");
    let database = &result["containers"][0];
    assert_eq!(database["container"], "database");
    assert_eq!(database["error"]["code"], "STARTUP_TIMEOUT");

    // Nothing was left running: the timed-out child was stopped, and `api`
    // never started because its dependency failed. The id is enough to ask —
    // the file was needed once and is remembered.
    let status = call(port, "compose::status", json!({ "id": "timeout" }))
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
async fn a_second_daemon_on_one_engine_is_refused() {
    isolate_state();
    let port = spawn_engine().await;

    let first = start_daemon(port).await;
    // Both would own `compose::up` in `default` and the engine would route a
    // call to one of them, leaving the other holding projects nobody can
    // address. The fixed worker name turns that into a rejection: the
    // `(default, compose)` lease is the only race-free way to say an engine
    // already has one.
    let second = start_daemon(port).await;

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
        "the second daemon must be told it lost the name, not sit there unreachable"
    );

    // And the first is still the one answering.
    let listed = call(port, "compose::list", json!({}))
        .await
        .expect("the daemon that won still serves");
    assert_eq!(listed["daemon"], "compose");

    second.abandon().await;
    first.shutdown().await;
}
