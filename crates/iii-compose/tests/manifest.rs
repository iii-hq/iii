//! Manifest reading and start-command precedence, against real directories.

use std::path::Path;

use iii_compose::{ComposeFile, StartSpec};

/// Writes a compose file plus optional worker dirs/manifests into a tempdir and
/// loads it, so path resolution goes through the same code the CLI uses.
fn project(tmp: &Path, compose: &str, workers: &[(&str, Option<&str>)]) -> ComposeFile {
    for (dir, manifest) in workers {
        let worker_dir = tmp.join(dir);
        std::fs::create_dir_all(&worker_dir).unwrap();
        if let Some(manifest) = manifest {
            std::fs::write(worker_dir.join("iii.worker.yaml"), manifest).unwrap();
        }
    }
    let path = tmp.join("worker-compose.yaml");
    std::fs::write(&path, compose).unwrap();
    ComposeFile::load(&path).expect("compose file should parse")
}

fn start_of(file: &ComposeFile, key: &str) -> Result<StartSpec, iii_compose::ComposeError> {
    iii_compose::manifest::resolve_start(key, &file.containers[key])
}

const MANIFEST: &str = r#"
name: api
runtime: rust
scripts:
  start: cargo run --release
  install: cargo build
"#;

#[test]
fn manifest_start_is_used_when_compose_has_no_run() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        "name: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n",
        &[("workers/api", Some(MANIFEST))],
    );

    assert_eq!(
        start_of(&file, "api").unwrap(),
        StartSpec::Shell("cargo run --release".to_string())
    );
}

#[test]
fn compose_run_wins_over_the_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        "name: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n    scripts:\n      run: ./dev.sh\n",
        &[("workers/api", Some(MANIFEST))],
    );

    assert_eq!(
        start_of(&file, "api").unwrap(),
        StartSpec::Shell("./dev.sh".to_string())
    );
}

#[test]
fn run_alone_is_enough_without_a_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        "name: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n    scripts:\n      run: ./dev.sh\n",
        &[("workers/api", None)],
    );

    assert_eq!(
        start_of(&file, "api").unwrap(),
        StartSpec::Shell("./dev.sh".to_string())
    );
}

#[test]
fn no_manifest_and_no_run_names_both_ways_out() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        "name: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n",
        &[("workers/api", None)],
    );

    let err = start_of(&file, "api").expect_err("a container with no start command is invalid");
    assert_eq!(err.code(), "MISSING_START_COMMAND");
    let message = err.to_string();
    assert!(message.contains("run:"), "{message}");
    assert!(message.contains("iii.worker.yaml"), "{message}");
}

#[test]
fn a_manifest_may_not_rename_the_container() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        "name: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n",
        &[(
            "workers/api",
            Some("name: orders-api\nscripts:\n  start: cargo run\n"),
        )],
    );

    let err = start_of(&file, "api").expect_err("a renaming manifest is invalid");
    assert_eq!(err.code(), "MANIFEST_NAME_MISMATCH");
}

#[test]
fn a_manifest_without_a_name_inherits_the_container_key() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        "name: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n",
        &[("workers/api", Some("scripts:\n  start: cargo run\n"))],
    );

    assert_eq!(
        start_of(&file, "api").unwrap(),
        StartSpec::Shell("cargo run".to_string())
    );
}

#[test]
fn a_missing_worker_directory_is_reported_before_the_start_command() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        "name: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n    scripts:\n      run: ./dev.sh\n",
        &[],
    );

    let err = start_of(&file, "api").expect_err("a missing worker directory is invalid");
    assert_eq!(err.code(), "MISSING_WORKER_DIRECTORY");
}

#[test]
fn packages_have_no_start_command_until_they_are_installed() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        "name: orders\ncontainers:\n  api:\n    worker: package://workers.iii.dev/api\n    version: \"1.0.0\"\n",
        &[],
    );

    // Installing needs the network, so `lifecycle::start_one` does it and builds
    // the `Exec` itself. Reaching the offline resolver with a package means the
    // install was skipped, not that packages are unsupported.
    let err = start_of(&file, "api").expect_err("a package has no start command yet");
    assert_eq!(err.code(), "PACKAGE_NOT_INSTALLED");
}

#[test]
fn validate_offline_resolves_paths_and_defers_packages() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    depends_on:
      - queue
  queue:
    worker: package://workers.iii.dev/queue
    version: "0.1.0"
"#,
        &[("workers/api", Some(MANIFEST))],
    );

    let report = iii_compose::manifest::validate_offline(&file, "orders-abcd1234")
        .expect("project should validate");

    assert_eq!(report.project, "orders");
    assert_eq!(report.namespace, "orders-abcd1234");
    assert_eq!(report.start_order, vec!["queue", "api"]);
    assert_eq!(report.deferred_packages, vec!["queue"]);
    assert_eq!(report.resolved.len(), 1);

    let api = &report.resolved[0];
    assert_eq!(api.key, "api");
    assert_eq!(
        api.start,
        StartSpec::Shell("cargo run --release".to_string())
    );
    assert_eq!(api.config_name, None);
    // No working_dir declared, so the container runs in its own worker dir.
    assert!(
        api.working_dir.ends_with("workers/api"),
        "unexpected dir: {}",
        api.working_dir.display()
    );
}
