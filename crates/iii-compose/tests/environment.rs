//! Environment resolution: env files, precedence and the reserved contract.

use std::path::Path;

use iii_compose::ComposeFile;

/// Keeps explicitly shared configuration IDs independent of the project namespace.
#[test]
fn explicit_configuration_name_wins_over_the_generated_default() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        "containers:\n  api:\n    worker: path://./api\n    config_name: shared-api\n",
        &[],
    );
    assert_eq!(
        file.containers["api"].resolved_config_name("orders", "api"),
        "shared-api"
    );
}

/// Derives runtime identity from the selected namespace without changing the parsed declaration.
#[test]
fn configuration_identity_uses_the_effective_namespace_without_mutating_yaml() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(tmp.path(), COMPOSE, &[]);
    let container = &file.containers["api"];
    let name = container.resolved_config_name("billing", "api");
    assert!(name.starts_with("billing-api-"), "{name}");
    assert_ne!(name, container.resolved_config_name("orders", "api"));
    assert!(container.config_name.is_none());
}

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

fn normalize_path(path: &Path) -> std::path::PathBuf {
    path.canonicalize()
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
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

#[cfg(windows)]
#[test]
fn windows_case_equivalent_keys_obey_source_precedence() {
    let tmp = tempfile::tempdir().unwrap();
    for (first, alias) in [
        ("token", "TOKEN"),
        ("TOKEN", "token"),
        ("föo", "FÖO"),
        ("FÖO", "föo"),
    ] {
        for (value, expected) in [
            ("compose", "compose"),
            ("' '", " "),
            ("''", "last-file"),
            ("\"\"", "last-file"),
            ("null", "last-file"),
            ("~", "last-file"),
            ("", "last-file"),
        ] {
            let compose = format!(
                "containers:\n  api:\n    worker: path://./api\n    env_file: [base.env, last.env]\n    environment:\n      {first}: {value}\n"
            );
            let file = project(
                tmp.path(),
                &compose,
                &[
                    ("base.env", &format!("{first}=base\n{alias}=same-file\n")),
                    ("last.env", &format!("{alias}=last-file\n")),
                ],
            );
            let env = file.containers["api"].resolve_user_env("api").unwrap();
            assert_eq!(env.len(), 1, "{first}/{alias}: {value:?}");
            assert_eq!(env.values().next().unwrap(), expected);
            let mut command = std::process::Command::new("cmd");
            command.env_clear().envs(&env);
            let actual: Vec<_> = command
                .get_envs()
                .map(|(_, value)| value.unwrap().to_str().unwrap())
                .collect();
            assert_eq!(actual, [expected]);
        }

        // An empty value in a later env file still overrides an earlier file;
        // preservation applies only to an empty Compose `environment` value.
        let file = project(
            tmp.path(),
            "containers:\n  api:\n    worker: path://./api\n    env_file: [base.env, last.env]\n",
            &[
                ("base.env", &format!("{first}=base\n")),
                ("last.env", &format!("{alias}=\n")),
            ],
        );
        let env = file.containers["api"].resolve_user_env("api").unwrap();
        assert_eq!(env.len(), 1);
        assert_eq!(env.values().next().unwrap(), "");
    }
}

#[test]
fn blank_environment_values_preserve_the_last_env_file_value() {
    for value in ["\"\"", "''", "", "null", "~"] {
        let tmp = tempfile::tempdir().unwrap();
        let compose = format!(
            "containers:\n  api:\n    worker: path://./api\n    env_file: [base.env, override.env]\n    environment:\n      TOKEN: {value}\n"
        );
        let file = project(
            tmp.path(),
            &compose,
            &[
                ("base.env", "TOKEN=base\nUNLISTED=preserved\n"),
                ("override.env", "TOKEN=override\n"),
            ],
        );

        let env = file.containers["api"].resolve_user_env("api").unwrap();

        assert_eq!(
            env,
            std::collections::BTreeMap::from([
                ("TOKEN".to_string(), "override".to_string()),
                ("UNLISTED".to_string(), "preserved".to_string()),
            ]),
            "environment value: {value:?}"
        );
    }
}

#[test]
fn nonempty_environment_values_still_override_env_files() {
    for (value, expected) in [
        ("literal", "literal"),
        ("\"null\"", "null"),
        ("false", "false"),
        ("0", "0"),
        ("\"  \"", "  "),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let compose = format!(
            "containers:\n  api:\n    worker: path://./api\n    env_file: [base.env]\n    environment:\n      TOKEN: {value}\n"
        );
        let file = project(tmp.path(), &compose, &[("base.env", "TOKEN=fromfile\n")]);

        let env = file.containers["api"].resolve_user_env("api").unwrap();

        assert_eq!(env["TOKEN"], expected, "environment value: {value:?}");
    }
}

#[test]
fn without_an_env_file_empty_strings_remain_empty_and_null_keys_are_omitted() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        r#"
containers:
  api:
    worker: path://./api
    environment:
      EMPTY: ""
      BARE:
      NULL_VALUE: null
      TILDE: ~
      LITERAL_NULL: "null"
"#,
        &[],
    );

    let env = file.containers["api"].resolve_user_env("api").unwrap();

    assert_eq!(
        env,
        std::collections::BTreeMap::from([
            ("EMPTY".to_string(), String::new()),
            ("LITERAL_NULL".to_string(), "null".to_string()),
        ])
    );
}

#[test]
fn optional_interpolation_preserves_env_files_unless_the_host_value_is_nonempty() {
    for (host_value, expected) in [
        (None, "fromfile"),
        (Some(""), "fromfile"),
        (Some("fromhost"), "fromhost"),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let mut document: serde_yaml::Value = serde_yaml::from_str(
            r#"
containers:
  api:
    worker: path://./api
    env_file: [base.env]
    environment:
      TOKEN: ${TOKEN:-}
"#,
        )
        .unwrap();
        // Inject the lookup instead of mutating the process-wide environment.
        iii_compose::interpolate::expand_tree(
            &mut document,
            &tmp.path().join("worker-compose.yaml"),
            &|_| host_value.map(str::to_string),
        )
        .unwrap();
        let compose = serde_yaml::to_string(&document).unwrap();
        let file = project(tmp.path(), &compose, &[("base.env", "TOKEN=fromfile\n")]);

        let env = file.containers["api"].resolve_user_env("api").unwrap();

        assert_eq!(env["TOKEN"], expected, "host value: {host_value:?}");
    }
}

#[test]
fn blank_environment_values_cannot_bypass_reserved_key_validation() {
    for reserved in iii_compose::spawn::RESERVED_ENV {
        for value in ["\"\"", "", "null", "~"] {
            let compose = format!(
                "containers:\n  api:\n    worker: path://./api\n    environment:\n      {reserved}: {value}\n"
            );

            let err = ComposeFile::parse(&compose, "/tmp/worker-compose.yaml")
                .expect_err("reserved keys must be rejected even without a value");

            assert_eq!(err.code(), "RESERVED_ENV_OVERRIDE", "{reserved}: {value}");
        }
    }
}

#[cfg(windows)]
#[test]
fn windows_reserved_key_validation_matches_native_environment_names() {
    // Use std's native environment-key map as an independent oracle. Windows
    // ordinal folding is not Rust's Unicode uppercase mapping: dotless i, for
    // example, must not be assumed to collide with ASCII I.
    fn assert_validation<T: std::fmt::Debug>(
        result: Result<T, iii_compose::ComposeError>,
        name: &str,
        collides: bool,
    ) {
        if collides {
            let err = result.expect_err("native aliases must be rejected");
            assert_eq!(err.code(), "RESERVED_ENV_OVERRIDE", "{name}");
            assert!(err.to_string().contains(name));
        } else {
            result.expect("distinct native names must remain valid");
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    for reserved in iii_compose::spawn::RESERVED_ENV {
        for name in [
            reserved.to_lowercase(),
            reserved.replace('I', "ı"),
            reserved.replace('I', "İ"),
            reserved.replace('S', "ſ"),
        ] {
            let mut native = std::process::Command::new("cmd");
            native
                .env_clear()
                .env(reserved, "reserved")
                .env(&name, "explicit");
            let collides = native.get_envs().count() == 1;
            if name == reserved.to_lowercase() {
                assert!(collides, "ASCII casing must collide on Windows");
            }
            for value in ["stale", "\"\"", "", "null", "~"] {
                let compose = format!(
                    "containers:\n  api:\n    worker: package://workers.iii.dev/queue\n    version: '0.1.0'\n    environment:\n      {name}: {value}\n"
                );
                assert_validation(
                    ComposeFile::parse(&compose, tmp.path().join("worker-compose.yaml")),
                    &name,
                    collides,
                );
            }

            let file = project(
                tmp.path(),
                "containers:\n  api:\n    worker: package://workers.iii.dev/queue\n    version: '0.1.0'\n    env_file: [base.env]\n",
                &[("base.env", &format!("{name}=stale\n"))],
            );
            assert_validation(
                file.containers["api"].resolve_user_env("api"),
                &name,
                collides,
            );
            assert_validation(
                iii_compose::manifest::validate_offline(&file, "reserved-test"),
                &name,
                collides,
            );
        }
    }
}

#[cfg(not(windows))]
#[test]
fn reserved_key_validation_remains_case_sensitive_on_other_platforms() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(
        tmp.path(),
        "containers:\n  api:\n    worker: package://workers.iii.dev/queue\n    version: '0.1.0'\n    env_file: [base.env]\n    environment:\n      iii_config: compose\n      ııı_config: unicode\n",
        &[("base.env", "iii_config_name=from-file\n")],
    );
    let env = file.containers["api"].resolve_user_env("api").unwrap();
    assert_eq!(env["iii_config"], "compose");
    assert_eq!(env["ııı_config"], "unicode");
    assert_eq!(env["iii_config_name"], "from-file");
    iii_compose::manifest::validate_offline(&file, "reserved-test").unwrap();
}

#[test]
fn duplicate_environment_keys_are_rejected_even_when_null() {
    let err = ComposeFile::parse(
        "containers:\n  api:\n    worker: path://./api\n    environment:\n      TOKEN:\n      TOKEN: value\n",
        "/tmp/worker-compose.yaml",
    )
    .expect_err("a null value must not hide a duplicate key");

    assert!(err.to_string().contains("duplicate"), "{err}");
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
        compose_namespace: "compose-finance",
        compose_file: std::path::Path::new("/srv/finance/worker-compose.yaml"),
        container_key: "state",
        start: &start,
        config_path: Some(std::path::Path::new("/run/state.yaml")),
        config_name: Some("state-finance"),
        working_dir: std::path::Path::new("."),
        user_env: &user_env,
    });

    assert_eq!(plan.env["III_CONFIG_NAME"], "state-finance");
    assert_eq!(plan.env["III_COMPOSE_NAMESPACE"], "compose-finance");
    let expected_compose_file =
        normalize_path(std::path::Path::new("/srv/finance/worker-compose.yaml"));
    let expected_compose_dir = expected_compose_file
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    assert_eq!(
        std::path::Path::new(&plan.env["III_COMPOSE_FILE"]),
        expected_compose_file
    );
    assert_eq!(
        std::path::Path::new(&plan.env["III_COMPOSE_DIR"]),
        expected_compose_dir
    );
    // The container key still names the worker. They are different questions:
    // one is what the engine routes to, the other is where the configuration
    // lives — and it is exactly their conflation that made the id global.
    assert_eq!(plan.env["III_WORKER_NAME"], "state");
}

/// Delivers a first-boot registration ID even when there is no configuration file.
#[test]
fn a_container_without_a_value_still_receives_its_configuration_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let file = project(tmp.path(), COMPOSE, &[]);
    let config_name = file.containers["api"].resolved_config_name("finance", "api");
    use iii_compose::manifest::StartSpec;
    use iii_compose::spawn::{SpawnCtx, spawn_plan};

    let user_env = std::collections::BTreeMap::new();
    let start = StartSpec::Shell("true".to_string());
    let plan = spawn_plan(&SpawnCtx {
        engine_url: "ws://127.0.0.1:49134",
        namespace: "finance",
        compose_namespace: "compose-finance",
        compose_file: std::path::Path::new("/srv/finance/worker-compose.yaml"),
        container_key: "api",
        start: &start,
        config_path: None,
        config_name: Some(&config_name),
        working_dir: std::path::Path::new("."),
        user_env: &user_env,
    });

    assert_eq!(plan.env["III_CONFIG_NAME"], config_name);
    assert!(!plan.env.contains_key("III_CONFIG"));
}
