//! Environment resolution: env files, precedence and the reserved contract.

use std::path::Path;

use iii_compose::ComposeFile;

/// Writes a compose file plus env files into a tempdir and loads it, so paths
/// resolve exactly as the CLI resolves them.
fn project(tmp: &Path, compose: &str, files: &[(&str, &str)]) -> ComposeFile {
    for (name, contents) in files {
        let path = tmp.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    let path = tmp.join("worker-compose.yaml");
    std::fs::write(&path, compose).unwrap();
    ComposeFile::load(&path).expect("compose file should parse")
}

const COMPOSE: &str = r#"
namespace: orders
containers:
  api:
    worker: path://./workers/api
    env_file:
      - base.env
      - override.env
    environment:
      RUST_LOG: debug
"#;

#[test]
fn env_files_apply_in_order_and_environment_wins() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        COMPOSE,
        &[
            (
                "base.env",
                "RUST_LOG=info\nDATABASE_URL=postgres://base\nPORT=8080\n",
            ),
            ("override.env", "DATABASE_URL=postgres://override\n"),
        ],
    );

    let env = file.containers["api"].resolve_user_env("api").unwrap();

    assert_eq!(env["PORT"], "8080", "untouched keys survive");
    assert_eq!(
        env["DATABASE_URL"], "postgres://override",
        "a later env_file wins"
    );
    assert_eq!(
        env["RUST_LOG"], "debug",
        "literal environment wins over every env_file"
    );
}

#[test]
fn env_files_tolerate_comments_blanks_quotes_and_export() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        r#"
namespace: orders
containers:
  api:
    worker: path://./workers/api
    env_file:
      - base.env
"#,
        &[(
            "base.env",
            "# a comment\n\nexport TOKEN=\"quoted value\"\nSINGLE='single'\nPLAIN=plain\nMALFORMED\n",
        )],
    );

    let env = file.containers["api"].resolve_user_env("api").unwrap();

    assert_eq!(env["TOKEN"], "quoted value");
    assert_eq!(env["SINGLE"], "single");
    assert_eq!(env["PLAIN"], "plain");
    assert_eq!(env.len(), 3, "comments, blanks and malformed lines skipped");
}

#[test]
fn an_env_file_cannot_shadow_the_reserved_contract() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        r#"
namespace: orders
containers:
  api:
    worker: path://./workers/api
    env_file:
      - base.env
"#,
        &[("base.env", "III_URL=ws://attacker:1\n")],
    );

    let err = file.containers["api"]
        .resolve_user_env("api")
        .expect_err("a reserved key from an env_file must be refused");
    assert_eq!(err.code(), "RESERVED_ENV_OVERRIDE");
}

#[test]
fn a_missing_env_file_fails_validation_before_anything_starts() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("workers/api")).unwrap();
    std::fs::write(
        tmp.path().join("workers/api/iii.worker.yaml"),
        "name: api\nscripts:\n  start: cargo run\n",
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
      - missing.env
"#,
        &[],
    );

    let err = iii_compose::manifest::validate_offline(&file, "orders-test")
        .expect_err("a missing env_file is a validation failure");
    assert_eq!(err.code(), "MISSING_ENV_FILE");
}

#[test]
fn a_container_without_env_resolves_to_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        r#"
namespace: orders
containers:
  api:
    worker: path://./workers/api
"#,
        &[],
    );

    assert!(
        file.containers["api"]
            .resolve_user_env("api")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_container_is_told_which_configuration_entry_is_its_own() {
    // A worker owns a configuration id and hardcodes it, which makes the id a
    // global scarce name: two projects each running `state` share one entry and
    // overwrite each other. Compose tells the container its id instead, so one
    // project can call it `state-finance` and another `state-hr`.
    use iii_compose::manifest::StartSpec;
    use iii_compose::spawn::{SpawnCtx, spawn_plan};

    let user_env = std::collections::BTreeMap::new();
    let start = StartSpec::Shell("true".to_string());
    let plan = spawn_plan(&SpawnCtx {
        engine_url: "ws://127.0.0.1:49134",
        namespace: "finance",
        container_key: "state",
        start: &start,
        config_path: Some(std::path::Path::new("/run/state.yaml")),
        config_name: Some("state-finance"),
        working_dir: std::path::Path::new("."),
        user_env: &user_env,
    });

    assert_eq!(plan.env["III_CONFIG_NAME"], "state-finance");
    // The container key still names the worker. They are different questions:
    // one is what the engine routes to, the other is where the configuration
    // lives — and it is exactly their conflation that made the id global.
    assert_eq!(plan.env["III_WORKER_NAME"], "state");
}

#[test]
fn a_container_without_configuration_is_told_nothing_about_one() {
    // A stale `III_CONFIG_NAME` would point a worker at an entry compose never
    // wrote, which is worse than the absence it replaces.
    use iii_compose::manifest::StartSpec;
    use iii_compose::spawn::{SpawnCtx, spawn_plan};

    let user_env = std::collections::BTreeMap::new();
    let start = StartSpec::Shell("true".to_string());
    let plan = spawn_plan(&SpawnCtx {
        engine_url: "ws://127.0.0.1:49134",
        namespace: "finance",
        container_key: "plain",
        start: &start,
        config_path: None,
        config_name: None,
        working_dir: std::path::Path::new("."),
        user_env: &user_env,
    });

    assert!(!plan.env.contains_key("III_CONFIG_NAME"));
    assert!(!plan.env.contains_key("III_CONFIG"));
}
