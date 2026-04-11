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

use super::config::{EngineConfig, WorkerEntry};
use super::registry::WorkerRegistration;
use super::traits::Worker;

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
}
