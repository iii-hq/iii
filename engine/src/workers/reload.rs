// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Config reload machinery.
//!
//! The types here track per-worker registrations so that when a worker is
//! destroyed (on shutdown or during reload) the engine can roll back the
//! functions / triggers it wrote into global registries. Task 1 adds only the
//! data types and the scope API on `Engine`. Later tasks wire this into
//! `FunctionsRegistry::register_function` and add the full reload pipeline.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::watch;

use super::config::{EngineConfig, WorkerEntry, WorkerRegistry};
use super::registry::WorkerRegistration;
use super::traits::Worker;
use crate::engine::Engine;

/// Everything a single worker registered into engine-global state while its
/// scope was active. On destroy, these IDs are removed from the registries.
#[derive(Debug, Default, Clone)]
pub struct WorkerRegistrations {
    pub function_ids: Vec<String>,
}

/// Internal state for the currently-active `begin_worker_scope` window.
///
/// Not part of the public API -- only the `Engine` scope methods construct,
/// mutate, and consume these.
#[derive(Debug, Default)]
pub(crate) struct ScopeBuilder {
    pub worker_name: String,
    pub function_ids: Vec<String>,
}

impl ScopeBuilder {
    pub fn new(worker_name: String) -> Self {
        Self {
            worker_name,
            function_ids: Vec::new(),
        }
    }

    pub fn into_registrations(self) -> WorkerRegistrations {
        WorkerRegistrations {
            function_ids: self.function_ids,
        }
    }
}

/// A worker currently being managed by the engine. The `entry` is the
/// `WorkerEntry` that produced it -- used for diffing during reload. The
/// `shutdown_tx` is unique to this worker, allowing individual reload-time
/// stop/start without affecting other workers. `registrations` are the
/// engine-global registrations made during `register_functions`, tracked so
/// they can be removed if the worker is destroyed during reload.
pub struct RunningWorker {
    pub entry: WorkerEntry,
    pub worker: Arc<dyn Worker>,
    pub shutdown_tx: watch::Sender<bool>,
    pub registrations: WorkerRegistrations,
}

/// The result of diffing a new config entry set against the currently-running
/// set. Each entry goes into exactly one of the four buckets. `added` and
/// `changed` carry full `WorkerEntry` values (needed by dry-run validation and
/// the commit phase). `removed` and `unchanged` carry only the names.
#[derive(Debug, Default)]
pub struct ReloadDiff {
    pub added: Vec<WorkerEntry>,
    pub removed: Vec<String>,
    pub changed: Vec<WorkerEntry>,
    pub unchanged: Vec<String>,
}

/// Partitions `new` against `old` into added/removed/changed/unchanged. Pure
/// function. Equality uses `WorkerEntry::PartialEq` which compares `name`,
/// `image`, and `config` structurally.
pub fn diff_entries(old: &[WorkerEntry], new: &[WorkerEntry]) -> ReloadDiff {
    let old_map: HashMap<&str, &WorkerEntry> =
        old.iter().map(|e| (e.name.as_str(), e)).collect();
    let new_map: HashMap<&str, &WorkerEntry> =
        new.iter().map(|e| (e.name.as_str(), e)).collect();

    let mut diff = ReloadDiff::default();

    for new_entry in new {
        match old_map.get(new_entry.name.as_str()) {
            None => diff.added.push(new_entry.clone()),
            Some(old_entry) => {
                if **old_entry == *new_entry {
                    diff.unchanged.push(new_entry.name.clone());
                } else {
                    diff.changed.push(new_entry.clone());
                }
            }
        }
    }

    for old_entry in old {
        if !new_map.contains_key(old_entry.name.as_str()) {
            diff.removed.push(old_entry.name.clone());
        }
    }

    diff
}

/// Orchestrates SIGHUP-triggered config reload. Task 6 adds only the
/// read-only phases (parse + normalize); dry-run validation, commit, and the
/// full `reload()` entry point come in later tasks.
pub struct ReloadManager;

impl ReloadManager {
    /// Phases 1 + 2: parse the YAML from `path`, expand env vars, flatten the
    /// `workers` + `modules` lists, auto-inject any missing mandatory workers,
    /// and reject duplicate names. Read-only -- does not touch running state.
    ///
    /// Returns the normalized `Vec<WorkerEntry>` ready for diffing.
    pub async fn parse_and_normalize(path: &str) -> anyhow::Result<Vec<WorkerEntry>> {
        let cfg = EngineConfig::config_file(path)
            .map_err(|e| anyhow::anyhow!("reload: parse failed: {}", e))?;

        let mut entries: Vec<WorkerEntry> = Vec::new();
        entries.extend(cfg.workers);
        entries.extend(cfg.modules);

        let names: std::collections::HashSet<String> =
            entries.iter().map(|e| e.name.clone()).collect();

        for registration in inventory::iter::<WorkerRegistration> {
            if registration.mandatory && !names.contains(registration.name) {
                entries.push(WorkerEntry {
                    name: registration.name.to_string(),
                    image: None,
                    config: None,
                });
            }
        }

        // Reject duplicate worker names.
        let mut seen = std::collections::HashSet::new();
        for e in &entries {
            if !seen.insert(e.name.clone()) {
                return Err(anyhow::anyhow!(
                    "reload: duplicate worker name '{}' in new config",
                    e.name
                ));
            }
        }

        Ok(entries)
    }

    /// Phase 4: dry-run validation. Creates and initializes each worker in
    /// `diff.added` and `diff.changed`, in that order. On any failure,
    /// destroys every worker that was successfully staged before the failure
    /// and returns a `reload: validation failed` error. The running engine is
    /// never touched by this method.
    ///
    /// The caller owns the returned `Vec<StagedWorker>` and is responsible for
    /// either promoting them via the commit phase or destroying them if the
    /// pipeline aborts for another reason.
    pub async fn validate_staging(
        diff: &ReloadDiff,
        engine: Arc<Engine>,
        registry: Arc<WorkerRegistry>,
    ) -> anyhow::Result<Vec<StagedWorker>> {
        let mut staged: Vec<StagedWorker> = Vec::new();

        let to_prepare = diff.added.iter().chain(diff.changed.iter());

        for entry in to_prepare {
            let created = entry.create_worker(engine.clone(), &registry).await;

            let worker = match created {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!(
                        "reload: validation failed for '{}': {}",
                        entry.name,
                        e
                    );
                    Self::rollback_staging(staged).await;
                    return Err(anyhow::anyhow!(
                        "reload: validation failed for '{}': {}",
                        entry.name,
                        e
                    ));
                }
            };

            if let Err(e) = worker.initialize().await {
                tracing::error!(
                    "reload: validation failed for '{}': initialize: {}",
                    entry.name,
                    e
                );
                // `worker` isn't yet pushed into `staged`; destroy it directly.
                if let Err(e2) = worker.destroy().await {
                    tracing::warn!(
                        "reload: rollback destroy failed for '{}': {}",
                        entry.name,
                        e2
                    );
                }
                Self::rollback_staging(staged).await;
                return Err(anyhow::anyhow!(
                    "reload: validation failed for '{}': {}",
                    entry.name,
                    e
                ));
            }

            staged.push(StagedWorker {
                entry: entry.clone(),
                worker,
            });
        }

        Ok(staged)
    }

    async fn rollback_staging(staged: Vec<StagedWorker>) {
        for s in staged {
            if let Err(e) = s.worker.destroy().await {
                tracing::warn!(
                    "reload: rollback destroy failed for '{}': {}",
                    s.entry.name,
                    e
                );
            }
        }
    }

    /// Phase 3 post-diff guards. Today: refuse removal of mandatory workers.
    ///
    /// Normal reload flow prevents mandatory workers from appearing in
    /// `diff.removed` because `parse_and_normalize` auto-injects them. This
    /// guard is belt-and-suspenders for any caller that composes the pipeline
    /// differently (e.g. test code constructing a diff directly).
    pub fn enforce_guards(diff: &ReloadDiff) -> anyhow::Result<()> {
        let mandatory_names: std::collections::HashSet<&'static str> =
            inventory::iter::<WorkerRegistration>
                .into_iter()
                .filter(|r| r.mandatory)
                .map(|r| r.name)
                .collect();

        for name in &diff.removed {
            if mandatory_names.contains(name.as_str()) {
                let msg = format!(
                    "reload: refused to remove mandatory worker '{}'",
                    name
                );
                tracing::error!("{}", msg);
                return Err(anyhow::anyhow!(msg));
            }
        }
        Ok(())
    }
}

/// A worker that has been successfully created and initialized during Phase 4
/// but has NOT yet had `register_functions` or `start_background_tasks`
/// called. Held briefly between validation and commit, and destroyed on
/// rollback.
pub struct StagedWorker {
    pub entry: WorkerEntry,
    pub worker: Box<dyn Worker>,
}

impl ReloadManager {
    /// Phase 5: apply the validated diff to `running`. Order:
    ///
    /// 1. For each CHANGED entry: stop the old running worker (per-worker
    ///    `shutdown_tx` + `destroy` + `remove_worker_registrations`), then
    ///    promote the staged replacement.
    /// 2. For each REMOVED name: stop and drop the running worker.
    /// 3. For each ADDED entry: promote the staged new worker.
    ///
    /// Failures inside this method are logged loudly but do not roll back --
    /// Phase 4 is the safety net. A commit failure leaves the engine in a
    /// state that does not match either the old or the new config, so the
    /// caller must log clearly and, ideally, surface the inconsistency to
    /// operators. In practice, a correctly-implemented Phase 4 makes this
    /// path unreachable under normal operation.
    pub async fn commit(
        diff: &ReloadDiff,
        staged: Vec<StagedWorker>,
        engine: Arc<Engine>,
        running: &mut Vec<RunningWorker>,
    ) -> anyhow::Result<()> {
        let mut staged_by_name: HashMap<String, StagedWorker> = staged
            .into_iter()
            .map(|s| (s.entry.name.clone(), s))
            .collect();

        // 1. CHANGED: stop old, promote staged replacement
        for entry in &diff.changed {
            if let Some(idx) = running.iter().position(|rw| rw.entry.name == entry.name) {
                let old = running.swap_remove(idx);
                let _ = old.shutdown_tx.send(true);
                if let Err(e) = old.worker.destroy().await {
                    tracing::error!(
                        "reload: COMMIT FAILURE destroying changed worker '{}': {}",
                        entry.name,
                        e
                    );
                }
                engine.remove_worker_registrations(&old.registrations);
            }

            let staged = staged_by_name.remove(&entry.name).ok_or_else(|| {
                anyhow::anyhow!(
                    "reload: internal error -- changed entry '{}' missing from staged set",
                    entry.name
                )
            })?;
            let rw = Self::promote(engine.clone(), staged).await;
            running.push(rw);
        }

        // 2. REMOVED: stop and drop
        for name in &diff.removed {
            if let Some(idx) = running.iter().position(|rw| &rw.entry.name == name) {
                let removed = running.swap_remove(idx);
                let _ = removed.shutdown_tx.send(true);
                if let Err(e) = removed.worker.destroy().await {
                    tracing::error!(
                        "reload: COMMIT FAILURE destroying removed worker '{}': {}",
                        name,
                        e
                    );
                }
                engine.remove_worker_registrations(&removed.registrations);
            }
        }

        // 3. ADDED: promote staged
        for entry in &diff.added {
            let staged = staged_by_name.remove(&entry.name).ok_or_else(|| {
                anyhow::anyhow!(
                    "reload: internal error -- added entry '{}' missing from staged set",
                    entry.name
                )
            })?;
            let rw = Self::promote(engine.clone(), staged).await;
            running.push(rw);
        }

        Ok(())
    }

    /// Promote a staged worker into a running worker: open a scope, register
    /// its functions, close the scope, allocate a per-worker shutdown channel,
    /// and start its background tasks.
    ///
    /// Note on `shutdown_tx`: `promote` uses the per-worker `shutdown_tx` for
    /// BOTH the `shutdown_rx` argument and the `shutdown_tx` argument to
    /// `start_background_tasks`. In `EngineBuilder::serve()`, the second
    /// argument is the GLOBAL shutdown tx so that workers like `WorkerManager`
    /// which catch SIGTERM/Ctrl+C can unwind the whole process. The global tx
    /// is a local inside `serve()` and is not reachable from here.
    ///
    /// This is acceptable because:
    ///
    /// - `WorkerManager` is mandatory and is created exactly once during
    ///   initial build; it never travels through the reload ADDED/CHANGED
    ///   paths, so its signal-handling task is unaffected.
    /// - User workers added via reload do not typically own global shutdown
    ///   handlers; they react to their own `shutdown_rx`.
    ///
    /// Task 10 can revisit this by threading the global tx into the reload
    /// pipeline if a future worker needs to fire global shutdown after being
    /// introduced via reload.
    async fn promote(engine: Arc<Engine>, staged: StagedWorker) -> RunningWorker {
        engine.begin_worker_scope(&staged.entry.name);
        staged.worker.register_functions(engine.clone());
        let registrations = engine.end_worker_scope();

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker_arc: Arc<dyn Worker> = Arc::from(staged.worker);

        if let Err(e) = worker_arc
            .start_background_tasks(shutdown_rx, shutdown_tx.clone())
            .await
        {
            tracing::error!(
                "reload: COMMIT FAILURE starting background tasks for '{}': {}",
                staged.entry.name,
                e
            );
        }

        RunningWorker {
            entry: staged.entry,
            worker: worker_arc,
            shutdown_tx,
            registrations,
        }
    }

    /// Full SIGHUP reload pipeline. Runs phases in order:
    ///
    /// 1. **Parse & normalize**: `parse_and_normalize(path)`. On failure logs
    ///    `reload: parse failed: ...` and returns early.
    /// 2. **Diff** against the current `running` set. Logs the summary.
    /// 3. **Enforce guards** (refuse mandatory removal). On failure logs and
    ///    returns early.
    /// 4. **Validate staging**: dry-run create + initialize added/changed
    ///    workers. On failure, rollback already happened inside
    ///    `validate_staging` -- just log and return.
    /// 5. **Commit**: apply the diff. Failures here leave inconsistent state;
    ///    they are logged loudly as `reload: COMMIT FAILURE`.
    ///
    /// Default-config mode (`config_path == None`) logs
    /// `reload: ignored, running with --use-default-config` and returns.
    ///
    /// This method is called from the serve loop on every SIGHUP. Callers are
    /// responsible for serializing concurrent reload attempts (typically via a
    /// mutex wrapping the shared `running` state).
    pub async fn reload(
        config_path: Option<&str>,
        engine: Arc<Engine>,
        registry: Arc<WorkerRegistry>,
        running: &mut Vec<RunningWorker>,
    ) {
        let path = match config_path {
            Some(p) => p,
            None => {
                tracing::info!("reload: ignored, running with --use-default-config");
                return;
            }
        };

        tracing::info!("reload: SIGHUP received, reloading from {}", path);

        // Phases 1 + 2
        let new_entries = match Self::parse_and_normalize(path).await {
            Ok(entries) => entries,
            Err(e) => {
                tracing::error!("{}", e);
                return;
            }
        };

        // Phase 3
        let old_entries: Vec<WorkerEntry> =
            running.iter().map(|rw| rw.entry.clone()).collect();
        let diff = diff_entries(&old_entries, &new_entries);
        tracing::info!(
            "reload: diff +{} added, -{} removed, ~{} changed, ={} unchanged",
            diff.added.len(),
            diff.removed.len(),
            diff.changed.len(),
            diff.unchanged.len(),
        );

        if let Err(e) = Self::enforce_guards(&diff) {
            tracing::error!("{}", e);
            return;
        }

        // Phase 4
        let staged = match Self::validate_staging(&diff, engine.clone(), registry.clone()).await {
            Ok(s) => s,
            Err(_) => return, // already logged
        };

        // Phase 5
        if let Err(e) = Self::commit(&diff, staged, engine.clone(), running).await {
            tracing::error!("reload: COMMIT FAILURE: {}", e);
            return;
        }

        tracing::info!("reload: success");
    }
}
