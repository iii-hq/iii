//! The reserved environment a hook sees, isolated in its own test binary.
//!
//! The assertion needs a polluted parent environment, and `set_var` is unsafe
//! because another thread reading the environment at the same moment is a data
//! race. Cargo runs the tests of one binary on parallel threads, so the way to
//! write it safely is to be the only test in the binary.

#![cfg(unix)]

use std::{path::Path, time::Duration};

use iii_compose::{hooks::await_pre_run, manifest::StartSpec, spawn::SpawnCtx};

#[tokio::test]
async fn a_stale_reserved_variable_never_reaches_a_hook() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("resolved.yaml");
    std::fs::write(&config, "server:\n  port: 3000\n").unwrap();
    let start = StartSpec::Shell(String::new());
    let empty = std::collections::BTreeMap::new();
    let ctx = SpawnCtx {
        engine_url: "ws://engine.test:49134",
        namespace: "orders-test",
        container_key: "api",
        start: &start,
        config_path: Some(config.as_path()),
        config_name: None,
        working_dir: tmp.path() as &Path,
        user_env: &empty,
    };

    // SAFETY: this is the only test in the binary, so no other thread is
    // reading or writing the environment while this runs.
    unsafe { std::env::set_var("III_WORKER_NAME", "stale-name") };
    let result = await_pre_run(
        &ctx,
        "printf '%s|%s|%s|%s' \"$III_URL\" \"$III_NAMESPACE\" \"$III_WORKER_NAME\" \"$III_CONFIG\" > seen.txt && test \"$PWD\" = \"$(pwd)\"",
        Duration::from_secs(5),
    )
    .await;
    unsafe { std::env::remove_var("III_WORKER_NAME") };

    assert!(result.is_ok(), "{result:?}");
    let seen = std::fs::read_to_string(tmp.path().join("seen.txt")).unwrap();
    assert_eq!(
        seen,
        format!(
            "ws://engine.test:49134|orders-test|api|{}",
            config.display()
        ),
        "the container's own name must win over whatever the parent exported"
    );
}
