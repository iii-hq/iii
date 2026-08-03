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

use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use tokio::sync::Mutex;

use crate::{
    config::ComposeFile,
    engine::EngineClient,
    error::{ComposeError, Result},
    lifecycle::{self, Children, LifecycleCtx, OpResult},
    namespace::project_namespace,
    process::Supervised,
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
    /// What the children printed, for `compose::logs`.
    logs: Arc<crate::logs::LogStore>,
    /// Set by `compose::stop`. The serve loop reads it and leaves through the
    /// same path a SIGTERM takes, so a remote stop and a local one cannot
    /// diverge in what they tear down.
    stop_requested: std::sync::atomic::AtomicBool,
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
            logs: Arc::new(crate::logs::LogStore::new()),
            stop_requested: std::sync::atomic::AtomicBool::new(false),
        });

        daemon.reconcile_recovered().await;
        Self::supervise(&daemon);
        Ok(daemon)
    }

    /// Starts the loop that notices a child dying after it was ready.
    ///
    /// One poll loop, not a task per child: `Supervised::wait` borrows a handle
    /// that lives behind the same lock every operation takes, so awaiting it
    /// there would hold the lock for the child's whole lifetime. Polling holds
    /// it for microseconds, and treats spawned and adopted children alike —
    /// an adopted process has no reaper to wait on in the first place.
    ///
    /// A `Weak` so the loop does not keep the daemon alive: when the last real
    /// reference goes, the upgrade fails and the task ends.
    fn supervise(daemon: &Arc<Self>) {
        let weak = Arc::downgrade(daemon);
        tokio::spawn(async move {
            let mut was_connected = true;
            loop {
                tokio::time::sleep(SUPERVISION_INTERVAL).await;
                let Some(daemon) = weak.upgrade() else { return };

                // The SDK reconnects and replays the daemon's own
                // registrations, but nothing re-checks the children. An engine
                // that restarted came back with an empty registry, and a worker
                // whose own reconnect failed is alive, unregistered, and
                // unreachable — while status would still call it ready.
                let connected = daemon.engine.is_connected();
                if connected && !was_connected {
                    daemon.reconcile_after_reconnect().await;
                }
                was_connected = connected;

                daemon.reap_unexpected_exits().await;
            }
        });
    }

    /// Re-checks every ready container against the engine after the connection
    /// came back.
    ///
    /// Children get their own `startup_timeout` to reappear: their SDKs
    /// reconnect with backoff, so a container missing one second after the
    /// daemon reconnects says nothing. One that never comes back is failed and
    /// cascaded, exactly like a container that exited — from the project's side
    /// the two are the same outage.
    async fn reconcile_after_reconnect(&self) {
        daemon_line(
            &self.id,
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
                &self.id,
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
    async fn wait_for_reregistration(&self, key: &str, budget: Duration) -> bool {
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
    async fn reap_unexpected_exits(&self) {
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
            daemon_line(&self.id, &format!("{key} {reason}"), Tone::Warn);

            // Cascade through the same path a targeted `down` takes: it stops
            // the dependents first and ends on the dead container itself, which
            // fires its `post_run` and drops it from the map. Leaving dependents
            // running would leave them talking to something that is gone.
            let dependents = crate::dag::transitive_dependents(&self.file, &key);
            if !dependents.is_empty() {
                daemon_line(
                    &self.id,
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
                            &self.id,
                            &format!("{key} survived (pid {}), adopted", record.pid),
                            Tone::Plain,
                        );
                        inner.children.insert(key.clone(), child);
                        // Its pipes belonged to the daemon that spawned it and
                        // died with it. Saying so beats an empty log that reads
                        // as a silent worker.
                        self.logs.append(
                            &key,
                            crate::logs::Stream::Compose,
                            "adopted from an earlier daemon; output before this point is gone"
                                .to_string(),
                        );
                    }
                    // `reconcile` verified the identity a moment ago, so this
                    // means the process exited in between. Record it as gone
                    // rather than claiming an adoption that did not happen.
                    None => {
                        daemon_line(
                            &self.id,
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

    /// Installed packages live beside the daemons, not inside one: the same
    /// `state 0.21.4` serves every project on this machine.
    fn package_cache(&self) -> PathBuf {
        self.store
            .dir()
            .parent()
            .map(|root| root.join("packages"))
            .unwrap_or_else(|| self.store.dir().join("packages"))
    }

    pub async fn up(&self, target: Option<&str>, operation_id: String) -> OpResult {
        let config_dir = self.config_dir();
        let package_cache = self.package_cache();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;

        let ctx = LifecycleCtx {
            file: &self.file,
            engine: &self.engine,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
            package_cache: &package_cache,
            logs: &self.logs,
        };

        let result =
            lifecycle::up(&ctx, children, &mut state.containers, target, operation_id).await;

        let snapshot = state.clone();
        drop(inner);
        let _ = self.store.save(&snapshot);
        result
    }

    pub async fn down(&self, target: Option<&str>, operation_id: String) -> OpResult {
        let config_dir = self.config_dir();
        let package_cache = self.package_cache();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;

        let ctx = LifecycleCtx {
            file: &self.file,
            engine: &self.engine,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
            package_cache: &package_cache,
            logs: &self.logs,
        };

        let result =
            lifecycle::down(&ctx, children, &mut state.containers, target, operation_id).await;

        let snapshot = state.clone();
        drop(inner);
        let _ = self.store.save(&snapshot);
        result
    }

    /// Asks the daemon to shut down, and reports what it is about to stop.
    ///
    /// The teardown itself happens on the serve loop rather than here: the
    /// caller is waiting on this invocation, and a daemon that tore its engine
    /// connection down mid-reply would leave them holding a broken socket
    /// instead of an answer.
    pub async fn request_stop(&self) -> serde_json::Value {
        self.stop_requested
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let inner = self.inner.lock().await;
        let stopping: Vec<String> = inner.children.keys().cloned().collect();
        drop(inner);

        serde_json::json!({
            "daemon_id": self.id,
            "daemon_pid": std::process::id(),
            "stopping": stopping,
        })
    }

    /// Whether `compose::stop` has been called.
    pub fn stop_requested(&self) -> bool {
        self.stop_requested.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Recent output of one container, or of every container that produced
    /// any. Bounded by the store: this is the tail, never the whole history.
    pub fn logs(&self, container: Option<&str>, tail: usize) -> serde_json::Value {
        match container {
            Some(key) => serde_json::json!({ key: self.logs.tail(key, tail) }),
            None => serde_json::to_value(self.logs.tail_all(tail)).unwrap_or_default(),
        }
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
        Tone::Plain => crate::report::line(&format!("{prefix} {message}")),
        Tone::Warn => crate::report::line(&format!("{prefix} {}", message.yellow())),
    }
}
