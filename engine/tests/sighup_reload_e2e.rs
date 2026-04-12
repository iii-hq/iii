// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! End-to-end integration tests for the SIGHUP-driven config reload pipeline.
//!
//! These tests spawn `EngineBuilder::serve()` in a background task, send a
//! real SIGHUP to the current process after rewriting the config file on disk,
//! and assert that the reload machinery behaves as expected without crashing
//! the engine.
//!
//! SIGHUP's default disposition is to terminate the process. Each test installs
//! a tokio-level SIGHUP handler BEFORE spawning `serve()` and holds it for the
//! full duration of the test so the default disposition never fires (tokio
//! signal handlers are process-global, so `serve()`'s own handler will also
//! observe the signal).

#![cfg(unix)]

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
use tokio::signal::unix::{SignalKind, signal};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// A minimal YAML config with no user-defined workers or modules. Mandatory
/// workers (telemetry, observability, engine-functions) are auto-injected by
/// `EngineBuilder::build()` and do not bind fixed ports.
fn minimal_config_yaml() -> &'static str {
    "workers: []\nmodules: []\n"
}

/// Write `contents` to `path` synchronously. The reload path reads the file
/// synchronously, so an atomic overwrite here is sufficient.
fn write_config(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write config file");
}

/// Installs a process-global SIGHUP handler so the default terminate
/// disposition is suppressed for the duration of the test. The returned
/// receiver must be kept alive until the test finishes.
fn install_sighup_guard() -> tokio::signal::unix::Signal {
    signal(SignalKind::hangup()).expect("install SIGHUP handler")
}

/// Send SIGHUP to the current process.
///
/// SAFETY: `kill(2)` with SIGHUP against our own pid is well-defined. The
/// signal is delivered asynchronously and caught by the SIGHUP handler we
/// installed earlier in the test.
fn raise_sighup() {
    unsafe {
        libc::kill(libc::getpid(), libc::SIGHUP);
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
// Task 13 support: TestEphemeralWorker
// ---------------------------------------------------------------------------

/// A minimal worker that registers a single known function ID when
/// `register_functions` is called. Used to verify that removing a worker from
/// the config via SIGHUP reload cleans up its registrations in
/// `Engine.functions`.
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
// Task 11: happy-path SIGHUP reload must not crash serve()
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn sighup_reload_does_not_crash_engine() {
    let _sighup_guard = install_sighup_guard();

    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let path = tmp.path().to_path_buf();
    write_config(&path, minimal_config_yaml());

    let cfg = EngineConfig::config_file(path.to_str().unwrap())
        .expect("load initial config");

    let builder = EngineBuilder::new()
        .with_config(cfg)
        .with_config_path(path.to_str().unwrap())
        .build()
        .await
        .expect("build engine");

    let handle = tokio::spawn(async move { builder.serve().await });

    // Let serve() spawn workers and enter its select loop.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Rewrite the config with a trivially-different-but-equivalent body. The
    // comment line forces a re-parse without adding workers that would bind
    // real ports.
    write_config(&path, "workers: []\nmodules: []\n# reload trigger\n");

    raise_sighup();

    // Let the reload pipeline run (parse + diff + validate + commit).
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        !handle.is_finished(),
        "serve() should still be running after SIGHUP reload"
    );

    handle.abort();
    let _ = handle.await;

    drop(tmp);
}

// ---------------------------------------------------------------------------
// Task 12: bad YAML must be rejected without crashing serve()
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn sighup_with_broken_yaml_does_not_crash_engine() {
    let _sighup_guard = install_sighup_guard();

    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let path = tmp.path().to_path_buf();
    write_config(&path, minimal_config_yaml());

    let cfg = EngineConfig::config_file(path.to_str().unwrap())
        .expect("load initial config");

    let builder = EngineBuilder::new()
        .with_config(cfg)
        .with_config_path(path.to_str().unwrap())
        .build()
        .await
        .expect("build engine");

    let handle = tokio::spawn(async move { builder.serve().await });

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Corrupt the config. `parse_and_normalize` must log a parse error and
    // leave the running workers untouched.
    write_config(&path, "this: is: not: [valid yaml");

    raise_sighup();

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        !handle.is_finished(),
        "serve() should still be running after SIGHUP with broken YAML"
    );

    handle.abort();
    let _ = handle.await;

    drop(tmp);
}

// ---------------------------------------------------------------------------
// Task 13: removing a worker from the config must clean up its registrations
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn sighup_reload_removes_worker_function_registrations() {
    let _sighup_guard = install_sighup_guard();

    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let path = tmp.path().to_path_buf();

    // Start with the ephemeral worker declared in the config so build() will
    // instantiate it and record its function registration in the engine.
    let initial_yaml = format!(
        "workers:\n  - name: {}\nmodules: []\n",
        TEST_EPHEMERAL_WORKER_NAME
    );
    write_config(&path, &initial_yaml);

    let cfg = EngineConfig::config_file(path.to_str().unwrap())
        .expect("load initial config");

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

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Rewrite the config with the worker removed.
    write_config(&path, minimal_config_yaml());

    raise_sighup();

    // Let the reload pipeline diff, destroy the removed worker, and clear its
    // registrations from Engine.functions.
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        !handle.is_finished(),
        "serve() should still be running after SIGHUP reload that removed a worker"
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
