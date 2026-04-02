// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Typed configuration and call-request formats for built-in trigger types.
//!
//! Each trigger type has two associated schemas:
//! - **Configuration format**: what config fields a trigger expects at registration time.
//! - **Call request format**: what payload the function receives when the trigger fires.
//!
//! These structs derive `JsonSchema` so the engine can auto-generate JSON Schema
//! definitions, following the same pattern used by `register_function`.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── HTTP ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HttpTriggerConfig {
    /// HTTP endpoint path (e.g. `/users/:id`)
    pub api_path: String,
    /// HTTP method (defaults to GET)
    #[serde(default = "default_http_method")]
    pub http_method: Option<HttpMethod>,
    /// Optional function ID to evaluate before invoking handler
    pub condition_function_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
    HEAD,
    OPTIONS,
}

fn default_http_method() -> Option<HttpMethod> {
    Some(HttpMethod::GET)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HttpCallRequest {
    /// URL query parameters
    pub query_params: HashMap<String, String>,
    /// URL path parameters
    pub path_params: HashMap<String, String>,
    /// HTTP request headers
    pub headers: HashMap<String, String>,
    /// Request path
    pub path: String,
    /// HTTP method
    pub method: String,
    /// Request body
    pub body: Value,
}

// ── Cron ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CronTriggerConfig {
    /// Cron expression (6-field format: sec min hour day month weekday)
    pub expression: String,
    /// Optional function ID to evaluate before invoking handler
    pub condition_function_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CronCallRequest {
    /// Always "cron"
    pub trigger: String,
    /// Unique job identifier
    pub job_id: String,
    /// Scheduled execution time (RFC3339)
    pub scheduled_time: String,
    /// Actual execution time (RFC3339)
    pub actual_time: String,
}

// ── Queue ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueueTriggerConfig {
    /// Queue topic to subscribe to
    pub topic: String,
    /// Optional function ID to evaluate before invoking handler
    pub condition_function_id: Option<String>,
    /// Queue-specific subscriber configuration
    pub queue_config: Option<Value>,
}

// Queue call request is dynamic (the published message data), no fixed struct.

// ── PubSub (subscribe) ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubscribeTriggerConfig {
    /// Topic to subscribe to
    pub topic: String,
    /// Optional function ID to evaluate before invoking handler
    pub condition_function_id: Option<String>,
}

// Subscribe call request is dynamic (the published event data), no fixed struct.

// ── State ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateTriggerConfig {
    /// State scope to watch (exact match filter)
    pub scope: Option<String>,
    /// State key to watch (exact match filter)
    pub key: Option<String>,
    /// Optional function ID to evaluate before invoking handler
    pub condition_function_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum StateEventType {
    #[serde(rename = "state:created")]
    Created,
    #[serde(rename = "state:updated")]
    Updated,
    #[serde(rename = "state:deleted")]
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateCallRequest {
    /// Always "state"
    #[serde(rename = "type")]
    pub message_type: String,
    /// Type of state change
    pub event_type: StateEventType,
    /// State scope
    pub scope: String,
    /// State key
    pub key: String,
    /// Previous value (null for created events)
    pub old_value: Option<Value>,
    /// New value
    pub new_value: Value,
}

// ── Stream ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamJoinLeaveTriggerConfig {
    /// Stream name to watch
    pub stream_name: Option<String>,
    /// Optional function ID to evaluate before invoking handler
    pub condition_function_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamTriggerConfig {
    /// Stream name to watch
    pub stream_name: Option<String>,
    /// Group ID filter
    pub group_id: Option<String>,
    /// Item ID filter
    pub item_id: Option<String>,
    /// Optional function ID to evaluate before invoking handler
    pub condition_function_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamJoinLeaveCallRequest {
    /// Event type (stream:join or stream:leave)
    pub event_type: String,
    /// Event timestamp (ms)
    pub timestamp: i64,
    /// Stream name
    pub stream_name: String,
    /// Group ID
    pub group_id: String,
    /// Peer ID
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamCallRequest {
    /// Stream event type (create, update, delete, sync)
    pub event_type: String,
    /// Event timestamp (ms)
    pub timestamp: i64,
    /// Stream name
    pub stream_name: String,
    /// Group ID
    pub group_id: String,
    /// Item ID
    pub id: Option<String>,
    /// Event-specific data (create/update/delete/sync payload)
    pub event: Value,
}

// ── Log (observability) ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LogTriggerConfig {
    /// Minimum log level to trigger on
    #[serde(default = "default_log_level")]
    pub level: Option<LogLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum LogLevel {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warn")]
    Warn,
    #[serde(rename = "error")]
    Error,
}

fn default_log_level() -> Option<LogLevel> {
    Some(LogLevel::All)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LogCallRequest {
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
