use std::io::Write;

use iii::EngineBuilder;
use iii::workers::config::{EngineConfig, WorkerEntry};
use iii::workers::reload::{ReloadDiff, ReloadManager, diff_entries};
use serde_json::json;

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
    assert!(
        result.is_err(),
        "expected parse error, got {:?}",
        result.ok()
    );
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
    assert!(
        names.contains(&"iii-engine-functions"),
        "missing iii-engine-functions, got: {:?}",
        names
    );
    assert!(
        names.contains(&"iii-worker-manager"),
        "missing iii-worker-manager, got: {:?}",
        names
    );
    assert!(
        names.contains(&"iii-telemetry"),
        "missing iii-telemetry, got: {:?}",
        names
    );
    assert!(
        names.contains(&"iii-observability"),
        "missing iii-observability, got: {:?}",
        names
    );
}

#[tokio::test]
async fn duplicate_worker_names_get_instance_ids() {
    let f = write_config(
        "workers:\n  - name: iii-stream\n    config:\n      port: 1\n  - name: iii-stream\n    config:\n      port: 2\nmodules: []\n",
    );
    let entries = ReloadManager::parse_and_normalize(f.path().to_str().unwrap())
        .await
        .expect("duplicates should get instance IDs");

    let stream_entries: Vec<_> = entries
        .iter()
        .filter(|e| e.name.starts_with("iii-stream"))
        .collect();
    assert_eq!(
        stream_entries.len(),
        2,
        "both iii-stream entries should be preserved"
    );
    assert_eq!(stream_entries[0].name, "iii-stream");
    assert_eq!(stream_entries[1].name, "iii-stream#1");
    assert_eq!(
        stream_entries[0].config,
        Some(serde_json::json!({"port": 1}))
    );
    assert_eq!(
        stream_entries[1].config,
        Some(serde_json::json!({"port": 2}))
    );
    // worker_type strips the #N suffix for factory lookup
    assert_eq!(stream_entries[0].worker_type(), "iii-stream");
    assert_eq!(stream_entries[1].worker_type(), "iii-stream");
}

async fn parse(yaml: &str) -> Vec<WorkerEntry> {
    let f = write_config(yaml);
    ReloadManager::parse_and_normalize(f.path().to_str().unwrap())
        .await
        .expect("normalize should succeed")
}

#[tokio::test]
async fn reload_keeps_sandbox_binding() {
    let yaml = "sandbox:\n  adapter: fake\n  namespace: fake-ns\nworkers: []\n";
    let (first, second) = (parse(yaml).await, parse(yaml).await);
    let router = first
        .iter()
        .find(|e| e.name == "iii-sandbox")
        .expect("router injected");
    assert_eq!(
        router.config,
        Some(json!({"adapter": "fake", "namespace": "fake-ns"}))
    );
    assert!(
        diff_entries(&first, &second)
            .unchanged
            .contains(&"iii-sandbox".to_string())
    );
    let unbound = parse("workers: []\n").await;
    assert_eq!(
        unbound
            .iter()
            .find(|e| e.name == "iii-sandbox")
            .unwrap()
            .config,
        None
    );
}

#[tokio::test]
async fn changed_sandbox_binding_marks_router_changed() {
    let diff = diff_entries(
        &parse("sandbox:\n  adapter: fake\n").await,
        &parse("sandbox:\n  adapter: other\n").await,
    );
    assert_eq!(
        diff.changed
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        vec!["iii-sandbox"]
    );
}

/// Minimal config whose `configuration` worker persists into a throwaway
/// tempdir, so the engine boot writes nothing into the repo-relative
/// `engine/config/`. Keep the TempDir alive for the test's duration.
fn minimal_config() -> (EngineConfig, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create config tempdir");
    let mut config: EngineConfig = serde_yaml::from_str("workers: []\nmodules: []\n").unwrap();
    config.workers.push(iii::workers::config::WorkerEntry {
        name: "configuration".to_string(),
        config: Some(serde_json::json!({
            "adapter": {
                "name": "fs",
                "config": { "directory": dir.path().to_string_lossy() }
            }
        })),
    });
    (config, dir)
}

#[tokio::test]
async fn commit_noop_when_diff_is_empty() {
    let (config, _config_dir) = minimal_config();
    let mut builder = EngineBuilder::new()
        .with_config(config)
        .build()
        .await
        .unwrap();

    let diff = ReloadDiff::default();

    let before: Vec<String> = builder
        .running()
        .iter()
        .map(|rw| rw.entry.name.clone())
        .collect();

    let (global_shutdown_tx, _global_shutdown_rx) = tokio::sync::watch::channel(false);
    ReloadManager::commit(
        &diff,
        builder.engine_handle(),
        builder.registry_handle(),
        builder.running_mut(),
        global_shutdown_tx,
    )
    .await
    .expect("empty-diff commit should succeed");

    let after: Vec<String> = builder
        .running()
        .iter()
        .map(|rw| rw.entry.name.clone())
        .collect();

    assert_eq!(
        before, after,
        "empty commit must leave the running set unchanged"
    );
}

#[tokio::test]
async fn user_defined_workers_are_rejected_with_compose_guidance() {
    let f = write_config("workers:\n  - name: my::CustomUserWorker\nmodules: []\n");
    let error = ReloadManager::parse_and_normalize(f.path().to_str().unwrap())
        .await
        .expect_err("user-managed workers must move to worker-compose.yaml");
    let message = error.to_string();
    assert!(message.contains("my::CustomUserWorker"));
    assert!(message.contains("worker-compose.yaml"));
    assert!(message.contains("https://iii.dev/docs/upgrading/workers-to-compose"));
}

#[test]
fn enforce_guards_refuses_to_remove_mandatory_workers() {
    let diff = ReloadDiff {
        added: Vec::new(),
        removed: vec!["iii-engine-functions".to_string()],
        changed: Vec::new(),
        unchanged: Vec::new(),
    };

    let result = ReloadManager::enforce_guards(&diff);
    assert!(
        result.is_err(),
        "expected refusal to remove mandatory worker"
    );
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("refused to remove mandatory worker"),
        "error should mention mandatory refusal, was: {}",
        err
    );
    assert!(
        err.contains("iii-engine-functions"),
        "error should mention worker name, was: {}",
        err
    );
}

#[test]
fn enforce_guards_allows_removing_non_mandatory_workers() {
    let diff = ReloadDiff {
        added: Vec::new(),
        removed: vec!["some::CustomUserWorker".to_string()],
        changed: Vec::new(),
        unchanged: Vec::new(),
    };

    assert!(ReloadManager::enforce_guards(&diff).is_ok());
}

#[test]
fn enforce_guards_allows_empty_diff() {
    assert!(ReloadManager::enforce_guards(&ReloadDiff::default()).is_ok());
}
