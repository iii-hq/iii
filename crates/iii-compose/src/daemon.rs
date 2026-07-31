// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The daemon: one compose file, one engine connection, one set of children.
//!
//! It owns three things that must not drift apart — the processes it started,
//! the durable record of them, and what the engine believes. Every mutation
//! goes through [`Daemon`] so the state file is written on the same path that
//! changed the processes.

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use tokio::sync::Mutex;

use crate::{
    config::ComposeFile,
    engine::EngineClient,
    error::{ComposeError, Result},
    lifecycle::{self, Children, LifecycleCtx, OpResult},
    namespace::project_namespace,
    state::{ChildStatus, DaemonState, Reconciliation, StateStore, reconcile},
};

pub struct Daemon {
    pub id: String,
    pub file: ComposeFile,
    /// Namespace the children register in. Independent of the daemon's own,
    /// which is `id`.
    pub project_namespace: String,
    pub engine_url: String,
    engine: EngineClient,
    store: StateStore,
    inner: Mutex<Inner>,
}

/// The parts that change together. One lock: a caller must never see the
/// children and the records disagree.
struct Inner {
    children: Children,
    state: DaemonState,
}

impl Daemon {
    /// Connects, adopts whatever survived a previous run, and returns a daemon
    /// ready to serve `compose::*`.
    pub async fn start(
        id: String,
        file: ComposeFile,
        engine_url: String,
        namespace_override: Option<&str>,
    ) -> Result<Arc<Self>> {
        let project_namespace = project_namespace(namespace_override, &file.name, &file.path);
        let store = StateStore::for_daemon(&id)?;

        let recovered = store.load()?;
        if let Some(state) = &recovered {
            // A daemon id is bound to one compose file for its lifetime.
            state.check_binding(&file.path)?;
        }
        let mut state =
            recovered.unwrap_or_else(|| DaemonState::new(&id, &file.path, &project_namespace));
        state.namespace = project_namespace.clone();

        let engine = EngineClient::connect(&engine_url, &id);

        let daemon = Arc::new(Self {
            id,
            file,
            project_namespace,
            engine_url,
            engine,
            store,
            inner: Mutex::new(Inner {
                children: BTreeMap::new(),
                state,
            }),
        });

        daemon.reconcile_recovered().await;
        Ok(daemon)
    }

    /// Fatal registration rejection, if the engine refused this daemon —
    /// another daemon already holds this `--id`.
    pub fn fatal_error(&self) -> Option<iii_sdk::Error> {
        self.engine.fatal_error()
    }

    pub fn engine(&self) -> &EngineClient {
        &self.engine
    }

    /// Classifies every recorded child from the previous run.
    ///
    /// A survivor is *reported*, not re-adopted into `children`: this daemon
    /// never took ownership of that process handle, so it cannot wait on it.
    /// It stays recorded, and `down` can still reach it by pid once supervision
    /// re-adoption lands.
    async fn reconcile_recovered(&self) {
        let mut inner = self.inner.lock().await;
        let recorded: Vec<(String, crate::state::ChildRecord)> = inner
            .state
            .containers
            .iter()
            .map(|(key, record)| (key.clone(), record.clone()))
            .collect();

        for (key, record) in recorded {
            match reconcile(&record) {
                Reconciliation::Adopt => {
                    daemon_line(
                        &self.id,
                        &format!(
                            "{key} survived (pid {}), still recorded as ready",
                            record.pid
                        ),
                        Tone::Plain,
                    );
                }
                Reconciliation::Gone => {
                    daemon_line(
                        &self.id,
                        &format!("{key} exited while the daemon was away"),
                        Tone::Warn,
                    );
                    if let Some(entry) = inner.state.containers.get_mut(&key) {
                        entry.status = ChildStatus::Failed;
                        entry.last_error = Some("exited while the daemon was away".to_string());
                    }
                }
                Reconciliation::Unverifiable => {
                    // Never signalled: the pid is alive but unproven.
                    daemon_line(
                        &self.id,
                        &format!(
                            "{key}: pid {} is alive but is not provably ours; left running \
                             for manual cleanup",
                            record.pid
                        ),
                        Tone::Warn,
                    );
                    if let Some(entry) = inner.state.containers.get_mut(&key) {
                        entry.status = ChildStatus::Failed;
                        entry.last_error =
                            Some(format!("pid {} could not be verified", record.pid));
                    }
                }
            }
        }

        let state = inner.state.clone();
        drop(inner);
        let _ = self.store.save(&state);
    }

    fn config_dir(&self) -> PathBuf {
        self.store.dir().join("config")
    }

    pub async fn up(&self, target: Option<&str>, operation_id: String) -> OpResult {
        let config_dir = self.config_dir();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;

        let ctx = LifecycleCtx {
            file: &self.file,
            engine: &self.engine,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
        };

        let result =
            lifecycle::up(&ctx, children, &mut state.containers, target, operation_id).await;

        let snapshot = state.clone();
        drop(inner);
        let _ = self.store.save(&snapshot);
        // The caller of a remote `up` gets the structured result, but whoever
        // is watching the daemon's terminal gets nothing unless we say it here.
        report_failure(&result);
        result
    }

    pub async fn down(&self, target: Option<&str>, operation_id: String) -> OpResult {
        let config_dir = self.config_dir();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;

        let ctx = LifecycleCtx {
            file: &self.file,
            engine: &self.engine,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
        };

        let result =
            lifecycle::down(&ctx, children, &mut state.containers, target, operation_id).await;

        let snapshot = state.clone();
        drop(inner);
        let _ = self.store.save(&snapshot);
        result
    }

    /// Current state of every declared container, for `compose::status`.
    pub async fn status(&self) -> Vec<ContainerStatus> {
        let inner = self.inner.lock().await;
        self.file
            .containers
            .keys()
            .map(|key| {
                let record = inner.state.containers.get(key);
                let running = inner
                    .children
                    .get(key)
                    .is_some_and(|c| matches!(c.poll(), crate::process::Outcome::Running));
                ContainerStatus {
                    container: key.clone(),
                    state: match (running, record.map(|r| r.status)) {
                        (true, _) => ChildStatus::Ready,
                        (false, Some(status)) => status,
                        (false, None) => ChildStatus::Stopped,
                    },
                    pid: record.map(|r| r.pid),
                    owned: inner.children.contains_key(key),
                    last_error: record.and_then(|r| r.last_error.clone()),
                }
            })
            .collect()
    }

    /// Intentional shutdown: stop every local child, then clear the state.
    /// A daemon that exits on purpose leaves nothing behind to reconcile.
    pub async fn shutdown(&self) {
        let operation_id = "shutdown".to_string();
        self.down(None, operation_id).await;
        let _ = self.store.clear();
        self.engine.shutdown().await;
    }

    /// Rejects a request aimed at a different daemon. `id` is optional — the
    /// namespace already selected this daemon — but a mismatch is always an
    /// error rather than a surprise.
    pub fn check_id(&self, requested: Option<&str>, function: &str) -> Result<()> {
        match requested {
            None => Ok(()),
            Some(id) if id == self.id => Ok(()),
            Some(id) => Err(ComposeError::WrongDaemon {
                expected: self.id.clone(),
                got: id.to_string(),
                function: function.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ContainerStatus {
    pub container: String,
    pub state: ChildStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Whether this daemon owns the process (started it and can stop it).
    pub owned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Prints a failed operation to the daemon's own terminal.
///
/// A remote `compose::up` answers the caller and nobody else. Without this, an
/// operator watching the daemon sees a project that silently never came up.
/// Successful operations stay quiet — the caller already knows.
pub fn report_failure(result: &OpResult) {
    use colored::Colorize;

    if result.status != lifecycle::OpStatus::Failed {
        return;
    }

    let code = result
        .containers
        .iter()
        .find_map(|container| container.error.as_ref())
        .map(|error| error.code.as_str())
        .unwrap_or("UP_FAILED");
    eprintln!(
        "{} {}",
        format!("error[{code}]").red().bold(),
        "up failed:".red()
    );

    for container in &result.containers {
        match &container.error {
            // The error's own Display already names the container; printing it
            // twice reads like two different things went wrong.
            Some(error) => eprintln!(
                "  {} {}",
                format!("{}:", container.container).bold(),
                strip_container_prefix(&error.message, &container.container).red()
            ),
            // Everything this operation had started is undone; saying so is
            // the difference between "it is down" and "it was never up". Amber,
            // not red: nothing went wrong with this one.
            None if container.state == ChildStatus::Stopped => eprintln!(
                "  {} {}",
                format!("{}:", container.container).bold(),
                "rolled back".yellow()
            ),
            None => {}
        }
    }
}

/// Drops a leading `container '<name>': ` from a message that is already being
/// printed under that container's name.
fn strip_container_prefix<'a>(message: &'a str, container: &str) -> &'a str {
    let prefix = format!("container '{container}': ");
    message.strip_prefix(&prefix).unwrap_or(message)
}

/// Severity of a daemon log line. Amber means "look at this", not "it broke".
enum Tone {
    Plain,
    Warn,
}

/// The daemon runs in the foreground; its log is stderr, like its children's.
/// The `[compose:<id>]` prefix is dimmed so a project's own output stays the
/// thing that stands out.
fn daemon_line(id: &str, message: &str, tone: Tone) {
    use colored::Colorize;
    let prefix = format!("[compose:{id}]").dimmed();
    match tone {
        Tone::Plain => eprintln!("{prefix} {message}"),
        Tone::Warn => eprintln!("{prefix} {}", message.yellow()),
    }
}
