// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! One project: a compose file, the children it started, and the state that
//! outlives them.
//!
//! A project owns everything scoped to one `worker-compose.yaml` — its
//! namespace, its supervision, its durable record. What it does not own is the
//! engine connection: one daemon serves many projects over a single socket, so
//! the client is shared and every project addresses its own namespace through
//! it.
//!
//! It owns three things that must not drift apart — the processes it started,
//! the durable record of them, and what the engine believes. Every mutation
//! goes through [`Project`] so the state file is written on the same path that
//! changed the processes.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use tokio::sync::Mutex;

use crate::{
    config::ComposeFile,
    engine::EngineClient,
    error::Result,
    lifecycle::{self, Children, LifecycleCtx, OpResult},
    namespace::project_namespace,
    process::Supervised,
    state::{ChildStatus, DaemonState, Reconciliation, StateStore, reconcile},
};

pub struct Project {
    pub file: ComposeFile,
    /// Namespace the containers register in. Resolved from the file, and
    /// unrelated to `id` — the id addresses the project, the namespace
    /// addresses its workers.
    pub project_namespace: String,
    pub engine_url: String,
    /// Shared with every other project on this daemon: one socket, many
    /// projects.
    engine: Arc<EngineClient>,
    store: StateStore,
    inner: Mutex<Inner>,
}

/// How often the supervisor checks whether a ready child is still alive.
/// Fast enough that a crash is reported while the operator is still watching,
/// slow enough that an idle project costs nothing.
const SUPERVISION_INTERVAL: Duration = Duration::from_millis(250);

/// The parts that change together. One lock: a caller must never see the
/// children and the records disagree.
struct Inner {
    children: Children,
    state: DaemonState,
}

impl Project {
    /// Loads a project, adopts whatever survived a previous run, and returns
    /// it ready to be operated.
    ///
    /// A project is its compose file. Its namespace comes from that file's
    /// `name:` and addresses its workers; the two are different questions and
    /// nothing else names the project.
    pub async fn open(
        daemon_namespace: &str,
        file: ComposeFile,
        engine: Arc<EngineClient>,
        engine_url: String,
    ) -> Result<Arc<Self>> {
        let project_namespace = project_namespace(None, file.namespace.as_deref());
        let store = StateStore::for_project(daemon_namespace, &file.path)?;

        let recovered = store.load()?;
        if let Some(state) = &recovered {
            // The directory is derived from the path, so this only fires on a
            // slug collision — and adopting another project's children is
            // exactly what it must not do.
            state.check_binding(&file.path)?;
        }
        let mut state =
            recovered.unwrap_or_else(|| DaemonState::new(&file.path, &project_namespace));
        state.namespace = project_namespace.clone();

        let project = Arc::new(Self {
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

        project.reconcile_recovered().await;
        Ok(project)
    }

    /// Re-checks every ready container against the engine after the connection
    /// came back.
    ///
    /// Children get their own `startup_timeout` to reappear: their SDKs
    /// reconnect with backoff, so a container missing one second after the
    /// daemon reconnects says nothing. One that never comes back is failed and
    /// cascaded, exactly like a container that exited — from the project's side
    /// the two are the same outage.
    pub(crate) async fn reconcile_after_reconnect(&self) {
        daemon_line(
            &self.project_namespace,
            "engine connection restored; re-checking the project",
            Tone::Plain,
        );

        let running: Vec<String> = {
            let inner = self.inner.lock().await;
            inner
                .children
                .iter()
                .filter(|(_, child)| matches!(child.poll(), crate::process::Outcome::Running))
                .map(|(key, _)| key.clone())
                .collect()
        };

        for key in running {
            let budget = self
                .file
                .containers
                .get(&key)
                .map(|container| container.startup_timeout)
                .unwrap_or(SUPERVISION_INTERVAL);

            if self.wait_for_reregistration(&key, budget).await {
                continue;
            }

            daemon_line(
                &self.project_namespace,
                &format!("{key} did not register again after the reconnect"),
                Tone::Warn,
            );
            self.down(Some(&key), format!("reconnect:{key}")).await;

            let mut inner = self.inner.lock().await;
            if let Some(entry) = inner.state.containers.get_mut(&key) {
                entry.status = ChildStatus::Failed;
                entry.last_error =
                    Some("did not register again after the engine reconnect".to_string());
            }
            let snapshot = inner.state.clone();
            drop(inner);
            let _ = self.store.save(&snapshot);
        }
    }

    /// Polls the engine until `key` is registered again, or the budget runs
    /// out. Never holds the lock across the wait — `up` and `status` have to
    /// stay answerable while a reconnect settles.
    pub(crate) async fn wait_for_reregistration(&self, key: &str, budget: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            if self
                .engine
                .is_registered(&self.project_namespace, key)
                .await
                .unwrap_or(false)
            {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(SUPERVISION_INTERVAL).await;
        }
    }

    /// Reacts to children that ended without anybody asking them to.
    ///
    /// `up` and `down` hold the lock for their whole run, so this only ever
    /// observes the gaps between operations — a deliberate stop removes the
    /// child from the map before signalling it, and is never seen here.
    pub(crate) async fn reap_unexpected_exits(&self) {
        let dead: Vec<(String, i32)> = {
            let inner = self.inner.lock().await;
            inner
                .children
                .iter()
                .filter_map(|(key, child)| match child.poll() {
                    crate::process::Outcome::Exited(status) => {
                        Some((key.clone(), status.code().unwrap_or(-1)))
                    }
                    crate::process::Outcome::Running => None,
                })
                .collect()
        };

        for (key, code) in dead {
            let reason = format!("exited unexpectedly with {code}");
            daemon_line(
                &self.project_namespace,
                &format!("{key} {reason}"),
                Tone::Warn,
            );

            // Cascade through the same path a targeted `down` takes: it stops
            // the dependents first and ends on the dead container itself, which
            // fires its `post_run` and drops it from the map. Leaving dependents
            // running would leave them talking to something that is gone.
            let dependents = crate::dag::transitive_dependents(&self.file, &key);
            if !dependents.is_empty() {
                daemon_line(
                    &self.project_namespace,
                    &format!("stopping what depended on {key}: {}", dependents.join(", ")),
                    Tone::Warn,
                );
            }
            self.down(Some(&key), format!("supervisor:{key}")).await;

            // After the cascade, so `down` marking everything Stopped does not
            // erase why this one went.
            let mut inner = self.inner.lock().await;
            if let Some(entry) = inner.state.containers.get_mut(&key) {
                entry.status = ChildStatus::Failed;
                entry.last_error = Some(reason);
            }
            let snapshot = inner.state.clone();
            drop(inner);
            let _ = self.store.save(&snapshot);
        }
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
    /// A survivor is adopted back into `children`, not merely reported. The
    /// handle is gone with the daemon that spawned it, so the adopted process
    /// is polled rather than waited on — but it lands in the same map as every
    /// other child, which is what `down` walks. Leaving it out was a silent
    /// leak: `stop_one` returns early on a key it does not hold, so teardown
    /// reported success over a process that kept running and kept its name.
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
                Reconciliation::Adopt => match Supervised::adopt(record.pid, &record.birth) {
                    Some(child) => {
                        daemon_line(
                            &self.project_namespace,
                            &format!("{key} survived (pid {}), adopted", record.pid),
                            Tone::Plain,
                        );
                        inner.children.insert(key.clone(), child);
                    }
                    // `reconcile` verified the identity a moment ago, so this
                    // means the process exited in between. Record it as gone
                    // rather than claiming an adoption that did not happen.
                    None => {
                        daemon_line(
                            &self.project_namespace,
                            &format!("{key} exited while it was being adopted"),
                            Tone::Warn,
                        );
                        if let Some(entry) = inner.state.containers.get_mut(&key) {
                            entry.status = ChildStatus::Failed;
                            entry.last_error =
                                Some("exited while it was being adopted".to_string());
                        }
                    }
                },
                Reconciliation::Gone => {
                    daemon_line(
                        &self.project_namespace,
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
                        &self.project_namespace,
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

    /// Everything this project owns on disk: its durable record, the
    /// configuration it was handed, and each container's output.
    ///
    /// Reported by `compose::status` because the directory is derived from the
    /// compose file rather than named by anyone — so asking is the only way to
    /// know, and an operator looking for a container's log should not have to
    /// reproduce a hash to find it.
    pub fn state_dir(&self) -> &Path {
        self.store.dir()
    }

    fn config_dir(&self) -> PathBuf {
        self.store.dir().join("config")
    }

    /// Where each container's own output is written. Beside the project's
    /// state, so a project that is removed takes its logs with it.
    fn log_dir(&self) -> PathBuf {
        self.store.dir().join("logs")
    }

    /// Per-container VM state for bundle containers: rootfs, boot script, pid
    /// file. Keyed by project rather than by worker name, so two projects
    /// running the same bundle under the same container key stay apart.
    fn vm_dir(&self) -> PathBuf {
        self.store.dir().join("vm")
    }

    /// Installed packages live at the root, not inside a daemon or a project:
    /// the same `state 0.21.4` serves every project on this machine, and
    /// deriving this by walking up from the state directory would silently
    /// re-scope it the next time that layout gains a level.
    fn package_cache(&self) -> PathBuf {
        StateStore::root()
            .unwrap_or_else(|_| self.store.dir().to_path_buf())
            .join("packages")
    }

    pub async fn up(&self, target: Option<&str>, operation_id: String) -> OpResult {
        let config_dir = self.config_dir();
        let log_dir = self.log_dir();
        let package_cache = self.package_cache();
        let vm_dir = self.vm_dir();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;

        let ctx = LifecycleCtx {
            file: &self.file,
            engine: &self.engine,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
            log_dir: &log_dir,
            package_cache: &package_cache,
            vm_dir: &vm_dir,
        };

        let result =
            lifecycle::up(&ctx, children, &mut state.containers, target, operation_id).await;

        let snapshot = state.clone();
        drop(inner);
        let _ = self.store.save(&snapshot);
        result
    }

    /// Bounces one container. See [`lifecycle::restart_one`] for why this does
    /// not take the container's graph with it.
    pub async fn restart_one(&self, key: &str, operation_id: String) -> OpResult {
        let config_dir = self.config_dir();
        let log_dir = self.log_dir();
        let package_cache = self.package_cache();
        let vm_dir = self.vm_dir();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;

        let ctx = LifecycleCtx {
            file: &self.file,
            engine: &self.engine,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
            log_dir: &log_dir,
            package_cache: &package_cache,
            vm_dir: &vm_dir,
        };

        let result =
            lifecycle::restart_one(&ctx, children, &mut state.containers, key, operation_id).await;

        let snapshot = state.clone();
        drop(inner);
        let _ = self.store.save(&snapshot);
        result
    }

    pub async fn down(&self, target: Option<&str>, operation_id: String) -> OpResult {
        let config_dir = self.config_dir();
        let log_dir = self.log_dir();
        let package_cache = self.package_cache();
        let vm_dir = self.vm_dir();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;

        let ctx = LifecycleCtx {
            file: &self.file,
            engine: &self.engine,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
            log_dir: &log_dir,
            package_cache: &package_cache,
            vm_dir: &vm_dir,
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

    /// Leaves without touching what was not started here.
    ///
    /// Used when the engine refuses this identity: another daemon already holds
    /// this `--id`, which means the recorded children are *its* children. This
    /// process adopted them a moment ago on the assumption it was the owner, so
    /// it must hand them back — stopping them here would take down a healthy
    /// project on the way out of a failed start. The state file is left alone
    /// for the same reason.
    pub async fn abandon(&self) {
        let mut inner = self.inner.lock().await;
        let spawned: Vec<String> = inner
            .children
            .iter()
            .filter(|(_, child)| !child.is_adopted())
            .map(|(key, _)| key.clone())
            .collect();

        // Anything this process spawned is its own mess to clean up; anything
        // it adopted is dropped, which for an adopted handle signals nothing.
        for key in spawned {
            if let Some(child) = inner.children.remove(&key) {
                child.stop(self.file.stop_timeout).await;
            }
        }
        inner.children.clear();
        drop(inner);

        self.engine.shutdown().await;
    }

    /// Intentional shutdown: stop every local child, then clear the state.
    /// A daemon that exits on purpose leaves nothing behind to reconcile.
    pub async fn shutdown(&self) {
        let operation_id = "shutdown".to_string();
        self.down(None, operation_id).await;
        let _ = self.store.clear();
        self.engine.shutdown().await;
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

/// Severity of a project log line. Amber means "look at this", not "it broke".
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
        Tone::Plain => crate::report::line(&format!("{prefix} {message}")),
        Tone::Warn => crate::report::line(&format!("{prefix} {}", message.yellow())),
    }
}
