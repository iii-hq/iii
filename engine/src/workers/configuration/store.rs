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
use std::sync::Arc;

use jsonschema::Validator;
use serde_json::{Map, Value};
use tokio::sync::RwLock;

use crate::workers::config::EngineConfig;
use crate::workers::configuration::adapters::{
    ConfigurationAdapter, ExternalChange, RegisterOutcome, SetOutcome,
};
use crate::workers::configuration::structs::{ConfigurationEntry, ConfigurationSchemaView};

/// Walk a JSON value and replace every string leaf with the env-var-expanded
/// form (`${VAR:default}` → process env or default). Maps and arrays are
/// walked recursively; non-string scalars pass through unchanged.
pub fn expand_value(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(EngineConfig::expand_env_vars(s)),
        Value::Array(items) => Value::Array(items.iter().map(expand_value).collect()),
        Value::Object(map) => {
            let mut out: Map<String, Value> = Map::with_capacity(map.len());
            for (k, val) in map {
                out.insert(k.clone(), expand_value(val));
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

        if validate && let Err(errs) = validate_against_schema(&value, &schema) {
            return Err(StoreError::SchemaInvalid(errs.join("; ")));
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
        if let Err(errs) = validate_against_schema(&value, &entry.schema) {
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
        let expanded = expand_value(&input);
        assert_eq!(expanded["host"], "db.local");
        assert_eq!(expanded["port"], 5432);
    }

    #[test]
    fn expand_value_uses_default_when_var_missing() {
        unsafe {
            std::env::remove_var("CFG_TEST_MISSING");
        }
        let input = json!({ "url": "${CFG_TEST_MISSING:http://default}" });
        assert_eq!(expand_value(&input)["url"], "http://default");
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
        let out = expand_value(&input);
        assert_eq!(out["users"][0]["name"], "alice");
        assert_eq!(out["users"][1]["name"], "static");
    }

    #[test]
    fn expand_value_passes_non_string_scalars_through() {
        let input = json!({ "n": 42, "b": true, "nil": null });
        let out = expand_value(&input);
        assert_eq!(out, input);
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
