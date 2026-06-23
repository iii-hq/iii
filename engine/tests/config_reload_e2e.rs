// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end integration tests for the file-watch-driven config reload
//! pipeline.
//!
//! These tests spawn `EngineBuilder::serve()` in a background task, modify the
//! config file on disk, and assert that the reload machinery detects the change
//! and behaves as expected.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use iii::EngineBuilder;
use iii::engine::Engine;
use iii::function::{Function, FunctionResult};
use iii::workers::config::EngineConfig;
use iii::workers::traits::Worker;
use serde_json::Value;
use serial_test::serial;

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Suppress the `iii-worker-ops` auto-injection. The reload pipeline spawns
/// fresh `EngineBuilder::serve()` instances per test; the injected daemon's
/// child process keeps listeners alive past the test's tokio runtime
/// shutdown, which makes the next test in the file panic with `AddrInUse`
/// when it tries to rebind. Tests don't exercise `worker::*` triggers, so
/// they don't need the daemon — set the opt-out before any builder runs.
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

/// A minimal YAML config with no user-defined workers or modules. Mandatory
/// workers (telemetry, observability, engine-functions) are auto-injected by
/// `EngineBuilder::build()` and do not bind fixed ports.
fn minimal_config_yaml() -> &'static str {
    "workers: []\nmodules: []\n"
}

/// Write `contents` to `path` synchronously.
fn write_config(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write config file");
}

/// Poll `config.yaml` at `path` until it contains `needle` (the seed-strip
/// breadcrumb), then return the rewritten contents. Panics after a generous
/// deadline. The poll replaces a fixed sleep so slow CI runners (notably
/// coverage instrumentation) don't flake when the strip pass runs slower than
/// any constant wait — the file is written atomically, so once the breadcrumb
/// is present the whole rewrite is.
async fn wait_for_config_rewrite(path: &Path, needle: &str) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let contents = std::fs::read_to_string(path).unwrap_or_default();
        if contents.contains(needle) {
            return contents;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for config rewrite to contain {needle:?}; last content:\n{contents}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
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

// ---------------------------------------------------------------------------
// TestEphemeralWorker
// ---------------------------------------------------------------------------

/// A minimal worker that registers a single known function ID when
/// `register_functions` is called. Used to verify that removing a worker from
/// the config cleans up its registrations in `Engine.functions`.
struct TestEphemeralWorker;

const TEST_EPHEMERAL_WORKER_NAME: &str = "test::EphemeralReloadWorker";
const TEST_EPHEMERAL_FUNCTION_ID: &str = "test::EphemeralReloadWorker::handler";

#[async_trait]
impl Worker for TestEphemeralWorker {
    fn name(&self) -> &'static str {
        "TestEphemeralWorker"
    }

    async fn create(
        _engine: Arc<Engine>,
        _config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Worker>> {
        Ok(Box::new(TestEphemeralWorker))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        engine.functions.register_function(
            TEST_EPHEMERAL_FUNCTION_ID.to_string(),
            make_dummy_function(TEST_EPHEMERAL_FUNCTION_ID),
        );
    }
}

// ---------------------------------------------------------------------------
// Valid config change: engine keeps running
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn config_change_reloads_without_crashing() {
    disable_builtin_daemons();
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let path = tmp.path().to_path_buf();
    write_config(&path, minimal_config_yaml());

    let cfg = EngineConfig::config_file(path.to_str().unwrap()).expect("load initial config");

    let builder = EngineBuilder::new()
        .with_config(cfg)
        .with_config_path(path.to_str().unwrap())
        .build()
        .await
        .expect("build engine");

    let handle = tokio::spawn(async move { builder.serve().await });

    // Let serve() spawn workers and start the file watcher.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Rewrite the config with a trivially-different-but-equivalent body.
    // The file watcher detects the change, debounces 500ms, then reloads.
    write_config(&path, "workers: []\nmodules: []\n# reload trigger\n");

    // Wait for watcher debounce (500ms) + reload pipeline.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    assert!(
        !handle.is_finished(),
        "serve() should still be running after a valid config reload"
    );

    handle.abort();
    let _ = handle.await;

    drop(tmp);
}

// ---------------------------------------------------------------------------
// Broken YAML: engine exits with error
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn broken_yaml_config_exits_engine() {
    disable_builtin_daemons();
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let path = tmp.path().to_path_buf();
    write_config(&path, minimal_config_yaml());

    let cfg = EngineConfig::config_file(path.to_str().unwrap()).expect("load initial config");

    let builder = EngineBuilder::new()
        .with_config(cfg)
        .with_config_path(path.to_str().unwrap())
        .build()
        .await
        .expect("build engine");

    let handle = tokio::spawn(async move { builder.serve().await });

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Corrupt the config. The engine must exit with an error describing
    // the parse failure.
    write_config(&path, "this: is: not: [valid yaml");

    // serve() must exit within 3 seconds (500ms debounce + reload + teardown).
    let result = tokio::time::timeout(Duration::from_secs(3), handle).await;
    assert!(
        result.is_ok(),
        "serve() did not exit within 3s of broken config write"
    );

    let serve_result = result.unwrap().expect("join");
    assert!(
        serve_result.is_err(),
        "serve() should return Err on broken config reload"
    );
    let err_msg = format!("{}", serve_result.unwrap_err());
    assert!(
        err_msg.contains("parse failed"),
        "error should mention parse failure, got: {}",
        err_msg
    );

    drop(tmp);
}

// ---------------------------------------------------------------------------
// Removing a worker from config cleans up its registrations
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn config_reload_removes_worker_function_registrations() {
    disable_builtin_daemons();
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let path = tmp.path().to_path_buf();

    // Start with the ephemeral worker declared in the config so build() will
    // instantiate it and record its function registration in the engine.
    let initial_yaml = format!(
        "workers:\n  - name: {}\nmodules: []\n",
        TEST_EPHEMERAL_WORKER_NAME
    );
    write_config(&path, &initial_yaml);

    let cfg = EngineConfig::config_file(path.to_str().unwrap()).expect("load initial config");

    let builder = EngineBuilder::new()
        .register_worker::<TestEphemeralWorker>(TEST_EPHEMERAL_WORKER_NAME)
        .with_config(cfg)
        .with_config_path(path.to_str().unwrap())
        .build()
        .await
        .expect("build engine");

    // Grab an Arc<Engine> handle before serve() consumes the builder so we
    // can inspect `engine.functions` across the reload boundary.
    let engine = builder.engine_handle();

    // Sanity: the worker's function must be present after build().
    assert!(
        engine.functions.get(TEST_EPHEMERAL_FUNCTION_ID).is_some(),
        "expected '{}' to be registered after build()",
        TEST_EPHEMERAL_FUNCTION_ID
    );

    let handle = tokio::spawn(async move { builder.serve().await });

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Rewrite the config with the worker removed.
    write_config(&path, minimal_config_yaml());

    // Wait for watcher debounce (500ms) + reload pipeline.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    assert!(
        !handle.is_finished(),
        "serve() should still be running after reload that removed a worker"
    );

    assert!(
        engine.functions.get(TEST_EPHEMERAL_FUNCTION_ID).is_none(),
        "expected '{}' to be removed from engine.functions after reload",
        TEST_EPHEMERAL_FUNCTION_ID
    );

    handle.abort();
    let _ = handle.await;

    drop(tmp);
}

// ---------------------------------------------------------------------------
// First-boot seed strips the worker's config: block from config.yaml and moves
// the value into the configuration store.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn first_seed_strips_config_block_and_moves_value_to_store() {
    disable_builtin_daemons();

    // The configuration worker persists value-only YAML into this temp dir so we
    // can assert the seed landed there. iii-pubsub's `local` adapter binds no
    // ports and writes no side-effect files, so it is a clean seeding subject.
    let store = tempfile::tempdir().expect("store dir");
    let store_dir = store.path().to_str().unwrap().to_string();

    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let path = tmp.path().to_path_buf();

    let yaml = format!(
        "workers:
  - name: configuration
    config:
      adapter:
        name: fs
        config:
          directory: {store_dir}
      ttl_seconds: 0
  - name: iii-pubsub
    config:
      adapter:
        name: local
modules: []
"
    );
    write_config(&path, &yaml);

    let cfg = EngineConfig::config_file(path.to_str().unwrap()).expect("load config");
    let builder = EngineBuilder::new()
        .with_config(cfg)
        .with_config_path(path.to_str().unwrap())
        .build()
        .await
        .expect("build engine");
    let handle = tokio::spawn(async move { builder.serve().await });

    // serve() runs the serial boot loop (pubsub seeds), then the strip pass
    // rewrites config.yaml — all before the file watcher is created. Poll for
    // the breadcrumb instead of a fixed sleep; the poll IS the assertion that
    // the seed block was stripped and the breadcrumb comment written.
    let rewritten = wait_for_config_rewrite(&path, "iii config set iii-pubsub").await;

    // Seed block gone, replaced by the breadcrumb comment; entry kept; the
    // non-seeding configuration block left intact.
    assert!(
        rewritten.contains(&format!("at {store_dir}/iii-pubsub.yaml")),
        "comment should point at the store location, got:\n{rewritten}"
    );
    assert!(
        !rewritten.contains("name: local"),
        "pubsub seed block should be stripped, got:\n{rewritten}"
    );
    assert!(
        rewritten.contains("- name: iii-pubsub"),
        "the worker entry itself must be kept, got:\n{rewritten}"
    );
    assert!(
        rewritten.contains(&format!("directory: {store_dir}")),
        "the configuration worker block must be left intact, got:\n{rewritten}"
    );

    // The value moved into the configuration store (value-only YAML).
    let stored = std::fs::read_to_string(store.path().join("iii-pubsub.yaml"))
        .expect("store file should exist");
    assert!(
        stored.contains("local"),
        "stored value should hold the seeded adapter, got:\n{stored}"
    );

    handle.abort();
    let _ = handle.await;

    drop(tmp);
}

// ---------------------------------------------------------------------------
// A value already persisted in the store (from a prior boot) means the
// config.yaml block is being ignored — it must still get stripped, even though
// nothing is seeded this boot.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn already_persisted_value_also_strips_stale_config_block() {
    disable_builtin_daemons();

    let store = tempfile::tempdir().expect("store dir");
    let store_dir = store.path().to_str().unwrap().to_string();

    // Simulate a prior boot: the store already holds iii-pubsub's value (empty
    // config = default adapter). The config.yaml block below is therefore being
    // ignored at runtime — it must still get stripped.
    std::fs::write(
        store.path().join("iii-pubsub.yaml"),
        "id: iii-pubsub\nname: PubSub\ndescription: pre-existing from a prior boot\nvalue: {}\n",
    )
    .expect("pre-seed store");

    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let path = tmp.path().to_path_buf();

    let yaml = format!(
        "workers:
  - name: configuration
    config:
      adapter:
        name: fs
        config:
          directory: {store_dir}
      ttl_seconds: 0
  - name: iii-pubsub
    config:
      adapter:
        name: local
modules: []
"
    );
    write_config(&path, &yaml);

    let cfg = EngineConfig::config_file(path.to_str().unwrap()).expect("load config");
    let builder = EngineBuilder::new()
        .with_config(cfg)
        .with_config_path(path.to_str().unwrap())
        .build()
        .await
        .expect("build engine");
    let handle = tokio::spawn(async move { builder.serve().await });

    let rewritten = wait_for_config_rewrite(&path, "iii config set iii-pubsub").await;
    assert!(
        rewritten.contains(&format!("at {store_dir}/iii-pubsub.yaml")),
        "comment should point at the store location, got:\n{rewritten}"
    );
    assert!(
        !rewritten.contains("name: local"),
        "stale pubsub block should be stripped even when not seeding, got:\n{rewritten}"
    );

    // The pre-existing stored value was NOT overwritten by the config.yaml block
    // — proving this was the already-persisted path, not a re-seed.
    let stored = std::fs::read_to_string(store.path().join("iii-pubsub.yaml"))
        .expect("store file should exist");
    assert!(
        stored.contains("value: {}"),
        "stored value must stay the pre-existing empty config, not the config.yaml block, got:\n{stored}"
    );

    handle.abort();
    let _ = handle.await;

    drop(tmp);
}

// ---------------------------------------------------------------------------
// External workers (e.g. `shell`) register over the bus and never call the
// engine-side register_config. The strip is store-driven, so once their value
// is in the store their stale config.yaml block is removed too — proven here
// with a non-builtin worker that registers nothing in-process.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn external_worker_block_stripped_from_store_value_alone() {
    disable_builtin_daemons();

    let store = tempfile::tempdir().expect("store dir");
    let store_dir = store.path().to_str().unwrap().to_string();

    // Value already in the store from a prior boot, for a worker that never
    // calls the engine-side register_config (the external-worker case).
    std::fs::write(
        store.path().join("fake-ext.yaml"),
        "id: fake-ext\nname: Fake\ndescription: external stand-in\nvalue:\n  some_key: stored\n",
    )
    .expect("pre-seed store");

    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let path = tmp.path().to_path_buf();
    let yaml = format!(
        "workers:
  - name: configuration
    config:
      adapter:
        name: fs
        config:
          directory: {store_dir}
      ttl_seconds: 0
  - name: fake-ext
    config:
      some_key: from_config_yaml
modules: []
"
    );
    write_config(&path, &yaml);

    let cfg = EngineConfig::config_file(path.to_str().unwrap()).expect("load config");
    let builder = EngineBuilder::new()
        .register_worker::<TestEphemeralWorker>("fake-ext")
        .with_config(cfg)
        .with_config_path(path.to_str().unwrap())
        .build()
        .await
        .expect("build engine");
    let handle = tokio::spawn(async move { builder.serve().await });

    let rewritten = wait_for_config_rewrite(&path, "iii config set fake-ext").await;
    assert!(
        rewritten.contains(&format!("at {store_dir}/fake-ext.yaml")),
        "comment should point at the store location, got:\n{rewritten}"
    );
    assert!(
        !rewritten.contains("from_config_yaml"),
        "stale external block should be gone, got:\n{rewritten}"
    );
    assert!(
        rewritten.contains("- name: fake-ext"),
        "the worker entry itself must be kept, got:\n{rewritten}"
    );

    handle.abort();
    let _ = handle.await;

    drop(tmp);
}
