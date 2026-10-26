// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Nothing compose starts outlives it.
//!
//! A compose process that dies without its teardown (SIGKILL, a panic, a
//! second Ctrl+C) used to leave every worker running with no owner, and the
//! next `--up` adopted them as "already running".

#![cfg(unix)]

use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

use iii_compose::{
    daemon::{Daemon, EnginePolicy},
    process::{BirthIdentity, Supervised, birth_identity, reaper, spawn_supervised},
    state::{ChildRecord, ChildStatus, DaemonState, Owner, StateStore},
};

fn alive(pid: u32) -> bool {
    iii_compose::process::is_running(pid)
}

fn eventually(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// This test binary, running only the named ignored fixture.
fn fixture(name: &str) -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        name,
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]);
    command
}

#[test]
fn children_die_with_a_compose_that_was_killed() {
    let mut compose = fixture("compose_fixture")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let child: u32 = BufReader::new(compose.stdout.take().unwrap())
        .lines()
        .find_map(|line| line.ok()?.split_once("child=")?.1.parse().ok())
        .expect("the fixture should report its child");
    assert!(alive(child));

    // No teardown runs: exactly what SIGKILL, a panic or a second Ctrl+C do.
    compose.kill().unwrap();
    compose.wait().unwrap();

    assert!(
        eventually(|| !alive(child)),
        "the reaper must stop a child its compose left behind"
    );
}

#[tokio::test]
#[ignore = "subprocess fixture for children_die_with_a_compose_that_was_killed"]
async fn compose_fixture() {
    reaper::set_program(
        std::env::current_exe().unwrap(),
        ["--exact", "reaper_fixture", "--ignored"]
            .map(Into::into)
            .to_vec(),
    );
    let mut command = tokio::process::Command::new("sleep");
    command.arg("60");
    let child = spawn_supervised(command).unwrap();
    println!("child={}", child.pid);
    tokio::time::sleep(Duration::from_secs(60)).await;
}

#[test]
#[ignore = "subprocess fixture: the reaper compose_fixture starts"]
fn reaper_fixture() {
    reaper::run();
}

fn group(seconds: &str) -> std::process::Child {
    use std::os::unix::process::CommandExt;
    std::process::Command::new("sleep")
        .arg(seconds)
        .process_group(0)
        .spawn()
        .unwrap()
}

fn announcement(child: &std::process::Child, birth: &BirthIdentity) -> String {
    match birth {
        BirthIdentity::StartTime(start) => format!("+{} {start}\n", child.id()),
        BirthIdentity::Unavailable => format!("+{} -\n", child.id()),
    }
}

#[test]
fn the_reaper_stops_only_what_is_still_announced_and_still_ours() {
    let mut stopped = group("30");
    let mut withdrawn = group("30");
    let mut recycled = group("30");
    let input = [
        announcement(&stopped, &birth_identity(stopped.id())),
        // Announced twice and withdrawn once: still owned.
        announcement(&stopped, &birth_identity(stopped.id())),
        format!("-{}\n", stopped.id()),
        announcement(&withdrawn, &birth_identity(withdrawn.id())),
        format!("-{}\n", withdrawn.id()),
        // The PID is alive but is not the process that was announced.
        announcement(&recycled, &BirthIdentity::StartTime(1)),
    ]
    .concat();

    reaper::serve(std::io::Cursor::new(input), Duration::from_secs(2));

    assert!(
        eventually(|| stopped.try_wait().unwrap().is_some()),
        "an announced group is stopped at EOF"
    );
    assert!(withdrawn.try_wait().unwrap().is_none());
    assert!(recycled.try_wait().unwrap().is_none());
    for mut child in [withdrawn, recycled] {
        child.kill().unwrap();
        child.wait().unwrap();
    }
}

const COMPOSE: &str = r#"
namespace: orders
stop_timeout: 2s
containers:
  api:
    worker: path://./workers/api
    scripts:
      run: ./api
"#;

/// One state root for the whole binary: the variable is process-wide and
/// tests run in parallel. Each test keeps apart by its own daemon namespace.
fn state_root() -> &'static Path {
    static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let root = tempfile::tempdir().unwrap();
        // SAFETY: `get_or_init` runs this once, before any caller returns.
        unsafe { std::env::set_var("III_COMPOSE_STATE_DIR", root.path()) };
        root
    })
    .path()
}

/// Opens a project whose recorded `api` is a live process, as a restarted
/// compose would find it, with `owner` as the compose that recorded it.
async fn reopen_with_survivor(owner: Option<Owner>) -> (tempfile::TempDir, Supervised) {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("workers/api")).unwrap();
    let file = tmp.path().join("worker-compose.yaml");
    std::fs::write(&file, COMPOSE).unwrap();
    let file = file.canonicalize().unwrap();

    let mut command = tokio::process::Command::new("sleep");
    command.arg("30");
    let survivor = spawn_supervised(command).unwrap();

    let namespace = format!("orphans-{}", uuid::Uuid::new_v4().simple());
    let store = StateStore::at(
        state_root()
            .join(iii_compose::state::project_slug(&file))
            .join(&namespace),
    );
    let mut state = DaemonState::new(&file, "orders");
    state.containers.insert(
        "api".to_string(),
        ChildRecord::from_supervised(&survivor, ChildStatus::Ready),
    );
    state.owner = owner;
    store.save(&state).unwrap();

    let daemon = Daemon::start(
        "ws://127.0.0.1:1/ws".to_string(),
        namespace,
        None,
        EnginePolicy::External,
    );
    daemon.project(Path::new(&file)).await.unwrap();
    (tmp, survivor)
}

#[tokio::test]
async fn a_survivor_of_a_compose_that_is_gone_is_stopped_not_adopted() {
    // `None` is what state written before owners were recorded looks like.
    let (_root, survivor) = reopen_with_survivor(None).await;
    assert!(
        tokio::time::timeout(Duration::from_secs(5), survivor.wait())
            .await
            .is_ok(),
        "a process nobody supervises must not be reused as `already running`"
    );
}

#[tokio::test]
async fn a_survivor_of_a_live_compose_is_still_adopted() {
    let (_root, survivor) = reopen_with_survivor(Some(Owner::current())).await;
    assert!(alive(survivor.pid), "a live owner's child is never stopped");
    survivor.stop(Duration::from_secs(2)).await;
}
