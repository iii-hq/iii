// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! One compose file is one project, even when two calls arrive together.
//!
//! `Project::open` adopts whatever children survived a previous daemon. Two
//! opens of the same file therefore adopt the same PIDs, and the one that
//! loses the race into the cache is still handed to its caller — which can
//! then take down processes the other believes it supervises.

use std::sync::Arc;

use iii_compose::daemon::Daemon;

/// Enough concurrent callers that the window between the cache miss and the
/// insert is hit, rather than hoping two tasks interleave.
const RACERS: usize = 16;

/// Keeps this binary's state out of `~/.iii/compose`, which is a real
/// directory on a real machine: a test that writes there leaves a daemon's
/// worth of state behind on every run.
///
/// The variable is process-wide, so it is written once under `get_or_init`
/// while cargo runs the tests of this binary on parallel threads.
fn isolate_state() {
    static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let root = tempfile::tempdir().expect("state root");
        // SAFETY: `get_or_init` runs this once, before any caller returns.
        unsafe { std::env::set_var("III_COMPOSE_STATE_DIR", root.path()) };
        root
    });
}

/// No engine is started. `Daemon::start` only kicks off a background connect,
/// and loading a project reads the file and the state store rather than the
/// engine, so the cache can be exercised on its own.
fn daemon() -> Arc<Daemon> {
    isolate_state();
    Daemon::start(
        "ws://127.0.0.1:1/ws".to_string(),
        format!("cache-test-{}", std::process::id()),
    )
}

const COMPOSE: &str = r#"
namespace: orders
containers:
  api:
    worker: path://./workers/api
    scripts:
      run: ./api
"#;

/// A compose file next to the worker directory it names. Validation is offline
/// but not blind: it checks that a `path://` worker exists.
fn project_dir() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("workers/api")).unwrap();
    tmp
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn many_calls_naming_one_file_get_one_project() {
    let tmp = project_dir();
    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(&file, COMPOSE).unwrap();

    // Separate tasks on separate threads, released together: `join!` would poll
    // both from one task, and the first would finish loading before the second
    // ever looked at the cache.
    let daemon = daemon();
    let gate = Arc::new(tokio::sync::Barrier::new(RACERS));
    let mut racers = Vec::new();
    for _ in 0..RACERS {
        let (daemon, gate, file) = (Arc::clone(&daemon), Arc::clone(&gate), file.clone());
        racers.push(tokio::spawn(async move {
            gate.wait().await;
            daemon.project(&file).await
        }));
    }

    let mut opened = Vec::new();
    for racer in racers {
        opened.push(racer.await.unwrap().unwrap());
    }
    let first = &opened[0];
    assert!(
        opened.iter().all(|other| Arc::ptr_eq(first, other)),
        "every caller must hold the same project, or each supervises its own copy"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_same_file_reached_twice_is_still_one_project() {
    let tmp = project_dir();
    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(&file, COMPOSE).unwrap();

    let daemon = daemon();
    let first = daemon.project(&file).await.unwrap();
    let second = daemon.project(&file).await.unwrap();

    assert!(Arc::ptr_eq(&first, &second));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_load_that_failed_is_retried_rather_than_cached() {
    let tmp = project_dir();
    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(&file, "containers:\n  api:\n    worker: nonsense\n").unwrap();

    let daemon = daemon();
    assert!(daemon.project(&file).await.is_err());

    // The cell holds no project, so a fixed file loads rather than replaying
    // the error the daemon happened to see first.
    std::fs::write(&file, COMPOSE).unwrap();
    assert!(daemon.project(&file).await.is_ok());
}
