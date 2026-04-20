use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use iii::EngineBuilder;
use iii::workers::config::{EngineConfig, WorkerEntry};
use iii::workers::reload::{ReloadDiff, ReloadManager, RunningWorker, WorkerRegistrations};
use iii::workers::traits::Worker;

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
        "workers:\n  - name: foo\n    config:\n      port: 1\n  - name: foo\n    config:\n      port: 2\nmodules: []\n",
    );
    let entries = ReloadManager::parse_and_normalize(f.path().to_str().unwrap())
        .await
        .expect("duplicates should get instance IDs");

    let foo_entries: Vec<_> = entries
        .iter()
        .filter(|e| e.name.starts_with("foo"))
        .collect();
    assert_eq!(foo_entries.len(), 2, "both foo entries should be preserved");
    assert_eq!(foo_entries[0].name, "foo");
    assert_eq!(foo_entries[1].name, "foo#1");
    assert_eq!(foo_entries[0].config, Some(serde_json::json!({"port": 1})));
    assert_eq!(foo_entries[1].config, Some(serde_json::json!({"port": 2})));
    // worker_type strips the #N suffix for factory lookup
    assert_eq!(foo_entries[0].worker_type(), "foo");
    assert_eq!(foo_entries[1].worker_type(), "foo");
}

fn minimal_config() -> EngineConfig {
    serde_yaml::from_str("workers: []\nmodules: []\n").unwrap()
}

#[tokio::test]
async fn commit_noop_when_diff_is_empty() {
    let mut builder = EngineBuilder::new()
        .with_config(minimal_config())
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
async fn user_defined_workers_are_preserved() {
    let f = write_config("workers:\n  - name: my::CustomUserWorker\nmodules: []\n");
    let entries = ReloadManager::parse_and_normalize(f.path().to_str().unwrap())
        .await
        .expect("normalize should succeed");
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"my::CustomUserWorker"));
    assert!(names.contains(&"iii-engine-functions")); // mandatory still injected
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

// -----------------------------------------------------------------------------
// Test helpers: a bare-minimum Worker that reports a configurable is_alive
// value so we can drive promote_dead_unchanged deterministically.
// -----------------------------------------------------------------------------

struct TestWorker {
    alive: AtomicBool,
}

#[async_trait::async_trait]
impl Worker for TestWorker {
    fn name(&self) -> &'static str {
        "test-worker"
    }

    async fn create(
        _engine: Arc<iii::engine::Engine>,
        _config: Option<serde_json::Value>,
    ) -> anyhow::Result<Box<dyn Worker>>
    where
        Self: Sized,
    {
        Err(anyhow::anyhow!("TestWorker::create not used in unit tests"))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
}

fn running_worker(name: &str, alive: bool) -> RunningWorker {
    let entry = WorkerEntry {
        name: name.to_string(),
        image: None,
        config: None,
    };
    let worker: Arc<dyn Worker> = Arc::new(TestWorker {
        alive: AtomicBool::new(alive),
    });
    let (shutdown_tx, _) = tokio::sync::watch::channel(false);
    RunningWorker {
        entry,
        worker,
        shutdown_tx,
        registrations: WorkerRegistrations::default(),
    }
}

#[tokio::test]
async fn promote_dead_unchanged_leaves_live_workers_alone() {
    let running = vec![running_worker("alive-worker", true)];
    let new_entries = vec![WorkerEntry {
        name: "alive-worker".into(),
        image: None,
        config: None,
    }];
    let mut diff = ReloadDiff {
        unchanged: vec!["alive-worker".into()],
        ..Default::default()
    };

    let promoted = ReloadManager::promote_dead_unchanged(&mut diff, &new_entries, &running).await;

    assert_eq!(promoted, Vec::<String>::new(), "nothing should be promoted");
    assert_eq!(diff.unchanged, vec!["alive-worker".to_string()]);
    assert!(diff.changed.is_empty());
}

#[tokio::test]
async fn promote_dead_unchanged_revives_dead_worker() {
    // This is the regression test for `iii worker add ./w --force` not
    // restarting the VM: the CLI rewrites config.yaml with the same entry,
    // the diff says unchanged, but the tracked worker is dead. The reloader
    // must promote it to `changed` so commit() destroys + restarts.
    let running = vec![
        running_worker("alive-worker", true),
        running_worker("dead-worker", false),
    ];
    let new_entries = vec![
        WorkerEntry {
            name: "alive-worker".into(),
            image: None,
            config: None,
        },
        WorkerEntry {
            name: "dead-worker".into(),
            image: None,
            config: None,
        },
    ];
    let mut diff = ReloadDiff {
        unchanged: vec!["alive-worker".into(), "dead-worker".into()],
        ..Default::default()
    };

    let promoted = ReloadManager::promote_dead_unchanged(&mut diff, &new_entries, &running).await;

    assert_eq!(promoted, vec!["dead-worker".to_string()]);
    assert_eq!(diff.unchanged, vec!["alive-worker".to_string()]);
    assert_eq!(diff.changed.len(), 1);
    assert_eq!(diff.changed[0].name, "dead-worker");
}

#[tokio::test]
async fn promote_dead_unchanged_skips_entries_without_a_tracked_worker() {
    // Defensive: if diff.unchanged contains a name not in `running`, we
    // cannot assess liveness. Leave it alone rather than dropping it.
    let running: Vec<RunningWorker> = Vec::new();
    let new_entries = vec![WorkerEntry {
        name: "orphan".into(),
        image: None,
        config: None,
    }];
    let mut diff = ReloadDiff {
        unchanged: vec!["orphan".into()],
        ..Default::default()
    };

    let promoted = ReloadManager::promote_dead_unchanged(&mut diff, &new_entries, &running).await;

    assert!(promoted.is_empty());
    assert_eq!(diff.unchanged, vec!["orphan".to_string()]);
    assert!(diff.changed.is_empty());
}
