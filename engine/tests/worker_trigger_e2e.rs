// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end test for the `worker` custom trigger type registered by
//! `iii-worker-ops`. Boots the engine in-process via `EngineBuilder::serve()`,
//! spawns the `worker_manager_daemon` in a tokio task (the daemon is a plain
//! `async fn`, no subprocess needed), connects an SDK client that subscribes
//! to `worker` with three different filter configs, fires `worker::add iii-http`,
//! and asserts the subscriber gets exactly the lifecycle events the filter
//! permits.
//!
//! `iii-http` is the canonical hermetic target: it is an engine builtin
//! baked into the iii binary (see
//! `crates/iii-worker/src/cli/managed.rs` "engine" branch), so the add path
//! writes only `iii.config.yaml` + `iii.lock` inside the test's tempdir and
//! never downloads a real artifact. `III_API_URL` is pointed at a dead
//! address so the best-effort telemetry ping fails fast.

use std::path::PathBuf;
use std::time::Duration;

use iii::EngineBuilder;
use iii::workers::config::EngineConfig;
use iii_sdk::{
    III, InitOptions, RegisterFunction, RegisterTriggerInput, TriggerRequest, register_worker,
};
use iii_worker::cli::app::WorkerManagerDaemonArgs;
use iii_worker::cli::worker_manager_daemon;
use serde_json::{Value, json};
use serial_test::serial;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Set process-global env vars exactly once. The test mutates env so it must
/// be `#[serial]`; the `Once` keeps this idempotent if other tests in the same
/// file ever land. Mirrors the pattern in `engine/tests/config_reload_e2e.rs`.
fn set_test_env(project_root: &std::path::Path) {
    static SET: std::sync::Once = std::sync::Once::new();
    SET.call_once(|| {
        // Safety: this runs once before any code in the test reads env;
        // the test is `#[serial]` so no parallel reader exists.
        unsafe {
            // Suppress engine auto-injection of `iii-worker-ops`; we spawn
            // the daemon in-process below, so we don't want a phantom
            // subprocess fighting it.
            std::env::set_var("IIIWORKER_DISABLE_BUILTIN_DAEMONS", "1");
            // Best-effort telemetry POST during `worker::add iii-http` would
            // otherwise resolve `api.workers.iii.dev`. Point it at a closed
            // local port so the 2s timeout fails immediately and never
            // touches the network.
            std::env::set_var("III_API_URL", "http://127.0.0.1:1");
        }
    });
    // `IIIWORKER_PROJECT_ROOT` is read per-run by the daemon, so we set it
    // unconditionally (cheap, and survives a `cargo test`-driven re-entry
    // with a different tempdir). The daemon's `WorkerManagerDaemonArgs` also
    // carries `project_root` explicitly; we mirror it into env as a belt-
    // and-suspenders so any deeper `handle_managed_*` code path that reads
    // env directly still lands in the sandbox.
    unsafe {
        std::env::set_var("IIIWORKER_PROJECT_ROOT", project_root);
    }
}

/// RAII guard that restores the process CWD when dropped. `handle_managed_add`
/// resolves config writes relative to CWD; the test changes CWD into the
/// tempdir for the duration of the run, then this guard puts CWD back so a
/// subsequent `#[serial]` test starts from a clean slate.
struct CwdGuard {
    prev: PathBuf,
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        if let Err(e) = std::env::set_current_dir(&self.prev) {
            eprintln!(
                "warn: failed to restore CWD to {prev:?}: {e}",
                prev = self.prev
            );
        }
    }
}

/// Reserve an ephemeral port for the engine WS server. Binds, reads the
/// port, drops the listener so the engine can bind to it. Same approach as
/// `engine/tests/otel_ws_no_worker_registration_test.rs`.
async fn pick_free_port() -> u16 {
    let probe = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind probe socket");
    let port = probe.local_addr().expect("local_addr").port();
    drop(probe);
    port
}

/// Block until the engine's WS server accepts TCP. 5s deadline; panics on
/// timeout so the test fails cleanly instead of hanging on the SDK retry
/// loop later.
async fn wait_for_ws(port: u16) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("engine WS server did not bind to 127.0.0.1:{port} within 5s");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Block until the daemon has registered `worker::add` with the engine.
/// Polls `engine::functions::list` (filtered by prefix) every 100ms with a
/// 10s deadline. Without this gate the test could race the daemon's WS
/// connection and `iii.trigger("worker::add", ...)` would return
/// `function_not_found`.
async fn wait_for_worker_add_function(probe: &III) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let resp = probe
            .trigger(TriggerRequest {
                function_id: "engine::functions::list".into(),
                payload: json!({ "prefix": "worker::", "include_internal": true }),
                action: None,
                timeout_ms: Some(2000),
            })
            .await;
        if let Ok(value) = resp
            && let Some(functions) = value.get("functions").and_then(|v| v.as_array())
            && functions
                .iter()
                .any(|f| f.get("function_id").and_then(|v| v.as_str()) == Some("worker::add"))
        {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("worker::add not registered with engine within 10s");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Write a minimal `iii.config.yaml` that pins `iii-worker-manager` to the
/// chosen ephemeral port. `EngineBuilder::build()` auto-injects mandatory
/// daemons (`iii-worker-manager`, `iii-telemetry`, `iii-engine-functions`,
/// `iii-http-functions`) but `iii-worker-manager`'s port has to be explicit
/// — without that, the WS server lands on `DEFAULT_PORT` (49134) and
/// collides with any other test or running engine on the dev box.
fn minimal_iii_config_yaml(ws_port: u16) -> String {
    format!(
        "workers:\n  - name: iii-worker-manager\n    config:\n      host: 127.0.0.1\n      port: {ws_port}\nmodules: []\n"
    )
}

/// Convenience holder for a single subscription side-channel — the mpsc
/// receiver that captures every payload routed to the subscriber. One per
/// filter scenario.
struct Subscriber {
    rx: mpsc::UnboundedReceiver<Value>,
}

/// Register a named handler that pushes every invocation into an mpsc, then
/// bind a `worker` trigger with `filter` against the same id. Returns the
/// receiver so the test can drain it post-fire.
fn register_subscriber(iii: &III, function_id: &str, filter: Value) -> Subscriber {
    let (tx, rx) = mpsc::unbounded_channel::<Value>();
    let tx_for_handler = tx.clone();
    iii.register_function(
        function_id,
        RegisterFunction::new_async(move |req: Value| {
            let tx = tx_for_handler.clone();
            async move {
                let _ = tx.send(req);
                Ok::<_, iii_sdk::IIIError>(json!({}))
            }
        })
        .description("e2e test subscriber"),
    );
    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "worker".into(),
        function_id: function_id.to_string(),
        config: filter,
        metadata: None,
    })
    .expect("register worker trigger");
    Subscriber { rx }
}

/// Drain `rx` until a `stage == "done"` event arrives (success path) or the
/// deadline expires. Always returns whatever was collected so the caller can
/// distinguish between "wrong stage chain" and "nothing arrived".
async fn collect_until_done(rx: &mut mpsc::UnboundedReceiver<Value>) -> Vec<Value> {
    let mut out = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return out;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(event)) => {
                let is_done = event.get("stage").and_then(|v| v.as_str()) == Some("done");
                out.push(event);
                if is_done {
                    return out;
                }
            }
            Ok(None) => return out,
            Err(_) => return out,
        }
    }
}

/// Drain whatever is in `rx` right now (no waiting). Used by the
/// non-matching-filter assertion to confirm zero events arrived.
fn drain_immediate(rx: &mut mpsc::UnboundedReceiver<Value>) -> Vec<Value> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        out.push(event);
    }
    out
}

// ---------------------------------------------------------------------------
// the test
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn worker_trigger_fires_add_lifecycle_events_to_subscribers() {
    // 1. Tempdir sandbox + env.
    let tempdir = TempDir::new().expect("create tempdir");
    let project_root: PathBuf = tempdir.path().to_path_buf();
    set_test_env(&project_root);

    // The daemon's `handle_managed_add` writes to CWD, not to the
    // `project_root` we pass in `WorkerManagerDaemonArgs` (the project
    // root is used for `ProjectCtx::open` for locking; the actual file
    // I/O still resolves relative to CWD). To keep all on-disk
    // mutations inside the tempdir — and so the test can read them
    // back deterministically — switch CWD before booting the engine.
    // Restored at scope exit so a parallel `#[serial]` test isn't
    // affected.
    let prev_cwd = std::env::current_dir().expect("read cwd");
    std::env::set_current_dir(&project_root).expect("set cwd to tempdir");
    let _cwd_guard = CwdGuard { prev: prev_cwd };

    // 2. Reserve the WS port and write the project config inside the sandbox.
    let ws_port = pick_free_port().await;
    let ws_url = format!("ws://127.0.0.1:{ws_port}");
    let config_path = project_root.join("iii.config.yaml");
    std::fs::write(&config_path, minimal_iii_config_yaml(ws_port)).expect("write config");

    // 3. Boot the engine in-process. `EngineBuilder` auto-injects mandatory
    //    daemons (telemetry, engine-functions, http-functions); the config
    //    only needs to nail down the WS port.
    let cfg = EngineConfig::config_file(config_path.to_str().expect("utf-8 path"))
        .expect("load engine config");
    let builder = EngineBuilder::new()
        .with_config(cfg)
        .build()
        .await
        .expect("build engine");
    let engine_handle = tokio::spawn(async move { builder.serve().await });

    // 4. Wait for the WS server to come up. Without this gate the daemon's
    //    `register_worker` retries silently and the rest of the test runs
    //    against an empty engine.
    wait_for_ws(ws_port).await;

    // 5. Spawn the daemon in-process. The daemon's `run` parks on
    //    `tokio::signal::ctrl_c()` after registering everything; abort the
    //    join handle in cleanup to drop the future and tear down the SDK
    //    connection thread.
    let daemon_args = WorkerManagerDaemonArgs {
        engine: ws_url.clone(),
        project_root: Some(project_root.clone()),
    };
    let daemon_handle = tokio::spawn(async move { worker_manager_daemon::run(daemon_args).await });

    // 6. Probe client waits until the daemon's `worker::add` shows up in
    //    `engine::functions::list`. This proves the daemon connected,
    //    registered its trigger type, and registered every `worker::*`
    //    function — i.e. the trigger surface is ready to drive.
    let probe = register_worker(&ws_url, InitOptions::default());
    wait_for_worker_add_function(&probe).await;

    // 7. Subscriber + driver share one III client. Three subscriptions
    //    exercise the filter matrix:
    //    - `add_subscriber` (operations:["add"]) sees every stage of the
    //      `worker::add iii-http` lifecycle.
    //    - `downloaded_subscriber` (stages:["downloaded"]) sees exactly
    //      one event.
    //    - `remove_subscriber` (operations:["remove"]) sees zero events.
    let test_client = register_worker(&ws_url, InitOptions::default());

    let mut add_subscriber = register_subscriber(
        &test_client,
        "test::on_worker_event::all",
        json!({ "operations": ["add"] }),
    );
    let mut downloaded_subscriber = register_subscriber(
        &test_client,
        "test::on_worker_event::downloaded",
        json!({ "stages": ["downloaded"] }),
    );
    let mut remove_subscriber = register_subscriber(
        &test_client,
        "test::on_worker_event::remove",
        json!({ "operations": ["remove"] }),
    );

    // The `register_trigger` message is fire-and-forget — give the daemon
    // a moment to route each `RegisterTrigger` through its
    // `WorkerTriggerHandler` before the driver fires. 500ms is generous
    // (the SDK's WS flush is sub-millisecond on localhost).
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 8. Drive `worker::add iii-http`. `force: true` + `reset_config: true`
    //    + `wait: false` keep the call short and ensure the orchestrator
    //    runs end-to-end even on re-runs that left state behind. We don't
    //    actually unwrap the response — only that the call returns Ok.
    let add_result = test_client
        .trigger(TriggerRequest {
            function_id: "worker::add".into(),
            payload: json!({
                "source": { "kind": "registry", "name": "iii-http" },
                "force": true,
                "reset_config": true,
                "wait": false,
            }),
            action: None,
            // `worker::add` has a 600s recommended timeout per
            // `op_metadata`; this test's iii-http path is hermetic so 60s
            // is plenty without making CI flake budget large.
            timeout_ms: Some(60_000),
        })
        .await
        .expect("worker::add trigger should succeed");
    assert_eq!(
        add_result.get("name").and_then(|v| v.as_str()),
        Some("iii-http"),
        "worker::add response should resolve canonical name iii-http: {add_result}"
    );

    // 9. Collect events from the wildcard-on-add subscriber until `done`
    //    arrives (or the deadline trips).
    let add_events = collect_until_done(&mut add_subscriber.rx).await;

    // 10. Stage-sequence assertions — every Started/Downloading/Downloaded/
    //     Done event for the iii-http add must land on add_subscriber.
    //     The producer-side ordering (single dispatcher mpsc in
    //     `IIIEventSink`) guarantees emit-order on the wire, but the
    //     subscriber-side `handle_invoke_function` spawns each handler
    //     task independently, so we can't rely on positional order in
    //     the captured Vec. We assert on stage SET membership plus
    //     timestamp-based ordering for stages with distinct timestamps.
    assert!(
        add_events.len() >= 4,
        "expected at least 4 events on add_subscriber (started/downloading/downloaded/done), \
         got {n}: {add_events:#?}",
        n = add_events.len()
    );

    let stages: std::collections::HashSet<&str> = add_events
        .iter()
        .filter_map(|e| e.get("stage").and_then(|s| s.as_str()))
        .collect();
    for required in ["started", "downloading", "downloaded", "done"] {
        assert!(
            stages.contains(required),
            "missing required stage {required:?} in {add_events:#?}"
        );
    }

    for event in &add_events {
        assert_eq!(
            event["operation"], "add",
            "all events should carry operation=add: {event}"
        );
        assert_eq!(
            event["worker"], "iii-http",
            "all events should carry worker=iii-http: {event}"
        );
        assert_eq!(
            event["caller_mode"], "trigger",
            "daemon-driven path always emits caller_mode=trigger: {event}"
        );
    }

    // 11. Filter-matrix assertions — the new `WorkerTriggerConfig` filter
    //     fields (operations / stages / workers) are the load-bearing piece
    //     of this change. Confirm that:
    //     - a stages-filtered subscriber receives exactly the `downloaded`
    //       event and nothing else;
    //     - an operations-filtered subscriber for a different op receives
    //       nothing at all.
    //
    // The filtered subscribers never see `done` (their filters exclude it),
    // so `collect_until_done` would block until the deadline. Instead, give
    // the engine a brief settle window for the fan-out spawned tasks to
    // deliver — by the time `add_subscriber` saw `done`, all sink-spawned
    // `iii.trigger(...)` calls for this `add` have been issued, but FIFO
    // is only guaranteed within a single function id. 250ms covers the
    // localhost WS round-trip with margin.
    tokio::time::sleep(Duration::from_millis(250)).await;

    let downloaded_events = drain_immediate(&mut downloaded_subscriber.rx);
    assert_eq!(
        downloaded_events.len(),
        1,
        "stages:[downloaded] subscriber should receive exactly one event, got {n}: {downloaded_events:#?}",
        n = downloaded_events.len()
    );
    assert_eq!(downloaded_events[0]["stage"], "downloaded");
    assert_eq!(downloaded_events[0]["operation"], "add");
    assert_eq!(downloaded_events[0]["worker"], "iii-http");

    let remove_events = drain_immediate(&mut remove_subscriber.rx);
    assert!(
        remove_events.is_empty(),
        "operations:[remove] subscriber should receive zero events from an `add`, got: {remove_events:#?}"
    );

    // 12. Side-effect assertions — proves the trigger path produced the
    //     same on-disk artifacts the CLI path would.
    //
    // The daemon picks its config file via CWD: `iii.config.yaml`
    // (canonical) takes precedence, otherwise `config.yaml` (legacy).
    // Our engine config sits in `iii.config.yaml`; `handle_managed_add`
    // tends to write to `config.yaml` when nothing forces it otherwise,
    // so we accept either as long as ONE of them lists `iii-http`.
    let canonical = project_root.join("iii.config.yaml");
    let legacy = project_root.join("config.yaml");
    let canonical_content = std::fs::read_to_string(&canonical).unwrap_or_default();
    let legacy_content = std::fs::read_to_string(&legacy).unwrap_or_default();
    assert!(
        canonical_content.contains("iii-http") || legacy_content.contains("iii-http"),
        "neither iii.config.yaml nor config.yaml in {project_root:?} lists iii-http after a \
         successful add. canonical:\n{canonical_content}\nlegacy:\n{legacy_content}"
    );
    // The lockfile path is project-scoped `iii.lock`.
    let lockfile_path = project_root.join("iii.lock");
    assert!(
        lockfile_path.exists(),
        "iii.lock should be written next to the config after a successful add (looked at {lockfile_path:?})"
    );

    // 13. Cleanup. Graceful shutdown for the SDK clients (joins their
    //     connection threads); abort for the engine + daemon tasks (their
    //     futures are blocked on accept/select forever). Tempdir drops at
    //     scope exit.
    drop(add_subscriber);
    drop(downloaded_subscriber);
    drop(remove_subscriber);
    test_client.shutdown_async().await;
    probe.shutdown_async().await;
    daemon_handle.abort();
    let _ = daemon_handle.await;
    engine_handle.abort();
    let _ = engine_handle.await;
}
