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

use iii_compose::daemon::{Daemon, EnginePolicy};

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
        None,
        EnginePolicy::External,
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

#[tokio::test]
async fn explicit_cli_namespace_overrides_the_project_file_namespace() {
    isolate_state();
    let tmp = project_dir();
    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(&file, COMPOSE).unwrap();
    let daemon = Daemon::start(
        "ws://127.0.0.1:1/ws".to_string(),
        "test".to_string(),
        Some("test".to_string()),
        EnginePolicy::External,
    );

    let project = daemon.project(&file).await.unwrap();

    assert_eq!(project.project_namespace, "test");
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

#[tokio::test]
async fn external_daemon_rejects_a_project_that_tries_to_own_an_engine() {
    let tmp = project_dir();
    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(
        &file,
        "engine: { workers: {} }\ncontainers:\n  api:\n    worker: path://./workers/api\n",
    )
    .unwrap();

    let err = match daemon().project(&file).await {
        Err(err) => err,
        Ok(_) => panic!("external daemon must reject engine ownership"),
    };
    assert_eq!(err.code(), "ENGINE_SECTION_REQUIRES_MANAGED_START");
}

#[tokio::test]
async fn explicit_external_engine_overrides_the_owner_file_engine_section() {
    isolate_state();
    let tmp = project_dir();
    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(
        &file,
        "engine: { url: 'ws://ignored:49134', workers: {} }\ncontainers:\n  api:\n    worker: path://./workers/api\n    scripts:\n      run: ./api\n",
    )
    .unwrap();
    let initial = iii_compose::ComposeFile::load(&file).unwrap();
    let daemon = Daemon::start(
        "ws://127.0.0.1:1/ws".to_string(),
        format!("external-override-test-{}", std::process::id()),
        None,
        EnginePolicy::external_overriding(&initial),
    );

    let project = daemon.project(&file).await.unwrap();

    assert_eq!(project.engine_url, "ws://127.0.0.1:1/ws");
}

#[tokio::test]
async fn external_daemon_validation_rejects_a_project_that_owns_an_engine() {
    let tmp = project_dir();
    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(
        &file,
        "engine: { workers: {} }\ncontainers:\n  api:\n    worker: path://./workers/api\n",
    )
    .unwrap();

    let err = daemon()
        .validate(Some(&file))
        .await
        .expect_err("offline validation must enforce engine ownership");
    assert_eq!(err.code(), "ENGINE_SECTION_REQUIRES_MANAGED_START");
}

#[tokio::test]
async fn managed_daemon_requires_restart_when_its_owner_engine_section_changes() {
    isolate_state();
    let tmp = project_dir();
    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(
        &file,
        "engine: { workers: {} }\ncontainers:\n  api:\n    worker: path://./workers/api\n",
    )
    .unwrap();
    let initial = iii_compose::ComposeFile::load(&file).unwrap();
    let policy = EnginePolicy::managed(&initial).unwrap();
    std::fs::write(
        &file,
        "engine:\n  workers:\n    iii-stream: { port: 3112 }\ncontainers:\n  api:\n    worker: path://./workers/api\n",
    )
    .unwrap();

    let daemon = Daemon::start(
        "ws://127.0.0.1:1/ws".to_string(),
        format!("managed-change-test-{}", std::process::id()),
        None,
        policy,
    );
    let err = match daemon.project(&file).await {
        Err(err) => err,
        Ok(_) => panic!("changed engine section must require a restart"),
    };
    assert_eq!(err.code(), "ENGINE_RESTART_REQUIRED");
}

#[tokio::test]
async fn up_rechecks_the_owner_engine_section_after_the_project_is_cached() {
    isolate_state();
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(&file, "engine: { workers: {} }\ncontainers: {}\n").unwrap();

    let initial = iii_compose::ComposeFile::load(&file).unwrap();
    let daemon = Daemon::start(
        "ws://127.0.0.1:1/ws".to_string(),
        format!("managed-cached-change-test-{}", std::process::id()),
        None,
        EnginePolicy::managed(&initial).unwrap(),
    );
    daemon
        .project(&file)
        .await
        .expect("the initial project should load into the cache");

    std::fs::write(
        &file,
        "engine:\n  workers:\n    iii-stream: { port: 3112 }\ncontainers: {}\n",
    )
    .unwrap();

    let err = daemon
        .up(Some(&file), None, "cached-engine-change".to_string())
        .await
        .expect_err("up must reject an engine section changed after the project was cached");
    assert_eq!(err.code(), "ENGINE_RESTART_REQUIRED");
}

#[tokio::test]
async fn managed_daemon_rejects_a_second_engine_owner() {
    isolate_state();
    let tmp = project_dir();
    let owner = tmp.path().join("owner.yaml");
    let other = tmp.path().join("other.yaml");
    let text = "engine: { workers: {} }\ncontainers:\n  api:\n    worker: path://./workers/api\n";
    std::fs::write(&owner, text).unwrap();
    std::fs::write(&other, text).unwrap();
    let initial = iii_compose::ComposeFile::load(&owner).unwrap();

    let daemon = Daemon::start(
        "ws://127.0.0.1:1/ws".to_string(),
        format!("managed-owner-test-{}", std::process::id()),
        None,
        EnginePolicy::managed(&initial).unwrap(),
    );
    let err = match daemon.project(&other).await {
        Err(err) => err,
        Ok(_) => panic!("a second file must not own the same engine"),
    };
    assert_eq!(err.code(), "ENGINE_ALREADY_OWNED");
}

fn managed_mutation_fixture(
    containers: &str,
) -> (tempfile::TempDir, std::path::PathBuf, Arc<Daemon>) {
    isolate_state();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("workers/api")).unwrap();
    std::fs::create_dir_all(tmp.path().join("workers/extra")).unwrap();
    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(
        &file,
        format!(
            "engine: {{ url: 'ws://127.0.0.1:1/ws', workers: {{}} }}\ncontainers:\n{containers}"
        ),
    )
    .unwrap();
    let initial = iii_compose::ComposeFile::load(&file).unwrap();
    let daemon = Daemon::start(
        "ws://127.0.0.1:1/ws".to_string(),
        format!("managed-mutation-test-{}", std::process::id()),
        None,
        EnginePolicy::managed(&initial).unwrap(),
    );
    (tmp, file, daemon)
}

fn change_managed_engine(file: &std::path::Path, containers: &str) -> String {
    let changed = format!(
        "engine:\n  url: ws://127.0.0.1:1/ws\n  workers:\n    iii-stream: {{ port: 3112 }}\ncontainers:\n{containers}"
    );
    std::fs::write(file, &changed).unwrap();
    changed
}

#[tokio::test]
async fn add_rejects_an_engine_change_before_editing_the_file() {
    let (_tmp, file, daemon) =
        managed_mutation_fixture("  api:\n    worker: path://./workers/api\n");
    let changed = change_managed_engine(&file, "  api:\n    worker: path://./workers/api\n");

    let err = daemon
        .add(
            Some(&file),
            &["./workers/extra".to_string()],
            "add-after-engine-change".to_string(),
        )
        .await
        .expect_err("add must reject the changed managed engine");

    assert_eq!(err.code(), "ENGINE_RESTART_REQUIRED");
    assert_eq!(std::fs::read_to_string(file).unwrap(), changed);
}

#[tokio::test]
async fn update_rejects_an_engine_change_before_editing_the_file() {
    let containers =
        "  state:\n    worker: package://api.workers.iii.dev/state\n    version: '1.0.0'\n";
    let (_tmp, file, daemon) = managed_mutation_fixture(containers);
    let changed = change_managed_engine(&file, containers);

    let err = daemon
        .update(
            Some(&file),
            &["state@2.0.0".to_string()],
            "update-after-engine-change".to_string(),
        )
        .await
        .expect_err("update must reject the changed managed engine");

    assert_eq!(err.code(), "ENGINE_RESTART_REQUIRED");
    assert_eq!(std::fs::read_to_string(file).unwrap(), changed);
}

#[tokio::test]
async fn update_accepts_multiple_unchanged_package_workers() {
    let containers = concat!(
        "  state:\n    worker: package://api.workers.iii.dev/state\n    version: '1.0.0'\n",
        "  cache:\n    worker: package://api.workers.iii.dev/cache\n    version: '2.0.0'\n",
    );
    let (_tmp, file, daemon) = managed_mutation_fixture(containers);
    let original = std::fs::read_to_string(&file).unwrap();

    let outcome = daemon
        .update(
            Some(&file),
            &["state@1.0.0".to_string(), "cache@2.0.0".to_string()],
            "batch-update-unchanged".to_string(),
        )
        .await
        .expect("the batch should succeed");
    let outcome = serde_json::to_value(outcome).unwrap();

    assert_eq!(outcome["status"], "ok");
    assert_eq!(outcome["changed"], false);
    assert_eq!(outcome["worker"], "state");
    assert_eq!(outcome["workers"], serde_json::json!(["state", "cache"]));
    assert_eq!(std::fs::read_to_string(file).unwrap(), original);
}

#[tokio::test]
async fn update_rejects_the_batch_before_editing_when_one_worker_is_not_a_package() {
    let containers = concat!(
        "  state:\n    worker: package://api.workers.iii.dev/state\n    version: '1.0.0'\n",
        "  api:\n    worker: path://./workers/api\n",
    );
    let (_tmp, file, daemon) = managed_mutation_fixture(containers);
    let original = std::fs::read_to_string(&file).unwrap();

    let err = daemon
        .update(
            Some(&file),
            &["state@2.0.0".to_string(), "api@2.0.0".to_string()],
            "batch-update-invalid".to_string(),
        )
        .await
        .expect_err("the path worker should reject the whole batch");

    assert_eq!(err.code(), "NOT_A_PACKAGE_CONTAINER");
    assert_eq!(std::fs::read_to_string(file).unwrap(), original);
}

#[tokio::test]
async fn remove_rejects_an_engine_change_before_editing_the_file() {
    let containers = concat!(
        "  api:\n    worker: path://./workers/api\n",
        "  extra:\n    worker: path://./workers/extra\n",
    );
    let (_tmp, file, daemon) = managed_mutation_fixture(containers);
    let changed = change_managed_engine(&file, containers);

    let err = daemon
        .remove(
            Some(&file),
            &["api".to_string()],
            "remove-after-engine-change".to_string(),
        )
        .await
        .expect_err("remove must reject the changed managed engine");

    assert_eq!(err.code(), "ENGINE_RESTART_REQUIRED");
    assert_eq!(std::fs::read_to_string(file).unwrap(), changed);
}

#[tokio::test]
async fn remove_validates_the_edited_file_before_writing_it() {
    let tmp = project_dir();
    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(&file, COMPOSE).unwrap();
    let daemon = daemon();
    let before = std::fs::read_to_string(&file).unwrap();

    let err = daemon
        .remove(
            Some(&file),
            &["api".to_string()],
            "remove-only-container".to_string(),
        )
        .await
        .expect_err("remove must reject an empty edited project");

    assert_eq!(err.code(), "EMPTY_CONTAINERS");
    assert_eq!(std::fs::read_to_string(file).unwrap(), before);
}

#[tokio::test]
async fn remove_validates_every_worker_before_writing_the_batch() {
    let containers = concat!(
        "  api:\n    worker: path://./workers/api\n",
        "  extra:\n    worker: path://./workers/extra\n",
    );
    let (_tmp, file, daemon) = managed_mutation_fixture(containers);
    let before = std::fs::read_to_string(&file).unwrap();

    let err = daemon
        .remove(
            Some(&file),
            &["api".to_string(), "missing".to_string()],
            "remove-invalid-batch".to_string(),
        )
        .await
        .expect_err("an unknown worker must reject the complete removal batch");

    assert_eq!(err.code(), "UNKNOWN_CONTAINER");
    assert_eq!(std::fs::read_to_string(file).unwrap(), before);
}
