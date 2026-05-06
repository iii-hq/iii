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
