// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod bridge;
pub mod fs;

use async_trait::async_trait;
use serde_json::Value;

use crate::workers::configuration::structs::ConfigurationEntry;

/// Persistent change report returned by `register`.
///
/// `Created` means the id had no prior entry; `Replaced` means the worker
/// updated metadata/schema (and possibly value, when `initial_value` was
/// provided) of an existing id.
#[derive(Debug, Clone)]
pub struct RegisterOutcome {
    pub kind: RegisterKind,
    pub entry: ConfigurationEntry,
    pub old_value: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterKind {
    Created,
    Replaced,
}

#[derive(Debug, Clone)]
pub struct SetOutcome {
    pub entry: ConfigurationEntry,
    pub old_value: Option<Value>,
}

/// External-edit report surfaced by adapters that watch their backing
/// store (e.g. the `fs` adapter's `notify` watcher). Drives the worker's
/// trigger fan-out for changes that did not originate from a local
/// `configuration::*` call.
#[derive(Debug, Clone)]
pub enum ExternalChange {
    Registered(ConfigurationEntry),
    Updated {
        entry: ConfigurationEntry,
        old_value: Option<Value>,
    },
    Deleted {
        entry: ConfigurationEntry,
    },
}

/// Channel sender the worker hands to adapters that surface external changes.
pub type ExternalChangeSender = tokio::sync::mpsc::UnboundedSender<ExternalChange>;

#[async_trait]
pub trait ConfigurationAdapter: Send + Sync {
    /// Insert or replace an entry. `initial_value` already fills `entry.value`
    /// when supplied — adapters should NOT inspect it separately.
    async fn register(&self, entry: ConfigurationEntry) -> anyhow::Result<RegisterOutcome>;

    /// Replace the value of an existing entry. Returns `None` from `get`
    /// if the id is unknown — `set` itself does not implicitly create.
    async fn set(&self, id: &str, value: Value) -> anyhow::Result<SetOutcome>;

    /// Return a single entry, or `None` if absent.
    async fn get(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>>;

    /// Remove an entry. Returns the removed entry when one was present.
    async fn delete(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>>;

    /// Return every stored entry, deterministic order is the adapter's
    /// responsibility (the worker re-sorts by id before returning to callers).
    async fn list(&self) -> anyhow::Result<Vec<ConfigurationEntry>>;

    /// Wire a sender that receives change events the adapter detects on its
    /// own (file edits, remote bridge events, etc.). Default no-op for adapters
    /// that have no out-of-band edit path.
    async fn watch(&self, _sender: ExternalChangeSender) -> anyhow::Result<()> {
        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()>;
}
