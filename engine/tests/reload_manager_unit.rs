use std::io::Write;

use iii::EngineBuilder;
use iii::workers::config::{EngineConfig, WorkerEntry};
use iii::workers::reload::{ReloadDiff, ReloadManager};

fn write_config(contents: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().expect("tempfile");
    f.write_all(contents.as_bytes()).expect("write");
    f
}

#[tokio::test]
async fn parse_error_is_reported() {
    let f = write_config("workers: [not: valid: yaml");
    let path = f.path().to_str().unwrap();
    let result = ReloadManager::parse_and_normalize(path).await;
    assert!(result.is_err(), "expected parse error, got {:?}", result.ok());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("reload: parse failed"),
        "error should mention 'reload: parse failed', was: {}",
        err
    );
}

#[tokio::test]
async fn empty_config_injects_all_mandatory_workers() {
    let f = write_config("workers: []\nmodules: []\n");
    let entries = ReloadManager::parse_and_normalize(f.path().to_str().unwrap())
        .await
        .expect("normalize should succeed");

    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    // These four workers are registered mandatory in the inventory.
    assert!(names.contains(&"iii-engine-functions"), "missing iii-engine-functions, got: {:?}", names);
    assert!(names.contains(&"iii-worker-manager"), "missing iii-worker-manager, got: {:?}", names);
    assert!(names.contains(&"iii-telemetry"), "missing iii-telemetry, got: {:?}", names);
    assert!(names.contains(&"iii-observability"), "missing iii-observability, got: {:?}", names);
}

#[tokio::test]
async fn duplicate_worker_name_rejected() {
    let f = write_config(
        "workers:\n  - name: foo\n  - name: foo\nmodules: []\n",
    );
    let result = ReloadManager::parse_and_normalize(f.path().to_str().unwrap()).await;
    assert!(result.is_err(), "expected duplicate-name rejection, got {:?}", result.ok());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("duplicate worker name"),
        "error should mention duplicate worker name, was: {}",
        err
    );
}

// Reuse the same minimal-config pattern that the other test files use to
// avoid port collisions with parallel tests.
fn minimal_config() -> EngineConfig {
    // No user workers; mandatory ones will auto-inject and none of them bind
    // fixed ports.
    serde_yaml::from_str("workers: []\nmodules: []\n").unwrap()
}

#[tokio::test]
async fn validation_succeeds_with_empty_diff() {
    let builder = EngineBuilder::new()
        .with_config(minimal_config())
        .build()
        .await
        .unwrap();

    let diff = ReloadDiff::default(); // nothing to add or change
    let staged = ReloadManager::validate_staging(
        &diff,
        builder.engine_handle(),
        builder.registry_handle(),
    )
    .await
    .expect("empty diff should validate");

    assert!(staged.is_empty());
}

#[tokio::test]
async fn validation_fails_on_unknown_worker_name() {
    let builder = EngineBuilder::new()
        .with_config(minimal_config())
        .build()
        .await
        .unwrap();

    // A built-in worker name with an `image` field is rejected by
    // `WorkerRegistry::create_worker` — this gives us a deterministic
    // creation failure without touching the network or spawning processes.
    let diff = ReloadDiff {
        added: vec![WorkerEntry {
            name: "iii-telemetry".to_string(),
            image: Some("not/a-real-image:latest".to_string()),
            config: None,
        }],
        removed: Vec::new(),
        changed: Vec::new(),
        unchanged: Vec::new(),
    };

    let result = ReloadManager::validate_staging(
        &diff,
        builder.engine_handle(),
        builder.registry_handle(),
    )
    .await;

    let err = match result {
        Ok(_) => panic!("expected validation failure for bad worker entry"),
        Err(e) => format!("{}", e),
    };
    assert!(
        err.contains("reload: validation failed"),
        "error should mention 'reload: validation failed', was: {}",
        err
    );
    assert!(
        err.contains("iii-telemetry"),
        "error should mention the failing worker name, was: {}",
        err
    );
}

#[tokio::test]
async fn commit_noop_when_diff_is_empty() {
    let mut builder = EngineBuilder::new()
        .with_config(minimal_config())
        .build()
        .await
        .unwrap();

    let diff = ReloadDiff::default();
    let staged = ReloadManager::validate_staging(
        &diff,
        builder.engine_handle(),
        builder.registry_handle(),
    )
    .await
    .unwrap();

    let before: Vec<String> = builder
        .running()
        .iter()
        .map(|rw| rw.entry.name.clone())
        .collect();

    ReloadManager::commit(
        &diff,
        staged,
        builder.engine_handle(),
        builder.running_mut(),
    )
    .await
    .expect("empty-diff commit should succeed");

    let after: Vec<String> = builder
        .running()
        .iter()
        .map(|rw| rw.entry.name.clone())
        .collect();

    assert_eq!(before, after, "empty commit must leave the running set unchanged");
}

#[tokio::test]
async fn user_defined_workers_are_preserved() {
    // The config uses a name that is NOT a builtin so we don't trigger strict
    // validation; the goal is just to verify the name passes through normalize
    // alongside the auto-injected mandatory workers.
    let f = write_config(
        "workers:\n  - name: my::CustomUserWorker\nmodules: []\n",
    );
    let entries = ReloadManager::parse_and_normalize(f.path().to_str().unwrap())
        .await
        .expect("normalize should succeed");
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"my::CustomUserWorker"));
    assert!(names.contains(&"iii-engine-functions")); // mandatory still injected
}
