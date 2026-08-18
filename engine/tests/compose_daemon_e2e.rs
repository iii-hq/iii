//! The compose daemon against a real engine.
//!
//! Everything here goes over a real WebSocket into a real `WorkerManager`: a
//! daemon registers `compose::*` in its own namespace — `default` when it was
//! given no `--id` — and the test reaches it the same way an operator does.
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
    call_in(port, None, function, payload).await
}

/// The same call, addressed to one daemon by name.
///
/// This is how an operator reaches a specific machine: `compose::*` lives in
/// the daemon's own namespace, so a bare call resolves in `default` and a
/// named one resolves nowhere else.
async fn call_in(
    port: u16,
    namespace: Option<&str>,
    function: &str,
    payload: Value,
) -> Result<Value, String> {
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
    let result = match namespace {
        Some(namespace) => client.trigger(request.namespace(namespace)).await,
        None => client.trigger(request).await,
    };
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
    ROOT.get_or_init(|| {
        let root = tempfile::tempdir().expect("state root");
        // SAFETY: `get_or_init` runs this once and serialises the callers, so
        // the write happens a single time rather than on every call — which is
        // what the comment above claimed and the code did not do.
        unsafe { std::env::set_var("III_COMPOSE_STATE_DIR", root.path()) };
        root
    });
}

async fn start_daemon(port: u16) -> Arc<Daemon> {
    start_daemon_named(port, iii_compose::namespace::DEFAULT_NAMESPACE).await
}

/// A daemon with an explicit identity, for the tests that need two of them.
///
/// The id is the namespace it answers in, so two daemons here are two
/// machines: distinct ids coexist, and the same id twice is the collision that
/// must be refused.
async fn start_daemon_named(port: u16, daemon_namespace: &str) -> Arc<Daemon> {
    let daemon = Daemon::start(
        format!("ws://127.0.0.1:{port}"),
        daemon_namespace.to_string(),
    );
    remote::register(&daemon);
    // The SDK flushes registrations after the namespace announce; give the
    // round trip a moment before the first call.
    tokio::time::sleep(Duration::from_millis(600)).await;
    daemon
}

const TWO_WORKERS: &str = r#"
namespace: orders
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
namespace: billing
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

    // The daemon runs from the test harness's directory, which holds no
    // compose file, so there is nothing to fall back to and nothing to guess.
    let error = call(port, "compose::down", json!({}))
        .await
        .expect_err("down without a file cannot mean anything");

    assert!(error.contains("NO_COMPOSE_FILE"), "unexpected: {error}");
    // The message is the invocation that would have worked, not a description
    // of the one that did not.
    assert!(error.contains("file="), "the way out is named: {error}");

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_that_is_not_there_is_said_so() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    // A mistyped project used to be an id nobody had bound, which quietly
    // became an empty project reporting "nothing to stop" — success for a
    // command that did nothing. A mistyped *file* cannot: it has to exist.
    let error = call(
        port,
        "compose::down",
        json!({ "file": "/nowhere/worker-compose.yaml" }),
    )
    .await
    .expect_err("a file nobody can read is not a project");

    assert!(error.contains("/nowhere"), "it names the file: {error}");

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
            json!({ "file": file.to_str().unwrap() }),
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
        json!({ "file": file.to_str().unwrap() }),
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

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn one_file_is_one_project_however_it_is_spelled() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    let tmp = tempfile::tempdir().unwrap();
    let orders = project(tmp.path(), TWO_WORKERS, &["database", "api"]);
    // The same file by a longer route. Treating this as a second project would
    // put two supervisors on one set of containers.
    let roundabout = tmp.path().join(".").join("worker-compose.yaml");

    for spelling in [&orders, &roundabout] {
        call(
            port,
            "compose::status",
            json!({ "file": spelling.to_str().unwrap() }),
        )
        .await
        .unwrap_or_else(|err| panic!("status should load {spelling:?}: {err}"));
    }

    let listed = call(port, "compose::list", json!({}))
        .await
        .expect("compose::list should answer");
    assert_eq!(
        listed["projects"].as_array().expect("projects").len(),
        1,
        "one file is one project: {listed}"
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
        json!({ "file": file.to_str().unwrap() }),
    )
    .await
    .expect("compose::up answers even when it fails");

    assert_eq!(result["status"], "failed");
    let database = &result["containers"][0];
    assert_eq!(database["container"], "database");
    assert_eq!(database["error"]["code"], "STARTUP_TIMEOUT");

    // Nothing was left running: the timed-out child was stopped, and `api`
    // never started because its dependency failed.
    let status = call(
        port,
        "compose::status",
        json!({ "file": file.to_str().unwrap() }),
    )
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
    // Same id, so both would own `compose::up` in the same namespace and the
    // engine would route a call to one of them, leaving the other holding
    // projects nobody can address. The fixed worker name turns that into a
    // rejection: the `(id, compose)` lease is the only race-free way to say a
    // machine identity is already taken.
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

/// Two machines, one engine — the point of the id.
///
/// Distinct ids are distinct namespaces, so both daemons keep `compose::*`
/// and an operator picks one with `--namespace`. Without this, an engine holds
/// exactly one machine and compose cannot supervise anything it is not running
/// beside.
#[tokio::test]
async fn two_daemons_with_distinct_ids_both_serve() {
    isolate_state();
    let port = spawn_engine().await;

    let _a = start_daemon_named(port, "pc-a").await;
    let _b = start_daemon_named(port, "pc-b").await;

    // Neither was rejected: the lease is `(id, compose)`, and these are two.
    for (daemon, id) in [(&_a, "pc-a"), (&_b, "pc-b")] {
        assert!(
            daemon.fatal_error().is_none(),
            "{id} should have kept its registration"
        );
    }

    // And each answers on its own address, reporting its own identity — an
    // operator who names the wrong one must not silently reach the other.
    for id in ["pc-a", "pc-b"] {
        let listed = call_in(port, Some(id), "compose::list", json!({}))
            .await
            .unwrap_or_else(|err| panic!("{id} should answer in its own namespace: {err}"));
        assert_eq!(listed["daemon_namespace"], id);
    }

    // A bare call still resolves in `default`, where neither of them is, so it
    // reaches nothing rather than picking a machine for the caller.
    assert!(
        call(port, "compose::list", json!({})).await.is_err(),
        "a call with no namespace must not be routed to an arbitrary daemon"
    );
}

/// `namespace=` in the payload is a guard, not a route.
///
/// The engine resolves by the flag and never reads the body, so a caller who
/// spells it as a payload field lands wherever the flag pointed. Saying so is
/// the difference between a fixable mistake and a project brought up on the
/// wrong machine.
#[tokio::test]
async fn naming_another_daemon_in_the_payload_is_refused() {
    isolate_state();
    let port = spawn_engine().await;
    let _daemon = start_daemon_named(port, "pc-a").await;

    let error = call_in(
        port,
        Some("pc-a"),
        "compose::list",
        json!({ "namespace": "pc-b" }),
    )
    .await
    .expect_err("a call that named another daemon must not be served here");

    assert!(error.contains("WRONG_DAEMON"), "unexpected error: {error}");
    // The message carries the invocation that would have worked.
    assert!(error.contains("--namespace pc-b"), "unexpected: {error}");

    // Naming the daemon it actually reached is fine, and is how a script keeps
    // itself honest.
    call_in(
        port,
        Some("pc-a"),
        "compose::list",
        json!({ "namespace": "pc-a" }),
    )
    .await
    .expect("agreeing with the daemon it reached is not an error");
}

/// A configuration worker that cannot answer fails the container.
///
/// The other half of the rule that lets a first boot through. An entry nobody
/// has registered yet is not a failure — the worker is what creates it. An
/// entry compose cannot *read* is, because starting the container would mean
/// booting it on defaults nobody asked for.
///
/// The engine here has no configuration worker at all, so the call fails as
/// `function_not_found` — an error, and pointedly not `NOT_FOUND`.
#[tokio::test(flavor = "multi_thread")]
async fn a_configuration_that_cannot_be_read_stops_the_container() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        r#"
namespace: orders
startup_timeout: 2s
stop_timeout: 1s
containers:
  database:
    worker: path://./workers/database
    config_name: nobody-can-read-this
    scripts:
      run: "sleep 30"
"#,
        &["database"],
    );

    let result = call(
        port,
        "compose::up",
        json!({ "file": file.to_str().unwrap() }),
    )
    .await
    .expect("compose::up answers even when it fails");

    assert_eq!(result["status"], "failed", "{result}");
    let database = &result["containers"][0];
    assert_eq!(database["error"]["code"], "CONFIG_FETCH_FAILED", "{result}");
    // Not mistaken for a first boot, which is the case that must proceed.
    assert_ne!(database["state"], "ready", "{result}");

    daemon.shutdown().await;
}
