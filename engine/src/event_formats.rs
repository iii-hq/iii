// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The events of the engine-hosted sources (`configuration::on-updated`,
//! `observability::on-log`, `observability::on-span`, `stream::on-changed`,
//! `stream::on-join`, `stream::on-leave`). Each of a source's calls has its
//! event as its input and as its output, so a type here is both the source's
//! `request_format` and its `response_format`, generated from `JsonSchema`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Stream ──────────────────────────────────────────────────────────────

/// `stream::on-changed`: one write to a stream item, or one custom event
/// sent to a group.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamChangedEvent {
    /// `create`, `update`, `delete`, `sync` or `event`
    pub event_type: String,
    /// Event timestamp (ms)
    pub timestamp: i64,
    /// Stream name
    pub stream_name: String,
    /// Group ID
    pub group_id: String,
    /// Item ID
    pub id: Option<String>,
    /// The change as stream subscribers receive it
    pub event: Value,
}

/// `stream::on-join` and `stream::on-leave`: a stream client subscribed to a
/// group (or an item), or left it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamJoinLeaveEvent {
    /// The client's subscription id
    pub subscription_id: String,
    /// Stream name
    pub stream_name: String,
    /// Group ID
    pub group_id: String,
    /// Item ID, when the client subscribed to one item
    pub id: Option<String>,
    /// What the stream's `auth_function` returned for the client
    pub context: Option<Value>,
}

// ── Configuration ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum ConfigurationEventType {
    #[serde(rename = "configuration:registered")]
    Registered,
    #[serde(rename = "configuration:updated")]
    Updated,
    #[serde(rename = "configuration:deleted")]
    Deleted,
}

/// `configuration::on-updated`: a configuration entry was registered,
/// updated or deleted.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationEvent {
    /// Always "configuration"
    #[serde(rename = "type")]
    pub message_type: String,
    /// Type of configuration change
    pub event_type: ConfigurationEventType,
    /// Configuration id (e.g. `iii-stream`)
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    pub description: String,
    /// JSON Schema describing the value shape
    pub schema: Value,
    /// Previous value (null on registered/deleted-without-prior-state events)
    pub old_value: Option<Value>,
    /// New value with `${VAR:default}` placeholders expanded; null on deletion
    pub new_value: Option<Value>,
    /// Optional caller-supplied metadata stored alongside the entry
    pub metadata: Option<Value>,
}

// ── Log (observability) ─────────────────────────────────────────────────

/// `observability::on-log`: one ingested log record.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LogEvent {
    /// Log timestamp in nanoseconds
    pub timestamp_unix_nano: u64,
    /// Observed timestamp in nanoseconds
    pub observed_timestamp_unix_nano: u64,
    /// Severity number (OpenTelemetry)
    pub severity_number: u32,
    /// Severity text (e.g. INFO, ERROR)
    pub severity_text: String,
    /// Log message body
    pub body: String,
    /// Log attributes
    pub attributes: Value,
    /// Trace ID
    pub trace_id: String,
    /// Span ID
    pub span_id: String,
    /// OpenTelemetry resource
    pub resource: Value,
    /// Service name
    pub service_name: String,
    /// Instrumentation scope name
    pub instrumentation_scope_name: String,
    /// Instrumentation scope version
    pub instrumentation_scope_version: String,
}

// ── Span (observability) ────────────────────────────────────────────────

/// `observability::on-span`: a coalesced "traces changed" tick. Span activity
/// is debounced and the distinct affected trace ids in the window are
/// delivered, rather than per-span full payloads: a "refetch soon" beat, not a
/// span feed. Re-read details via `engine::traces::list` for summaries or
/// `engine::traces::spans` / `engine::traces::tree` for full detail.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SpanEvent {
    /// Distinct trace ids that had span activity in this coalesced window.
    pub trace_ids: Vec<String>,
}
