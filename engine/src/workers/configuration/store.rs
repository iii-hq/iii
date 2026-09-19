// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! In-memory cache + schema validation layer that sits between the
//! `configuration::*` engine functions and the on-disk / remote adapter.
//!
//! Loading is lazy: the cache stays empty until either `register` or
//! `prime_from_adapter` populates it. Reads check the cache first and fall
//! back to the adapter; writes update both atomically.

use std::collections::HashMap;
use std::env;
use std::sync::{Arc, LazyLock};

use jsonschema::Validator;
use regex::Regex;
use serde_json::{Map, Value};
use tokio::sync::{Mutex as TokioMutex, RwLock};

use crate::workers::configuration::adapters::{
    AdapterEnsureOutcome, ConfigurationAdapter, EnsureCandidate, EnsureSupport, ExternalChange,
    RegisterKind, RegisterOutcome, SetOutcome,
};
use crate::workers::configuration::structs::{
    ConfigurationEntry, ConfigurationSchemaView, EnsureAction,
};

/// Regex matching a single `${VAR}` / `${VAR:default}` reference. The class
/// `[^}:]+` / `[^}]*` mirrors `EngineConfig::expand_env_vars`
/// (`engine/src/workers/config.rs`) and the console's `env-template.ts`
/// parser, so all three readers agree on the grammar.
static ENV_VAR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{([^}:]+)(?::([^}]*))?\}").unwrap());

/// Matches a string that is a single whole `${...}` placeholder (optionally
/// padded by whitespace) and nothing else. Only such "lone" placeholders get
/// scalar type-coercion after substitution — mirroring how an unquoted YAML
/// scalar (`port: ${HTTP_PORT:3111}`) used to infer its type back when
/// `config.yaml` expanded env vars on raw text *before* parsing. Mixed /
/// embedded templates (surrounding text or multiple placeholders) stay strings.
static LONE_PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\$\{[^}]*\}\s*$").unwrap());

/// Non-panicking env-var substitution for one string. Mirrors the CLI's
/// `expand_env_vars` (`crates/iii-worker/src/cli/config_file.rs`): on a missing
/// var with no default, the var name is recorded in `missing` and the literal
/// `${VAR}` is left in place so partial output stays usable. The engine-only
/// `__III_ENGINE_VERSION__` sentinel default is honored (config.rs parity).
///
/// Unlike `EngineConfig::expand_env_vars` this NEVER panics — the configuration
/// worker's read/boot paths must surface a missing var as a loggable error, not
/// brick the engine.
fn expand_leaf(s: &str, missing: &mut Vec<String>) -> String {
    ENV_VAR_RE
        .replace_all(s, |caps: &regex::Captures| {
            let var_name = &caps[1];
            let default_value = caps.get(2).map(|m| m.as_str());
            match env::var(var_name) {
                Ok(value) => value,
                Err(_) => match default_value {
                    Some("__III_ENGINE_VERSION__") => env!("CARGO_PKG_VERSION").to_string(),
                    Some(default) => default.to_string(),
                    None => {
                        missing.push(var_name.to_string());
                        caps[0].to_string()
                    }
                },
            }
        })
        .to_string()
}

/// Expand one string leaf, coercing scalar types when the original is a lone
/// `${...}` placeholder. The substituted text of a lone placeholder is
/// re-parsed as a YAML 1.2 scalar (`serde_yaml`) so `"8080"`→`8080`,
/// `"true"`→`true`, while bare words / `007` / `on`/`off` stay strings — the
/// same type inference unquoted YAML applied to the legacy `config.yaml` reader.
///
/// IMPORTANT: keep this coercion in lock-step with the console's
/// `coerceScalar` in
/// `workers/console/web/.../schema-form/validate.ts`, or UI and engine will
/// disagree about which env-driven values are valid.
fn expand_string(s: &str, missing: &mut Vec<String>) -> Value {
    let substituted = expand_leaf(s, missing);
    if LONE_PLACEHOLDER.is_match(s) {
        let trimmed = substituted.trim();
        // `${X:}` (empty default) keeps an empty string; YAML would read "" as null.
        if trimmed.is_empty() {
            return Value::String(substituted);
        }
        if let Ok(parsed) = serde_yaml::from_str::<Value>(trimmed) {
            return parsed;
        }
    }
    Value::String(substituted)
}

/// Walk a JSON value, replacing every string leaf with its env-expanded +
/// type-coerced form (`${VAR:default}` → process env or default). Maps and
/// arrays are walked recursively; non-string scalars pass through unchanged.
///
/// Returns the expanded value plus the list of `${VAR}` references that had no
/// env value and no default (deduplicated by occurrence order). A non-empty
/// list means the value cannot be fully evaluated: read/boot callers must log
/// an ERROR and skip loading rather than handing back a value still carrying
/// literal `${VAR}` text.
pub fn expand_value(v: &Value) -> (Value, Vec<String>) {
    let mut missing: Vec<String> = Vec::new();
    let expanded = expand_value_inner(v, &mut missing);
    (expanded, missing)
}

fn expand_value_inner(v: &Value, missing: &mut Vec<String>) -> Value {
    match v {
        Value::String(s) => expand_string(s, missing),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|i| expand_value_inner(i, missing))
                .collect(),
        ),
        Value::Object(map) => {
            let mut out: Map<String, Value> = Map::with_capacity(map.len());
            for (k, val) in map {
                out.insert(k.clone(), expand_value_inner(val, missing));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Validate `value` against `schema`. Returns a list of human-readable
/// error strings; an empty list means the value is valid.
pub fn validate_against_schema(value: &Value, schema: &Value) -> Result<(), Vec<String>> {
    let validator = match Validator::new(schema) {
        Ok(v) => v,
        Err(err) => {
            return Err(vec![format!("invalid JSON Schema: {}", err)]);
        }
    };
    let errors: Vec<String> = validator
        .iter_errors(value)
        .map(|e| e.to_string())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("configuration '{0}' is not registered; call configuration::register first")]
    NotRegistered(String),
    #[error("invalid configuration id '{0}': must match [a-z0-9_-]{{1,64}}")]
    InvalidId(String),
    #[error("schema validation failed: {0}")]
    SchemaInvalid(String),
    #[error(
        "configuration '{0}' has no schema available yet; the owning worker must register before values can be set"
    )]
    SchemaUnavailable(String),
    #[error(transparent)]
    Adapter(#[from] anyhow::Error),
}

/// Outcome of [`ConfigurationStore::ensure`]. `action` reports what happened to
/// the stored value. `register_kind` is `Some` when THIS store owns the event
/// fan-out (a `Local` adapter, e.g. `fs`): it mirrors the created-vs-replaced
/// signal so the worker picks the right `configuration:*` event. It is `None`
/// for a `Delegated` adapter (e.g. the bridge): the authoritative remote engine
/// emits its own `configuration:*` event, relayed to local subscribers via the
/// bridge watcher, so the handler must NOT double-fire it.
#[derive(Debug, Clone)]
pub struct EnsureOutcome {
    pub action: EnsureAction,
    pub register_kind: Option<RegisterKind>,
    pub entry: ConfigurationEntry,
    pub old_value: Option<Value>,
}

/// Whether a reconciled [`ExternalChange`] should be fanned out to trigger
/// subscribers, returned by [`ConfigurationStore::apply_external`].
///
/// External changes reach the store through a channel, so a snapshot the
/// watcher captured can be applied only AFTER a newer local mutation commits.
/// `apply_external` reconciles the cache against the authoritative adapter
/// instead of trusting the queued snapshot, and reports here whether the change
/// still reflects the live state (and must fan out) or was superseded (and must
/// stay silent so no stale `configuration:*` event is emitted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalApply {
    /// The change still matches the authoritative store: the cache was updated
    /// and the worker must fan out the corresponding `configuration:*` event.
    Fanout,
    /// The change was superseded by a newer local mutation, or the authoritative
    /// adapter read failed. The cache was left reflecting the current
    /// authoritative state and NO event must be fanned out: the local mutation
    /// already emitted its own, or the queued value never became live.
    Suppressed,
}

pub struct ConfigurationStore {
    adapter: Arc<dyn ConfigurationAdapter>,
    /// Authoritative in-memory cache. Source of truth for `get`/`list`/`schema`.
    /// Populated lazily from the adapter and kept in sync on every mutation.
    entries: Arc<RwLock<HashMap<String, ConfigurationEntry>>>,
    /// Serializes every mutating operation (register/ensure/set/delete) and
    /// cache reconciliation (apply_external/prime_from_adapter) so a
    /// read-prior -> adapter-write -> cache-update sequence is linearizable and
    /// cannot be interleaved by a concurrent mutation that would overwrite it
    /// with a stale value. Held across the adapter await; the `entries` lock is
    /// only taken for the short cache reads/writes inside, never across the
    /// await, so reads (get/list/schema) never block on this lock and no
    /// lock-order deadlock is possible. Scope: one engine process / store.
    write_lock: TokioMutex<()>,
}

impl ConfigurationStore {
    /// Create an empty cache and mutation lock for one authoritative adapter.
    pub fn new(adapter: Arc<dyn ConfigurationAdapter>) -> Self {
        Self {
            adapter,
            entries: Arc::new(RwLock::new(HashMap::new())),
            write_lock: TokioMutex::new(()),
        }
    }

    pub fn adapter(&self) -> &Arc<dyn ConfigurationAdapter> {
        &self.adapter
    }

    /// Pull every entry the adapter knows about into the cache. Called once
    /// during worker `initialize()`.
    pub async fn prime_from_adapter(&self) -> anyhow::Result<()> {
        let _write = self.write_lock.lock().await;
        let entries = self.adapter.list().await?;
        let mut cache = self.entries.write().await;
        cache.clear();
        for entry in entries {
            cache.insert(entry.id.clone(), entry);
        }
        Ok(())
    }

    /// Refresh schema and metadata, preserving the current value unless an explicit
    /// initial value requests replacement. Serialize storage and cache mutations.
    pub async fn register(
        &self,
        id: String,
        name: String,
        description: String,
        schema: Value,
        initial_value: Option<Value>,
        metadata: Option<Value>,
    ) -> Result<RegisterOutcome, StoreError> {
        Self::validate_id(&id)?;

        // Linearize with every other mutation on this store: hold the write
        // lock across the read-prior -> adapter-write -> cache-update sequence
        // so a concurrent set/register/ensure cannot slip in and be overwritten
        // by the value we read before the adapter round-trip. The `entries` lock
        // is only taken briefly inside, never across the adapter await.
        let _write = self.write_lock.lock().await;

        // Determine the value being installed and whether to validate it.
        // Existing entries keep their value unless `initial_value` is supplied.
        // New entries default to `Value::Null`.
        let prior = self.entries.read().await.get(&id).cloned();
        let (value, validate) = match (initial_value, prior.as_ref()) {
            // A caller-supplied value is always validated against the schema.
            (Some(v), _) => (v, true),
            // Re-registration without a new value reuses the stored value as-is
            // and does NOT re-validate it against the (possibly newly-tightened)
            // schema. A schema refresh must always go through so the console and
            // `set` see the current schema; `set` enforces it on the next write.
            // Re-validating here would let a now-invalid stored value — e.g. an
            // older seed persisted before the schema tightened — silently block
            // every future schema update (the worker swallows the register error
            // at boot, so the console would keep rendering the stale schema).
            (None, Some(existing)) => (existing.value.clone(), false),
            // Brand-new entry with no seed: the implicit `Null` placeholder is
            // never validated (the schema may legitimately disallow null).
            (None, None) => (Value::Null, false),
        };

        // Validate the APPLIED (env-expanded + type-coerced) value, never the
        // raw template: `${HTTP_PORT:3111}` must validate as the integer 3111,
        // not be rejected as a string. A value that can't be fully evaluated
        // yet (a var with no env value and no default) is stored raw and
        // re-validated later at read time, once the var is present.
        if validate {
            let (applied, missing) = expand_value(&value);
            if missing.is_empty()
                && let Err(errs) = validate_against_schema(&applied, &schema)
            {
                return Err(StoreError::SchemaInvalid(errs.join("; ")));
            }
        }

        let entry = ConfigurationEntry {
            id: id.clone(),
            name,
            description,
            schema,
            value,
            metadata,
        };
        let outcome = self.adapter.register(entry.clone()).await?;
        self.entries.write().await.insert(id, outcome.entry.clone());
        Ok(outcome)
    }

    /// Idempotent seed-if-absent. Unlike [`register`], which overwrites the
    /// stored value whenever `initial_value` is supplied, `ensure` writes the
    /// `candidate` seed ONLY when there is no non-null value stored yet. A value
    /// that already exists (including `false`, `0`, or `""` — these are real
    /// values, not "empty") is preserved verbatim, bytes untouched, and the
    /// candidate is neither applied nor validated. Name, description, schema,
    /// and metadata are always refreshed, exactly like a metadata-only
    /// `register`.
    ///
    /// The seed-vs-preserve decision must be made against the AUTHORITATIVE
    /// store, so `ensure` dispatches on the adapter's
    /// [`ConfigurationAdapter::ensure_support`]:
    /// - `Local` (e.g. `fs`): the local cache is authoritative, so the decision
    ///   is made here under `write_lock` (see [`ensure_local`]).
    /// - `Delegated` (e.g. the bridge): the authority is remote and the local
    ///   `write_lock` cannot guard it, so the ORIGINAL candidate is forwarded to
    ///   the adapter and the decision is made there (see [`ensure_delegated`]).
    ///   The store never decides against a possibly stale local cache and never
    ///   falls back to a legacy register.
    ///
    /// [`ensure_local`]: ConfigurationStore::ensure_local
    /// [`ensure_delegated`]: ConfigurationStore::ensure_delegated
    pub async fn ensure(
        &self,
        id: String,
        name: String,
        description: String,
        schema: Value,
        candidate: Option<Value>,
        metadata: Option<Value>,
    ) -> Result<EnsureOutcome, StoreError> {
        Self::validate_id(&id)?;
        match self.adapter.ensure_support() {
            EnsureSupport::Local => {
                self.ensure_local(id, name, description, schema, candidate, metadata)
                    .await
            }
            EnsureSupport::Delegated => {
                self.ensure_delegated(id, name, description, schema, candidate, metadata)
                    .await
            }
        }
    }

    /// `ensure` for a `Local` adapter whose on-disk / in-process store is the
    /// authority the local cache mirrors. The read-prior -> decide ->
    /// adapter-write -> cache-update sequence runs under `write_lock`, so two
    /// concurrent `ensure` calls with different seeds resolve to one winner
    /// (first seed installed, second preserved) with no stale overwrite, and an
    /// `ensure` racing a `set` never clobbers the newer value.
    async fn ensure_local(
        &self,
        id: String,
        name: String,
        description: String,
        schema: Value,
        candidate: Option<Value>,
        metadata: Option<Value>,
    ) -> Result<EnsureOutcome, StoreError> {
        let _write = self.write_lock.lock().await;

        let prior = self.entries.read().await.get(&id).cloned();
        let has_stored_value = prior.as_ref().is_some_and(|p| !p.value.is_null());

        // Decide the value to install and whether it needs validation.
        let (value, action, validate) = if has_stored_value {
            // A real value is already stored: preserve its raw bytes and ignore
            // the candidate seed (an unused seed is never validated).
            (
                prior
                    .as_ref()
                    .expect("checked non-null above")
                    .value
                    .clone(),
                EnsureAction::Preserved,
                false,
            )
        } else {
            match candidate {
                // Seed applied: validate it against the schema like `register`.
                Some(v) => (v, EnsureAction::Seeded, true),
                // No seed and no stored value: create/refresh with a null value.
                None => (Value::Null, EnsureAction::Registered, false),
            }
        };

        // Validate the APPLIED (env-expanded + coerced) seed only. A candidate
        // that cannot be fully evaluated yet (a `${VAR}` with no env value and
        // no default) is stored raw and re-validated at read time, matching
        // `register`.
        if validate {
            let (applied, missing) = expand_value(&value);
            if missing.is_empty()
                && let Err(errs) = validate_against_schema(&applied, &schema)
            {
                return Err(StoreError::SchemaInvalid(errs.join("; ")));
            }
        }

        let entry = ConfigurationEntry {
            id: id.clone(),
            name,
            description,
            schema,
            value,
            metadata,
        };
        let outcome = self.adapter.register(entry).await?;
        self.entries.write().await.insert(id, outcome.entry.clone());

        Ok(EnsureOutcome {
            action,
            register_kind: Some(outcome.kind),
            entry: outcome.entry,
            old_value: outcome.old_value,
        })
    }

    /// `ensure` for a `Delegated` adapter (e.g. the bridge) whose authoritative
    /// store lives elsewhere. The local `write_lock` cannot guard that store, so
    /// we do NOT decide seed-vs-preserve here and we do NOT read the local cache
    /// for the value. Instead we forward the ORIGINAL candidate to the adapter,
    /// let the authoritative store decide, and reconcile our cache from the
    /// returned entry. The `write_lock` is still held so the cache update stays
    /// linearized with local reads and any concurrent `apply_external`. Any
    /// adapter error (including an old remote engine with no
    /// `configuration::ensure`) propagates as `StoreError::Adapter`; there is no
    /// fallback to a legacy read-then-register seed.
    async fn ensure_delegated(
        &self,
        id: String,
        name: String,
        description: String,
        schema: Value,
        candidate: Option<Value>,
        metadata: Option<Value>,
    ) -> Result<EnsureOutcome, StoreError> {
        let _write = self.write_lock.lock().await;
        let outcome: AdapterEnsureOutcome = self
            .adapter
            .ensure(EnsureCandidate {
                id: id.clone(),
                name,
                description,
                schema,
                candidate,
                metadata,
            })
            .await?;
        self.entries.write().await.insert(id, outcome.entry.clone());
        Ok(EnsureOutcome {
            action: outcome.action,
            // The authoritative store emits its own `configuration:*` event,
            // relayed to local subscribers via the bridge watcher; the handler
            // must NOT double-fire, so no local register_kind is reported.
            register_kind: None,
            entry: outcome.entry,
            old_value: None,
        })
    }

    /// Validate the applied value and persist its raw template without losing a
    /// concurrent registration or leaving the cache ahead of failed storage.
    pub async fn set(&self, id: &str, value: Value) -> Result<SetOutcome, StoreError> {
        Self::validate_id(id)?;
        let _write = self.write_lock.lock().await;

        let entry = self.entries.read().await.get(id).cloned();
        let entry = match entry {
            Some(e) => e,
            None => return Err(StoreError::NotRegistered(id.to_string())),
        };

        // No schema cached yet (the owning worker hasn't re-registered this
        // session) — reject rather than validate against a null schema.
        if entry.schema.is_null() {
            return Err(StoreError::SchemaUnavailable(id.to_string()));
        }
        // Validate the APPLIED value (see `register`); the raw template is what
        // gets stored, so it can be re-evaluated whenever the env changes.
        let (applied, missing) = expand_value(&value);
        if missing.is_empty()
            && let Err(errs) = validate_against_schema(&applied, &entry.schema)
        {
            return Err(StoreError::SchemaInvalid(errs.join("; ")));
        }

        let outcome = self.adapter.set(id, value).await?;
        self.entries
            .write()
            .await
            .insert(id.to_string(), outcome.entry.clone());
        Ok(outcome)
    }

    /// Return the last committed raw entry without holding the mutation lock across caller work.
    pub async fn get(&self, id: &str) -> Option<ConfigurationEntry> {
        self.entries.read().await.get(id).cloned()
    }

    /// Delete from storage before removing the cache entry under the mutation lock.
    pub async fn delete(&self, id: &str) -> Result<Option<ConfigurationEntry>, StoreError> {
        let _write = self.write_lock.lock().await;
        let removed = self.adapter.delete(id).await?;
        if removed.is_some() {
            self.entries.write().await.remove(id);
        }
        Ok(removed)
    }

    pub async fn list(&self) -> Vec<ConfigurationSchemaView> {
        let cache = self.entries.read().await;
        let mut views: Vec<ConfigurationSchemaView> =
            cache.values().map(ConfigurationSchemaView::from).collect();
        views.sort_by(|a, b| a.id.cmp(&b.id));
        views
    }

    /// Expose schema metadata without returning the entry's potentially sensitive configuration value.
    pub async fn schema_view(&self, id: &str) -> Option<ConfigurationSchemaView> {
        self.entries
            .read()
            .await
            .get(id)
            .map(ConfigurationSchemaView::from)
    }

    /// Reconcile an external change (file edit, remote bridge event) into the
    /// cache and report whether it should be fanned out.
    ///
    /// The change was captured by an adapter watcher and delivered through a
    /// channel, so by the time it reaches this method a newer local
    /// register/ensure/set (or delete) may already have committed under the same
    /// `write_lock`. Trusting the queued snapshot verbatim would then revert the
    /// cache to a stale value (a later `get` returns stale, a later `ensure`
    /// could clobber) or drop a locally recreated entry.
    ///
    /// For a `Local` adapter (whose in-process store IS the authority the cache
    /// mirrors, e.g. `fs`) this re-reads the adapter's CURRENT state under the
    /// `write_lock` and reconciles the cache to it, ignoring a snapshot a newer
    /// local mutation superseded. A failed authoritative read keeps the
    /// last-known cache untouched -- never a spurious delete. For a `Delegated`
    /// adapter (whose authority is a remote engine reached over the bridge,
    /// whose `get` is a remote RPC that cannot distinguish NOT_FOUND from a
    /// transient failure) the ordered relayed event stream IS the authority, so
    /// the snapshot is applied directly.
    ///
    /// Returns [`ExternalApply::Fanout`] when the change still reflects the
    /// authoritative store and its `configuration:*` event should be emitted, or
    /// [`ExternalApply::Suppressed`] when it was superseded and must stay silent.
    pub async fn apply_external(&self, change: &ExternalChange) -> ExternalApply {
        let _write = self.write_lock.lock().await;
        match self.adapter.ensure_support() {
            EnsureSupport::Local => self.apply_external_reconciled(change).await,
            EnsureSupport::Delegated => self.apply_external_snapshot(change).await,
        }
    }

    /// Apply a queued snapshot verbatim (the `Delegated` path). The remote
    /// authority emits ordered events relayed over a single bridge connection,
    /// so the value in the event is authoritative; re-querying the remote would
    /// add an RPC that cannot tell a delete from a transient failure. Assumes
    /// the caller holds `write_lock`.
    async fn apply_external_snapshot(&self, change: &ExternalChange) -> ExternalApply {
        let mut cache = self.entries.write().await;
        match change {
            ExternalChange::Registered(entry) | ExternalChange::Updated { entry, .. } => {
                cache.insert(entry.id.clone(), entry.clone());
            }
            ExternalChange::Deleted { entry } => {
                cache.remove(&entry.id);
            }
        }
        ExternalApply::Fanout
    }

    /// Reconcile a queued change against the authoritative adapter (the `Local`
    /// path). Re-reads the adapter's current state and updates the cache to it,
    /// so a snapshot a newer local mutation superseded neither reverts a value
    /// nor drops a recreated entry. Assumes the caller holds `write_lock`; the
    /// `entries` lock is taken only after the adapter read completes, never
    /// across it.
    async fn apply_external_reconciled(&self, change: &ExternalChange) -> ExternalApply {
        let id = change.id();
        let current = match self.adapter.get(id).await {
            Ok(current) => current,
            Err(err) => {
                // A failed authoritative read must never be turned into a
                // spurious delete or revert: keep the last-known cache.
                tracing::warn!(
                    configuration_id = %id,
                    error = %err,
                    "Failed to reconcile external configuration change against the adapter; keeping the cached value"
                );
                return ExternalApply::Suppressed;
            }
        };

        let mut cache = self.entries.write().await;
        match change {
            ExternalChange::Registered(_) | ExternalChange::Updated { .. } => match current {
                // The entry still exists at the authority: adopt its current
                // value. Fan out only when it still matches the queued snapshot;
                // otherwise a newer local write already emitted its own event.
                Some(current) => {
                    let still_current = change.value() == Some(&current.value);
                    cache.insert(id.to_string(), current);
                    if still_current {
                        ExternalApply::Fanout
                    } else {
                        ExternalApply::Suppressed
                    }
                }
                // The entry was deleted at the authority after this snapshot was
                // captured: reconcile (remove) and let the delete's own event
                // drive the fan-out.
                None => {
                    cache.remove(id);
                    ExternalApply::Suppressed
                }
            },
            ExternalChange::Deleted { .. } => match current {
                // The delete still reflects the authority: apply and fan out.
                None => {
                    cache.remove(id);
                    ExternalApply::Fanout
                }
                // A local register/ensure recreated the entry before this delete
                // landed: keep the live value, drop the stale delete.
                Some(current) => {
                    cache.insert(id.to_string(), current);
                    ExternalApply::Suppressed
                }
            },
        }
    }

    fn validate_id(id: &str) -> Result<(), StoreError> {
        if id.is_empty() || id.len() > 64 {
            return Err(StoreError::InvalidId(id.to_string()));
        }
        if !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(StoreError::InvalidId(id.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn expand_value_replaces_env_var_in_string() {
        unsafe {
            std::env::set_var("CFG_TEST_HOST", "db.local");
        }
        let input = json!({ "host": "${CFG_TEST_HOST:fallback}", "port": 5432 });
        let (expanded, missing) = expand_value(&input);
        assert!(missing.is_empty());
        assert_eq!(expanded["host"], "db.local");
        assert_eq!(expanded["port"], 5432);
    }

    #[test]
    fn expand_value_uses_default_when_var_missing() {
        unsafe {
            std::env::remove_var("CFG_TEST_MISSING");
        }
        let input = json!({ "url": "${CFG_TEST_MISSING:http://default}" });
        assert_eq!(expand_value(&input).0["url"], "http://default");
    }

    #[test]
    fn expand_value_walks_arrays_and_nested_objects() {
        unsafe {
            std::env::set_var("CFG_TEST_NAME", "alice");
        }
        let input = json!({
            "users": [
                { "name": "${CFG_TEST_NAME:?}" },
                { "name": "static" }
            ]
        });
        let (out, _missing) = expand_value(&input);
        assert_eq!(out["users"][0]["name"], "alice");
        assert_eq!(out["users"][1]["name"], "static");
    }

    #[test]
    fn expand_value_passes_non_string_scalars_through() {
        let input = json!({ "n": 42, "b": true, "nil": null });
        let (out, _missing) = expand_value(&input);
        assert_eq!(out, input);
    }

    // --- #1916: scalar type-coercion for lone `${...}` placeholders ---
    //
    // These pin the coercion contract the console mirrors in
    // `workers/console/web/.../schema-form/validate.ts::coerceScalar`.

    #[test]
    fn expand_value_coerces_lone_placeholder_default_to_integer() {
        // The headline bug: `port: ${HTTP_PORT:3111}` must become the integer
        // 3111, not the string "3111".
        unsafe {
            std::env::remove_var("CFG_COERCE_PORT");
        }
        let input = json!({ "port": "${CFG_COERCE_PORT:3111}" });
        let (out, missing) = expand_value(&input);
        assert!(missing.is_empty());
        assert_eq!(out["port"], json!(3111));
        assert!(out["port"].is_i64(), "must coerce to a JSON integer");
    }

    #[test]
    fn expand_value_coerces_lone_placeholder_env_to_integer() {
        unsafe {
            std::env::set_var("CFG_COERCE_ENVPORT", "8080");
        }
        let input = json!({ "port": "${CFG_COERCE_ENVPORT:3111}" });
        let (out, _missing) = expand_value(&input);
        assert_eq!(out["port"], json!(8080));
        assert!(out["port"].is_i64());
    }

    #[test]
    fn expand_value_coerces_bool_and_float() {
        unsafe {
            std::env::remove_var("CFG_COERCE_FLAG");
            std::env::remove_var("CFG_COERCE_RATIO");
        }
        let input = json!({
            "flag": "${CFG_COERCE_FLAG:true}",
            "ratio": "${CFG_COERCE_RATIO:3.5}",
        });
        let (out, _missing) = expand_value(&input);
        assert_eq!(out["flag"], json!(true));
        assert_eq!(out["ratio"], json!(3.5));
    }

    #[test]
    fn expand_value_keeps_bare_string_default() {
        unsafe {
            std::env::remove_var("CFG_COERCE_HOST");
        }
        let input = json!({ "host": "${CFG_COERCE_HOST:localhost}" });
        let (out, _missing) = expand_value(&input);
        assert_eq!(out["host"], json!("localhost"));
    }

    #[test]
    fn expand_value_keeps_yaml_keyword_strings() {
        // serde_yaml (YAML 1.2 core) does NOT treat on/off/yes/no as booleans;
        // they must round-trip as strings, not get mangled into bools.
        unsafe {
            std::env::remove_var("CFG_COERCE_MODE");
        }
        let input = json!({ "mode": "${CFG_COERCE_MODE:on}" });
        let (out, _missing) = expand_value(&input);
        assert_eq!(out["mode"], json!("on"));
    }

    #[test]
    fn expand_value_does_not_coerce_embedded_template() {
        // Surrounding text means the leaf stays a string even if the result
        // looks numeric.
        unsafe {
            std::env::remove_var("CFG_COERCE_EMB");
        }
        let input = json!({ "url": "redis://${CFG_COERCE_EMB:6379}" });
        let (out, _missing) = expand_value(&input);
        assert_eq!(out["url"], json!("redis://6379"));
    }

    #[test]
    fn expand_value_empty_default_stays_empty_string() {
        unsafe {
            std::env::remove_var("CFG_COERCE_EMPTY");
        }
        let input = json!({ "name": "${CFG_COERCE_EMPTY:}" });
        let (out, _missing) = expand_value(&input);
        assert_eq!(out["name"], json!(""));
    }

    #[test]
    fn expand_value_reports_missing_var_without_panic() {
        unsafe {
            std::env::remove_var("CFG_COERCE_REQUIRED");
        }
        let input = json!({ "port": "${CFG_COERCE_REQUIRED}" });
        let (out, missing) = expand_value(&input);
        assert_eq!(missing, vec!["CFG_COERCE_REQUIRED".to_string()]);
        // Literal left in place so partial output is still inspectable.
        assert_eq!(out["port"], json!("${CFG_COERCE_REQUIRED}"));
    }

    #[test]
    fn validate_against_schema_passes_valid_value() {
        let schema = json!({ "type": "object", "required": ["port"], "properties": { "port": { "type": "integer" } } });
        assert!(validate_against_schema(&json!({ "port": 3112 }), &schema).is_ok());
    }

    #[test]
    fn validate_against_schema_rejects_invalid_value() {
        let schema = json!({ "type": "object", "required": ["port"], "properties": { "port": { "type": "integer" } } });
        let err = validate_against_schema(&json!({ "port": "nope" }), &schema)
            .expect_err("string is not integer");
        assert!(!err.is_empty());
    }

    // Schema evolution: re-registering with a tightened schema must refresh the
    // stored schema even when the existing value no longer satisfies it. Workers
    // re-send their schema on every boot; if a value persisted under an older,
    // looser schema (e.g. a seed with `config: null`) blocked re-registration,
    // the console would keep rendering the stale schema forever.
    #[tokio::test]
    async fn register_refreshes_schema_over_now_invalid_existing_value() {
        use crate::workers::configuration::adapters::ConfigurationAdapter;
        use crate::workers::configuration::adapters::fs::FsAdapter;

        let dir = tempfile::tempdir().unwrap();
        let adapter = Arc::new(
            FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
                .await
                .unwrap(),
        ) as Arc<dyn ConfigurationAdapter>;
        let store = ConfigurationStore::new(adapter);

        // An older, looser schema accepts `config: null` (the shape an old seed
        // serialized to).
        store
            .register(
                "demo".into(),
                "Demo".into(),
                String::new(),
                json!({ "type": "object" }),
                Some(json!({ "adapter": { "name": "kv", "config": null } })),
                None,
            )
            .await
            .expect("seed registers under the lenient schema");

        // The tightened schema rejects `config: null`. Re-registering with NO new
        // value (what a worker does on every boot) must still refresh the schema.
        let strict = json!({
            "type": "object",
            "properties": {
                "adapter": {
                    "type": "object",
                    "required": ["config"],
                    "properties": { "config": { "type": "object" } }
                }
            }
        });
        store
            .register(
                "demo".into(),
                "Demo".into(),
                String::new(),
                strict.clone(),
                None,
                None,
            )
            .await
            .expect("a now-invalid existing value must not block the schema refresh");

        let view = store.schema_view("demo").await.expect("entry exists");
        assert_eq!(
            view.schema, strict,
            "the stored schema must be refreshed to the tightened version"
        );
    }

    // `set` against an entry whose schema is null (the shape a disk-loaded entry
    // has before its worker re-registers) must be rejected, then succeed once a
    // real schema is registered.
    #[tokio::test]
    async fn set_without_schema_is_rejected_then_succeeds_after_register() {
        use crate::workers::configuration::adapters::ConfigurationAdapter;
        use crate::workers::configuration::adapters::fs::FsAdapter;

        let dir = tempfile::tempdir().unwrap();
        let adapter = Arc::new(
            FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
                .await
                .unwrap(),
        ) as Arc<dyn ConfigurationAdapter>;
        let store = ConfigurationStore::new(adapter);

        store
            .register(
                "demo".into(),
                "Demo".into(),
                String::new(),
                Value::Null,
                None,
                None,
            )
            .await
            .expect("register with a null schema (no value validated)");

        let err = store
            .set("demo", json!({ "port": 1 }))
            .await
            .expect_err("set must be rejected while no schema is available");
        assert!(matches!(err, StoreError::SchemaUnavailable(_)));

        store
            .register(
                "demo".into(),
                "Demo".into(),
                String::new(),
                json!({ "type": "object" }),
                None,
                None,
            )
            .await
            .expect("schema refresh");
        store
            .set("demo", json!({ "port": 1 }))
            .await
            .expect("set succeeds once a real schema is present");
    }

    #[test]
    /// Reject names outside the lowercase bounded identifier contract before reaching storage.
    fn validate_id_rejects_uppercase_and_long_ids() {
        assert!(matches!(
            ConfigurationStore::validate_id("UPPER"),
            Err(StoreError::InvalidId(_))
        ));
        let long = "a".repeat(65);
        assert!(matches!(
            ConfigurationStore::validate_id(&long),
            Err(StoreError::InvalidId(_))
        ));
        assert!(ConfigurationStore::validate_id("iii-stream").is_ok());
        assert!(ConfigurationStore::validate_id("a_b-c-1").is_ok());
    }

    // ================================================================
    //  Atomicity / linearizability of the mutating surface
    //  (configuration seed-vs-set race fix). All concurrency tests are
    //  deterministic: ordering is forced with Notify gates, never sleeps.
    // ================================================================

    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    use crate::workers::configuration::adapters::fs::FsAdapter;

    /// Build a filesystem adapter rooted in the test's private temporary directory.
    async fn fs_adapter(dir: &std::path::Path) -> Arc<dyn ConfigurationAdapter> {
        Arc::new(
            FsAdapter::new(Some(json!({ "directory": dir.to_str().unwrap() })))
                .await
                .unwrap(),
        ) as Arc<dyn ConfigurationAdapter>
    }

    /// Allow object-valued fixtures without coupling concurrency assertions to schema details.
    fn any_object_schema() -> Value {
        json!({ "type": "object" })
    }

    /// Require an integer port so tests can distinguish applied from ignored seed validation.
    fn required_int_port_schema() -> Value {
        json!({
            "type": "object",
            "required": ["port"],
            "properties": { "port": { "type": "integer" } }
        })
    }

    /// Adapter wrapper that parks the FIRST `register` call: it signals
    /// `entered`, then awaits `release` before delegating. The store holds
    /// `write_lock` across the adapter round-trip, so this parks a mutation
    /// inside its critical section and lets a test prove a second mutation
    /// cannot interleave. Every later call passes straight through.
    struct GateAdapter {
        inner: Arc<dyn ConfigurationAdapter>,
        armed: AtomicBool,
        entered: Notify,
        release: Notify,
    }

    impl GateAdapter {
        /// Wrap storage with a one-shot gate that tests can arm before registration.
        fn wrap(inner: Arc<dyn ConfigurationAdapter>) -> Arc<Self> {
            Arc::new(Self {
                inner,
                armed: AtomicBool::new(false),
                entered: Notify::new(),
                release: Notify::new(),
            })
        }
    }

    #[async_trait::async_trait]
    impl ConfigurationAdapter for GateAdapter {
        /// Preserve the wrapped adapter's authoritative initialization strategy.
        fn ensure_support(&self) -> EnsureSupport {
            self.inner.ensure_support()
        }
        /// Forward the original initialization candidate without modifying its semantics.
        async fn ensure(&self, candidate: EnsureCandidate) -> anyhow::Result<AdapterEnsureOutcome> {
            self.inner.ensure(candidate).await
        }
        /// Park the armed mutation until released, then execute the real storage write.
        async fn register(&self, entry: ConfigurationEntry) -> anyhow::Result<RegisterOutcome> {
            if self.armed.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            self.inner.register(entry).await
        }
        /// Let operator writes use the real adapter while the test gates only registration.
        async fn set(&self, id: &str, value: Value) -> anyhow::Result<SetOutcome> {
            self.inner.set(id, value).await
        }
        /// Read the real adapter state so assertions compare cache with authoritative storage.
        async fn get(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            self.inner.get(id).await
        }
        /// Delegate deletions unchanged so the fixture can exercise real mutation ordering.
        async fn delete(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            self.inner.delete(id).await
        }
        /// Preserve authoritative enumeration when priming the store through this fixture.
        async fn list(&self) -> anyhow::Result<Vec<ConfigurationEntry>> {
            self.inner.list().await
        }
        /// Release wrapped adapter resources without adding fixture-specific storage mutations.
        async fn destroy(&self) -> anyhow::Result<()> {
            self.inner.destroy().await
        }
    }

    /// Adapter wrapper whose `register`/`set`/`get` fail while `fail` is set, so
    /// a test can drive a storage error and confirm the cache is untouched and
    /// the `write_lock` was released (a later mutation still succeeds). Failing
    /// `get` too lets an `apply_external` reconcile hit an authoritative-read
    /// failure and keep the last-known cache with no spurious delete.
    struct ToggleFailAdapter {
        inner: Arc<dyn ConfigurationAdapter>,
        fail: AtomicBool,
    }

    impl ToggleFailAdapter {
        /// Start with successful storage operations and permit explicit failure injection.
        fn wrap(inner: Arc<dyn ConfigurationAdapter>) -> Arc<Self> {
            Arc::new(Self {
                inner,
                fail: AtomicBool::new(false),
            })
        }
    }

    #[async_trait::async_trait]
    impl ConfigurationAdapter for ToggleFailAdapter {
        /// Preserve the wrapped adapter's authoritative initialization strategy.
        fn ensure_support(&self) -> EnsureSupport {
            self.inner.ensure_support()
        }
        /// Forward the original initialization candidate without modifying its semantics.
        async fn ensure(&self, candidate: EnsureCandidate) -> anyhow::Result<AdapterEnsureOutcome> {
            self.inner.ensure(candidate).await
        }
        /// Inject a register failure before storage changes, or delegate normally.
        async fn register(&self, entry: ConfigurationEntry) -> anyhow::Result<RegisterOutcome> {
            if self.fail.load(Ordering::SeqCst) {
                anyhow::bail!("injected register failure");
            }
            self.inner.register(entry).await
        }
        /// Inject a write failure without modifying the real adapter or its cache.
        async fn set(&self, id: &str, value: Value) -> anyhow::Result<SetOutcome> {
            if self.fail.load(Ordering::SeqCst) {
                anyhow::bail!("injected set failure");
            }
            self.inner.set(id, value).await
        }
        /// Simulate a failed authoritative read, distinct from an absent entry.
        async fn get(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            if self.fail.load(Ordering::SeqCst) {
                anyhow::bail!("injected get failure");
            }
            self.inner.get(id).await
        }
        /// Delegate deletions unchanged so the fixture can exercise real mutation ordering.
        async fn delete(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            self.inner.delete(id).await
        }
        /// Preserve authoritative enumeration when priming the store through this fixture.
        async fn list(&self) -> anyhow::Result<Vec<ConfigurationEntry>> {
            self.inner.list().await
        }
        /// Release wrapped adapter resources without adding fixture-specific storage mutations.
        async fn destroy(&self) -> anyhow::Result<()> {
            self.inner.destroy().await
        }
    }

    /// First initialization persists the candidate consistently in storage and cache.
    #[tokio::test]
    async fn ensure_seeds_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigurationStore::new(fs_adapter(dir.path()).await);
        let out = store
            .ensure(
                "demo".into(),
                "Demo".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "port": 3112 })),
                None,
            )
            .await
            .unwrap();
        assert_eq!(out.action, EnsureAction::Seeded);
        assert_eq!(out.entry.value, json!({ "port": 3112 }));
        assert_eq!(
            store.get("demo").await.unwrap().value,
            json!({ "port": 3112 })
        );
    }

    /// A registered null placeholder remains eligible for atomic initialization.
    #[tokio::test]
    async fn ensure_seeds_when_stored_value_is_null() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigurationStore::new(fs_adapter(dir.path()).await);
        // A null placeholder is the shape a seedless register / disk load leaves.
        store
            .register(
                "demo".into(),
                "Demo".into(),
                String::new(),
                any_object_schema(),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(store.get("demo").await.unwrap().value.is_null());

        let out = store
            .ensure(
                "demo".into(),
                "Demo".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "port": 1 })),
                None,
            )
            .await
            .unwrap();
        assert_eq!(out.action, EnsureAction::Seeded);
        assert_eq!(store.get("demo").await.unwrap().value, json!({ "port": 1 }));
    }

    /// An unused seed cannot replace existing data or reject an otherwise valid refresh.
    #[tokio::test]
    async fn ensure_preserves_existing_value_even_when_seed_differs_or_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigurationStore::new(fs_adapter(dir.path()).await);
        let schema = required_int_port_schema();
        store
            .register(
                "demo".into(),
                "Demo".into(),
                String::new(),
                schema.clone(),
                Some(json!({ "port": 10 })),
                None,
            )
            .await
            .unwrap();

        // A different AND schema-invalid seed must be ignored (never validated)
        // and the stored value preserved verbatim.
        let out = store
            .ensure(
                "demo".into(),
                "Demo".into(),
                String::new(),
                schema,
                Some(json!({ "port": "not-an-int" })),
                None,
            )
            .await
            .unwrap();
        assert_eq!(out.action, EnsureAction::Preserved);
        assert_eq!(
            store.get("demo").await.unwrap().value,
            json!({ "port": 10 })
        );
    }

    /// False, zero and an empty string are stored values, not first-boot placeholders.
    #[tokio::test]
    async fn ensure_preserves_falsey_values_false_zero_empty_string() {
        for stored in [json!(false), json!(0), json!("")] {
            let dir = tempfile::tempdir().unwrap();
            let store = ConfigurationStore::new(fs_adapter(dir.path()).await);
            // Boolean `true` schema accepts any value, so these scalars register.
            store
                .register(
                    "demo".into(),
                    "Demo".into(),
                    String::new(),
                    json!(true),
                    Some(stored.clone()),
                    None,
                )
                .await
                .unwrap();
            let out = store
                .ensure(
                    "demo".into(),
                    "Demo".into(),
                    String::new(),
                    json!(true),
                    Some(json!({ "seed": "ignored" })),
                    None,
                )
                .await
                .unwrap();
            assert_eq!(
                out.action,
                EnsureAction::Preserved,
                "stored value {stored} is real, not empty"
            );
            assert_eq!(store.get("demo").await.unwrap().value, stored);
        }
    }

    /// Retain unresolved environment templates verbatim for expansion at read time.
    #[tokio::test]
    async fn ensure_stores_seed_with_unresolved_env_var_raw() {
        unsafe {
            std::env::remove_var("CFG_ENSURE_UNSET");
        }
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigurationStore::new(fs_adapter(dir.path()).await);
        // The candidate references an unset var with no default: it cannot be
        // validated yet, so it is stored raw (register-parity), not rejected.
        let out = store
            .ensure(
                "demo".into(),
                "Demo".into(),
                String::new(),
                required_int_port_schema(),
                Some(json!({ "port": "${CFG_ENSURE_UNSET}" })),
                None,
            )
            .await
            .unwrap();
        assert_eq!(out.action, EnsureAction::Seeded);
        assert_eq!(
            store.get("demo").await.unwrap().value,
            json!({ "port": "${CFG_ENSURE_UNSET}" })
        );
    }

    /// A seed selected for persistence must satisfy the applied-value schema.
    #[tokio::test]
    async fn ensure_rejects_invalid_seed_when_applied() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigurationStore::new(fs_adapter(dir.path()).await);
        let err = store
            .ensure(
                "demo".into(),
                "Demo".into(),
                String::new(),
                required_int_port_schema(),
                Some(json!({ "port": "nope" })),
                None,
            )
            .await
            .expect_err("an applied seed is validated against the schema");
        assert!(matches!(err, StoreError::SchemaInvalid(_)));
    }

    /// Gate competing registrars to prove the later candidate cannot replace the winning seed.
    #[tokio::test]
    async fn two_concurrent_ensures_first_seed_wins_no_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let gate = GateAdapter::wrap(fs_adapter(dir.path()).await);
        let store = Arc::new(ConfigurationStore::new(
            gate.clone() as Arc<dyn ConfigurationAdapter>
        ));

        gate.armed.store(true, Ordering::SeqCst);

        let s_a = store.clone();
        let a = tokio::spawn(async move {
            s_a.ensure(
                "demo".into(),
                "A".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "seed": "a" })),
                None,
            )
            .await
        });

        // A is now parked inside adapter.register, still holding write_lock.
        gate.entered.notified().await;

        let s_b = store.clone();
        let b = tokio::spawn(async move {
            s_b.ensure(
                "demo".into(),
                "B".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "seed": "b" })),
                None,
            )
            .await
        });

        // Only after A commits and drops write_lock can B pass; it then sees the
        // seeded "a" and preserves it. No sleep: correctness is enforced by the
        // lock ordering, not timing.
        gate.release.notify_one();
        let a_out = a.await.unwrap().unwrap();
        let b_out = b.await.unwrap().unwrap();

        assert_eq!(a_out.action, EnsureAction::Seeded);
        assert_eq!(
            b_out.action,
            EnsureAction::Preserved,
            "the second seed must not clobber the first"
        );
        assert_eq!(
            store.get("demo").await.unwrap().value,
            json!({ "seed": "a" })
        );
        // Storage matches cache.
        assert_eq!(
            gate.inner.get("demo").await.unwrap().unwrap().value,
            json!({ "seed": "a" })
        );
    }

    /// A metadata refresh must not restore a stale value over a serialized operator update.
    #[tokio::test]
    async fn concurrent_metadata_register_and_set_do_not_lose_the_set() {
        let dir = tempfile::tempdir().unwrap();
        let gate = GateAdapter::wrap(fs_adapter(dir.path()).await);
        let store = Arc::new(ConfigurationStore::new(
            gate.clone() as Arc<dyn ConfigurationAdapter>
        ));
        let schema = required_int_port_schema();

        // Seed a null placeholder with a real schema (the shape a boot register
        // leaves before any value is set).
        store
            .register(
                "demo".into(),
                "Demo".into(),
                String::new(),
                schema.clone(),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(store.get("demo").await.unwrap().value.is_null());

        // Arm the gate for the NEXT register (the metadata-only re-register).
        gate.armed.store(true, Ordering::SeqCst);

        let s_a = store.clone();
        let sch = schema.clone();
        let a = tokio::spawn(async move {
            // Metadata-only re-register reuses the (null) value it reads.
            s_a.register(
                "demo".into(),
                "Demo v2".into(),
                String::new(),
                sch,
                None,
                None,
            )
            .await
        });

        // A has read prior (null) and is parked inside adapter.register while
        // still holding write_lock.
        gate.entered.notified().await;

        let s_b = store.clone();
        let b = tokio::spawn(async move { s_b.set("demo", json!({ "port": 4242 })).await });

        // Release A; it writes back the stale null it read. Only after A drops
        // write_lock can B run its set. Without the lock B's write would land
        // between A's read and A's write and be clobbered by the null.
        gate.release.notify_one();
        a.await.unwrap().unwrap();
        b.await.unwrap().unwrap();

        // The set survived: the metadata-only register did not overwrite it.
        assert_eq!(
            store.get("demo").await.unwrap().value,
            json!({ "port": 4242 })
        );
        assert_eq!(
            gate.inner.get("demo").await.unwrap().unwrap().value,
            json!({ "port": 4242 })
        );
    }

    /// Failed writes preserve the cache and release the mutation lock for subsequent writes.
    #[tokio::test]
    async fn storage_failure_leaves_cache_consistent_and_releases_lock() {
        let dir = tempfile::tempdir().unwrap();
        let fail = ToggleFailAdapter::wrap(fs_adapter(dir.path()).await);
        let store = ConfigurationStore::new(fail.clone() as Arc<dyn ConfigurationAdapter>);
        let schema = required_int_port_schema();

        store
            .register(
                "demo".into(),
                "Demo".into(),
                String::new(),
                schema,
                Some(json!({ "port": 1 })),
                None,
            )
            .await
            .unwrap();

        fail.fail.store(true, Ordering::SeqCst);
        let err = store
            .set("demo", json!({ "port": 2 }))
            .await
            .expect_err("adapter failure surfaces");
        assert!(matches!(err, StoreError::Adapter(_)));
        // Cache untouched by the failed write.
        assert_eq!(store.get("demo").await.unwrap().value, json!({ "port": 1 }));

        // The lock was released: a later successful mutation still works.
        fail.fail.store(false, Ordering::SeqCst);
        store
            .set("demo", json!({ "port": 3 }))
            .await
            .expect("lock released, set succeeds");
        assert_eq!(store.get("demo").await.unwrap().value, json!({ "port": 3 }));
    }

    /// Interleaved initialization and deletion converge to matching storage and cache state.
    #[tokio::test]
    async fn ensure_and_delete_serialize_without_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let gate = GateAdapter::wrap(fs_adapter(dir.path()).await);
        let store = Arc::new(ConfigurationStore::new(
            gate.clone() as Arc<dyn ConfigurationAdapter>
        ));

        gate.armed.store(true, Ordering::SeqCst);
        let s_a = store.clone();
        let a = tokio::spawn(async move {
            s_a.ensure(
                "demo".into(),
                "A".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "seed": "a" })),
                None,
            )
            .await
        });
        gate.entered.notified().await;

        let s_b = store.clone();
        let b = tokio::spawn(async move { s_b.delete("demo").await });

        gate.release.notify_one();
        a.await.unwrap().unwrap();
        b.await.unwrap().unwrap();

        // delete ran strictly after ensure committed, so the entry is gone from
        // both cache and storage with no half-applied state.
        assert!(store.get("demo").await.is_none());
        assert!(gate.inner.get("demo").await.unwrap().is_none());
    }

    /// Delegated adapter that RECORDS the candidate the store forwards to
    /// `ensure` and returns a canned outcome. `register` panics: a delegated
    /// ensure must NEVER fall back to `register`, so any register call is a bug.
    struct RecordingDelegatedAdapter {
        recorded: std::sync::Mutex<Option<EnsureCandidate>>,
        reply: ConfigurationEntry,
        reply_action: EnsureAction,
        fail: AtomicBool,
    }

    impl RecordingDelegatedAdapter {
        /// Prepare an authoritative remote reply and capture the candidate sent to it.
        fn new(reply: ConfigurationEntry, reply_action: EnsureAction) -> Arc<Self> {
            Arc::new(Self {
                recorded: std::sync::Mutex::new(None),
                reply,
                reply_action,
                fail: AtomicBool::new(false),
            })
        }
    }

    #[async_trait::async_trait]
    impl ConfigurationAdapter for RecordingDelegatedAdapter {
        /// Model an adapter whose remote authority, not the local cache, decides seeding.
        fn ensure_support(&self) -> EnsureSupport {
            EnsureSupport::Delegated
        }
        /// Record the forwarded seed and return the configured remote outcome or failure.
        async fn ensure(&self, candidate: EnsureCandidate) -> anyhow::Result<AdapterEnsureOutcome> {
            *self.recorded.lock().unwrap() = Some(candidate);
            if self.fail.load(Ordering::SeqCst) {
                anyhow::bail!("injected remote ensure failure");
            }
            Ok(AdapterEnsureOutcome {
                action: self.reply_action,
                entry: self.reply.clone(),
            })
        }
        /// Any legacy registration is an unsafe fallback and must fail this fixture immediately.
        async fn register(&self, _entry: ConfigurationEntry) -> anyhow::Result<RegisterOutcome> {
            panic!("delegated ensure must never fall back to register");
        }
        /// Initialization is not allowed to use unconditional set as an alternative write path.
        async fn set(&self, _id: &str, _value: Value) -> anyhow::Result<SetOutcome> {
            unreachable!()
        }
        /// Model an empty local mirror without supplying authority for the remote seed decision.
        async fn get(&self, _id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            Ok(None)
        }
        /// The recording fixture contains no persistent entry to delete.
        async fn delete(&self, _id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            Ok(None)
        }
        /// Start store priming with an empty mirror so delegated outcomes remain authoritative.
        async fn list(&self) -> anyhow::Result<Vec<ConfigurationEntry>> {
            Ok(Vec::new())
        }
        /// No background resources are owned by this in-memory delegation fixture.
        async fn destroy(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// Construct an object-schema entry for delegated initialization and reconciliation fixtures.
    fn mk_entry(id: &str, value: Value) -> ConfigurationEntry {
        ConfigurationEntry {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            schema: any_object_schema(),
            value,
            metadata: None,
        }
    }

    /// Preserve the remote decision even when the local cache contains a conflicting stale value.
    #[tokio::test]
    async fn delegated_ensure_forwards_original_candidate_not_cached_value() {
        // The authoritative store lives behind a Delegated adapter. Even if the
        // local cache already holds a (possibly stale) value, ensure must
        // forward the ORIGINAL candidate so the remote decides — never the
        // cached value — and reconcile the cache from the returned entry.
        let adapter = RecordingDelegatedAdapter::new(
            mk_entry("demo", json!({ "port": 5000 })),
            EnsureAction::Preserved,
        );
        let store = ConfigurationStore::new(adapter.clone() as Arc<dyn ConfigurationAdapter>);

        // Prime the local cache with a stale value that must NOT be forwarded.
        store
            .apply_external(&ExternalChange::Registered(mk_entry(
                "demo",
                json!({ "port": 1111 }),
            )))
            .await;

        let out = store
            .ensure(
                "demo".into(),
                "Demo".into(),
                "d".into(),
                any_object_schema(),
                Some(json!({ "port": 9999 })),
                None,
            )
            .await
            .unwrap();

        let forwarded = adapter
            .recorded
            .lock()
            .unwrap()
            .clone()
            .expect("ensure was forwarded to the adapter");
        assert_eq!(
            forwarded.candidate,
            Some(json!({ "port": 9999 })),
            "the ORIGINAL candidate must be forwarded, not the cached value"
        );
        // The local cache is reconciled from the AUTHORITATIVE returned entry.
        assert_eq!(out.entry.value, json!({ "port": 5000 }));
        assert_eq!(
            store.get("demo").await.unwrap().value,
            json!({ "port": 5000 })
        );
        assert_eq!(out.action, EnsureAction::Preserved);
        // A delegated ensure never owns the local fan-out (relayed by watcher).
        assert!(out.register_kind.is_none());
    }

    /// Remote failure must not trigger the unsafe legacy registration path or populate the cache.
    #[tokio::test]
    async fn delegated_ensure_error_does_not_fall_back_to_register() {
        let adapter =
            RecordingDelegatedAdapter::new(mk_entry("demo", Value::Null), EnsureAction::Registered);
        adapter.fail.store(true, Ordering::SeqCst);
        let store = ConfigurationStore::new(adapter.clone() as Arc<dyn ConfigurationAdapter>);

        let err = store
            .ensure(
                "demo".into(),
                "Demo".into(),
                "d".into(),
                any_object_schema(),
                Some(json!({ "port": 9999 })),
                None,
            )
            .await
            .expect_err("remote ensure failure must surface, not fall back to register");
        assert!(matches!(err, StoreError::Adapter(_)));
        // Cache stays empty: no legacy register wrote anything.
        assert!(store.get("demo").await.is_none());
    }

    /// A `Delegated` adapter that does not override `ensure` (the trait default)
    /// must fail closed rather than let the store seed against a stale cache.
    struct DefaultDelegatedAdapter;

    #[async_trait::async_trait]
    impl ConfigurationAdapter for DefaultDelegatedAdapter {
        // ensure_support defaults to Delegated; ensure defaults to a fail-closed bail.
        /// Unsupported atomic initialization must never silently invoke the legacy writer.
        async fn register(&self, _entry: ConfigurationEntry) -> anyhow::Result<RegisterOutcome> {
            panic!("default-delegated adapter must not register");
        }
        /// Initialization is not allowed to use unconditional set as an alternative write path.
        async fn set(&self, _id: &str, _value: Value) -> anyhow::Result<SetOutcome> {
            unreachable!()
        }
        /// Model an empty local mirror without supplying authority for the remote seed decision.
        async fn get(&self, _id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            Ok(None)
        }
        /// The recording fixture contains no persistent entry to delete.
        async fn delete(&self, _id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            Ok(None)
        }
        /// Start store priming with an empty mirror so delegated outcomes remain authoritative.
        async fn list(&self) -> anyhow::Result<Vec<ConfigurationEntry>> {
            Ok(Vec::new())
        }
        /// No background resources are owned by this in-memory delegation fixture.
        async fn destroy(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// An adapter without an atomic implementation must reject initialization before any write.
    #[tokio::test]
    async fn default_delegated_adapter_fails_closed_on_ensure() {
        let store = ConfigurationStore::new(
            Arc::new(DefaultDelegatedAdapter) as Arc<dyn ConfigurationAdapter>
        );
        let err = store
            .ensure(
                "demo".into(),
                "Demo".into(),
                "d".into(),
                any_object_schema(),
                Some(json!({ "port": 1 })),
                None,
            )
            .await
            .expect_err("a Delegated adapter with no ensure impl must fail closed");
        assert!(matches!(err, StoreError::Adapter(_)));
    }

    // ================================================================
    //  apply_external reconciles a QUEUED/DELAYED watcher snapshot
    //  against the authoritative adapter (CodeRabbit #4053820511).
    //
    //  The fs watcher captures a snapshot under the ADAPTER cache lock and
    //  queues it; `apply_external` applies it later under the STORE
    //  write_lock (a different lock). A local mutation can commit in that
    //  gap, so a verbatim apply of the queued snapshot reverts the cache
    //  (get returns stale, a later ensure could clobber). These tests force
    //  that ordering deterministically -- no sleeps, no watcher timing.
    // ================================================================

    /// RED->GREEN regression: a queued external update applied after a newer
    /// local set must not revert the cache to the stale snapshot value --
    /// reconcile against the authoritative adapter, which holds the new value.
    #[tokio::test]
    async fn apply_external_stale_update_does_not_revert_newer_local_set() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigurationStore::new(fs_adapter(dir.path()).await);
        store
            .register(
                "demo".into(),
                "demo".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "port": 1 })),
                None,
            )
            .await
            .unwrap();

        // The watcher captured an external update to {port: 2} and queued it.
        let queued = ExternalChange::Updated {
            entry: mk_entry("demo", json!({ "port": 2 })),
            old_value: Some(json!({ "port": 1 })),
        };

        // A newer local set to {port: 3} commits before the queued event is
        // applied: it updates BOTH the adapter and the cache.
        store.set("demo", json!({ "port": 3 })).await.unwrap();

        // Applying the stale queued snapshot must NOT revert the cache to 2.
        store.apply_external(&queued).await;

        assert_eq!(
            store.get("demo").await.unwrap().value,
            json!({ "port": 3 }),
            "a queued external snapshot must not clobber a newer local set"
        );
        assert_eq!(
            store.adapter().get("demo").await.unwrap().unwrap().value,
            json!({ "port": 3 }),
            "the authoritative adapter value is unchanged"
        );
    }

    /// RED->GREEN regression: a queued external delete applied after a local
    /// register recreated the entry must not drop it -- reconcile against the
    /// authoritative adapter, which now holds the recreated value.
    #[tokio::test]
    async fn apply_external_stale_delete_does_not_drop_locally_recreated_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigurationStore::new(fs_adapter(dir.path()).await);
        store
            .register(
                "demo".into(),
                "demo".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "port": 1 })),
                None,
            )
            .await
            .unwrap();

        // The watcher captured an external delete and queued it.
        let queued = ExternalChange::Deleted {
            entry: mk_entry("demo", json!({ "port": 1 })),
        };

        // A newer local register recreates the entry before the delete lands.
        store
            .register(
                "demo".into(),
                "demo".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "port": 7 })),
                None,
            )
            .await
            .unwrap();

        // Applying the stale delete must NOT drop the recreated entry.
        store.apply_external(&queued).await;

        assert_eq!(
            store.get("demo").await.unwrap().value,
            json!({ "port": 7 }),
            "a queued external delete must not drop a locally recreated entry"
        );
    }

    /// Scenario 1: a queued external update a newer local set superseded must
    /// reconcile the cache silently, reporting `Suppressed` so no stale
    /// `configuration:*` event is fanned out.
    #[tokio::test]
    async fn apply_external_superseded_update_suppresses_fan_out() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigurationStore::new(fs_adapter(dir.path()).await);
        store
            .register(
                "demo".into(),
                "demo".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "port": 1 })),
                None,
            )
            .await
            .unwrap();
        let queued = ExternalChange::Updated {
            entry: mk_entry("demo", json!({ "port": 2 })),
            old_value: Some(json!({ "port": 1 })),
        };
        store.set("demo", json!({ "port": 3 })).await.unwrap();
        assert_eq!(
            store.apply_external(&queued).await,
            ExternalApply::Suppressed
        );
    }

    /// Scenario 2: a queued delete a local recreate superseded reconciles
    /// silently, reporting `Suppressed` so the recreated entry stays and no
    /// delete event fires.
    #[tokio::test]
    async fn apply_external_superseded_delete_suppresses_fan_out() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigurationStore::new(fs_adapter(dir.path()).await);
        store
            .register(
                "demo".into(),
                "demo".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "port": 1 })),
                None,
            )
            .await
            .unwrap();
        let queued = ExternalChange::Deleted {
            entry: mk_entry("demo", json!({ "port": 1 })),
        };
        store
            .register(
                "demo".into(),
                "demo".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "port": 7 })),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            store.apply_external(&queued).await,
            ExternalApply::Suppressed
        );
    }

    /// Scenario 4: a genuine external edit (no interleaving local write) still
    /// ingests into the cache and fans out.
    #[tokio::test]
    async fn apply_external_fresh_update_ingests_and_fans_out() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigurationStore::new(fs_adapter(dir.path()).await);
        store
            .register(
                "demo".into(),
                "demo".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "port": 1 })),
                None,
            )
            .await
            .unwrap();
        // The watcher already applied the external edit to the adapter's
        // authoritative state; deliver the matching Updated snapshot.
        store
            .adapter()
            .set("demo", json!({ "port": 9 }))
            .await
            .unwrap();
        let change = ExternalChange::Updated {
            entry: mk_entry("demo", json!({ "port": 9 })),
            old_value: Some(json!({ "port": 1 })),
        };
        assert_eq!(store.apply_external(&change).await, ExternalApply::Fanout);
        assert_eq!(store.get("demo").await.unwrap().value, json!({ "port": 9 }));
    }

    /// Scenario 4: a genuine external removal still drops the entry and fans out
    /// a delete.
    #[tokio::test]
    async fn apply_external_real_delete_removes_and_fans_out() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigurationStore::new(fs_adapter(dir.path()).await);
        store
            .register(
                "demo".into(),
                "demo".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "port": 1 })),
                None,
            )
            .await
            .unwrap();
        // The watcher already removed the entry from the authoritative store.
        store.adapter().delete("demo").await.unwrap();
        let change = ExternalChange::Deleted {
            entry: mk_entry("demo", json!({ "port": 1 })),
        };
        assert_eq!(store.apply_external(&change).await, ExternalApply::Fanout);
        assert!(store.get("demo").await.is_none());
    }

    /// Scenario 4: a failed authoritative read must leave the cache intact
    /// (never a spurious delete or revert) and suppress the fan-out.
    #[tokio::test]
    async fn apply_external_read_failure_keeps_cache_and_suppresses() {
        let dir = tempfile::tempdir().unwrap();
        let fail = ToggleFailAdapter::wrap(fs_adapter(dir.path()).await);
        let store = ConfigurationStore::new(fail.clone() as Arc<dyn ConfigurationAdapter>);
        store
            .register(
                "demo".into(),
                "demo".into(),
                String::new(),
                any_object_schema(),
                Some(json!({ "port": 1 })),
                None,
            )
            .await
            .unwrap();
        fail.fail.store(true, Ordering::SeqCst);
        let change = ExternalChange::Updated {
            entry: mk_entry("demo", json!({ "port": 2 })),
            old_value: Some(json!({ "port": 1 })),
        };
        assert_eq!(
            store.apply_external(&change).await,
            ExternalApply::Suppressed
        );
        fail.fail.store(false, Ordering::SeqCst);
        assert_eq!(store.get("demo").await.unwrap().value, json!({ "port": 1 }));
    }

    /// A `Delegated` adapter's authority is a remote engine with an ordered
    /// relayed event stream, so the snapshot is applied verbatim and fans out
    /// (no reconcile RPC that could mistake a transient failure for a delete).
    #[tokio::test]
    async fn apply_external_delegated_applies_snapshot_without_reconcile() {
        let adapter = RecordingDelegatedAdapter::new(
            mk_entry("demo", json!({ "port": 1 })),
            EnsureAction::Preserved,
        );
        let store = ConfigurationStore::new(adapter as Arc<dyn ConfigurationAdapter>);
        let change = ExternalChange::Registered(mk_entry("demo", json!({ "port": 5000 })));
        assert_eq!(store.apply_external(&change).await, ExternalApply::Fanout);
        assert_eq!(
            store.get("demo").await.unwrap().value,
            json!({ "port": 5000 })
        );
    }
}
