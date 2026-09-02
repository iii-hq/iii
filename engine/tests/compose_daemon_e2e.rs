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
use iii_compose::{
    ComposeFile,
    daemon::{Daemon, EnginePolicy},
    remote,
};
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::{InitOptions, RegisterFunction, register_worker};
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
        None,
        iii_compose::daemon::EnginePolicy::External,
    );
    remote::register(&daemon);
    // The SDK flushes registrations after the namespace announce; give the
    // round trip a moment before the first call.
    tokio::time::sleep(Duration::from_millis(600)).await;
    daemon
}

/// Registers the readiness identity for a test child process.
fn register_test_worker(port: u16, namespace: &str, name: &str) -> iii_sdk::IIIClient {
    let mut metadata = iii_sdk::iii::WorkerMetadata {
        name: name.to_string(),
        ..Default::default()
    };
    metadata.namespace = Some(namespace.to_string());

    register_worker(
        &format!("ws://127.0.0.1:{port}"),
        InitOptions {
            metadata: Some(metadata),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
    )
}

/// Waits until every child has crossed an explicit process-start barrier.
async fn wait_for_start_markers(paths: &[&std::path::Path]) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if paths.iter().all(|path| path.exists()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("children did not reach their start barriers");
}

/// Waits for the engine to observe a worker registration or its removal.
async fn wait_for_worker_state(daemon: &Daemon, namespace: &str, name: &str, registered: bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if daemon.engine().is_registered(namespace, name).await.ok() == Some(registered) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("worker {namespace}/{name} did not reach registered={registered}"));
}

async fn wait_for_operation(port: u16, namespace: Option<&str>, operation_id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let snapshot = call_in(
                port,
                namespace,
                "compose::operation",
                json!({ "operation_id": operation_id }),
            )
            .await
            .expect("compose::operation should answer");
            if snapshot["status"] != "running" {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("operation {operation_id} did not finish"))
}

fn operation_containers(operation: &Value) -> Vec<&str> {
    let mut names = operation["containers"]
        .as_array()
        .expect("operation should report containers")
        .iter()
        .map(|container| {
            container["container"]
                .as_str()
                .expect("container result should have a name")
        })
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
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
    start_after: [database]
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
async fn schema_introspection_is_callable_and_matches_engine_metadata() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    let schema = call(
        port,
        "compose::schema",
        json!({ "function_id": "compose::up" }),
    )
    .await
    .expect("compose::schema should answer");
    let schemas = schema["schemas"].as_array().expect("schemas array");
    assert_eq!(schemas.len(), 1, "the filter returns one entry: {schema}");
    assert_eq!(schemas[0]["function_id"], "compose::up");
    assert!(schemas[0]["request"]["properties"]["file"].is_object());
    assert!(schemas[0]["response"]["properties"]["changed"].is_object());
    assert!(schemas[0]["response"]["properties"]["containers"].is_null());
    assert_eq!(schemas[0]["default_timeout_ms"], 600_000);
    assert_eq!(schemas[0]["idempotent"], true);

    let info = call(
        port,
        "engine::functions::info",
        json!({ "function_id": "compose::up" }),
    )
    .await
    .expect("engine::functions::info should find compose::up");
    assert_eq!(info["request_schema"], schemas[0]["request"]);
    assert_eq!(info["response_schema"], schemas[0]["response"]);
    assert_eq!(info["metadata"]["default_timeout_ms"], 600_000);

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_project_scoped_call_uses_the_daemons_default_file() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    // Engine integration tests run from `engine/`, whose canonical Compose
    // fixture now owns an engine. A bare project-scoped call must find that
    // default file. Because this particular daemon is attached to an external
    // engine, the ownership guard then rejects the managed file.
    let error = call(port, "compose::down", json!({}))
        .await
        .expect_err("an external daemon must not load a managed engine file");

    assert!(
        error.contains("ENGINE_SECTION_REQUIRES_MANAGED_START"),
        "unexpected: {error}"
    );
    assert!(
        error.contains("worker-compose.yaml") && error.contains("without --engine"),
        "the way out is named: {error}"
    );

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
async fn add_edits_several_workers_and_reconciles_the_project_once() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    let tmp = tempfile::tempdir().unwrap();
    let existing_started = tmp.path().join("workers/existing/started");
    let database_started = tmp.path().join("workers/database/started");
    let web_started = tmp.path().join("workers/web/started");
    let file = project(
        tmp.path(),
        r#"
namespace: addition
startup_timeout: 3s
stop_timeout: 100ms
containers:
  existing:
    worker: path://./workers/existing
    scripts:
      run: "echo started >> started && sleep 30"
"#,
        &["existing", "database", "web"],
    );
    for worker in ["database", "web"] {
        std::fs::write(
            tmp.path()
                .join("workers")
                .join(worker)
                .join("iii.worker.yaml"),
            "scripts:\n  start: \"echo started >> started && sleep 30\"\n",
        )
        .expect("write worker manifest");
    }

    let add = call(
        port,
        "compose::add",
        json!({
            "file": file.to_str().unwrap(),
            "workers": ["./workers/database", "./workers/web"],
        }),
    );
    let ready = async {
        wait_for_start_markers(&[
            existing_started.as_path(),
            database_started.as_path(),
            web_started.as_path(),
        ])
        .await;
        let existing = register_test_worker(port, "addition", "existing");
        let database = register_test_worker(port, "addition", "database");
        let web = register_test_worker(port, "addition", "web");
        for worker in ["existing", "database", "web"] {
            wait_for_worker_state(&daemon, "addition", worker, true).await;
        }
        (existing, database, web)
    };
    let (result, (existing, database, web)) = tokio::join!(add, ready);
    let result = result.expect("compose::add should answer");

    assert_eq!(result["status"], "accepted", "{result}");
    assert_eq!(result["requested"], 2, "{result}");
    let operation_id = result["operation_id"]
        .as_str()
        .expect("accepted add should name its operation");
    for internal in ["containers", "down", "restarted", "up", "changed"] {
        assert!(
            result.get(internal).is_none(),
            "mutation leaked {internal}: {result}"
        );
    }
    let operation = wait_for_operation(port, None, operation_id).await;
    assert_eq!(operation["status"], "succeeded", "{operation}");
    let status = call(
        port,
        "compose::status",
        json!({ "file": file.to_str().unwrap() }),
    )
    .await
    .expect("status after add");
    assert_eq!(
        operation_containers(&status),
        vec!["database", "existing", "web"],
        "status did not report the complete edited worker set: {status}"
    );

    let edited = std::fs::read_to_string(&file).expect("read edited compose file");
    for worker in ["database", "web"] {
        assert_eq!(
            edited.matches(&format!("  {worker}:\n")).count(),
            1,
            "worker should be declared once: {edited}"
        );
    }
    for marker in [&existing_started, &database_started, &web_started] {
        let starts = std::fs::read_to_string(marker).expect("read start count");
        assert_eq!(
            starts.lines().count(),
            1,
            "the batch should restart the project once: {starts}"
        );
    }

    existing.shutdown_async().await;
    database.shutdown_async().await;
    web.shutdown_async().await;
    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn add_starts_a_managed_project_declared_with_null_containers() {
    isolate_state();
    let port = spawn_engine().await;

    let tmp = tempfile::tempdir().unwrap();
    let worker_dir = tmp.path().join("workers/state");
    std::fs::create_dir_all(&worker_dir).expect("worker dir");
    std::fs::write(
        worker_dir.join("iii.worker.yaml"),
        "scripts:\n  start: \"echo started > started && sleep 30\"\n",
    )
    .expect("write worker manifest");

    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(
        &file,
        format!(
            "namespace: empty-add\nstartup_timeout: 3s\nstop_timeout: 100ms\nengine:\n  url: ws://127.0.0.1:{port}\n  workers: {{}}\ncontainers:\n"
        ),
    )
    .expect("write compose file");
    let compose = ComposeFile::load(&file).expect("empty managed project should parse");
    let engine = compose.engine.clone().expect("managed engine spec");

    let daemon_namespace = "empty-add-daemon";
    let daemon = Daemon::start(
        format!("ws://127.0.0.1:{port}"),
        daemon_namespace.to_string(),
        None,
        EnginePolicy::Managed {
            owner: compose.path.clone(),
            spec: engine,
        },
    );
    remote::register(&daemon);
    tokio::time::sleep(Duration::from_millis(600)).await;

    let up = call_in(
        port,
        Some(daemon_namespace),
        "compose::up",
        json!({ "file": file.to_str().unwrap() }),
    )
    .await
    .expect("empty managed project should start");
    assert_eq!(up["status"], "ok", "{up}");
    assert_eq!(up["changed"], false, "{up}");
    assert!(
        up.get("containers").is_none(),
        "mutation leaked internals: {up}"
    );

    let started = worker_dir.join("started");
    let add = call_in(
        port,
        Some(daemon_namespace),
        "compose::add",
        json!({
            "file": file.to_str().unwrap(),
            "workers": ["./workers/state"],
        }),
    );
    let ready = async {
        wait_for_start_markers(&[started.as_path()]).await;
        let worker = register_test_worker(port, "empty-add", "state");
        worker.register_function(
            "state::ping",
            RegisterFunction::new_async(|input: Value| async move {
                Ok(json!({ "pong": input["message"] }))
            }),
        );
        wait_for_worker_state(&daemon, "empty-add", "state", true).await;
        worker
    };
    let (result, worker) = tokio::join!(add, ready);
    let result = result.expect("compose::add should answer");

    assert_eq!(result["status"], "accepted", "{result}");
    assert_eq!(result["requested"], 1, "{result}");
    let operation_id = result["operation_id"]
        .as_str()
        .expect("accepted add should name its operation");
    assert!(
        result.get("up").is_none(),
        "mutation leaked internals: {result}"
    );
    let operation = wait_for_operation(port, Some(daemon_namespace), operation_id).await;
    assert_eq!(operation["status"], "succeeded", "{operation}");

    let edited = std::fs::read_to_string(&file).expect("read edited compose file");
    assert!(
        edited.contains("containers:\n  # added by compose::add\n  state:\n"),
        "first worker was not written as a block: {edited}"
    );

    let ping = call_in(
        port,
        Some("empty-add"),
        "state::ping",
        json!({ "message": "hello" }),
    )
    .await
    .expect("the added worker function should answer");
    assert_eq!(ping, json!({ "pong": "hello" }));

    let stop = call_in(
        port,
        Some(daemon_namespace),
        "compose::stop",
        json!({ "file": file.to_str().unwrap() }),
    )
    .await
    .expect("managed project should stop");
    assert_eq!(stop["stopping"], json!([file.to_str().unwrap()]), "{stop}");

    worker.shutdown_async().await;
    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn remove_drops_dependency_edges_and_keeps_survivors_running() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    let tmp = tempfile::tempdir().unwrap();
    let foundation_started = tmp.path().join("workers/foundation/started");
    let keep_started = tmp.path().join("workers/keep/started");
    let discard_started = tmp.path().join("workers/discard/started");
    let file = project(
        tmp.path(),
        r#"
namespace: removal
startup_timeout: 3s
stop_timeout: 100ms
containers:
  foundation:
    worker: path://./workers/foundation
    scripts:
      run: "touch started && sleep 30"
  keep:
    worker: path://./workers/keep
    start_after: [foundation]
    scripts:
      run: "touch started && sleep 30"
  discard:
    worker: path://./workers/discard
    scripts:
      run: "touch started && sleep 30"
"#,
        &["foundation", "keep", "discard"],
    );

    // Each child creates its marker only after compose has captured the
    // readiness baseline and spawned it. Registration therefore cannot race
    // with baseline capture.
    let up = call(
        port,
        "compose::up",
        json!({ "file": file.to_str().unwrap() }),
    );
    let ready = async {
        wait_for_start_markers(&[foundation_started.as_path(), discard_started.as_path()]).await;
        let foundation = register_test_worker(port, "removal", "foundation");
        let discard = register_test_worker(port, "removal", "discard");
        wait_for_worker_state(&daemon, "removal", "foundation", true).await;
        wait_for_worker_state(&daemon, "removal", "discard", true).await;
        wait_for_start_markers(&[keep_started.as_path()]).await;
        let keep = register_test_worker(port, "removal", "keep");
        wait_for_worker_state(&daemon, "removal", "keep", true).await;
        (foundation, keep, discard)
    };
    let (up, (foundation, keep, discard)) = tokio::join!(up, ready);
    let up = up.expect("compose::up should answer");
    assert_eq!(up["status"], "ok", "project did not start: {up}");

    let before = call(
        port,
        "compose::status",
        json!({ "file": file.to_str().unwrap() }),
    )
    .await
    .expect("status before remove");
    assert_eq!(
        operation_containers(&before),
        vec!["discard", "foundation", "keep"],
        "initial up used the wrong worker set: {before}"
    );
    let pid = |status: &Value, key: &str| {
        status["containers"]
            .as_array()
            .expect("containers")
            .iter()
            .find(|container| container["container"] == key)
            .unwrap_or_else(|| panic!("missing {key}: {status}"))["pid"]
            .as_u64()
            .unwrap_or_else(|| panic!("missing pid for {key}: {status}"))
    };
    let keep_pid = pid(&before, "keep");
    let discard_pid = pid(&before, "discard");

    let result = call(
        port,
        "compose::remove",
        json!({
            "file": file.to_str().unwrap(),
            "worker": "foundation",
        }),
    )
    .await
    .expect("compose::remove should answer");

    assert_eq!(result["status"], "ok", "{result}");
    assert_eq!(result["worker"], "foundation", "{result}");
    assert_eq!(result["changed"], true, "{result}");
    for internal in ["containers", "down", "restarted", "up", "operation_id"] {
        assert!(
            result.get(internal).is_none(),
            "mutation leaked {internal}: {result}"
        );
    }

    let edited = std::fs::read_to_string(&file).expect("read edited compose file");
    assert!(
        edited.contains("  keep:"),
        "kept worker was removed: {edited}"
    );
    assert!(
        !edited.contains("start_after: [foundation]"),
        "dependency edge survived: {edited}"
    );
    assert!(
        !edited.contains("  foundation:"),
        "named worker survived: {edited}"
    );

    let after = call(
        port,
        "compose::status",
        json!({ "file": file.to_str().unwrap() }),
    )
    .await
    .expect("status after remove");
    assert_eq!(
        pid(&after, "keep"),
        keep_pid,
        "dependent restarted: {after}"
    );
    assert_eq!(
        pid(&after, "discard"),
        discard_pid,
        "unrelated worker restarted: {after}"
    );
    assert!(
        after["containers"]
            .as_array()
            .expect("containers")
            .iter()
            .all(|container| container["container"] != "foundation"),
        "removed worker remains declared: {after}"
    );

    let later_up = call(
        port,
        "compose::up",
        json!({ "file": file.to_str().unwrap() }),
    )
    .await
    .expect("up after remove should still answer");
    assert_eq!(later_up["status"], "ok", "{later_up}");
    assert_eq!(
        later_up["changed"], false,
        "later up moved workers: {later_up}"
    );

    let final_status = call(
        port,
        "compose::status",
        json!({ "file": file.to_str().unwrap() }),
    )
    .await
    .expect("final status");
    assert_eq!(pid(&final_status, "keep"), keep_pid);
    assert_eq!(pid(&final_status, "discard"), discard_pid);

    foundation.shutdown_async().await;
    keep.shutdown_async().await;
    discard.shutdown_async().await;
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
    assert_eq!(result["error"]["code"], "STARTUP_TIMEOUT");
    assert!(
        result.get("containers").is_none(),
        "mutation leaked internals: {result}"
    );

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

/// `required: false` moves the blast radius of a failed start from the whole
/// operation to the one container that declared it, and a dependent starts on
/// the same declaration: `start_after` is a start order, not a claim that the
/// dependent cannot run without it.
#[tokio::test(flavor = "multi_thread")]
async fn a_container_that_is_not_required_fails_alone_and_its_dependent_still_starts() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    let tmp = tempfile::tempdir().unwrap();
    let api_started = tmp.path().join("workers/api/started");
    let file = project(
        tmp.path(),
        r#"
namespace: optional
startup_timeout: 2s
stop_timeout: 100ms
containers:
  mailer:
    worker: path://./workers/mailer
    required: false
    scripts:
      run: "sleep 30"
  api:
    worker: path://./workers/api
    start_after: [mailer]
    scripts:
      run: "touch started && sleep 30"
"#,
        &["mailer", "api"],
    );

    // `mailer` never registers, so it times out. `api` waits behind it and is
    // started anyway once that failure is recorded.
    let up = call(
        port,
        "compose::up",
        json!({ "file": file.to_str().unwrap() }),
    );
    let ready = async {
        wait_for_start_markers(&[api_started.as_path()]).await;
        let api = register_test_worker(port, "optional", "api");
        wait_for_worker_state(&daemon, "optional", "api", true).await;
        api
    };
    let (up, api) = tokio::join!(up, ready);
    let up = up.expect("compose::up should answer");

    assert_eq!(
        up["status"], "ok",
        "a container that is not required must not fail the operation: {up}"
    );
    assert_eq!(
        up["not_required_failures"],
        json!(["mailer"]),
        "the return has to name what is down under an ok: {up}"
    );
    assert_eq!(
        up["error"]["code"], "STARTUP_TIMEOUT",
        "the reason for the contained failure is still reported: {up}"
    );

    // Rollback is what `required: true` buys, so nothing may have been undone.
    let status = call(
        port,
        "compose::status",
        json!({ "file": file.to_str().unwrap() }),
    )
    .await
    .expect("status after a contained failure");
    let state = |key: &str| {
        status["containers"]
            .as_array()
            .expect("containers")
            .iter()
            .find(|container| container["container"] == key)
            .unwrap_or_else(|| panic!("missing {key}: {status}"))["state"]
            .clone()
    };
    assert_eq!(state("api"), "ready", "dependent was rolled back: {status}");
    assert_ne!(
        state("mailer"),
        "ready",
        "the failed container must not report ready: {status}"
    );

    api.shutdown_async().await;
    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_continue_from_a_cursor_after_the_worker_is_ready() {
    isolate_state();
    let port = spawn_engine().await;
    let daemon = start_daemon(port).await;

    let tmp = tempfile::tempdir().unwrap();
    let started = tmp.path().join("workers/queue/started");
    let emit = tmp.path().join("workers/queue/emit-after-ready");
    let file = project(
        tmp.path(),
        r#"
namespace: worker-logs
startup_timeout: 3s
stop_timeout: 100ms
containers:
  queue:
    worker: path://./workers/queue
    scripts:
      run: |
        touch started
        printf '%s\n' 'output before ready'
        while [ ! -f emit-after-ready ]; do sleep 0.02; done
        printf '%s\n' 'output after ready'
        printf '%s\n' 'error after ready' >&2
        sleep 30
"#,
        &["queue"],
    );

    let up = call(
        port,
        "compose::up",
        json!({ "file": file.to_str().unwrap() }),
    );
    let ready = async {
        wait_for_start_markers(&[started.as_path()]).await;
        let worker = register_test_worker(port, "worker-logs", "queue");
        wait_for_worker_state(&daemon, "worker-logs", "queue", true).await;
        worker
    };
    let (up, worker) = tokio::join!(up, ready);
    let up = up.expect("compose::up should answer");
    assert_eq!(up["status"], "ok", "project did not start: {up}");

    let before = call(
        port,
        "compose::logs",
        json!({
            "file": file.to_str().unwrap(),
            "container": "queue",
            "tail": 10,
            "wait_ms": 1_000,
        }),
    )
    .await
    .expect("compose::logs should return startup output");
    assert_eq!(
        before["containers"][0]["entries"][0]["message"], "output before ready",
        "startup output was not retained: {before}"
    );
    let cursor = before["containers"][0]["cursor"].clone();

    std::fs::write(&emit, "now").expect("release worker output");
    let after = call(
        port,
        "compose::logs",
        json!({
            "file": file.to_str().unwrap(),
            "container": "queue",
            "cursors": { "queue": cursor },
            "tail": 10,
            "wait_ms": 2_000,
        }),
    )
    .await
    .expect("compose::logs should continue after its cursor");
    let entries = after["containers"][0]["entries"]
        .as_array()
        .expect("log entries");
    assert!(
        entries.iter().any(|entry| {
            entry["stream"] == "stdout" && entry["message"] == "output after ready"
        }),
        "stdout after readiness was not returned: {after}"
    );
    assert!(
        entries.iter().any(|entry| {
            entry["stream"] == "stderr" && entry["message"] == "error after ready"
        }),
        "stderr after readiness was not returned: {after}"
    );

    worker.shutdown_async().await;
    call(
        port,
        "compose::down",
        json!({ "file": file.to_str().unwrap() }),
    )
    .await
    .expect("compose::down should stop the fixture");
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
    assert_eq!(result["error"]["code"], "CONFIG_FETCH_FAILED", "{result}");
    assert!(
        result.get("containers").is_none(),
        "mutation leaked internals: {result}"
    );
    // Not mistaken for a first boot, which is the case that must proceed.
    let status = call(
        port,
        "compose::status",
        json!({ "file": file.to_str().unwrap() }),
    )
    .await
    .expect("status after failed up");
    assert_ne!(status["containers"][0]["state"], "ready", "{status}");

    daemon.shutdown().await;
}
