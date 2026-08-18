//! Durable state and what a restarting daemon does with the children it left.

#![cfg(unix)]

use std::{path::Path, time::Duration};

use iii_compose::{
    manifest::StartSpec,
    process::{BirthIdentity, Supervised, spawn_supervised},
    spawn::{SpawnCtx, spawn_plan},
    state::{ChildRecord, ChildStatus, DaemonState, Reconciliation, StateStore},
};

/// Empty user environment for tests that only care about the reserved contract.
fn empty_env() -> &'static std::collections::BTreeMap<String, String> {
    static EMPTY: std::sync::OnceLock<std::collections::BTreeMap<String, String>> =
        std::sync::OnceLock::new();
    EMPTY.get_or_init(std::collections::BTreeMap::new)
}

fn spawn(script: &str, cwd: &Path) -> Supervised {
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

fn state_with(child: &Supervised, compose: &Path) -> DaemonState {
    let mut state = DaemonState::new(compose, "orders-1234abcd");
    state.containers.insert(
        "api".to_string(),
        ChildRecord::from_supervised(child, ChildStatus::Ready),
    );
    state
}

#[test]
fn state_survives_a_save_and_load_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let store = StateStore::at(tmp.path().join("host-a"));
    let mut state = DaemonState::new(Path::new("/srv/app/c.yaml"), "orders-1234abcd");
    state.containers.insert(
        "api".to_string(),
        ChildRecord::new(4242, BirthIdentity::StartTime(99), ChildStatus::Ready),
    );

    assert_eq!(
        store.load().unwrap(),
        None,
        "no state before the first save"
    );
    store.save(&state).unwrap();

    assert_eq!(store.load().unwrap(), Some(state));
}

#[test]
fn state_is_owner_only_and_leaves_no_temp_file() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let store = StateStore::at(tmp.path().join("host-a"));
    store
        .save(&DaemonState::new(Path::new("/srv/app/c.yaml"), "ns"))
        .unwrap();

    let file_mode = std::fs::metadata(store.path())
        .unwrap()
        .permissions()
        .mode();
    let dir_mode = std::fs::metadata(store.dir()).unwrap().permissions().mode();
    assert_eq!(
        file_mode & 0o777,
        0o600,
        "state may hold signalling targets"
    );
    assert_eq!(dir_mode & 0o777, 0o700);

    let leftovers: Vec<_> = std::fs::read_dir(store.dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name != "state.json")
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp files left behind: {leftovers:?}"
    );
}

#[test]
fn a_corrupt_state_file_is_an_error_not_a_silent_reset() {
    let tmp = tempfile::tempdir().unwrap();
    let store = StateStore::at(tmp.path().join("host-a"));
    std::fs::create_dir_all(store.dir()).unwrap();
    std::fs::write(store.path(), "{ not json").unwrap();

    let err = store
        .load()
        .expect_err("corrupt state must not load as empty");
    assert_eq!(err.code(), "INVALID_STATE_FILE");
    // The file is left in place: it is evidence about children that may still
    // be running.
    assert!(store.path().exists());
}

#[test]
fn state_recorded_for_another_compose_file_is_refused() {
    // Unreachable through the normal path now that the directory is derived
    // from the compose file — it takes a slug collision, or a state file
    // someone moved. Kept because what it prevents is one project adopting and
    // later killing another's children.
    let state = DaemonState::new(Path::new("/srv/a/compose.yaml"), "ns");

    state
        .check_binding(Path::new("/srv/a/compose.yaml"))
        .unwrap();
    let err = state
        .check_binding(Path::new("/srv/b/compose.yaml"))
        .expect_err("state belongs to the file it recorded");
    assert_eq!(err.code(), "INVALID_STATE_FILE");
}

#[test]
fn a_project_is_its_file_and_two_files_never_share_a_directory() {
    use iii_compose::state::project_slug;

    // Readable, so `~/.iii/compose` can be browsed, and unique, so two
    // projects whose directories happen to share a name stay apart.
    let a = project_slug(Path::new("/srv/orders/worker-compose.yaml"));
    let b = project_slug(Path::new("/opt/orders/worker-compose.yaml"));

    assert!(a.starts_with("orders-"), "unexpected slug: {a}");
    assert!(b.starts_with("orders-"), "unexpected slug: {b}");
    assert_ne!(a, b, "two different files must not share a state directory");

    // And the same file is the same project however often it is asked for.
    assert_eq!(
        a,
        project_slug(Path::new("/srv/orders/worker-compose.yaml"))
    );
}

#[tokio::test]
async fn a_surviving_child_is_re_adopted() {
    let tmp = tempfile::tempdir().unwrap();
    let compose = tmp.path().join("worker-compose.yaml");
    let child = spawn("while :; do sleep 0.05; done", tmp.path());

    // The daemon dies here: state is on disk, the child keeps running.
    let store = StateStore::at(tmp.path().join("host-a"));
    store.save(&state_with(&child, &compose)).unwrap();

    let reloaded = store.load().unwrap().expect("state should reload");
    let record = &reloaded.containers["api"];
    assert_eq!(
        iii_compose::state::reconcile(record),
        Reconciliation::Adopt,
        "a live child must be adopted, or teardown walks past it"
    );

    child.stop(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn a_child_that_died_while_the_daemon_was_away_is_reported_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let compose = tmp.path().join("worker-compose.yaml");
    let child = spawn("exit 0", tmp.path());
    let state = state_with(&child, &compose);
    child.wait().await;

    let record = &state.containers["api"];
    assert_eq!(iii_compose::state::reconcile(record), Reconciliation::Gone);
}

#[tokio::test]
async fn a_recycled_pid_is_never_adopted_and_never_signalled() {
    let tmp = tempfile::tempdir().unwrap();
    let child = spawn("while :; do sleep 0.05; done", tmp.path());

    // A forged record: the PID is alive, but it is not the process that was
    // recorded. This is what a restart after a PID wrap looks like.
    let forged = ChildRecord::new(
        child.pid,
        BirthIdentity::StartTime(u64::MAX),
        ChildStatus::Ready,
    );

    assert_eq!(
        iii_compose::state::reconcile(&forged),
        Reconciliation::Unverifiable
    );
    assert!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(child.pid as i32), None).is_ok(),
        "reconcile must not signal a process it cannot verify"
    );

    child.stop(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn an_unverifiable_fingerprint_is_never_adopted() {
    let tmp = tempfile::tempdir().unwrap();
    let child = spawn("while :; do sleep 0.05; done", tmp.path());

    // A record written where no fingerprint was available. The PID is live, and
    // it still resolves to Unverifiable rather than Adopt.
    let record = ChildRecord::new(child.pid, BirthIdentity::Unavailable, ChildStatus::Ready);
    assert_eq!(
        iii_compose::state::reconcile(&record),
        Reconciliation::Unverifiable
    );

    child.stop(Duration::from_secs(2)).await;
}

#[test]
fn a_clean_shutdown_clears_the_state() {
    let tmp = tempfile::tempdir().unwrap();
    let store = StateStore::at(tmp.path().join("host-a"));
    store
        .save(&DaemonState::new(Path::new("/srv/app/c.yaml"), "ns"))
        .unwrap();

    store.clear().unwrap();
    assert_eq!(store.load().unwrap(), None);
    // Clearing twice is not an error: shutdown paths run more than once.
    store.clear().unwrap();
}
