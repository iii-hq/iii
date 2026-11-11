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
    process::ExitStatus,
    sync::Arc,
    time::Duration,
};

use tokio::{
    sync::{Mutex, RwLock},
    time::Instant,
};

use crate::report;
use crate::{
    config::{ComposeFile, RestartConfig},
    engine::EngineClient,
    error::{ComposeError, Result},
    lifecycle::{self, Child, Children, LifecycleCtx, OpResult, OpStatus},
    logs::{LogCursor, LogStore, LogStream, LogsOutcome},
    process::Supervised,
    restart,
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
    /// `DaemonState::sandbox_token`, copied out so a lifecycle call needs no
    /// state lock to read it.
    sandbox_token: String,
    /// The VMs a 0.26 daemon left running, by container, each stopped when its container next
    /// starts (spec 7.3, step 2).
    legacy_vms: std::sync::Mutex<BTreeMap<String, crate::state::VmRecord>>,
    inner: Mutex<Inner>,
    shutdown: crate::shutdown::ShutdownController,
}

/// How often the supervisor checks whether a ready child is still alive.
/// Fast enough that a crash is reported while the operator is still watching,
/// slow enough that an idle project costs nothing.
const SUPERVISION_INTERVAL: Duration = Duration::from_millis(250);

/// Where one container is in its restart budget.
#[derive(Debug, Clone, Copy, Default)]
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

/// Identifies one supervisor claim. Operator operations invalidate the active
/// claim before they change the same container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RestartLease(u64);

#[derive(Debug, Clone, Copy)]
enum RestartCause {
    UnexpectedExit,
    RetryDue,
}

/// In-memory restart state for one daemon run.
///
/// Deliberately not in [`DaemonState`], so not on disk. An attempt count
/// describes one supervisor's patience with one crash loop, and a daemon that
/// restarts re-reconciles from scratch: carrying the old count over would let
/// a project come back with its budget already spent.
#[derive(Debug, Default)]
struct RestartBookkeeping {
    attempts: BTreeMap<String, RestartAttempts>,
    leases: BTreeMap<String, RestartLease>,
    next_lease: u64,
}

impl RestartBookkeeping {
    fn claim(&mut self, key: &str) -> Option<RestartLease> {
        if self.leases.contains_key(key) {
            return None;
        }
        self.next_lease = self.next_lease.wrapping_add(1);
        let lease = RestartLease(self.next_lease);
        self.leases.insert(key.to_string(), lease);
        Some(lease)
    }

    fn is_current(&self, key: &str, lease: RestartLease) -> bool {
        self.leases
            .get(key)
            .is_some_and(|current| *current == lease)
    }

    fn release(&mut self, key: &str, lease: RestartLease) {
        if self.is_current(key, lease) {
            self.leases.remove(key);
        }
    }

    /// Transfers control from the supervisor to an operator operation without
    /// refilling the crash-loop budget.
    fn operator_took_control(&mut self, target: Option<&str>) {
        match target {
            Some(key) => {
                self.leases.remove(key);
                if let Some(attempt) = self.attempts.get_mut(key) {
                    attempt.due = None;
                }
            }
            None => {
                self.leases.clear();
                for attempt in self.attempts.values_mut() {
                    attempt.due = None;
                }
            }
        }
    }

    fn remove(&mut self, key: &str) {
        self.attempts.remove(key);
        self.leases.remove(key);
    }
}

/// The parts that change together. One lock: a caller must never see the
/// children and the records disagree.
struct Inner {
    children: Children,
    state: DaemonState,
    restarts: RestartBookkeeping,
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
        Self::open_with_shutdown(
            daemon_namespace,
            project_namespace,
            file,
            engine,
            engine_url,
            crate::shutdown::ShutdownController::default(),
        )
        .await
    }

    pub(crate) async fn open_with_shutdown(
        daemon_namespace: &str,
        project_namespace: String,
        file: ComposeFile,
        engine: Arc<EngineClient>,
        engine_url: String,
        shutdown: crate::shutdown::ShutdownController,
    ) -> Result<Arc<Self>> {
        let store = StateStore::for_project(daemon_namespace, &file.path)?;
        let file_path = file.path.clone();

        let recovered = store.load()?;
        if let Some(state) = &recovered {
            // Two compose files in one directory must not adopt each other's
            // children when they use the same namespace.
            state.check_binding(&file.path)?;
        }
        let mut state =
            recovered.unwrap_or_else(|| DaemonState::new(&file.path, &project_namespace));
        state.namespace = project_namespace.clone();
        // Saved by `reconcile_recovered` below, so a restart reuses it.
        let sandbox_token = state
            .sandbox_token
            .get_or_insert_with(|| uuid::Uuid::new_v4().simple().to_string())
            .clone();
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
            shutdown,
            post_runs: crate::hooks::PostRunSupervisor::default(),
            logs,
            store,
            sandbox_token,
            legacy_vms: std::sync::Mutex::new(BTreeMap::new()),
            inner: Mutex::new(Inner {
                children: BTreeMap::new(),
                state,
                restarts: RestartBookkeeping::default(),
            }),
        });

        project.reconcile_recovered().await;
        Ok(project)
    }

    /// Canonical path that identifies this project. The path never changes
    /// when the parsed declaration is refreshed.
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// The file as held in memory.
    #[cfg(test)]
    pub(crate) async fn held_file(&self) -> ComposeFile {
        self.file.read().await.clone()
    }

    /// Commits the package lock and refreshes the metadata used by future starts.
    /// Existing declarations and running child supervision stay unchanged.
    /// Cancellation while waiting for the metadata lock leaves disk untouched;
    /// after it is acquired, persistence and attachment have no cancellation point.
    /// The declarations of `rewritten`, the containers the explicit VM mode was just written for
    /// (spec 7.3) and the adapter it added or updated, are taken whole from `resolved`.
    pub(crate) async fn commit_prepared_packages(
        &self,
        resolved: &ComposeFile,
        prepared: &crate::lockfile::PreparedLock,
        rewritten: &[String],
    ) -> Result<()> {
        let mut current = self.file.write().await;
        // Built aside and swapped in, so a dependency cycle leaves both the lock and
        // the held file as they were.
        let mut next = current.clone();
        for key in rewritten {
            if let Some(container) = resolved.containers.get(key) {
                next.containers.insert(key.clone(), container.clone());
            }
        }
        // `up` may just have written the adapter into an already loaded project.
        if let Some(key) =
            crate::sandbox::adapter_key(resolved, crate::sandbox::adapter_name(resolved))?
            && !next.containers.contains_key(&key)
        {
            next.containers
                .insert(key.clone(), resolved.containers[&key].clone());
        }
        for (key, container) in &mut next.containers {
            let Some(source) = resolved.containers.get(key) else {
                continue;
            };
            if source.worker == container.worker && source.version == container.version {
                container.resolved_package = source.resolved_package.clone();
            }
        }
        crate::dag::validate_dependencies(&next)?;
        prepared.write_if_changed()?;
        *current = next;
        Ok(())
    }

    /// Adopts the selector and lock result `updated` declares for each of
    /// `keys`, without touching what runs.
    ///
    /// An update that changes no package content and no topology writes the
    /// file and the lock but restarts nothing, so nothing re-reads the file.
    /// Without this the held declaration keeps the selector the update
    /// replaced, and `compose::status` reports it until the next restart.
    /// Only a declaration whose source did not change is refreshed.
    pub(crate) async fn adopt_updated_selectors<'a>(
        &self,
        updated: &ComposeFile,
        keys: impl IntoIterator<Item = &'a String>,
    ) {
        let mut current = self.file.write().await;
        for key in keys {
            let (Some(container), Some(source)) =
                (current.containers.get_mut(key), updated.containers.get(key))
            else {
                continue;
            };
            if source.worker == container.worker {
                container.version = source.version.clone();
                container.resolved_package = source.resolved_package.clone();
            }
        }
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
        let shutdown = self.shutdown.signal();
        if shutdown.requested() {
            return;
        }
        daemon_line(
            &self.project_namespace,
            "engine connection restored; re-checking the project",
            Tone::Plain,
        );

        let running: Vec<(String, Duration)> = {
            let Some(inner) = shutdown.run(self.inner.lock()).await else {
                return;
            };
            let Some(file) = shutdown.run(self.file.read()).await else {
                return;
            };
            inner
                .children
                .iter()
                .filter(|(_, child)| {
                    matches!(
                        child.poll(),
                        crate::process::Outcome::Running | crate::process::Outcome::Unknown
                    )
                })
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
            match self.wait_for_reregistration(&key, budget).await {
                Some(true) => continue,
                Some(false) => {}
                None => return,
            }
            if shutdown.requested() {
                return;
            }

            daemon_line(
                &self.project_namespace,
                &format!("{key} did not register again after the reconnect"),
                Tone::Warn,
            );
            self.down(Some(&key), format!("reconnect:{key}")).await;

            let Some(mut inner) = shutdown.run(self.inner.lock()).await else {
                return;
            };
            if shutdown.requested() {
                return;
            }
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
    pub(crate) async fn wait_for_reregistration(
        &self,
        key: &str,
        budget: Duration,
    ) -> Option<bool> {
        // None is intentional shutdown, not a registration failure. Do not
        // cascade down or persist a failure record after cancellation.
        self.shutdown
            .signal()
            .run(async {
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
            })
            .await
    }

    /// Reacts to children that ended without anybody asking them to.
    ///
    /// `up` and `down` hold the lock for their whole run, so this only ever
    /// observes the gaps between operations — a deliberate stop removes the
    /// child from the map before signalling it, and is never seen here.
    pub(crate) async fn reap_unexpected_exits(&self) {
        crate::report::in_project_scope(&self.file_path, self.reap_scoped()).await
    }

    async fn reap_scoped(&self) {
        let dead: Vec<(String, ExitStatus)> = {
            let inner = self.inner.lock().await;
            inner
                .children
                .iter()
                .filter_map(|(key, child)| match child.poll() {
                    crate::process::Outcome::Exited(status) => Some((key.clone(), status)),
                    // An adapter that does not answer says nothing about its VM.
                    crate::process::Outcome::Running | crate::process::Outcome::Unknown => None,
                })
                .collect()
        };

        for (key, status) in dead {
            let clean_exit = status.success();
            let reason = if clean_exit {
                format!("exited successfully ({status})")
            } else {
                format!("exited unexpectedly ({status})")
            };
            daemon_line(
                &self.project_namespace,
                &format!("{key} {reason}"),
                if clean_exit { Tone::Plain } else { Tone::Warn },
            );

            // A container that asked to be restarted gets the first attempt
            // immediately: its dependents stay up, and the exit only cascades
            // once the budget below is spent.
            if self
                .restart_config(&key)
                .await
                .wants_restart(status.code().unwrap_or(-1))
            {
                if let Some(lease) = self.claim_restart(&key, RestartCause::UnexpectedExit).await {
                    self.run_restart_attempt(&key, lease, RestartCause::UnexpectedExit)
                        .await;
                }
                continue;
            }

            if clean_exit {
                self.cascade_stop(&key).await;
            } else {
                self.cascade_failure(&key, reason).await;
            }
        }
    }

    /// Takes the attempts that came due since the last tick.
    ///
    /// The waiting happens here rather than in [`Self::run_restart_attempt`] so
    /// a backoff never blocks the supervision loop, and so a project whose
    /// worker is in a crash loop does not stop the daemon noticing anything
    /// else. The tick interval is the granularity of the backoff.
    pub(crate) async fn drive_restarts(&self) {
        crate::report::in_project_scope(&self.file_path, self.drive_restarts_scoped()).await
    }

    async fn drive_restarts_scoped(&self) {
        let now = Instant::now();
        let due: Vec<String> = {
            let inner = self.inner.lock().await;
            inner
                .restarts
                .attempts
                .iter()
                .filter(|(_, attempt)| attempt.due.is_some_and(|due| due <= now))
                .map(|(key, _)| key.clone())
                .collect()
        };

        for key in due {
            if let Some(lease) = self.claim_restart(&key, RestartCause::RetryDue).await {
                self.run_restart_attempt(&key, lease, RestartCause::RetryDue)
                    .await;
            }
        }
    }

    /// Claims the right to restart under the same lock used by operator
    /// operations. The attempt is not spent until the claim is checked again
    /// immediately before the bounce.
    async fn claim_restart(&self, key: &str, cause: RestartCause) -> Option<RestartLease> {
        let mut inner = self.inner.lock().await;
        if !Self::restart_is_eligible(&inner, key, cause, Instant::now()) {
            if matches!(cause, RestartCause::RetryDue)
                && !inner
                    .state
                    .containers
                    .get(key)
                    .is_some_and(|record| record.status == ChildStatus::Restarting)
                && let Some(attempt) = inner.restarts.attempts.get_mut(key)
            {
                attempt.due = None;
            }
            return None;
        }
        inner.restarts.claim(key)
    }

    fn restart_is_eligible(inner: &Inner, key: &str, cause: RestartCause, now: Instant) -> bool {
        match cause {
            RestartCause::UnexpectedExit => inner
                .children
                .get(key)
                .is_some_and(|child| matches!(child.poll(), crate::process::Outcome::Exited(_))),
            RestartCause::RetryDue => {
                inner
                    .state
                    .containers
                    .get(key)
                    .is_some_and(|record| record.status == ChildStatus::Restarting)
                    && inner
                        .restarts
                        .attempts
                        .get(key)
                        .is_some_and(|attempt| attempt.due.is_some_and(|due| due <= now))
            }
        }
    }

    /// One attempt: the same stop-then-start that `compose::restart` performs,
    /// so a supervised restart and an operator's restart are the same act and
    /// cannot drift apart. It also means the attempt inherits the rule that a
    /// restart touches one container and not a graph, which is what keeps the
    /// dependents up while this one is gone.
    async fn run_restart_attempt(&self, key: &str, lease: RestartLease, cause: RestartCause) {
        let restart_config = self.restart_config(key).await;
        let Some(mut inner) = self.shutdown.signal().run(self.inner.lock()).await else {
            return;
        };
        if self.shutdown.signal().requested() {
            inner.restarts.release(key, lease);
            return;
        }
        if !inner.restarts.is_current(key, lease)
            || !Self::restart_is_eligible(&inner, key, cause, Instant::now())
        {
            inner.restarts.release(key, lease);
            return;
        }

        if matches!(cause, RestartCause::UnexpectedExit)
            && inner
                .state
                .containers
                .get(key)
                .map_or(Duration::ZERO, |record| {
                    Duration::from_secs(seconds_since(record.started_at))
                })
                >= restart_config.window
        {
            inner.restarts.attempts.remove(key);
        }

        let spent = {
            let attempt = inner.restarts.attempts.entry(key.to_string()).or_default();
            if attempt.spent >= restart_config.max_attempts {
                None
            } else {
                attempt.spent += 1;
                Some(attempt.spent)
            }
        };
        let Some(spent) = spent else {
            inner.restarts.release(key, lease);
            drop(inner);
            self.report_gave_up(key, restart_config.max_attempts).await;
            self.cascade_failure(key, Self::exhausted_reason(restart_config.max_attempts))
                .await;
            return;
        };

        daemon_line(
            &self.project_namespace,
            &format!(
                "restarting {key} (attempt {spent} of {})",
                restart_config.max_attempts
            ),
            Tone::Warn,
        );

        // Written before the start so a `compose::status` racing the attempt
        // says `restarting` rather than the `failed` this is trying to undo.
        if let Some(entry) = inner.state.containers.get_mut(key) {
            entry.status = ChildStatus::Restarting;
        }
        let restarting = inner.state.clone();
        let _ = self.store.save(&restarting);

        let result = self
            .restart_one_locked(
                &mut inner,
                key,
                format!("supervisor:{key}"),
                Some((spent, restart_config.max_attempts)),
            )
            .await;
        let ready = result
            .containers
            .iter()
            .any(|result| result.container == key && result.state == ChildStatus::Ready);
        inner.restarts.release(key, lease);
        if self.shutdown.signal().requested() {
            let snapshot = inner.state.clone();
            drop(inner);
            let _ = self.store.save(&snapshot);
            return;
        }
        if ready {
            // The record is already `Ready` and nothing more is owed. The spent
            // count survives, so a worker that comes back for a moment each
            // time still runs out of attempts.
            if let Some(attempt) = inner.restarts.attempts.get_mut(key) {
                attempt.due = None;
            }
            let snapshot = inner.state.clone();
            drop(inner);
            let _ = self.store.save(&snapshot);
            return;
        }

        // Only wait if there is something to wait for. Backing off after the
        // last attempt would hold the container in `restarting` for a wait
        // nobody is going to use, and delay the operator's answer by it.
        let next_retry = match inner.restarts.attempts.get_mut(key) {
            Some(attempt) if attempt.spent < restart_config.max_attempts => {
                let delay = restart::backoff(&restart_config, attempt.spent);
                attempt.due = Some(Instant::now() + delay);
                Some((attempt.spent + 1, delay))
            }
            _ => None,
        };

        if next_retry.is_some()
            && let Some(entry) = inner.state.containers.get_mut(key)
        {
            entry.status = ChildStatus::Restarting;
        }
        // Emit while holding the existing project lock: down cancels retries
        // under this same lock and must not be followed by a stale waiting row.
        if let Some((next_attempt, delay)) = next_retry {
            crate::report::retry_waiting(key, next_attempt, restart_config.max_attempts, delay);
        }
        let snapshot = inner.state.clone();
        drop(inner);
        let _ = self.store.save(&snapshot);

        if next_retry.is_none() {
            self.report_gave_up(key, restart_config.max_attempts).await;
            self.cascade_failure(key, Self::exhausted_reason(restart_config.max_attempts))
                .await;
        }
    }

    /// Fails the container and takes its transitive dependents with it: what
    /// compose did for every unexpected exit before there was a policy, and
    /// what it still does once one has run out of attempts.
    async fn cascade_failure(&self, key: &str, reason: String) {
        self.stop_with_dependents(key).await;

        // After the cascade, so `down` marking everything Stopped does not
        // erase why this one went.
        self.mark(key, ChildStatus::Failed, Some(reason)).await;
    }

    /// Records a successful exit that its policy does not restart as stopped.
    async fn cascade_stop(&self, key: &str) {
        self.stop_with_dependents(key).await;
        self.mark(key, ChildStatus::Stopped, None).await;
    }

    async fn stop_with_dependents(&self, key: &str) {
        self.inner.lock().await.restarts.remove(key);

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
    }

    /// This container's declared answer to exiting after it was ready. A
    /// container the file no longer declares gets `no`, matching `is_required`:
    /// the rule that stops is the one to fall back on.
    async fn restart_config(&self, key: &str) -> RestartConfig {
        self.file
            .read()
            .await
            .containers
            .get(key)
            .map(|container| container.restart.clone())
            .unwrap_or_default()
    }

    fn exhausted_reason(max_attempts: u32) -> String {
        format!("did not stay up after {max_attempts} restart attempts")
    }

    async fn report_gave_up(&self, key: &str, max_attempts: u32) {
        daemon_line(
            &self.project_namespace,
            &format!("{key} {}: giving up", Self::exhausted_reason(max_attempts)),
            Tone::Warn,
        );
    }

    /// Writes a container's status and persists it on the same path, which is
    /// the rule the whole module is built on.
    async fn mark(&self, key: &str, status: ChildStatus, last_error: Option<String>) {
        let mut inner = self.inner.lock().await;
        if let Some(entry) = inner.state.containers.get_mut(key) {
            entry.status = status;
            entry.last_error = last_error;
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
    ///
    /// Survivors are adopted only from a compose process that is still alive:
    /// this one, reopening the project in place, or another that is about to
    /// refuse this one (see [`Project::abandon`]). A survivor of a compose that
    /// is gone was left behind by a crash, and keeping it would keep whatever
    /// configuration it started with; it is stopped and started fresh instead.
    async fn reconcile_recovered(&self) {
        let mut inner = self.inner.lock().await;
        let owner_alive = inner
            .state
            .owner
            .as_ref()
            .is_some_and(crate::state::Owner::is_alive);
        let mut left_behind = Vec::new();
        let recorded: Vec<(String, crate::state::ChildRecord)> = inner
            .state
            .containers
            .iter()
            .map(|(key, record)| (key.clone(), record.clone()))
            .collect();

        for (key, record) in recorded {
            // A VM a 0.26 daemon left running, which its container has not stopped yet: the
            // record this daemon marked stopped, or failed when that stop failed, still names
            // it. It stays due to be stopped (spec 7.3, step 2), unless its owner is gone and
            // the adapter's lifeline took it.
            if let Some(vm) = record
                .vm
                .clone()
                .filter(|_| reconcile(&record) == Reconciliation::Stopped)
            {
                if owner_alive {
                    self.legacy_vms
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(key.clone(), vm);
                } else if let Some(entry) = inner.state.containers.get_mut(&key) {
                    entry.vm = None;
                }
                continue;
            }
            // Only its adapter can answer for a VM that the record says may
            // still run.
            let vm = record
                .vm
                .clone()
                .filter(|_| reconcile(&record) != Reconciliation::Stopped);
            let verdict = match &vm {
                Some(vm) => crate::sandbox::vm_reconciliation(
                    owner_alive,
                    crate::sandbox::worker_status(self.engine.client(), vm, &self.sandbox_token)
                        .await,
                ),
                None => reconcile(&record),
            };
            match (verdict, record.pid, &record.birth, vm) {
                // Compose boots no VM any more: one a 0.26 daemon left running is stopped when
                // its container next starts (spec 7.3, step 2), until then it is not supervised.
                (Reconciliation::Adopt, .., Some(vm)) => {
                    daemon_line(
                        &self.project_namespace,
                        &format!(
                            "{key}: VM {} instance {}, started by Compose 0.26, still runs; it is \
                             stopped when {key} next starts",
                            vm.vm_id, vm.instance
                        ),
                        Tone::Warn,
                    );
                    self.legacy_vms
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(key.clone(), vm);
                    if let Some(entry) = inner.state.containers.get_mut(&key) {
                        entry.status = ChildStatus::Stopped;
                    }
                }
                // The adapter's lifeline took it with the compose that is gone.
                (Reconciliation::Stopped, .., Some(_)) => {
                    if let Some(entry) = inner.state.containers.get_mut(&key) {
                        entry.status = ChildStatus::Stopped;
                        entry.vm = None;
                    }
                }
                (Reconciliation::Adopt, Some(pid), Some(birth), None) => {
                    match Supervised::adopt(pid, birth) {
                        Some(child) if !owner_alive => left_behind.push((key.clone(), child)),
                        Some(child) => {
                            daemon_line(
                                &self.project_namespace,
                                &format!("{key} survived (pid {pid}), adopted"),
                                Tone::Plain,
                            );
                            inner.children.insert(key.clone(), Child::Process(child));
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
                    }
                }
                // Stopped by this daemon, or failed with its last_error saved:
                // the expected state after `down` or a failure, not an anomaly
                // worth a line.
                (Reconciliation::Stopped, ..) => {}
                (Reconciliation::Unverifiable, Some(pid), ..) => {
                    // Never signalled: the pid is alive but unproven.
                    daemon_line(
                        &self.project_namespace,
                        &format!(
                            "{key}: pid {pid} is alive but is not provably ours; left running \
                             for manual cleanup"
                        ),
                        Tone::Warn,
                    );
                    if let Some(entry) = inner.state.containers.get_mut(&key) {
                        entry.status = ChildStatus::Failed;
                        entry.last_error = Some(format!("pid {pid} could not be verified"));
                    }
                }
                // The rest is gone: a process that is not running, or a VM its
                // adapter says has exited or no longer knows.
                (
                    Reconciliation::Gone | Reconciliation::Adopt | Reconciliation::Unverifiable,
                    ..,
                ) => {
                    daemon_line(
                        &self.project_namespace,
                        &format!("{key} exited while the daemon was away"),
                        Tone::Warn,
                    );
                    if let Some(entry) = inner.state.containers.get_mut(&key) {
                        entry.status = ChildStatus::Failed;
                        entry.last_error = Some("exited while the daemon was away".to_string());
                        entry.vm = None;
                    }
                }
            }
        }

        if !left_behind.is_empty() {
            let grace = self.file.read().await.stop_timeout;
            futures::future::join_all(left_behind.iter().map(|(_, child)| child.stop(grace))).await;
            for (key, child) in &left_behind {
                daemon_line(
                    &self.project_namespace,
                    &format!(
                        "{key} (pid {}) outlived the compose that started it; stopped it",
                        child.pid
                    ),
                    Tone::Warn,
                );
                if let Some(entry) = inner.state.containers.get_mut(key) {
                    entry.status = ChildStatus::Stopped;
                }
            }
        }
        if !owner_alive {
            inner.state.owner = Some(crate::state::Owner::current());
        }

        let state = inner.state.clone();
        drop(inner);
        let _ = self.store.save(&state);
    }

    /// Everything this project owns on disk: its durable record, the
    /// configuration it was handed, and each container's output.
    ///
    /// Reported by `compose::status`, including when the operator relocates
    /// state with `III_COMPOSE_STATE_DIR`.
    pub fn state_dir(&self) -> &Path {
        self.store.dir()
    }

    /// Installed packages live at the root, not inside a daemon or a project:
    /// the same `state 0.21.4` serves every project on this machine, and
    /// deriving this by walking up from the state directory would silently
    /// re-scope it the next time that layout gains a level.
    fn package_cache(&self) -> PathBuf {
        StateStore::package_cache().unwrap_or_else(|_| self.store.dir().join("packages"))
    }

    pub async fn up(&self, target: Option<&str>, operation_id: String) -> OpResult {
        self.up_until_shutdown(target, operation_id.clone(), self.shutdown.signal())
            .await
            .unwrap_or_else(|| lifecycle::cancelled_op(operation_id))
    }

    pub(crate) async fn up_until_shutdown(
        &self,
        target: Option<&str>,
        operation_id: String,
        shutdown: crate::shutdown::ShutdownSignal,
    ) -> Option<OpResult> {
        let shutdown = shutdown.or(self.shutdown.signal());
        let package_cache = self.package_cache();
        let empty_env_diagnostics = crate::config::EmptyEnvDiagnostics::default();
        let mut inner = match shutdown.run(self.inner.lock()).await {
            Some(inner) => inner,
            None => {
                report::empty_env_warnings(&empty_env_diagnostics.snapshot());
                return None;
            }
        };
        inner.restarts.operator_took_control(target);
        let Inner {
            children, state, ..
        } = &mut *inner;
        let file = match shutdown.run(self.file.read()).await {
            Some(file) => file,
            None => {
                drop(inner);
                report::empty_env_warnings(&empty_env_diagnostics.snapshot());
                return None;
            }
        };

        let ctx = LifecycleCtx {
            shutdown: &self.shutdown,
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            logs: &self.logs,
            package_cache: &package_cache,
            state_dir: self.store.dir(),
            sandbox_token: &self.sandbox_token,
            empty_env_diagnostics: &empty_env_diagnostics,
            legacy_vms: &self.legacy_vms,
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
        report::empty_env_warnings(&empty_env_diagnostics.snapshot());
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
        let package_cache = self.package_cache();
        let empty_env_diagnostics = crate::config::EmptyEnvDiagnostics::default();
        let mut inner = self.inner.lock().await;
        inner.restarts.operator_took_control(None);
        {
            let mut current = self.file.write().await;
            *current = file;
        }
        let vms = self.vms_hosted_by_any(&inner, restart).await;
        let Inner {
            children, state, ..
        } = &mut *inner;
        let file = self.file.read().await;
        let ctx = LifecycleCtx {
            shutdown: &self.shutdown,
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            logs: &self.logs,
            package_cache: &package_cache,
            state_dir: self.store.dir(),
            sandbox_token: &self.sandbox_token,
            empty_env_diagnostics: &empty_env_diagnostics,
            legacy_vms: &self.legacy_vms,
        };

        let operation = crate::operation::active(&operation_id);
        let signal = self.shutdown.signal();
        let shutdown = match operation.as_ref() {
            Some(operation) => signal.or(crate::shutdown::ShutdownSignal::from_receiver(
                operation.cancellation(),
            )),
            None => signal,
        };
        // A VM ends with its adapter, so one being restarted stops them first, and the `up` below
        // boots them on its replacement. This one lock keeps the reaper from seeing them gone.
        for vm in vms.iter().rev() {
            lifecycle::stop_for_up(&ctx, children, &mut state.containers, vm).await;
        }
        let mut restarted = Vec::with_capacity(restart.len());
        let mut interrupted = false;
        for key in restart.iter().filter(|key| !vms.contains(key)) {
            let result = lifecycle::restart_one_until_shutdown(
                &ctx,
                children,
                &mut state.containers,
                key,
                format!("{operation_id}-restart-{key}"),
                shutdown.clone(),
            )
            .await;
            let Some(result) = result else {
                interrupted = true;
                break;
            };
            restarted.push(result);
        }
        let up_operation_id = format!("{operation_id}-up");
        let up = if interrupted {
            lifecycle::cancelled_op(up_operation_id)
        } else {
            let result = lifecycle::up_until_shutdown(
                &ctx,
                children,
                &mut state.containers,
                None,
                up_operation_id.clone(),
                shutdown,
            )
            .await;
            interrupted = result.is_none();
            result.unwrap_or_else(|| lifecycle::cancelled_op(up_operation_id))
        };

        let snapshot = state.clone();
        drop(file);
        drop(inner);
        let _ = self.store.save(&snapshot);
        report::empty_env_warnings(&empty_env_diagnostics.snapshot());
        (restarted, up, interrupted)
    }

    /// Applies removals without dropping supervision of surviving containers.
    ///
    /// Removed workers are stopped against the old declaration so their cleanup
    /// hooks and environments are still available. The validated new declaration
    /// then replaces the held file, and normal idempotent `up` starts only
    /// anything that was already missing.
    pub async fn reconcile_removals(
        &self,
        file: ComposeFile,
        removed: &[String],
        operation_id: String,
    ) -> (Vec<OpResult>, OpResult) {
        let package_cache = self.package_cache();
        let empty_env_diagnostics = crate::config::EmptyEnvDiagnostics::default();
        let mut inner = self.inner.lock().await;
        inner.restarts.operator_took_control(None);
        let vms = self.vms_hosted_by_any(&inner, removed).await;
        let Inner {
            children,
            state,
            restarts,
        } = &mut *inner;

        let stopped = {
            let current = self.file.read().await;
            let ctx = LifecycleCtx {
                shutdown: &self.shutdown,
                file: &current,
                engine: &self.engine,
                post_runs: &self.post_runs,
                compose_namespace: &self.compose_namespace,
                project_namespace: &self.project_namespace,
                engine_url: &self.engine_url,
                logs: &self.logs,
                package_cache: &package_cache,
                state_dir: self.store.dir(),
                sandbox_token: &self.sandbox_token,
                empty_env_diagnostics: &empty_env_diagnostics,
                legacy_vms: &self.legacy_vms,
            };
            // Removing an adapter that a VM still needs generates it again: as in
            // `reconcile_file`, its VMs stop first and the `up` below boots them on it.
            for vm in vms.iter().rev().filter(|vm| !removed.contains(vm)) {
                lifecycle::stop_for_up(&ctx, children, &mut state.containers, vm).await;
            }
            let mut stopped = Vec::with_capacity(removed.len());
            for (index, worker) in removed.iter().enumerate() {
                stopped.push(
                    lifecycle::remove_one(
                        &ctx,
                        children,
                        &mut state.containers,
                        worker,
                        format!("{operation_id}-remove-{index}"),
                    )
                    .await,
                );
                restarts.remove(worker);
            }
            stopped
        };

        {
            let mut current = self.file.write().await;
            *current = file;
        }
        let file = self.file.read().await;
        let ctx = LifecycleCtx {
            shutdown: &self.shutdown,
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            logs: &self.logs,
            package_cache: &package_cache,
            state_dir: self.store.dir(),
            sandbox_token: &self.sandbox_token,
            empty_env_diagnostics: &empty_env_diagnostics,
            legacy_vms: &self.legacy_vms,
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
        report::empty_env_warnings(&empty_env_diagnostics.snapshot());
        (stopped, up)
    }

    /// Bounces one container. See [`lifecycle::restart_one`] for why this does
    /// not take the container's graph with it.
    ///
    /// The sandbox adapter is the exception: its VMs end with it, so restarting
    /// it is the operator's act on them too. They follow it under this one lock,
    /// so the reaper never sees them gone and their restart budget pays nothing,
    /// whether the adapter comes back or they fail with it.
    pub async fn restart_one(&self, key: &str, operation_id: String) -> OpResult {
        let mut inner = self.inner.lock().await;
        let vms = self.vms_hosted_by(&inner, key).await;
        inner.restarts.operator_took_control(Some(key));
        for vm in &vms {
            inner.restarts.operator_took_control(Some(vm));
        }
        let mut result = self
            .restart_one_locked(&mut inner, key, operation_id.clone(), None)
            .await;

        let ready = result
            .containers
            .iter()
            .any(|result| result.container == key && result.state == ChildStatus::Ready);
        if ready {
            for vm in &vms {
                let bounced = self
                    .restart_one_locked(&mut inner, vm, operation_id.clone(), None)
                    .await;
                result.changed |= bounced.changed;
                result.containers.extend(bounced.containers);
                if bounced.status == OpStatus::Failed {
                    result.status = OpStatus::Failed;
                    result.primary_error = result.primary_error.or(bounced.primary_error);
                }
            }
        } else if !vms.is_empty() && !self.shutdown.signal().requested() {
            daemon_line(
                &self.project_namespace,
                &format!(
                    "{key} did not come back, so these were not restarted: {}",
                    vms.join(", ")
                ),
                Tone::Warn,
            );
            self.fail_vms_of_lost_adapter(&mut inner, key, &vms, &mut result)
                .await;
        }

        let snapshot = inner.state.clone();
        drop(inner);
        let _ = self.store.save(&snapshot);
        result
    }

    async fn fail_vms_of_lost_adapter(
        &self,
        inner: &mut Inner,
        adapter: &str,
        vms: &[String],
        result: &mut OpResult,
    ) {
        let package_cache = self.package_cache();
        let empty_env_diagnostics = crate::config::EmptyEnvDiagnostics::default();
        let Inner {
            children, state, ..
        } = inner;
        let file = self.file.read().await;
        let ctx = LifecycleCtx {
            shutdown: &self.shutdown,
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            logs: &self.logs,
            package_cache: &package_cache,
            state_dir: self.store.dir(),
            sandbox_token: &self.sandbox_token,
            empty_env_diagnostics: &empty_env_diagnostics,
            legacy_vms: &self.legacy_vms,
        };
        lifecycle::fail_vms_of_lost_adapter(
            &ctx,
            children,
            &mut state.containers,
            adapter,
            vms,
            result,
        )
        .await;
    }

    /// [`Self::vms_hosted_by`] whichever of `keys` is the adapter.
    async fn vms_hosted_by_any(&self, inner: &Inner, keys: &[String]) -> Vec<String> {
        let mut vms = Vec::new();
        for key in keys {
            vms.extend(self.vms_hosted_by(inner, key).await);
        }
        vms
    }

    /// The VMs running under `key`, dependencies first; none unless `key` is
    /// this project's adapter.
    async fn vms_hosted_by(&self, inner: &Inner, key: &str) -> Vec<String> {
        let file = self.file.read().await;
        let adapter = crate::sandbox::adapter_name(&file);
        if !crate::sandbox::adapter_key(&file, adapter)
            .is_ok_and(|found| found.as_deref() == Some(key))
        {
            return Vec::new();
        }
        file.start_order()
            .unwrap_or_default()
            .into_iter()
            .filter(|vm| inner.children.contains_key(vm) && crate::sandbox::runs_own_vm(&file, vm))
            .collect()
    }

    async fn restart_one_locked(
        &self,
        inner: &mut Inner,
        key: &str,
        operation_id: String,
        supervised_attempt: Option<(u32, u32)>,
    ) -> OpResult {
        let package_cache = self.package_cache();
        let empty_env_diagnostics = crate::config::EmptyEnvDiagnostics::default();
        let Inner {
            children, state, ..
        } = inner;
        let file = self.file.read().await;
        let ctx = LifecycleCtx {
            shutdown: &self.shutdown,
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            logs: &self.logs,
            package_cache: &package_cache,
            state_dir: self.store.dir(),
            sandbox_token: &self.sandbox_token,
            empty_env_diagnostics: &empty_env_diagnostics,
            legacy_vms: &self.legacy_vms,
        };

        let result = if let Some((attempt, total_attempts)) = supervised_attempt {
            lifecycle::restart_one_supervised(
                &ctx,
                children,
                &mut state.containers,
                key,
                operation_id,
                attempt,
                total_attempts,
            )
            .await
        } else {
            lifecycle::restart_one(&ctx, children, &mut state.containers, key, operation_id).await
        };
        report::empty_env_warnings(&empty_env_diagnostics.snapshot());
        result
    }

    /// Whether container `key` runs here now.
    pub(crate) async fn is_running(&self, key: &str) -> bool {
        self.inner
            .lock()
            .await
            .children
            .get(key)
            .is_some_and(|child| !matches!(child.poll(), crate::process::Outcome::Exited(_)))
    }

    /// `compose::build` (spec 7.4): runs the `scripts.build` of `worker`, or of every container
    /// that declares one, in start order, and restarts nothing. The adapter starts first when a
    /// VM build needs it and the project is down. `cancel` stops the build that runs and skips the
    /// rest. A container whose `scripts.build` is `""` or absent is `skipped`.
    pub(crate) async fn build(
        &self,
        worker: Option<&str>,
        operation_id: &str,
        cancel: &crate::shutdown::ShutdownSignal,
    ) -> Result<lifecycle::BuildRun> {
        let file = self.file.read().await.clone();
        let targets: Vec<String> = match worker {
            Some(key) if !file.containers.contains_key(key) => {
                return Err(ComposeError::UnknownContainer {
                    container: key.to_string(),
                });
            }
            Some(key) => vec![key.to_string()],
            None => file
                .start_order()?
                .into_iter()
                .filter(|key| file.containers[key].scripts.build.is_some())
                .collect(),
        };
        let operation = crate::operation::active(operation_id);
        let vm_build = targets.iter().any(|key| {
            file.containers[key]
                .scripts
                .build_command()
                .is_some_and(crate::sandbox::is_sandbox_build)
        });
        let adapter = crate::sandbox::adapter_name(&file);
        if vm_build
            && let Ok(Some(adapter_key)) = crate::sandbox::adapter_key(&file, adapter)
            && !self.is_running(&adapter_key).await
        {
            if let Some(operation) = &operation {
                operation
                    .emit(
                        Some(&adapter_key),
                        "starting",
                        "starting the sandbox adapter",
                    )
                    .await;
            }
            // The build's cancel reaches the adapter's start too: a cancelled build waits for no
            // adapter, and builds nothing.
            let started = self
                .up_until_shutdown(
                    Some(&adapter_key),
                    format!("{operation_id}-adapter"),
                    cancel.clone(),
                )
                .await;
            match started {
                None => {
                    return Ok(lifecycle::BuildRun {
                        results: Vec::new(),
                        cancelled: true,
                    });
                }
                Some(started) if started.status == OpStatus::Failed => daemon_line(
                    &self.project_namespace,
                    &format!("{adapter_key} did not start, so the VM builds will fail"),
                    Tone::Warn,
                ),
                Some(_) => {}
            }
        }

        let package_cache = self.package_cache();
        let empty_env_diagnostics = crate::config::EmptyEnvDiagnostics::default();
        let ctx = LifecycleCtx {
            shutdown: &self.shutdown,
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            logs: &self.logs,
            package_cache: &package_cache,
            state_dir: self.store.dir(),
            sandbox_token: &self.sandbox_token,
            empty_env_diagnostics: &empty_env_diagnostics,
            legacy_vms: &self.legacy_vms,
        };
        let mut results = Vec::with_capacity(targets.len());
        for key in targets {
            if cancel.requested() {
                break;
            }
            let emit = |phase: &'static str, detail: String| {
                let (operation, key) = (operation.clone(), key.clone());
                async move {
                    if let Some(operation) = operation {
                        operation.emit(Some(&key), phase, detail).await;
                    }
                }
            };
            if file.containers[&key].scripts.build_command().is_none() {
                emit("skipped", "nothing to build".to_string()).await;
                results.push(lifecycle::BuildResult::skipped(&key));
                continue;
            }
            // Held to the end of the build: no start or rebuild of it builds beside this one.
            let Some(slot) = cancel.run(crate::build_step::slot(&file.path, &key)).await else {
                break;
            };
            if slot.stopping() {
                // Its remove is under way: it builds nothing.
                let error = crate::build_step::Outcome::stopped()
                    .into_error(&key)
                    .expect("a stopped build failed");
                emit("failed", error.to_string()).await;
                results.push(lifecycle::BuildResult::failed(
                    &key,
                    lifecycle::OpError::from(&error),
                ));
                continue;
            }
            emit("building", "running scripts.build".to_string()).await;
            let result = match lifecycle::build_one(&ctx, &key, cancel).await {
                None => lifecycle::BuildResult::skipped(&key),
                Some(Err(error)) => {
                    lifecycle::BuildResult::failed(&key, lifecycle::OpError::from(&error))
                }
                Some(Ok(crate::build_step::Outcome::Cancelled)) => lifecycle::BuildResult::failed(
                    &key,
                    lifecycle::OpError::from(&ComposeError::OperationCancelled {
                        operation_id: operation_id.to_string(),
                    }),
                ),
                Some(Ok(crate::build_step::Outcome::Built { build_id })) => {
                    lifecycle::BuildResult::built(&key, build_id)
                }
                Some(Ok(crate::build_step::Outcome::Failed { code, message })) => {
                    lifecycle::BuildResult::failed(
                        &key,
                        lifecycle::OpError::from_parts(code, message),
                    )
                }
            };
            match &result.error {
                None => emit("built", "scripts.build finished".to_string()).await,
                Some(error) => emit("failed", format!("{}: {}", error.code, error.message)).await,
            }
            results.push(result);
        }
        // Asked after the builds: one whose process the stop killed may have ended as `failed`
        // before the cancellation was seen.
        Ok(lifecycle::BuildRun {
            results,
            cancelled: cancel.requested(),
        })
    }

    /// Spec 5.3: rebuilds each of `keys` that runs here and whose package `file` changed, while it
    /// keeps running on its old build, so the restart that follows only runs the new one. Those
    /// whose build failed go on running as they are, and are not to be restarted. Stops at the
    /// first sign of `cancel`, building nothing more.
    pub(crate) async fn rebuild_changed(
        &self,
        file: &ComposeFile,
        keys: &[String],
        operation_id: &str,
        cancel: &crate::shutdown::ShutdownSignal,
    ) -> lifecycle::Rebuilt {
        let operation = crate::operation::active(operation_id);
        let package_cache = self.package_cache();
        let empty_env_diagnostics = crate::config::EmptyEnvDiagnostics::default();
        let ctx = LifecycleCtx {
            shutdown: &self.shutdown,
            file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            logs: &self.logs,
            package_cache: &package_cache,
            state_dir: self.store.dir(),
            sandbox_token: &self.sandbox_token,
            empty_env_diagnostics: &empty_env_diagnostics,
            legacy_vms: &self.legacy_vms,
        };
        let mut rebuilt = lifecycle::Rebuilt::default();
        for key in keys {
            if cancel.requested() {
                rebuilt.cancelled = true;
                break;
            }
            let Some(container) = file.containers.get(key) else {
                continue;
            };
            if !self.is_running(key).await {
                continue;
            }
            // Held to the end of its build: one already running ends first, and leaves what is
            // still due. The wait gives way to a cancel.
            let Some(slot) = cancel.run(crate::build_step::slot(&file.path, key)).await else {
                rebuilt.cancelled = true;
                break;
            };
            if slot.stopping() {
                // Its remove is under way: it gives up before deciding anything.
                rebuilt.failed.push((
                    key.clone(),
                    crate::build_step::Outcome::stopped()
                        .into_error(key)
                        .expect("a stopped build failed"),
                ));
                continue;
            }
            let Some(reason @ crate::build_step::Reason::PackageChanged { .. }) =
                crate::build_step::due(&file.path, key, container).await
            else {
                continue;
            };
            let detail = reason.describe();
            report::daemon_line(&format!("{key}: {detail}"), false);
            self.logs
                .sink(key)
                .write_line(crate::logs::LogStream::Stdout, &format!("{key}: {detail}"))
                .await;
            if let Some(operation) = &operation {
                operation.emit(Some(key), "building", detail).await;
            }
            let error = match Box::pin(lifecycle::build_one(&ctx, key, cancel)).await {
                None | Some(Ok(crate::build_step::Outcome::Built { .. })) => {
                    // Until it starts again, a retried add or update restarts it.
                    crate::build_step::mark_rebuilt(&file.path, key);
                    rebuilt.built.push(key.clone());
                    continue;
                }
                Some(Ok(crate::build_step::Outcome::Cancelled)) => {
                    rebuilt.cancelled = true;
                    break;
                }
                Some(Ok(failure)) => failure
                    .into_error(key)
                    .expect("a build that neither built nor was cancelled failed"),
                Some(Err(error)) => error,
            };
            if let Some(operation) = &operation {
                operation.emit(Some(key), "failed", error.to_string()).await;
            }
            rebuilt.failed.push((key.clone(), error));
        }
        rebuilt
    }

    /// The containers of `file` that run here on another build than their current one: one made
    /// from a package other than the one locked now (a rebuild an add or update left undone), or
    /// one an add or update rebuilt and did not restart, cancelled or failed
    /// ([`crate::build_step::rebuilt_unrestarted`]). A retried add or update finishes them (spec
    /// 5.3). A build made by `compose::build` or `iii sandbox build` is not one: its restart is the
    /// user's (spec 3.3).
    pub(crate) async fn pending_rebuilds(&self, file: &ComposeFile) -> Vec<String> {
        let mut pending = Vec::new();
        for (key, container) in &file.containers {
            if self.is_running(key).await
                && (crate::build_step::rebuilt_unrestarted(&file.path, key)
                    || crate::build_step::package_changed(&file.path, key, container).await)
            {
                pending.push(key.clone());
            }
        }
        pending
    }

    /// The VMs that go down with the adapters among `restart`: a VM ends with its adapter.
    pub(crate) async fn vms_restarting_with(&self, restart: &[String]) -> Vec<String> {
        let inner = self.inner.lock().await;
        self.vms_hosted_by_any(&inner, restart).await
    }

    pub async fn down(&self, target: Option<&str>, operation_id: String) -> OpResult {
        let package_cache = self.package_cache();
        let empty_env_diagnostics = crate::config::EmptyEnvDiagnostics::default();
        let mut inner = self.inner.lock().await;
        inner.restarts.operator_took_control(target);
        let Inner {
            children, state, ..
        } = &mut *inner;
        let file = self.file.read().await;

        let ctx = LifecycleCtx {
            shutdown: &self.shutdown,
            file: &file,
            engine: &self.engine,
            post_runs: &self.post_runs,
            compose_namespace: &self.compose_namespace,
            project_namespace: &self.project_namespace,
            engine_url: &self.engine_url,
            logs: &self.logs,
            package_cache: &package_cache,
            state_dir: self.store.dir(),
            sandbox_token: &self.sandbox_token,
            empty_env_diagnostics: &empty_env_diagnostics,
            legacy_vms: &self.legacy_vms,
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
            .iter()
            .map(|(key, container)| {
                let record = inner
                    .as_ref()
                    .and_then(|inner| inner.state.containers.get(key))
                    .or_else(|| stored.as_ref().and_then(|state| state.containers.get(key)));
                let running = inner.as_ref().is_some_and(|inner| {
                    inner.children.get(key).is_some_and(|child| {
                        matches!(
                            child.poll(),
                            crate::process::Outcome::Running | crate::process::Outcome::Unknown
                        )
                    })
                });
                let state = match (running, record.map(|record| record.status)) {
                    (true, _) => ChildStatus::Ready,
                    (false, Some(status)) => status,
                    (false, None) => ChildStatus::Stopped,
                };
                ContainerStatus {
                    container: key.clone(),
                    state,
                    version: container.version_label(),
                    requested: container.requested().map(str::to_owned),
                    pid: record.filter(|_| running).and_then(|record| record.pid),
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
        self.shutdown.cancel();
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
    /// Locked package version, or `local` for a `path://` worker. Absent for
    /// a package while the lock holds no result for its current selector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Registry selector of a `package://` worker as the project declares it
    /// in `worker-compose.yaml`: a tag such as `latest`, a semver range, or an
    /// exact version. `compose::update worker=<name>` resolves it again, and
    /// `version` is present only while the lock holds a result for this
    /// selector. Absent for a `path://` worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested: Option<String>,
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

#[cfg(all(test, unix))]
mod reconnect_shutdown_tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_interrupts_reconnect_wait_without_recreating_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");
        std::fs::write(&path,
            "startup_timeout: 60s\nstop_timeout: 100ms\ncontainers:\n  worker:\n    worker: path://.\n    scripts: { run: sleep 60 }\n").unwrap();
        let engine = Arc::new(EngineClient::connect(
            "ws://127.0.0.1:1/ws",
            "compose",
            "reconnect-test",
        ));
        let project = Project::open(
            &format!("reconnect-{}", uuid::Uuid::new_v4()),
            "default".into(),
            ComposeFile::load(&path).unwrap(),
            engine,
            "ws://127.0.0.1:1/ws".into(),
        )
        .await
        .unwrap();
        let mut command = tokio::process::Command::new("sleep");
        command.arg("60");
        let child = crate::process::spawn_supervised(command).unwrap();
        let pid = child.pid;
        {
            let mut inner = project.inner.lock().await;
            inner.state.containers.insert(
                "worker".into(),
                crate::state::ChildRecord::from_supervised(&child, ChildStatus::Ready),
            );
            inner
                .children
                .insert("worker".into(), Child::Process(child));
            project.store.save(&inner.state).unwrap();
        }
        let mut reconnect = Box::pin(project.reconcile_after_reconnect());
        assert!(futures::poll!(reconnect.as_mut()).is_pending());
        tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(reconnect, project.shutdown());
        })
        .await
        .expect("shutdown must not wait for the 60-second reconnect budget");
        assert!(!crate::process::is_running(pid));
        assert!(!project.store.path().exists());
        assert_eq!(
            project
                .wait_for_reregistration("worker", Duration::from_secs(60))
                .await,
            None
        );
    }
}

#[cfg(test)]
mod sandbox_token_tests {
    use super::*;

    #[tokio::test]
    async fn the_sandbox_token_is_minted_once_and_survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &path,
            "containers:\n  worker:\n    worker: path://.\n    scripts: { run: sleep 60 }\n",
        )
        .unwrap();
        let namespace = format!("token-{}", uuid::Uuid::new_v4());
        let open = || async {
            Project::open(
                &namespace,
                "default".into(),
                ComposeFile::load(&path).unwrap(),
                Arc::new(EngineClient::connect(
                    "ws://127.0.0.1:1/ws",
                    "compose",
                    "token-test",
                )),
                "ws://127.0.0.1:1/ws".into(),
            )
            .await
            .unwrap()
        };

        let first = open().await;
        assert_eq!(first.sandbox_token.len(), 32);
        let saved = first.store.load().unwrap().unwrap();
        assert_eq!(saved.sandbox_token.as_deref(), Some(&*first.sandbox_token));
        assert_eq!(open().await.sandbox_token, first.sandbox_token);
    }
}

#[cfg(test)]
mod restart_backoff_tests {
    use super::{RestartAttempts, RestartBookkeeping};

    #[test]
    fn operator_control_invalidates_a_supervisor_lease() {
        let mut restarts = RestartBookkeeping::default();
        let lease = restarts.claim("api").expect("claim restart");

        restarts.operator_took_control(Some("api"));

        assert!(!restarts.is_current("api", lease));
    }

    #[test]
    fn operator_control_pauses_retries_without_refilling_the_budget() {
        let mut restarts = RestartBookkeeping::default();
        restarts.attempts.insert(
            "api".to_string(),
            RestartAttempts {
                spent: 3,
                due: Some(tokio::time::Instant::now()),
            },
        );

        restarts.operator_took_control(Some("api"));

        let attempt = restarts.attempts["api"];
        assert_eq!((attempt.spent, attempt.due), (3, None));
    }

    #[test]
    fn removing_a_container_forgets_its_restart_bookkeeping() {
        let mut restarts = RestartBookkeeping::default();
        restarts.attempts.insert(
            "api".to_string(),
            RestartAttempts {
                spent: 3,
                due: Some(tokio::time::Instant::now()),
            },
        );
        let lease = restarts.claim("api").expect("claim restart");

        restarts.remove("api");

        assert_eq!(
            (
                restarts.attempts.contains_key("api"),
                restarts.is_current("api", lease),
            ),
            (false, false)
        );
    }
}

#[cfg(all(test, unix))]
mod adapter_restart_tests {
    use iii_sandbox_contract::{SandboxErrorCode, State};
    use serde_json::json;

    use super::*;
    use crate::{
        daemon::MutationOutcome,
        sandbox::tests::{answered, fake_engine_url, manifest, not_found, refused, status},
    };

    /// Writes its pid for the stand-in engine, which calls it registered while
    /// that pid lives. A `fail` file beside it makes its next start fail.
    const RUN: &str = "test -e fail && exit 1\necho $$ > pid\nexec sleep 60\n";

    /// What `iii` is to `vm` and `web`: `sandbox build` writes the `build.json` the real one
    /// would, and `sandbox run` notes its start in `../started`, then runs [`RUN`] in the worker's
    /// directory, standing for the VM. `../refuse` fails it as the command fails itself, and an
    /// `exit_on_boot` file makes it exit before it registers.
    pub(super) fn fake_iii() -> String {
        format!(
            "#!/bin/sh\n\
             if [ \"$2\" = build ]; then\n\
             mkdir -p \"$III_BUILD_DIR\" && printf '{{\"schema\":1,\"container\":\"%s\",\"source\":{{\"kind\":\"path\",\"dir\":\"./%s\"}},\"command\":\"iii sandbox build\",\"dependency_files\":{{}},\"built_at\":\"2026-11-10T00:00:00Z\"}}' \"$III_WORKER_NAME\" \"$III_WORKER_NAME\" > \"$III_BUILD_DIR/build.json\"\n\
             exit 0\n\
             fi\n\
             test -e ../refuse && exit 125\n\
             test -e exit_on_boot && exit 1\n\
             echo \"$III_WORKER_NAME\" >> ../started\n\
             {RUN}"
        )
    }

    struct Fixture {
        dir: tempfile::TempDir,
        project: Arc<Project>,
    }

    fn alive(dir: &Path, worker: &str) -> Option<u32> {
        std::fs::read_to_string(dir.join(worker).join("pid"))
            .ok()?
            .trim()
            .parse()
            .ok()
            .filter(|pid| crate::process::is_running(*pid))
    }

    /// `sb` is the adapter of `vm` and `web`, which run their VMs through `iii sandbox run`, the
    /// latter declared first but started after the former, beside the plain worker `api`. All of
    /// them are up.
    async fn fixture(policy: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        for (worker, text) in [
            ("sb", "name: sandbox-libkrun\nscripts: {start: sh run.sh}\n"),
            ("api", "scripts: {start: sh run.sh}\n"),
            (
                "vm",
                "runtime: {base_image: python}\nscripts: {start: run}\n",
            ),
            (
                "web",
                "runtime: {base_image: python}\nscripts: {start: run}\n",
            ),
        ] {
            manifest(dir.path(), worker, text);
            std::fs::write(dir.path().join(worker).join("run.sh"), RUN).unwrap();
        }
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(bin.join("iii"), fake_iii()).unwrap();
        std::fs::set_permissions(
            bin.join("iii"),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let vm = format!(
            "environment: {{PATH: '{}:/usr/bin:/bin'}}, scripts: {{build: iii sandbox build, run: \
             iii sandbox run}}{policy}",
            bin.display()
        );
        let path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &path,
            format!(
                "startup_timeout: 5s\nstop_timeout: 100ms\ncontainers:\n  \
                 web: {{worker: path://./web, start_after: [vm, sb], {vm}}}\n  \
                 vm: {{worker: path://./vm, start_after: [sb], {vm}}}\n  \
                 sb: {{worker: path://./sb}}\n  \
                 api: {{worker: path://./api}}\n"
            ),
        )
        .unwrap();

        let root = dir.path().to_path_buf();
        let (url, _called) = fake_engine_url(move |function, _| match function {
            "engine::workers::list" => {
                let workers = ["sb", "api", "vm", "web"]
                    .into_iter()
                    .filter(|worker| alive(&root, worker).is_some())
                    .map(|worker| json!({ "name": worker, "namespace": "shop" }))
                    .collect::<Vec<_>>();
                answered(json!({ "workers": workers }))
            }
            "engine::functions::list" => answered(json!({ "functions": [] })),
            "configuration::get" => {
                json!({ "error": { "code": "NOT_FOUND", "message": "none" } })
            }
            "configuration::migration-capabilities" => {
                answered(json!({ "source_priority_archive_revision": 1 }))
            }
            "configuration::migrate" => answered(json!({ "action": "missing" })),
            _ => not_found(),
        })
        .await;

        let engine = Arc::new(EngineClient::connect(&url, "t", "shop"));
        while !engine.is_connected() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let project = Project::open(
            &format!("adapter-restart-{}", uuid::Uuid::new_v4()),
            "shop".into(),
            ComposeFile::load(&path).unwrap(),
            engine,
            url,
        )
        .await
        .unwrap();
        let up = project.up(None, "up".into()).await;
        assert_eq!(up.status, OpStatus::Ok, "{up:?}");
        let _ = std::fs::remove_file(dir.path().join("started"));
        Fixture { dir, project }
    }

    impl Fixture {
        /// A supervision tick past a restart, which is what would count a VM container as
        /// crashed.
        async fn supervise(&self) {
            tokio::time::sleep(Duration::from_millis(1300)).await;
            self.project.reap_unexpected_exits().await;
        }

        async fn state_of(&self, key: &str) -> ChildStatus {
            self.project
                .status()
                .await
                .into_iter()
                .find(|status| status.container == key)
                .unwrap()
                .state
        }

        async fn last_error_of(&self, key: &str) -> String {
            self.project
                .status()
                .await
                .into_iter()
                .find(|status| status.container == key)
                .unwrap()
                .last_error
                .unwrap_or_default()
        }

        fn fail_adapter_starts(&self) {
            std::fs::write(self.dir.path().join("sb").join("fail"), "").unwrap();
        }

        async fn spent(&self) -> Vec<String> {
            let inner = self.project.inner.lock().await;
            inner.restarts.attempts.keys().cloned().collect()
        }

        /// The VM containers whose `iii sandbox run` started since the fixture was up.
        fn started(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.path().join("started"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }

        fn alive(&self, worker: &str) -> Option<u32> {
            alive(self.dir.path(), worker)
        }

        /// The file again, as an edit hands it over.
        fn file(&self) -> ComposeFile {
            ComposeFile::load(self.dir.path().join("worker-compose.yaml")).unwrap()
        }
    }

    fn containers(result: &OpResult) -> Vec<&str> {
        result
            .containers
            .iter()
            .map(|result| result.container.as_str())
            .collect()
    }

    /// The reply `compose::restart` gives for `result`.
    fn reply(result: &OpResult, worker: &str) -> serde_json::Value {
        serde_json::to_value(MutationOutcome::from_operations(
            result.status,
            result.changed,
            Some(worker),
            None,
            None,
            std::iter::once(result),
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn restarting_the_adapter_brings_its_vms_back_and_spends_no_budget() {
        let fixture = fixture("").await;
        let before = fixture.alive("sb");

        let result = fixture.project.restart_one("sb", "restart".into()).await;
        fixture.supervise().await;

        assert_eq!(fixture.state_of("vm").await, ChildStatus::Ready);
        assert_eq!(fixture.state_of("web").await, ChildStatus::Ready);
        assert_ne!(fixture.alive("sb"), before);
        assert_eq!(fixture.spent().await, Vec::<String>::new());
        assert_eq!(fixture.started(), ["vm", "web"], "dependencies first");
        assert_eq!(result.status, OpStatus::Ok, "{result:?}");
        assert_eq!(containers(&result), ["sb", "vm", "web"]);
        assert_eq!(
            reply(&result, "sb")["affected_workers"],
            json!(["vm", "web"])
        );
        fixture.project.shutdown().await;
    }

    #[tokio::test]
    async fn adapter_restarts_never_give_up_on_a_vm_that_always_restarts() {
        let fixture = fixture(", restart: always").await;

        for _ in 0..6 {
            let result = fixture.project.restart_one("sb", "restart".into()).await;
            assert_eq!(result.status, OpStatus::Ok, "{result:?}");
            fixture.supervise().await;
        }

        assert_eq!(fixture.state_of("vm").await, ChildStatus::Ready);
        assert_eq!(fixture.state_of("web").await, ChildStatus::Ready);
        assert_eq!(fixture.spent().await, Vec::<String>::new());
        fixture.project.shutdown().await;
    }

    /// Only the adapter's restart reaches beyond the container it names.
    #[tokio::test]
    async fn any_other_restart_touches_only_the_container_it_names() {
        let fixture = fixture("").await;
        let (adapter, web) = (fixture.alive("sb"), fixture.alive("web"));

        let api = fixture.alive("api");
        let result = fixture.project.restart_one("api", "restart".into()).await;
        assert_eq!(containers(&result), ["api"]);
        assert_ne!(fixture.alive("api"), api);
        assert_eq!(fixture.started(), Vec::<String>::new());

        let api = fixture.alive("api");
        let result = fixture.project.restart_one("vm", "restart".into()).await;
        assert_eq!(containers(&result), ["vm"]);
        assert_eq!(fixture.started(), ["vm"]);
        assert_eq!(reply(&result, "vm").get("affected_workers"), None);

        assert_eq!(fixture.alive("api"), api);
        assert_eq!(fixture.alive("sb"), adapter);
        assert_eq!(fixture.alive("web"), web);
        fixture.project.shutdown().await;
    }

    #[tokio::test]
    async fn an_adapter_that_does_not_come_back_fails_its_vms_unattempted() {
        let fixture = fixture("").await;
        fixture.fail_adapter_starts();

        let result = fixture.project.restart_one("sb", "restart".into()).await;

        assert_eq!(containers(&result), ["sb", "vm", "web"]);
        let reply = reply(&result, "sb");
        assert_eq!(reply["status"], "ok");
        assert_eq!(reply["affected_workers"], json!(["vm", "web"]));
        assert_eq!(reply["not_required_failures"], json!(["sb", "vm", "web"]));
        assert_eq!(fixture.state_of("sb").await, ChildStatus::Failed);
        for vm in ["vm", "web"] {
            assert_eq!(fixture.state_of(vm).await, ChildStatus::Failed);
            assert!(fixture.last_error_of(vm).await.contains("adapter 'sb'"));
        }
        assert_eq!(fixture.started(), Vec::<String>::new());
        fixture.project.shutdown().await;
    }

    /// Left in `children`, they were `already running` to this `up` and a crash
    /// to the supervision tick after it.
    #[tokio::test]
    async fn the_up_after_a_failed_adapter_restart_boots_its_vms_and_spends_no_budget() {
        for policy in ["", ", restart: always"] {
            let fixture = fixture(policy).await;
            fixture.fail_adapter_starts();
            fixture.project.restart_one("sb", "restart".into()).await;
            std::fs::remove_file(fixture.dir.path().join("sb").join("fail")).unwrap();

            let up = fixture.project.up(None, "up".into()).await;
            fixture.supervise().await;

            assert_eq!(up.status, OpStatus::Ok, "{up:?}");
            assert_eq!(fixture.started(), ["vm", "web"], "dependencies first");
            assert_eq!(fixture.state_of("vm").await, ChildStatus::Ready);
            assert_eq!(fixture.state_of("web").await, ChildStatus::Ready);
            assert_eq!(fixture.last_error_of("vm").await, "");
            assert_eq!(fixture.spent().await, Vec::<String>::new());
            fixture.project.shutdown().await;
        }
    }

    #[tokio::test]
    async fn a_required_vm_fails_the_restart_of_an_adapter_that_did_not_come_back() {
        let fixture = fixture(", required: true").await;
        fixture.fail_adapter_starts();

        let result = fixture.project.restart_one("sb", "restart".into()).await;

        assert_eq!(result.status, OpStatus::Failed, "{result:?}");
        let reply = reply(&result, "sb");
        assert_eq!(reply["affected_workers"], json!(["vm", "web"]));
        let message = reply["error"]["message"].as_str().unwrap();
        assert!(
            message.contains("'vm'") && message.contains("'sb'"),
            "{reply}"
        );
        fixture.project.shutdown().await;
    }

    /// The daemon going down is not the adapter failing: its VMs stop with the rest.
    #[tokio::test]
    async fn a_restart_cancelled_by_shutdown_does_not_fail_the_vms() {
        let fixture = fixture("").await;
        fixture.project.shutdown.cancel();

        let result = fixture.project.restart_one("sb", "restart".into()).await;

        assert_eq!(containers(&result), ["*"]);
        assert_eq!(fixture.state_of("vm").await, ChildStatus::Ready);
        assert_eq!(fixture.last_error_of("vm").await, "");
        fixture.project.shutdown().await;
    }

    #[tokio::test]
    async fn a_required_vm_that_does_not_come_back_fails_the_restart_that_took_it() {
        let fixture = fixture(", required: true").await;
        std::fs::write(fixture.dir.path().join("refuse"), "").unwrap();

        let result = fixture.project.restart_one("sb", "restart".into()).await;

        assert_eq!(result.status, OpStatus::Failed, "{result:?}");
        let reply = reply(&result, "sb");
        assert_eq!(reply["affected_workers"], json!(["vm", "web"]));
        assert_eq!(reply["error"]["code"], "CHILD_EXITED_BEFORE_REGISTRATION");
        assert!(fixture.alive("sb").is_some());
        assert_eq!(fixture.state_of("vm").await, ChildStatus::Failed);
        fixture.project.shutdown().await;
    }

    /// The adapter is `worker` and every VM is in `affected_workers`, so only
    /// the error can say which one this was.
    #[tokio::test]
    async fn a_vm_that_exits_before_registering_is_named_by_the_error() {
        let fixture = fixture(", required: true").await;
        std::fs::write(fixture.dir.path().join("web").join("exit_on_boot"), "").unwrap();

        let result = fixture.project.restart_one("sb", "restart".into()).await;

        let reply = reply(&result, "sb");
        assert_eq!(reply["status"], "failed", "{reply}");
        assert_eq!(reply["error"]["code"], "CHILD_EXITED_BEFORE_REGISTRATION");
        assert_eq!(
            reply["error"]["message"],
            "container 'web' exited with 1 before it registered"
        );
        fixture.project.shutdown().await;
    }

    /// `compose::add` restarts a changed adapter through the same list as any
    /// other container, a VM listed before it included.
    #[tokio::test]
    async fn an_edit_that_restarts_the_adapter_takes_its_vms_along() {
        let fixture = fixture(", restart: always").await;
        let before = fixture.alive("sb");

        let (restarted, up, interrupted) = fixture
            .project
            .reconcile_file(fixture.file(), &["web".into(), "sb".into()], "add".into())
            .await;
        fixture.supervise().await;

        assert!(!interrupted);
        assert_eq!(up.status, OpStatus::Ok, "{up:?}");
        assert_ne!(fixture.alive("sb"), before);
        assert_eq!(fixture.state_of("vm").await, ChildStatus::Ready);
        assert_eq!(fixture.state_of("web").await, ChildStatus::Ready);
        assert_eq!(fixture.spent().await, Vec::<String>::new());
        assert_eq!(fixture.started(), ["vm", "web"], "dependencies first");
        assert_eq!(restarted.len(), 1, "{restarted:?}");
        fixture.project.shutdown().await;
    }

    /// `compose::remove` of an adapter a VM still needs generates it again.
    #[tokio::test]
    async fn removing_an_adapter_its_vms_still_need_takes_them_along() {
        let fixture = fixture(", restart: always").await;
        let (api, before) = (fixture.alive("api"), fixture.alive("sb"));

        let (stopped, up) = fixture
            .project
            .reconcile_removals(fixture.file(), &["sb".into()], "remove".into())
            .await;
        fixture.supervise().await;

        assert_eq!(up.status, OpStatus::Ok, "{up:?}");
        assert_eq!(stopped.len(), 1, "{stopped:?}");
        assert_ne!(fixture.alive("sb"), before);
        assert_eq!(fixture.alive("api"), api);
        assert_eq!(fixture.state_of("vm").await, ChildStatus::Ready);
        assert_eq!(fixture.state_of("web").await, ChildStatus::Ready);
        assert_eq!(fixture.started(), ["vm", "web"], "dependencies first");
        assert_eq!(fixture.spent().await, Vec::<String>::new());
        fixture.project.shutdown().await;
    }

    /// What the stand-in adapter and engine hold for `own`'s VM.
    #[derive(Default)]
    struct OwnVm {
        /// What `worker::status` says of instance 1; `None` is no such VM (S504).
        state: Option<State>,
        /// A worker named `own` is connected, with or without a command running.
        name_held: bool,
        /// Each `worker::stop` it answered.
        stops: Vec<serde_json::Value>,
    }

    /// `sb` beside `own`, a container that runs its own VM with `policy`, neither started.
    async fn own_vm_project(
        policy: &str,
    ) -> (
        tempfile::TempDir,
        Arc<Project>,
        Arc<std::sync::Mutex<OwnVm>>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        manifest(
            dir.path(),
            "sb",
            "name: sandbox-libkrun\nscripts: {start: sh run.sh}\n",
        );
        std::fs::write(dir.path().join("sb/run.sh"), RUN).unwrap();
        manifest(
            dir.path(),
            "own",
            "runtime: {base_image: python}\nscripts: {start: run}\n",
        );
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(bin.join("iii"), fake_iii()).unwrap();
        std::fs::set_permissions(
            bin.join("iii"),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &path,
            format!(
                "startup_timeout: 5s\nstop_timeout: 100ms\ncontainers:\n  \
                 sb: {{worker: path://./sb}}\n  \
                 own:\n    worker: path://./own\n    start_after: [sb]\n    {policy}\n    \
                 environment: {{PATH: '{}:/usr/bin:/bin'}}\n    \
                 scripts: {{build: iii sandbox build, run: iii sandbox run}}\n",
                bin.display()
            ),
        )
        .unwrap();

        let vm = Arc::new(std::sync::Mutex::new(OwnVm::default()));
        let (held, root) = (vm.clone(), dir.path().to_path_buf());
        let (url, _called) = fake_engine_url(move |function, payload| match function {
            "engine::workers::list" => {
                let name_held = held.lock().unwrap().name_held;
                let workers = ["sb", "own"]
                    .into_iter()
                    .filter(|worker| {
                        alive(&root, worker).is_some() || (*worker == "own" && name_held)
                    })
                    .map(|worker| json!({ "name": worker, "namespace": "shop" }))
                    .collect::<Vec<_>>();
                answered(json!({ "workers": workers }))
            }
            "engine::functions::list" => answered(json!({ "functions": [] })),
            "sandbox-libkrun::worker::status" => {
                match (&held.lock().unwrap().state, payload["instance"].as_u64()) {
                    (None, _) => refused(SandboxErrorCode::S504, 0),
                    (Some(state), Some(1)) => answered(status(state.clone(), None, None)),
                    (Some(_), _) => refused(SandboxErrorCode::S503, 1),
                }
            }
            "sandbox-libkrun::worker::stop" => {
                let mut vm = held.lock().unwrap();
                vm.stops.push(payload.clone());
                vm.state = Some(State::Stopped);
                vm.name_held = false;
                answered(status(State::Stopped, None, Some(15)))
            }
            "configuration::get" => {
                json!({ "error": { "code": "NOT_FOUND", "message": "none" } })
            }
            "configuration::migration-capabilities" => {
                answered(json!({ "source_priority_archive_revision": 1 }))
            }
            "configuration::migrate" => answered(json!({ "action": "missing" })),
            _ => not_found(),
        })
        .await;
        let engine = Arc::new(EngineClient::connect(&url, "t", "shop"));
        while !engine.is_connected() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let project = Project::open(
            &format!("own-vm-{}", uuid::Uuid::new_v4()),
            "shop".into(),
            ComposeFile::load(&path).unwrap(),
            engine,
            url,
        )
        .await
        .unwrap();
        (dir, project, vm)
    }

    /// SIGKILLs `own`'s `iii sandbox run` as its VM runs on under the worker's name, and
    /// lets the supervisor see it. Returns the command's pid.
    async fn kill_own_vm_command(
        dir: &Path,
        project: &Project,
        vm: &std::sync::Mutex<OwnVm>,
    ) -> u32 {
        {
            let mut vm = vm.lock().unwrap();
            vm.state = Some(State::Running);
            vm.name_held = true;
        }
        // The command writes its own pid once it has started: wait for a live one rather
        // than race a slow start (a killed command's file reads as none). It is not the
        // pid the supervisor runs where `sh -c` stays the command's parent (dash).
        let pid = own_command_pid(dir).await;
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid as i32),
            nix::sys::signal::Signal::SIGKILL,
        )
        .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        project.reap_unexpected_exits().await;
        pid
    }

    /// The pid `own`'s running command wrote, once it has written one.
    async fn own_command_pid(dir: &Path) -> u32 {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(pid) = alive(dir, "own") {
                return pid;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "own's command never wrote its pid"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn own_status(project: &Project) -> ContainerStatus {
        project
            .status()
            .await
            .into_iter()
            .find(|status| status.container == "own")
            .unwrap()
    }

    /// Up, ready, on a command of its own: the one a crash took is `killed`.
    async fn assert_own_vm_came_back(project: &Project, killed: u32) {
        let own = own_status(project).await;
        assert_eq!(own.state, ChildStatus::Ready, "{:?}", own.last_error);
        assert_eq!(own.last_error, None);
        assert_ne!(own.pid, Some(killed));
        assert!(crate::process::is_running(own.pid.unwrap()));
    }

    /// Its VM lives on under its name, so there is no release to wait out and nothing to
    /// stop: the next command reattaches. Each crash costs one attempt, however many come.
    #[tokio::test]
    async fn a_killed_iii_sandbox_run_costs_one_attempt_and_comes_back_on_its_vm() {
        for policy in ["restart: always", "restart: on-failure"] {
            let (dir, project, vm) = own_vm_project(policy).await;
            let up = project.up(None, "up".into()).await;
            assert_eq!(up.status, OpStatus::Ok, "{up:?}");

            for crash in 1..=2 {
                let killed = kill_own_vm_command(dir.path(), &project, &vm).await;

                assert_own_vm_came_back(&project, killed).await;
                assert_eq!(
                    project.inner.lock().await.restarts.attempts["own"].spent,
                    crash
                );
                // The pid the next crash kills, written once the command has started.
                own_command_pid(dir.path()).await;
            }
            assert_eq!(vm.lock().unwrap().stops, Vec::<serde_json::Value>::new());
            project.shutdown().await;
        }
    }

    /// With no policy to run the command again, an operator's `up` does, as long as the
    /// lease has not taken the VM.
    #[tokio::test]
    async fn an_up_after_a_killed_iii_sandbox_run_comes_back_on_its_vm() {
        let (dir, project, vm) = own_vm_project("").await;
        let up = project.up(None, "up".into()).await;
        assert_eq!(up.status, OpStatus::Ok, "{up:?}");
        let killed = kill_own_vm_command(dir.path(), &project, &vm).await;
        assert_eq!(own_status(&project).await.state, ChildStatus::Failed);

        let up = project.up(None, "up".into()).await;

        assert_eq!(
            reply(&up, "own")["not_required_failures"],
            json!(null),
            "{up:?}"
        );
        assert_own_vm_came_back(&project, killed).await;
        assert_eq!(vm.lock().unwrap().stops, Vec::<serde_json::Value>::new());
        project.shutdown().await;
    }

    /// A restart someone asked for, after an edit say, boots the VM anew even though the
    /// command that ran it is already gone.
    #[tokio::test]
    async fn a_restart_asked_for_after_a_killed_iii_sandbox_run_stops_its_vm() {
        let (dir, project, vm) = own_vm_project("").await;
        let up = project.up(None, "up".into()).await;
        assert_eq!(up.status, OpStatus::Ok, "{up:?}");
        let killed = kill_own_vm_command(dir.path(), &project, &vm).await;

        let restart = project.restart_one("own", "restart".into()).await;

        assert_eq!(restart.status, OpStatus::Ok, "{restart:?}");
        assert_own_vm_came_back(&project, killed).await;
        let stops = std::mem::take(&mut vm.lock().unwrap().stops);
        assert_eq!(stops.len(), 1, "{stops:?}");
        assert_eq!(stops[0]["instance"], 1);
        assert_eq!(stops[0]["auth"], json!(project.sandbox_token));
        project.shutdown().await;
    }

    /// Its VM runs on under the name until the lease ends it, so `down` then `up` would
    /// reattach to the old source and environment.
    #[tokio::test]
    async fn a_down_after_a_killed_iii_sandbox_run_stops_its_vm() {
        let (dir, project, vm) = own_vm_project("").await;
        let up = project.up(None, "up".into()).await;
        assert_eq!(up.status, OpStatus::Ok, "{up:?}");
        kill_own_vm_command(dir.path(), &project, &vm).await;

        let down = project.down(Some("own"), "down".into()).await;

        assert!(down.changed, "{down:?}");
        let stops = std::mem::take(&mut vm.lock().unwrap().stops);
        assert_eq!(stops.len(), 1, "{stops:?}");
        assert_eq!(own_status(&project).await.state, ChildStatus::Stopped);
        project.shutdown().await;
    }

    /// A name held with no VM of ours running, or none that could hold it yet, is a stranger's.
    #[tokio::test]
    async fn a_name_held_while_the_vm_does_not_run_is_still_refused() {
        for state in [
            None,
            Some(State::Preparing),
            Some(State::Exited),
            Some(State::Stopped),
        ] {
            let (dir, project, vm) = own_vm_project("").await;
            {
                let mut vm = vm.lock().unwrap();
                vm.state = state.clone();
                vm.name_held = true;
            }

            let up = project.up(None, "up".into()).await;

            let own = up
                .containers
                .iter()
                .find(|result| result.container == "own")
                .unwrap();
            assert_eq!(own.state, ChildStatus::Failed, "{state:?}");
            let error = own.error.as_ref().unwrap();
            assert_eq!(error.code, "CONTAINER_NAME_TAKEN", "{state:?}");
            assert_eq!(alive(dir.path(), "own"), None);
            project.shutdown().await;
        }
    }

    /// A build can outlast `startup_timeout`, which starts with the process, so its first build
    /// is over before the process starts (spec 5.2), and a restart only runs it (spec 3.3).
    #[tokio::test]
    async fn a_container_that_runs_its_own_vm_is_built_before_its_process_starts() {
        let (dir, project, _) = own_vm_project("").await;
        let build = dir.path().join(".build/own/build.json");

        let up = project.up(None, "up".into()).await;

        assert_eq!(up.status, OpStatus::Ok, "{up:?}");
        assert!(alive(dir.path(), "own").is_some());
        let built = std::fs::metadata(&build).unwrap().modified().unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".build/.gitignore")).unwrap(),
            "*\n"
        );

        let restart = project.restart_one("own", "restart".into()).await;
        assert_eq!(restart.status, OpStatus::Ok, "{restart:?}");
        assert_eq!(
            std::fs::metadata(&build).unwrap().modified().unwrap(),
            built
        );
        project.shutdown().await;
    }

    /// Its VM ended with the adapter, but its `iii sandbox run` may not have.
    #[tokio::test]
    async fn a_process_running_its_own_vm_is_stopped_when_the_adapter_does_not_come_back() {
        let (dir, project, _) = own_vm_project("").await;
        let up = project.up(None, "up".into()).await;
        assert_eq!(up.status, OpStatus::Ok, "{up:?}");
        std::fs::write(dir.path().join("sb").join("fail"), "").unwrap();

        let result = project.restart_one("sb", "restart".into()).await;

        assert_eq!(alive(dir.path(), "own"), None);
        assert_eq!(containers(&result), ["sb", "own"]);
        let status = project.status().await;
        let own = status.iter().find(|status| status.container == "own");
        assert_eq!(own.unwrap().state, ChildStatus::Failed);
        project.shutdown().await;
    }

    /// `status: ok` means every required worker is up, so the reply names the
    /// rest, as it does for a VM restarted by name.
    #[tokio::test]
    async fn a_vm_that_is_not_required_and_does_not_come_back_is_named_in_the_reply() {
        let fixture = fixture("").await;
        std::fs::write(fixture.dir.path().join("refuse"), "").unwrap();

        let result = fixture.project.restart_one("sb", "restart".into()).await;

        assert_eq!(result.status, OpStatus::Ok, "{result:?}");
        let reply = reply(&result, "sb");
        assert_eq!(reply["affected_workers"], json!(["vm", "web"]));
        assert_eq!(reply["not_required_failures"], json!(["vm", "web"]));
        assert_eq!(fixture.state_of("vm").await, ChildStatus::Failed);
        fixture.project.shutdown().await;
    }

    /// `sb` beside `own`, declared with `fields`, `iii` being `script`.
    fn write_small_project(dir: &Path, fields: &str, script: &str) -> PathBuf {
        manifest(
            dir,
            "sb",
            "name: sandbox-libkrun\nscripts: {start: sh run.sh}\n",
        );
        std::fs::write(dir.join("sb/run.sh"), RUN).unwrap();
        manifest(
            dir,
            "own",
            "runtime: {base_image: python}\nscripts: {start: run}\n",
        );
        let bin = dir.join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(bin.join("iii"), script).unwrap();
        std::fs::set_permissions(
            bin.join("iii"),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let path = dir.join("worker-compose.yaml");
        std::fs::write(
            &path,
            format!(
                "startup_timeout: 5s\nstop_timeout: 100ms\ncontainers:\n  sb: {{worker: path://./sb}}\n  \
                 own:\n    worker: path://./own\n    environment: {{PATH: '{}:/usr/bin:/bin'}}\n    {fields}\n",
                bin.display()
            ),
        )
        .unwrap();
        path
    }

    /// The project at `path`, opened by daemon `daemon` on a stand-in engine that asks
    /// `answer` first: what `sb` and `own` hold of the worker list while their pids live, and the
    /// configuration calls, are answered when it does not.
    async fn open_small_project(
        path: &Path,
        daemon: &str,
        answer: impl Fn(&str, &serde_json::Value) -> Option<serde_json::Value> + Send + Sync + 'static,
    ) -> Arc<Project> {
        let root = path.parent().unwrap().to_path_buf();
        let (url, _called) = fake_engine_url(move |function, payload| {
            if let Some(answer) = answer(function, payload) {
                return answer;
            }
            match function {
                "engine::workers::list" => {
                    let workers = ["sb", "own"]
                        .into_iter()
                        .filter(|worker| alive(&root, worker).is_some())
                        .map(|worker| json!({ "name": worker, "namespace": "shop" }))
                        .collect::<Vec<_>>();
                    answered(json!({ "workers": workers }))
                }
                "engine::functions::list" => answered(json!({ "functions": [] })),
                "configuration::get" => {
                    json!({ "error": { "code": "NOT_FOUND", "message": "none" } })
                }
                "configuration::migration-capabilities" => {
                    answered(json!({ "source_priority_archive_revision": 1 }))
                }
                "configuration::migrate" => answered(json!({ "action": "missing" })),
                _ => not_found(),
            }
        })
        .await;
        let engine = Arc::new(EngineClient::connect(&url, "t", "shop"));
        while !engine.is_connected() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Project::open(
            daemon,
            "shop".into(),
            ComposeFile::load(path).unwrap(),
            engine,
            url,
        )
        .await
        .unwrap()
    }

    const EXPLICIT: &str =
        "start_after: [sb]\n    scripts: {build: iii sandbox build, run: iii sandbox run}";

    /// Spec 7.3, step 2: the VM a 0.26 daemon left running holds the worker's name. Once the
    /// adapter is up it is stopped, before the first build, its record is dropped, and its data
    /// stays: nothing calls `worker::destroy`.
    #[tokio::test]
    async fn a_vm_a_026_daemon_left_running_is_stopped_before_the_first_build() {
        stops_the_026_vm_before_the_first_build(ChildStatus::Ready).await;
    }

    /// The same VM, in a project reopened before its container started: the record this daemon
    /// marked stopped, or failed when a start could not stop the VM, still names it.
    #[tokio::test]
    async fn a_reopened_project_still_stops_the_026_vm_it_had_not_stopped() {
        for recorded in [ChildStatus::Stopped, ChildStatus::Failed] {
            stops_the_026_vm_before_the_first_build(recorded).await;
        }
    }

    async fn stops_the_026_vm_before_the_first_build(recorded: ChildStatus) {
        let dir = tempfile::tempdir().unwrap();
        let path = write_small_project(dir.path(), EXPLICIT, &fake_iii());
        let daemon = format!("legacy-vm-{}", uuid::Uuid::new_v4());
        let file = ComposeFile::load(&path).unwrap();
        let store = StateStore::for_project(&daemon, &file.path).unwrap();
        let mut state = DaemonState::new(&file.path, "shop");
        state.owner = Some(crate::state::Owner::current());
        state.containers.insert(
            "own".into(),
            crate::state::ChildRecord {
                pid: None,
                birth: None,
                vm: Some(crate::state::VmRecord {
                    adapter: "sandbox-libkrun".into(),
                    namespace: "shop".into(),
                    vm_id: "v026".into(),
                    instance: 4,
                }),
                status: recorded,
                started_at: 0,
                last_error: None,
            },
        );
        store.save(&state).unwrap();

        let legacy = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let calls = Arc::new(std::sync::Mutex::new(Vec::<(String, bool)>::new()));
        let (running, called, build) = (
            legacy.clone(),
            calls.clone(),
            dir.path().join(".build/own/build.json"),
        );
        let project = open_small_project(&path, &daemon, move |function, payload| {
            let live = running.load(std::sync::atomic::Ordering::SeqCst);
            match function {
                // The 0.26 guest holds the name while its VM runs.
                "engine::workers::list" if live => Some(answered(
                    json!({ "workers": [{ "name": "own", "namespace": "shop" }] }),
                )),
                "sandbox-libkrun::worker::status"
                    if live && payload["vm_id"] == "v026" && payload["instance"] == 4 =>
                {
                    Some(answered(status(State::Running, None, None)))
                }
                "sandbox-libkrun::worker::stop" | "sandbox-libkrun::worker::destroy" => {
                    called
                        .lock()
                        .unwrap()
                        .push((function.to_string(), build.exists()));
                    running.store(false, std::sync::atomic::Ordering::SeqCst);
                    Some(answered(status(State::Stopped, None, Some(15))))
                }
                "sandbox-libkrun::worker::status" => Some(refused(SandboxErrorCode::S504, 0)),
                _ => None,
            }
        })
        .await;

        let up = project.up(None, "up".into()).await;

        assert_eq!(up.status, OpStatus::Ok, "{recorded:?}: {up:?}");
        assert_eq!(
            *calls.lock().unwrap(),
            [("sandbox-libkrun::worker::stop".to_string(), false)],
            "{recorded:?}"
        );
        assert!(dir.path().join(".build/own/build.json").exists());
        assert!(alive(dir.path(), "own").is_some());
        assert_eq!(store.load().unwrap().unwrap().containers["own"].vm, None);
        project.shutdown().await;
    }

    /// Spec 5.2: a failed first build fails the start with `BUILD_FAILED`, carrying the build's
    /// own code; `build: ""` starts at once, with no build.
    #[tokio::test]
    async fn a_failed_first_build_fails_the_start_with_build_failed() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_small_project(
            dir.path(),
            "scripts: {build: 'echo no; exit 3', run: sh ../sb/run.sh}",
            &fake_iii(),
        );
        let project = open_small_project(
            &path,
            &format!("build-failed-{}", uuid::Uuid::new_v4()),
            |_, _| None,
        )
        .await;

        let up = project.up(Some("own"), "up".into()).await;

        let own = &up.containers[0];
        assert_eq!(own.state, ChildStatus::Failed, "{up:?}");
        let error = own.error.clone().unwrap();
        assert_eq!(error.code, "BUILD_FAILED");
        let cause = error.cause.unwrap();
        assert_eq!(
            (cause.code.as_str(), cause.message.as_str()),
            ("EXIT_3", "no")
        );
        project.shutdown().await;

        let dir = tempfile::tempdir().unwrap();
        let path = write_small_project(
            dir.path(),
            "scripts: {build: '', run: sh ../sb/run.sh}",
            &fake_iii(),
        );
        let project = open_small_project(
            &path,
            &format!("no-build-{}", uuid::Uuid::new_v4()),
            |_, _| None,
        )
        .await;
        let up = project.up(Some("own"), "up".into()).await;
        assert_eq!(up.status, OpStatus::Ok, "{up:?}");
        assert!(!dir.path().join(".build").exists());
        project.shutdown().await;
    }

    /// Spec 7.4: `compose::build` starts the adapter of a project that is down, builds, and
    /// starts nothing else.
    #[tokio::test]
    async fn compose_build_starts_the_adapter_and_runs_nothing() {
        let fixture = fixture("").await;
        fixture.project.down(None, "down".into()).await;
        assert!(fixture.alive("sb").is_none());
        std::fs::remove_dir_all(fixture.dir.path().join(".build")).unwrap();
        let controller = crate::shutdown::ShutdownController::default();

        let results = fixture
            .project
            .build(None, "build", &controller.signal())
            .await
            .unwrap()
            .results;

        let built: Vec<_> = results
            .iter()
            .map(|result| (result.worker.as_str(), result.status))
            .collect();
        assert_eq!(
            built,
            [
                ("vm", lifecycle::BuildStatus::Built),
                ("web", lifecycle::BuildStatus::Built)
            ]
        );
        assert!(fixture.alive("sb").is_some());
        assert_eq!(fixture.alive("vm"), None);
        assert_eq!(fixture.started(), Vec::<String>::new());
        assert!(fixture.dir.path().join(".build/vm/build.json").exists());
        fixture.project.shutdown().await;
    }

    /// The cancel of `compose::build` reaches the start of the adapter it waits for: the build
    /// ends at once, cancelled, and builds nothing.
    #[tokio::test]
    async fn a_cancelled_compose_build_stops_waiting_for_the_adapter() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_small_project(dir.path(), EXPLICIT, &fake_iii());
        // No worker ever registers: the adapter's start would wait out its 5 s timeout.
        let project = open_small_project(
            &path,
            &format!("build-cancel-{}", uuid::Uuid::new_v4()),
            |function, _| {
                (function == "engine::workers::list").then(|| answered(json!({ "workers": [] })))
            },
        )
        .await;
        let controller = crate::shutdown::ShutdownController::default();
        let signal = controller.signal();
        let began = std::time::Instant::now();
        let (run, ()) = tokio::join!(project.build(None, "build", &signal), async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            controller.cancel();
        });

        let run = run.unwrap();
        assert!(run.cancelled);
        assert!(run.results.is_empty(), "{:?}", run.results);
        assert!(
            began.elapsed() < Duration::from_secs(4),
            "{:?}",
            began.elapsed()
        );
        assert!(!dir.path().join(".build/own").exists());
        project.shutdown().await;
    }

    /// The adapter refuses to destroy the state of a build that runs (S509): `remove` stops the
    /// build first, then destroys, asking again while the adapter aborts it.
    #[tokio::test]
    async fn remove_stops_a_running_build_before_it_destroys_the_vm() {
        let dir = tempfile::tempdir().unwrap();
        let script = fake_iii().replacen(
            "if [ \"$2\" = build ]; then\n",
            "if [ \"$2\" = build ]; then\ntest -e ../slow && echo $$ > ../build.pid && exec sleep 30\n",
            1,
        );
        let path = write_small_project(dir.path(), EXPLICIT, &script);
        let pid_file = dir.path().join("build.pid");
        let destroys = Arc::new(std::sync::Mutex::new(Vec::<bool>::new()));
        let (called, pid) = (destroys.clone(), pid_file.clone());
        let project = open_small_project(
            &path,
            &format!("remove-build-{}", uuid::Uuid::new_v4()),
            move |function, _| match function {
                "sandbox-libkrun::worker::destroy" => {
                    let building = std::fs::read_to_string(&pid)
                        .ok()
                        .and_then(|pid| pid.trim().parse().ok())
                        .is_some_and(crate::process::is_running);
                    let mut called = called.lock().unwrap();
                    called.push(building);
                    Some(if building || called.len() == 1 {
                        refused(SandboxErrorCode::S509, 0)
                    } else {
                        answered(json!({ "destroyed": true }))
                    })
                }
                "sandbox-libkrun::worker::status" | "sandbox-libkrun::worker::stop" => {
                    Some(refused(SandboxErrorCode::S504, 0))
                }
                _ => None,
            },
        )
        .await;
        let up = project.up(None, "up".into()).await;
        assert_eq!(up.status, OpStatus::Ok, "{up:?}");
        std::fs::write(dir.path().join("slow"), "").unwrap();
        let controller = crate::shutdown::ShutdownController::default();
        let building = tokio::spawn({
            let (project, signal) = (project.clone(), controller.signal());
            async move { project.build(Some("own"), "build", &signal).await }
        });
        while !pid_file.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let next = ComposeFile::parse(
            "startup_timeout: 5s\nstop_timeout: 100ms\ncontainers:\n  sb: {worker: path://./sb}\n",
            &path,
        )
        .unwrap();
        let (removed, _) = project
            .reconcile_removals(next, &["own".into()], "remove".into())
            .await;

        assert_eq!(removed[0].status, OpStatus::Ok, "{removed:?}");
        assert_eq!(*destroys.lock().unwrap(), [false, false]);
        let results = building.await.unwrap().unwrap().results;
        assert_eq!(results[0].status, lifecycle::BuildStatus::Failed);
        assert_eq!(results[0].error.as_ref().unwrap().code, "BUILD_STOPPED");
        assert!(!dir.path().join(".build/own").exists());
        project.shutdown().await;
    }

    /// Spec 5.3: a container whose package changed is rebuilt while its old build runs, then
    /// its restart runs the new build and builds nothing; a rebuild that fails leaves it running
    /// and is `BUILD_FAILED`.
    #[tokio::test]
    async fn a_changed_package_is_rebuilt_while_the_old_build_runs() {
        use crate::build_record;

        let dir = tempfile::tempdir().unwrap();
        let path = write_small_project(
            dir.path(),
            "scripts: {build: 'touch ../building; sleep 1; rm ../building', run: sh ../sb/run.sh}",
            &fake_iii(),
        );
        let project = open_small_project(
            &path,
            &format!("rebuild-{}", uuid::Uuid::new_v4()),
            |_, _| None,
        )
        .await;
        let up = project.up(Some("own"), "up".into()).await;
        assert_eq!(up.status, OpStatus::Ok, "{up:?}");
        let running = alive(dir.path(), "own").unwrap();
        let build = dir.path().join(".build/own");
        let built = build_record::read(&build).unwrap().unwrap();
        let moved = |record: &crate::build_record::BuildRecord| {
            let mut moved = record.clone();
            moved.source = crate::build_record::BuildSource::Path {
                dir: "./old".into(),
            };
            build_record::write(&build, &moved).unwrap();
        };
        moved(&built);
        let file = ComposeFile::load(&path).unwrap();
        let marker = dir.path().join("building");
        let own = ["own".to_string()];

        let controller = crate::shutdown::ShutdownController::default();
        let signal = controller.signal();
        let ((rebuilt, lines), during) = tokio::join!(
            crate::report::capture_daemon_lines(
                project.rebuild_changed(&file, &own, "update", &signal)
            ),
            async {
                while !marker.exists() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                alive(dir.path(), "own")
            }
        );

        assert!(rebuilt.failed.is_empty(), "{rebuilt:?}");
        assert_eq!(rebuilt.built, own);
        assert_eq!(
            during,
            Some(running),
            "the old build stopped during the rebuild"
        );
        assert_eq!(lines, ["own: package changed (./old -> ./own); rebuilding"]);
        assert_eq!(
            build_record::read(&build).unwrap().unwrap().source,
            built.source
        );
        let (restart, lines) =
            crate::report::capture_daemon_lines(project.restart_one("own", "restart".into())).await;
        assert_eq!(restart.status, OpStatus::Ok, "{restart:?}");
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("rebuilding") || line.contains("no build yet")),
            "{lines:?}"
        );

        // A failed rebuild: still running on the build it had.
        let running = alive(dir.path(), "own").unwrap();
        moved(&build_record::read(&build).unwrap().unwrap());
        std::fs::write(
            &path,
            std::fs::read_to_string(&path).unwrap().replace(
                "touch ../building; sleep 1; rm ../building",
                "echo nope; exit 4",
            ),
        )
        .unwrap();
        let failing = ComposeFile::load(&path).unwrap();
        let failed = project
            .rebuild_changed(&failing, &own, "update", &signal)
            .await
            .failed;
        assert_eq!(failed.len(), 1, "{failed:?}");
        assert_eq!(failed[0].1.code(), "BUILD_FAILED");
        assert_eq!(alive(dir.path(), "own"), Some(running));

        // A remove under way: the rebuild gives up before it decides anything.
        let stopping = crate::build_step::stop(&path, "own").await;
        let failed = project
            .rebuild_changed(&failing, &own, "update", &signal)
            .await
            .failed;
        assert!(
            matches!(
                &failed[..],
                [(key, crate::error::ComposeError::BuildFailed { code, .. })]
                    if key == "own" && code == "BUILD_STOPPED"
            ),
            "{failed:?}"
        );
        drop(stopping);
        project.shutdown().await;
    }

    /// The VMs that go down with a restarting adapter: what an add cannot keep on its old build.
    #[tokio::test]
    async fn the_vms_of_a_restarting_adapter_restart_with_it() {
        let fixture = fixture("").await;
        let mut vms = fixture.project.vms_restarting_with(&["sb".into()]).await;
        vms.sort();
        assert_eq!(vms, ["vm", "web"]);
        assert!(
            fixture
                .project
                .vms_restarting_with(&["api".into(), "vm".into()])
                .await
                .is_empty()
        );
        fixture.project.shutdown().await;
    }

    /// A cancel stops a rebuild at once and builds nothing more: the container keeps running on
    /// its old build, and the caller restarts nothing.
    #[tokio::test]
    async fn a_cancelled_rebuild_stops_and_builds_nothing_more() {
        use crate::build_record;

        let dir = tempfile::tempdir().unwrap();
        let path = write_small_project(
            dir.path(),
            "scripts: {build: 'test -e ../slow && touch ../building && sleep 30; true', run: sh ../sb/run.sh}",
            &fake_iii(),
        );
        let project = open_small_project(
            &path,
            &format!("cancel-rebuild-{}", uuid::Uuid::new_v4()),
            |_, _| None,
        )
        .await;
        let up = project.up(Some("own"), "up".into()).await;
        assert_eq!(up.status, OpStatus::Ok, "{up:?}");
        let running = alive(dir.path(), "own").unwrap();
        let build = dir.path().join(".build/own");
        let mut moved = build_record::read(&build).unwrap().unwrap();
        moved.source = crate::build_record::BuildSource::Path {
            dir: "./old".into(),
        };
        build_record::write(&build, &moved).unwrap();
        std::fs::write(dir.path().join("slow"), "").unwrap();
        let file = ComposeFile::load(&path).unwrap();
        let own = ["own".to_string()];

        let controller = crate::shutdown::ShutdownController::default();
        let signal = controller.signal();
        let marker = dir.path().join("building");
        let began = std::time::Instant::now();
        let (rebuilt, ()) = tokio::join!(
            project.rebuild_changed(&file, &own, "update", &signal),
            async {
                while !marker.exists() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                controller.cancel();
            }
        );

        assert!(rebuilt.cancelled, "{rebuilt:?}");
        assert!(
            rebuilt.built.is_empty() && rebuilt.failed.is_empty(),
            "{rebuilt:?}"
        );
        assert!(
            began.elapsed() < Duration::from_secs(20),
            "{:?}",
            began.elapsed()
        );
        assert_eq!(alive(dir.path(), "own"), Some(running));
        assert_eq!(build_record::read(&build).unwrap().unwrap(), moved);

        // Asked once cancelled, it builds nothing at all.
        std::fs::remove_file(&marker).unwrap();
        let rebuilt = project
            .rebuild_changed(&file, &own, "update", &signal)
            .await;
        assert!(rebuilt.cancelled, "{rebuilt:?}");
        assert!(!marker.exists());
        project.shutdown().await;
    }
}
