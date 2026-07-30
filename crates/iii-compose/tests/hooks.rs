//! pre_start and post_run behaviour against real scripts.

#![cfg(unix)]

use std::{path::Path, time::Duration};

use iii_compose::{
    hooks::{fire_post_run, run_pre_start},
    manifest::StartSpec,
    spawn::SpawnCtx,
};

const IDLE: StartSpec = StartSpec::Shell(String::new());

/// Empty user environment for tests that only care about the reserved contract.
fn empty_env() -> &'static std::collections::BTreeMap<String, String> {
    static EMPTY: std::sync::OnceLock<std::collections::BTreeMap<String, String>> =
        std::sync::OnceLock::new();
    EMPTY.get_or_init(std::collections::BTreeMap::new)
}

fn ctx<'a>(cwd: &'a Path, start: &'a StartSpec, config: Option<&'a Path>) -> SpawnCtx<'a> {
    SpawnCtx {
        engine_url: "ws://engine.test:49134",
        namespace: "orders-test",
        container_key: "api",
        start,
        config_path: config,
        working_dir: cwd,
        user_env: empty_env(),
    }
}

async fn wait_for_file(path: &Path) -> String {
    for _ in 0..150 {
        if let Ok(text) = std::fs::read_to_string(path)
            && !text.trim().is_empty()
        {
            return text;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("file was never written: {}", path.display());
}

fn is_alive(pid: u32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
}

#[tokio::test]
async fn a_successful_pre_start_passes() {
    let tmp = tempfile::tempdir().unwrap();
    let start = IDLE;
    let result = run_pre_start(
        &ctx(tmp.path(), &start, None),
        "echo migrating; exit 0",
        Duration::from_secs(5),
    )
    .await;

    assert!(result.is_ok(), "{result:?}");
}

#[tokio::test]
async fn a_failing_pre_start_reports_its_exit_code() {
    let tmp = tempfile::tempdir().unwrap();
    let start = IDLE;
    let err = run_pre_start(
        &ctx(tmp.path(), &start, None),
        "echo broken >&2; exit 3",
        Duration::from_secs(5),
    )
    .await
    .expect_err("a non-zero hook must fail the container");

    assert_eq!(err.code(), "HOOK_FAILED");
    assert!(err.to_string().contains('3'), "{err}");
}

#[tokio::test]
async fn a_hung_pre_start_times_out_and_leaves_nothing_running() {
    let tmp = tempfile::tempdir().unwrap();
    let pid_file = tmp.path().join("hook.pid");
    let start = IDLE;

    let hook = format!("echo $$ > {}; sleep 999", pid_file.display());
    let waiter = tokio::spawn(async move {
        let path = pid_file.clone();
        wait_for_file(&path).await.trim().parse::<u32>().unwrap()
    });

    let err = run_pre_start(
        &ctx(tmp.path(), &start, None),
        &hook,
        Duration::from_millis(400),
    )
    .await
    .expect_err("a hung hook must time out");
    let hook_pid = waiter.await.unwrap();

    assert_eq!(err.code(), "HOOK_TIMEOUT");
    for _ in 0..100 {
        if !is_alive(hook_pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("hook process {hook_pid} survived the timeout");
}

#[tokio::test]
async fn hooks_run_with_the_container_environment_and_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("resolved.yaml");
    std::fs::write(&config, "server:\n  port: 3000\n").unwrap();
    let start = IDLE;

    // A stale reserved variable in the parent environment must not reach a hook.
    unsafe { std::env::set_var("III_WORKER_NAME", "stale-name") };
    let result = run_pre_start(
        &ctx(tmp.path(), &start, Some(&config)),
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
        )
    );
}

#[tokio::test]
async fn post_run_fires_without_being_awaited() {
    let tmp = tempfile::tempdir().unwrap();
    let marker = tmp.path().join("drained.txt");
    let start = IDLE;

    fire_post_run(
        &ctx(tmp.path(), &start, None),
        &format!("sleep 0.1; echo $III_WORKER_NAME > {}", marker.display()),
    );

    assert!(
        !marker.exists(),
        "post_run must not block the caller until it finishes"
    );
    assert_eq!(wait_for_file(&marker).await.trim(), "api");
}

#[tokio::test]
async fn a_failing_post_run_is_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let start = IDLE;

    // Nothing to assert beyond "this returns and does not panic": a broken
    // cleanup script must never propagate into teardown.
    fire_post_run(&ctx(tmp.path(), &start, None), "exit 9");
    tokio::time::sleep(Duration::from_millis(200)).await;
}
