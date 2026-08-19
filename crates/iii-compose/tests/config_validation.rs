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
namespace: orders
containers:
  database:
    worker: package://workers.iii.dev/database
    version: 1.4.2
    config_name: orders-db
  api:
    worker: path://./workers/api
    depends_on:
      - database
    config_name: orders-api
    config_override:
      server:
        port: 3000
    scripts:
      pre_run: ./scripts/migrate.sh
      pre_run_timeout: 90s
      run: cargo run --release
      post_run: ./scripts/drain.sh
    working_dir: ./workers/api
"#;

#[test]
fn accepts_the_canonical_project() {
    let file = parse(CANONICAL).expect("canonical project should parse");

    assert_eq!(file.namespace.as_deref(), Some("orders"));
    assert_eq!(file.containers.len(), 2);
    assert_eq!(file.start_order().unwrap(), vec!["database", "api"]);

    let api = &file.containers["api"];
    assert_eq!(api.depends_on, vec!["database".to_string()]);
    assert_eq!(api.config_name.as_deref(), Some("orders-api"));
    assert_eq!(
        api.scripts.pre_run_timeout,
        std::time::Duration::from_secs(90)
    );
    assert_eq!(api.scripts.run.as_deref(), Some("cargo run --release"));
    assert_eq!(api.working_dir, Some(PathBuf::from("/srv/app/workers/api")));

    assert_eq!(
        file.containers["database"].config_name.as_deref(),
        Some("orders-db")
    );
    assert_eq!(
        file.containers["api"].config_name.as_deref(),
        Some("orders-api")
    );
}

#[test]
fn pre_run_timeout_defaults_to_sixty_seconds() {
    let file = parse(
        r#"
namespace: orders
containers:
  api:
    worker: path://./workers/api
"#,
    )
    .unwrap();

    assert_eq!(
        file.containers["api"].scripts.pre_run_timeout,
        std::time::Duration::from_secs(60)
    );
}

#[test]
fn rejects_an_empty_container_map() {
    assert_eq!(
        code(
            r#"
namespace: orders
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
namespace: orders
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
namespace: orders
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
namespace: orders
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
namespace: orders
hot_reload: true
containers:
  api:
    worker: path://./workers/api
"#,
    );
    let container = code(
        r#"
namespace: orders
containers:
  api:
    worker: path://./workers/api
    port: 8080
"#,
    );
    let scripts = code(
        r#"
namespace: orders
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
namespace: orders
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
namespace: orders
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
namespace: orders
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
namespace: orders
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
    // Driven off the constant rather than a hand-written list: III_CONFIG_NAME
    // was added to the contract and missed here, so a sixth key would have been
    // untested the same way.
    for reserved in iii_compose::spawn::RESERVED_ENV {
        let text = format!(
            r#"
namespace: orders
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
namespace: orders
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
namespace: orders
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
namespace: orders
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
namespace: orders
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
fn rejects_a_pre_run_timeout_without_a_pre_run() {
    assert_eq!(
        code(
            r#"
namespace: orders
containers:
  api:
    worker: path://./workers/api
    scripts:
      pre_run_timeout: 30s
"#
        ),
        "PRE_RUN_TIMEOUT_WITHOUT_PRE_RUN"
    );
}

#[test]
fn rejects_a_timeout_without_a_unit() {
    assert_eq!(
        code(
            r#"
namespace: orders
containers:
  api:
    worker: path://./workers/api
    scripts:
      pre_run: ./migrate.sh
      pre_run_timeout: 30
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
namespace: orders
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
namespace: orders
containers:
  api:
    worker: package://workers.iii.dev/api
"#
        ),
        "MISSING_VERSION_FOR_PACKAGE"
    );
}

#[test]
fn orders_a_diamond_graph_dependencies_first() {
    let file = parse(
        r#"
namespace: orders
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

/// `name:` used to be rewritten to fit the namespace charset rather than
/// checked against it. The value is what an operator types into `iii trigger
/// --namespace` and into every `worker.trigger` call, so a value they cannot
/// type back is worse than a refusal at load time.
#[test]
fn a_name_outside_the_namespace_charset_is_refused() {
    let with_name = |name: &str| {
        format!(
            "namespace: \"{name}\"\ncontainers:\n  api:\n    worker: path://./workers/api\n    scripts:\n      run: ./api\n"
        )
    };

    // All four of these sanitized to `my-shop`, so four different declarations
    // addressed one namespace and none of them said so.
    for collided in ["My Shop!", "my/shop", "my shop", "MY-SHOP"] {
        assert_eq!(
            code(&with_name(collided)),
            "INVALID_NAMESPACE",
            "for {collided:?}"
        );
    }

    // And a name made entirely of rejected characters became the literal
    // `project`, naming a namespace after nothing the file contained.
    assert_eq!(code(&with_name("!!!")), "INVALID_NAMESPACE");

    // What the set does hold still parses, unchanged.
    for accepted in ["my-shop", "shop_2", "a"] {
        let file = parse(&with_name(accepted)).expect("should parse");
        assert_eq!(file.namespace.as_deref(), Some(accepted));
    }
}

/// An absent name is not an invalid one: the project lands in `default`.
#[test]
fn no_name_at_all_is_still_allowed() {
    let file = parse(
        "containers:\n  api:\n    worker: path://./workers/api\n    scripts:\n      run: ./api\n",
    )
    .expect("a file without a name should parse");
    assert_eq!(file.namespace, None);
}

/// `name:` is not a second spelling of `namespace:`. Nothing has shipped, so
/// this guards a future rather than a past: adding it back as a convenience
/// alias would put two keys on one coordinate, and the shorter one is the one
/// people write. The error that refuses it also points at the right key, which
/// is what makes one spelling affordable.
#[test]
fn name_is_not_an_alias_for_namespace() {
    let text = "name: orders\ncontainers:\n  api:\n    worker: path://./workers/api\n    scripts:\n      run: ./api\n";
    let err = parse(text).expect_err("`name:` is not a field");
    assert_eq!(err.code(), "INVALID_COMPOSE_FILE");

    let message = err.to_string();
    assert!(
        message.contains("name"),
        "should name the bad key: {message}"
    );
    assert!(
        message.contains("namespace"),
        "and list `namespace` among the accepted keys: {message}"
    );
}

/// `config_uri` is gone rather than deprecated. How a configuration is read
/// and stored is the configuration worker's business — it has an adapter for
/// that — so the compose file says which configuration and nothing about where
/// it lives. A URI here would have been compose describing transport it does
/// not own, and a `file://` form would have contradicted the very adapter that
/// decides it.
#[test]
fn config_uri_is_not_a_second_spelling() {
    let text = r#"
namespace: orders
containers:
  api:
    worker: path://./workers/api
    config_uri: worker://configuration/get/orders-api
    scripts:
      run: ./api
"#;
    let err = parse(text).expect_err("`config_uri` is not a field");
    assert_eq!(err.code(), "INVALID_COMPOSE_FILE");
    assert!(
        err.to_string().contains("config_name"),
        "the error should point at the key that replaced it: {err}"
    );
}
