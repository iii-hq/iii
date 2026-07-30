//! Supervision against real processes: groups, teardown and identity.

#![cfg(unix)]

use std::{path::Path, time::Duration};

use iii_compose::{
    manifest::StartSpec,
    process::{BirthIdentity, Outcome, spawn_supervised},
    spawn::{SpawnCtx, spawn_plan},
};

/// Empty user environment for tests that only care about the reserved contract.
fn empty_env() -> &'static std::collections::BTreeMap<String, String> {
    static EMPTY: std::sync::OnceLock<std::collections::BTreeMap<String, String>> =
        std::sync::OnceLock::new();
    EMPTY.get_or_init(std::collections::BTreeMap::new)
}

fn spawn(script: &str, cwd: &Path) -> iii_compose::process::Supervised {
    let start = StartSpec::Shell(script.to_string());
    let ctx = SpawnCtx {
        engine_url: "ws://127.0.0.1:49134",
        namespace: "orders-test",
        container_key: "api",
        start: &start,
        config_path: None,
        working_dir: cwd,
        user_env: empty_env(),
    };
    spawn_supervised(spawn_plan(&ctx).command()).expect("child should spawn")
}

fn is_alive(pid: u32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
}

async fn wait_until_gone(pid: u32) -> bool {
    for _ in 0..100 {
        if !is_alive(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

async fn read_pid_file(path: &Path) -> u32 {
    for _ in 0..100 {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(pid) = text.trim().parse() {
                return pid;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("pid file was never written: {}", path.display());
}

#[tokio::test]
async fn stop_takes_down_grandchildren_too() {
    let tmp = tempfile::tempdir().unwrap();
    // The worker spawns its own child, as a language runtime or build tool
    // would, and reports its pid.
    let child = spawn(
        "sh -c 'while :; do sleep 0.05; done' & echo $! > grandchild.pid; wait",
        tmp.path(),
    );
    let grandchild = read_pid_file(&tmp.path().join("grandchild.pid")).await;

    assert!(is_alive(grandchild), "grandchild should be running");
    child.stop(Duration::from_secs(2)).await;

    assert!(
        wait_until_gone(grandchild).await,
        "grandchild {grandchild} survived the group teardown"
    );
    assert!(wait_until_gone(child.pid).await);
}

#[tokio::test]
async fn a_child_that_ignores_sigterm_is_killed_after_the_grace() {
    let tmp = tempfile::tempdir().unwrap();
    let child = spawn("trap '' TERM; while :; do sleep 0.05; done", tmp.path());

    // Let the trap install before signalling.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let started = std::time::Instant::now();
    child.stop(Duration::from_millis(200)).await;

    assert!(
        started.elapsed() >= Duration::from_millis(200),
        "stop returned before the grace elapsed"
    );
    assert!(wait_until_gone(child.pid).await);
}

#[tokio::test]
async fn stop_returns_immediately_for_a_child_that_already_exited() {
    let tmp = tempfile::tempdir().unwrap();
    let child = spawn("exit 7", tmp.path());

    let status = child.wait().await;
    assert_eq!(status.code(), Some(7));
    assert_eq!(child.poll(), Outcome::Exited(status));

    let started = std::time::Instant::now();
    child.stop(Duration::from_secs(10)).await;
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "stop waited on an already-dead child"
    );
}

#[tokio::test]
async fn wait_resolves_when_the_child_exits_on_its_own() {
    let tmp = tempfile::tempdir().unwrap();
    let child = spawn("sleep 0.1; exit 3", tmp.path());

    assert_eq!(child.poll(), Outcome::Running);
    assert_eq!(child.wait().await.code(), Some(3));
}

#[tokio::test]
async fn every_child_leads_its_own_process_group() {
    let tmp = tempfile::tempdir().unwrap();
    let child = spawn("while :; do sleep 0.05; done", tmp.path());

    assert_eq!(child.pgid, child.pid as i32);
    child.stop(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn a_recycled_pid_is_never_recognised() {
    let tmp = tempfile::tempdir().unwrap();
    let child = spawn("exit 0", tmp.path());
    let recorded = child.birth.clone();
    child.wait().await;

    // Same PID number, different process: the fingerprint must not match, so a
    // restarting daemon can never signal a stranger.
    let other = BirthIdentity::StartTime(u64::MAX);
    assert!(!recorded.matches(&other));
    assert!(!BirthIdentity::Unavailable.matches(&BirthIdentity::Unavailable));

    #[cfg(target_os = "linux")]
    assert!(
        matches!(recorded, BirthIdentity::StartTime(_)),
        "linux should fingerprint children"
    );
}
