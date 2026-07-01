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
use tokio::sync::RwLock;

use crate::workers::configuration::adapters::{
    ConfigurationAdapter, ExternalChange, RegisterOutcome, SetOutcome,
};
use crate::workers::configuration::structs::{ConfigurationEntry, ConfigurationSchemaView};

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

pub struct ConfigurationStore {
    adapter: Arc<dyn ConfigurationAdapter>,
    /// Authoritative in-memory cache. Source of truth for `get`/`list`/`schema`.
    /// Populated lazily from the adapter and kept in sync on every mutation.
    entries: Arc<RwLock<HashMap<String, ConfigurationEntry>>>,
}

impl ConfigurationStore {
    pub fn new(adapter: Arc<dyn ConfigurationAdapter>) -> Self {
        Self {
            adapter,
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn adapter(&self) -> &Arc<dyn ConfigurationAdapter> {
        &self.adapter
    }

    /// Pull every entry the adapter knows about into the cache. Called once
    /// during worker `initialize()`.
    pub async fn prime_from_adapter(&self) -> anyhow::Result<()> {
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

    pub async fn set(&self, id: &str, value: Value) -> Result<SetOutcome, StoreError> {
        Self::validate_id(id)?;

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
}
