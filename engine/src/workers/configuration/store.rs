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

    pub async fn get(&self, id: &str) -> Option<ConfigurationEntry> {
        self.entries.read().await.get(id).cloned()
    }

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

    pub async fn schema_view(&self, id: &str) -> Option<ConfigurationSchemaView> {
        self.entries
            .read()
            .await
            .get(id)
            .map(ConfigurationSchemaView::from)
    }

    /// Apply an external change (file edit, remote bridge event) into the
    /// cache without round-tripping through the adapter again.
    pub async fn apply_external(&self, change: &ExternalChange) {
        let _write = self.write_lock.lock().await;
        let mut cache = self.entries.write().await;
        match change {
            ExternalChange::Registered(entry) | ExternalChange::Updated { entry, .. } => {
                cache.insert(entry.id.clone(), entry.clone());
            }
            ExternalChange::Deleted { entry } => {
                cache.remove(&entry.id);
            }
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

    async fn fs_adapter(dir: &std::path::Path) -> Arc<dyn ConfigurationAdapter> {
        Arc::new(
            FsAdapter::new(Some(json!({ "directory": dir.to_str().unwrap() })))
                .await
                .unwrap(),
        ) as Arc<dyn ConfigurationAdapter>
    }

    fn any_object_schema() -> Value {
        json!({ "type": "object" })
    }

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
        fn ensure_support(&self) -> EnsureSupport {
            self.inner.ensure_support()
        }
        async fn ensure(&self, candidate: EnsureCandidate) -> anyhow::Result<AdapterEnsureOutcome> {
            self.inner.ensure(candidate).await
        }
        async fn register(&self, entry: ConfigurationEntry) -> anyhow::Result<RegisterOutcome> {
            if self.armed.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            self.inner.register(entry).await
        }
        async fn set(&self, id: &str, value: Value) -> anyhow::Result<SetOutcome> {
            self.inner.set(id, value).await
        }
        async fn get(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            self.inner.get(id).await
        }
        async fn delete(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            self.inner.delete(id).await
        }
        async fn list(&self) -> anyhow::Result<Vec<ConfigurationEntry>> {
            self.inner.list().await
        }
        async fn destroy(&self) -> anyhow::Result<()> {
            self.inner.destroy().await
        }
    }

    /// Adapter wrapper whose `register`/`set` fail while `fail` is set, so a
    /// test can drive a storage error and confirm the cache is untouched and
    /// the `write_lock` was released (a later mutation still succeeds).
    struct ToggleFailAdapter {
        inner: Arc<dyn ConfigurationAdapter>,
        fail: AtomicBool,
    }

    impl ToggleFailAdapter {
        fn wrap(inner: Arc<dyn ConfigurationAdapter>) -> Arc<Self> {
            Arc::new(Self {
                inner,
                fail: AtomicBool::new(false),
            })
        }
    }

    #[async_trait::async_trait]
    impl ConfigurationAdapter for ToggleFailAdapter {
        fn ensure_support(&self) -> EnsureSupport {
            self.inner.ensure_support()
        }
        async fn ensure(&self, candidate: EnsureCandidate) -> anyhow::Result<AdapterEnsureOutcome> {
            self.inner.ensure(candidate).await
        }
        async fn register(&self, entry: ConfigurationEntry) -> anyhow::Result<RegisterOutcome> {
            if self.fail.load(Ordering::SeqCst) {
                anyhow::bail!("injected register failure");
            }
            self.inner.register(entry).await
        }
        async fn set(&self, id: &str, value: Value) -> anyhow::Result<SetOutcome> {
            if self.fail.load(Ordering::SeqCst) {
                anyhow::bail!("injected set failure");
            }
            self.inner.set(id, value).await
        }
        async fn get(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            self.inner.get(id).await
        }
        async fn delete(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            self.inner.delete(id).await
        }
        async fn list(&self) -> anyhow::Result<Vec<ConfigurationEntry>> {
            self.inner.list().await
        }
        async fn destroy(&self) -> anyhow::Result<()> {
            self.inner.destroy().await
        }
    }

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
        fn ensure_support(&self) -> EnsureSupport {
            EnsureSupport::Delegated
        }
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
        async fn register(&self, _entry: ConfigurationEntry) -> anyhow::Result<RegisterOutcome> {
            panic!("delegated ensure must never fall back to register");
        }
        async fn set(&self, _id: &str, _value: Value) -> anyhow::Result<SetOutcome> {
            unreachable!()
        }
        async fn get(&self, _id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            Ok(None)
        }
        async fn delete(&self, _id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            Ok(None)
        }
        async fn list(&self) -> anyhow::Result<Vec<ConfigurationEntry>> {
            Ok(Vec::new())
        }
        async fn destroy(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

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
        async fn register(&self, _entry: ConfigurationEntry) -> anyhow::Result<RegisterOutcome> {
            panic!("default-delegated adapter must not register");
        }
        async fn set(&self, _id: &str, _value: Value) -> anyhow::Result<SetOutcome> {
            unreachable!()
        }
        async fn get(&self, _id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            Ok(None)
        }
        async fn delete(&self, _id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
            Ok(None)
        }
        async fn list(&self) -> anyhow::Result<Vec<ConfigurationEntry>> {
            Ok(Vec::new())
        }
        async fn destroy(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

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
}
