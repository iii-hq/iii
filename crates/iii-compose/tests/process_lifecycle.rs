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
        config_name: None,
        working_dir: cwd,
        user_env: empty_env(),
    };
    spawn_supervised(spawn_plan(&ctx).command().expect("a host container"))
        .expect("child should spawn")
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
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(pid) = text.trim().parse()
        {
            return pid;
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

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    assert!(
        matches!(recorded, BirthIdentity::StartTime(_)),
        "every platform compose ships on must fingerprint its children"
    );
}

// ── Adoption ─────────────────────────────────────────────────────────────
//
// A daemon that restarts finds its children still running. Adoption is what
// lets teardown reach them: a process left out of the children map is one
// `down` walks straight past, reporting success over something still alive.

#[tokio::test]
async fn an_adopted_child_can_still_be_stopped() {
    let tmp = tempfile::tempdir().unwrap();
    let child = spawn("sleep 30", tmp.path());
    let (pid, birth) = (child.pid, child.birth.clone());

    // Dropping the handle is what a daemon restart does to it: the process
    // outlives the supervision that spawned it.
    drop(child);
    assert!(is_alive(pid), "the process should outlive its handle");

    let adopted = iii_compose::process::Supervised::adopt(pid, &birth)
        .expect("a live pid with a matching identity is adoptable");
    assert_eq!(adopted.pid, pid);
    assert_eq!(adopted.poll(), Outcome::Running);

    adopted.stop(Duration::from_secs(2)).await;
    assert!(
        wait_until_gone(pid).await,
        "adoption must make stop reach it"
    );
}

#[tokio::test]
async fn adoption_refuses_a_pid_whose_identity_does_not_match() {
    let tmp = tempfile::tempdir().unwrap();
    let child = spawn("sleep 30", tmp.path());
    let pid = child.pid;

    // Stands in for a recycled PID: alive, but not the process we recorded.
    let wrong = BirthIdentity::StartTime(u64::MAX);
    assert!(
        iii_compose::process::Supervised::adopt(pid, &wrong).is_none(),
        "a mismatched identity must never be adopted, let alone signalled"
    );
    assert!(is_alive(pid), "and the stranger must be left alone");

    child.stop(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn an_unverifiable_identity_is_never_adopted() {
    let tmp = tempfile::tempdir().unwrap();
    let child = spawn("sleep 30", tmp.path());
    let pid = child.pid;

    assert!(
        iii_compose::process::Supervised::adopt(pid, &BirthIdentity::Unavailable).is_none(),
        "without a fingerprint there is nothing to prove ownership with"
    );

    child.stop(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn an_adopted_child_reports_its_own_exit() {
    let tmp = tempfile::tempdir().unwrap();
    let child = spawn("sleep 0.2", tmp.path());
    let (pid, birth) = (child.pid, child.birth.clone());
    drop(child);

    let adopted =
        iii_compose::process::Supervised::adopt(pid, &birth).expect("still running when adopted");

    // No reaper task backs an adopted process, so `wait` polls liveness. The
    // status is unrecoverable — only the fact of the exit survives.
    adopted.wait().await;
    assert!(matches!(adopted.poll(), Outcome::Exited(_)));
}

#[tokio::test]
async fn adopting_the_group_leader_adopts_its_grandchildren() {
    let tmp = tempfile::tempdir().unwrap();
    let marker = tmp.path().join("grandchild.pid");
    let child = spawn(
        &format!(
            "sh -c 'echo $$ > {} ; sleep 30' & sleep 30",
            marker.display()
        ),
        tmp.path(),
    );
    let (pid, birth) = (child.pid, child.birth.clone());
    let grandchild = read_pid_file(&marker).await;
    drop(child);

    let adopted = iii_compose::process::Supervised::adopt(pid, &birth).expect("adoptable");
    adopted.stop(Duration::from_secs(2)).await;

    assert!(wait_until_gone(pid).await);
    assert!(
        wait_until_gone(grandchild).await,
        "the pid is the group id, so adopting the leader reaches the whole tree"
    );
}

// ── The leader is not the group ──────────────────────────────────────────
//
// `run: ./worker` goes through a shell, and a shell does not exec a command it
// has to wait on: the recorded pid is the shell, the worker is its child, and
// both share the group. Killing the shell therefore leaves the worker running.

#[tokio::test]
async fn stopping_a_dead_leader_still_clears_its_group() {
    let tmp = tempfile::tempdir().unwrap();
    let marker = tmp.path().join("child.pid");
    // The trailing `wait` keeps the leader alive until we kill it, so the
    // child is orphaned rather than reaped.
    let child = spawn(
        &format!("sh -c 'echo $$ > {} ; sleep 30' & wait", marker.display()),
        tmp.path(),
    );
    let leader = child.pid;
    let worker = read_pid_file(&marker).await;

    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(leader as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
    assert!(wait_until_gone(leader).await, "the leader should be gone");
    assert!(is_alive(worker), "and the worker should have outlived it");

    child.stop(Duration::from_secs(2)).await;
    assert!(
        wait_until_gone(worker).await,
        "stop must sweep the group, not return on the leader's status alone"
    );
}

#[tokio::test]
async fn stopping_a_child_that_left_nothing_behind_is_immediate() {
    let tmp = tempfile::tempdir().unwrap();
    let child = spawn("true", tmp.path());
    child.wait().await;

    // An empty group is never signalled: the sweep probes first, so this
    // returns without waiting out the grace.
    let began = std::time::Instant::now();
    child.stop(Duration::from_secs(5)).await;
    assert!(
        began.elapsed() < Duration::from_secs(1),
        "an empty group should not cost the grace period"
    );
}
