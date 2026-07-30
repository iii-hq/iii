//! Schema and dependency-graph validation.
//!
//! Every rejection asserts the stable error code, not the prose: the codes are
//! the contract `compose::*` callers match on.

use std::path::PathBuf;

use iii_compose::ComposeFile;

fn parse(text: &str) -> Result<ComposeFile, iii_compose::ComposeError> {
    ComposeFile::parse(text, PathBuf::from("/srv/app/worker-compose.yaml"))
}

fn code(text: &str) -> String {
    parse(text)
        .expect_err("compose file should be rejected")
        .code()
        .to_string()
}

const CANONICAL: &str = r#"
name: orders
containers:
  database:
    worker: package://workers.iii.dev/database
    version: 1.4.2
    config_name: orders-db
  api:
    worker: path://./workers/api
    depends_on:
      - database
    config_uri: worker://configuration/get/orders-api
    config_override:
      server:
        port: 3000
    scripts:
      pre_start: ./scripts/migrate.sh
      pre_start_timeout: 90s
      run: cargo run --release
      post_run: ./scripts/drain.sh
    working_dir: ./workers/api
"#;

#[test]
fn accepts_the_canonical_project() {
    let file = parse(CANONICAL).expect("canonical project should parse");

    assert_eq!(file.name, "orders");
    assert_eq!(file.containers.len(), 2);
    assert_eq!(file.start_order().unwrap(), vec!["database", "api"]);

    let api = &file.containers["api"];
    assert_eq!(api.depends_on, vec!["database".to_string()]);
    assert_eq!(api.config_name.as_deref(), Some("orders-api"));
    assert_eq!(
        api.scripts.pre_start_timeout,
        std::time::Duration::from_secs(90)
    );
    assert_eq!(api.scripts.run.as_deref(), Some("cargo run --release"));
    assert_eq!(api.working_dir, Some(PathBuf::from("/srv/app/workers/api")));

    // config_name and the config_uri alias resolve to the same entry name.
    assert_eq!(
        file.containers["database"].config_name.as_deref(),
        Some("orders-db")
    );
}

#[test]
fn pre_start_timeout_defaults_to_sixty_seconds() {
    let file = parse(
        r#"
name: orders
containers:
  api:
    worker: path://./workers/api
"#,
    )
    .unwrap();

    assert_eq!(
        file.containers["api"].scripts.pre_start_timeout,
        std::time::Duration::from_secs(60)
    );
}

#[test]
fn rejects_an_empty_container_map() {
    assert_eq!(
        code(
            r#"
name: orders
containers: {}
"#
        ),
        "EMPTY_CONTAINERS"
    );
}

#[test]
fn rejects_an_unknown_dependency() {
    assert_eq!(
        code(
            r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    depends_on:
      - databse
"#
        ),
        "UNKNOWN_DEPENDENCY"
    );
}

#[test]
fn rejects_a_self_dependency() {
    assert_eq!(
        code(
            r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    depends_on:
      - api
"#
        ),
        "SELF_DEPENDENCY"
    );
}

#[test]
fn reports_the_cycle_path_in_declaration_order() {
    let err = parse(
        r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    depends_on:
      - queue
  queue:
    worker: path://./workers/queue
    depends_on:
      - database
  database:
    worker: path://./workers/database
    depends_on:
      - api
"#,
    )
    .expect_err("a cycle should be rejected");

    assert_eq!(err.code(), "DEPENDENCY_CYCLE");
    assert_eq!(
        err.to_string(),
        "dependency cycle: api -> queue -> database -> api"
    );
}

#[test]
fn rejects_unknown_fields_at_every_level() {
    let top_level = code(
        r#"
name: orders
hot_reload: true
containers:
  api:
    worker: path://./workers/api
"#,
    );
    let container = code(
        r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    port: 8080
"#,
    );
    let scripts = code(
        r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    scripts:
      start: cargo run
"#,
    );

    assert_eq!(
        [top_level.as_str(), container.as_str(), scripts.as_str()],
        ["INVALID_COMPOSE_FILE"; 3]
    );
}

/// Fields still outside v1. `schema_version` needs a versioning story of its
/// own, `config` inline duplicates `config_override`, and `image://` waits for
/// the OCI runtime phase. This is the tripwire that fails the day one of them is
/// adopted without a decision.
#[test]
fn rejects_fields_still_outside_v1() {
    assert_eq!(
        code(
            r#"
name: orders
schema_version: 1
containers:
  api:
    worker: path://./workers/api
"#
        ),
        "INVALID_COMPOSE_FILE"
    );
    assert_eq!(
        code(
            r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    config:
      a: 1
"#
        ),
        "INVALID_COMPOSE_FILE"
    );
}

#[test]
fn accepts_environment_env_file_and_timeouts() {
    let file = parse(
        r#"
name: orders
startup_timeout: 45s
stop_timeout: 5s
containers:
  api:
    worker: path://./workers/api
    environment:
      RUST_LOG: info
      PORT: "3000"
    env_file:
      - .env
      - ./config/.env.production
    startup_timeout: 90s
"#,
    )
    .expect("environment, env_file and timeouts are part of v1");

    assert_eq!(file.startup_timeout, std::time::Duration::from_secs(45));
    assert_eq!(file.stop_timeout, std::time::Duration::from_secs(5));

    let api = &file.containers["api"];
    assert_eq!(api.environment["RUST_LOG"], "info");
    assert_eq!(api.environment["PORT"], "3000");
    assert_eq!(
        api.env_file,
        vec![
            PathBuf::from("/srv/app/.env"),
            PathBuf::from("/srv/app/config/.env.production"),
        ],
        "env files resolve against the compose directory, in declared order"
    );
    assert_eq!(
        api.startup_timeout,
        std::time::Duration::from_secs(90),
        "a container override wins over the file default"
    );
}

#[test]
fn timeouts_fall_back_to_the_documented_defaults() {
    let file = parse(
        r#"
name: orders
containers:
  api:
    worker: path://./workers/api
"#,
    )
    .unwrap();

    assert_eq!(file.startup_timeout, std::time::Duration::from_secs(60));
    assert_eq!(file.stop_timeout, std::time::Duration::from_secs(10));
    assert_eq!(
        file.containers["api"].startup_timeout,
        std::time::Duration::from_secs(60),
        "a container inherits the file's readiness budget"
    );
}

#[test]
fn rejects_a_user_environment_that_shadows_the_reserved_contract() {
    // Silently dropping it would look like it took effect.
    for reserved in ["III_URL", "III_NAMESPACE", "III_CONFIG", "III_WORKER_NAME"] {
        let text = format!(
            r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    environment:
      {reserved}: mine
"#
        );
        assert_eq!(code(&text), "RESERVED_ENV_OVERRIDE", "key: {reserved}");
    }
}

#[test]
fn rejects_duplicate_environment_keys() {
    assert_eq!(
        code(
            r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    environment:
      RUST_LOG: info
      RUST_LOG: debug
"#
        ),
        "INVALID_COMPOSE_FILE"
    );
}

#[test]
fn rejects_a_file_level_timeout_without_a_unit() {
    assert_eq!(
        code(
            r#"
name: orders
startup_timeout: 60
containers:
  api:
    worker: path://./workers/api
"#
        ),
        "INVALID_DURATION"
    );
}

#[test]
fn rejects_duplicate_yaml_keys() {
    assert_eq!(
        code(
            r#"
name: orders
containers:
  api:
    worker: path://./workers/api
  api:
    worker: path://./workers/other
"#
        ),
        "INVALID_COMPOSE_FILE"
    );
}

#[test]
fn rejects_run_on_a_package_worker() {
    assert_eq!(
        code(
            r#"
name: orders
containers:
  api:
    worker: package://workers.iii.dev/api
    version: "1.0.0"
    scripts:
      run: cargo run
"#
        ),
        "RUN_NOT_ALLOWED_FOR_PACKAGE"
    );
}

#[test]
fn rejects_a_pre_start_timeout_without_a_pre_start() {
    assert_eq!(
        code(
            r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    scripts:
      pre_start_timeout: 30s
"#
        ),
        "PRE_START_TIMEOUT_WITHOUT_PRE_START"
    );
}

#[test]
fn rejects_a_timeout_without_a_unit() {
    assert_eq!(
        code(
            r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    scripts:
      pre_start: ./migrate.sh
      pre_start_timeout: 30
"#
        ),
        "INVALID_DURATION"
    );
}

#[test]
fn rejects_worker_sources_outside_v1() {
    assert_eq!(
        code(
            r#"
name: orders
containers:
  runtime:
    worker: image://docker.io/library/node@sha256:abc
"#
        ),
        "UNSUPPORTED_WORKER_SOURCE"
    );
}

#[test]
fn requires_a_version_for_package_workers() {
    assert_eq!(
        code(
            r#"
name: orders
containers:
  api:
    worker: package://workers.iii.dev/api
"#
        ),
        "MISSING_VERSION_FOR_PACKAGE"
    );
}

#[test]
fn rejects_two_config_sources() {
    assert_eq!(
        code(
            r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    config_name: orders-api
    config_uri: worker://configuration/get/orders-api
"#
        ),
        "CONFLICTING_CONFIG_SOURCE"
    );
}

#[test]
fn rejects_config_uris_outside_the_configuration_worker() {
    assert_eq!(
        code(
            r#"
name: orders
containers:
  api:
    worker: path://./workers/api
    config_uri: file://./config/api.yaml
"#
        ),
        "UNSUPPORTED_CONFIG_URI"
    );
}

#[test]
fn orders_a_diamond_graph_dependencies_first() {
    let file = parse(
        r#"
name: orders
containers:
  web:
    worker: path://./workers/web
    depends_on:
      - api
      - queue
  api:
    worker: path://./workers/api
    depends_on:
      - database
  queue:
    worker: path://./workers/queue
    depends_on:
      - database
  database:
    worker: path://./workers/database
"#,
    )
    .unwrap();

    let order = file.start_order().unwrap();
    let position = |key: &str| order.iter().position(|entry| entry == key).unwrap();

    assert!(position("database") < position("api"));
    assert!(position("database") < position("queue"));
    assert!(position("api") < position("web"));
    assert!(position("queue") < position("web"));
    assert_eq!(order.len(), 4);
}
