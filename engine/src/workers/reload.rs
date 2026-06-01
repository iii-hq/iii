// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Config reload machinery.
//!
//! The types here track per-worker registrations so that when a worker is
//! destroyed (on shutdown or during reload) the engine can roll back the
//! functions / triggers it wrote into global registries.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::watch;

use super::config::{
    EngineConfig, WorkerEntry, WorkerRegistry, assign_instance_ids,
    runtime_worker_info_from_registration,
};
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
    pub function_ids: Vec<String>,
}

impl ScopeBuilder {
    pub fn new() -> Self {
        Self {
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
/// `changed` carry full `WorkerEntry` values (needed by the commit phase).
/// `removed` and `unchanged` carry only the names.
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
    let old_map: HashMap<&str, &WorkerEntry> = old.iter().map(|e| (e.name.as_str(), e)).collect();
    let new_map: HashMap<&str, &WorkerEntry> = new.iter().map(|e| (e.name.as_str(), e)).collect();

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

/// Orchestrates config file reload. Watches for file changes and runs the
/// full reload pipeline: parse, normalize, diff, commit.
pub struct ReloadManager;

impl ReloadManager {
    /// Parse the YAML from `path`, expand env vars, flatten the `workers` +
    /// `modules` lists, auto-inject any missing mandatory workers, and assign
    /// unique instance IDs to duplicate names. Read-only -- does not touch
    /// running state.
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

        assign_instance_ids(&mut entries);

        Ok(entries)
    }

    /// Walks `diff.unchanged` and asks each tracked worker whether its
    /// backing process is still alive. Dead ones get pulled out of
    /// `unchanged` and pushed into `changed` so the commit step will
    /// destroy + restart them.
    ///
    /// This fixes the class of bugs where the engine's `running` list
    /// believes a worker is up but its detached VM / child process has died
    /// or been reaped out of band — most commonly via
    /// `iii worker add --force`, which wipes the managed dir and rewrites
    /// config.yaml with a structurally identical entry, leaving diff with
    /// nothing to do.
    ///
    /// Returns the list of names that were promoted so the caller can log
    /// them (e.g. in the reload summary).
    pub async fn promote_dead_unchanged(
        diff: &mut ReloadDiff,
        new_entries: &[WorkerEntry],
        running: &[RunningWorker],
    ) -> Vec<String> {
        let new_by_name: HashMap<&str, &WorkerEntry> =
            new_entries.iter().map(|e| (e.name.as_str(), e)).collect();
        let running_by_name: HashMap<&str, &RunningWorker> = running
            .iter()
            .map(|rw| (rw.entry.name.as_str(), rw))
            .collect();
        let mut promoted: Vec<String> = Vec::new();
        let mut still_unchanged: Vec<String> = Vec::with_capacity(diff.unchanged.len());
        for name in diff.unchanged.drain(..) {
            let is_alive = match running_by_name.get(name.as_str()) {
                Some(rw) => rw.worker.is_alive().await,
                None => true, // no tracked worker — can't assess, leave alone
            };
            if is_alive {
                still_unchanged.push(name);
            } else if let Some(entry) = new_by_name.get(name.as_str()) {
                tracing::warn!(
                    worker = %name,
                    "reload: tracked worker is dead, promoting to CHANGED for restart"
                );
                diff.changed.push((*entry).clone());
                promoted.push(name);
            } else {
                // unchanged must appear in new_entries by construction of
                // diff_entries; this branch is a defensive fallback.
                still_unchanged.push(name);
            }
        }
        diff.unchanged = still_unchanged;
        promoted
    }

    /// Refuse removal of mandatory workers.
    pub fn enforce_guards(diff: &ReloadDiff) -> anyhow::Result<()> {
        let mandatory_names: std::collections::HashSet<&'static str> =
            inventory::iter::<WorkerRegistration>
                .into_iter()
                .filter(|r| r.mandatory)
                .map(|r| r.name)
                .collect();

        for name in &diff.removed {
            if mandatory_names.contains(name.as_str()) {
                let msg = format!("reload: refused to remove mandatory worker '{}'", name);
                tracing::error!("{}", msg);
                return Err(anyhow::anyhow!(msg));
            }
        }
        Ok(())
    }

    /// Apply the diff to `running`. For each changed/added entry, creates,
    /// initializes, and starts the worker directly — no dry-run staging.
    ///
    /// Order:
    /// 1. CHANGED: destroy old worker, create + start replacement.
    /// 2. REMOVED: destroy old worker.
    /// 3. ADDED: create + start new worker.
    pub async fn commit(
        diff: &ReloadDiff,
        engine: Arc<Engine>,
        registry: Arc<WorkerRegistry>,
        running: &mut Vec<RunningWorker>,
        global_shutdown_tx: watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        // 1. CHANGED: destroy old, start new
        for entry in &diff.changed {
            if let Some(idx) = running.iter().position(|rw| rw.entry.name == entry.name) {
                let old = running.swap_remove(idx);
                let _ = old.shutdown_tx.send(true);
                if let Err(e) = old.worker.destroy().await {
                    tracing::error!(
                        "reload: destroy failed for changed worker '{}': {}",
                        entry.name,
                        e
                    );
                }
                engine.remove_worker_registrations(&old.registrations);
                engine.remove_runtime_worker(&old.entry.name);
            }

            let rw =
                Self::start_worker(entry, engine.clone(), &registry, global_shutdown_tx.clone())
                    .await?;
            running.push(rw);
        }

        // 2. REMOVED: destroy
        for name in &diff.removed {
            if let Some(idx) = running.iter().position(|rw| &rw.entry.name == name) {
                let removed = running.swap_remove(idx);
                let _ = removed.shutdown_tx.send(true);
                if let Err(e) = removed.worker.destroy().await {
                    tracing::error!(
                        "reload: destroy failed for removed worker '{}': {}",
                        name,
                        e
                    );
                }
                engine.remove_worker_registrations(&removed.registrations);
                engine.remove_runtime_worker(&removed.entry.name);
            }
        }

        // 3. ADDED: start new
        for entry in &diff.added {
            let rw =
                Self::start_worker(entry, engine.clone(), &registry, global_shutdown_tx.clone())
                    .await?;
            running.push(rw);
        }

        Ok(())
    }

    /// Create, initialize, start background tasks, and register a worker.
    async fn start_worker(
        entry: &WorkerEntry,
        engine: Arc<Engine>,
        registry: &Arc<WorkerRegistry>,
        global_shutdown_tx: watch::Sender<bool>,
    ) -> anyhow::Result<RunningWorker> {
        let worker = entry
            .create_worker(engine.clone(), registry)
            .await
            .map_err(|e| {
                anyhow::anyhow!("reload: failed to create worker '{}': {}", entry.name, e)
            })?;

        worker.initialize().await.map_err(|e| {
            anyhow::anyhow!(
                "reload: failed to initialize worker '{}': {}",
                entry.name,
                e
            )
        })?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker_arc: Arc<dyn Worker> = Arc::from(worker);

        worker_arc
            .start_background_tasks(shutdown_rx, global_shutdown_tx)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "reload: failed to start background tasks for '{}': {}",
                    entry.name,
                    e
                )
            })?;

        engine.begin_worker_scope(&entry.name);
        worker_arc.register_functions(engine.clone());
        let registrations = engine.end_worker_scope();

        if let Some(runtime_worker) =
            runtime_worker_info_from_registration(entry, worker_arc.as_ref(), &registrations)
        {
            engine.upsert_runtime_worker(runtime_worker);
        }

        Ok(RunningWorker {
            entry: entry.clone(),
            worker: worker_arc,
            shutdown_tx,
            registrations,
        })
    }

    /// Full config reload pipeline:
    ///
    /// 1. **Parse & normalize** the config file.
    /// 2. **Diff** against the current `running` set.
    /// 3. **Enforce guards** (refuse mandatory removal).
    /// 4. **Commit** — destroy old workers, create + start new ones.
    ///
    /// On any failure the error is returned so `serve()` can exit the process.
    pub async fn reload(
        config_path: Option<&str>,
        engine: Arc<Engine>,
        registry: Arc<WorkerRegistry>,
        running: &mut Vec<RunningWorker>,
        global_shutdown_tx: watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        let path = match config_path {
            Some(p) => p,
            None => {
                tracing::info!("reload: ignored, running with --use-default-config");
                return Ok(());
            }
        };

        tracing::info!("reload: config changed, reloading from {}", path);

        let new_entries = Self::parse_and_normalize(path).await.map_err(|e| {
            tracing::error!("reload: FATAL: {}", e);
            e
        })?;

        let old_entries: Vec<WorkerEntry> = running.iter().map(|rw| rw.entry.clone()).collect();
        let mut diff = diff_entries(&old_entries, &new_entries);

        let promoted = Self::promote_dead_unchanged(&mut diff, &new_entries, running).await;

        tracing::info!(
            "reload: diff +{} added, -{} removed, ~{} changed ({} revived), ={} unchanged",
            diff.added.len(),
            diff.removed.len(),
            diff.changed.len(),
            promoted.len(),
            diff.unchanged.len(),
        );

        Self::enforce_guards(&diff).map_err(|e| {
            tracing::error!("reload: FATAL: {}", e);
            e
        })?;

        Self::commit(&diff, engine.clone(), registry, running, global_shutdown_tx)
            .await
            .map_err(|e| {
                tracing::error!("reload: FATAL: {}", e);
                e
            })?;

        tracing::info!("reload: success");
        Ok(())
    }
}
