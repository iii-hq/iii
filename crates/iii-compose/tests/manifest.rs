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
        "namespace: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n",
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
        "namespace: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n    scripts:\n      run: ./dev.sh\n",
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
        "namespace: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n    scripts:\n      run: ./dev.sh\n",
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
        "namespace: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n",
        &[("workers/api", None)],
    );

    let err = start_of(&file, "api").expect_err("a container with no start command is invalid");
    assert_eq!(err.code(), "MISSING_START_COMMAND");
    let message = err.to_string();
    assert!(message.contains("run:"), "{message}");
    assert!(message.contains("iii.worker.yaml"), "{message}");
}

/// The container key wins over the manifest's `name`, the same way `run` wins
/// over `scripts.start`. This used to be `MANIFEST_NAME_MISMATCH`, which
/// refused a configuration that works: the key reaches the child as
/// `III_WORKER_NAME`, so a worker honouring the reserved contract registers
/// under it whatever its own manifest declares. An operator deploying a worker
/// they did not write could not rename it without editing a vendored file.
#[test]
fn the_container_key_wins_over_the_manifest_name() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        "namespace: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n",
        &[(
            "workers/api",
            Some("name: orders-api\nscripts:\n  start: cargo run\n"),
        )],
    );

    // It resolves rather than failing, and the manifest still supplies the
    // start command it is there for.
    assert_eq!(
        start_of(&file, "api").expect("a differently-named manifest is not an error"),
        StartSpec::Shell("cargo run".to_string())
    );

    // And the name the child is told is the compose key, not the manifest's.
    let report = iii_compose::manifest::validate_offline(&file, "orders-abcd1234")
        .expect("project should validate");
    assert_eq!(report.resolved[0].key, "api");
}

#[test]
fn a_manifest_without_a_name_inherits_the_container_key() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        "namespace: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n",
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
        "namespace: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n    scripts:\n      run: ./dev.sh\n",
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
        "namespace: orders\ncontainers:\n  api:\n    worker: package://workers.iii.dev/api\n    version: \"1.0.0\"\n",
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
namespace: orders
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

/// A reserved key inside an `env_file` used to survive `validate` and surface
/// during `start_one`, by which point earlier containers in the graph were
/// running and had to be rolled back. The same key written under
/// `environment:` failed at parse time, so where the error appeared depended
/// on which of the two forms the operator chose.
#[test]
fn a_reserved_key_in_an_env_file_fails_validation() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("api.env"),
        "DATABASE_URL=postgres://localhost/app\nIII_NAMESPACE=somewhere-else\n",
    )
    .unwrap();

    let file = project(
        tmp.path(),
        r#"
namespace: orders
containers:
  api:
    worker: path://./workers/api
    env_file:
      - ./api.env
"#,
        &[("workers/api", Some(MANIFEST))],
    );

    let err = iii_compose::manifest::validate_offline(&file, "orders-abcd1234")
        .expect_err("a reserved key must not reach `up`");
    assert_eq!(err.code(), "RESERVED_ENV_OVERRIDE");
    assert!(
        err.to_string().contains("III_NAMESPACE"),
        "the error should name the key: {err}"
    );
}

/// The same rule for a `package://` container. Env-file checks used to sit
/// after the branch that defers packages, so a registry worker was exempt from
/// all of them — including the missing-file check, whose whole point is to
/// fail before anything starts.
#[test]
fn a_package_container_gets_the_same_env_file_checks() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("queue.env"),
        "III_URL=ws://elsewhere:1234\n",
    )
    .unwrap();

    let file = project(
        tmp.path(),
        r#"
namespace: orders
containers:
  queue:
    worker: package://workers.iii.dev/queue
    version: "0.1.0"
    env_file:
      - ./queue.env
"#,
        &[],
    );

    let err = iii_compose::manifest::validate_offline(&file, "orders-abcd1234")
        .expect_err("deferring the package must not defer its env files");
    assert_eq!(err.code(), "RESERVED_ENV_OVERRIDE");

    // And the missing-file case, which was exempt for the same reason.
    let file = project(
        tmp.path(),
        r#"
namespace: orders
containers:
  queue:
    worker: package://workers.iii.dev/queue
    version: "0.1.0"
    env_file:
      - ./not-here.env
"#,
        &[],
    );
    let err = iii_compose::manifest::validate_offline(&file, "orders-abcd1234")
        .expect_err("a missing env file must fail before anything starts");
    assert_eq!(err.code(), "MISSING_ENV_FILE");
}

/// The check reads the file, so the ordinary case has to keep passing: a
/// plain env file with no reserved key validates and starts nothing early.
#[test]
fn an_ordinary_env_file_still_validates() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("api.env"),
        "# a comment\nexport DATABASE_URL=\"postgres://localhost/app\"\nLOG_LEVEL=debug\n",
    )
    .unwrap();

    let file = project(
        tmp.path(),
        r#"
namespace: orders
containers:
  api:
    worker: path://./workers/api
    env_file:
      - ./api.env
"#,
        &[("workers/api", Some(MANIFEST))],
    );

    let report = iii_compose::manifest::validate_offline(&file, "orders-abcd1234")
        .expect("an ordinary env file should validate");
    assert_eq!(report.resolved.len(), 1);
    assert_eq!(report.resolved[0].env_file.len(), 1);
}

/// Waves are what makes a start parallel: everything in one has nothing to
/// wait for inside it. A project where one worker calls the other three is two
/// waves, not four steps.
#[test]
fn independent_containers_share_a_wave() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        r#"
namespace: orders
containers:
  a:
    worker: package://workers.iii.dev/a
    version: "1.0.0"
  b:
    worker: package://workers.iii.dev/b
    version: "1.0.0"
  c:
    worker: package://workers.iii.dev/c
    version: "1.0.0"
  hub:
    worker: package://workers.iii.dev/hub
    version: "1.0.0"
    depends_on: [a, b, c]
"#,
        &[],
    );

    let order = iii_compose::dag::topo_order(&file).expect("a graph without cycles");
    let waves = iii_compose::dag::waves(&file, &order);
    assert_eq!(waves.len(), 2, "expected two waves, got {waves:?}");
    assert_eq!(waves[0], vec!["a", "b", "c"], "{waves:?}");
    assert_eq!(waves[1], vec!["hub"], "{waves:?}");
}

/// And a chain cannot be flattened: each link waits for the one before it.
#[test]
fn a_chain_is_one_container_per_wave() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        r#"
namespace: orders
containers:
  first:
    worker: package://workers.iii.dev/first
    version: "1.0.0"
  second:
    worker: package://workers.iii.dev/second
    version: "1.0.0"
    depends_on: [first]
  third:
    worker: package://workers.iii.dev/third
    version: "1.0.0"
    depends_on: [second]
"#,
        &[],
    );

    let order = iii_compose::dag::topo_order(&file).expect("a graph without cycles");
    let waves = iii_compose::dag::waves(&file, &order);
    assert_eq!(waves.len(), 3, "{waves:?}");
    for wave in &waves {
        assert_eq!(wave.len(), 1, "a chain cannot overlap: {waves:?}");
    }
}

/// A partial order — what `up container=x` plans — must still draw every
/// container it holds. A parent outside the order is never walked, so treating
/// it as one would drop the dependency under it.
#[test]
fn a_partial_order_still_outlines_everything_in_it() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        r#"
namespace: orders
containers:
  database:
    worker: package://workers.iii.dev/database
    version: "1.0.0"
  api:
    worker: package://workers.iii.dev/api
    version: "1.0.0"
    depends_on: [database]
"#,
        &[],
    );

    // `database` alone: its dependent is not being started.
    let outline = iii_compose::dag::outline(&file, &["database".to_string()]);
    assert_eq!(
        outline,
        vec![("database".to_string(), 0)],
        "the container in the order must be drawn: {outline:?}"
    );
}
