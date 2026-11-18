// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! These tests call the real Project::status acquisition and projection paths.
//! No engine connections, subprocesses, VM adapters, sleeps or readiness polling.

use futures::FutureExt;
use tokio::sync::watch;

use super::*;
use crate::{process::BirthIdentity, state::ChildRecord};

const PID: u32 = 42_123;

struct ProjectFixture {
    project: Project,
    _dir: tempfile::TempDir,
}

impl ProjectFixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("worker-compose.yaml");
        let file = ComposeFile::parse(
            "containers:\n  app: {worker: 'path://.', scripts: {run: unused}}\n",
            &file_path,
        )
        .unwrap();
        let store = StateStore::at(dir.path().join("state"));
        let logs = LogStore::open(dir.path().join("logs")).unwrap();
        let state = DaemonState::new(&file_path, "status-test");
        Self {
            project: Project {
                file: RwLock::new(file),
                file_path,
                compose_namespace: "status-test".into(),
                project_namespace: "status-test".into(),
                engine_url: "unused".into(),
                engine: Arc::new(EngineClient::disconnected()),
                post_runs: crate::hooks::PostRunSupervisor::default(),
                logs,
                store,
                sandbox_token: "unused".into(),
                legacy_vms: std::sync::Mutex::new(BTreeMap::new()),
                inner: Mutex::new(Inner {
                    children: BTreeMap::new(),
                    state,
                    restarts: RestartBookkeeping::default(),
                }),
                shutdown: crate::shutdown::ShutdownController::default(),
            },
            _dir: dir,
        }
    }
}

fn record(status: ChildStatus) -> ChildRecord {
    ChildRecord::new(PID, BirthIdentity::Unavailable, status)
}

fn registered_child(inner: &mut Inner) -> watch::Sender<Option<ExitStatus>> {
    let (exit, receiver) = watch::channel(None);
    inner
        .children
        .insert("app".into(), Supervised::with_test_exit(PID, receiver));
    inner
        .state
        .containers
        .insert("app".into(), record(ChildStatus::Starting));
    exit
}

async fn assert_waits_for_commit(persisted: Option<ChildStatus>) {
    let fixture = ProjectFixture::new();
    let project = &fixture.project;
    if let Some(status) = persisted {
        let mut stored = DaemonState::new(project.file_path(), "status-test");
        stored.containers.insert("app".into(), record(status));
        project.store.save(&stored).unwrap();
    }
    let stored_before = project.store.load().unwrap();

    // Stand at up's commit barrier: the child already exists, but up still
    // owns inner and has not committed the ready record or persisted it.
    let mut inner = project.inner.lock().await;
    let _exit = registered_child(&mut inner);
    let mut status = Box::pin(project.status());
    assert!(
        futures::poll!(status.as_mut()).is_pending(),
        "lock contention must not publish stopped/unowned from persisted state"
    );

    inner.state.containers.get_mut("app").unwrap().status = ChildStatus::Ready;
    drop(inner);
    let rows = status
        .now_or_never()
        .expect("the committed in-memory snapshot is immediately available")
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].state, rows[0].owned, rows[0].pid),
        (ChildStatus::Ready, true, Some(PID))
    );
    assert_eq!(project.store.load().unwrap(), stored_before);
    assert!(project.inner.try_lock().is_ok());
    assert!(project.file.try_write().is_ok());
}

#[tokio::test]
async fn status_waits_for_current_commit_with_no_persisted_state() {
    assert_waits_for_commit(None).await;
}

#[tokio::test]
async fn status_waits_for_current_commit_with_persisted_stopped_state() {
    assert_waits_for_commit(Some(ChildStatus::Stopped)).await;
}

#[tokio::test]
async fn status_does_not_hold_file_while_waiting_for_reconcile_inner() {
    let fixture = ProjectFixture::new();
    let project = &fixture.project;
    let mut inner = project.inner.lock().await;
    let _exit = registered_child(&mut inner);
    let mut status = Box::pin(project.status());
    assert!(futures::poll!(status.as_mut()).is_pending());

    // Reconcile's exact lock order, while status is positively waiting.
    // try_write proves this is possible without a hang-prone timeout oracle.
    let mut file = project
        .file
        .try_write()
        .expect("a status waiter must not block reconcile's file writer");
    *file = ComposeFile::parse(
        "containers:\n  app: {worker: 'path://.', scripts: {run: unused}}\n  \
         added: {worker: 'path://.', scripts: {run: unused}}\n",
        project.file_path(),
    )
    .unwrap();
    inner.state.containers.get_mut("app").unwrap().status = ChildStatus::Ready;
    drop(file);
    drop(inner);

    let rows = status
        .now_or_never()
        .expect("reconcile released both locks")
        .unwrap();
    assert_eq!(rows.len(), 2);
    let app = rows.iter().find(|row| row.container == "app").unwrap();
    let added = rows.iter().find(|row| row.container == "added").unwrap();
    assert_eq!(
        (app.state, app.owned, app.pid),
        (ChildStatus::Ready, true, Some(PID))
    );
    assert_eq!(
        (added.state, added.owned, added.pid),
        (ChildStatus::Stopped, false, None)
    );
}

#[tokio::test]
async fn status_shutdown_cancels_waiting_for_inner_without_rows() {
    let fixture = ProjectFixture::new();
    let project = &fixture.project;
    let _inner = project.inner.lock().await;
    let mut status = Box::pin(project.status());
    assert!(futures::poll!(status.as_mut()).is_pending());

    project.shutdown.cancel();
    let error = status
        .now_or_never()
        .expect("cancellation is latched")
        .unwrap_err();
    assert_eq!(error.code(), "OPERATION_CANCELLED");
    assert!(project.file.try_write().is_ok());
}

#[tokio::test]
async fn status_shutdown_cancels_waiting_for_file_and_releases_inner() {
    let fixture = ProjectFixture::new();
    let project = &fixture.project;
    let _file = project.file.write().await;
    let mut status = Box::pin(project.status());
    assert!(futures::poll!(status.as_mut()).is_pending());
    assert!(project.inner.try_lock().is_err());

    project.shutdown.cancel();
    let error = status
        .now_or_never()
        .expect("cancellation is latched")
        .unwrap_err();
    assert_eq!(error.code(), "OPERATION_CANCELLED");
    assert!(project.inner.try_lock().is_ok());
}

#[tokio::test]
async fn status_already_cancelled_rejects_even_an_accessible_snapshot() {
    let fixture = ProjectFixture::new();
    fixture.project.shutdown.cancel();
    assert_eq!(
        fixture.project.status().await.unwrap_err().code(),
        "OPERATION_CANCELLED"
    );
}

#[tokio::test]
async fn status_dropped_request_releases_inner_while_waiting_for_file() {
    let fixture = ProjectFixture::new();
    let project = &fixture.project;
    let _file = project.file.write().await;
    let mut status = Box::pin(project.status());
    assert!(futures::poll!(status.as_mut()).is_pending());
    assert!(project.inner.try_lock().is_err());

    // The RPC deadline drops this same future, without cancelling lifecycle work.
    drop(status);
    assert!(project.inner.try_lock().is_ok());
    assert!(!project.shutdown.signal().requested());
}

#[tokio::test]
async fn status_quiescent_project_is_genuinely_stopped_and_unowned() {
    let fixture = ProjectFixture::new();
    let rows = fixture.project.status().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].state, rows[0].owned, rows[0].pid),
        (ChildStatus::Stopped, false, None)
    );
    let json = serde_json::to_value(&rows[0]).unwrap();
    assert_eq!(json["state"], "stopped");
    assert_eq!(json["owned"], false);
    assert!(json.get("pid").is_none());
    assert!(json.get("busy").is_none());
}

#[tokio::test]
async fn status_ignores_persisted_ready_and_preserves_current_record_semantics() {
    let fixture = ProjectFixture::new();
    let project = &fixture.project;
    let mut stored = DaemonState::new(project.file_path(), "status-test");
    stored
        .containers
        .insert("app".into(), record(ChildStatus::Ready));
    project.store.save(&stored).unwrap();

    for current in [
        None,
        Some(ChildStatus::Stopped),
        Some(ChildStatus::Failed),
        Some(ChildStatus::Ready),
    ] {
        let mut inner = project.inner.lock().await;
        inner.state.containers.clear();
        if let Some(state) = current {
            let mut record = record(state);
            record.last_error = Some("current record".into());
            inner.state.containers.insert("app".into(), record);
        }
        drop(inner);
        let rows = project.status().await.unwrap();
        assert_eq!(
            (rows[0].state, rows[0].owned, rows[0].pid),
            (current.unwrap_or(ChildStatus::Stopped), false, None)
        );
        assert_eq!(
            rows[0].last_error.as_deref(),
            current.map(|_| "current record")
        );
    }
}

#[tokio::test]
async fn status_reaped_child_keeps_ownership_but_not_a_running_pid() {
    let fixture = ProjectFixture::new();
    let project = &fixture.project;
    let mut inner = project.inner.lock().await;
    let exit = registered_child(&mut inner);
    inner.state.containers.get_mut("app").unwrap().status = ChildStatus::Failed;
    #[cfg(unix)]
    let ended = <ExitStatus as std::os::unix::process::ExitStatusExt>::from_raw(1 << 8);
    #[cfg(windows)]
    let ended = <ExitStatus as std::os::windows::process::ExitStatusExt>::from_raw(1);
    exit.send_replace(Some(ended));
    drop(inner);

    let rows = project.status().await.unwrap();
    assert_eq!(
        (rows[0].state, rows[0].owned, rows[0].pid),
        (ChildStatus::Failed, true, None)
    );
}
