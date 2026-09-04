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

use tokio::{
    sync::{Mutex, RwLock},
    time::Instant,
};

use crate::{
    config::{ComposeFile, RestartPolicy},
    engine::EngineClient,
    error::{ComposeError, Result},
    lifecycle::{self, Children, LifecycleCtx, OpResult, OpStatus},
    logs::{LogCursor, LogStore, LogStream, LogsOutcome},
    process::Supervised,
    state::{ChildStatus, DaemonState, Reconciliation, StateStore, reconcile},
};

pub struct Project {
    /// Current parsed compose file. Mutations replace it in place so an add
    /// can reconcile new declarations without dropping supervision of the
    /// workers that are already running.
    file: RwLock<ComposeFile>,
    file_path: PathBuf,
    compose_namespace: String,
    /// Namespace the containers register in. Resolved from the file, and
    /// unrelated to `id` — the id addresses the project, the namespace
    /// addresses its workers.
    pub project_namespace: String,
    pub engine_url: String,
    /// Shared with every other project on this daemon: one socket, many
    /// projects.
    engine: Arc<EngineClient>,
    post_runs: crate::hooks::PostRunSupervisor,
    logs: LogStore,
    store: StateStore,
    inner: Mutex<Inner>,
    /// Attempt bookkeeping for the containers the supervisor is currently
    /// retrying, keyed by container.
    ///
    /// Deliberately not in [`DaemonState`], so not on disk. An attempt count
    /// describes one supervisor's patience with one crash loop, and a daemon
    /// that restarts re-reconciles from scratch: carrying the old count over
    /// would let a project come back with its budget already spent.
    restarts: Mutex<BTreeMap<String, RestartAttempts>>,
}

/// How often the supervisor checks whether a ready child is still alive.
/// Fast enough that a crash is reported while the operator is still watching,
/// slow enough that an idle project costs nothing.
const SUPERVISION_INTERVAL: Duration = Duration::from_millis(250);

/// Wait before the second restart attempt. Each further attempt doubles it, up
/// to [`RESTART_BACKOFF_MAX`]. The first attempt does not wait at all: a worker
/// that died because of a transient is back before the operator looks up, and
/// one that is genuinely broken has stopped being hammered within seconds.
const RESTART_BACKOFF_BASE: Duration = Duration::from_millis(500);

/// Ceiling on the wait between attempts. Past this, a longer backoff would only
/// delay the operator's answer without giving the worker anything it needs.
const RESTART_BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Consecutive attempts before the supervisor gives up. On the last one it does
/// what it would have done with no policy at all: fail the container and take
/// its dependents down. A cap is what separates a restart policy from a busy
/// loop.
const RESTART_MAX_ATTEMPTS: u32 = 5;

/// How long a container has to hold `Ready` before its attempt budget refills.
/// Without it, a worker that crashes once an hour would spend five attempts
/// over an afternoon and then never be restarted again, which is the opposite
/// of what its file asked for.
const RESTART_BUDGET_RESET_AFTER: Duration = Duration::from_secs(60);

/// Where one container is in its restart budget.
#[derive(Debug, Clone, Copy)]
struct RestartAttempts {
    /// Attempts already spent in this run of failures.
    spent: u32,
    /// When the next attempt becomes due, and `None` when none is owed — the
    /// container came back, or an operator took it over. The supervision tick
    /// is what makes an attempt due, so the backoff never blocks the loop.
    ///
    /// A container that is not owed an attempt still keeps its `spent` count.
    /// Clearing it on every recovery would refill the budget for a worker that
    /// comes back for a second each time, which is the busy loop wearing a
    /// slower coat. Only [`Project::refill_budget_if_it_held`] clears it.
    due: Option<Instant>,
}

impl RestartAttempts {
    /// Backoff before the attempt after `spent` have been made: doubling from
    /// [`RESTART_BACKOFF_BASE`], held at [`RESTART_BACKOFF_MAX`].
    fn backoff(spent: u32) -> Duration {
        RESTART_BACKOFF_BASE
            .saturating_mul(2u32.saturating_pow(spent.saturating_sub(1)))
            .min(RESTART_BACKOFF_MAX)
    }
}

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
    /// `project_namespace` is already resolved by the daemon so an explicit
    /// CLI namespace cannot be lost while the project is opened.
    pub async fn open(
        daemon_namespace: &str,
        project_namespace: String,
        file: ComposeFile,
        engine: Arc<EngineClient>,
        engine_url: String,
    ) -> Result<Arc<Self>> {
        let store = StateStore::for_project(daemon_namespace, &file.path)?;
        let file_path = file.path.clone();

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
        let log_dir = store.dir().join("logs");
        let logs = LogStore::open(log_dir.clone()).map_err(|source| ComposeError::Io {
            path: log_dir,
            source,
        })?;

        let project = Arc::new(Self {
            file: RwLock::new(file),
            file_path,
            compose_namespace: daemon_namespace.to_string(),
            project_namespace,
            engine_url,
            engine,
            post_runs: crate::hooks::PostRunSupervisor::default(),
            logs,
            store,
            inner: Mutex::new(Inner {
                children: BTreeMap::new(),
                state,
            }),
            restarts: Mutex::new(BTreeMap::new()),
        });

        project.reconcile_recovered().await;
        Ok(project)
    }

    /// Canonical path that identifies this project. The path never changes
    /// when the parsed declaration is refreshed.
    pub fn file_path(&self) -> &Path {
        &self.file_path
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

        let running: Vec<(String, Duration)> = {
            let inner = self.inner.lock().await;
            let file = self.file.read().await;
            inner
                .children
                .iter()
                .filter(|(_, child)| matches!(child.poll(), crate::process::Outcome::Running))
                .map(|(key, _)| {
                    let budget = file
                        .containers
                        .get(key)
                        .map(|container| container.startup_timeout)
                        .unwrap_or(SUPERVISION_INTERVAL);
                    (key.clone(), budget)
                })
                .collect()
        };

        for (key, budget) in running {
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

            // A container that asked to be restarted gets the first attempt
            // immediately: its dependents stay up, and the exit only cascades
            // once the budget below is spent.
            if self.restart_policy(&key).await.wants_restart(code) {
                self.refill_budget_if_it_held(&key).await;
                if self.spend_attempt(&key).await {
                    self.run_restart_attempt(&key).await;
                    continue;
                }
                self.report_gave_up(&key).await;
                self.cascade_failure(&key, Self::exhausted_reason()).await;
                continue;
            }

            self.cascade_failure(&key, reason).await;
        }
    }

    /// Takes the attempts that came due since the last tick.
    ///
    /// The waiting happens here rather than in [`Self::run_restart_attempt`] so
    /// a backoff never blocks the supervision loop, and so a project whose
    /// worker is in a crash loop does not stop the daemon noticing anything
    /// else. The tick interval is the granularity of the backoff.
    pub(crate) async fn drive_restarts(&self) {
        let now = Instant::now();
        let due: Vec<String> = {
            let attempts = self.restarts.lock().await;
            attempts
                .iter()
                .filter(|(_, attempt)| attempt.due.is_some_and(|due| due <= now))
                .map(|(key, _)| key.clone())
                .collect()
        };

        for key in due {
            // An operator who ran `up` or `restart` in the meantime has taken
            // the container back, and the supervisor stops competing for it.
            // The spent count stays: the operator changed who is driving, not
            // whether this container has been crashing.
            let still_waiting = {
                let inner = self.inner.lock().await;
                inner
                    .state
                    .containers
                    .get(&key)
                    .is_some_and(|record| record.status == ChildStatus::Restarting)
            };
            if !still_waiting {
                if let Some(attempt) = self.restarts.lock().await.get_mut(&key) {
                    attempt.due = None;
                }
                continue;
            }

            if self.spend_attempt(&key).await {
                self.run_restart_attempt(&key).await;
                continue;
            }

            self.report_gave_up(&key).await;
            self.cascade_failure(&key, Self::exhausted_reason()).await;
        }
    }

    /// One attempt: the same stop-then-start that `compose::restart` performs,
    /// so a supervised restart and an operator's restart are the same act and
    /// cannot drift apart. It also means the attempt inherits the rule that a
    /// restart touches one container and not a graph, which is what keeps the
    /// dependents up while this one is gone.
    async fn run_restart_attempt(&self, key: &str) {
        let spent = self
            .restarts
            .lock()
            .await
            .get(key)
            .map_or(1, |attempt| attempt.spent);
        daemon_line(
            &self.project_namespace,
            &format!("restarting {key} (attempt {spent} of {RESTART_MAX_ATTEMPTS})"),
            Tone::Warn,
        );

        // Written before the start so a `compose::status` racing the attempt
        // says `restarting` rather than the `failed` this is trying to undo.
        self.mark(key, ChildStatus::Restarting, None).await;

        let result = self.restart_one(key, format!("supervisor:{key}")).await;
        if result.status == OpStatus::Ok {
            // The record is already `Ready` and nothing more is owed. The spent
            // count survives, so a worker that comes back for a moment each
            // time still runs out of attempts.
            if let Some(attempt) = self.restarts.lock().await.get_mut(key) {
                attempt.due = None;
            }
            return;
        }

        // Only wait if there is something to wait for. Backing off after the
        // last attempt would hold the container in `restarting` for a wait
        // nobody is going to use, and delay the operator's answer by it.
        let exhausted = {
            let mut attempts = self.restarts.lock().await;
            match attempts.get_mut(key) {
                Some(attempt) if attempt.spent < RESTART_MAX_ATTEMPTS => {
                    attempt.due = Some(Instant::now() + RestartAttempts::backoff(attempt.spent));
                    false
                }
                _ => true,
            }
        };

        if exhausted {
            self.report_gave_up(key).await;
            self.cascade_failure(key, Self::exhausted_reason()).await;
        } else {
            self.mark(key, ChildStatus::Restarting, None).await;
        }
    }

    /// Fails the container and takes its transitive dependents with it: what
    /// compose did for every unexpected exit before there was a policy, and
    /// what it still does once one has run out of attempts.
    async fn cascade_failure(&self, key: &str, reason: String) {
        self.restarts.lock().await.remove(key);

        // Cascade through the same path a targeted `down` takes: it stops
        // the dependents first and ends on the dead container itself, which
        // fires its `post_run` and drops it from the map. Leaving dependents
        // running would leave them talking to something that is gone.
        let file = self.file.read().await;
        let dependents = crate::dag::transitive_dependents(&file, key);
        drop(file);
        if !dependents.is_empty() {
            daemon_line(
                &self.project_namespace,
                &format!("stopping what depended on {key}: {}", dependents.join(", ")),
                Tone::Warn,
            );
        }
        self.down(Some(key), format!("supervisor:{key}")).await;

        // After the cascade, so `down` marking everything Stopped does not
        // erase why this one went.
        self.mark(key, ChildStatus::Failed, Some(reason)).await;
    }

    /// This container's declared answer to exiting after it was ready. A
    /// container the file no longer declares gets `no`, matching `is_required`:
    /// the rule that stops is the one to fall back on.
    async fn restart_policy(&self, key: &str) -> RestartPolicy {
        self.file
            .read()
            .await
            .containers
            .get(key)
            .map_or(RestartPolicy::No, |container| container.restart)
    }

    /// Takes one attempt from the budget. `false` means it is spent, which is
    /// the supervisor's cue to stop and let the failure cascade.
    async fn spend_attempt(&self, key: &str) -> bool {
        let mut attempts = self.restarts.lock().await;
        let attempt = attempts.entry(key.to_string()).or_insert(RestartAttempts {
            spent: 0,
            due: None,
        });
        if attempt.spent >= RESTART_MAX_ATTEMPTS {
            return false;
        }
        attempt.spent += 1;
        true
    }

    /// Refills the budget of a container that had held ready long enough to
    /// count as recovered, so this crash starts a new run of failures rather
    /// than continuing one the container already came back from.
    async fn refill_budget_if_it_held(&self, key: &str) {
        let held_for = {
            let inner = self.inner.lock().await;
            inner
                .state
                .containers
                .get(key)
                .map_or(0, |record| seconds_since(record.started_at))
        };
        if held_for >= RESTART_BUDGET_RESET_AFTER.as_secs() {
            self.restarts.lock().await.remove(key);
        }
    }

    fn exhausted_reason() -> String {
        format!("did not stay up after {RESTART_MAX_ATTEMPTS} restart attempts")
    }

    async fn report_gave_up(&self, key: &str) {
        daemon_line(
            &self.project_namespace,
            &format!("{key} {}: giving up", Self::exhausted_reason()),
            Tone::Warn,
        );
    }

    /// Writes a container's status and persists it on the same path, which is
    /// the rule the whole module is built on.
    async fn mark(&self, key: &str, status: ChildStatus, last_error: Option<String>) {
        let mut inner = self.inner.lock().await;
        if let Some(entry) = inner.state.containers.get_mut(key) {
            entry.status = status;
            if last_error.is_some() {
                entry.last_error = last_error;
            }
        }
        let snapshot = inner.state.clone();
        drop(inner);
        let _ = self.store.save(&snapshot);
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

    /// Per-container VM state: rootfs, boot script, pid file. Keyed by project
    /// rather than by worker name, so two projects using the same container key
    /// stay apart.
    fn vm_dir(&self) -> PathBuf {
        self.store.dir().join("vm")
    }

    /// Installed packages live at the root, not inside a daemon or a project:
    /// the same `state 0.21.4` serves every project on this machine, and
    /// deriving this by walking up from the state directory would silently
    /// re-scope it the next time that layout gains a level.
    fn package_cache(&self) -> PathBuf {
        StateStore::package_cache().unwrap_or_else(|_| self.store.dir().join("packages"))
    }

    pub async fn up(&self, target: Option<&str>, operation_id: String) -> OpResult {
        let config_dir = self.config_dir();
        let package_cache = self.package_cache();
        let vm_dir = self.vm_dir();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;
        let file = self.file.read().await;

        let ctx = LifecycleCtx {
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
            logs: &self.logs,
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

    pub(crate) async fn up_until_shutdown(
        &self,
        target: Option<&str>,
        operation_id: String,
        shutdown: crate::shutdown::ShutdownSignal,
    ) -> Option<OpResult> {
        let config_dir = self.config_dir();
        let package_cache = self.package_cache();
        let vm_dir = self.vm_dir();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;
        let file = self.file.read().await;

        let ctx = LifecycleCtx {
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
            logs: &self.logs,
            package_cache: &package_cache,
            vm_dir: &vm_dir,
        };

        let result = lifecycle::up_until_shutdown(
            &ctx,
            children,
            &mut state.containers,
            target,
            operation_id,
            shutdown,
        )
        .await;

        let snapshot = state.clone();
        drop(inner);
        let _ = self.store.save(&snapshot);
        result
    }

    /// Applies a compose-file edit without dropping supervision of unchanged
    /// containers. Existing containers whose declarations changed are
    /// restarted in place; then the normal idempotent `up` path starts only
    /// declarations that are not already running.
    pub async fn reconcile_file(
        &self,
        file: ComposeFile,
        restart: &[String],
        operation_id: String,
    ) -> (Vec<OpResult>, OpResult, bool) {
        let config_dir = self.config_dir();
        let package_cache = self.package_cache();
        let vm_dir = self.vm_dir();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;

        {
            let mut current = self.file.write().await;
            *current = file;
        }
        let file = self.file.read().await;
        let ctx = LifecycleCtx {
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
            logs: &self.logs,
            package_cache: &package_cache,
            vm_dir: &vm_dir,
        };

        let operation = crate::operation::active(&operation_id);
        let shutdown = operation.as_ref().map(|operation| {
            crate::shutdown::ShutdownSignal::from_receiver(operation.cancellation())
        });
        let mut restarted = Vec::with_capacity(restart.len());
        let mut interrupted = false;
        for key in restart {
            let result = if let Some(shutdown) = shutdown.clone() {
                lifecycle::restart_one_until_shutdown(
                    &ctx,
                    children,
                    &mut state.containers,
                    key,
                    format!("{operation_id}-restart-{key}"),
                    shutdown,
                )
                .await
            } else {
                Some(
                    lifecycle::restart_one(
                        &ctx,
                        children,
                        &mut state.containers,
                        key,
                        format!("{operation_id}-restart-{key}"),
                    )
                    .await,
                )
            };
            let Some(result) = result else {
                interrupted = true;
                break;
            };
            restarted.push(result);
        }
        let up_operation_id = format!("{operation_id}-up");
        let up = if interrupted {
            OpResult {
                operation_id: up_operation_id,
                status: crate::lifecycle::OpStatus::Failed,
                changed: false,
                containers: Vec::new(),
            }
        } else if let Some(shutdown) = shutdown {
            let result = lifecycle::up_until_shutdown(
                &ctx,
                children,
                &mut state.containers,
                None,
                up_operation_id.clone(),
                shutdown,
            )
            .await;
            if result.is_none() {
                interrupted = true;
            }
            result.unwrap_or_else(|| OpResult {
                operation_id: up_operation_id,
                status: crate::lifecycle::OpStatus::Failed,
                changed: false,
                containers: Vec::new(),
            })
        } else {
            lifecycle::up(&ctx, children, &mut state.containers, None, up_operation_id).await
        };

        let snapshot = state.clone();
        drop(file);
        drop(inner);
        let _ = self.store.save(&snapshot);
        (restarted, up, interrupted)
    }

    /// Applies a removal without dropping supervision of surviving containers.
    ///
    /// The removed worker is stopped against the old declaration so its
    /// cleanup hook and environment are still available. The validated new
    /// declaration then replaces the held file, and normal idempotent `up`
    /// starts only anything that was already missing.
    pub async fn reconcile_removal(
        &self,
        file: ComposeFile,
        removed: &str,
        operation_id: String,
    ) -> (OpResult, OpResult) {
        let config_dir = self.config_dir();
        let package_cache = self.package_cache();
        let vm_dir = self.vm_dir();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;

        let stopped = {
            let current = self.file.read().await;
            let ctx = LifecycleCtx {
                file: &current,
                engine: &self.engine,
                post_runs: &self.post_runs,
                compose_namespace: &self.compose_namespace,
                project_namespace: &self.project_namespace,
                engine_url: &self.engine_url,
                config_dir: &config_dir,
                logs: &self.logs,
                package_cache: &package_cache,
                vm_dir: &vm_dir,
            };
            lifecycle::remove_one(
                &ctx,
                children,
                &mut state.containers,
                removed,
                format!("{operation_id}-down"),
            )
            .await
        };

        {
            let mut current = self.file.write().await;
            *current = file;
        }
        let file = self.file.read().await;
        let ctx = LifecycleCtx {
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
            logs: &self.logs,
            package_cache: &package_cache,
            vm_dir: &vm_dir,
        };
        let up = lifecycle::up(
            &ctx,
            children,
            &mut state.containers,
            None,
            format!("{operation_id}-up"),
        )
        .await;

        let snapshot = state.clone();
        drop(file);
        drop(inner);
        let _ = self.store.save(&snapshot);
        (stopped, up)
    }

    /// Bounces one container. See [`lifecycle::restart_one`] for why this does
    /// not take the container's graph with it.
    pub async fn restart_one(&self, key: &str, operation_id: String) -> OpResult {
        let config_dir = self.config_dir();
        let package_cache = self.package_cache();
        let vm_dir = self.vm_dir();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;
        let file = self.file.read().await;

        let ctx = LifecycleCtx {
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
            logs: &self.logs,
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
        let package_cache = self.package_cache();
        let vm_dir = self.vm_dir();
        let mut inner = self.inner.lock().await;
        let Inner { children, state } = &mut *inner;
        let file = self.file.read().await;

        let ctx = LifecycleCtx {
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            config_dir: &config_dir,
            logs: &self.logs,
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
        let file = self.file.read().await;
        let inner = self.inner.try_lock().ok();
        let stored = if inner.is_none() {
            self.store.load().ok().flatten()
        } else {
            None
        };
        file.containers
            .keys()
            .map(|key| {
                let record = inner
                    .as_ref()
                    .and_then(|inner| inner.state.containers.get(key))
                    .or_else(|| stored.as_ref().and_then(|state| state.containers.get(key)));
                let running = inner.as_ref().is_some_and(|inner| {
                    inner.children.get(key).is_some_and(|child| {
                        matches!(child.poll(), crate::process::Outcome::Running)
                    })
                });
                ContainerStatus {
                    container: key.clone(),
                    state: match (running, record.map(|record| record.status)) {
                        (true, _) => ChildStatus::Ready,
                        (false, Some(status)) => status,
                        (false, None) => ChildStatus::Stopped,
                    },
                    pid: record.map(|record| record.pid),
                    owned: inner
                        .as_ref()
                        .is_some_and(|inner| inner.children.contains_key(key)),
                    log_path: self.logs.path(key),
                    last_error: record.and_then(|record| record.last_error.clone()),
                }
            })
            .collect()
    }

    /// Reads retained stdout and stderr without exposing arbitrary host paths.
    pub async fn logs(
        &self,
        container: Option<&str>,
        cursors: BTreeMap<String, LogCursor>,
        tail: usize,
        stream: Option<LogStream>,
        wait_ms: u64,
    ) -> Result<LogsOutcome> {
        let file = self.file.read().await;
        let containers = match container {
            Some(container) if file.containers.contains_key(container) => {
                vec![container.to_string()]
            }
            Some(container) => {
                return Err(ComposeError::UnknownContainer {
                    container: container.to_string(),
                });
            }
            None => file.containers.keys().cloned().collect(),
        };
        drop(file);

        self.logs
            .query(
                containers,
                cursors,
                tail,
                stream,
                Duration::from_millis(wait_ms.min(crate::logs::MAX_WAIT_MS)),
            )
            .await
            .map_err(|source| ComposeError::Io {
                path: self.logs.dir().to_path_buf(),
                source,
            })
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
        let stop_timeout = self.file.read().await.stop_timeout;
        for key in spawned {
            if let Some(child) = inner.children.remove(&key) {
                child.stop(stop_timeout).await;
            }
        }
        inner.children.clear();
        drop(inner);

        self.post_runs.shutdown().await;
        self.engine.shutdown().await;
    }

    /// Intentional shutdown: stop every local child, then clear the state.
    /// A daemon that exits on purpose leaves nothing behind to reconcile.
    pub async fn shutdown(&self) {
        let operation_id = "shutdown".to_string();
        self.down(None, operation_id).await;
        self.post_runs.shutdown().await;
        let _ = self.store.clear();
        self.engine.shutdown().await;
    }
}

#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema, PartialEq, Eq)]
pub struct ContainerStatus {
    pub container: String,
    pub state: ChildStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Whether this daemon owns the process (started it and can stop it).
    pub owned: bool,
    /// Rotating stdout and stderr file on the daemon host.
    pub log_path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// How long ago a `started_at` was, in seconds. Saturating, so a clock that
/// went backwards reads as "just now" rather than as a very old container.
fn seconds_since(unix_secs: u64) -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|now| now.as_secs().saturating_sub(unix_secs))
        .unwrap_or_default()
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

#[cfg(test)]
mod restart_backoff_tests {
    use super::{RESTART_BACKOFF_BASE, RESTART_BACKOFF_MAX, RESTART_MAX_ATTEMPTS, RestartAttempts};

    /// The wait doubles per attempt and then stops doubling. A ceiling the
    /// budget can never reach would be a ceiling in name only, so this pins
    /// both halves.
    #[test]
    fn backoff_doubles_and_then_holds_at_the_ceiling() {
        assert_eq!(RestartAttempts::backoff(1), RESTART_BACKOFF_BASE);
        assert_eq!(RestartAttempts::backoff(2), RESTART_BACKOFF_BASE * 2);
        assert_eq!(RestartAttempts::backoff(3), RESTART_BACKOFF_BASE * 4);
        assert_eq!(
            RestartAttempts::backoff(30),
            RESTART_BACKOFF_MAX,
            "a long-running loop must not overflow into an unbounded wait"
        );
    }

    /// Without a cap the policy is a busy loop, which is what the issue this
    /// field came from was written to avoid.
    #[test]
    fn the_budget_is_spent_in_bounded_time() {
        let total: std::time::Duration = (1..=RESTART_MAX_ATTEMPTS)
            .map(RestartAttempts::backoff)
            .sum();
        assert!(
            total <= RESTART_BACKOFF_MAX * RESTART_MAX_ATTEMPTS,
            "every attempt waits at most the ceiling"
        );
        assert!(
            total >= RESTART_BACKOFF_BASE,
            "attempts after the first always wait"
        );
    }
}
