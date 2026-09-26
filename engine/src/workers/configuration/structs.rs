// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single configuration entry tracked by the worker.
///
/// `value` is stored verbatim — including any `${VAR:default}` template
/// strings. Expansion happens on read (`configuration::get`) and during
/// trigger fan-out so env-var changes propagate without a worker restart.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationEntry {
    /// Stable identifier (e.g. `iii-stream`, `iii-observability`).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// One-line description of what this configuration controls.
    pub description: String,
    /// JSON Schema describing the shape of `value`. Not persisted to disk by
    /// the fs adapter; rebuilt in-memory when the owning worker re-registers.
    #[serde(default)]
    pub schema: Value,
    /// Stored configuration body. May contain `${VAR:default}` placeholders.
    #[serde(default)]
    pub value: Value,
    /// Optional caller-supplied metadata (owner team, change ticket, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// Subset of a [`ConfigurationEntry`] returned from `list` / `schema` —
/// the schema-only view that never leaks the stored value.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationSchemaView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl From<&ConfigurationEntry> for ConfigurationSchemaView {
    fn from(entry: &ConfigurationEntry) -> Self {
        Self {
            id: entry.id.clone(),
            name: entry.name.clone(),
            description: entry.description.clone(),
            schema: entry.schema.clone(),
            metadata: entry.metadata.clone(),
        }
    }
}

/// Move one exact legacy id with source priority, retaining its original backup.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationMigrateInput {
    pub from_id: String,
    pub to_id: String,
}

/// Migration never treats an existing null value as absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MigrateAction {
    Migrated,
    Preserved,
    Missing,
}

/// Authoritative raw destination snapshot, without environment expansion.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationMigrateResult {
    pub action: MigrateAction,
    pub entry: Option<ConfigurationEntry>,
}

// ── function inputs / outputs ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationRegisterInput {
    /// Configuration id. Must match `[a-z0-9_-]{1,64}` so it is safe as a
    /// filename in the `fs` adapter.
    pub id: String,
    /// Human-readable name shown in `configuration::list`.
    pub name: String,
    /// Description shown in `configuration::list`.
    pub description: String,
    /// JSON Schema describing the value shape. `set` validates against this.
    pub schema: Value,
    /// Optional initial value to install. Validated against `schema`.
    pub initial_value: Option<Value>,
    /// Optional opaque metadata stored alongside the entry.
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationEnsureInput {
    /// Configuration id. Must match `[a-z0-9_-]{1,64}` so it is safe as a
    /// filename in the `fs` adapter.
    pub id: String,
    /// Human-readable name shown in `configuration::list`.
    pub name: String,
    /// Description shown in `configuration::list`.
    pub description: String,
    /// JSON Schema describing the value shape. `set` validates against this.
    pub schema: Value,
    /// Optional seed value. Applied atomically ONLY when no non-null value is
    /// already stored for this id (the id is absent or its stored value is
    /// `null`). When a non-null value already exists it is preserved verbatim
    /// and this candidate is ignored — never validated. When the seed IS
    /// applied it is validated against `schema` exactly like
    /// `configuration::register` validates `initial_value`.
    pub initial_value: Option<Value>,
    /// Optional opaque metadata stored alongside the entry.
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationSetInput {
    pub id: String,
    /// New configuration value. Validated against the schema when available.
    pub value: Value,
    /// Persist the complete value (default true). False updates only active memory
    /// and permits delivery before the owning worker registers its schema.
    #[serde(default = "default_flush")]
    pub flush: bool,
}

fn default_flush() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationGetInput {
    pub id: String,
    /// When `true`, return the stored value verbatim (including `${VAR}` placeholders).
    /// When `false` (default), expand `${VAR:default}` against the live process env.
    #[serde(default)]
    pub raw: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationListInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationSchemaInput {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationGetResult {
    pub id: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationSetResult {
    /// Previous stored value, or `null` when the entry had no value yet.
    pub old_value: Option<Value>,
    /// Current stored value (templates not expanded).
    pub new_value: Value,
}

/// What `configuration::ensure` did with the stored value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum EnsureAction {
    /// The candidate `initial_value` was written because no non-null value was
    /// stored yet (the id was absent or its stored value was `null`).
    #[serde(rename = "seeded")]
    Seeded,
    /// A non-null value was already stored and preserved verbatim; the
    /// candidate seed (if any) was ignored. Name, description, schema, and
    /// metadata were still refreshed.
    #[serde(rename = "preserved")]
    Preserved,
    /// The entry was created or refreshed but no seed was applied and no value
    /// existed, so the value stays `null` (awaiting a later `set`/`ensure`).
    #[serde(rename = "registered")]
    Registered,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationEnsureResult {
    /// What ensure did: seeded the candidate, preserved an existing value, or
    /// registered the entry with a null value.
    pub action: EnsureAction,
    /// The stored entry after ensure. Templates inside `value` are kept
    /// verbatim; expansion happens on read.
    pub entry: ConfigurationEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationListResult {
    pub configurations: Vec<ConfigurationSchemaView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum ConfigurationEventType {
    #[serde(rename = "configuration:registered")]
    Registered,
    #[serde(rename = "configuration:updated")]
    Updated,
    #[serde(rename = "configuration:deleted")]
    Deleted,
}

/// Trigger event payload. Mirrors [`crate::trigger_formats::ConfigurationCallRequest`]
/// but lives next to the worker so the in-process invoker doesn't depend on
/// the schema-only struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationEventData {
    #[serde(rename = "type")]
    pub message_type: String,
    pub event_type: ConfigurationEventType,
    pub id: String,
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub old_value: Option<Value>,
    pub new_value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_view_drops_value_field() {
        let entry = ConfigurationEntry {
            id: "iii-stream".into(),
            name: "Stream worker".into(),
            description: "...".into(),
            schema: json!({ "type": "object" }),
            value: json!({ "port": 3112 }),
            metadata: None,
        };
        let view = ConfigurationSchemaView::from(&entry);
        let serialized = serde_json::to_value(view).unwrap();
        assert!(serialized.get("value").is_none());
        assert_eq!(serialized["id"], "iii-stream");
    }

    #[test]
    fn event_type_serde_roundtrip() {
        for (variant, wire) in [
            (
                ConfigurationEventType::Registered,
                "configuration:registered",
            ),
            (ConfigurationEventType::Updated, "configuration:updated"),
            (ConfigurationEventType::Deleted, "configuration:deleted"),
        ] {
            let v = serde_json::to_value(&variant).unwrap();
            assert_eq!(v.as_str().unwrap(), wire);
        }
    }

    #[test]
    fn get_input_defaults_raw_to_false() {
        let input: ConfigurationGetInput =
            serde_json::from_value(json!({ "id": "iii-stream" })).unwrap();
        assert!(!input.raw);
    }
}
