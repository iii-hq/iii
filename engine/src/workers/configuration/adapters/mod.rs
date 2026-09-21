// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod bridge;
pub mod fs;

use async_trait::async_trait;
use serde_json::Value;

use crate::workers::configuration::structs::{
    ConfigurationEntry, ConfigurationMigrateResult, EnsureAction,
};

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

/// How the configuration store must implement `configuration::ensure` for a
/// given adapter. The store's `write_lock` only serializes the LOCAL cache, so
/// this tells the store whether it may decide seed-vs-preserve itself or must
/// hand the decision to the authoritative store behind the adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureSupport {
    /// The adapter's backing store IS the authority the local cache mirrors
    /// (the `fs` adapter). The store may make the seed-vs-preserve decision
    /// itself under `write_lock` and persist through `register`, because the
    /// local cache is authoritative for this adapter.
    Local,
    /// The adapter delegates to a remote authority (the `bridge`). The store
    /// MUST forward the ORIGINAL candidate to [`ConfigurationAdapter::ensure`]
    /// so the seed-vs-preserve decision is made at the authoritative engine,
    /// never against the possibly stale local cache.
    Delegated,
}

/// Seed candidate handed to an adapter that performs an authoritative
/// `configuration::ensure` itself. Carries the ORIGINAL candidate exactly as
/// the caller supplied it — never a value read back from the local cache — so
/// the seed-vs-preserve decision happens where the value actually lives.
#[derive(Debug, Clone)]
pub struct EnsureCandidate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub schema: Value,
    /// Original seed candidate, verbatim from `configuration::ensure`.
    pub candidate: Option<Value>,
    pub metadata: Option<Value>,
}

/// Outcome of an adapter-authoritative ensure: the authoritative store's
/// decision plus the entry as it now stands there.
#[derive(Debug, Clone)]
pub struct AdapterEnsureOutcome {
    pub action: EnsureAction,
    pub entry: ConfigurationEntry,
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
impl ExternalChange {
    /// The configuration id this change concerns, regardless of variant.
    pub fn id(&self) -> &str {
        match self {
            ExternalChange::Registered(entry)
            | ExternalChange::Updated { entry, .. }
            | ExternalChange::Deleted { entry } => &entry.id,
        }
    }

    /// The value this change would install, or `None` for a delete. The store
    /// uses it to decide whether a queued snapshot still matches the
    /// authoritative adapter state before fanning the change out.
    pub fn value(&self) -> Option<&Value> {
        match self {
            ExternalChange::Registered(entry) | ExternalChange::Updated { entry, .. } => {
                Some(&entry.value)
            }
            ExternalChange::Deleted { .. } => None,
        }
    }
}

/// Channel sender the worker hands to adapters that surface external changes.
pub type ExternalChangeSender = tokio::sync::mpsc::UnboundedSender<ExternalChange>;

#[async_trait]
pub trait ConfigurationAdapter: Send + Sync {
    /// Insert or replace an entry. `initial_value` already fills `entry.value`
    /// when supplied — adapters should NOT inspect it separately.
    async fn register(&self, entry: ConfigurationEntry) -> anyhow::Result<RegisterOutcome>;

    /// Declares how the store must implement `configuration::ensure` for this
    /// adapter. The default is [`EnsureSupport::Delegated`], paired with the
    /// fail-closed default [`ConfigurationAdapter::ensure`] below: a custom
    /// adapter whose real store the local `write_lock` cannot guard must not
    /// have the store silently decide seed-vs-preserve against a possibly stale
    /// cache. An adapter whose local cache IS authoritative (the `fs` adapter)
    /// overrides this to [`EnsureSupport::Local`].
    fn ensure_support(&self) -> EnsureSupport {
        EnsureSupport::Delegated
    }

    /// Atomically seed-if-absent at the adapter's authoritative store, using
    /// the ORIGINAL candidate (never a value read from the local cache). Only
    /// invoked when [`ConfigurationAdapter::ensure_support`] returns
    /// [`EnsureSupport::Delegated`]. The default fails closed so a `Delegated`
    /// adapter that has not implemented atomic ensure surfaces a clear error
    /// instead of silently falling back to an unsafe read-then-register seed.
    async fn ensure(&self, _candidate: EnsureCandidate) -> anyhow::Result<AdapterEnsureOutcome> {
        anyhow::bail!(
            "this configuration adapter does not support atomic configuration::ensure; \
             refusing an unsafe read-then-register fallback"
        )
    }

    /// Move at the authoritative store, preserving a pre-existing destination.
    /// Implementations must preserve raw values and metadata, serialize with other
    /// mutations, and keep their cache recoverable on failure. No copy/delete fallback.
    async fn migrate(
        &self,
        _from_id: &str,
        _to_id: &str,
    ) -> anyhow::Result<ConfigurationMigrateResult> {
        anyhow::bail!(
            "this configuration adapter does not support configuration::migrate; refusing an unsafe copy/delete fallback"
        )
    }

    /// Move a legacy entry even if the destination exists. Implementations
    /// must back up the replaced destination and fail closed if unsupported.
    async fn migrate_replace(
        &self,
        _from_id: &str,
        _to_id: &str,
    ) -> anyhow::Result<ConfigurationMigrateResult> {
        anyhow::bail!("adapter does not support configuration::migrate-replace")
    }

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
