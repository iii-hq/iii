use std::io::Write;

use iii::workers::reload::ReloadManager;

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
