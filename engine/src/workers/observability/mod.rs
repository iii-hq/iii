// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod configuration;
pub mod logs_layer;
pub mod metrics;
pub mod otel;
pub(crate) mod otlp_exporter;
mod sampler;
pub(crate) mod trace_store;

pub mod config;

use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    sync::Arc,
    time::{Instant, SystemTime},
};

use async_trait::async_trait;
use colored::Colorize;
use function_macros::{function, service};
use futures::Future;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock as TokioRwLock;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
    workers::traits::Worker,
};

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct TracesListInput {
    /// Filter by specific trace ID
    trace_id: Option<String>,
    /// Filter to a specific set of trace IDs (ignored when `trace_id` is set;
    /// used by grouped views to expand one group's members in a single call).
    #[serde(default)]
    trace_ids: Option<Vec<String>>,
    /// Pagination offset (default: 0)
    offset: Option<usize>,
    /// Pagination limit (default: 100)
    limit: Option<usize>,
    /// Filter by service name (case-insensitive substring match)
    service_name: Option<String>,
    /// Filter by span name (case-insensitive substring match)
    name: Option<String>,
    /// Filter by status. For `engine::traces::list`, accepts `error`,
    /// `pending`, `ok`, or `unset` (case-insensitive); other values match no
    /// traces. For `engine::traces::spans`, this remains a case-insensitive
    /// substring match on the raw span status.
    status: Option<String>,
    /// Minimum span duration in milliseconds (sub-ms precision)
    min_duration_ms: Option<f64>,
    /// Maximum span duration in milliseconds (sub-ms precision)
    max_duration_ms: Option<f64>,
    /// Start time in unix timestamp milliseconds (include spans overlapping after this)
    start_time: Option<u64>,
    /// End time in unix timestamp milliseconds (include spans overlapping before this)
    end_time: Option<u64>,
    /// Sort field: "start_time" | "duration" (alias "duration_ms") |
    /// "service_name" | "name" (default: "start_time"). Unknown values fall
    /// back to "start_time".
    sort_by: Option<String>,
    /// Sort order: "asc" | "desc" (default: "asc")
    sort_order: Option<String>,
    /// Filter by span attributes (array of [key, value] pairs, AND logic, exact match)
    attributes: Option<Vec<Vec<String>>>,
    /// Exclude listed rows whose own attributes match ANY [key, value] pair
    /// (OR logic, exact match). Applied to the root/row span itself — hiding
    /// a function hides traces rooted at it, not traces that merely call it.
    #[serde(default)]
    exclude_attributes: Option<Vec<Vec<String>>>,
    /// Include internal engine traces (engine.* functions). Defaults to false.
    #[serde(default)]
    include_internal: Option<bool>,
    /// Search across all spans in each trace, not just root spans.
    /// When true and a `name` filter is set, traces are matched if ANY span
    /// in the trace matches the name filter. Defaults to false.
    #[serde(default)]
    search_all_spans: Option<bool>,
    /// Attribute keys to project onto each trace summary. Only these
    /// arbitrary attributes are returned; full span attributes remain on
    /// `engine::traces::spans` and `engine::traces::tree`.
    #[serde(default)]
    attribute_projection: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Default, JsonSchema)]
pub struct TracesClearInput {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct TracesTreeInput {
    /// Trace ID to build the tree for
    trace_id: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct TracesGroupByInput {
    /// Span attribute key to group by. Spans without this attribute are skipped.
    attribute: String,
    /// Earliest end_time (ms since epoch) to include.
    #[serde(default)]
    since_ms: Option<u64>,
    /// Max groups returned after sorting by `first_seen_ms` descending. Default 100.
    #[serde(default)]
    limit: Option<u32>,
    /// Include engine-internal spans. Defaults to false, matching `traces::list`.
    #[serde(default)]
    include_internal: Option<bool>,
    /// Attribute whose value becomes each group's human-readable `label`
    /// (e.g. group by `iii.session.id` with label `iii.session.name`). The
    /// newest group member carrying the attribute wins, so renames surface.
    #[serde(default)]
    label_attribute: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TraceGroup {
    pub value: String,
    /// Display label resolved from `label_attribute`, when requested and present.
    pub label: Option<String>,
    pub trace_ids: Vec<String>,
    pub span_count: u32,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    pub duration_ms: u32,
    pub error_count: u32,
}

#[derive(Serialize)]
pub struct SpanTreeNode {
    #[serde(flatten)]
    pub span: otel::StoredSpan,
    pub children: Vec<SpanTreeNode>,
}

// =========================================================================
// Response types for the engine::traces / metrics / logs / alerts / sampling
// / health query functions. Typed so engine::functions::info surfaces a
// response_schema (the macro emits no schema for `Option<Value>`). Heavy or
// dynamic leaves (spans, logs, raw metric points) stay `serde_json::Value`:
// their leaf types don't derive JsonSchema, and `to_value(Vec<Leaf>)` equals
// `to_value(Vec<Value>)`, so serialization is byte-identical while the
// envelope schema (the top-level contract an agent needs) is still exposed.
// Field declaration order matches the prior `json!` literals.
// =========================================================================

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TracesListResult {
    /// One compact aggregate per matching trace.
    pub traces: Vec<TraceSummary>,
    /// Total matching traces before pagination.
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    /// Indicates whether the result includes complete durable history.
    pub storage: Value,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TracesSpansResult {
    /// Full stored span records, including attributes, events and links.
    pub spans: Vec<Value>,
    /// Total matching spans before pagination.
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub storage: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TraceSummaryStatus {
    Ok,
    Error,
    Pending,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TraceSummary {
    pub trace_id: String,
    pub name: String,
    pub start_time_unix_nano: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time_unix_nano: Option<u64>,
    pub status: TraceSummaryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub trace_tags: std::collections::BTreeMap<String, String>,
    /// Only keys requested through `attribute_projection` are present.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub attributes: std::collections::BTreeMap<String, String>,
    pub span_count: usize,
    pub error_count: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TracesTreeResult {
    /// Root span-tree nodes (each a serialized, flattened span with nested children).
    pub roots: Vec<Value>,
    pub storage: Value,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TracesGroupByResult {
    pub groups: Vec<TraceGroup>,
    pub storage: Value,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OkResult {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LogsClearResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LogsListResult {
    /// Stored OTEL log records (serialized).
    pub logs: Vec<Value>,
    /// Total matching logs before pagination.
    pub total: usize,
    /// Echo of the applied filters (present only when storage exists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<LogsListQuery>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LogsListQuery {
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub severity_min: Option<i32>,
    pub severity_text: Option<String>,
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MetricsListResult {
    pub engine_metrics: EngineMetricsView,
    /// Stored SDK metric points (serialized).
    pub sdk_metrics: Vec<Value>,
    pub timestamp: i64,
    /// Time-bucketed aggregates (present only when an aggregate_interval was requested and produced data).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aggregated_metrics: Vec<Value>,
    /// Echo of the applied query filters (present only when a filter was supplied).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<MetricsListQuery>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EngineMetricsView {
    pub invocations: InvocationsView,
    pub workers: WorkersView,
    pub performance: PerformanceView,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct InvocationsView {
    pub total: u64,
    pub success: u64,
    pub error: u64,
    pub deferred: u64,
    /// Per-function invocation counts.
    pub by_function: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkersView {
    pub spawns: u64,
    pub deaths: u64,
    pub active: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PerformanceView {
    pub avg_duration_ms: f64,
    pub p50_duration_ms: f64,
    pub p95_duration_ms: f64,
    pub p99_duration_ms: f64,
    pub min_duration_ms: f64,
    pub max_duration_ms: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MetricsListQuery {
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub aggregate_interval: Option<u64>,
    pub metric_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SamplingRulesResult {
    pub traces: SamplingTracesView,
    pub logs: SamplingLogsView,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SamplingTracesView {
    pub default_ratio: f64,
    pub rules: Vec<SamplingRuleView>,
    pub parent_based: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SamplingRuleView {
    // No skip_serializing_if: the prior json! always emitted these (null when unset).
    pub operation: Option<String>,
    pub service: Option<String>,
    pub rate: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SamplingLogsView {
    pub sampling_ratio: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HealthCheckResult {
    pub status: String,
    pub components: HealthComponentsView,
    pub timestamp: i64,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HealthComponentsView {
    /// Each component is `{ status: "healthy"|"disabled", details: <object|null> }`.
    pub otel: Value,
    pub metrics: Value,
    pub logs: Value,
    pub spans: Value,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AlertsListResult {
    /// Current evaluated alert states (serialized).
    pub alerts: Vec<Value>,
    /// Configured alert rules (present when the alert manager is initialized).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<config::AlertRule>>,
    pub firing_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AlertsEvaluateResult {
    pub evaluated: bool,
    /// Alerts triggered by this evaluation pass (present when the manager is initialized).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triggered_alerts: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RollupsListResult {
    /// Pre-aggregated metric rollups (serialized).
    pub rollups: Vec<Value>,
    /// Pre-aggregated histogram rollups (serialized).
    pub histogram_rollups: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<usize>,
    /// "on_the_fly" when computed live because no rollup storage exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<RollupsListQuery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RollupsListQuery {
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub metric_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BaggageGetResult {
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BaggageSetResult {
    pub success: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BaggageGetAllResult {
    pub baggage: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Default, JsonSchema)]
pub struct MetricsListInput {
    /// Start time in Unix timestamp milliseconds
    pub start_time: Option<u64>,
    /// End time in Unix timestamp milliseconds
    pub end_time: Option<u64>,
    /// Filter by metric name
    pub metric_name: Option<String>,
    /// Aggregate interval in seconds
    pub aggregate_interval: Option<u64>,
}

#[derive(Serialize, Deserialize, Default, JsonSchema)]
pub struct LogsListInput {
    /// Start time in Unix timestamp milliseconds
    pub start_time: Option<u64>,
    /// End time in Unix timestamp milliseconds
    pub end_time: Option<u64>,
    /// Filter by trace ID
    pub trace_id: Option<String>,
    /// Filter by span ID
    pub span_id: Option<String>,
    /// Minimum severity number (1-24, higher = more severe)
    pub severity_min: Option<i32>,
    /// Filter by severity text (e.g., "ERROR", "WARN", "INFO")
    pub severity_text: Option<String>,
    /// Pagination offset (default: 0)
    pub offset: Option<usize>,
    /// Maximum number of logs to return
    pub limit: Option<usize>,
}

#[derive(Serialize, Deserialize, Default, JsonSchema)]
pub struct LogsClearInput {}

#[derive(Serialize, Deserialize, Default, JsonSchema)]
pub struct HealthCheckInput {}

#[derive(Serialize, Deserialize, Default, JsonSchema)]
pub struct AlertsListInput {}

#[derive(Serialize, Deserialize, Default, JsonSchema)]
pub struct AlertsEvaluateInput {}

#[derive(Serialize, Deserialize, Default, JsonSchema)]
pub struct RollupsListInput {
    /// Start time in Unix timestamp milliseconds
    pub start_time: Option<u64>,
    /// End time in Unix timestamp milliseconds
    pub end_time: Option<u64>,
    /// Rollup level index (0 = 1 min, 1 = 5 min, 2 = 1 hour)
    pub level: Option<usize>,
    /// Filter by metric name
    pub metric_name: Option<String>,
}

// =============================================================================
// Resource Attributes Helper
// =============================================================================

/// Extract resource attributes from OTEL config for log entries.
///
/// Returns a HashMap containing:
/// - `service.name` - The service name from OTEL config
/// - `service.namespace` - The service namespace (if configured)
/// - `service.version` - The service version (if configured)
/// - `deployment.environment` - From DEPLOYMENT_ENVIRONMENT env var (if set)
fn get_resource_attributes() -> HashMap<String, String> {
    otel::get_otel_config()
        .map(|cfg| {
            let mut map = HashMap::new();
            if let Some(name) = &cfg.service_name {
                map.insert("service.name".to_string(), name.clone());
            }
            if let Some(ns) = &cfg.service_namespace {
                map.insert("service.namespace".to_string(), ns.clone());
            }
            if let Some(ver) = &cfg.service_version {
                map.insert("service.version".to_string(), ver.clone());
            }
            // Add deployment environment if set
            if let Ok(env) = std::env::var("DEPLOYMENT_ENVIRONMENT") {
                map.insert("deployment.environment".to_string(), env);
            }
            map
        })
        .unwrap_or_default()
}

/// Parses an environment variable into type `T`, logging a warning if the value is present but
/// not valid. Returns `None` when the variable is unset or cannot be parsed.
fn parse_env_var<T: std::str::FromStr>(name: &str) -> Option<T> {
    let val = std::env::var(name).ok()?;
    match val.parse() {
        Ok(v) => Some(v),
        Err(_) => {
            tracing::warn!("Invalid value '{}' for {}, ignoring", val, name);
            None
        }
    }
}

fn memory_exporter_not_enabled_error<T>() -> FunctionResult<T, ErrorBody> {
    FunctionResult::Failure(ErrorBody {
        code: "memory_exporter_not_enabled".to_string(),
        message: "In-memory span storage is not available. Set exporter: memory or both in config."
            .to_string(),
        stacktrace: None,
    })
}

fn healthy_component(details: Value) -> Value {
    serde_json::json!({
        "status": "healthy",
        "details": details,
    })
}

// Cheap by design: `status()` stats ~3 files (db + WAL sidecars); a blocking
// task hop would cost more than the fs metadata reads it avoids.
fn trace_storage_status_value() -> Value {
    serde_json::to_value(otel::trace_storage_status()).unwrap_or(Value::Null)
}

/// Run an archive-touching query off the async executor. Every trace-archive
/// read opens a SQLite connection and decodes JSON payloads (and `clear`
/// waits on the writer thread), so handlers must not run them on a tokio
/// worker. JoinError only occurs on closure panic or runtime shutdown; map it
/// to a failure instead of propagating the panic into the handler.
async fn run_blocking_query<T: Send + 'static>(
    operation: &'static str,
    query: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ErrorBody> {
    tokio::task::spawn_blocking(query)
        .await
        .map_err(|join_error| {
            ErrorBody::new(
                "observability_blocking_task_failed",
                format!("{operation} background task failed: {join_error}"),
            )
        })
}

fn disabled_component() -> Value {
    serde_json::json!({
        "status": "disabled",
        "details": null,
    })
}

fn should_trigger_for_level(trigger_level: &str, log_level: &str) -> bool {
    trigger_level == "all" || trigger_level == log_level
}

/// Whether a span matches a `trace` trigger's optional filters. An absent
/// filter matches anything; present filters are ANDed and compared
/// case-insensitively. Mirrors `should_trigger_for_level` for the log trigger.
fn should_trigger_for_span(
    config_service: Option<&str>,
    config_status: Option<&str>,
    span: &otel::StoredSpan,
) -> bool {
    if let Some(service) = config_service
        && !span.service_name.eq_ignore_ascii_case(service)
    {
        return false;
    }
    if let Some(status) = config_status
        && !span.status.eq_ignore_ascii_case(status)
    {
        return false;
    }
    true
}

/// The `function_id` attribute a span was produced under, if any.
fn span_function_id(span: &otel::StoredSpan) -> Option<&str> {
    span.attributes
        .iter()
        .find(|(k, _)| k == "function_id")
        .map(|(_, v)| v.as_str())
}

/// Engine-internal / plumbing spans, excluded from the `trace` trigger the
/// same way `traces::list` hides them by default (`iii.function.kind=internal`
/// or an `engine::` function id). Keeps the trigger focused on real work and
/// is the first half of breaking the trigger's feedback loop.
fn is_internal_span(span: &otel::StoredSpan) -> bool {
    span.attributes.iter().any(|(k, v)| {
        (k == "iii.function.kind" && v == "internal")
            || (k == "function_id" && v.starts_with("engine::"))
    })
}

fn span_attribute<'a>(span: &'a otel::StoredSpan, key: &str) -> Option<&'a str> {
    span.attributes
        .iter()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, value)| value.as_str())
}

fn is_trace_tag_attribute(key: &str) -> bool {
    key.starts_with("iii.tag.")
        || matches!(
            key,
            "iii.session.id" | "iii.session.name" | "iii.message.id"
        )
}

fn representative_trace_span(spans: &[otel::StoredSpan]) -> Option<&otel::StoredSpan> {
    let present_span_ids: HashSet<&str> = spans.iter().map(|span| span.span_id.as_str()).collect();
    spans
        .iter()
        .filter(|span| {
            span.parent_span_id
                .as_deref()
                .is_none_or(|parent| !present_span_ids.contains(parent))
        })
        .min_by(|a, b| {
            a.start_time_unix_nano
                .cmp(&b.start_time_unix_nano)
                .then_with(|| a.span_id.cmp(&b.span_id))
        })
        .or_else(|| {
            spans.iter().min_by(|a, b| {
                a.start_time_unix_nano
                    .cmp(&b.start_time_unix_nano)
                    .then_with(|| a.span_id.cmp(&b.span_id))
            })
        })
}

fn ordered_trace_attributes(
    spans: &[otel::StoredSpan],
    include: impl Fn(&str) -> bool,
) -> std::collections::BTreeMap<String, String> {
    let mut ordered: Vec<_> = spans.iter().collect();
    ordered.sort_by(|a, b| {
        a.start_time_unix_nano
            .cmp(&b.start_time_unix_nano)
            .then_with(|| a.span_id.cmp(&b.span_id))
    });
    let mut attributes = std::collections::BTreeMap::new();
    for span in ordered {
        for (key, value) in &span.attributes {
            if include(key) {
                attributes.insert(key.clone(), value.clone());
            }
        }
    }
    attributes
}

fn summarize_trace(
    spans: &[otel::StoredSpan],
    projection: &HashSet<String>,
) -> Option<TraceSummary> {
    let representative = representative_trace_span(spans)?;
    let trace_tags = ordered_trace_attributes(spans, is_trace_tag_attribute);
    let attributes = ordered_trace_attributes(spans, |key| projection.contains(key));
    let error_count = spans
        .iter()
        .filter(|span| span.status.eq_ignore_ascii_case("error"))
        .count();
    let outcome_is_error = trace_tags
        .get("iii.tag.outcome")
        .is_some_and(|outcome| matches!(outcome.as_str(), "failed" | "error"));
    let has_pending = spans.iter().any(|span| span.pending);
    let status = if error_count > 0 || outcome_is_error {
        TraceSummaryStatus::Error
    } else if has_pending {
        TraceSummaryStatus::Pending
    } else {
        TraceSummaryStatus::Ok
    };
    let start_time_unix_nano = spans
        .iter()
        .map(|span| span.start_time_unix_nano)
        .min()
        .unwrap_or(representative.start_time_unix_nano);
    let end_time_unix_nano = if has_pending {
        None
    } else {
        spans.iter().map(|span| span.end_time_unix_nano).max()
    };

    Some(TraceSummary {
        trace_id: representative.trace_id.clone(),
        name: representative.name.clone(),
        start_time_unix_nano,
        end_time_unix_nano,
        status,
        service_name: (!representative.service_name.is_empty())
            .then(|| representative.service_name.clone()),
        function_id: span_attribute(representative, "faas.invoked_name")
            .or_else(|| span_attribute(representative, "function_id"))
            .map(str::to_string),
        topic: span_attribute(representative, "messaging.destination.name").map(str::to_string),
        trace_tags,
        attributes,
        span_count: spans.len(),
        error_count,
    })
}

fn span_matches_attribute_pairs(span: &otel::StoredSpan, pairs: &[Vec<String>]) -> bool {
    pairs.iter().all(|pair| {
        pair.len() == 2
            && span
                .attributes
                .iter()
                .any(|(key, value)| key == &pair[0] && value == &pair[1])
    })
}

/// Apply filters whose trace-summary semantics are determined entirely by the
/// representative root. Returning `true` only means that the trace remains a
/// candidate: aggregate status, duration/time and child-span searches are
/// still evaluated after the candidate traces have been loaded in full.
fn trace_might_match_root_filters(
    root_spans: &[otel::StoredSpan],
    input: &TracesListInput,
    include_internal: bool,
) -> bool {
    let Some(representative) = representative_trace_span(root_spans) else {
        return false;
    };

    // After internal rows are removed, a non-internal child can become the
    // representative root. Keep these traces for the exact post-load pass.
    if !include_internal && is_internal_span(representative) {
        return true;
    }

    if let Some(service_name) = input.service_name.as_deref()
        && !representative
            .service_name
            .to_lowercase()
            .contains(&service_name.to_lowercase())
    {
        return false;
    }

    let search_all = input.search_all_spans.unwrap_or(false);
    if !search_all {
        if let Some(name) = input.name.as_deref()
            && !representative
                .name
                .to_lowercase()
                .contains(&name.to_lowercase())
        {
            return false;
        }
        if let Some(pairs) = input.attributes.as_deref()
            && !span_matches_attribute_pairs(representative, pairs)
        {
            return false;
        }
    }

    if let Some(excluded) = input.exclude_attributes.as_deref()
        && excluded.iter().any(|pair| {
            pair.len() == 2
                && representative
                    .attributes
                    .iter()
                    .any(|(key, value)| key == &pair[0] && value == &pair[1])
        })
    {
        return false;
    }

    true
}

fn trace_matches_list_filters(
    summary: &TraceSummary,
    spans: &[otel::StoredSpan],
    input: &TracesListInput,
    now_ns: u64,
) -> bool {
    let Some(representative) = representative_trace_span(spans) else {
        return false;
    };
    let search_all = input.search_all_spans.unwrap_or(false);

    if let Some(service_name) = input.service_name.as_deref()
        && !summary
            .service_name
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(&service_name.to_lowercase())
    {
        return false;
    }
    if let Some(name) = input.name.as_deref() {
        let needle = name.to_lowercase();
        let matches = if search_all {
            spans
                .iter()
                .any(|span| span.name.to_lowercase().contains(&needle))
        } else {
            summary.name.to_lowercase().contains(&needle)
        };
        if !matches {
            return false;
        }
    }
    if let Some(status) = input.status.as_deref() {
        let matches = match status.to_lowercase().as_str() {
            "error" => matches!(summary.status, TraceSummaryStatus::Error),
            "pending" => matches!(summary.status, TraceSummaryStatus::Pending),
            "ok" => matches!(summary.status, TraceSummaryStatus::Ok),
            "unset" => {
                matches!(summary.status, TraceSummaryStatus::Ok)
                    && representative.status.eq_ignore_ascii_case("unset")
            }
            _ => false,
        };
        if !matches {
            return false;
        }
    }

    let effective_end = summary.end_time_unix_nano.unwrap_or(now_ns);
    let duration_ms =
        effective_end.saturating_sub(summary.start_time_unix_nano) as f64 / 1_000_000.0;
    if input
        .min_duration_ms
        .is_some_and(|minimum| duration_ms < minimum)
        || input
            .max_duration_ms
            .is_some_and(|maximum| duration_ms > maximum)
    {
        return false;
    }
    if input
        .start_time
        .is_some_and(|start_ms| effective_end < start_ms.saturating_mul(1_000_000))
        || input
            .end_time
            .is_some_and(|end_ms| summary.start_time_unix_nano > end_ms.saturating_mul(1_000_000))
    {
        return false;
    }

    if let Some(pairs) = input.attributes.as_deref() {
        let matches = if search_all {
            spans
                .iter()
                .any(|span| span_matches_attribute_pairs(span, pairs))
        } else {
            span_matches_attribute_pairs(representative, pairs)
        };
        if !matches {
            return false;
        }
    }
    if let Some(excluded) = input.exclude_attributes.as_deref()
        && excluded.iter().any(|pair| {
            pair.len() == 2
                && representative
                    .attributes
                    .iter()
                    .any(|(key, value)| key == &pair[0] && value == &pair[1])
        })
    {
        return false;
    }

    true
}

// =============================================================================
// OpenTelemetry Module
// =============================================================================

/// Trigger type ID for log events from the observability module
pub const LOG_TRIGGER_TYPE: &str = "log";

/// Log triggers for OTEL module
pub struct OtelLogTriggers {
    pub triggers: Arc<TokioRwLock<HashSet<Trigger>>>,
}

impl Default for OtelLogTriggers {
    fn default() -> Self {
        Self::new()
    }
}

impl OtelLogTriggers {
    pub fn new() -> Self {
        Self {
            triggers: Arc::new(TokioRwLock::new(HashSet::new())),
        }
    }
}

/// Trigger type ID for span/trace events from the observability module
pub const TRACE_TRIGGER_TYPE: &str = "trace";

/// Trailing-edge coalesce window for the `trace` trigger fan-out. Spans arrive
/// batched and at high volume; collapsing a burst into one tick keeps the
/// fan-out (and the spans its own delivery produces) to a trickle.
const TRACE_COALESCE_MS: u64 = 300;

/// Trace (span) triggers for the OTEL module
pub struct OtelTraceTriggers {
    pub triggers: Arc<TokioRwLock<HashSet<Trigger>>>,
}

impl Default for OtelTraceTriggers {
    fn default() -> Self {
        Self::new()
    }
}

impl OtelTraceTriggers {
    pub fn new() -> Self {
        Self {
            triggers: Arc::new(TokioRwLock::new(HashSet::new())),
        }
    }
}

/// Input for OTEL log functions (log.info, log.warn, log.error)
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct OtelLogInput {
    /// Optional trace ID for correlation
    trace_id: Option<String>,
    /// Optional span ID for correlation
    span_id: Option<String>,
    /// The log message
    message: String,
    /// Additional structured data/attributes
    data: Option<Value>,
    /// Service name (defaults to function name if not provided)
    service_name: Option<String>,
}

/// Input for baggage.get function
#[derive(Serialize, Deserialize, Default, JsonSchema)]
pub struct BaggageGetInput {
    /// The baggage key to retrieve
    pub key: String,
}

/// Input for baggage.set function
#[derive(Serialize, Deserialize, Default, JsonSchema)]
pub struct BaggageSetInput {
    /// The baggage key to set
    pub key: String,
    /// The baggage value to set
    pub value: String,
}

/// Input for baggage.getAll function (empty)
#[derive(Serialize, Deserialize, Default, JsonSchema)]
pub struct BaggageGetAllInput {}

/// OpenTelemetry configuration module.
/// This module provides OTEL-native logging, traces, metrics, and logs access.
/// It sets the global OTEL configuration from YAML before logging is initialized.
#[derive(Clone)]
pub struct ObservabilityWorker {
    /// The config.yaml block passed to `create()`, or
    /// [`config::ObservabilityWorkerConfig::default`] when auto-injected.
    /// Used as the seed for first-time `configuration::register` and as the
    /// fetch fallback; the configuration worker entry is the runtime source
    /// of truth afterwards. May still contain `${VAR:default}` templates —
    /// the live OTEL global is published with those expanded.
    _config: config::ObservabilityWorkerConfig,
    triggers: Arc<OtelLogTriggers>,
    trace_triggers: Arc<OtelTraceTriggers>,
    engine: Arc<Engine>,
    /// Shutdown signal sender for background tasks
    shutdown_tx: Arc<tokio::sync::watch::Sender<bool>>,
    /// The live worker shutdown receiver, stored by `start_background_tasks`
    /// so `apply_config` can hand respawned tasks the same lifecycle. `None`
    /// until started / after destroy — task rebuilds are refused then (the
    /// other apply tiers still run).
    worker_shutdown_rx: Arc<std::sync::Mutex<Option<tokio::sync::watch::Receiver<bool>>>>,
    /// Stop signal for the current log-retention task instance (respawned on
    /// `logs_retention_seconds` changes).
    logs_retention_stop: Arc<std::sync::Mutex<Option<tokio::sync::watch::Sender<bool>>>>,
    /// Stop signal for the current OTLP logs exporter task instance
    /// (respawned on exporter/batch/flush/endpoint/identity changes).
    logs_exporter_stop: Arc<std::sync::Mutex<Option<tokio::sync::watch::Sender<bool>>>>,
    /// Stop signal for the current log-trigger subscriber task instance.
    /// Respawned when `logs_enabled` flips false->true at runtime so the `log`
    /// trigger fan-out reactivates without an engine restart.
    logs_trigger_stop: Arc<std::sync::Mutex<Option<tokio::sync::watch::Sender<bool>>>>,
    /// Serializes concurrent `apply_config` runs (rapid configuration edits).
    apply_lock: Arc<tokio::sync::Mutex<()>>,
}

/// A compiled [`config::SpanCollapseRule`] with precompiled regex patterns.
struct CompiledCollapseRule {
    name: regex::Regex,
    service: Option<regex::Regex>,
}

impl CompiledCollapseRule {
    fn matches(&self, name: &str, service: &str) -> bool {
        self.name.is_match(name) && self.service.as_ref().is_none_or(|s| s.is_match(service))
    }
}

/// Convert a `*`/`?` wildcard pattern into an anchored regex (mirrors the
/// sampler's wildcard handling).
fn collapse_wildcard_to_regex(pattern: &str) -> Result<regex::Regex, regex::Error> {
    let escaped = regex::escape(pattern);
    let regex_pattern = escaped.replace(r"\*", ".*").replace(r"\?", ".");
    regex::Regex::new(&format!("^{}$", regex_pattern))
}

/// True for the engine-internal trigger fan-out wrapper spans
/// (`state_triggers`/`stream_triggers`).
fn is_trigger_wrapper(name: &str) -> bool {
    name == "state_triggers" || name == "stream_triggers"
}

/// Drop NO-OP trigger fan-out wrappers from the assembled tree: a
/// `state_triggers`/`stream_triggers` span with no children fanned out to a
/// handler that produced nothing traceable (e.g. suppressed observability
/// consumers) — pure noise, and a turn step emits many. Wrappers that DID invoke
/// a handler are kept, so the "ran because of a state/stream write" causality
/// stays visible. Iterates to a fixpoint so a wrapper left childless by pruning a
/// nested wrapper also drops. Childless spans have nothing to reparent, so the
/// tree stays connected without rewriting any parent links.
fn prune_empty_trigger_spans(mut spans: Vec<otel::StoredSpan>) -> Vec<otel::StoredSpan> {
    loop {
        let has_child: std::collections::HashSet<String> = spans
            .iter()
            .filter_map(|s| s.parent_span_id.clone())
            .collect();
        let before = spans.len();
        spans.retain(|s| !(is_trigger_wrapper(&s.name) && !has_child.contains(&s.span_id)));
        if spans.len() == before {
            break;
        }
    }
    spans
}

/// Compiled collapse rules for the global config, cached after first use and
/// recompiled by `refresh_collapse_rules` when the configuration-worker
/// apply path changes them. Callers on the hot path (one per coalesce tick /
/// REST request) must not recompile the regexes each time. Returns an empty
/// set — without poisoning the cache — if the config is not set yet.
static COLLAPSE_RULES: std::sync::RwLock<Option<Arc<Vec<CompiledCollapseRule>>>> =
    std::sync::RwLock::new(None);

fn cached_collapse_rules() -> Arc<Vec<CompiledCollapseRule>> {
    {
        let cached = COLLAPSE_RULES
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(rules) = cached.as_ref() {
            return rules.clone();
        }
    }
    match otel::get_otel_config() {
        Some(config) => {
            let compiled = Arc::new(compile_collapse_rules(&config.collapse_spans));
            let mut cached = COLLAPSE_RULES
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            cached.get_or_insert_with(|| compiled.clone()).clone()
        }
        None => Arc::new(Vec::new()),
    }
}

/// Recompile the collapse-rule cache from the given rules (configuration
/// apply path).
pub(crate) fn refresh_collapse_rules(rules: &[config::SpanCollapseRule]) {
    let compiled = Arc::new(compile_collapse_rules(rules));
    *COLLAPSE_RULES
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(compiled);
}

/// Compile the configured collapse rules, skipping any with invalid patterns.
fn compile_collapse_rules(rules: &[config::SpanCollapseRule]) -> Vec<CompiledCollapseRule> {
    rules
        .iter()
        .filter_map(|r| {
            let name = collapse_wildcard_to_regex(&r.name).ok()?;
            let service = match &r.service {
                Some(p) => Some(collapse_wildcard_to_regex(p).ok()?),
                None => None,
            };
            Some(CompiledCollapseRule { name, service })
        })
        .collect()
}

/// Remove spans matching any collapse rule, reparenting each surviving span to
/// its nearest non-collapsed ancestor so the trace tree stays connected.
///
/// Operates on the full set of spans for a trace, so reparenting is exact
/// regardless of span arrival order. Raw spans in storage / exported to the
/// collector are untouched — this only affects the assembled tree view.
fn collapse_spans(
    spans: Vec<otel::StoredSpan>,
    rules: &[CompiledCollapseRule],
) -> Vec<otel::StoredSpan> {
    if rules.is_empty() {
        return spans;
    }

    let collapsed: std::collections::HashSet<String> = spans
        .iter()
        .filter(|s| rules.iter().any(|r| r.matches(&s.name, &s.service_name)))
        .map(|s| s.span_id.clone())
        .collect();

    if collapsed.is_empty() {
        return spans;
    }

    let parent_of: HashMap<String, Option<String>> = spans
        .iter()
        .map(|s| (s.span_id.clone(), s.parent_span_id.clone()))
        .collect();

    // Walk up the chain of collapsed ancestors to the first survivor (or root).
    let resolve = |start: Option<String>| -> Option<String> {
        let mut pid = start;
        let mut guard = 0usize;
        while let Some(id) = pid.clone() {
            if !collapsed.contains(&id) {
                break;
            }
            pid = parent_of.get(&id).cloned().flatten();
            guard += 1;
            if guard > 100_000 {
                break; // cycle guard
            }
        }
        pid
    };

    spans
        .into_iter()
        .filter(|s| !collapsed.contains(&s.span_id))
        .map(|mut s| {
            s.parent_span_id = resolve(s.parent_span_id.take());
            s
        })
        .collect()
}

/// One pipeline for turning a raw stored trace into its presentable span
/// set: prune trigger wrappers that produced nothing, then apply the
/// configured collapse rules. `traces::tree` goes through this one function
/// so alternate consumers cannot drift from it.
fn correct_trace_spans(
    spans: Vec<otel::StoredSpan>,
    rules: &[CompiledCollapseRule],
) -> Vec<otel::StoredSpan> {
    collapse_spans(prune_empty_trigger_spans(spans), rules)
}

fn build_span_tree(spans: Vec<otel::StoredSpan>) -> Vec<SpanTreeNode> {
    // Span ids present in this set. A span whose parent is NOT present is a
    // local trace root — covers traces entering iii from an external caller via
    // an incoming `traceparent`, whose server span points at the remote caller's
    // span (never stored here). Without this the whole subtree is orphaned and
    // the trace detail view renders nothing.
    let present_ids: std::collections::HashSet<String> =
        spans.iter().map(|s| s.span_id.clone()).collect();
    let mut children_map: HashMap<String, Vec<otel::StoredSpan>> = HashMap::new();
    let mut roots: Vec<otel::StoredSpan> = Vec::new();

    for span in spans {
        match &span.parent_span_id {
            Some(parent_id) if present_ids.contains(parent_id) => {
                children_map
                    .entry(parent_id.clone())
                    .or_default()
                    .push(span);
            }
            _ => roots.push(span),
        }
    }

    roots
        .into_iter()
        .map(|root| build_span_tree_node(root, &mut children_map))
        .collect()
}

fn build_span_tree_node(
    span: otel::StoredSpan,
    children_map: &mut HashMap<String, Vec<otel::StoredSpan>>,
) -> SpanTreeNode {
    let children = children_map
        .remove(&span.span_id)
        .unwrap_or_default()
        .into_iter()
        .map(|child| build_span_tree_node(child, children_map))
        .collect();

    SpanTreeNode { span, children }
}

#[service(name = "otel")]
impl ObservabilityWorker {
    /// The authoritative log-storage capacity: the live global configuration
    /// (kept current by the configuration-worker apply path) with the yaml
    /// seed as fallback. Passing the seed directly would revert a runtime
    /// edit on the next lazy re-init.
    fn effective_logs_max_count(&self) -> Option<usize> {
        otel::get_otel_config()
            .and_then(|c| c.logs_max_count)
            .or(self._config.logs_max_count)
    }

    fn from_config(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Self> {
        let otel_config: config::ObservabilityWorkerConfig = match config {
            Some(cfg) => serde_json::from_value(cfg)?,
            None => config::ObservabilityWorkerConfig::default(),
        };
        let otel_config = otel_config.normalized();

        // Seed the global OTEL config so logging can use it. Expand
        // `${VAR:default}` placeholders for the live snapshot; `_config`
        // keeps the template form so first-boot `configuration::register`
        // can seed it. On the serve path the global is already populated
        // during logging init; first-set semantics keep that value
        // authoritative.
        if !otel::set_otel_config(otel_config.clone().with_env_expanded()) {
            tracing::debug!(
                "ObservabilityWorker created with the global config already set; keeping it"
            );
        }

        let (shutdown_tx, _) = tokio::sync::watch::channel(false);

        Ok(ObservabilityWorker {
            _config: otel_config,
            triggers: Arc::new(OtelLogTriggers::new()),
            trace_triggers: Arc::new(OtelTraceTriggers::new()),
            engine,
            shutdown_tx: Arc::new(shutdown_tx),
            worker_shutdown_rx: Arc::new(std::sync::Mutex::new(None)),
            logs_retention_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_exporter_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_trigger_stop: Arc::new(std::sync::Mutex::new(None)),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    /// Construct a worker from a raw config value — mirrors
    /// `ConfigurationWorker::for_test` so integration tests in `engine/tests/`
    /// can drive the concrete worker without booting the full engine.
    #[doc(hidden)]
    pub fn for_test(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Self> {
        Self::from_config(engine, config)
    }

    /// The live effective configuration: the global snapshot kept current by
    /// the configuration-worker apply path, with the yaml seed as fallback.
    pub fn current_config(&self) -> config::ObservabilityWorkerConfig {
        otel::get_otel_config()
            .map(|cfg| (*cfg).clone())
            .unwrap_or_else(|| self._config.clone())
    }

    /// True while the worker is started and has not been destroyed.
    ///
    /// `worker_shutdown_rx` is set at the top of `start_background_tasks`
    /// (before the change trigger that drives `on_config_change` is registered)
    /// and cleared by `destroy`, so a deferred apply — the timeout retry in
    /// `on_config_change` — checks this before touching process-global
    /// telemetry state. A retry that fires after the owning worker was torn
    /// down becomes a no-op instead of mutating globals on behalf of a worker
    /// that no longer exists.
    pub(crate) fn is_active(&self) -> bool {
        self.worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned")
            .is_some()
    }

    /// Register the `iii-observability::on-config-change` handler. Idempotent
    /// (replace-by-id), so it is safe to call from both `register_functions`
    /// (which runs inside the worker scope for destroy/reload cleanup) and
    /// `start_background_tasks` (which registers the trigger and runs first
    /// on reload).
    fn register_config_handler(&self, engine: &Arc<Engine>) {
        let worker = self.clone();
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: configuration::CONFIG_FN_ID.to_string(),
                description: Some(
                    "Internal: re-apply the iii-observability configuration when the \
                     authoritative configuration entry changes."
                        .to_string(),
                ),
                request_format: None,
                response_format: None,
                metadata: Some(serde_json::json!({ "internal": true })),
            },
            crate::engine::Handler::new(move |_payload: Value| {
                let worker = worker.clone();
                async move {
                    configuration::on_config_change(&worker).await;
                    crate::function::FunctionResult::Success(Some(
                        serde_json::json!({ "ok": true }),
                    ))
                }
            }),
        );
    }

    /// Fetch the authoritative configuration and apply it per field tier.
    ///
    /// - LIVE: the global config snapshot is swapped unconditionally —
    ///   per-use readers (ingest gates, `logs_console_output`,
    ///   `logs_sampling_ratio`, resource attributes) pick it up immediately.
    /// - LIMITS: storage capacities/retention are re-applied unconditionally
    ///   (idempotent atomics), so a catch-up apply converges even when the
    ///   value itself did not change.
    /// - SWAP: sampler, alert rules, collapse-rule cache, the engine log
    ///   level, and the durable trace archive (`trace_storage.enabled` /
    ///   `directory` replace the store live, backfilling the hot cache) are
    ///   rebuilt only when their fields changed.
    /// - TASK-REBUILD: the log-retention, OTLP-logs-exporter, and log-trigger
    ///   subscriber tasks are respawned when their captured settings changed or
    ///   when `logs_enabled` flips false->true (which revives the store in the
    ///   LIMITS tier); refused (warn) when the worker has not started or was
    ///   destroyed.
    /// - RESTART-ONLY: trace exporter wiring, resource identity on traces,
    ///   pipeline enablement, metrics exporter, and the log format are baked
    ///   in at process start; changes are reported with a warning and take
    ///   effect at the next engine start (the persisted entry is read at
    ///   boot).
    pub(crate) async fn apply_config(&self) -> anyhow::Result<()> {
        let _guard = self.apply_lock.lock().await;
        if !self.is_active() {
            tracing::debug!(
                "iii-observability: worker no longer active; skipping configuration apply"
            );
            return Ok(());
        }

        let old = self.current_config();
        let new = tokio::time::timeout(
            configuration::CONFIG_BUS_TIMEOUT,
            configuration::fetch_config(self.engine.as_ref(), &old),
        )
        .await
        .map_err(|elapsed| anyhow::Error::new(elapsed).context("configuration::get timed out"))??;

        // Apply the engine log level BEFORE publishing the global snapshot, so
        // current_config() never advertises a level we failed to install. On a
        // failed reload we keep the previous level in the published config,
        // leaving old.level != new.level so the next apply retries instead of
        // silently masking the drift.
        let mut effective = new.clone();
        if old.level != new.level {
            match &new.level {
                Some(level) => {
                    if let Err(err) = crate::logging::reload_log_level(level) {
                        tracing::warn!(
                            error = %err,
                            "iii-observability: log level not applied; keeping current level"
                        );
                        effective.level = old.level.clone();
                    }
                }
                // Removing the key does not revert to a default: the boot
                // level may have come from env/CLI, not this entry.
                None => tracing::debug!(
                    "iii-observability: level removed from configuration; keeping current level"
                ),
            }
        }

        // LIVE tier: swap the global snapshot (carrying the level we actually
        // installed, per the reload above).
        otel::update_otel_config(effective);

        // SWAP tier (trace archive): `enabled`/`directory` changes replace the
        // durable store live. Runs BEFORE the LIMITS tier so `update_limits`
        // and the hot-cache dirty-protection logic see the post-swap archive.
        // A `None` block never swaps: legacy memory-only configs keep their
        // boot behavior. Limits-only edits skip this and take the cheap
        // `update_limits` path below.
        let swap_key = |storage: &Option<config::TraceStorageConfig>| {
            storage
                .as_ref()
                .map(|storage| (storage.enabled, storage.directory.clone()))
        };
        if new.trace_storage.is_some()
            && swap_key(&old.trace_storage) != swap_key(&new.trace_storage)
        {
            if otel::get_span_storage().is_some() {
                let next = new.trace_storage.clone();
                let enabled = next.as_ref().is_some_and(|storage| storage.enabled);
                let directory = next
                    .as_ref()
                    .map(|storage| storage.directory.clone())
                    .unwrap_or_default();
                // Shutting the previous store down drains dirty spans and can
                // block for seconds; keep it off the async executor.
                tokio::task::spawn_blocking(move || {
                    otel::configure_trace_storage(next);
                    if let Some(hot) = otel::get_span_storage() {
                        otel::attach_trace_disk_storage(&hot);
                        if let Some(archive) = otel::get_trace_disk_storage() {
                            // The old store's shutdown drain emptied the dirty
                            // set; re-mark the hot cache so the new archive
                            // backfills instead of starting empty.
                            hot.mark_all_dirty();
                            archive.notify();
                        }
                    }
                })
                .await
                .map_err(|join_error| {
                    anyhow::anyhow!("trace archive reconfigure task panicked: {join_error}")
                })?;
                tracing::info!(
                    enabled,
                    directory = %directory,
                    "iii-observability: trace archive reconfigured"
                );
            } else {
                tracing::debug!(
                    "iii-observability: span pipeline not initialized; trace storage \
                     changes apply at the next engine start"
                );
            }
        }

        // LIMITS tier: idempotent, applied unconditionally.
        if let Some(storage) = otel::get_span_storage()
            && let Some(max) = new.memory_max_spans
        {
            storage.set_max_spans(max);
        }
        if let Some(storage) = otel::get_span_storage()
            && let Some(trace_storage) = &new.trace_storage
        {
            storage.set_low_watermark_ratio(trace_storage.memory_low_watermark_ratio);
            storage.set_max_bytes(trace_storage.memory_max_bytes);
        }
        // No `enabled` guard: after the swap tier a disabled config leaves no
        // installed archive, so the `get_trace_disk_storage()` gate suffices.
        if let Some(archive) = otel::get_trace_disk_storage()
            && let Some(trace_storage) = &new.trace_storage
        {
            archive.update_limits(trace_storage);
        }
        // Only retune the log store; never CREATE it when logs are disabled —
        // initialize() deliberately skips log storage in that case and the
        // ingest path must not lazily revive it. init_log_storage is
        // update-if-exists, so this retunes an enabled store and no-ops when
        // logs are off and the store was never created.
        if otel::logs_enabled(Some(&new)) {
            otel::init_log_storage(new.logs_max_count);
        }
        // Retune the metric store only when it already exists; never CREATE it
        // here. Unlike init_log_storage, init_metric_storage builds a store when
        // absent, and `metrics_enabled` is restart-tier — so a worker that
        // booted with metrics off (no store) must not lazily acquire one on an
        // unrelated config edit. The boot store is built by init_metrics().
        if metrics::get_metric_storage().is_some() {
            metrics::init_metric_storage(new.metrics_max_count, new.metrics_retention_seconds);
        }

        // SWAP tier: rebuild only what changed.
        if old.collapse_spans != new.collapse_spans {
            refresh_collapse_rules(&new.collapse_spans);
            tracing::info!("iii-observability: span collapse rules recompiled");
        }
        if old.alerts != new.alerts {
            match metrics::get_alert_manager() {
                Some(manager) => {
                    manager.update_rules(new.alerts.clone());
                    tracing::info!(
                        rules = new.alerts.len(),
                        "iii-observability: alert rules replaced"
                    );
                }
                None => tracing::warn!(
                    "iii-observability: alert manager not initialized; alert changes \
                     apply at the next engine start"
                ),
            }
        }
        if old.sampling != new.sampling
            || old.sampling_ratio != new.sampling_ratio
            || old.service_name != new.service_name
        {
            otel::refresh_sampler();
            tracing::info!("iii-observability: sampler rebuilt");
        }

        // TASK-REBUILD tier.
        //
        // `logs_enabled` false->true revives the log store in the LIMITS tier
        // above, but the log-trigger subscriber, OTLP exporter, and retention
        // task all bailed at boot when the store was absent. Treat that
        // transition as a respawn trigger for all three so the `log` trigger
        // fan-out and OTLP export reactivate without an engine restart. Only
        // the false->true edge fires this (a true->false edge is handled by
        // the per-call ingest gate, leaving the idle tasks as-is, matching the
        // prior behavior).
        let logs_reenabled = otel::logs_enabled(Some(&new)) && !otel::logs_enabled(Some(&old));
        let respawn_retention =
            old.logs_retention_seconds != new.logs_retention_seconds || logs_reenabled;
        // `endpoint` / `service_name` / `service_version` are deliberately NOT
        // respawn triggers: they are restart-tier for the trace exporter, and
        // rebuilding only the logs exporter against a new endpoint/identity
        // would split logs onto the new collector while traces stay on the old
        // one until restart. Keeping them restart-tier moves every signal
        // together at the next boot (see the restart-tier warning below).
        let respawn_exporter = logs_reenabled
            || old.logs_exporter != new.logs_exporter
            || old.logs_batch_size != new.logs_batch_size
            || old.logs_flush_interval_ms != new.logs_flush_interval_ms;
        if respawn_retention || respawn_exporter {
            let started = self.is_active();
            // Only (re)spawn when the worker is started AND enabled: a
            // disabled worker runs no log tasks, and `enabled` is
            // restart-tier, so a config change must not start them mid-life.
            if started && new.enabled.unwrap_or(true) {
                if respawn_retention {
                    self.spawn_logs_retention(&new);
                }
                if respawn_exporter {
                    self.spawn_logs_exporter(&new);
                }
                if logs_reenabled {
                    self.spawn_log_trigger_subscriber();
                    tracing::info!(
                        "iii-observability: logs re-enabled; log trigger subscriber and \
                         exporter reactivated"
                    );
                }
            } else if started {
                tracing::debug!(
                    "iii-observability: observability disabled; log exporter/retention \
                     changes apply at the next engine start"
                );
            } else {
                tracing::warn!(
                    "iii-observability: background tasks not running; log exporter/retention \
                     changes apply when the worker starts"
                );
            }
        }

        // RESTART-ONLY tier: report what will only apply at the next boot.
        let mut restart_fields = Vec::new();
        if old.enabled != new.enabled {
            restart_fields.push("enabled (pipeline construction; ingest gate applies live)");
        }
        if old.exporter != new.exporter {
            restart_fields.push("exporter");
        }
        if old.endpoint != new.endpoint {
            restart_fields.push("endpoint (trace + logs exporters)");
        }
        if old.service_name != new.service_name
            || old.service_version != new.service_version
            || old.service_namespace != new.service_namespace
        {
            restart_fields.push("service identity (trace resource + logs exporter)");
        }
        if old.format != new.format {
            restart_fields.push("format");
        }
        if old.metrics_enabled != new.metrics_enabled {
            restart_fields.push("metrics_enabled");
        }
        if old.metrics_exporter != new.metrics_exporter {
            restart_fields.push("metrics_exporter");
        }
        if !restart_fields.is_empty() {
            tracing::warn!(
                fields = ?restart_fields,
                "iii-observability: restart-tier fields changed; they apply at the next \
                 engine start (the stored entry is read at boot)"
            );
        }

        Ok(())
    }

    /// (Re)spawn the log-trigger subscriber that fans `log` ingest events out
    /// to registered `log` triggers, stopping any previous instance. Returns
    /// early (after parking the stop sender) when log storage has not been
    /// created — so a `logs_enabled` false->true toggle, which creates storage
    /// in the LIMITS tier, can respawn this without an engine restart. Follows
    /// both its per-instance stop signal and the worker shutdown signal.
    fn spawn_log_trigger_subscriber(&self) {
        // Verify the prerequisites first so we never stop the previous
        // subscriber without replacing it. A `logs_enabled` false->true toggle
        // creates log storage in the LIMITS tier before this is called, so the
        // early returns here are only hit when the worker is destroyed or the
        // store was never initialized.
        let Some(storage) = otel::get_log_storage() else {
            tracing::debug!(
                "[ObservabilityWorker] Log storage not available; log trigger subscriber not started"
            );
            return;
        };
        let Some(mut shutdown_rx) = self
            .worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned")
            .clone()
        else {
            return;
        };

        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        // Subscribe BEFORE replacing the old stop sender so the handoff window
        // carries bounded duplicates (the broadcast reaches every live receiver
        // and `log` trigger delivery is at-least-once) rather than dropped logs.
        let mut rx = storage.subscribe();
        let previous = self
            .logs_trigger_stop
            .lock()
            .expect("logs_trigger_stop mutex poisoned")
            .replace(stop_tx);
        if let Some(previous) = previous {
            let _ = previous.send(true);
        }

        let triggers = self.triggers.clone();
        let engine = self.engine.clone();

        tokio::spawn(async move {
            tracing::debug!("[ObservabilityWorker] Log trigger subscriber started");
            loop {
                tokio::select! {
                    result = shutdown_rx.changed() => {
                        if result.is_err() || *shutdown_rx.borrow() {
                            tracing::debug!("[ObservabilityWorker] Log trigger subscriber shutting down");
                            break;
                        }
                    }
                    result = stop_rx.changed() => {
                        if result.is_err() || *stop_rx.borrow() {
                            tracing::debug!("[ObservabilityWorker] Log trigger subscriber replaced");
                            break;
                        }
                    }
                    result = rx.recv() => {
                        match result {
                            Ok(log) => {
                                ObservabilityWorker::invoke_triggers_for_log(&triggers, &engine, &log).await;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                tracing::warn!(skipped, "Log trigger subscriber lagged, some logs were skipped");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                tracing::debug!("[ObservabilityWorker] Log broadcast channel closed");
                                break;
                            }
                        }
                    }
                }
            }
            tracing::debug!("[ObservabilityWorker] Log trigger subscriber stopped");
        });
    }

    /// (Re)spawn the log-retention sweep from `cfg`, stopping any previous
    /// instance. The task also follows the worker shutdown signal.
    fn spawn_logs_retention(&self, cfg: &config::ObservabilityWorkerConfig) {
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        let previous = self
            .logs_retention_stop
            .lock()
            .expect("logs_retention_stop mutex poisoned")
            .replace(stop_tx);
        if let Some(previous) = previous {
            let _ = previous.send(true);
        }

        let Some(retention_seconds) = cfg.logs_retention_seconds.filter(|&s| s > 0) else {
            return; // retention disabled: previous task stopped, nothing to spawn
        };
        let Some(retention_ns) = retention_seconds.checked_mul(1_000_000_000) else {
            tracing::warn!(
                "logs_retention_seconds overflow when converting to nanoseconds; \
                 disabling log retention task"
            );
            return;
        };
        let Some(log_storage) = otel::get_log_storage() else {
            return;
        };
        let Some(mut shutdown_rx) = self
            .worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned")
            .clone()
        else {
            return;
        };

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
            loop {
                tokio::select! {
                    result = shutdown_rx.changed() => {
                        if result.is_err() || *shutdown_rx.borrow() {
                            tracing::debug!("[ObservabilityWorker] Log retention task shutting down");
                            break;
                        }
                    }
                    result = stop_rx.changed() => {
                        if result.is_err() || *stop_rx.borrow() {
                            tracing::debug!("[ObservabilityWorker] Log retention task replaced");
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        log_storage.apply_retention(retention_ns);
                    }
                }
            }
        });
    }

    /// (Re)spawn the OTLP logs exporter from `cfg`, stopping any previous
    /// instance. The exporter task follows both its per-instance stop signal
    /// and the worker shutdown signal.
    fn spawn_logs_exporter(&self, cfg: &config::ObservabilityWorkerConfig) {
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        // Hold the previous instance's stop sender; it is signaled only once
        // the replacement is ready to consume (or immediately on the
        // disabled-exit paths below). Stopping it before the new receiver
        // subscribes would drop every log broadcast in the gap.
        let previous = self
            .logs_exporter_stop
            .lock()
            .expect("logs_exporter_stop mutex poisoned")
            .replace(stop_tx);
        let stop_previous = move || {
            if let Some(previous) = previous {
                let _ = previous.send(true);
            }
        };

        // Resolve the exporter type with the OTEL_LOGS_EXPORTER env fallback
        // (the field is None when the yaml block omits it; the Default impl's
        // Some(Memory) only applies when the whole block is absent).
        let exporter_type = cfg
            .logs_exporter
            .clone()
            .or_else(|| {
                std::env::var("OTEL_LOGS_EXPORTER")
                    .ok()
                    .map(|v| match v.to_lowercase().as_str() {
                        "otlp" => config::LogsExporterType::Otlp,
                        "both" => config::LogsExporterType::Both,
                        _ => config::LogsExporterType::Memory,
                    })
            })
            .unwrap_or(config::LogsExporterType::Memory);
        if exporter_type == config::LogsExporterType::Memory {
            stop_previous(); // OTLP export disabled: stop the old instance
            return;
        }
        let Some(log_storage) = otel::get_log_storage() else {
            stop_previous();
            return;
        };
        let Some(worker_shutdown_rx) = self
            .worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned")
            .clone()
        else {
            stop_previous();
            return;
        };

        let endpoint = cfg
            .endpoint
            .clone()
            .unwrap_or_else(|| "http://localhost:4317".to_string());
        let service_name = cfg
            .service_name
            .clone()
            .unwrap_or_else(|| "iii".to_string());
        let service_version = cfg
            .service_version
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        // Subscribe the new receiver BEFORE stopping the old exporter, so the
        // window between the two carries bounded duplicates rather than
        // dropped logs (the broadcast delivers to every live receiver).
        let rx = log_storage.subscribe();
        stop_previous();
        let mut exporter =
            otel::OtlpLogsExporter::new(endpoint.clone(), service_name, service_version);

        if let Some(batch_size) = cfg
            .logs_batch_size
            .or_else(|| parse_env_var("OTEL_LOGS_BATCH_SIZE"))
        {
            exporter = exporter.with_batch_size(batch_size);
        }

        if let Some(flush_interval_ms) = cfg
            .logs_flush_interval_ms
            .or_else(|| parse_env_var("OTEL_LOGS_FLUSH_INTERVAL_MS"))
        {
            exporter =
                exporter.with_flush_interval(std::time::Duration::from_millis(flush_interval_ms));
        }

        // The exporter consumes a single shutdown receiver; bridge the
        // per-instance stop and the worker-wide shutdown into one channel.
        let (bridge_tx, bridge_rx) = tokio::sync::watch::channel(false);
        {
            let mut stop_rx = stop_rx;
            let mut worker_rx = worker_shutdown_rx;
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        result = stop_rx.changed() => {
                            if result.is_err() || *stop_rx.borrow() {
                                let _ = bridge_tx.send(true);
                                break;
                            }
                        }
                        result = worker_rx.changed() => {
                            if result.is_err() || *worker_rx.borrow() {
                                let _ = bridge_tx.send(true);
                                break;
                            }
                        }
                    }
                }
            });
        }

        exporter.start_with_shutdown(rx, bridge_rx);

        tracing::info!(
            "{} OTLP logs exporter started (endpoint: {})",
            "[LOGS]".cyan(),
            endpoint
        );
    }

    // =========================================================================
    // OTEL-native Log Functions (recommended over legacy logger.*)
    // =========================================================================

    #[function(
        id = "engine::log::info",
        description = "Record an INFO-level OTEL log (severity 9) with optional trace/span correlation and structured data. No-op when logs_enabled is false or dropped by logs_sampling_ratio."
    )]
    pub async fn log_info(&self, input: OtelLogInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.store_and_emit_log(&input, "INFO", 9).await;
        FunctionResult::NoResult
    }

    #[function(
        id = "engine::log::warn",
        description = "Record a WARN-level OTEL log (severity 13) with optional trace/span correlation and structured data. No-op when logs_enabled is false or dropped by logs_sampling_ratio."
    )]
    pub async fn log_warn(&self, input: OtelLogInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.store_and_emit_log(&input, "WARN", 13).await;
        FunctionResult::NoResult
    }

    #[function(
        id = "engine::log::error",
        description = "Record an ERROR-level OTEL log (severity 17) with optional trace/span correlation and structured data. No-op when logs_enabled is false or dropped by logs_sampling_ratio."
    )]
    pub async fn log_error(&self, input: OtelLogInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.store_and_emit_log(&input, "ERROR", 17).await;
        FunctionResult::NoResult
    }

    #[function(
        id = "engine::log::debug",
        description = "Record a DEBUG-level OTEL log (severity 5) with optional trace/span correlation and structured data. No-op when logs_enabled is false or dropped by logs_sampling_ratio."
    )]
    pub async fn log_debug(&self, input: OtelLogInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.store_and_emit_log(&input, "DEBUG", 5).await;
        FunctionResult::NoResult
    }

    #[function(
        id = "engine::log::trace",
        description = "Record a TRACE-level OTEL log (severity 1) with optional trace/span correlation and structured data. No-op when logs_enabled is false or dropped by logs_sampling_ratio."
    )]
    pub async fn log_trace(&self, input: OtelLogInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.store_and_emit_log(&input, "TRACE", 1).await;
        FunctionResult::NoResult
    }

    // =========================================================================
    // Baggage Functions
    // =========================================================================

    #[function(
        id = "engine::baggage::get",
        description = "Read one baggage entry by key from the current OTEL context, returning { value } (null if unset). Diagnostic only: reads ambient process context, not per-invocation baggage."
    )]
    pub async fn baggage_get(
        &self,
        input: BaggageGetInput,
    ) -> FunctionResult<BaggageGetResult, ErrorBody> {
        use opentelemetry::baggage::BaggageExt;

        let cx = opentelemetry::Context::current();
        let baggage = cx.baggage();
        let value = baggage.get(&input.key).map(|v| v.to_string());
        FunctionResult::Success(BaggageGetResult { value })
    }

    #[function(
        id = "engine::baggage::set",
        description = "Set a baggage key/value on a fresh OTEL context for verification only; the new context is not propagated to the caller or global state. Use SDK-level headers for real propagation."
    )]
    pub async fn baggage_set(
        &self,
        input: BaggageSetInput,
    ) -> FunctionResult<BaggageSetResult, ErrorBody> {
        use opentelemetry::KeyValue;
        use opentelemetry::baggage::BaggageExt;

        // Note: Baggage in OpenTelemetry is immutable - we create a new context
        // but since this is a function call, we can't actually propagate the new context
        // back to the caller. This function is mainly useful for verification/debugging.
        // Real baggage propagation should be done at the SDK/invocation level.
        let cx = opentelemetry::Context::current();
        let _new_cx = cx.with_baggage([KeyValue::new(input.key.clone(), input.value.clone())]);

        FunctionResult::Success(BaggageSetResult {
            success: true,
            note: "Baggage set in new context. For propagation, use SDK-level baggage headers."
                .to_string(),
        })
    }

    #[function(
        id = "engine::baggage::get_all",
        description = "Read all baggage entries from the current OTEL context as a { baggage } map. Diagnostic only: reflects ambient process context, not per-invocation baggage."
    )]
    pub async fn baggage_get_all(
        &self,
        _input: BaggageGetAllInput,
    ) -> FunctionResult<BaggageGetAllResult, ErrorBody> {
        use opentelemetry::baggage::BaggageExt;

        let cx = opentelemetry::Context::current();
        let baggage = cx.baggage();
        let items: std::collections::HashMap<String, String> = baggage
            .iter()
            .map(|(k, (v, _))| (k.to_string(), v.to_string()))
            .collect();
        FunctionResult::Success(BaggageGetAllResult { baggage: items })
    }

    /// Store a log in OTEL format and emit tracing event
    async fn store_and_emit_log(
        &self,
        input: &OtelLogInput,
        severity_text: &str,
        severity_number: i32,
    ) {
        // Respect logs_enabled: if explicitly disabled, skip storage/emit entirely.
        if !otel::logs_enabled(otel::get_otel_config().as_deref()) {
            return;
        }

        // Check sampling ratio before storing
        let should_sample = {
            let ratio = otel::get_otel_config()
                .map(|c| c.logs_sampling_ratio)
                .unwrap_or(1.0);
            ratio >= 1.0 || rand::random::<f64>() < ratio
        };

        if !should_sample {
            return;
        }

        // Initialize storage if not already done, honoring the configured cap.
        if otel::get_log_storage().is_none() {
            otel::init_log_storage(self.effective_logs_max_count());
        }

        let service_name = input
            .service_name
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        // Convert data to attributes HashMap
        let mut attributes = HashMap::new();
        if let Some(data) = &input.data
            && let Some(obj) = data.as_object()
        {
            for (key, value) in obj {
                attributes.insert(key.clone(), value.clone());
            }
        }

        let stored_log = otel::StoredLog {
            timestamp_unix_nano: timestamp,
            observed_timestamp_unix_nano: timestamp,
            severity_number,
            severity_text: severity_text.to_string(),
            body: input.message.clone(),
            attributes,
            trace_id: input.trace_id.clone(),
            span_id: input.span_id.clone(),
            resource: get_resource_attributes(),
            service_name: service_name.clone(),
            instrumentation_scope_name: Some("iii".to_string()),
            instrumentation_scope_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        };

        // Store the log if storage is available
        if let Some(storage) = otel::get_log_storage() {
            storage.store(stored_log.clone());
        } else {
            // Use thread-local to warn once per thread
            thread_local! {
                static WARNED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
            }
            WARNED.with(|warned| {
                if !warned.get() {
                    tracing::warn!(
                        "Log storage not initialized - logs will not be stored. \
                        Call otel::init_log_storage() or ensure ObservabilityWorker is initialized."
                    );
                    warned.set(true);
                }
            });
        }

        // Emit tracing event for console/OTLP export
        let data_str = input
            .data
            .as_ref()
            .map(|d| serde_json::to_string(d).unwrap_or_default())
            .unwrap_or_else(|| "{}".to_string());

        match severity_number {
            1..=4 => {
                tracing::trace!(service = %service_name, trace_id = ?input.trace_id, span_id = ?input.span_id, data = %data_str, "{}", input.message)
            }
            5..=8 => {
                tracing::debug!(service = %service_name, trace_id = ?input.trace_id, span_id = ?input.span_id, data = %data_str, "{}", input.message)
            }
            9..=12 => {
                tracing::info!(service = %service_name, trace_id = ?input.trace_id, span_id = ?input.span_id, data = %data_str, "{}", input.message)
            }
            13..=16 => {
                tracing::warn!(service = %service_name, trace_id = ?input.trace_id, span_id = ?input.span_id, data = %data_str, "{}", input.message)
            }
            _ => {
                tracing::error!(service = %service_name, trace_id = ?input.trace_id, span_id = ?input.span_id, data = %data_str, "{}", input.message)
            }
        }

        // Note: Log triggers are now handled by the broadcast subscriber in start_log_subscriber
        // This ensures all logs (from OTLP, function calls, tracing layer) trigger handlers uniformly
    }

    /// Invoke log triggers for a given log entry (static method for use in spawned tasks)
    async fn invoke_triggers_for_log(
        triggers: &Arc<OtelLogTriggers>,
        engine: &Arc<Engine>,
        log: &otel::StoredLog,
    ) {
        let triggers_guard = triggers.triggers.read().await;
        let log_level = log.severity_text.to_lowercase();

        for trigger in triggers_guard.iter() {
            let trigger_level = trigger
                .config
                .get("level")
                .and_then(|v| v.as_str())
                .unwrap_or("all");

            if should_trigger_for_level(trigger_level, &log_level) {
                // Send OTEL format matching StoredLog / OtelLogEvent
                let log_data = serde_json::json!({
                    "timestamp_unix_nano": log.timestamp_unix_nano,
                    "observed_timestamp_unix_nano": log.observed_timestamp_unix_nano,
                    "severity_number": log.severity_number,
                    "severity_text": log.severity_text,
                    "body": log.body,
                    "attributes": log.attributes,
                    "trace_id": log.trace_id,
                    "span_id": log.span_id,
                    "resource": log.resource,
                    "service_name": log.service_name,
                    "instrumentation_scope_name": log.instrumentation_scope_name,
                    "instrumentation_scope_version": log.instrumentation_scope_version,
                });

                let engine = engine.clone();
                let namespace = trigger.namespace.clone();
                let function_id = trigger.function_id.clone();
                // Trigger metadata must ride along (state/stream/cron parity):
                // remote consumers key delivery routing on it.
                let metadata = trigger.metadata.clone();

                tokio::spawn(async move {
                    let _ = engine
                        .call_with_metadata_ns(&namespace, &function_id, log_data, metadata)
                        .await;
                });
            }
        }
    }

    /// Body of the trace-trigger subscriber task: wait for span storage to
    /// exist, attach to its broadcast exactly once, then coalesce span
    /// activity into periodic `trace` trigger fan-outs and live stream
    /// pushes, excluding engine-internal spans and the trigger's own
    /// delivery spans (`engine.call` to a consumer fn is itself instrumented
    /// as a span, so without the exclusion the trigger would re-fire on its
    /// own output — an unbounded feedback loop).
    ///
    /// Span storage is a process-wide `OnceLock` created by OTEL pipeline
    /// init (`logging::init_*` → `init_otel`), which has NO ordering
    /// guarantee with worker startup — storage absence at spawn time is
    /// normal, not terminal. Poll with bounded backoff until it appears:
    /// once set it lives for the process (runtime `exporter`/`enabled`
    /// changes are restart-tier and never remove it), so attaching once is
    /// enough. When the OTEL pipeline is disabled storage never appears and
    /// this task idles at the capped interval until worker shutdown.
    ///
    /// `lookup` abstracts `otel::get_span_storage` so tests can drive the
    /// wait-then-attach path without the process-global `OnceLock`.
    async fn run_trace_trigger_subscriber(
        triggers: Arc<OtelTraceTriggers>,
        engine: Arc<Engine>,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
        mut lookup: impl FnMut() -> Option<Arc<otel::InMemorySpanStorage>>,
    ) {
        let storage = match lookup() {
            Some(storage) => storage,
            None => {
                tracing::debug!(
                    "[ObservabilityWorker] Span storage not yet initialized (OTEL pipeline init pending or disabled); trace trigger subscriber waiting"
                );
                const MAX_DELAY: tokio::time::Duration = tokio::time::Duration::from_secs(5);
                let mut delay = tokio::time::Duration::from_millis(50);
                loop {
                    tokio::select! {
                        result = shutdown_rx.changed() => {
                            if result.is_err() || *shutdown_rx.borrow() {
                                tracing::debug!(
                                    "[ObservabilityWorker] Trace trigger subscriber shutting down before span storage appeared"
                                );
                                return;
                            }
                        }
                        _ = tokio::time::sleep(delay) => {
                            delay = std::cmp::min(delay * 2, MAX_DELAY);
                        }
                    }
                    if let Some(storage) = lookup() {
                        break storage;
                    }
                }
            }
        };

        let mut rx = storage.subscribe();
        tracing::debug!("[ObservabilityWorker] Trace trigger subscriber started");

        // Snapshot of registered trigger function ids, refreshed each
        // window — a span produced by delivering one of these is the
        // trigger's own output and must not re-fire it.
        let mut trigger_fns: HashSet<String> = triggers
            .triggers
            .read()
            .await
            .iter()
            .map(|t| t.function_id.clone())
            .collect();
        let mut window: Vec<otel::StoredSpan> = Vec::new();
        let mut ticker =
            tokio::time::interval(tokio::time::Duration::from_millis(TRACE_COALESCE_MS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                result = shutdown_rx.changed() => {
                    if result.is_err() || *shutdown_rx.borrow() {
                        tracing::debug!("[ObservabilityWorker] Trace trigger subscriber shutting down");
                        break;
                    }
                }
                result = rx.recv() => {
                    match result {
                        Ok(span) => {
                            // Loop-break, aligned with the DEFAULT list view
                            // the tick invalidates: drop every internal span
                            // (machinery AND parented built-ins), spans
                            // attributed to observability functions (the
                            // pipeline must not observe itself), and the
                            // triggers' own delivery spans. Parented
                            // built-ins must not tick either: a consumer that
                            // reacts to a tick by querying through its bridge
                            // produces exactly such spans (`POST _console/*`
                            // → `call engine::console::*` → worker `execute`),
                            // and letting any of them re-arm the window is a
                            // self-sustaining 300ms loop.
                            if is_internal_span(&span) {
                                continue;
                            }
                            if span_function_id(&span).is_some_and(
                                crate::workers::telemetry::is_observability_function_id,
                            ) {
                                continue;
                            }
                            if span_function_id(&span).is_some_and(|f| trigger_fns.contains(f)) {
                                continue;
                            }
                            // Worker-side execution spans (`execute <fn>`,
                            // exported over OTLP) carry NO function_id
                            // attribute, so the attribute-based exclusions
                            // above miss them. Derive the id from the span
                            // NAME and re-apply the same rules.
                            let named_fn = span
                                .name
                                .strip_prefix("execute ")
                                .or_else(|| span.name.strip_prefix("call "));
                            if let Some(named_fn) = named_fn {
                                if trigger_fns.contains(named_fn)
                                    || crate::workers::telemetry::is_observability_function_id(
                                        named_fn,
                                    )
                                    || named_fn.starts_with("engine::")
                                {
                                    continue;
                                }
                            }
                            window.push(span);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "Trace trigger subscriber lagged, some spans were skipped");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            tracing::debug!("[ObservabilityWorker] Span broadcast channel closed");
                            break;
                        }
                    }
                }
                _ = ticker.tick() => {
                    trigger_fns = triggers
                        .triggers
                        .read()
                        .await
                        .iter()
                        .map(|t| t.function_id.clone())
                        .collect();
                    if window.is_empty() {
                        continue;
                    }
                    let batch = std::mem::take(&mut window);
                    ObservabilityWorker::fire_trace_triggers(&triggers, &engine, &batch).await;
                }
            }
        }

        tracing::debug!("[ObservabilityWorker] Trace trigger subscriber stopped");
    }

    /// Invoke trace (span) triggers for a given span (static method for use in
    /// spawned tasks). Mirrors `invoke_triggers_for_log`: every span lands on
    /// the broadcast channel, this filters per-trigger and fans out via
    /// `engine.call`. Fire-and-forget — handler results are ignored and a
    /// failing handler never affects span storage.
    /// Fan out one coalesced "traces changed" tick per matching trigger for a
    /// batch of spans (a debounce window). Each trigger receives a light
    /// `{ trace_ids: [...] }` payload — the distinct traces touched in the
    /// window that match its filter — rather than per-span full payloads.
    ///
    /// The handler is invoked fire-and-forget in the trigger's function
    /// namespace; results are ignored. NOTE: that engine call is itself
    /// instrumented as a span, so the subscriber MUST exclude trigger-delivery
    /// spans before they reach here (see `run_trace_trigger_subscriber`) or the
    /// trigger would re-fire on its own delivery — an unbounded feedback loop.
    async fn fire_trace_triggers(
        triggers: &Arc<OtelTraceTriggers>,
        engine: &Arc<Engine>,
        batch: &[otel::StoredSpan],
    ) {
        let triggers_guard = triggers.triggers.read().await;

        for trigger in triggers_guard.iter() {
            let config_service = trigger.config.get("service_name").and_then(|v| v.as_str());
            let config_status = trigger.config.get("status").and_then(|v| v.as_str());

            let mut trace_ids: Vec<String> = batch
                .iter()
                .filter(|s| should_trigger_for_span(config_service, config_status, s))
                .map(|s| s.trace_id.clone())
                .collect();
            trace_ids.sort();
            trace_ids.dedup();

            if trace_ids.is_empty() {
                continue;
            }

            let payload = serde_json::json!({ "trace_ids": trace_ids });
            let engine = engine.clone();
            let namespace = trigger.namespace.clone();
            let function_id = trigger.function_id.clone();
            // Trigger metadata must ride along (state/stream/cron parity):
            // remote consumers key delivery routing on it.
            let metadata = trigger.metadata.clone();

            tokio::spawn(async move {
                let _ = engine
                    .call_with_metadata_ns(&namespace, &function_id, payload, metadata)
                    .await;
            });
        }
    }

    // =========================================================================
    // Traces Functions
    // =========================================================================

    #[function(
        id = "engine::traces::list",
        description = "List compact trace summaries with aggregate status/counts, filtering, pagination, sort, and optional attribute_projection. Child spans are searched and aggregated in the engine; full span payloads are available from engine::traces::spans. Requires exporter memory or both, else fails memory_exporter_not_enabled."
    )]
    pub async fn list_traces(
        &self,
        input: TracesListInput,
    ) -> FunctionResult<TracesListResult, ErrorBody> {
        match otel::get_span_storage() {
            Some(_storage) => {
                let include_internal = input.include_internal.unwrap_or(false);
                let offset = input.offset.unwrap_or(0);
                let limit = input.limit.unwrap_or(100);
                let sort_order_asc = input
                    .sort_order
                    .as_deref()
                    .map(|order| order.eq_ignore_ascii_case("asc"))
                    .unwrap_or(true);
                // The hot path is the ordinary trace list: identify and page
                // distinct trace roots first, then read full spans only for
                // those trace IDs. This preserves one-row-per-trace semantics
                // even for distributed traces with more than one dangling
                // root, while keeping full records off the wire.
                let unfiltered = input.trace_id.is_none()
                    && input.trace_ids.is_none()
                    && input.service_name.is_none()
                    && input.name.is_none()
                    && input.status.is_none()
                    && input.min_duration_ms.is_none()
                    && input.max_duration_ms.is_none()
                    && input.start_time.is_none()
                    && input.end_time.is_none()
                    && input.attributes.is_none()
                    && input.exclude_attributes.is_none()
                    && matches!(input.sort_by.as_deref(), None | Some("start_time"));
                let query_started = Instant::now();
                let query_trace_id = input.trace_id.clone();
                let query_trace_ids = input.trace_ids.clone();
                let query_input = input.clone();
                let query_view = run_blocking_query("traces::list summary view", move || {
                    if unfiltered {
                        let mut roots = otel::get_query_root_spans();
                        if !include_internal {
                            roots.retain(|span| !is_internal_span(span));
                        }
                        roots.sort_by(|a, b| {
                            let cmp = a
                                .start_time_unix_nano
                                .cmp(&b.start_time_unix_nano)
                                .then_with(|| a.trace_id.cmp(&b.trace_id))
                                .then_with(|| a.span_id.cmp(&b.span_id));
                            if sort_order_asc { cmp } else { cmp.reverse() }
                        });
                        let mut seen = HashSet::new();
                        roots.retain(|span| seen.insert(span.trace_id.clone()));
                        let total = roots.len();
                        let trace_ids: Vec<String> = roots
                            .into_iter()
                            .skip(offset)
                            .take(limit)
                            .map(|span| span.trace_id)
                            .collect();
                        (otel::get_query_spans_by_trace_ids(&trace_ids), Some(total))
                    } else if let Some(trace_id) = query_trace_id {
                        (otel::get_query_spans_by_trace_id(&trace_id), None)
                    } else if let Some(trace_ids) = query_trace_ids {
                        (otel::get_query_spans_by_trace_ids(&trace_ids), None)
                    } else {
                        // Root-level filters can reject traces before their
                        // child payloads are decoded. Filters that depend on
                        // aggregate/child data are deliberately deferred to
                        // the exact pass below.
                        let roots = otel::get_query_root_spans();
                        let mut roots_by_trace = HashMap::<String, Vec<otel::StoredSpan>>::new();
                        for root in roots {
                            roots_by_trace
                                .entry(root.trace_id.clone())
                                .or_default()
                                .push(root);
                        }
                        let trace_ids: Vec<String> = roots_by_trace
                            .into_iter()
                            .filter_map(|(trace_id, root_spans)| {
                                trace_might_match_root_filters(
                                    &root_spans,
                                    &query_input,
                                    include_internal,
                                )
                                .then_some(trace_id)
                            })
                            .collect();
                        (otel::get_query_spans_by_trace_ids(&trace_ids), None)
                    }
                })
                .await;
                let (mut spans, prepaginated_total) = match query_view {
                    Ok(result) => result,
                    Err(error) => return FunctionResult::Failure(error),
                };
                let query_view_elapsed = query_started.elapsed();
                if !include_internal {
                    spans.retain(|span| !is_internal_span(span));
                }

                let projection: HashSet<String> = input
                    .attribute_projection
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .filter(|key| !key.is_empty())
                    .cloned()
                    .collect();
                let now_ns = otel::now_unix_nanos();
                let mut by_trace = HashMap::<String, Vec<otel::StoredSpan>>::new();
                for span in spans {
                    by_trace
                        .entry(span.trace_id.clone())
                        .or_default()
                        .push(span);
                }

                let mut summaries: Vec<TraceSummary> = by_trace
                    .into_values()
                    .filter_map(|trace_spans| {
                        let summary = summarize_trace(&trace_spans, &projection)?;
                        trace_matches_list_filters(&summary, &trace_spans, &input, now_ns)
                            .then_some(summary)
                    })
                    .collect();
                let filtering_elapsed = query_started.elapsed();
                summaries.sort_by(|a, b| {
                    let cmp = match input.sort_by.as_deref().unwrap_or("start_time") {
                        "duration" | "duration_ms" => {
                            let a_duration = a
                                .end_time_unix_nano
                                .unwrap_or(now_ns)
                                .saturating_sub(a.start_time_unix_nano);
                            let b_duration = b
                                .end_time_unix_nano
                                .unwrap_or(now_ns)
                                .saturating_sub(b.start_time_unix_nano);
                            a_duration.cmp(&b_duration)
                        }
                        "service_name" => a.service_name.cmp(&b.service_name),
                        "name" => a.name.cmp(&b.name),
                        _ => a.start_time_unix_nano.cmp(&b.start_time_unix_nano),
                    }
                    .then_with(|| a.trace_id.cmp(&b.trace_id));
                    if sort_order_asc { cmp } else { cmp.reverse() }
                });

                let total = prepaginated_total.unwrap_or(summaries.len());
                let traces = if prepaginated_total.is_some() {
                    summaries
                } else {
                    summaries.into_iter().skip(offset).take(limit).collect()
                };
                tracing::debug!(
                    operation = "engine::traces::list",
                    total,
                    returned_traces = traces.len(),
                    projected_attributes = projection.len(),
                    query_view_ms = query_view_elapsed.as_secs_f64() * 1_000.0,
                    aggregation_ms =
                        (filtering_elapsed - query_view_elapsed).as_secs_f64() * 1_000.0,
                    total_ms = query_started.elapsed().as_secs_f64() * 1_000.0,
                    "trace summary list query completed"
                );

                FunctionResult::Success(TracesListResult {
                    traces,
                    total,
                    offset,
                    limit,
                    storage: trace_storage_status_value(),
                })
            }
            None => memory_exporter_not_enabled_error(),
        }
    }

    #[function(
        id = "engine::traces::spans",
        description = "List full stored spans, including attributes, events and links, with filtering and pagination. Use engine::traces::list for compact trace summaries. Requires exporter memory or both, else fails memory_exporter_not_enabled."
    )]
    pub async fn list_trace_spans(
        &self,
        input: TracesListInput,
    ) -> FunctionResult<TracesSpansResult, ErrorBody> {
        match otel::get_span_storage() {
            Some(_storage) => {
                let search_all = input.search_all_spans.unwrap_or(false);
                let include_internal = input.include_internal.unwrap_or(false);
                let offset = input.offset.unwrap_or(0);
                let limit = input.limit.unwrap_or(100);
                let sort_order_asc = input
                    .sort_order
                    .as_deref()
                    .map(|order| order.eq_ignore_ascii_case("asc"))
                    .unwrap_or(true);
                let unfiltered = input.trace_id.is_none()
                    && input.trace_ids.is_none()
                    && input.service_name.is_none()
                    && input.name.is_none()
                    && input.status.is_none()
                    && input.min_duration_ms.is_none()
                    && input.max_duration_ms.is_none()
                    && input.start_time.is_none()
                    && input.end_time.is_none()
                    && input.attributes.is_none()
                    && input.exclude_attributes.is_none()
                    && matches!(input.sort_by.as_deref(), None | Some("start_time"));
                let is_unfiltered_all_spans_page = search_all && include_internal && unfiltered;
                // The console default: root listing with no filters.
                // `include_internal` is deliberately NOT constrained — both
                // values are handled in SQL by the paged root view.
                let is_unfiltered_root_page = !search_all && unfiltered;
                let query_started = Instant::now();
                // The elapsed metric includes spawn_blocking queue time — that
                // is the latency the caller actually experiences.
                let query_trace_id = input.trace_id.clone();
                let query_trace_ids = input.trace_ids.clone();
                let query_view = run_blocking_query("traces::list query view", move || {
                    if is_unfiltered_all_spans_page {
                        let page =
                            otel::get_query_spans_page_by_start_time(offset, limit, sort_order_asc);
                        (page.spans, Some(page.total))
                    } else if is_unfiltered_root_page {
                        let page = otel::get_query_root_spans_page_by_start_time(
                            offset,
                            limit,
                            sort_order_asc,
                            include_internal,
                        );
                        (page.spans, Some(page.total))
                    } else if let Some(ref trace_id) = query_trace_id {
                        (otel::get_query_spans_by_trace_id(trace_id), None)
                    } else if let Some(ref trace_ids) = query_trace_ids {
                        (otel::get_query_spans_by_trace_ids(trace_ids), None)
                    } else if search_all {
                        (otel::get_query_spans(), None)
                    } else {
                        (otel::get_query_root_spans(), None)
                    }
                })
                .await;
                let (all_spans, prepaginated_total) = match query_view {
                    Ok(result) => result,
                    Err(error) => return FunctionResult::Failure(error),
                };
                let query_view_elapsed = query_started.elapsed();

                // One "now" for the whole listing — pending spans measure
                // duration-so-far / activity against it.
                let now_ns = otel::now_unix_nanos();

                // Pre-compute trace IDs that have any span matching the name filter
                let name_matched_trace_ids: Option<std::collections::HashSet<String>> =
                    if search_all {
                        if let Some(ref name_filter) = input.name {
                            let name_lower = name_filter.to_lowercase();
                            Some(
                                all_spans
                                    .iter()
                                    .filter(|s| s.name.to_lowercase().contains(&name_lower))
                                    .map(|s| s.trace_id.clone())
                                    .collect(),
                            )
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                // Trace-level attribute match, mirroring `name_matched_trace_ids`:
                // a root is included if any span in its trace matches all pairs.
                let attributes_matched_trace_ids: Option<std::collections::HashSet<String>> =
                    if search_all {
                        input.attributes.as_ref().map(|attrs| {
                            all_spans
                                .iter()
                                .filter(|s| {
                                    attrs.iter().all(|pair| {
                                        pair.len() == 2
                                            && s.attributes
                                                .iter()
                                                .any(|(k, v)| k == &pair[0] && v == &pair[1])
                                    })
                                })
                                .map(|s| s.trace_id.clone())
                                .collect()
                        })
                    } else {
                        None
                    };

                // A span is a trace root when it has no parent OR its parent is
                // absent from the store. The latter covers traces that entered
                // iii from an external caller via an incoming `traceparent`: the
                // server span's parent is the remote caller's span, which lives
                // in another service and is never stored here. Without this,
                // root-only listing hides every distributed trace. Mirrors
                // `build_span_tree`'s dangling-parent handling.
                let present_span_ids: std::collections::HashSet<String> =
                    all_spans.iter().map(|s| s.span_id.clone()).collect();

                let mut filtered: Vec<_> = all_spans
                    .into_iter()
                    // Root-only by default; `search_all_spans` widens to children too.
                    .filter(|s| {
                        search_all
                            || s.parent_span_id
                                .as_ref()
                                .is_none_or(|p| !present_span_ids.contains(p))
                    })
                    .filter(|s| {
                        // Exclude internal engine traces unless explicitly requested
                        if !include_internal {
                            let is_internal = s.attributes.iter().any(|(k, v)| {
                                (k == "iii.function.kind" && v == "internal")
                                    || (k == "function_id" && v.starts_with("engine::"))
                            });
                            if is_internal {
                                return false;
                            }
                        }
                        true
                    })
                    .filter(|s| {
                        // Row-level exclusion (e.g. the console's hidden
                        // functions): drop rows matching ANY [key, value] pair.
                        let Some(ref excl) = input.exclude_attributes else {
                            return true;
                        };
                        !excl.iter().any(|pair| {
                            pair.len() == 2
                                && s.attributes
                                    .iter()
                                    .any(|(k, v)| k == &pair[0] && v == &pair[1])
                        })
                    })
                    .filter(|s| {
                        if let Some(ref sn) = input.service_name
                            && !s.service_name.to_lowercase().contains(&sn.to_lowercase())
                        {
                            return false;
                        }
                        if let Some(ref n) = input.name {
                            if search_all {
                                // When searching all spans, check if this root's trace_id was matched
                                if let Some(ref matched_ids) = name_matched_trace_ids
                                    && !matched_ids.contains(&s.trace_id)
                                {
                                    return false;
                                }
                            } else {
                                // Original behavior: filter root span name only
                                if !s.name.to_lowercase().contains(&n.to_lowercase()) {
                                    return false;
                                }
                            }
                        }
                        if let Some(ref st) = input.status
                            && !s.status.to_lowercase().contains(&st.to_lowercase())
                        {
                            return false;
                        }
                        // Pending (in-flight) spans measure duration-so-far
                        // and count as active "now" for the time window — a
                        // span running for 10s legitimately matches
                        // `min_duration_ms: 5000`, and an end sentinel of 0
                        // must not drop it from "recent" views.
                        let duration_ms: f64 = s.duration_ns(now_ns) as f64 / 1_000_000.0;
                        if let Some(min) = input.min_duration_ms
                            && duration_ms < min
                        {
                            return false;
                        }
                        if let Some(max) = input.max_duration_ms
                            && duration_ms > max
                        {
                            return false;
                        }
                        if let Some(start) = input.start_time {
                            let start_ns = start * 1_000_000;
                            if s.effective_end_ns(now_ns) < start_ns {
                                return false;
                            }
                        }
                        if let Some(end) = input.end_time {
                            let end_ns = end * 1_000_000;
                            if s.start_time_unix_nano > end_ns {
                                return false;
                            }
                        }
                        if let Some(ref attrs) = input.attributes {
                            if search_all {
                                if let Some(ref matched_ids) = attributes_matched_trace_ids
                                    && !matched_ids.contains(&s.trace_id)
                                {
                                    return false;
                                }
                            } else {
                                // Root-only when search_all_spans=false, for
                                // back-compat with callers querying root-tagged
                                // attrs like `iii.function.kind`.
                                for pair in attrs {
                                    if pair.len() == 2 {
                                        let key = &pair[0];
                                        let value = &pair[1];
                                        if !s.attributes.iter().any(|(k, v)| k == key && v == value)
                                        {
                                            return false;
                                        }
                                    }
                                }
                            }
                        }
                        true
                    })
                    .collect();
                let filtering_elapsed = query_started.elapsed();

                filtered.sort_by(|a, b| {
                    let cmp = match input.sort_by.as_deref().unwrap_or("start_time") {
                        // Accept both "duration" and "duration_ms"; the rest of
                        // the API (min_duration_ms/max_duration_ms, the output
                        // span `duration_ms`) uses the `_ms` suffix, so callers
                        // reasonably pass either spelling.
                        "duration" | "duration_ms" => {
                            let da = a.duration_ns(now_ns) as f64;
                            let db = b.duration_ns(now_ns) as f64;
                            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                        }
                        "service_name" => a.service_name.cmp(&b.service_name),
                        "name" => a.name.cmp(&b.name),
                        _ => a.start_time_unix_nano.cmp(&b.start_time_unix_nano),
                    };
                    if sort_order_asc { cmp } else { cmp.reverse() }
                });
                let sorting_elapsed = query_started.elapsed();

                let total = prepaginated_total.unwrap_or(filtered.len());
                let spans: Vec<_> = if prepaginated_total.is_some() {
                    filtered
                } else {
                    filtered.into_iter().skip(offset).take(limit).collect()
                };

                let page_trace_ids: Vec<String> = spans
                    .iter()
                    .map(|span| span.trace_id.clone())
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect();
                let tag_started = Instant::now();
                let unique_page_traces = page_trace_ids.len();
                let enrichment_trace_ids = page_trace_ids;
                let tags_by_trace_id =
                    match run_blocking_query("traces::list tag enrichment", move || {
                        otel::get_query_trace_tags_by_trace_ids(&enrichment_trace_ids)
                    })
                    .await
                    {
                        Ok(tags) => tags,
                        Err(error) => return FunctionResult::Failure(error),
                    };
                let tag_elapsed = tag_started.elapsed();
                let serialization_started = Instant::now();
                let result_spans: Vec<Value> = spans
                    .into_iter()
                    .map(|s| {
                        let tags = tags_by_trace_id
                            .get(&s.trace_id)
                            .cloned()
                            .unwrap_or_default();
                        let mut value = serde_json::to_value(s).unwrap_or(Value::Null);
                        if !tags.is_empty()
                            && let Value::Object(ref mut map) = value
                        {
                            map.insert(
                                "trace_tags".to_string(),
                                serde_json::to_value(tags).unwrap_or(Value::Null),
                            );
                        }
                        value
                    })
                    .collect();
                let serialization_elapsed = serialization_started.elapsed();
                tracing::debug!(
                    operation = "engine::traces::spans",
                    search_all_spans = search_all,
                    total,
                    returned_spans = result_spans.len(),
                    unique_page_traces,
                    query_view_ms = query_view_elapsed.as_secs_f64() * 1_000.0,
                    filtering_ms = (filtering_elapsed - query_view_elapsed).as_secs_f64() * 1_000.0,
                    sorting_ms = (sorting_elapsed - filtering_elapsed).as_secs_f64() * 1_000.0,
                    tag_enrichment_ms = tag_elapsed.as_secs_f64() * 1_000.0,
                    serialization_ms = serialization_elapsed.as_secs_f64() * 1_000.0,
                    total_ms = query_started.elapsed().as_secs_f64() * 1_000.0,
                    "trace list query completed"
                );

                FunctionResult::Success(TracesSpansResult {
                    spans: result_spans,
                    total,
                    offset,
                    limit,
                    storage: trace_storage_status_value(),
                })
            }
            None => memory_exporter_not_enabled_error(),
        }
    }

    #[function(
        id = "engine::traces::tree",
        description = "Build the nested span tree for one trace_id as { roots }, pruning no-op trigger wrappers and collapsing configured pass-through spans. Requires exporter memory or both, else fails memory_exporter_not_enabled."
    )]
    pub async fn get_trace_tree(
        &self,
        input: TracesTreeInput,
    ) -> FunctionResult<TracesTreeResult, ErrorBody> {
        match otel::get_span_storage() {
            Some(_storage) => {
                let trace_id = input.trace_id.clone();
                let all_spans = match run_blocking_query("traces::tree", move || {
                    otel::get_query_spans_by_trace_id(&trace_id)
                })
                .await
                {
                    Ok(spans) => spans,
                    Err(error) => return FunctionResult::Failure(error),
                };

                if all_spans.is_empty() {
                    return FunctionResult::Success(TracesTreeResult {
                        roots: Vec::new(),
                        storage: trace_storage_status_value(),
                    });
                }

                let all_spans = correct_trace_spans(all_spans, &cached_collapse_rules());

                let roots = build_span_tree(all_spans);

                FunctionResult::Success(TracesTreeResult {
                    roots: roots
                        .into_iter()
                        .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
                        .collect(),
                    storage: trace_storage_status_value(),
                })
            }
            None => memory_exporter_not_enabled_error(),
        }
    }

    #[function(
        id = "engine::traces::clear",
        description = "Drop every span from the in-memory trace store, returning { success: true }. Requires exporter memory or both, else fails memory_exporter_not_enabled."
    )]
    pub async fn clear_traces(
        &self,
        _input: TracesClearInput,
    ) -> FunctionResult<OkResult, ErrorBody> {
        match otel::get_span_storage() {
            Some(storage) => {
                // clear() waits on the writer thread (up to 5s); keep it off
                // the async executor. The hot clear stays sync — pure memory.
                if let Some(archive) = otel::get_trace_disk_storage() {
                    let cleared =
                        run_blocking_query("traces::clear", move || archive.clear()).await;
                    match cleared {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            return FunctionResult::Failure(ErrorBody::new(
                                "trace_storage_clear_failed",
                                format!("could not clear durable trace storage: {error}"),
                            ));
                        }
                        Err(error) => return FunctionResult::Failure(error),
                    }
                }
                storage.clear();
                FunctionResult::Success(OkResult { success: true })
            }
            None => memory_exporter_not_enabled_error(),
        }
    }

    /// Aggregate stored spans by one attribute value. Returns up to
    /// `limit` groups (default 100), each with trace_ids, span_count,
    /// duration, and error_count.
    #[function(
        id = "engine::traces::group_by",
        description = "Aggregate stored spans by one attribute into groups (trace_ids, span_count, duration, error_count), newest-first, capped at limit (default 100); skips spans lacking the attribute and engine-internal spans unless include_internal. Requires exporter memory or both."
    )]
    pub async fn group_traces_by(
        &self,
        input: TracesGroupByInput,
    ) -> FunctionResult<TracesGroupByResult, ErrorBody> {
        match otel::get_span_storage() {
            Some(_storage) => {
                let query_started = Instant::now();
                let attribute = input.attribute.clone();
                let label_attribute = input.label_attribute.clone();
                let all_spans = match run_blocking_query("traces::group_by", move || {
                    otel::get_query_group_rows_by_attribute(&attribute, label_attribute.as_deref())
                })
                .await
                {
                    Ok(rows) => rows,
                    Err(error) => return FunctionResult::Failure(error),
                };
                let query_elapsed = query_started.elapsed();

                let include_internal = input.include_internal.unwrap_or(false);
                let since_ns = input.since_ms.map(|ms| ms.saturating_mul(1_000_000));
                let limit = input.limit.unwrap_or(100) as usize;
                // Pending spans count as active "now" for the window and for
                // last_seen (their end sentinel is 0).
                let now_ns = otel::now_unix_nanos();

                struct GroupBuilder {
                    trace_ids: std::collections::HashSet<String>,
                    span_count: u32,
                    first_seen_ns: u64,
                    last_seen_ns: u64,
                    error_count: u32,
                    label: Option<String>,
                    label_seen_at_ns: u64,
                }
                let mut groups: std::collections::HashMap<String, GroupBuilder> =
                    std::collections::HashMap::new();

                for span in &all_spans {
                    if !include_internal && span.is_internal {
                        continue;
                    }
                    if let Some(min_ns) = since_ns
                        && span.effective_end_ns(now_ns) < min_ns
                    {
                        continue;
                    }

                    let is_error = span.status.eq_ignore_ascii_case("error");
                    let entry = groups
                        .entry(span.value.clone())
                        .or_insert_with(|| GroupBuilder {
                            trace_ids: std::collections::HashSet::new(),
                            span_count: 0,
                            first_seen_ns: span.start_time_ns,
                            last_seen_ns: span.effective_end_ns(now_ns),
                            error_count: 0,
                            label: None,
                            label_seen_at_ns: 0,
                        });
                    entry.trace_ids.insert(span.trace_id.clone());
                    entry.span_count += 1;
                    entry.first_seen_ns = entry.first_seen_ns.min(span.start_time_ns);
                    entry.last_seen_ns = entry.last_seen_ns.max(span.effective_end_ns(now_ns));
                    if is_error {
                        entry.error_count += 1;
                    }
                    // Newest span carrying the label attribute wins, so e.g.
                    // session renames surface on the group heading.
                    if span.start_time_ns >= entry.label_seen_at_ns
                        && let Some(label) = &span.label
                    {
                        entry.label = Some(label.clone());
                        entry.label_seen_at_ns = span.start_time_ns;
                    }
                }

                let grouping_elapsed = query_started.elapsed();

                let mut result: Vec<TraceGroup> = groups
                    .into_iter()
                    .map(|(value, b)| {
                        let first_ms = b.first_seen_ns / 1_000_000;
                        let last_ms = b.last_seen_ns / 1_000_000;
                        // Saturate; durations beyond u32::MAX ms are diagnostic noise.
                        let duration_ms =
                            u32::try_from(last_ms.saturating_sub(first_ms)).unwrap_or(u32::MAX);
                        let mut trace_ids: Vec<String> = b.trace_ids.into_iter().collect();
                        trace_ids.sort();
                        TraceGroup {
                            value,
                            label: b.label,
                            trace_ids,
                            span_count: b.span_count,
                            first_seen_ms: first_ms,
                            last_seen_ms: last_ms,
                            duration_ms,
                            error_count: b.error_count,
                        }
                    })
                    .collect();

                result.sort_by(|a, b| b.first_seen_ms.cmp(&a.first_seen_ms));
                result.truncate(limit);
                tracing::debug!(
                    operation = "engine::traces::group_by",
                    attribute = %input.attribute,
                    source_rows = all_spans.len(),
                    returned_groups = result.len(),
                    query_view_ms = query_elapsed.as_secs_f64() * 1_000.0,
                    grouping_ms = (grouping_elapsed - query_elapsed).as_secs_f64() * 1_000.0,
                    total_ms = query_started.elapsed().as_secs_f64() * 1_000.0,
                    "trace group query completed"
                );

                FunctionResult::Success(TracesGroupByResult {
                    groups: result,
                    storage: trace_storage_status_value(),
                })
            }
            None => memory_exporter_not_enabled_error(),
        }
    }

    // =========================================================================
    // Metrics Functions
    // =========================================================================

    #[function(
        id = "engine::metrics::list",
        description = "Return engine invocation/worker counters and span-derived latency percentiles, plus stored SDK metrics filtered by name/time and optionally aggregated by interval. engine_metrics is always present; sdk_metrics is empty when no metric storage exists."
    )]
    pub async fn list_metrics(
        &self,
        input: MetricsListInput,
    ) -> FunctionResult<MetricsListResult, ErrorBody> {
        use std::sync::atomic::Ordering;

        let accumulator = metrics::get_metrics_accumulator();

        // Get SDK metrics from storage with optional filtering
        let sdk_metrics = if let Some(storage) = metrics::get_metric_storage() {
            if let (Some(start), Some(end)) = (input.start_time, input.end_time) {
                // Convert milliseconds to nanoseconds with overflow checking
                let start_ns = match start.checked_mul(1_000_000) {
                    Some(ns) => ns,
                    None => {
                        tracing::warn!("start_time overflow when converting to nanoseconds");
                        return FunctionResult::Failure(ErrorBody {
                            code: "time_value_overflow".to_string(),
                            message: "start_time value too large".to_string(),
                            stacktrace: None,
                        });
                    }
                };
                let end_ns = match end.checked_mul(1_000_000) {
                    Some(ns) => ns,
                    None => {
                        tracing::warn!("end_time overflow when converting to nanoseconds");
                        return FunctionResult::Failure(ErrorBody {
                            code: "time_value_overflow".to_string(),
                            message: "end_time value too large".to_string(),
                            stacktrace: None,
                        });
                    }
                };

                if let Some(name) = &input.metric_name {
                    storage.get_metrics_by_name_in_range(name, start_ns, end_ns)
                } else {
                    storage.get_metrics_in_range(start_ns, end_ns)
                }
            } else if let Some(name) = &input.metric_name {
                storage.get_metrics_by_name(name)
            } else {
                storage.get_metrics()
            }
        } else {
            Vec::new()
        };

        // Get aggregated metrics if interval is specified
        let aggregated_metrics = if let Some(interval_secs) = input.aggregate_interval {
            if let Some(storage) = metrics::get_metric_storage() {
                if let (Some(start), Some(end)) = (input.start_time, input.end_time) {
                    // Convert with overflow checking
                    let start_ns = match start.checked_mul(1_000_000) {
                        Some(ns) => ns,
                        None => {
                            tracing::warn!("start_time overflow in aggregated metrics");
                            return FunctionResult::Failure(ErrorBody {
                                code: "time_value_overflow".to_string(),
                                message: "start_time value too large".to_string(),
                                stacktrace: None,
                            });
                        }
                    };
                    let end_ns = match end.checked_mul(1_000_000) {
                        Some(ns) => ns,
                        None => {
                            tracing::warn!("end_time overflow in aggregated metrics");
                            return FunctionResult::Failure(ErrorBody {
                                code: "time_value_overflow".to_string(),
                                message: "end_time value too large".to_string(),
                                stacktrace: None,
                            });
                        }
                    };
                    let interval_ns = match interval_secs.checked_mul(1_000_000_000) {
                        Some(ns) => ns,
                        None => {
                            tracing::warn!(
                                "aggregate_interval overflow when converting to nanoseconds"
                            );
                            return FunctionResult::Failure(ErrorBody {
                                code: "time_value_overflow".to_string(),
                                message: "aggregate_interval value too large".to_string(),
                                stacktrace: None,
                            });
                        }
                    };
                    storage.get_aggregated_metrics(start_ns, end_ns, interval_ns)
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Build response with accumulator data (engine internal metrics)
        let invocations_total = accumulator.invocations_total.load(Ordering::Relaxed);
        let invocations_success = accumulator.invocations_success.load(Ordering::Relaxed);
        let invocations_error = accumulator.invocations_error.load(Ordering::Relaxed);
        let invocations_deferred = accumulator.invocations_deferred.load(Ordering::Relaxed);
        let workers_spawns = accumulator.workers_spawns.load(Ordering::Relaxed);
        let workers_deaths = accumulator.workers_deaths.load(Ordering::Relaxed);

        // Calculate performance metrics from span storage
        let (
            avg_duration_ms,
            p50_duration_ms,
            p95_duration_ms,
            p99_duration_ms,
            min_duration_ms,
            max_duration_ms,
        ) = if let Some(storage) = otel::get_span_storage() {
            storage.calculate_performance_metrics()
        } else {
            (0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
        };

        // `aggregated_metrics` serializes only when non-empty (skip_serializing_if),
        // matching the prior "add only if non-empty" behavior.
        let aggregated_metrics: Vec<Value> = aggregated_metrics
            .into_iter()
            .map(|m| serde_json::to_value(m).unwrap_or(Value::Null))
            .collect();

        // `query` echoes the applied filters, present only when one was supplied.
        let query = if input.start_time.is_some()
            || input.end_time.is_some()
            || input.aggregate_interval.is_some()
        {
            Some(MetricsListQuery {
                start_time: input.start_time,
                end_time: input.end_time,
                aggregate_interval: input.aggregate_interval,
                metric_name: input.metric_name,
            })
        } else {
            None
        };

        FunctionResult::Success(MetricsListResult {
            engine_metrics: EngineMetricsView {
                invocations: InvocationsView {
                    total: invocations_total,
                    success: invocations_success,
                    error: invocations_error,
                    deferred: invocations_deferred,
                    by_function: accumulator.get_by_function(),
                },
                workers: WorkersView {
                    spawns: workers_spawns,
                    deaths: workers_deaths,
                    active: workers_spawns.saturating_sub(workers_deaths),
                },
                performance: PerformanceView {
                    avg_duration_ms,
                    p50_duration_ms,
                    p95_duration_ms,
                    p99_duration_ms,
                    min_duration_ms,
                    max_duration_ms,
                },
            },
            sdk_metrics: sdk_metrics
                .into_iter()
                .map(|m| serde_json::to_value(m).unwrap_or(Value::Null))
                .collect(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            aggregated_metrics,
            query,
        })
    }

    // =========================================================================
    // Logs Functions
    // =========================================================================

    #[function(
        id = "engine::logs::list",
        description = "List stored OTEL logs filtered by trace/span id, severity, and time range, with pagination and a total count. Returns an empty result when logs_enabled is false (no log storage)."
    )]
    pub async fn list_logs(
        &self,
        input: LogsListInput,
    ) -> FunctionResult<LogsListResult, ErrorBody> {
        match otel::get_log_storage() {
            Some(storage) => {
                let (total, logs) = storage.get_logs_filtered(
                    input.trace_id.as_deref(),
                    input.span_id.as_deref(),
                    input.severity_min,
                    input.severity_text.as_deref(),
                    input.start_time,
                    input.end_time,
                    input.offset,
                    input.limit,
                );
                FunctionResult::Success(LogsListResult {
                    logs: logs
                        .into_iter()
                        .map(|l| serde_json::to_value(l).unwrap_or(Value::Null))
                        .collect(),
                    total,
                    query: Some(LogsListQuery {
                        trace_id: input.trace_id,
                        span_id: input.span_id,
                        severity_min: input.severity_min,
                        severity_text: input.severity_text,
                        start_time: input.start_time,
                        end_time: input.end_time,
                        offset: input.offset,
                        limit: input.limit,
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                })
            }
            None => {
                // Honor logs_enabled: do NOT lazily revive storage when logs
                // are disabled at config time. Return an empty result so API
                // consumers get a consistent shape. Gate on the LIVE config
                // (not the boot seed `_config`) so a runtime logs_enabled
                // toggle agrees with the ingest path in `store_and_emit_log`.
                if otel::logs_enabled(otel::get_otel_config().as_deref()) {
                    otel::init_log_storage(self.effective_logs_max_count());
                }
                FunctionResult::Success(LogsListResult {
                    logs: Vec::new(),
                    total: 0,
                    query: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                })
            }
        }
    }

    #[function(
        id = "engine::logs::clear",
        description = "Drop every stored OTEL log, returning { success: true }. Succeeds as a no-op when log storage was never initialized (logs_enabled false)."
    )]
    pub async fn clear_logs(
        &self,
        _input: LogsClearInput,
    ) -> FunctionResult<LogsClearResult, ErrorBody> {
        match otel::get_log_storage() {
            Some(storage) => {
                storage.clear();
                FunctionResult::Success(LogsClearResult {
                    success: true,
                    message: None,
                })
            }
            None => FunctionResult::Success(LogsClearResult {
                success: true,
                message: Some("No log storage initialized".to_string()),
            }),
        }
    }

    // =========================================================================
    // Sampling Diagnostic Functions
    // =========================================================================

    #[function(
        id = "engine::sampling::rules",
        description = "Report the active trace sampling config (default ratio, per-operation/service rules, parent_based) and the logs sampling_ratio, read from live config. Defaults to ratio 1.0 with no rules when sampling is unconfigured."
    )]
    pub async fn get_sampling_rules(
        &self,
        _input: LogsClearInput, // Reusing empty input type
    ) -> FunctionResult<SamplingRulesResult, ErrorBody> {
        let config = otel::get_otel_config();

        let (default_ratio, rules, parent_based, logs_sampling_ratio) = match config {
            Some(cfg) => {
                let default_ratio = cfg
                    .sampling
                    .as_ref()
                    .and_then(|s| s.default)
                    .or(cfg.sampling_ratio)
                    .unwrap_or(1.0);

                let rules: Vec<SamplingRuleView> = cfg
                    .sampling
                    .as_ref()
                    .map(|s| {
                        s.rules
                            .iter()
                            .map(|r| SamplingRuleView {
                                operation: r.operation.clone(),
                                service: r.service.clone(),
                                rate: r.rate,
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let parent_based = cfg
                    .sampling
                    .as_ref()
                    .and_then(|s| s.parent_based)
                    .unwrap_or(true);

                (default_ratio, rules, parent_based, cfg.logs_sampling_ratio)
            }
            None => (1.0, Vec::new(), true, 1.0),
        };

        FunctionResult::Success(SamplingRulesResult {
            traces: SamplingTracesView {
                default_ratio,
                rules,
                parent_based,
            },
            logs: SamplingLogsView {
                sampling_ratio: logs_sampling_ratio,
            },
            timestamp: chrono::Utc::now().timestamp_millis(),
        })
    }

    // =========================================================================
    // Health Check Functions
    // =========================================================================

    #[function(
        id = "engine::health::check",
        description = "Report observability subsystem health: per-component status (otel, metrics, logs, spans) marked healthy with counts or disabled, plus engine version. Always succeeds regardless of configuration."
    )]
    pub async fn health_check(
        &self,
        _input: HealthCheckInput,
    ) -> FunctionResult<HealthCheckResult, ErrorBody> {
        // Check OTEL configuration
        let otel_component = if let Some(config) = otel::get_otel_config() {
            let enabled = config.enabled.unwrap_or(false);
            if enabled {
                healthy_component(serde_json::json!({
                    "enabled": true,
                    "service_name": config.service_name,
                    "exporter": format!("{:?}", config.exporter),
                }))
            } else {
                disabled_component()
            }
        } else {
            disabled_component()
        };

        // Check metrics storage
        let metrics_component = if let Some(storage) = metrics::get_metric_storage() {
            healthy_component(serde_json::json!({
                "stored_metrics": storage.len(),
            }))
        } else {
            disabled_component()
        };

        // Check logs storage
        let logs_component = if let Some(storage) = otel::get_log_storage() {
            healthy_component(serde_json::json!({
                "stored_logs": storage.len(),
            }))
        } else {
            disabled_component()
        };

        // Check span storage. The component status must surface a degraded
        // archive — the overall status already does, and a consumer reading
        // only `components.spans.status` must not see "healthy" while the
        // durable tier is failing.
        let trace_storage_status = otel::trace_storage_status();
        let spans_component = if let Some(storage) = otel::get_span_storage() {
            let details = serde_json::json!({
                "stored_spans": storage.len(),
                "hot_bytes": storage.hot_bytes(),
                "archive": trace_storage_status,
            });
            if trace_storage_status.archive == "degraded" {
                serde_json::json!({
                    "status": "degraded",
                    "details": details,
                })
            } else {
                healthy_component(details)
            }
        } else {
            disabled_component()
        };

        let overall_status = if trace_storage_status.archive == "degraded" {
            "degraded"
        } else {
            "healthy"
        };

        FunctionResult::Success(HealthCheckResult {
            status: overall_status.to_string(),
            components: HealthComponentsView {
                otel: otel_component,
                metrics: metrics_component,
                logs: logs_component,
                spans: spans_component,
            },
            timestamp: chrono::Utc::now().timestamp_millis(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        })
    }

    // =========================================================================
    // Alerts Functions
    // =========================================================================

    #[function(
        id = "engine::alerts::list",
        description = "List configured alert rules with their current evaluated states and a firing_count. Returns empty when no alert rules are configured or the alert manager is not initialized."
    )]
    pub async fn list_alerts(
        &self,
        _input: AlertsListInput,
    ) -> FunctionResult<AlertsListResult, ErrorBody> {
        if let Some(manager) = metrics::get_alert_manager() {
            let states = manager.get_states();
            let firing = manager.get_firing_alerts();

            FunctionResult::Success(AlertsListResult {
                alerts: states
                    .into_iter()
                    .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
                    .collect(),
                rules: Some(manager.get_rules()),
                firing_count: firing.len(),
                message: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            })
        } else {
            FunctionResult::Success(AlertsListResult {
                alerts: Vec::new(),
                rules: None,
                firing_count: 0,
                message: Some("Alert manager not initialized".to_string()),
                timestamp: chrono::Utc::now().timestamp_millis(),
            })
        }
    }

    #[function(
        id = "engine::alerts::evaluate",
        description = "Force an immediate alert-rule evaluation against current metrics and return any triggered_alerts, bypassing the periodic tick. Returns evaluated:false when the alert manager is not initialized; produces nothing without configured rules."
    )]
    pub async fn evaluate_alerts(
        &self,
        _input: AlertsEvaluateInput,
    ) -> FunctionResult<AlertsEvaluateResult, ErrorBody> {
        if let Some(manager) = metrics::get_alert_manager() {
            let events = manager.evaluate().await;

            FunctionResult::Success(AlertsEvaluateResult {
                evaluated: true,
                triggered_alerts: Some(
                    events
                        .into_iter()
                        .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
                        .collect(),
                ),
                message: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            })
        } else {
            FunctionResult::Success(AlertsEvaluateResult {
                evaluated: false,
                triggered_alerts: None,
                message: Some("Alert manager not initialized".to_string()),
                timestamp: chrono::Utc::now().timestamp_millis(),
            })
        }
    }

    // =========================================================================
    // Rollups Functions
    // =========================================================================

    #[function(
        id = "engine::rollups::list",
        description = "Return pre-aggregated metric rollups and histograms for a level (0=1m, 1=5m, 2=1h) over a time range (default last hour), optionally by metric name. Falls back to on-the-fly aggregation over metric storage when no rollup storage exists."
    )]
    pub async fn list_rollups(
        &self,
        input: RollupsListInput,
    ) -> FunctionResult<RollupsListResult, ErrorBody> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        // Default to last hour if no time range specified
        let end_ns = if let Some(end_time) = input.end_time {
            match end_time.checked_mul(1_000_000) {
                Some(ns) => ns,
                None => {
                    tracing::warn!("end_time overflow when converting to nanoseconds in rollups");
                    return FunctionResult::Failure(ErrorBody {
                        code: "time_value_overflow".to_string(),
                        message: "end_time value too large".to_string(),
                        stacktrace: None,
                    });
                }
            }
        } else {
            now
        };

        let start_ns = if let Some(start_time) = input.start_time {
            match start_time.checked_mul(1_000_000) {
                Some(ns) => ns,
                None => {
                    tracing::warn!("start_time overflow when converting to nanoseconds in rollups");
                    return FunctionResult::Failure(ErrorBody {
                        code: "time_value_overflow".to_string(),
                        message: "start_time value too large".to_string(),
                        stacktrace: None,
                    });
                }
            }
        } else {
            end_ns.saturating_sub(3600 * 1_000_000_000)
        };

        let level = input.level.unwrap_or(0);

        if let Some(storage) = metrics::get_rollup_storage() {
            let rollups =
                storage.get_rollups(level, start_ns, end_ns, input.metric_name.as_deref());
            let histograms = storage.get_histogram_rollups(
                level,
                start_ns,
                end_ns,
                input.metric_name.as_deref(),
            );

            FunctionResult::Success(RollupsListResult {
                rollups: rollups
                    .into_iter()
                    .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
                    .collect(),
                histogram_rollups: histograms
                    .into_iter()
                    .map(|h| serde_json::to_value(h).unwrap_or(Value::Null))
                    .collect(),
                level: Some(level),
                source: None,
                query: Some(RollupsListQuery {
                    start_time: input.start_time,
                    end_time: input.end_time,
                    metric_name: input.metric_name,
                }),
                message: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            })
        } else {
            // Rollup storage not initialized, fall back to on-the-fly aggregation
            let interval_ns = match level {
                0 => 60 * 1_000_000_000,   // 1 minute
                1 => 300 * 1_000_000_000,  // 5 minutes
                _ => 3600 * 1_000_000_000, // 1 hour
            };

            if let Some(storage) = metrics::get_metric_storage() {
                let rollups = storage.get_aggregated_metrics(start_ns, end_ns, interval_ns);
                let histograms = storage.get_aggregated_histograms(start_ns, end_ns, interval_ns);

                FunctionResult::Success(RollupsListResult {
                    rollups: rollups
                        .into_iter()
                        .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
                        .collect(),
                    histogram_rollups: histograms
                        .into_iter()
                        .map(|h| serde_json::to_value(h).unwrap_or(Value::Null))
                        .collect(),
                    level: Some(level),
                    source: Some("on_the_fly".to_string()),
                    query: Some(RollupsListQuery {
                        start_time: input.start_time,
                        end_time: input.end_time,
                        metric_name: input.metric_name,
                    }),
                    message: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                })
            } else {
                FunctionResult::Success(RollupsListResult {
                    rollups: Vec::new(),
                    histogram_rollups: Vec::new(),
                    level: None,
                    source: None,
                    query: None,
                    message: Some("Metric storage not initialized".to_string()),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                })
            }
        }
    }
}

impl TriggerRegistrator for ObservabilityWorker {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        // The worker owns both the `log` and `trace` trigger types; route by
        // the trigger's declared type into the matching registry.
        if trigger.trigger_type == TRACE_TRIGGER_TYPE {
            let triggers = &self.trace_triggers.triggers;
            let service = trigger
                .config
                .get("service_name")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string();
            let status = trigger
                .config
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string();

            tracing::info!(
                "{} trace trigger {} (service: {}, status: {}) → {}",
                "[REGISTERED]".green(),
                trigger.id.purple(),
                service.cyan(),
                status.cyan(),
                trigger.function_id.cyan()
            );

            return Box::pin(async move {
                triggers.write().await.insert(trigger);
                Ok(())
            });
        }

        let triggers = &self.triggers.triggers;
        let level = trigger
            .config
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("all")
            .to_string();

        tracing::info!(
            "{} log trigger {} (level: {}) → {}",
            "[REGISTERED]".green(),
            trigger.id.purple(),
            level.cyan(),
            trigger.function_id.cyan()
        );

        Box::pin(async move {
            triggers.write().await.insert(trigger);
            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let triggers = if trigger.trigger_type == TRACE_TRIGGER_TYPE {
            &self.trace_triggers.triggers
        } else {
            &self.triggers.triggers
        };

        Box::pin(async move {
            tracing::debug!(trigger_id = %trigger.id, trigger_type = %trigger.trigger_type, "Unregistering trigger");
            triggers.write().await.remove(&trigger);
            Ok(())
        })
    }
}

#[async_trait]
impl Worker for ObservabilityWorker {
    fn name(&self) -> &'static str {
        "ObservabilityWorker"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        Ok(Box::new(Self::from_config(engine, config)?))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine.clone());
        // Registered here so the worker scope tracks the handler and removes
        // it automatically on destroy/reload. The hook order differs by
        // pipeline: initial boot runs `register_functions` BEFORE
        // `start_background_tasks` (workers/config.rs), reload runs it AFTER
        // (reload.rs) — so `start_background_tasks` also registers the
        // handler (if absent) before subscribing to configuration events.
        self.register_config_handler(&engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        // Read the authoritative config, not the yaml seed: on the serve path
        // the boot merge has already published the persisted entry as the
        // global, so initialize() and start_background_tasks must agree on the
        // same source or they half-initialize the worker (one registers the
        // trigger types / alert manager, the other skips them).
        let config = self.current_config();
        let enabled = config.enabled.unwrap_or(true);
        if !enabled {
            tracing::info!(
                "{} Observability disabled by configuration",
                "[OTEL]".yellow()
            );
            return Ok(());
        }

        // Initialize metrics. Called even when the metrics signal is
        // disabled: init_metrics returns false in that case but still
        // applies the configured storage limits, which SDK metric ingestion
        // uses regardless of the export toggle.
        let metrics_config = metrics::MetricsConfig::default();
        if metrics::init_metrics(&metrics_config) {
            // Pre-initialize global engine metrics only if init succeeded
            let _ = metrics::get_engine_metrics();
        }

        // Initialize log storage only when logs are enabled
        if otel::logs_enabled(Some(&config)) {
            otel::init_log_storage(self.effective_logs_max_count());
        } else {
            tracing::info!(
                "{} OTEL logs disabled via logs_enabled=false; skipping log storage",
                "[OTEL]".cyan()
            );
        }

        // Initialize rollup storage for multi-level metric aggregation
        metrics::init_rollup_storage();
        tracing::info!(
            "{} Rollup storage initialized with 3 levels (1m, 5m, 1h)",
            "[ROLLUPS]".cyan()
        );

        // Always initialize the alert manager (even with zero rules) so a
        // later configuration-worker edit can hot-add rules via
        // update_rules; the 10s evaluation tick is a no-op while empty.
        // Seed from the authoritative config (not the yaml seed): on a restart
        // the first apply_config sees old == new and skips the alert SWAP
        // tier, so a manager seeded from the stale yaml rules would silently
        // revert a runtime edit until the next alerts change.
        if !config.alerts.is_empty() {
            tracing::info!(
                "{} {} alert rules configured",
                "[ALERTS]".cyan(),
                config.alerts.len()
            );
        }
        metrics::init_alert_manager_with_engine(config.alerts.clone(), self.engine.clone());

        // Register log trigger type
        let log_trigger_type = TriggerType::new(
            LOG_TRIGGER_TYPE,
            "Log event trigger",
            Box::new(self.clone()),
            None,
        );

        let _ = self.engine.register_trigger_type(log_trigger_type).await;

        // Register trace (span) trigger type — lets any client react to spans
        // as they land, mirroring the log trigger.
        let trace_trigger_type = TriggerType::new(
            TRACE_TRIGGER_TYPE,
            "Trace/span event trigger",
            Box::new(self.clone()),
            None,
        );

        let _ = self.engine.register_trigger_type(trace_trigger_type).await;

        tracing::info!(
            "{} OpenTelemetry module initialized (log, trace, traces, metrics, logs, rollups functions available)",
            "[READY]".green()
        );
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        shutdown_rx: tokio::sync::watch::Receiver<bool>,
        _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        // Stored unconditionally (before the enabled gate) so `apply_config`
        // can hand respawned tasks the worker lifecycle even on deployments
        // that boot disabled.
        *self
            .worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned") = Some(shutdown_rx.clone());

        // Adopt the configuration worker as the runtime source of truth
        // BEFORE the enabled gate — mirroring iii-http, which always adopts.
        // This runs even when observability boots disabled, so the
        // `iii-observability` entry is always registered (a remote
        // `enabled: true` can be persisted and applied at the next start), the
        // change trigger always watches, and the restart-tier warning fires.
        // `configuration::*` is callable here on both pipelines; failures
        // degrade to the static config.yaml block. Every bus call is bounded.
        let register = tokio::time::timeout(
            configuration::CONFIG_BUS_TIMEOUT,
            configuration::register_config(self.engine.as_ref(), Some(&self._config)),
        )
        .await
        .map_err(|_| anyhow::anyhow!("configuration::register timed out"))
        .and_then(|result| result);
        if let Err(err) = register {
            tracing::warn!(
                error = %err,
                "iii-observability: configuration::register failed; continuing with static config"
            );
        }

        // Initial sync: fetch the authoritative value and apply it per tier
        // (apply_config carries its own bus timeout).
        if let Err(err) = self.apply_config().await {
            tracing::warn!(
                error = %err,
                "iii-observability: failed to read configuration; continuing with static config"
            );
        }

        // Register the handler before the trigger so a configuration event can
        // never fan out to a missing function. On reload, `register_functions`
        // runs after this hook and re-registers the handler inside the worker
        // scope; the `get` check keeps the initial-boot path (where it already
        // ran) from logging a spurious "already registered" overwrite.
        if self
            .engine
            .functions
            .get(
                crate::protocol::DEFAULT_NAMESPACE,
                configuration::CONFIG_FN_ID,
            )
            .is_none()
        {
            self.register_config_handler(&self.engine);
        }
        if let Err(err) = configuration::register_config_trigger(&self.engine).await {
            tracing::warn!(
                error = %err,
                "iii-observability: failed to watch configuration changes; hot-reload disabled"
            );
        } else {
            // Catch-up pass: replay any `configuration::set` that landed
            // between the initial sync above and the trigger subscription.
            configuration::on_config_change(self).await;
        }

        // Live background tasks run only when observability is enabled. The
        // trace/log pipeline is built at process start, so `enabled` is
        // restart-tier; this gate controls only the per-process task set, not
        // configuration adoption (done above).
        if !self.current_config().enabled.unwrap_or(true) {
            tracing::debug!(
                "[ObservabilityWorker] Observability disabled; skipping background tasks"
            );
            return Ok(());
        }

        // Start the log-trigger subscriber (respawnable: a runtime
        // logs_enabled false->true toggle re-runs this via apply_config).
        self.spawn_log_trigger_subscriber();

        // Start span subscriber: coalesce span activity into periodic `trace`
        // trigger fan-outs, excluding engine-internal spans and the trigger's
        // OWN delivery spans (see `run_trace_trigger_subscriber`). The task
        // waits for span storage to appear rather than checking once — OTEL
        // pipeline init has no ordering guarantee with worker startup, and a
        // one-shot check that lost that race used to disable trace triggers
        // for the life of the process.
        tokio::spawn(ObservabilityWorker::run_trace_trigger_subscriber(
            self.trace_triggers.clone(),
            self.engine.clone(),
            shutdown_rx.clone(),
            otel::get_span_storage,
        ));

        // The durable archive owns its writer thread, but retention and
        // pending-snapshot expiry are driven by the worker lifecycle so they
        // follow reload/shutdown boundaries. The hot-cache cleanup also runs
        // when durable storage is disabled. The archive is looked up per tick,
        // not captured: the SWAP tier of apply_config can replace it live.
        let mut archive_shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
            loop {
                tokio::select! {
                    result = archive_shutdown_rx.changed() => {
                        if result.is_err() || *archive_shutdown_rx.borrow() {
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        let pending_max_age = otel::get_otel_config()
                            .and_then(|config| config.trace_storage.as_ref().map(|storage| storage.pending_max_age_seconds))
                            .unwrap_or(3_600);
                        if let Some(storage) = otel::get_span_storage() {
                            storage.expire_pending(pending_max_age);
                        }
                        if let Some(archive) = otel::get_trace_disk_storage() {
                            // retain() waits on the writer thread; keep the
                            // wait off the async executor.
                            let outcome = tokio::task::spawn_blocking(move || {
                                let result = archive.retain();
                                (archive, result)
                            })
                            .await;
                            if let Ok((archive, Err(error))) = outcome {
                                archive.mark_degraded(error);
                            }
                        }
                    }
                }
            }
        });

        // Log retention runs as a respawnable task; spawned below from the
        // post-adoption effective configuration.

        // Spawn background task for metrics retention cleanup and rollup processing
        if let Some(storage) = metrics::get_metric_storage() {
            let mut shutdown_rx = shutdown_rx.clone();

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
                loop {
                    tokio::select! {
                        result = shutdown_rx.changed() => {
                            if result.is_err() {
                                tracing::debug!("[ObservabilityWorker] Shutdown channel closed");
                                break;
                            }
                            if *shutdown_rx.borrow() {
                                tracing::debug!("[ObservabilityWorker] Metrics retention task shutting down");
                                break;
                            }
                        }
                        _ = interval.tick() => {
                            storage.apply_retention();
                            if let Some(rollup_storage) = metrics::get_rollup_storage() {
                                rollup_storage.apply_retention();
                            }
                        }
                    }
                }
            });
        }

        // Spawn background task for alert evaluation. Always spawned (the
        // 10s tick is a no-op while the rule set is empty) so rules hot-added
        // through the configuration worker are evaluated without a restart.
        {
            let mut shutdown_rx = shutdown_rx.clone();

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
                loop {
                    tokio::select! {
                        result = shutdown_rx.changed() => {
                            if result.is_err() {
                                tracing::debug!("[ObservabilityWorker] Shutdown channel closed");
                                break;
                            }
                            if *shutdown_rx.borrow() {
                                tracing::debug!("[ObservabilityWorker] Alert evaluation task shutting down");
                                break;
                            }
                        }
                        _ = interval.tick() => {
                            if let Some(manager) = metrics::get_alert_manager() {
                                let events = manager.evaluate().await;
                                if !events.is_empty() {
                                    tracing::debug!("{} triggered alerts", events.len());
                                }
                            }
                        }
                    }
                }
            });
        }

        // Spawn the respawnable log tasks from the effective configuration.
        // The helpers stop any instance the initial apply_config above may
        // already have spawned, so this cannot double-spawn.
        let effective = self.current_config();
        self.spawn_logs_retention(&effective);
        self.spawn_logs_exporter(&effective);

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Shutting down ObservabilityWorker...");

        // Best-effort: the trigger is registered outside the worker scope, so
        // remove it explicitly to keep ReloadManager restarts duplicate-free.
        let _ = self
            .engine
            .trigger_registry
            .unregister_trigger(
                configuration::CONFIG_TRIGGER_ID.to_string(),
                Some(configuration::CONFIG_TRIGGER_TYPE.to_string()),
            )
            .await;

        // Serialize with any in-flight `apply_config` so a task respawn
        // cannot land after the shutdown below; clearing the stored receiver
        // makes later applies refuse the task-rebuild tier entirely.
        {
            let _guard = self.apply_lock.lock().await;
            self.worker_shutdown_rx
                .lock()
                .expect("worker_shutdown_rx mutex poisoned")
                .take();
        }
        for stop in [
            &self.logs_retention_stop,
            &self.logs_exporter_stop,
            &self.logs_trigger_stop,
        ] {
            if let Some(stop) = stop.lock().expect("stop mutex poisoned").take() {
                let _ = stop.send(true);
            }
        }

        // Signal all background tasks to stop
        let _ = self.shutdown_tx.send(true);

        // Give background tasks time to finish
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Shutdown OTEL components
        otel::shutdown_otel();
        metrics::shutdown_metrics();

        tracing::info!("ObservabilityWorker shutdown complete");
        Ok(())
    }
}

crate::register_worker!(
    "iii-observability",
    ObservabilityWorker,
    description = "OpenTelemetry-based traces, metrics, logs, alerts, and sampling.",
    mandatory
);

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::collections::{HashMap, HashSet};

    // ── Coverage: log-trigger level filter, ingest gate, is_active, collapse ──

    #[test]
    fn should_trigger_for_level_matches_all_and_exact() {
        assert!(should_trigger_for_level("all", "info"));
        assert!(should_trigger_for_level("error", "error"));
        assert!(!should_trigger_for_level("error", "warn"));
        assert!(!should_trigger_for_level("info", "debug"));
    }

    #[test]
    fn is_active_reflects_worker_shutdown_rx() {
        let module = make_test_module(Arc::new(Engine::new()));
        assert!(
            !module.is_active(),
            "for_test/make_test_module leaves rx None"
        );

        let (_tx, rx) = tokio::sync::watch::channel(false);
        *module
            .worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned") = Some(rx);
        assert!(module.is_active(), "set receiver -> active");

        module
            .worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned")
            .take();
        assert!(
            !module.is_active(),
            "destroy clears the receiver -> inactive"
        );
    }

    #[tokio::test]
    #[serial]
    async fn store_and_emit_log_respects_logs_enabled_gate() {
        reset_observability_test_state();
        let module = make_test_module(Arc::new(Engine::new()));
        let storage = otel::get_log_storage().expect("log storage");
        storage.clear();

        let make_input = |body: &str| OtelLogInput {
            trace_id: None,
            span_id: None,
            message: body.to_string(),
            data: None,
            service_name: Some("gate-test".to_string()),
        };

        // Logs disabled -> ingest is a no-op at the gate.
        otel::update_otel_config(config::ObservabilityWorkerConfig {
            logs_enabled: Some(false),
            ..config::ObservabilityWorkerConfig::default()
        });
        let _ = module.log_info(make_input("dropped")).await;
        assert_eq!(storage.len(), 0, "disabled logs must not be stored");

        // Re-enabled -> ingest stores again.
        otel::update_otel_config(config::ObservabilityWorkerConfig {
            logs_enabled: Some(true),
            ..config::ObservabilityWorkerConfig::default()
        });
        let _ = module.log_info(make_input("kept")).await;
        assert_eq!(storage.len(), 1, "re-enabled logs must be stored");
        assert_eq!(storage.get_logs()[0].body, "kept");

        // Leave the process-global config at its unset baseline so serial
        // siblings (e.g. test_initialize_returns_ok_when_disabled) still fall
        // back to their own _config.
        otel::clear_otel_config_for_test();
    }

    #[tokio::test]
    async fn invoke_triggers_for_log_filters_by_level() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let engine = Arc::new(Engine::new());
        let all_hits = Arc::new(AtomicUsize::new(0));
        let error_hits = Arc::new(AtomicUsize::new(0));

        for (fid, counter) in [
            ("test::rec-all", all_hits.clone()),
            ("test::rec-error", error_hits.clone()),
        ] {
            let counter = counter.clone();
            engine.register_function_handler(
                crate::engine::RegisterFunctionRequest {
                    function_id: fid.to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                crate::engine::Handler::new(move |_payload: Value| {
                    let counter = counter.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        FunctionResult::Success(Some(serde_json::json!({ "ok": true })))
                    }
                }),
            );
        }

        let triggers = Arc::new(OtelLogTriggers::new());
        {
            let mut guard = triggers.triggers.write().await;
            guard.insert(Trigger {
                id: "t-all".to_string(),
                trigger_type: LOG_TRIGGER_TYPE.to_string(),
                function_id: "test::rec-all".to_string(),
                config: serde_json::json!({ "level": "all" }),
                worker_id: None,
                metadata: None,
                namespace: "default".to_string(),
                trigger_namespace: None,
                home_namespace: crate::protocol::default_namespace(),
                provider_namespace: crate::protocol::default_namespace(),
            });
            guard.insert(Trigger {
                id: "t-error".to_string(),
                trigger_type: LOG_TRIGGER_TYPE.to_string(),
                function_id: "test::rec-error".to_string(),
                config: serde_json::json!({ "level": "error" }),
                worker_id: None,
                metadata: None,
                namespace: "default".to_string(),
                trigger_namespace: None,
                home_namespace: crate::protocol::default_namespace(),
                provider_namespace: crate::protocol::default_namespace(),
            });
        }

        // A WARN log: matches the "all" trigger, not the "error" one.
        let warn_log = otel::StoredLog {
            timestamp_unix_nano: 1,
            observed_timestamp_unix_nano: 1,
            severity_number: 13,
            severity_text: "WARN".to_string(),
            body: "warn".to_string(),
            attributes: HashMap::new(),
            trace_id: None,
            span_id: None,
            resource: HashMap::new(),
            service_name: "svc".to_string(),
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
        };
        ObservabilityWorker::invoke_triggers_for_log(&triggers, &engine, &warn_log).await;

        // Fan-out is fire-and-forget tokio::spawn; poll for the effect.
        for _ in 0..40 {
            if all_hits.load(Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        assert_eq!(
            all_hits.load(Ordering::SeqCst),
            1,
            "level=all must fire on WARN"
        );
        assert_eq!(
            error_hits.load(Ordering::SeqCst),
            0,
            "level=error must NOT fire on WARN"
        );
    }

    /// Regression: the trace-trigger subscriber must survive span storage
    /// appearing AFTER worker startup (the old one-shot check returned
    /// permanently and trace triggers never fired for the process life),
    /// then deliver coalesced fires that honor the service/status filters
    /// and the loop-prevention exclusions.
    #[tokio::test]
    async fn trace_subscriber_waits_for_late_storage_then_fires_filtered_triggers() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // `engine.call` instruments invocations via the global meter; without
        // it the fire-and-forget delivery tasks panic and no payload arrives.
        metrics::ensure_default_meter();

        let engine = Arc::new(Engine::new());
        let all_payloads: Arc<std::sync::Mutex<Vec<Value>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let error_payloads: Arc<std::sync::Mutex<Vec<Value>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));

        for (fid, sink) in [
            ("test::trace-all", all_payloads.clone()),
            ("test::trace-error", error_payloads.clone()),
        ] {
            engine.register_function_handler(
                crate::engine::RegisterFunctionRequest {
                    function_id: fid.to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                crate::engine::Handler::new(move |payload: Value| {
                    let sink = sink.clone();
                    async move {
                        sink.lock().unwrap().push(payload);
                        FunctionResult::Success(Some(serde_json::json!({ "ok": true })))
                    }
                }),
            );
        }

        let triggers = Arc::new(OtelTraceTriggers::new());
        {
            let mut guard = triggers.triggers.write().await;
            guard.insert(Trigger {
                id: "t-all".to_string(),
                namespace: crate::protocol::DEFAULT_NAMESPACE.to_string(),
                trigger_type: TRACE_TRIGGER_TYPE.to_string(),
                function_id: "test::trace-all".to_string(),
                config: serde_json::json!({}),
                worker_id: None,
                metadata: None,
                trigger_namespace: None,
                home_namespace: crate::protocol::default_namespace(),
                provider_namespace: crate::protocol::default_namespace(),
            });
            guard.insert(Trigger {
                id: "t-error".to_string(),
                namespace: crate::protocol::DEFAULT_NAMESPACE.to_string(),
                trigger_type: TRACE_TRIGGER_TYPE.to_string(),
                function_id: "test::trace-error".to_string(),
                config: serde_json::json!({ "status": "error", "service_name": "svc" }),
                worker_id: None,
                metadata: None,
                trigger_namespace: None,
                home_namespace: crate::protocol::default_namespace(),
                provider_namespace: crate::protocol::default_namespace(),
            });
        }

        // Storage the subscriber must NOT see on its first checks: the lookup
        // misses three times before answering, simulating OTEL pipeline init
        // finishing after worker startup.
        let storage = Arc::new(otel::InMemorySpanStorage::new(64));
        let lookup_calls = Arc::new(AtomicUsize::new(0));
        let lookup = {
            let storage = storage.clone();
            let lookup_calls = lookup_calls.clone();
            move || {
                let n = lookup_calls.fetch_add(1, Ordering::SeqCst);
                if n < 3 { None } else { Some(storage.clone()) }
            }
        };

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let handle = tokio::spawn(ObservabilityWorker::run_trace_trigger_subscriber(
            triggers.clone(),
            engine.clone(),
            shutdown_rx,
            lookup,
        ));

        let ids_of = |payloads: &std::sync::Mutex<Vec<Value>>| -> HashSet<String> {
            payloads
                .lock()
                .unwrap()
                .iter()
                .flat_map(|p| {
                    p.get("trace_ids")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default()
                })
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        };

        // Re-broadcast the same batch until the subscriber has attached and a
        // coalesce tick delivered it: broadcasts before the (late) subscribe
        // are lost by design — exactly the production race under test.
        for _ in 0..30 {
            storage.add_spans(vec![
                // Kept: plain user spans.
                make_span("t-ok", "s1", None, "op", "svc", 1, 2, "ok", vec![]),
                make_span("t-err", "s2", None, "op", "svc", 1, 2, "error", vec![]),
                make_span(
                    "t-err-other",
                    "s3",
                    None,
                    "op",
                    "other",
                    1,
                    2,
                    "error",
                    vec![],
                ),
                // Excluded: the trigger's own delivery span.
                make_span(
                    "t-loop",
                    "s4",
                    None,
                    "call test::trace-all",
                    "svc",
                    1,
                    2,
                    "ok",
                    vec![("function_id", "test::trace-all")],
                ),
                // Excluded: context-free engine machinery span.
                make_span(
                    "t-int",
                    "s5",
                    None,
                    "call stream::send",
                    "iii",
                    1,
                    2,
                    "ok",
                    vec![("iii.function.kind", "internal")],
                ),
                // Excluded: observability-pipeline span (parented, so only the
                // observability-function filter can drop it).
                make_span(
                    "t-obs",
                    "s6",
                    Some("s1"),
                    "call traces::list",
                    "svc",
                    1,
                    2,
                    "ok",
                    vec![("function_id", "iii-observability::spans")],
                ),
            ]);
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            let all = ids_of(&all_payloads);
            if ["t-ok", "t-err", "t-err-other"]
                .iter()
                .all(|t| all.contains(*t))
                && ids_of(&error_payloads).contains("t-err")
            {
                break;
            }
        }

        // One extra coalesce window so any wrongly-included span still in
        // flight lands before the negative assertions.
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;

        assert!(
            lookup_calls.load(Ordering::SeqCst) >= 4,
            "subscriber must retry the storage lookup past initial misses"
        );

        let all = ids_of(&all_payloads);
        for kept in ["t-ok", "t-err", "t-err-other"] {
            assert!(all.contains(kept), "unfiltered trigger must see {kept}");
        }
        for excluded in ["t-loop", "t-int", "t-obs"] {
            assert!(
                !all.contains(excluded),
                "{excluded} must be excluded from the trigger window (feedback loop)"
            );
        }

        let errors = ids_of(&error_payloads);
        assert!(
            errors.contains("t-err"),
            "status/service filtered trigger must see the matching span"
        );
        for filtered in ["t-ok", "t-err-other", "t-loop", "t-int", "t-obs"] {
            assert!(
                !errors.contains(filtered),
                "{filtered} must not pass the status=error + service_name=svc filter"
            );
        }

        // Worker shutdown stops the attached subscriber.
        shutdown_tx.send(true).expect("subscriber still listening");
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("subscriber must exit on shutdown")
            .expect("subscriber task must not panic");
    }

    /// Regression: worker shutdown must terminate the subscriber while it is
    /// still WAITING for span storage (storage never appears — e.g. OTEL
    /// pipeline disabled), instead of the task outliving the worker.
    #[tokio::test]
    async fn trace_subscriber_shutdown_while_waiting_for_storage() {
        let engine = Arc::new(Engine::new());
        let triggers = Arc::new(OtelTraceTriggers::new());
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let handle = tokio::spawn(ObservabilityWorker::run_trace_trigger_subscriber(
            triggers,
            engine,
            shutdown_rx,
            || None,
        ));

        // Let the task enter its wait loop, then signal worker shutdown.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        shutdown_tx.send(true).expect("waiter still listening");

        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("waiting subscriber must exit on shutdown")
            .expect("subscriber task must not panic");
    }

    /// The common production path: storage already initialized when the
    /// subscriber spawns. The first lookup must hit (the wait loop never
    /// runs) and delivery must work end to end.
    #[tokio::test]
    async fn trace_subscriber_attaches_immediately_when_storage_already_present() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // `engine.call` instruments invocations via the global meter; without
        // it the fire-and-forget delivery tasks panic and no payload arrives.
        metrics::ensure_default_meter();

        let engine = Arc::new(Engine::new());
        let payloads: Arc<std::sync::Mutex<Vec<Value>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        {
            let sink = payloads.clone();
            engine.register_function_handler(
                crate::engine::RegisterFunctionRequest {
                    function_id: "test::trace-fast".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                crate::engine::Handler::new(move |payload: Value| {
                    let sink = sink.clone();
                    async move {
                        sink.lock().unwrap().push(payload);
                        FunctionResult::Success(Some(serde_json::json!({ "ok": true })))
                    }
                }),
            );
        }

        let triggers = Arc::new(OtelTraceTriggers::new());
        triggers.triggers.write().await.insert(Trigger {
            id: "t-fast".to_string(),
            namespace: crate::protocol::DEFAULT_NAMESPACE.to_string(),
            trigger_type: TRACE_TRIGGER_TYPE.to_string(),
            function_id: "test::trace-fast".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
            trigger_namespace: None,
            home_namespace: crate::protocol::default_namespace(),
            provider_namespace: crate::protocol::default_namespace(),
        });

        let storage = Arc::new(otel::InMemorySpanStorage::new(16));
        let lookup_calls = Arc::new(AtomicUsize::new(0));
        let lookup = {
            let storage = storage.clone();
            let lookup_calls = lookup_calls.clone();
            move || {
                lookup_calls.fetch_add(1, Ordering::SeqCst);
                Some(storage.clone())
            }
        };

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let handle = tokio::spawn(ObservabilityWorker::run_trace_trigger_subscriber(
            triggers,
            engine,
            shutdown_rx,
            lookup,
        ));

        // Broadcasts sent before the spawned task subscribes are lost, so
        // re-send until a coalesce tick delivers.
        for _ in 0..30 {
            storage.add_spans(vec![make_span(
                "t-fast",
                "s1",
                None,
                "op",
                "svc",
                1,
                2,
                "ok",
                vec![],
            )]);
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            if !payloads.lock().unwrap().is_empty() {
                break;
            }
        }

        assert!(
            payloads.lock().unwrap().iter().any(|p| p["trace_ids"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|v| v == "t-fast"))),
            "trigger must fire with storage present from the start"
        );
        assert_eq!(
            lookup_calls.load(Ordering::SeqCst),
            1,
            "first lookup must hit; the wait loop must not run"
        );

        shutdown_tx.send(true).expect("subscriber still listening");
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("subscriber must exit on shutdown")
            .expect("subscriber task must not panic");
    }

    /// Worker teardown can drop the shutdown sender instead of sending
    /// `true` (e.g. a reload replacing the worker); the subscriber still
    /// waiting for storage must treat the closed channel as shutdown.
    #[tokio::test]
    async fn trace_subscriber_exits_when_shutdown_sender_dropped_while_waiting() {
        let engine = Arc::new(Engine::new());
        let triggers = Arc::new(OtelTraceTriggers::new());
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let handle = tokio::spawn(ObservabilityWorker::run_trace_trigger_subscriber(
            triggers,
            engine,
            shutdown_rx,
            || None,
        ));

        // Let the task enter its wait loop, then drop the sender.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        drop(shutdown_tx);

        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("waiting subscriber must exit when the shutdown sender is dropped")
            .expect("subscriber task must not panic");
    }

    /// Low-level function handler that records the invocation `metadata` each
    /// delivery arrives with — the only handler shape that can observe it
    /// in-process (closure handlers never see metadata).
    struct MetadataCapture(Arc<std::sync::Mutex<Vec<Option<Value>>>>);

    impl crate::function::FunctionHandler for MetadataCapture {
        fn handle_function<'a>(
            &'a self,
            _invocation_id: Option<uuid::Uuid>,
            _function_id: String,
            _input: Value,
            metadata: Option<Value>,
        ) -> std::pin::Pin<
            Box<dyn Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'a>,
        > {
            let sink = self.0.clone();
            Box::pin(async move {
                sink.lock().unwrap().push(metadata);
                FunctionResult::Success(Some(serde_json::json!({ "ok": true })))
            })
        }
    }

    fn register_metadata_capture_ns(
        engine: &Arc<Engine>,
        namespace: &str,
        function_id: &str,
    ) -> Arc<std::sync::Mutex<Vec<Option<Value>>>> {
        let captured: Arc<std::sync::Mutex<Vec<Option<Value>>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        engine.register_function_ns(
            namespace,
            crate::engine::RegisterFunctionRequest {
                function_id: function_id.to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Box::new(MetadataCapture(captured.clone())),
        );
        captured
    }

    async fn wait_for_capture(captured: &Arc<std::sync::Mutex<Vec<Option<Value>>>>) {
        // Fan-out is fire-and-forget tokio::spawn; poll for the effect.
        for _ in 0..40 {
            if !captured.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }

    /// Trace trigger deliveries must resolve the function in the trigger's
    /// namespace and carry the trigger's registered metadata, matching the
    /// state/stream/cron fan-outs. Remote consumers (e.g. session-wake
    /// harnesses) depend on both for routing.
    #[tokio::test]
    async fn fire_trace_triggers_delivers_to_namespace_with_metadata() {
        // `engine.call` instruments invocations via the global meter; without
        // it the fire-and-forget delivery tasks panic and no payload arrives.
        metrics::ensure_default_meter();

        let engine = Arc::new(Engine::new());
        let namespace = "harness-ns";
        let captured = register_metadata_capture_ns(&engine, namespace, "test::trace-meta");

        let triggers = Arc::new(OtelTraceTriggers::new());
        triggers.triggers.write().await.insert(Trigger {
            id: "t-meta".to_string(),
            namespace: namespace.to_string(),
            trigger_type: TRACE_TRIGGER_TYPE.to_string(),
            function_id: "test::trace-meta".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: Some(serde_json::json!({ "__binding": "session-wake-7" })),
            trigger_namespace: None,
            home_namespace: crate::protocol::default_namespace(),
            provider_namespace: crate::protocol::default_namespace(),
        });

        let batch = vec![make_span("t1", "s1", None, "op", "svc", 1, 2, "ok", vec![])];
        ObservabilityWorker::fire_trace_triggers(&triggers, &engine, &batch).await;

        wait_for_capture(&captured).await;
        assert_eq!(
            captured.lock().unwrap().as_slice(),
            &[Some(serde_json::json!({ "__binding": "session-wake-7" }))],
            "trace delivery must carry the trigger's registered metadata"
        );
    }

    /// Log trigger deliveries have the same namespace and metadata contract as
    /// the trace path.
    #[tokio::test]
    async fn log_trigger_delivery_uses_namespace_and_metadata() {
        metrics::ensure_default_meter();

        let engine = Arc::new(Engine::new());
        let namespace = "harness-ns";
        let captured = register_metadata_capture_ns(&engine, namespace, "test::log-meta");

        let triggers = Arc::new(OtelLogTriggers::new());
        triggers.triggers.write().await.insert(Trigger {
            id: "t-log-meta".to_string(),
            namespace: namespace.to_string(),
            trigger_type: LOG_TRIGGER_TYPE.to_string(),
            function_id: "test::log-meta".to_string(),
            config: serde_json::json!({ "level": "all" }),
            worker_id: None,
            metadata: Some(serde_json::json!({ "__binding": "log-wake-1" })),
            trigger_namespace: None,
            home_namespace: crate::protocol::default_namespace(),
            provider_namespace: crate::protocol::default_namespace(),
        });

        let log = make_log(None, None, "INFO", 9, "hello", "svc", 1);
        ObservabilityWorker::invoke_triggers_for_log(&triggers, &engine, &log).await;

        wait_for_capture(&captured).await;
        assert_eq!(
            captured.lock().unwrap().as_slice(),
            &[Some(serde_json::json!({ "__binding": "log-wake-1" }))],
            "log delivery must carry the trigger's registered metadata"
        );
    }

    #[test]
    #[serial]
    fn refresh_collapse_rules_recompiles_cache() {
        refresh_collapse_rules(&[
            config::SpanCollapseRule {
                name: "wrapper*".to_string(),
                service: None,
            },
            config::SpanCollapseRule {
                name: "proxy*".to_string(),
                service: None,
            },
        ]);
        assert_eq!(
            cached_collapse_rules().len(),
            2,
            "refresh must recompile the cache from the new rules"
        );

        refresh_collapse_rules(&[]);
        assert_eq!(
            cached_collapse_rules().len(),
            0,
            "clearing rules must empty the cache"
        );
    }

    // =========================================================================
    // Helper: create a StoredSpan with configurable fields
    // =========================================================================
    #[allow(clippy::too_many_arguments)]
    fn make_span(
        trace_id: &str,
        span_id: &str,
        parent_span_id: Option<&str>,
        name: &str,
        service_name: &str,
        start_ns: u64,
        end_ns: u64,
        status: &str,
        attributes: Vec<(&str, &str)>,
    ) -> otel::StoredSpan {
        otel::StoredSpan {
            trace_id: trace_id.to_string(),
            span_id: span_id.to_string(),
            parent_span_id: parent_span_id.map(|s| s.to_string()),
            name: name.to_string(),
            start_time_unix_nano: start_ns,
            end_time_unix_nano: end_ns,
            status: status.to_string(),
            status_description: None,
            attributes: attributes
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            service_name: service_name.to_string(),
            events: vec![],
            links: vec![],
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
            flags: None,
            trace_state: None,
            pending: false,
        }
    }

    fn make_log(
        trace_id: Option<&str>,
        span_id: Option<&str>,
        severity_text: &str,
        severity_number: i32,
        body: &str,
        service_name: &str,
        timestamp_ns: u64,
    ) -> otel::StoredLog {
        otel::StoredLog {
            timestamp_unix_nano: timestamp_ns,
            observed_timestamp_unix_nano: timestamp_ns,
            severity_number,
            severity_text: severity_text.to_string(),
            body: body.to_string(),
            attributes: HashMap::new(),
            trace_id: trace_id.map(|s| s.to_string()),
            span_id: span_id.map(|s| s.to_string()),
            resource: HashMap::new(),
            service_name: service_name.to_string(),
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
        }
    }

    fn make_test_module(engine: Arc<Engine>) -> ObservabilityWorker {
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        ObservabilityWorker {
            _config: config::ObservabilityWorkerConfig::default(),
            triggers: Arc::new(OtelLogTriggers::new()),
            trace_triggers: Arc::new(OtelTraceTriggers::new()),
            engine,
            shutdown_tx: Arc::new(shutdown_tx),
            worker_shutdown_rx: Arc::new(std::sync::Mutex::new(None)),
            logs_retention_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_exporter_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_trigger_stop: Arc::new(std::sync::Mutex::new(None)),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    fn reset_observability_test_state() {
        metrics::ensure_default_meter();

        if let Some(storage) = otel::get_log_storage() {
            storage.clear();
        } else {
            otel::init_log_storage(Some(128));
        }

        if let Some(storage) = otel::get_span_storage() {
            storage.clear();
        } else {
            let _ = otel::InMemorySpanExporter::new(128, "test".to_string());
        }

        if let Some(storage) = metrics::get_metric_storage() {
            storage.clear();
        } else {
            metrics::init_metric_storage(Some(128), Some(3600));
        }
    }

    struct OtelConfigTestGuard(Option<Arc<config::ObservabilityWorkerConfig>>);

    impl OtelConfigTestGuard {
        fn install(config: config::ObservabilityWorkerConfig) -> Self {
            Self(otel::update_otel_config(config))
        }
    }

    impl Drop for OtelConfigTestGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(previous) => {
                    otel::update_otel_config((*previous).clone());
                }
                None => otel::clear_otel_config_for_test(),
            }
        }
    }

    fn make_number_metric(
        name: &str,
        value: f64,
        timestamp_unix_nano: u64,
    ) -> metrics::StoredMetric {
        metrics::StoredMetric {
            name: name.to_string(),
            description: "test metric".to_string(),
            unit: "1".to_string(),
            metric_type: metrics::StoredMetricType::Gauge,
            data_points: vec![metrics::StoredDataPoint::Number(
                metrics::StoredNumberDataPoint {
                    value,
                    attributes: vec![("worker.id".to_string(), "worker-1".to_string())],
                    timestamp_unix_nano,
                },
            )],
            service_name: "svc".to_string(),
            timestamp_unix_nano,
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
        }
    }

    // =========================================================================
    // build_span_tree tests
    // =========================================================================

    #[test]
    fn test_build_span_tree_empty() {
        let tree = build_span_tree(vec![]);
        assert!(tree.is_empty());
    }

    #[test]
    fn test_build_span_tree_single_root() {
        let span = make_span("t1", "s1", None, "root", "svc", 100, 200, "ok", vec![]);
        let tree = build_span_tree(vec![span]);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].span.name, "root");
        assert_eq!(tree[0].span.span_id, "s1");
        assert!(tree[0].children.is_empty());
    }

    #[test]
    fn test_build_span_tree_parent_child() {
        let root = make_span("t1", "s1", None, "root", "svc", 100, 500, "ok", vec![]);
        let child = make_span(
            "t1",
            "s2",
            Some("s1"),
            "child",
            "svc",
            150,
            400,
            "ok",
            vec![],
        );

        let tree = build_span_tree(vec![root, child]);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].span.name, "root");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].span.name, "child");
    }

    #[test]
    fn test_collapse_spans_removes_and_reparents() {
        // call (engine) -> trigger (worker wrapper) -> harness.<fn> (handler)
        let root = make_span(
            "t",
            "call",
            None,
            "call h::trigger",
            "iii-test",
            100,
            500,
            "ok",
            vec![],
        );
        let wrapper = make_span(
            "t",
            "trig",
            Some("call"),
            "trigger h::trigger",
            "harness",
            110,
            480,
            "ok",
            vec![],
        );
        let leaf = make_span(
            "t",
            "leaf",
            Some("trig"),
            "harness.h::trigger",
            "harness",
            120,
            470,
            "ok",
            vec![],
        );

        let rules = compile_collapse_rules(&[config::SpanCollapseRule {
            name: "trigger *".to_string(),
            service: Some("harness".to_string()),
        }]);
        let collapsed = collapse_spans(vec![root, wrapper, leaf], &rules);

        // wrapper removed, leaf reparented to the wrapper's parent (call)
        assert_eq!(collapsed.len(), 2);
        assert!(!collapsed.iter().any(|s| s.span_id == "trig"));
        let leaf = collapsed.iter().find(|s| s.span_id == "leaf").unwrap();
        assert_eq!(leaf.parent_span_id.as_deref(), Some("call"));

        // tree stays connected: call -> leaf
        let tree = build_span_tree(collapsed);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].span.span_id, "call");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].span.span_id, "leaf");
    }

    #[test]
    fn test_prune_empty_trigger_wrappers() {
        // writer -> state_triggers -> turn::on_approval: a trigger that RAN a fn.
        let writer = make_span(
            "t",
            "w",
            None,
            "call approval::resolve",
            "iii-test",
            100,
            500,
            "ok",
            vec![],
        );
        let ran = make_span(
            "t",
            "st",
            Some("w"),
            "state_triggers",
            "iii-test",
            110,
            480,
            "ok",
            vec![("iii.function.kind", "internal")],
        );
        let handler = make_span(
            "t",
            "h",
            Some("st"),
            "call turn::on_approval",
            "iii-worker",
            120,
            470,
            "ok",
            vec![],
        );
        // a stream_triggers wrapper with no children → no-op fan-out (noise).
        let empty = make_span(
            "t",
            "ss",
            Some("w"),
            "stream_triggers",
            "iii-test",
            130,
            140,
            "ok",
            vec![("iii.function.kind", "internal")],
        );

        let pruned = prune_empty_trigger_spans(vec![writer, ran, handler, empty]);

        // Empty wrapper dropped; the one that ran a function is KEPT.
        assert!(
            !pruned.iter().any(|s| s.span_id == "ss"),
            "childless stream_triggers should be pruned",
        );
        assert!(
            pruned.iter().any(|s| s.span_id == "st"),
            "state_triggers that invoked a handler should be kept",
        );
        assert!(
            pruned.iter().any(|s| s.span_id == "h"),
            "the handler survives"
        );

        // Tree stays connected: approval::resolve -> state_triggers -> turn::on_approval.
        let tree = build_span_tree(pruned);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].span.span_id, "w");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].span.span_id, "st");
        assert_eq!(tree[0].children[0].children[0].span.span_id, "h");
    }

    #[test]
    fn test_collapse_spans_service_scoping() {
        // The engine's own `trigger *` (service iii-test) must survive; only the
        // worker's `trigger *` (service harness) collapses.
        let engine_trigger = make_span(
            "t",
            "et",
            None,
            "trigger foo",
            "iii-test",
            100,
            500,
            "ok",
            vec![],
        );
        let worker_trigger = make_span(
            "t",
            "wt",
            Some("et"),
            "trigger foo",
            "harness",
            110,
            480,
            "ok",
            vec![],
        );
        let leaf = make_span(
            "t",
            "leaf",
            Some("wt"),
            "foo.body",
            "harness",
            120,
            470,
            "ok",
            vec![],
        );

        let rules = compile_collapse_rules(&[config::SpanCollapseRule {
            name: "trigger *".to_string(),
            service: Some("harness".to_string()),
        }]);
        let collapsed = collapse_spans(vec![engine_trigger, worker_trigger, leaf], &rules);

        assert!(
            collapsed.iter().any(|s| s.span_id == "et"),
            "engine trigger survives"
        );
        assert!(
            !collapsed.iter().any(|s| s.span_id == "wt"),
            "worker trigger collapsed"
        );
        let leaf = collapsed.iter().find(|s| s.span_id == "leaf").unwrap();
        assert_eq!(
            leaf.parent_span_id.as_deref(),
            Some("et"),
            "leaf reparented past the collapsed worker trigger"
        );
    }

    #[test]
    fn test_collapse_spans_no_rules_is_noop() {
        let a = make_span("t", "a", None, "x", "svc", 1, 2, "ok", vec![]);
        let b = make_span("t", "b", Some("a"), "y", "svc", 1, 2, "ok", vec![]);
        let out = collapse_spans(vec![a, b], &[]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn test_build_span_tree_multiple_children() {
        let root = make_span("t1", "s1", None, "root", "svc", 100, 500, "ok", vec![]);
        let child1 = make_span(
            "t1",
            "s2",
            Some("s1"),
            "child1",
            "svc",
            150,
            300,
            "ok",
            vec![],
        );
        let child2 = make_span(
            "t1",
            "s3",
            Some("s1"),
            "child2",
            "svc",
            200,
            400,
            "ok",
            vec![],
        );

        let tree = build_span_tree(vec![root, child1, child2]);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 2);
    }

    #[test]
    fn test_build_span_tree_deep_nesting() {
        let root = make_span("t1", "s1", None, "root", "svc", 100, 600, "ok", vec![]);
        let child = make_span(
            "t1",
            "s2",
            Some("s1"),
            "child",
            "svc",
            110,
            500,
            "ok",
            vec![],
        );
        let grandchild = make_span(
            "t1",
            "s3",
            Some("s2"),
            "grandchild",
            "svc",
            120,
            400,
            "ok",
            vec![],
        );

        let tree = build_span_tree(vec![root, child, grandchild]);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].span.name, "root");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].span.name, "child");
        assert_eq!(tree[0].children[0].children.len(), 1);
        assert_eq!(tree[0].children[0].children[0].span.name, "grandchild");
        assert!(tree[0].children[0].children[0].children.is_empty());
    }

    #[test]
    fn test_build_span_tree_multiple_roots() {
        let root1 = make_span("t1", "s1", None, "root1", "svc", 100, 300, "ok", vec![]);
        let root2 = make_span("t1", "s2", None, "root2", "svc", 200, 400, "ok", vec![]);

        let tree = build_span_tree(vec![root1, root2]);

        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn test_build_span_tree_preserves_span_data() {
        let span = make_span(
            "trace-abc",
            "span-123",
            None,
            "my-operation",
            "my-service",
            1000,
            2000,
            "error",
            vec![("key1", "val1")],
        );

        let tree = build_span_tree(vec![span]);

        assert_eq!(tree[0].span.trace_id, "trace-abc");
        assert_eq!(tree[0].span.span_id, "span-123");
        assert_eq!(tree[0].span.name, "my-operation");
        assert_eq!(tree[0].span.service_name, "my-service");
        assert_eq!(tree[0].span.start_time_unix_nano, 1000);
        assert_eq!(tree[0].span.end_time_unix_nano, 2000);
        assert_eq!(tree[0].span.status, "error");
        assert_eq!(
            tree[0].span.attributes,
            vec![("key1".to_string(), "val1".to_string())]
        );
    }

    // =========================================================================
    // InMemorySpanStorage tests
    // =========================================================================

    #[test]
    fn test_span_storage_new_empty() {
        let storage = otel::InMemorySpanStorage::new(100);
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_span_storage_add_and_get() {
        let storage = otel::InMemorySpanStorage::new(100);
        let span = make_span("t1", "s1", None, "test", "svc", 100, 200, "ok", vec![]);

        storage.add_spans(vec![span]);

        assert_eq!(storage.len(), 1);
        assert!(!storage.is_empty());

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "test");
    }

    #[test]
    fn test_span_storage_get_by_trace_id() {
        let storage = otel::InMemorySpanStorage::new(100);
        let span1 = make_span("t1", "s1", None, "span1", "svc", 100, 200, "ok", vec![]);
        let span2 = make_span("t2", "s2", None, "span2", "svc", 100, 200, "ok", vec![]);
        let span3 = make_span("t1", "s3", None, "span3", "svc", 300, 400, "ok", vec![]);

        storage.add_spans(vec![span1, span2, span3]);

        let t1_spans = storage.get_spans_by_trace_id("t1");
        assert_eq!(t1_spans.len(), 2);

        let t2_spans = storage.get_spans_by_trace_id("t2");
        assert_eq!(t2_spans.len(), 1);
        assert_eq!(t2_spans[0].name, "span2");

        let t3_spans = storage.get_spans_by_trace_id("nonexistent");
        assert!(t3_spans.is_empty());
    }

    // Serial: eviction protects dirty spans whenever the GLOBAL archive is
    // attached (`evict_to_capacity` consults it), so this must not overlap
    // the `#[serial]` archive tests.
    #[test]
    #[serial]
    fn test_span_storage_eviction() {
        let storage = otel::InMemorySpanStorage::new(3);
        let span1 = make_span("t1", "s1", None, "first", "svc", 100, 200, "ok", vec![]);
        let span2 = make_span("t2", "s2", None, "second", "svc", 200, 300, "ok", vec![]);
        let span3 = make_span("t3", "s3", None, "third", "svc", 300, 400, "ok", vec![]);

        storage.add_spans(vec![span1, span2, span3]);
        assert_eq!(storage.len(), 3);

        // Adding a 4th span should evict the first
        let span4 = make_span("t4", "s4", None, "fourth", "svc", 400, 500, "ok", vec![]);
        storage.add_spans(vec![span4]);

        assert_eq!(storage.len(), 3);
        let spans = storage.get_spans();
        assert_eq!(spans[0].name, "second");
        assert_eq!(spans[1].name, "third");
        assert_eq!(spans[2].name, "fourth");

        // Evicted trace should be gone from index
        let t1_spans = storage.get_spans_by_trace_id("t1");
        assert!(t1_spans.is_empty());
    }

    #[test]
    fn test_span_storage_clear() {
        let storage = otel::InMemorySpanStorage::new(100);
        storage.add_spans(vec![
            make_span("t1", "s1", None, "a", "svc", 100, 200, "ok", vec![]),
            make_span("t2", "s2", None, "b", "svc", 200, 300, "ok", vec![]),
        ]);

        assert_eq!(storage.len(), 2);
        storage.clear();
        assert_eq!(storage.len(), 0);
        assert!(storage.is_empty());
        assert!(storage.get_spans().is_empty());
        assert!(storage.get_spans_by_trace_id("t1").is_empty());
    }

    #[test]
    fn test_span_storage_performance_metrics_empty() {
        let storage = otel::InMemorySpanStorage::new(100);
        let (avg, p50, p95, p99, min, max) = storage.calculate_performance_metrics();

        assert_eq!(avg, 0.0);
        assert_eq!(p50, 0.0);
        assert_eq!(p95, 0.0);
        assert_eq!(p99, 0.0);
        assert_eq!(min, 0.0);
        assert_eq!(max, 0.0);
    }

    #[test]
    fn test_span_storage_performance_metrics_single_span() {
        let storage = otel::InMemorySpanStorage::new(100);
        // Duration = 10_000_000 ns = 10 ms
        let span = make_span("t1", "s1", None, "test", "svc", 0, 10_000_000, "ok", vec![]);
        storage.add_spans(vec![span]);

        let (avg, p50, _p95, _p99, min, max) = storage.calculate_performance_metrics();

        assert!((avg - 10.0).abs() < 0.001);
        assert!((p50 - 10.0).abs() < 0.001);
        assert!((min - 10.0).abs() < 0.001);
        assert!((max - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_span_storage_performance_metrics_multiple_spans() {
        let storage = otel::InMemorySpanStorage::new(100);
        // Durations: 5ms, 10ms, 15ms, 20ms, 25ms
        storage.add_spans(vec![
            make_span("t1", "s1", None, "a", "svc", 0, 5_000_000, "ok", vec![]),
            make_span("t2", "s2", None, "b", "svc", 0, 10_000_000, "ok", vec![]),
            make_span("t3", "s3", None, "c", "svc", 0, 15_000_000, "ok", vec![]),
            make_span("t4", "s4", None, "d", "svc", 0, 20_000_000, "ok", vec![]),
            make_span("t5", "s5", None, "e", "svc", 0, 25_000_000, "ok", vec![]),
        ]);

        let (avg, _p50, _p95, _p99, min, max) = storage.calculate_performance_metrics();

        // avg = (5+10+15+20+25)/5 = 15
        assert!((avg - 15.0).abs() < 0.001);
        assert!((min - 5.0).abs() < 0.001);
        assert!((max - 25.0).abs() < 0.001);
    }

    // =========================================================================
    // InMemoryLogStorage tests
    // =========================================================================

    #[test]
    fn test_log_storage_new_empty() {
        let storage = otel::InMemoryLogStorage::new(100);
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_log_storage_apply_retention_drops_old_and_keeps_recent() {
        let storage = otel::InMemoryLogStorage::new(100);
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let one_hour_ns: u64 = 3600 * 1_000_000_000;
        let old_ts = now_ns.saturating_sub(2 * one_hour_ns); // 2h ago
        let recent_ts = now_ns.saturating_sub(60 * 1_000_000_000); // 1m ago

        storage.store(make_log(None, None, "INFO", 9, "old", "svc", old_ts));
        storage.store(make_log(None, None, "INFO", 9, "recent", "svc", recent_ts));
        assert_eq!(storage.len(), 2);

        // Retain only last hour: old entry must be dropped, recent kept.
        storage.apply_retention(one_hour_ns);

        let logs = storage.get_logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].body, "recent");
    }

    #[test]
    fn test_log_storage_apply_retention_falls_back_to_observed_timestamp_when_event_time_zero() {
        // Regression test: OTLP logs spec allows time_unix_nano == 0 to mean
        // "unknown event time". Receivers must fall back to
        // observed_time_unix_nano. Without the fallback, such logs are
        // evicted on the first retention tick despite a valid observation
        // time.
        let storage = otel::InMemoryLogStorage::new(100);
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let one_hour_ns: u64 = 3600 * 1_000_000_000;
        let recent_observed = now_ns.saturating_sub(60 * 1_000_000_000); // 1m ago

        // Hand-craft a log with timestamp_unix_nano == 0 but a recent
        // observed_timestamp_unix_nano — the exact shape produced by
        // ingest_otlp_logs when the SDK sends time_unix_nano=0.
        let log = otel::StoredLog {
            timestamp_unix_nano: 0,
            observed_timestamp_unix_nano: recent_observed,
            severity_number: 9,
            severity_text: "INFO".to_string(),
            body: "observed-only".to_string(),
            attributes: HashMap::new(),
            trace_id: None,
            span_id: None,
            resource: HashMap::new(),
            service_name: "svc".to_string(),
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
        };
        storage.store(log);
        assert_eq!(storage.len(), 1);

        storage.apply_retention(one_hour_ns);

        let logs = storage.get_logs();
        assert_eq!(
            logs.len(),
            1,
            "log with zero event timestamp must be preserved via observed timestamp"
        );
        assert_eq!(logs[0].body, "observed-only");
    }

    #[test]
    fn test_log_storage_apply_retention_evicts_when_both_timestamps_expired() {
        // Complement to the fallback test: if BOTH timestamps are expired,
        // the log must still be evicted.
        let storage = otel::InMemoryLogStorage::new(100);
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let one_hour_ns: u64 = 3600 * 1_000_000_000;
        let old_observed = now_ns.saturating_sub(2 * one_hour_ns); // 2h ago

        let log = otel::StoredLog {
            timestamp_unix_nano: 0,
            observed_timestamp_unix_nano: old_observed,
            severity_number: 9,
            severity_text: "INFO".to_string(),
            body: "stale-observed".to_string(),
            attributes: HashMap::new(),
            trace_id: None,
            span_id: None,
            resource: HashMap::new(),
            service_name: "svc".to_string(),
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
        };
        storage.store(log);
        storage.apply_retention(one_hour_ns);

        assert_eq!(storage.get_logs().len(), 0);
    }

    #[test]
    fn test_log_storage_apply_retention_scans_whole_buffer_for_out_of_order() {
        // Regression test: logs are stored in arrival order, not timestamp
        // order. An older-timestamped log that lands AFTER a newer one must
        // still be evicted by retention. Proves apply_retention scans the
        // entire buffer rather than stopping at the first non-expired front.
        let storage = otel::InMemoryLogStorage::new(100);
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let one_hour_ns: u64 = 3600 * 1_000_000_000;
        let old_ts = now_ns.saturating_sub(2 * one_hour_ns); // 2h ago, expired
        let recent_ts = now_ns.saturating_sub(60 * 1_000_000_000); // 1m ago, fresh

        // Arrival order puts the fresh log FIRST, then a backdated log
        // (simulates clock skew / SDK batch flushing older records).
        storage.store(make_log(None, None, "INFO", 9, "recent", "svc", recent_ts));
        storage.store(make_log(None, None, "INFO", 9, "backdated", "svc", old_ts));
        assert_eq!(storage.len(), 2);

        storage.apply_retention(one_hour_ns);

        let logs = storage.get_logs();
        assert_eq!(
            logs.len(),
            1,
            "backdated entry must be evicted even when trapped behind a newer one"
        );
        assert_eq!(logs[0].body, "recent");
    }

    #[test]
    fn test_log_storage_store_and_get() {
        let storage = otel::InMemoryLogStorage::new(100);
        let log = make_log(Some("t1"), Some("s1"), "INFO", 9, "hello", "svc", 1000);

        storage.store(log);

        assert_eq!(storage.len(), 1);
        let logs = storage.get_logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].body, "hello");
    }

    #[test]
    fn test_log_storage_add_logs_bulk() {
        let storage = otel::InMemoryLogStorage::new(100);
        let logs = vec![
            make_log(None, None, "INFO", 9, "msg1", "svc", 1000),
            make_log(None, None, "WARN", 13, "msg2", "svc", 2000),
            make_log(None, None, "ERROR", 17, "msg3", "svc", 3000),
        ];

        storage.add_logs(logs);

        assert_eq!(storage.len(), 3);
    }

    #[test]
    fn test_log_storage_eviction() {
        let storage = otel::InMemoryLogStorage::new(2);
        storage.store(make_log(None, None, "INFO", 9, "first", "svc", 1000));
        storage.store(make_log(None, None, "INFO", 9, "second", "svc", 2000));
        storage.store(make_log(None, None, "INFO", 9, "third", "svc", 3000));

        assert_eq!(storage.len(), 2);
        let logs = storage.get_logs();
        assert_eq!(logs[0].body, "second");
        assert_eq!(logs[1].body, "third");
    }

    #[test]
    fn test_log_storage_clear() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.store(make_log(None, None, "INFO", 9, "msg", "svc", 1000));

        storage.clear();
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_log_storage_get_by_trace_id() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.add_logs(vec![
            make_log(Some("t1"), None, "INFO", 9, "a", "svc", 1000),
            make_log(Some("t2"), None, "WARN", 13, "b", "svc", 2000),
            make_log(Some("t1"), None, "ERROR", 17, "c", "svc", 3000),
            make_log(None, None, "DEBUG", 5, "d", "svc", 4000),
        ]);

        let t1_logs = storage.get_logs_by_trace_id("t1");
        assert_eq!(t1_logs.len(), 2);

        let t2_logs = storage.get_logs_by_trace_id("t2");
        assert_eq!(t2_logs.len(), 1);
        assert_eq!(t2_logs[0].body, "b");

        let no_logs = storage.get_logs_by_trace_id("nonexistent");
        assert!(no_logs.is_empty());
    }

    #[test]
    fn test_log_storage_get_by_span_id() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.add_logs(vec![
            make_log(None, Some("span1"), "INFO", 9, "a", "svc", 1000),
            make_log(None, Some("span2"), "WARN", 13, "b", "svc", 2000),
            make_log(None, Some("span1"), "ERROR", 17, "c", "svc", 3000),
        ]);

        let s1_logs = storage.get_logs_by_span_id("span1");
        assert_eq!(s1_logs.len(), 2);

        let s2_logs = storage.get_logs_by_span_id("span2");
        assert_eq!(s2_logs.len(), 1);
    }

    // =========================================================================
    // get_logs_filtered tests
    // =========================================================================

    #[test]
    fn test_log_storage_filtered_no_filters() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.add_logs(vec![
            make_log(None, None, "INFO", 9, "a", "svc", 1000),
            make_log(None, None, "WARN", 13, "b", "svc", 2000),
        ]);

        let (total, logs) =
            storage.get_logs_filtered(None, None, None, None, None, None, None, None);
        assert_eq!(total, 2);
        assert_eq!(logs.len(), 2);
        // Results should be sorted newest first
        assert_eq!(logs[0].body, "b");
        assert_eq!(logs[1].body, "a");
    }

    #[test]
    fn test_log_storage_filtered_by_trace_id() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.add_logs(vec![
            make_log(Some("t1"), None, "INFO", 9, "a", "svc", 1000),
            make_log(Some("t2"), None, "WARN", 13, "b", "svc", 2000),
            make_log(Some("t1"), None, "ERROR", 17, "c", "svc", 3000),
        ]);

        let (total, logs) =
            storage.get_logs_filtered(Some("t1"), None, None, None, None, None, None, None);
        assert_eq!(total, 2);
        assert_eq!(logs.len(), 2);
    }

    #[test]
    fn test_log_storage_filtered_by_span_id() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.add_logs(vec![
            make_log(None, Some("s1"), "INFO", 9, "a", "svc", 1000),
            make_log(None, Some("s2"), "WARN", 13, "b", "svc", 2000),
        ]);

        let (total, logs) =
            storage.get_logs_filtered(None, Some("s1"), None, None, None, None, None, None);
        assert_eq!(total, 1);
        assert_eq!(logs[0].body, "a");
    }

    #[test]
    fn test_log_storage_filtered_by_severity_min() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.add_logs(vec![
            make_log(None, None, "DEBUG", 5, "debug", "svc", 1000),
            make_log(None, None, "INFO", 9, "info", "svc", 2000),
            make_log(None, None, "WARN", 13, "warn", "svc", 3000),
            make_log(None, None, "ERROR", 17, "error", "svc", 4000),
        ]);

        // severity_min = 13 should return WARN and ERROR
        let (total, logs) =
            storage.get_logs_filtered(None, None, Some(13), None, None, None, None, None);
        assert_eq!(total, 2);
        assert!(logs.iter().all(|l| l.severity_number >= 13));
    }

    #[test]
    fn test_log_storage_filtered_by_severity_text() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.add_logs(vec![
            make_log(None, None, "INFO", 9, "info-msg", "svc", 1000),
            make_log(None, None, "WARN", 13, "warn-msg", "svc", 2000),
            make_log(None, None, "ERROR", 17, "error-msg", "svc", 3000),
        ]);

        // Case-insensitive match
        let (total, logs) =
            storage.get_logs_filtered(None, None, None, Some("warn"), None, None, None, None);
        assert_eq!(total, 1);
        assert_eq!(logs[0].severity_text, "WARN");
    }

    #[test]
    fn test_log_storage_filtered_by_time_range() {
        let storage = otel::InMemoryLogStorage::new(100);
        // Timestamps in nanoseconds; filter uses milliseconds
        storage.add_logs(vec![
            make_log(None, None, "INFO", 9, "old", "svc", 1_000_000_000), // 1000 ms
            make_log(None, None, "INFO", 9, "mid", "svc", 2_000_000_000), // 2000 ms
            make_log(None, None, "INFO", 9, "new", "svc", 3_000_000_000), // 3000 ms
        ]);

        // start_time=1500ms, end_time=2500ms -> only "mid" at 2000ms matches
        let (total, logs) = storage.get_logs_filtered(
            None,
            None,
            None,
            None,
            Some(1500), // 1500 ms = 1_500_000_000 ns
            Some(2500), // 2500 ms = 2_500_000_000 ns
            None,
            None,
        );
        assert_eq!(total, 1);
        assert_eq!(logs[0].body, "mid");
    }

    #[test]
    fn test_log_storage_filtered_pagination() {
        let storage = otel::InMemoryLogStorage::new(100);
        for i in 0..10 {
            storage.store(make_log(
                None,
                None,
                "INFO",
                9,
                &format!("msg-{}", i),
                "svc",
                (i + 1) * 1000,
            ));
        }

        // offset=2, limit=3
        let (total, logs) =
            storage.get_logs_filtered(None, None, None, None, None, None, Some(2), Some(3));
        assert_eq!(total, 10);
        assert_eq!(logs.len(), 3);
    }

    #[test]
    fn test_log_storage_filtered_combined() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.add_logs(vec![
            make_log(Some("t1"), Some("s1"), "INFO", 9, "a", "svc", 1_000_000_000),
            make_log(
                Some("t1"),
                Some("s1"),
                "ERROR",
                17,
                "b",
                "svc",
                2_000_000_000,
            ),
            make_log(
                Some("t1"),
                Some("s2"),
                "ERROR",
                17,
                "c",
                "svc",
                3_000_000_000,
            ),
            make_log(
                Some("t2"),
                Some("s1"),
                "ERROR",
                17,
                "d",
                "svc",
                4_000_000_000,
            ),
        ]);

        // Filter: trace_id=t1 AND span_id=s1 AND severity_min=13
        let (total, logs) = storage.get_logs_filtered(
            Some("t1"),
            Some("s1"),
            Some(13),
            None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(total, 1);
        assert_eq!(logs[0].body, "b");
    }

    #[test]
    fn test_log_storage_subscribe_broadcast() {
        let storage = otel::InMemoryLogStorage::new(100);
        let mut rx = storage.subscribe();

        let log = make_log(None, None, "INFO", 9, "broadcast", "svc", 1000);
        storage.store(log);

        // The broadcast receiver should have received the log
        let received = rx.try_recv();
        assert!(received.is_ok());
        assert_eq!(received.unwrap().body, "broadcast");
    }

    // =========================================================================
    // OtelLogTriggers tests
    // =========================================================================

    #[test]
    fn test_otel_log_triggers_default() {
        let triggers = OtelLogTriggers::default();
        // Should create with empty triggers
        let triggers_arc = triggers.triggers.clone();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let triggers_set = rt.block_on(async { triggers_arc.read().await.len() });
        assert_eq!(triggers_set, 0);
    }

    #[test]
    fn test_otel_log_triggers_new() {
        let triggers = OtelLogTriggers::new();
        let triggers_arc = triggers.triggers.clone();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let triggers_set = rt.block_on(async { triggers_arc.read().await.len() });
        assert_eq!(triggers_set, 0);
    }

    // =========================================================================
    // Trace (span) trigger tests — mirror the log trigger
    // =========================================================================

    #[test]
    fn test_trace_trigger_type_constant() {
        assert_eq!(TRACE_TRIGGER_TYPE, "trace");
    }

    #[test]
    fn test_otel_trace_triggers_default_is_empty() {
        let triggers = OtelTraceTriggers::default();
        let triggers_arc = triggers.triggers.clone();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let len = rt.block_on(async { triggers_arc.read().await.len() });
        assert_eq!(len, 0);
    }

    #[test]
    fn test_otel_trace_triggers_new_is_empty() {
        let triggers = OtelTraceTriggers::new();
        let triggers_arc = triggers.triggers.clone();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let len = rt.block_on(async { triggers_arc.read().await.len() });
        assert_eq!(len, 0);
    }

    #[test]
    fn test_span_storage_subscribe_broadcast() {
        let storage = otel::InMemorySpanStorage::new(100);
        let mut rx = storage.subscribe();

        let span = make_span("t1", "s1", None, "GET /", "svc", 1000, 2000, "ok", vec![]);
        storage.add_spans(vec![span]);

        // The broadcast receiver should have received the span.
        let received = rx.try_recv();
        assert!(received.is_ok());
        let received = received.unwrap();
        assert_eq!(received.trace_id, "t1");
        assert_eq!(received.span_id, "s1");
    }

    #[test]
    fn test_span_storage_broadcast_one_per_span() {
        let storage = otel::InMemorySpanStorage::new(100);
        let mut rx = storage.subscribe();

        storage.add_spans(vec![
            make_span("t1", "s1", None, "a", "svc", 1, 2, "ok", vec![]),
            make_span("t1", "s2", Some("s1"), "b", "svc", 1, 2, "ok", vec![]),
        ]);

        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err()); // exactly two, one per span
    }

    #[test]
    fn test_should_trigger_for_span_filters() {
        let span = make_span("t1", "s1", None, "GET /", "checkout", 1, 2, "error", vec![]);

        // No filters → always fires.
        assert!(should_trigger_for_span(None, None, &span));
        // Matching service (case-insensitive) → fires.
        assert!(should_trigger_for_span(Some("CHECKOUT"), None, &span));
        // Non-matching service → suppressed.
        assert!(!should_trigger_for_span(Some("billing"), None, &span));
        // Matching status → fires.
        assert!(should_trigger_for_span(None, Some("error"), &span));
        // Non-matching status → suppressed.
        assert!(!should_trigger_for_span(None, Some("ok"), &span));
        // Both must match (AND).
        assert!(should_trigger_for_span(
            Some("checkout"),
            Some("error"),
            &span
        ));
        assert!(!should_trigger_for_span(
            Some("checkout"),
            Some("ok"),
            &span
        ));
    }

    #[test]
    fn test_is_internal_span_and_function_id() {
        // engine:: function id → internal (excluded from the trigger).
        let engine_span = make_span(
            "t",
            "s",
            None,
            "n",
            "svc",
            1,
            2,
            "ok",
            vec![("function_id", "engine::traces::list")],
        );
        assert!(is_internal_span(&engine_span));

        // iii.function.kind=internal → internal.
        let kind_internal = make_span(
            "t",
            "s",
            None,
            "n",
            "svc",
            1,
            2,
            "ok",
            vec![("iii.function.kind", "internal")],
        );
        assert!(is_internal_span(&kind_internal));

        // A trigger-delivery span (a console fn) is NOT "internal" — it is
        // excluded by the function-id loop-break, not by is_internal.
        let delivery = make_span(
            "t",
            "s",
            None,
            "n",
            "svc",
            1,
            2,
            "ok",
            vec![("function_id", "console::devtools::traces_changed::b1")],
        );
        assert!(!is_internal_span(&delivery));
        assert_eq!(
            span_function_id(&delivery),
            Some("console::devtools::traces_changed::b1")
        );

        // No function_id attribute → None, not internal.
        let bare = make_span("t", "s", None, "n", "svc", 1, 2, "ok", vec![]);
        assert!(!is_internal_span(&bare));
        assert_eq!(span_function_id(&bare), None);
    }

    #[test]
    fn trace_root_candidate_filter_only_rejects_root_determined_mismatches() {
        let root = make_span(
            "trace",
            "root",
            None,
            "checkout request",
            "checkout",
            1,
            2,
            "ok",
            vec![("tenant", "alpha")],
        );

        assert!(!trace_might_match_root_filters(
            std::slice::from_ref(&root),
            &TracesListInput {
                service_name: Some("billing".to_string()),
                ..Default::default()
            },
            false,
        ));
        assert!(!trace_might_match_root_filters(
            std::slice::from_ref(&root),
            &TracesListInput {
                name: Some("child operation".to_string()),
                ..Default::default()
            },
            false,
        ));
        assert!(trace_might_match_root_filters(
            std::slice::from_ref(&root),
            &TracesListInput {
                name: Some("child operation".to_string()),
                search_all_spans: Some(true),
                ..Default::default()
            },
            false,
        ));
        assert!(trace_might_match_root_filters(
            std::slice::from_ref(&root),
            &TracesListInput {
                // Aggregate status cannot be decided from the root alone.
                status: Some("error".to_string()),
                ..Default::default()
            },
            false,
        ));

        let internal_root = make_span(
            "internal-trace",
            "internal-root",
            None,
            "internal",
            "iii",
            1,
            2,
            "ok",
            vec![("function_id", "engine::traces::list")],
        );
        assert!(trace_might_match_root_filters(
            &[internal_root],
            &TracesListInput {
                service_name: Some("application".to_string()),
                ..Default::default()
            },
            false,
        ));
    }

    #[test]
    fn pending_internal_span_is_still_filtered_from_trigger_feed() {
        // Loop-safety regression: a live (pending) snapshot of an internal
        // wrapper span carries its `iii.function.kind=internal` creation attr,
        // so the trace-trigger subscriber's `is_internal_span` post-filter
        // excludes it exactly like the final span — pending snapshots must not
        // re-fire the feed that produced them.
        let mut pending_wrapper = make_span(
            "t",
            "s",
            None,
            "stream_triggers",
            "iii",
            1,
            0,
            "unset",
            vec![("iii.function.kind", "internal")],
        );
        pending_wrapper.pending = true;
        assert!(is_internal_span(&pending_wrapper));

        // Same for a pending engine::* builtin call span.
        let mut pending_builtin = make_span(
            "t",
            "s2",
            None,
            "call engine::traces::list",
            "iii",
            1,
            0,
            "unset",
            vec![("function_id", "engine::traces::list")],
        );
        pending_builtin.pending = true;
        assert!(is_internal_span(&pending_builtin));
    }

    // =========================================================================
    // Input struct deserialization tests
    // =========================================================================

    #[test]
    fn test_traces_list_input_defaults() {
        let input: TracesListInput = serde_json::from_str("{}").unwrap();
        assert!(input.trace_id.is_none());
        assert!(input.offset.is_none());
        assert!(input.limit.is_none());
        assert!(input.service_name.is_none());
        assert!(input.name.is_none());
        assert!(input.status.is_none());
        assert!(input.min_duration_ms.is_none());
        assert!(input.max_duration_ms.is_none());
        assert!(input.start_time.is_none());
        assert!(input.end_time.is_none());
        assert!(input.sort_by.is_none());
        assert!(input.sort_order.is_none());
        assert!(input.attributes.is_none());
        assert!(input.include_internal.is_none());
        assert!(input.attribute_projection.is_none());
    }

    #[test]
    fn test_traces_list_input_full() {
        let json = r#"{
            "trace_id": "abc123",
            "offset": 10,
            "limit": 50,
            "service_name": "my-svc",
            "name": "my-span",
            "status": "error",
            "min_duration_ms": 1.5,
            "max_duration_ms": 100.0,
            "start_time": 1000,
            "end_time": 2000,
            "sort_by": "duration",
            "sort_order": "desc",
            "attributes": [["key1", "val1"]],
            "include_internal": true,
            "attribute_projection": ["custom.label"]
        }"#;
        let input: TracesListInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.trace_id.unwrap(), "abc123");
        assert_eq!(input.offset.unwrap(), 10);
        assert_eq!(input.limit.unwrap(), 50);
        assert_eq!(input.service_name.unwrap(), "my-svc");
        assert_eq!(input.name.unwrap(), "my-span");
        assert_eq!(input.status.unwrap(), "error");
        assert!((input.min_duration_ms.unwrap() - 1.5).abs() < f64::EPSILON);
        assert!((input.max_duration_ms.unwrap() - 100.0).abs() < f64::EPSILON);
        assert_eq!(input.start_time.unwrap(), 1000);
        assert_eq!(input.end_time.unwrap(), 2000);
        assert_eq!(input.sort_by.unwrap(), "duration");
        assert_eq!(input.sort_order.unwrap(), "desc");
        assert_eq!(input.attributes.unwrap().len(), 1);
        assert!(input.include_internal.unwrap());
        assert_eq!(
            input.attribute_projection.unwrap(),
            vec!["custom.label".to_string()]
        );
    }

    #[test]
    fn test_metrics_list_input_defaults() {
        let input: MetricsListInput = serde_json::from_str("{}").unwrap();
        assert!(input.start_time.is_none());
        assert!(input.end_time.is_none());
        assert!(input.metric_name.is_none());
        assert!(input.aggregate_interval.is_none());
    }

    #[test]
    fn test_metrics_list_input_full() {
        let json = r#"{
            "start_time": 1000,
            "end_time": 2000,
            "metric_name": "requests.total",
            "aggregate_interval": 60
        }"#;
        let input: MetricsListInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.start_time.unwrap(), 1000);
        assert_eq!(input.end_time.unwrap(), 2000);
        assert_eq!(input.metric_name.unwrap(), "requests.total");
        assert_eq!(input.aggregate_interval.unwrap(), 60);
    }

    #[test]
    fn test_logs_list_input_defaults() {
        let input: LogsListInput = serde_json::from_str("{}").unwrap();
        assert!(input.start_time.is_none());
        assert!(input.end_time.is_none());
        assert!(input.trace_id.is_none());
        assert!(input.span_id.is_none());
        assert!(input.severity_min.is_none());
        assert!(input.severity_text.is_none());
        assert!(input.offset.is_none());
        assert!(input.limit.is_none());
    }

    #[test]
    fn test_logs_list_input_full() {
        let json = r#"{
            "start_time": 1000,
            "end_time": 2000,
            "trace_id": "trace-abc",
            "span_id": "span-123",
            "severity_min": 13,
            "severity_text": "WARN",
            "offset": 5,
            "limit": 25
        }"#;
        let input: LogsListInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.start_time.unwrap(), 1000);
        assert_eq!(input.end_time.unwrap(), 2000);
        assert_eq!(input.trace_id.unwrap(), "trace-abc");
        assert_eq!(input.span_id.unwrap(), "span-123");
        assert_eq!(input.severity_min.unwrap(), 13);
        assert_eq!(input.severity_text.unwrap(), "WARN");
        assert_eq!(input.offset.unwrap(), 5);
        assert_eq!(input.limit.unwrap(), 25);
    }

    #[test]
    fn test_rollups_list_input_defaults() {
        let input: RollupsListInput = serde_json::from_str("{}").unwrap();
        assert!(input.start_time.is_none());
        assert!(input.end_time.is_none());
        assert!(input.level.is_none());
        assert!(input.metric_name.is_none());
    }

    // =========================================================================
    // SpanTreeNode serialization tests
    // =========================================================================

    #[test]
    fn test_span_tree_node_serialization() {
        let span = make_span("t1", "s1", None, "root", "svc", 100, 200, "ok", vec![]);
        let node = SpanTreeNode {
            span,
            children: vec![],
        };

        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["trace_id"], "t1");
        assert_eq!(json["span_id"], "s1");
        assert_eq!(json["name"], "root");
        assert_eq!(json["children"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_span_tree_node_serialization_with_children() {
        let root = make_span("t1", "s1", None, "root", "svc", 100, 500, "ok", vec![]);
        let child = make_span(
            "t1",
            "s2",
            Some("s1"),
            "child",
            "svc",
            150,
            400,
            "ok",
            vec![],
        );

        let node = SpanTreeNode {
            span: root,
            children: vec![SpanTreeNode {
                span: child,
                children: vec![],
            }],
        };

        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["children"].as_array().unwrap().len(), 1);
        assert_eq!(json["children"][0]["name"], "child");
    }

    // =========================================================================
    // Span storage: multiple spans same trace
    // =========================================================================

    #[test]
    fn test_span_storage_multiple_spans_same_trace() {
        let storage = otel::InMemorySpanStorage::new(100);
        let root = make_span("t1", "s1", None, "root", "svc", 100, 500, "ok", vec![]);
        let child1 = make_span(
            "t1",
            "s2",
            Some("s1"),
            "child1",
            "svc",
            150,
            300,
            "ok",
            vec![],
        );
        let child2 = make_span(
            "t1",
            "s3",
            Some("s1"),
            "child2",
            "svc",
            200,
            400,
            "ok",
            vec![],
        );

        storage.add_spans(vec![root, child1, child2]);

        let trace_spans = storage.get_spans_by_trace_id("t1");
        assert_eq!(trace_spans.len(), 3);
    }

    // =========================================================================
    // Span storage: eviction updates secondary index correctly
    // =========================================================================

    // Serial: eviction protects dirty spans whenever the GLOBAL archive is
    // attached (`evict_to_capacity` consults it), so this must not overlap
    // the `#[serial]` archive tests.
    #[test]
    #[serial]
    fn test_span_storage_eviction_index_integrity() {
        let storage = otel::InMemorySpanStorage::new(2);

        // Add two spans from trace t1
        storage.add_spans(vec![make_span(
            "t1",
            "s1",
            None,
            "first",
            "svc",
            100,
            200,
            "ok",
            vec![],
        )]);
        storage.add_spans(vec![make_span(
            "t1",
            "s2",
            Some("s1"),
            "second",
            "svc",
            200,
            300,
            "ok",
            vec![],
        )]);

        // Both should be found by trace_id
        assert_eq!(storage.get_spans_by_trace_id("t1").len(), 2);

        // Adding from a different trace evicts the first span of t1
        storage.add_spans(vec![make_span(
            "t2",
            "s3",
            None,
            "third",
            "svc",
            300,
            400,
            "ok",
            vec![],
        )]);

        // t1 should have only one span left
        let t1_spans = storage.get_spans_by_trace_id("t1");
        assert_eq!(t1_spans.len(), 1);
        assert_eq!(t1_spans[0].span_id, "s2");

        // t2 should have one span
        let t2_spans = storage.get_spans_by_trace_id("t2");
        assert_eq!(t2_spans.len(), 1);
        assert_eq!(t2_spans[0].span_id, "s3");
    }

    // =========================================================================
    // Log storage: filtered with start_time only
    // =========================================================================

    #[test]
    fn test_log_storage_filtered_start_time_only() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.add_logs(vec![
            make_log(None, None, "INFO", 9, "old", "svc", 1_000_000_000),
            make_log(None, None, "INFO", 9, "new", "svc", 3_000_000_000),
        ]);

        // start_time=2000 ms -> only "new" at 3000ms should match
        let (total, logs) =
            storage.get_logs_filtered(None, None, None, None, Some(2000), None, None, None);
        assert_eq!(total, 1);
        assert_eq!(logs[0].body, "new");
    }

    // =========================================================================
    // Log storage: filtered with end_time only
    // =========================================================================

    #[test]
    fn test_log_storage_filtered_end_time_only() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.add_logs(vec![
            make_log(None, None, "INFO", 9, "old", "svc", 1_000_000_000),
            make_log(None, None, "INFO", 9, "new", "svc", 3_000_000_000),
        ]);

        // end_time=2000 ms -> only "old" at 1000ms should match
        let (total, logs) =
            storage.get_logs_filtered(None, None, None, None, None, Some(2000), None, None);
        assert_eq!(total, 1);
        assert_eq!(logs[0].body, "old");
    }

    // =========================================================================
    // Log storage: filtered results sorted newest first
    // =========================================================================

    #[test]
    fn test_log_storage_filtered_sorted_newest_first() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.add_logs(vec![
            make_log(None, None, "INFO", 9, "first", "svc", 1000),
            make_log(None, None, "INFO", 9, "second", "svc", 2000),
            make_log(None, None, "INFO", 9, "third", "svc", 3000),
        ]);

        let (_total, logs) =
            storage.get_logs_filtered(None, None, None, None, None, None, None, None);

        assert_eq!(logs[0].body, "third");
        assert_eq!(logs[1].body, "second");
        assert_eq!(logs[2].body, "first");
    }

    // =========================================================================
    // Log storage: empty filter returns empty for overflow timestamps
    // =========================================================================

    #[test]
    fn test_log_storage_filtered_overflow_start_time() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.store(make_log(None, None, "INFO", 9, "msg", "svc", 1000));

        // u64::MAX cannot be multiplied by 1_000_000 -> overflow -> empty result
        let (total, logs) =
            storage.get_logs_filtered(None, None, None, None, Some(u64::MAX), None, None, None);
        assert_eq!(total, 0);
        assert!(logs.is_empty());
    }

    #[test]
    fn test_log_storage_filtered_overflow_end_time() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.store(make_log(None, None, "INFO", 9, "msg", "svc", 1000));

        let (total, logs) =
            storage.get_logs_filtered(None, None, None, None, None, Some(u64::MAX), None, None);
        assert_eq!(total, 0);
        assert!(logs.is_empty());
    }

    // =========================================================================
    // build_span_tree: orphan spans (parent not in set) become implicit roots
    // =========================================================================

    #[test]
    fn test_build_span_tree_orphan_child_becomes_root() {
        // A span whose parent_span_id is set but absent from the span list is a
        // local trace root (e.g. the server span of a trace that entered iii via
        // an incoming `traceparent`, whose parent lives in the remote caller).
        let orphan = make_span(
            "t1",
            "s2",
            Some("missing-parent"),
            "orphan",
            "svc",
            100,
            200,
            "ok",
            vec![],
        );

        let tree = build_span_tree(vec![orphan]);

        assert_eq!(tree.len(), 1, "orphan with missing parent must be a root");
        assert_eq!(tree[0].span.name, "orphan");
    }

    // =========================================================================
    // LOG_TRIGGER_TYPE constant test
    // =========================================================================

    #[test]
    fn test_log_trigger_type_constant() {
        assert_eq!(LOG_TRIGGER_TYPE, "log");
    }

    // =========================================================================
    // build_span_tree: mixed roots and children from different traces
    // =========================================================================

    #[test]
    fn test_build_span_tree_mixed_traces() {
        let root_t1 = make_span("t1", "s1", None, "root-t1", "svc", 100, 500, "ok", vec![]);
        let child_t1 = make_span(
            "t1",
            "s2",
            Some("s1"),
            "child-t1",
            "svc",
            150,
            400,
            "ok",
            vec![],
        );
        let root_t2 = make_span("t2", "s3", None, "root-t2", "svc", 200, 600, "ok", vec![]);

        let tree = build_span_tree(vec![root_t1, child_t1, root_t2]);

        assert_eq!(tree.len(), 2);

        // Find the tree for t1 root
        let t1_root = tree.iter().find(|n| n.span.trace_id == "t1").unwrap();
        assert_eq!(t1_root.children.len(), 1);
        assert_eq!(t1_root.children[0].span.name, "child-t1");

        // t2 root should have no children
        let t2_root = tree.iter().find(|n| n.span.trace_id == "t2").unwrap();
        assert!(t2_root.children.is_empty());
    }

    // =========================================================================
    // Span storage: add_spans with empty vec
    // =========================================================================

    #[test]
    fn test_span_storage_add_empty() {
        let storage = otel::InMemorySpanStorage::new(100);
        storage.add_spans(vec![]);
        assert!(storage.is_empty());
    }

    // =========================================================================
    // Log storage: add_logs with empty vec
    // =========================================================================

    #[test]
    fn test_log_storage_add_empty() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.add_logs(vec![]);
        assert!(storage.is_empty());
    }

    // =========================================================================
    // Log storage: bulk eviction
    // =========================================================================

    #[test]
    fn test_log_storage_bulk_eviction() {
        let storage = otel::InMemoryLogStorage::new(3);
        let logs: Vec<_> = (0..5)
            .map(|i| {
                make_log(
                    None,
                    None,
                    "INFO",
                    9,
                    &format!("msg-{}", i),
                    "svc",
                    i * 1000,
                )
            })
            .collect();

        storage.add_logs(logs);

        assert_eq!(storage.len(), 3);
        let stored = storage.get_logs();
        assert_eq!(stored[0].body, "msg-2");
        assert_eq!(stored[1].body, "msg-3");
        assert_eq!(stored[2].body, "msg-4");
    }

    // =========================================================================
    // Span storage: performance metrics with identical durations
    // =========================================================================

    #[test]
    fn test_span_storage_performance_metrics_identical_durations() {
        let storage = otel::InMemorySpanStorage::new(100);
        // All spans have exactly 5ms duration
        for i in 0..10 {
            storage.add_spans(vec![make_span(
                &format!("t{}", i),
                &format!("s{}", i),
                None,
                &format!("span{}", i),
                "svc",
                0,
                5_000_000,
                "ok",
                vec![],
            )]);
        }

        let (avg, p50, p95, p99, min, max) = storage.calculate_performance_metrics();

        assert!((avg - 5.0).abs() < 0.001);
        assert!((p50 - 5.0).abs() < 0.001);
        assert!((p95 - 5.0).abs() < 0.001);
        assert!((p99 - 5.0).abs() < 0.001);
        assert!((min - 5.0).abs() < 0.001);
        assert!((max - 5.0).abs() < 0.001);
    }

    // =========================================================================
    // Log storage: filtered with limit=0 returns nothing
    // =========================================================================

    #[test]
    fn test_log_storage_filtered_limit_zero() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.store(make_log(None, None, "INFO", 9, "msg", "svc", 1000));

        let (total, logs) =
            storage.get_logs_filtered(None, None, None, None, None, None, None, Some(0));
        assert_eq!(total, 1); // total is computed before pagination
        assert!(logs.is_empty()); // but limit=0 means nothing returned
    }

    // =========================================================================
    // Log storage: filtered with offset beyond total returns nothing
    // =========================================================================

    #[test]
    fn test_log_storage_filtered_offset_beyond_total() {
        let storage = otel::InMemoryLogStorage::new(100);
        storage.store(make_log(None, None, "INFO", 9, "msg", "svc", 1000));

        let (total, logs) = storage.get_logs_filtered(
            None,
            None,
            None,
            None,
            None,
            None,
            Some(100), // far beyond total
            None,
        );
        assert_eq!(total, 1);
        assert!(logs.is_empty());
    }

    // =========================================================================
    // Log storage: log with attributes
    // =========================================================================

    #[test]
    fn test_log_storage_with_attributes() {
        let storage = otel::InMemoryLogStorage::new(100);
        let mut log = make_log(None, None, "INFO", 9, "msg", "svc", 1000);
        log.attributes.insert(
            "custom_key".to_string(),
            serde_json::Value::String("custom_value".to_string()),
        );
        storage.store(log);

        let logs = storage.get_logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(
            logs[0].attributes.get("custom_key").unwrap(),
            &serde_json::Value::String("custom_value".to_string())
        );
    }

    // =========================================================================
    // build_span_tree: wide tree (many children under one parent)
    // =========================================================================

    #[test]
    fn test_build_span_tree_wide() {
        let mut spans = vec![make_span(
            "t1",
            "s0",
            None,
            "root",
            "svc",
            0,
            1000,
            "ok",
            vec![],
        )];

        for i in 1..=20 {
            spans.push(make_span(
                "t1",
                &format!("s{}", i),
                Some("s0"),
                &format!("child-{}", i),
                "svc",
                i as u64 * 10,
                i as u64 * 10 + 50,
                "ok",
                vec![],
            ));
        }

        let tree = build_span_tree(spans);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 20);
    }

    // =========================================================================
    // build_span_tree: three-level deep nesting with attributes
    // =========================================================================

    #[test]
    fn test_build_span_tree_three_level_nesting() {
        let spans = vec![
            make_span(
                "t1",
                "root",
                None,
                "root-span",
                "svc",
                0,
                1000,
                "ok",
                vec![("level", "0")],
            ),
            make_span(
                "t1",
                "child1",
                Some("root"),
                "child-span",
                "svc",
                100,
                900,
                "ok",
                vec![("level", "1")],
            ),
            make_span(
                "t1",
                "grandchild1",
                Some("child1"),
                "grandchild-span",
                "svc",
                200,
                800,
                "error",
                vec![("level", "2")],
            ),
        ];

        let tree = build_span_tree(spans);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].span.name, "root-span");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].span.name, "child-span");
        assert_eq!(tree[0].children[0].children.len(), 1);
        assert_eq!(tree[0].children[0].children[0].span.name, "grandchild-span");
        assert_eq!(tree[0].children[0].children[0].span.status, "error");
    }

    // =========================================================================
    // build_span_tree: multiple roots from different traces
    // =========================================================================

    #[test]
    fn test_build_span_tree_multiple_traces() {
        let spans = vec![
            make_span("t1", "s1", None, "root-t1", "svc-a", 0, 100, "ok", vec![]),
            make_span(
                "t1",
                "s2",
                Some("s1"),
                "child-t1",
                "svc-a",
                10,
                90,
                "ok",
                vec![],
            ),
            make_span("t2", "s3", None, "root-t2", "svc-b", 0, 200, "ok", vec![]),
        ];

        let tree = build_span_tree(spans);

        // Should have 2 root nodes (one from each trace)
        assert_eq!(tree.len(), 2);

        let root_names: Vec<&str> = tree.iter().map(|n| n.span.name.as_str()).collect();
        assert!(root_names.contains(&"root-t1"));
        assert!(root_names.contains(&"root-t2"));

        // The t1 root should have one child
        let t1_root = tree.iter().find(|n| n.span.name == "root-t1").unwrap();
        assert_eq!(t1_root.children.len(), 1);
    }

    // =========================================================================
    // build_span_tree: preserves status_description and attributes (new variant)
    // =========================================================================

    #[test]
    fn test_build_span_tree_preserves_status_description() {
        let mut span = make_span(
            "t1",
            "s1",
            None,
            "data-span",
            "data-service",
            12345,
            67890,
            "error",
            vec![("key1", "val1"), ("key2", "val2")],
        );
        span.status_description = Some("bad request".to_string());

        let tree = build_span_tree(vec![span]);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].span.service_name, "data-service");
        assert_eq!(tree[0].span.start_time_unix_nano, 12345);
        assert_eq!(tree[0].span.end_time_unix_nano, 67890);
        assert_eq!(tree[0].span.status, "error");
        assert_eq!(
            tree[0].span.status_description,
            Some("bad request".to_string())
        );
        assert_eq!(tree[0].span.attributes.len(), 2);
    }

    // =========================================================================
    // InMemoryLogStorage: get_logs_filtered with combined filters
    // =========================================================================

    #[test]
    fn test_log_storage_filtered_by_severity_and_trace() {
        let storage = otel::InMemoryLogStorage::new(100);

        storage.add_logs(vec![
            make_log(Some("t1"), None, "INFO", 9, "info from t1", "svc-a", 1000),
            make_log(
                Some("t1"),
                None,
                "ERROR",
                17,
                "error from t1",
                "svc-a",
                2000,
            ),
            make_log(Some("t2"), None, "INFO", 9, "info from t2", "svc-b", 3000),
            make_log(
                Some("t2"),
                None,
                "ERROR",
                17,
                "error from t2",
                "svc-b",
                4000,
            ),
        ]);

        // Filter by trace_id and severity_min
        let (total, logs) =
            storage.get_logs_filtered(Some("t1"), None, Some(17), None, None, None, None, None);

        assert_eq!(total, 1);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].body, "error from t1");
    }

    #[test]
    fn test_log_storage_filtered_by_time_range_new() {
        let storage = otel::InMemoryLogStorage::new(100);

        // Note: timestamps in logs are in nanoseconds, but start_time/end_time
        // params to get_logs_filtered are in milliseconds
        storage.add_logs(vec![
            make_log(None, None, "INFO", 9, "early", "svc", 1_000_000_000), // 1000ms
            make_log(None, None, "INFO", 9, "middle", "svc", 5_000_000_000), // 5000ms
            make_log(None, None, "INFO", 9, "late", "svc", 9_000_000_000),  // 9000ms
        ]);

        let (total, logs) = storage.get_logs_filtered(
            None,
            None,
            None,
            None,
            Some(3000), // start_time in ms
            Some(7000), // end_time in ms
            None,
            None,
        );

        assert_eq!(total, 1);
        assert_eq!(logs[0].body, "middle");
    }

    #[test]
    fn test_log_storage_filtered_by_trace_and_span_combined() {
        let storage = otel::InMemoryLogStorage::new(100);

        storage.add_logs(vec![
            make_log(Some("t1"), Some("s1"), "INFO", 9, "log-1", "svc", 1000),
            make_log(Some("t1"), Some("s2"), "INFO", 9, "log-2", "svc", 2000),
            make_log(Some("t2"), Some("s3"), "INFO", 9, "log-3", "svc", 3000),
        ]);

        // Filter by trace_id
        let (total, logs) =
            storage.get_logs_filtered(Some("t1"), None, None, None, None, None, None, None);
        assert_eq!(total, 2);
        assert_eq!(logs.len(), 2);

        // Filter by span_id
        let (total, logs) =
            storage.get_logs_filtered(None, Some("s2"), None, None, None, None, None, None);
        assert_eq!(total, 1);
        assert_eq!(logs[0].body, "log-2");
    }

    #[test]
    fn test_log_storage_filtered_with_limit_new() {
        let storage = otel::InMemoryLogStorage::new(100);

        for i in 0..10u64 {
            storage.add_logs(vec![make_log(
                None,
                None,
                "INFO",
                9,
                &format!("log-{}", i),
                "svc",
                i * 1000,
            )]);
        }

        let (total, logs) = storage.get_logs_filtered(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(3), // limit
        );

        assert_eq!(total, 10); // total reflects all matching, not just returned
        assert_eq!(logs.len(), 3);
    }

    #[test]
    fn test_log_storage_filtered_by_severity_text_exact() {
        let storage = otel::InMemoryLogStorage::new(100);

        storage.add_logs(vec![
            make_log(None, None, "INFO", 9, "info msg", "svc", 1000),
            make_log(None, None, "WARN", 13, "warn msg", "svc", 2000),
            make_log(None, None, "ERROR", 17, "error msg", "svc", 3000),
        ]);

        let (total, logs) =
            storage.get_logs_filtered(None, None, None, Some("error"), None, None, None, None);
        assert_eq!(total, 1);
        assert_eq!(logs[0].severity_text, "ERROR");
    }

    // =========================================================================
    // InMemoryLogStorage: edge cases
    // =========================================================================

    #[test]
    fn test_log_storage_get_logs_by_trace_id_empty() {
        let storage = otel::InMemoryLogStorage::new(100);
        let result = storage.get_logs_by_trace_id("nonexistent");
        assert!(result.is_empty());
    }

    #[test]
    fn test_log_storage_get_logs_by_span_id_empty() {
        let storage = otel::InMemoryLogStorage::new(100);
        let result = storage.get_logs_by_span_id("nonexistent");
        assert!(result.is_empty());
    }

    #[test]
    fn test_log_storage_len_and_is_empty() {
        let storage = otel::InMemoryLogStorage::new(100);
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);

        storage.add_logs(vec![make_log(None, None, "INFO", 9, "test", "svc", 1000)]);
        assert!(!storage.is_empty());
        assert_eq!(storage.len(), 1);

        storage.clear();
        assert!(storage.is_empty());
    }

    // =========================================================================
    // SpanTreeNode serialization (new variant)
    // =========================================================================

    #[test]
    fn test_span_tree_node_json_output() {
        let root = make_span("t1", "s1", None, "root", "svc", 0, 100, "ok", vec![]);
        let child = make_span("t1", "s2", Some("s1"), "child", "svc", 10, 90, "ok", vec![]);

        let tree = build_span_tree(vec![root, child]);

        // Should serialize without error
        let json = serde_json::to_string(&tree).expect("serialize");
        assert!(json.contains("root"));
        assert!(json.contains("child"));
        assert!(json.contains("children"));
    }

    // =========================================================================
    // OtelLogTriggers basic construction
    // =========================================================================

    #[test]
    fn test_otel_log_triggers_new_is_empty() {
        let triggers = OtelLogTriggers::new();
        // Should be empty on construction
        let triggers_read = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(triggers.triggers.read());
        assert!(triggers_read.is_empty());
    }

    #[test]
    fn test_otel_log_triggers_default_is_empty() {
        let triggers = OtelLogTriggers::default();
        let triggers_read = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(triggers.triggers.read());
        assert!(triggers_read.is_empty());
    }

    // NOTE: test_get_resource_attributes_includes_deployment_environment_without_otel_config
    // was removed because get_resource_attributes() only checks DEPLOYMENT_ENVIRONMENT
    // inside the .map() closure that runs when get_otel_config() returns Some.
    // Without otel config the function returns an empty HashMap via unwrap_or_default().

    // =========================================================================
    // InMemoryLogStorage: subscribe and broadcast
    // =========================================================================

    #[tokio::test]
    async fn test_log_storage_subscribe_receives_new_logs() {
        let storage = otel::InMemoryLogStorage::new(100);
        let mut rx = storage.subscribe();

        let log = make_log(None, None, "INFO", 9, "broadcast test", "svc", 1000);
        storage.add_logs(vec![log]);

        // Try to receive the broadcasted log
        let received: Result<Result<otel::StoredLog, tokio::sync::broadcast::error::RecvError>, _> =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
        assert!(received.is_ok());
        let received_log = received.unwrap().unwrap();
        assert_eq!(received_log.body, "broadcast test");
    }

    // =========================================================================
    // InMemorySpanStorage operations via module
    // =========================================================================

    #[test]
    fn test_span_storage_add_and_get_by_trace_id_via_mod() {
        let storage = otel::InMemorySpanStorage::new(100);

        storage.add_spans(vec![
            make_span(
                "trace-a",
                "s1",
                None,
                "span-1",
                "svc",
                100,
                200,
                "ok",
                vec![],
            ),
            make_span(
                "trace-a",
                "s2",
                Some("s1"),
                "span-2",
                "svc",
                150,
                180,
                "ok",
                vec![],
            ),
            make_span(
                "trace-b",
                "s3",
                None,
                "span-3",
                "svc",
                300,
                400,
                "ok",
                vec![],
            ),
        ]);

        let trace_a_spans = storage.get_spans_by_trace_id("trace-a");
        assert_eq!(trace_a_spans.len(), 2);

        let trace_b_spans = storage.get_spans_by_trace_id("trace-b");
        assert_eq!(trace_b_spans.len(), 1);
    }

    // =========================================================================
    // Input struct deserialization edge cases
    // =========================================================================

    #[test]
    fn test_traces_list_input_with_all_fields() {
        let json = serde_json::json!({
            "trace_id": "abc123",
            "service_name": "my-svc",
            "name": "GET /api",
            "min_duration_ms": 100.0,
            "status": "error",
            "limit": 50,
            "offset": 10,
            "start_time": 1704067200000u64,
            "end_time": 1704153600000u64
        });

        let input: TracesListInput = serde_json::from_value(json).expect("deserialize");
        assert_eq!(input.trace_id, Some("abc123".to_string()));
        assert_eq!(input.service_name, Some("my-svc".to_string()));
        assert_eq!(input.name, Some("GET /api".to_string()));
        assert_eq!(input.min_duration_ms, Some(100.0));
        assert_eq!(input.status, Some("error".to_string()));
        assert_eq!(input.limit, Some(50));
        assert_eq!(input.offset, Some(10));
    }

    #[test]
    fn test_metrics_list_input_with_fields() {
        let json = serde_json::json!({
            "metric_name": "cpu.usage",
            "start_time": 1704067200000u64,
            "end_time": 1704153600000u64,
            "aggregate_interval": 60
        });

        let input: MetricsListInput = serde_json::from_value(json).expect("deserialize");
        assert_eq!(input.metric_name, Some("cpu.usage".to_string()));
        assert_eq!(input.aggregate_interval, Some(60));
    }

    #[test]
    fn test_logs_list_input_all_fields() {
        let json = serde_json::json!({
            "trace_id": "t1",
            "span_id": "s1",
            "severity_min": 9,
            "severity_text": "ERROR",
            "limit": 25,
            "offset": 5,
            "start_time": 1704067200000u64,
            "end_time": 1704153600000u64
        });

        let input: LogsListInput = serde_json::from_value(json).expect("deserialize");
        assert_eq!(input.trace_id, Some("t1".to_string()));
        assert_eq!(input.span_id, Some("s1".to_string()));
        assert_eq!(input.severity_min, Some(9));
        assert_eq!(input.severity_text, Some("ERROR".to_string()));
        assert_eq!(input.limit, Some(25));
    }

    #[test]
    fn test_list_inputs_preserve_explicit_zero_values() {
        let traces: TracesListInput = serde_json::from_value(serde_json::json!({
            "offset": 0,
            "limit": 0,
            "include_internal": false
        }))
        .expect("deserialize traces");
        assert_eq!(traces.offset, Some(0));
        assert_eq!(traces.limit, Some(0));
        assert_eq!(traces.include_internal, Some(false));

        let metrics: MetricsListInput = serde_json::from_value(serde_json::json!({
            "start_time": 0,
            "end_time": 0,
            "aggregate_interval": 0
        }))
        .expect("deserialize metrics");
        assert_eq!(metrics.start_time, Some(0));
        assert_eq!(metrics.end_time, Some(0));
        assert_eq!(metrics.aggregate_interval, Some(0));

        let logs: LogsListInput = serde_json::from_value(serde_json::json!({
            "start_time": 0,
            "end_time": 0,
            "offset": 0,
            "limit": 0
        }))
        .expect("deserialize logs");
        assert_eq!(logs.start_time, Some(0));
        assert_eq!(logs.end_time, Some(0));
        assert_eq!(logs.offset, Some(0));
        assert_eq!(logs.limit, Some(0));

        let rollups: RollupsListInput = serde_json::from_value(serde_json::json!({
            "start_time": 0,
            "end_time": 0,
            "level": 0
        }))
        .expect("deserialize rollups");
        assert_eq!(rollups.start_time, Some(0));
        assert_eq!(rollups.end_time, Some(0));
        assert_eq!(rollups.level, Some(0));
    }

    #[test]
    fn test_build_span_tree_matches_parent_by_span_id_regardless_of_trace() {
        // build_span_tree matches children to parents purely by parent_span_id,
        // without checking trace_id. So span-b (from trace-b) with
        // parent_span_id = "span-a" still becomes a child of span-a (from trace-a).
        let tree = build_span_tree(vec![
            make_span(
                "trace-a",
                "span-a",
                None,
                "root-a",
                "svc",
                1,
                2,
                "ok",
                vec![],
            ),
            make_span(
                "trace-b",
                "span-b",
                Some("span-a"),
                "child-b",
                "svc",
                3,
                4,
                "ok",
                vec![],
            ),
        ]);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].span.trace_id, "trace-a");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].span.span_id, "span-b");
    }

    #[test]
    fn test_log_storage_filtered_by_trace_and_severity_text_exact_match() {
        let storage = otel::InMemoryLogStorage::new(8);
        storage.add_logs(vec![
            make_log(
                Some("trace-1"),
                Some("span-1"),
                "INFO",
                9,
                "info",
                "svc",
                1_000_000_000,
            ),
            make_log(
                Some("trace-1"),
                Some("span-2"),
                "ERROR",
                17,
                "error",
                "svc",
                2_000_000_000,
            ),
            make_log(
                Some("trace-2"),
                Some("span-3"),
                "ERROR",
                17,
                "other-trace-error",
                "svc",
                3_000_000_000,
            ),
        ]);

        let (total, logs) = storage.get_logs_filtered(
            Some("trace-1"),
            None,
            None,
            Some("error"),
            None,
            None,
            None,
            None,
        );

        assert_eq!(total, 1);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].body, "error");
    }

    #[test]
    fn test_span_tree_node_serialization_includes_status_description() {
        let mut span = make_span(
            "trace-a",
            "span-a",
            None,
            "root",
            "svc",
            1,
            2,
            "error",
            vec![],
        );
        span.status_description = Some("boom".to_string());

        let json = serde_json::to_value(&SpanTreeNode {
            span,
            children: vec![],
        })
        .expect("serialize");

        assert_eq!(json["status_description"], "boom");
        assert_eq!(json["children"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    #[serial]
    async fn test_otel_module_log_and_baggage_functions() {
        reset_observability_test_state();

        let module = make_test_module(Arc::new(Engine::new()));
        let storage = otel::get_log_storage().expect("log storage should exist");
        storage.clear();

        let input = OtelLogInput {
            trace_id: Some("trace-1".to_string()),
            span_id: Some("span-1".to_string()),
            message: "hello".to_string(),
            data: Some(serde_json::json!({"ok": true})),
            service_name: Some("svc-a".to_string()),
        };

        assert!(matches!(
            module.log_info(input).await,
            FunctionResult::NoResult
        ));
        assert!(matches!(
            module
                .log_warn(OtelLogInput {
                    trace_id: None,
                    span_id: None,
                    message: "warn".to_string(),
                    data: None,
                    service_name: Some("svc-b".to_string()),
                })
                .await,
            FunctionResult::NoResult
        ));
        assert!(matches!(
            module
                .log_error(OtelLogInput {
                    trace_id: None,
                    span_id: None,
                    message: "error".to_string(),
                    data: None,
                    service_name: None,
                })
                .await,
            FunctionResult::NoResult
        ));
        assert!(matches!(
            module
                .log_debug(OtelLogInput {
                    trace_id: None,
                    span_id: None,
                    message: "debug".to_string(),
                    data: None,
                    service_name: None,
                })
                .await,
            FunctionResult::NoResult
        ));
        assert!(matches!(
            module
                .log_trace(OtelLogInput {
                    trace_id: None,
                    span_id: None,
                    message: "trace".to_string(),
                    data: None,
                    service_name: None,
                })
                .await,
            FunctionResult::NoResult
        ));

        let logs = storage.get_logs();
        assert_eq!(logs.len(), 5);
        assert!(logs.iter().any(|log| log.severity_text == "INFO"));
        assert!(logs.iter().any(|log| log.severity_text == "WARN"));
        assert!(logs.iter().any(|log| log.severity_text == "ERROR"));
        assert!(logs.iter().any(|log| log.severity_text == "DEBUG"));
        assert!(logs.iter().any(|log| log.severity_text == "TRACE"));

        let get_result = module
            .baggage_get(BaggageGetInput {
                key: "missing".to_string(),
            })
            .await;
        match get_result {
            FunctionResult::Success(value) => {
                assert!(serde_json::to_value(&value).unwrap()["value"].is_null())
            }
            _ => panic!("expected baggage_get to succeed"),
        }

        let set_result = module
            .baggage_set(BaggageSetInput {
                key: "user.id".to_string(),
                value: "123".to_string(),
            })
            .await;
        match set_result {
            FunctionResult::Success(value) => {
                assert_eq!(serde_json::to_value(&value).unwrap()["success"], true)
            }
            _ => panic!("expected baggage_set to succeed"),
        }

        let get_all_result = module.baggage_get_all(BaggageGetAllInput {}).await;
        match get_all_result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                assert!(value["baggage"].is_object());
            }
            _ => panic!("expected baggage_get_all to succeed"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_trace_summaries_aggregate_child_errors_and_project_only_requested_attributes() {
        reset_observability_test_state();

        let module = make_test_module(Arc::new(Engine::new()));
        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();

        let mut child = make_span(
            "trace-summary",
            "child",
            Some("root"),
            "execute billing::charge",
            "billing-worker",
            1_100_000_000,
            1_900_000_000,
            "error",
            vec![
                ("custom.label", "checkout"),
                ("secret.payload", "must-not-leak"),
                ("iii.tag.outcome", "failed"),
            ],
        );
        child.events.push(otel::StoredSpanEvent {
            name: "large-event".to_string(),
            timestamp_unix_nano: 1_500_000_000,
            attributes: vec![("payload".to_string(), "x".repeat(256 * 1024))],
        });
        span_storage.add_spans(vec![
            make_span(
                "trace-summary",
                "root",
                None,
                "handle checkout",
                "gateway",
                1_000_000_000,
                2_000_000_000,
                "ok",
                vec![
                    ("function_id", "checkout::handle"),
                    ("messaging.destination.name", "payments"),
                ],
            ),
            child,
        ]);

        let result = module
            .list_traces(TracesListInput {
                name: Some("billing::charge".to_string()),
                search_all_spans: Some(true),
                include_internal: Some(true),
                attribute_projection: Some(vec!["custom.label".to_string()]),
                ..Default::default()
            })
            .await;

        let value = match result {
            FunctionResult::Success(value) => serde_json::to_value(value).unwrap(),
            _ => panic!("expected list_traces success"),
        };
        let traces = value["traces"].as_array().expect("traces array");
        assert_eq!(traces.len(), 1);
        let summary = &traces[0];
        assert_eq!(summary["trace_id"], "trace-summary");
        assert_eq!(summary["name"], "handle checkout");
        assert_eq!(summary["status"], "error", "child error must fail trace");
        assert_eq!(summary["span_count"], 2);
        assert_eq!(summary["error_count"], 1);
        assert_eq!(summary["function_id"], "checkout::handle");
        assert_eq!(summary["topic"], "payments");
        assert_eq!(summary["attributes"]["custom.label"], "checkout");
        assert!(summary["attributes"].get("secret.payload").is_none());
        assert_eq!(summary["trace_tags"]["iii.tag.outcome"], "failed");
        assert!(summary.get("events").is_none());
        assert!(summary.get("links").is_none());
        assert!(summary.get("span_id").is_none());
        assert!(
            serde_json::to_vec(summary).unwrap().len() < 2_000,
            "large child event must not reach the summary payload"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_trace_summaries_return_one_row_per_trace_and_paginate_traces() {
        reset_observability_test_state();

        let module = make_test_module(Arc::new(Engine::new()));
        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            make_span("t-1", "r-1", None, "one", "svc", 1, 10, "ok", vec![]),
            make_span(
                "t-1",
                "c-1",
                Some("r-1"),
                "one child",
                "svc",
                2,
                9,
                "ok",
                vec![],
            ),
            // A second dangling root in the same distributed trace must not
            // become a second list row or inflate the trace total.
            make_span(
                "t-1",
                "r-remote",
                Some("remote-parent"),
                "remote branch",
                "svc",
                3,
                8,
                "ok",
                vec![],
            ),
            make_span("t-2", "r-2", None, "two", "svc", 20, 30, "ok", vec![]),
        ]);

        let result = module
            .list_traces(TracesListInput {
                limit: Some(1),
                sort_order: Some("asc".to_string()),
                ..Default::default()
            })
            .await;
        match result {
            FunctionResult::Success(value) => {
                assert_eq!(value.total, 2);
                assert_eq!(value.traces.len(), 1);
                assert_eq!(value.traces[0].trace_id, "t-1");
                assert_eq!(value.traces[0].span_count, 3);
            }
            _ => panic!("expected list_traces success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_filtered_trace_summary_pagination_uses_stable_tiebreaker() {
        reset_observability_test_state();

        let module = make_test_module(Arc::new(Engine::new()));
        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            make_span(
                "t-c",
                "r-c",
                None,
                "shared root",
                "shared-service",
                1_000,
                2_000,
                "ok",
                vec![],
            ),
            make_span(
                "t-a",
                "r-a",
                None,
                "shared root",
                "shared-service",
                1_000,
                2_000,
                "ok",
                vec![],
            ),
            make_span(
                "t-b",
                "r-b",
                None,
                "shared root",
                "shared-service",
                1_000,
                2_000,
                "ok",
                vec![],
            ),
        ]);

        for sort_by in ["start_time", "duration_ms", "service_name", "name"] {
            for (sort_order, expected) in [
                ("asc", vec!["t-a", "t-b", "t-c"]),
                ("desc", vec!["t-c", "t-b", "t-a"]),
            ] {
                let mut actual = Vec::new();
                for offset in 0..3 {
                    let result = module
                        .list_traces(TracesListInput {
                            offset: Some(offset),
                            limit: Some(1),
                            name: Some("shared".to_string()),
                            sort_by: Some(sort_by.to_string()),
                            sort_order: Some(sort_order.to_string()),
                            ..Default::default()
                        })
                        .await;
                    match result {
                        FunctionResult::Success(value) => {
                            assert_eq!(value.total, 3);
                            assert_eq!(value.traces.len(), 1);
                            actual.push(value.traces[0].trace_id.clone());
                        }
                        _ => panic!("expected list_traces success"),
                    }
                }
                assert_eq!(
                    actual, expected,
                    "{sort_by} {sort_order} must keep pages deterministic when values tie"
                );
            }
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_list_traces_treats_dangling_remote_parent_as_root() {
        reset_observability_test_state();

        let module = make_test_module(Arc::new(Engine::new()));
        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();

        // A trace that entered iii from an external caller: the server span's
        // parent is the remote caller's span id, which is never stored here.
        // Its child (parent present in the store) is NOT a root.
        span_storage.add_spans(vec![
            make_span(
                "t-remote",
                "s-http",
                Some("remoteparent0001"),
                "POST /x",
                "iii-engine",
                1_000_000_000,
                1_100_000_000,
                "OK",
                vec![],
            ),
            make_span(
                "t-remote",
                "s-child",
                Some("s-http"),
                "execute fn",
                "worker",
                1_010_000_000,
                1_090_000_000,
                "OK",
                vec![],
            ),
        ]);

        let input = TracesListInput {
            trace_id: None,
            trace_ids: None,
            offset: Some(0),
            limit: Some(10),
            service_name: None,
            name: None,
            status: None,
            min_duration_ms: None,
            max_duration_ms: None,
            start_time: None,
            end_time: None,
            sort_by: None,
            sort_order: None,
            attributes: None,
            exclude_attributes: None,
            include_internal: Some(false),
            search_all_spans: None,
            attribute_projection: None,
        };

        let spans = match module.list_trace_spans(input).await {
            FunctionResult::Success(v) => serde_json::to_value(&v).unwrap()["spans"]
                .as_array()
                .expect("spans array")
                .clone(),
            _ => panic!("expected list_traces success"),
        };

        // Only the dangling-parent server span surfaces as a root.
        assert_eq!(spans.len(), 1, "dangling-parent span must be a root");
        assert_eq!(spans[0]["name"].as_str().unwrap(), "POST /x");
    }

    #[tokio::test]
    #[serial]
    async fn test_list_traces_sort_by_duration_ms_and_service_name() {
        reset_observability_test_state();

        let module = make_test_module(Arc::new(Engine::new()));
        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();

        // Three root traces whose start-time order differs from both their
        // duration order and their service-name order, so a silent fallback to
        // the default start_time sort would be observable in the assertions.
        span_storage.add_spans(vec![
            // start 5s, duration 300ms, service "alpha"
            make_span(
                "t-a",
                "r-a",
                None,
                "root-a",
                "alpha",
                5_000_000_000,
                5_300_000_000,
                "OK",
                vec![],
            ),
            // start 1s, duration 900ms, service "charlie"
            make_span(
                "t-b",
                "r-b",
                None,
                "root-b",
                "charlie",
                1_000_000_000,
                1_900_000_000,
                "OK",
                vec![],
            ),
            // start 3s, duration 100ms, service "bravo"
            make_span(
                "t-c",
                "r-c",
                None,
                "root-c",
                "bravo",
                3_000_000_000,
                3_100_000_000,
                "OK",
                vec![],
            ),
        ]);

        let base_input = || TracesListInput {
            trace_id: None,
            trace_ids: None,
            offset: Some(0),
            limit: Some(10),
            service_name: None,
            name: None,
            status: None,
            min_duration_ms: None,
            max_duration_ms: None,
            start_time: None,
            end_time: None,
            sort_by: None,
            sort_order: None,
            attributes: None,
            exclude_attributes: None,
            include_internal: Some(false),
            search_all_spans: None,
            attribute_projection: None,
        };

        let order = |result: FunctionResult<TracesSpansResult, ErrorBody>| -> Vec<String> {
            match result {
                FunctionResult::Success(value) => serde_json::to_value(&value).unwrap()["spans"]
                    .as_array()
                    .expect("spans array")
                    .iter()
                    .map(|s| s["trace_id"].as_str().expect("trace_id").to_string())
                    .collect(),
                _ => panic!("expected list_traces success"),
            }
        };

        // duration_ms desc: 900ms (t-b), 300ms (t-a), 100ms (t-c)
        let desc = order(
            module
                .list_trace_spans(TracesListInput {
                    sort_by: Some("duration_ms".to_string()),
                    sort_order: Some("desc".to_string()),
                    ..base_input()
                })
                .await,
        );
        assert_eq!(
            desc,
            vec!["t-b", "t-a", "t-c"],
            "duration_ms desc must order by descending duration"
        );

        // duration_ms asc: 100ms (t-c), 300ms (t-a), 900ms (t-b)
        let asc = order(
            module
                .list_trace_spans(TracesListInput {
                    sort_by: Some("duration_ms".to_string()),
                    sort_order: Some("asc".to_string()),
                    ..base_input()
                })
                .await,
        );
        assert_eq!(
            asc,
            vec!["t-c", "t-a", "t-b"],
            "duration_ms asc must order by ascending duration"
        );

        // service_name asc: alpha (t-a), bravo (t-c), charlie (t-b)
        let by_service = order(
            module
                .list_trace_spans(TracesListInput {
                    sort_by: Some("service_name".to_string()),
                    sort_order: Some("asc".to_string()),
                    ..base_input()
                })
                .await,
        );
        assert_eq!(
            by_service,
            vec!["t-a", "t-c", "t-b"],
            "service_name asc must order alphabetically by service"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_otel_module_traces_logs_metrics_health_and_alert_views() {
        reset_observability_test_state();

        let engine = Arc::new(Engine::new());
        let module = make_test_module(engine.clone());

        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            make_span(
                "trace-visible",
                "root-visible",
                None,
                "visible-root",
                "svc",
                1_000_000_000,
                2_000_000_000,
                "OK",
                vec![("http.method", "GET")],
            ),
            make_span(
                "trace-visible",
                "child-visible",
                Some("root-visible"),
                "visible-child",
                "svc",
                1_100_000_000,
                1_500_000_000,
                "OK",
                vec![],
            ),
            make_span(
                "trace-internal",
                "root-internal",
                None,
                "internal-root",
                "svc",
                1_000_000_000,
                1_100_000_000,
                "OK",
                vec![("iii.function.kind", "internal")],
            ),
        ]);

        let traces_result = module
            .list_trace_spans(TracesListInput {
                trace_id: None,
                trace_ids: None,
                offset: Some(0),
                limit: Some(10),
                service_name: Some("svc".to_string()),
                name: Some("visible".to_string()),
                status: Some("ok".to_string()),
                min_duration_ms: Some(100.0),
                max_duration_ms: Some(1500.0),
                start_time: Some(900),
                end_time: Some(2500),
                sort_by: Some("name".to_string()),
                sort_order: Some("asc".to_string()),
                attributes: Some(vec![vec!["http.method".to_string(), "GET".to_string()]]),
                exclude_attributes: None,
                include_internal: Some(false),
                search_all_spans: None,
                attribute_projection: None,
            })
            .await;

        match traces_result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let spans = value["spans"].as_array().expect("spans array");
                assert_eq!(spans.len(), 1);
                assert_eq!(spans[0]["trace_id"], "trace-visible");
            }
            _ => panic!("expected list_traces success"),
        }

        let tree_result = module
            .get_trace_tree(TracesTreeInput {
                trace_id: "trace-visible".to_string(),
            })
            .await;
        match tree_result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let roots = value["roots"].as_array().expect("roots array");
                assert_eq!(roots.len(), 1);
                assert_eq!(roots[0]["children"].as_array().unwrap().len(), 1);
            }
            _ => panic!("expected get_trace_tree success"),
        }

        let metric_storage = metrics::get_metric_storage().expect("metric storage should exist");
        metric_storage.clear();
        metric_storage.add_metrics(vec![make_number_metric("test.metric", 12.0, 2_000_000_000)]);

        let metrics_overflow = module
            .list_metrics(MetricsListInput {
                start_time: Some(u64::MAX),
                end_time: Some(1),
                metric_name: None,
                aggregate_interval: None,
            })
            .await;
        match metrics_overflow {
            // Arithmetic overflow on the ms->ns conversion is a real input
            // error, surfaced as a Failure (not a Success with an error field).
            FunctionResult::Failure(err) => {
                assert_eq!(err.code, "time_value_overflow");
                assert_eq!(err.message, "start_time value too large");
            }
            _ => panic!("expected list_metrics overflow failure"),
        }

        let metrics_ok = module
            .list_metrics(MetricsListInput {
                start_time: Some(1000),
                end_time: Some(3000),
                metric_name: Some("test.metric".to_string()),
                aggregate_interval: Some(1),
            })
            .await;
        match metrics_ok {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                assert!(value["sdk_metrics"].is_array());
                assert!(value.get("query").is_some());
            }
            _ => panic!("expected list_metrics success"),
        }

        let log_storage = otel::get_log_storage().expect("log storage should exist");
        log_storage.clear();
        log_storage.store(make_log(
            Some("trace-visible"),
            Some("span-visible"),
            "ERROR",
            17,
            "boom",
            "svc",
            2_000_000_000,
        ));

        let logs_result = module
            .list_logs(LogsListInput {
                start_time: Some(1500),
                end_time: Some(2500),
                trace_id: Some("trace-visible".to_string()),
                span_id: None,
                severity_min: Some(9),
                severity_text: Some("error".to_string()),
                offset: Some(0),
                limit: Some(10),
            })
            .await;
        match logs_result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                assert_eq!(value["total"], 1);
                assert_eq!(value["logs"].as_array().unwrap().len(), 1);
            }
            _ => panic!("expected list_logs success"),
        }

        let clear_logs_result = module.clear_logs(LogsClearInput {}).await;
        assert!(matches!(clear_logs_result, FunctionResult::Success(_)));
        assert_eq!(log_storage.len(), 0);

        let rollups_result = module
            .list_rollups(RollupsListInput {
                start_time: Some(1000),
                end_time: Some(3000),
                metric_name: Some("test.metric".to_string()),
                level: Some(0),
            })
            .await;
        match rollups_result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                assert!(value["rollups"].is_array());
            }
            _ => panic!("expected list_rollups success"),
        }

        let health_result = module.health_check(HealthCheckInput {}).await;
        match health_result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                assert_eq!(value["status"], "healthy");
                assert!(value["components"].is_object());
            }
            _ => panic!("expected health_check success"),
        }

        let alerts_result = module.list_alerts(AlertsListInput {}).await;
        match alerts_result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                assert!(value["alerts"].is_array());
            }
            _ => panic!("expected list_alerts success"),
        }

        let evaluate_result = module.evaluate_alerts(AlertsEvaluateInput {}).await;
        match evaluate_result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                assert!(value.get("evaluated").is_some());
            }
            _ => panic!("expected evaluate_alerts success"),
        }

        assert!(matches!(
            module.clear_traces(TracesClearInput {}).await,
            FunctionResult::Success(_)
        ));
        assert_eq!(span_storage.len(), 0);
    }

    #[tokio::test]
    #[serial]
    async fn test_traces_list_attribute_filter_with_search_all_spans_matches_child() {
        reset_observability_test_state();

        let engine = Arc::new(Engine::new());
        let module = make_test_module(engine.clone());

        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            make_span(
                "trace-with-msg",
                "root",
                None,
                "handle_invocation harness::status",
                "svc",
                1_000_000_000,
                2_000_000_000,
                "OK",
                vec![],
            ),
            make_span(
                "trace-with-msg",
                "child",
                Some("root"),
                "harness.status",
                "svc",
                1_100_000_000,
                1_500_000_000,
                "OK",
                vec![("iii.message.id", "M-target")],
            ),
            make_span(
                "trace-other",
                "root-other",
                None,
                "handle_invocation other",
                "svc",
                1_000_000_000,
                1_100_000_000,
                "OK",
                vec![("iii.message.id", "M-other")],
            ),
        ]);

        let result = module
            .list_trace_spans(TracesListInput {
                trace_id: None,
                trace_ids: None,
                offset: Some(0),
                limit: Some(10),
                service_name: None,
                name: None,
                status: None,
                min_duration_ms: None,
                max_duration_ms: None,
                start_time: None,
                end_time: None,
                sort_by: None,
                sort_order: None,
                attributes: Some(vec![vec![
                    "iii.message.id".to_string(),
                    "M-target".to_string(),
                ]]),
                exclude_attributes: None,
                include_internal: Some(true),
                search_all_spans: Some(true),
                attribute_projection: None,
            })
            .await;

        match result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let spans = value["spans"].as_array().expect("spans array");
                assert_eq!(
                    spans.len(),
                    2,
                    "expected root + child of trace-with-msg, got {spans:?}"
                );
                let span_ids: std::collections::HashSet<String> = spans
                    .iter()
                    .map(|s| s["span_id"].as_str().unwrap().to_string())
                    .collect();
                assert!(span_ids.contains("root"));
                assert!(span_ids.contains("child"));
                for span in spans {
                    assert_eq!(span["trace_id"], "trace-with-msg");
                }
            }
            _ => panic!("expected list_traces success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_traces_list_returns_children_only_when_search_all_spans() {
        reset_observability_test_state();

        let engine = Arc::new(Engine::new());
        let module = make_test_module(engine.clone());

        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            make_span(
                "tid-1",
                "root-1",
                None,
                "handle_invocation harness::status",
                "iii",
                1_000_000_000,
                2_000_000_000,
                "OK",
                vec![],
            ),
            make_span(
                "tid-1",
                "child-1",
                Some("root-1"),
                "call harness::status",
                "iii-rust-sdk",
                1_100_000_000,
                1_500_000_000,
                "OK",
                vec![],
            ),
        ]);

        let result_root_only = module
            .list_trace_spans(TracesListInput {
                trace_id: None,
                trace_ids: None,
                offset: Some(0),
                limit: Some(10),
                service_name: None,
                name: None,
                status: None,
                min_duration_ms: None,
                max_duration_ms: None,
                start_time: None,
                end_time: None,
                sort_by: None,
                sort_order: None,
                attributes: None,
                exclude_attributes: None,
                include_internal: Some(true),
                search_all_spans: Some(false),
                attribute_projection: None,
            })
            .await;
        match result_root_only {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let spans = value["spans"].as_array().expect("spans array");
                assert_eq!(spans.len(), 1, "default mode = root only");
                assert_eq!(spans[0]["span_id"], "root-1");
            }
            _ => panic!("expected success"),
        }

        let result_all = module
            .list_trace_spans(TracesListInput {
                trace_id: None,
                trace_ids: None,
                offset: Some(0),
                limit: Some(10),
                service_name: None,
                name: None,
                status: None,
                min_duration_ms: None,
                max_duration_ms: None,
                start_time: None,
                end_time: None,
                sort_by: None,
                sort_order: None,
                attributes: None,
                exclude_attributes: None,
                include_internal: Some(true),
                search_all_spans: Some(true),
                attribute_projection: None,
            })
            .await;
        match result_all {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let spans = value["spans"].as_array().expect("spans array");
                assert_eq!(spans.len(), 2, "widened mode = root + children");
                let names: std::collections::HashSet<String> = spans
                    .iter()
                    .map(|s| s["name"].as_str().unwrap().to_string())
                    .collect();
                assert!(names.contains("handle_invocation harness::status"));
                assert!(names.contains("call harness::status"));
            }
            _ => panic!("expected success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_traces_list_attribute_filter_without_search_all_spans_stays_root_only() {
        reset_observability_test_state();

        let engine = Arc::new(Engine::new());
        let module = make_test_module(engine.clone());

        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            make_span(
                "trace-with-msg",
                "root",
                None,
                "root-name",
                "svc",
                1_000_000_000,
                2_000_000_000,
                "OK",
                vec![],
            ),
            make_span(
                "trace-with-msg",
                "child",
                Some("root"),
                "child-name",
                "svc",
                1_100_000_000,
                1_500_000_000,
                "OK",
                vec![("iii.message.id", "M-target")],
            ),
        ]);

        let result = module
            .list_trace_spans(TracesListInput {
                trace_id: None,
                trace_ids: None,
                offset: Some(0),
                limit: Some(10),
                service_name: None,
                name: None,
                status: None,
                min_duration_ms: None,
                max_duration_ms: None,
                start_time: None,
                end_time: None,
                sort_by: None,
                sort_order: None,
                attributes: Some(vec![vec![
                    "iii.message.id".to_string(),
                    "M-target".to_string(),
                ]]),
                exclude_attributes: None,
                include_internal: Some(true),
                search_all_spans: Some(false),
                attribute_projection: None,
            })
            .await;

        match result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let spans = value["spans"].as_array().expect("spans array");
                assert_eq!(
                    spans.len(),
                    0,
                    "child-only attribute MUST not match under search_all_spans=false"
                );
            }
            _ => panic!("expected list_traces success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_traces_group_by_attribute_returns_correct_aggregates() {
        reset_observability_test_state();

        let engine = Arc::new(Engine::new());
        let module = make_test_module(engine.clone());

        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            make_span(
                "trace-A",
                "A1",
                None,
                "root-A",
                "svc",
                1_000_000_000,
                2_000_000_000,
                "OK",
                vec![("iii.message.id", "M-1")],
            ),
            make_span(
                "trace-A",
                "A2",
                Some("A1"),
                "child-A",
                "svc",
                1_100_000_000,
                1_500_000_000,
                "OK",
                vec![("iii.message.id", "M-1")],
            ),
            make_span(
                "trace-B",
                "B1",
                None,
                "root-B",
                "svc",
                3_000_000_000,
                4_000_000_000,
                "OK",
                vec![("iii.message.id", "M-2")],
            ),
            make_span(
                "trace-B",
                "B2",
                Some("B1"),
                "child-B-1",
                "svc",
                3_100_000_000,
                3_500_000_000,
                "Error",
                vec![("iii.message.id", "M-2")],
            ),
            make_span(
                "trace-B",
                "B3",
                Some("B1"),
                "child-B-2",
                "svc",
                3_200_000_000,
                3_800_000_000,
                "OK",
                vec![("iii.message.id", "M-2")],
            ),
            // No iii.message.id — must be skipped by group_by.
            make_span(
                "trace-C",
                "C1",
                None,
                "root-C",
                "svc",
                5_000_000_000,
                5_500_000_000,
                "OK",
                vec![("other.attr", "value")],
            ),
        ]);

        let result = module
            .group_traces_by(TracesGroupByInput {
                attribute: "iii.message.id".to_string(),
                since_ms: None,
                limit: Some(100),
                include_internal: Some(true),
                label_attribute: None,
            })
            .await;

        match result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let groups = value["groups"].as_array().expect("groups array");
                assert_eq!(
                    groups.len(),
                    2,
                    "expected 2 groups (M-1, M-2); trace-C has no iii.message.id"
                );
                // Sorted by first_seen_ms DESC.
                assert_eq!(groups[0]["value"], "M-2");
                assert_eq!(groups[0]["span_count"], 3);
                assert_eq!(groups[0]["error_count"], 1);
                let m2_trace_ids = groups[0]["trace_ids"].as_array().unwrap();
                assert_eq!(m2_trace_ids.len(), 1);
                assert_eq!(m2_trace_ids[0], "trace-B");

                assert_eq!(groups[1]["value"], "M-1");
                assert_eq!(groups[1]["span_count"], 2);
                assert_eq!(groups[1]["error_count"], 0);
                let m1_trace_ids = groups[1]["trace_ids"].as_array().unwrap();
                assert_eq!(m1_trace_ids.len(), 1);
                assert_eq!(m1_trace_ids[0], "trace-A");
            }
            _ => panic!("expected group_traces_by success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_traces_group_by_since_ms_filters_old_spans() {
        reset_observability_test_state();

        let engine = Arc::new(Engine::new());
        let module = make_test_module(engine.clone());

        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            make_span(
                "trace-old",
                "old",
                None,
                "root-old",
                "svc",
                500_000_000,
                1_000_000_000,
                "OK",
                vec![("iii.message.id", "M-old")],
            ),
            make_span(
                "trace-new",
                "new",
                None,
                "root-new",
                "svc",
                4_000_000_000,
                5_000_000_000,
                "OK",
                vec![("iii.message.id", "M-new")],
            ),
        ]);

        let result = module
            .group_traces_by(TracesGroupByInput {
                attribute: "iii.message.id".to_string(),
                since_ms: Some(2000),
                limit: Some(100),
                include_internal: Some(true),
                label_attribute: None,
            })
            .await;

        match result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let groups = value["groups"].as_array().expect("groups array");
                assert_eq!(groups.len(), 1, "only M-new should survive since_ms filter");
                assert_eq!(groups[0]["value"], "M-new");
            }
            _ => panic!("expected group_traces_by success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_traces_group_by_limit_truncates_to_n_groups() {
        reset_observability_test_state();

        let engine = Arc::new(Engine::new());
        let module = make_test_module(engine.clone());

        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();

        let spans: Vec<_> = (0..5)
            .map(|i| {
                let start_ns = 1_000_000_000_u64 + (i as u64) * 1_000_000_000;
                make_span(
                    &format!("trace-{i}"),
                    &format!("span-{i}"),
                    None,
                    &format!("root-{i}"),
                    "svc",
                    start_ns,
                    start_ns + 500_000_000,
                    "OK",
                    vec![("iii.message.id", &format!("M-{i}"))],
                )
            })
            .collect();
        span_storage.add_spans(spans);

        let result = module
            .group_traces_by(TracesGroupByInput {
                attribute: "iii.message.id".to_string(),
                since_ms: None,
                limit: Some(2),
                include_internal: Some(true),
                label_attribute: None,
            })
            .await;

        match result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let groups = value["groups"].as_array().expect("groups array");
                assert_eq!(groups.len(), 2, "limit=2 must truncate the 5 groups to 2");
                // Sorted by first_seen_ms DESC.
                assert_eq!(groups[0]["value"], "M-4");
                assert_eq!(groups[1]["value"], "M-3");
            }
            _ => panic!("expected group_traces_by success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_traces_group_by_excludes_internal_spans_by_default() {
        reset_observability_test_state();

        let engine = Arc::new(Engine::new());
        let module = make_test_module(engine.clone());

        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            make_span(
                "trace-internal-1",
                "i1",
                None,
                "engine work",
                "svc",
                1_000_000_000,
                2_000_000_000,
                "OK",
                vec![
                    ("iii.message.id", "M-internal"),
                    ("iii.function.kind", "internal"),
                ],
            ),
            make_span(
                "trace-internal-2",
                "i2",
                None,
                "engine work",
                "svc",
                1_000_000_000,
                2_000_000_000,
                "OK",
                vec![
                    ("iii.message.id", "M-internal2"),
                    ("function_id", "engine::traces::list"),
                ],
            ),
            make_span(
                "trace-user",
                "u1",
                None,
                "user work",
                "svc",
                1_000_000_000,
                2_000_000_000,
                "OK",
                vec![("iii.message.id", "M-user")],
            ),
        ]);

        let result = module
            .group_traces_by(TracesGroupByInput {
                attribute: "iii.message.id".to_string(),
                since_ms: None,
                limit: Some(100),
                include_internal: None,
                label_attribute: None,
            })
            .await;

        match result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let groups = value["groups"].as_array().expect("groups array");
                assert_eq!(
                    groups.len(),
                    1,
                    "internal spans must be excluded by default"
                );
                assert_eq!(groups[0]["value"], "M-user");
            }
            _ => panic!("expected group_traces_by success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_traces_group_by_label_attribute_newest_wins() {
        reset_observability_test_state();

        let engine = Arc::new(Engine::new());
        let module = make_test_module(engine.clone());

        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            // Session S-1 renamed between turns: the newest span's name wins.
            make_span(
                "trace-A",
                "A1",
                None,
                "turn-1",
                "harness",
                1_000_000_000,
                2_000_000_000,
                "OK",
                vec![("iii.session.id", "S-1"), ("iii.session.name", "untitled")],
            ),
            make_span(
                "trace-B",
                "B1",
                None,
                "turn-2",
                "harness",
                3_000_000_000,
                4_000_000_000,
                "OK",
                vec![
                    ("iii.session.id", "S-1"),
                    ("iii.session.name", "refactor auth"),
                ],
            ),
            // Session S-2 has no name attribute anywhere: label stays null.
            make_span(
                "trace-C",
                "C1",
                None,
                "turn-3",
                "harness",
                5_000_000_000,
                6_000_000_000,
                "OK",
                vec![("iii.session.id", "S-2")],
            ),
        ]);

        let result = module
            .group_traces_by(TracesGroupByInput {
                attribute: "iii.session.id".to_string(),
                since_ms: None,
                limit: Some(100),
                include_internal: Some(true),
                label_attribute: Some("iii.session.name".to_string()),
            })
            .await;

        match result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let groups = value["groups"].as_array().expect("groups array");
                assert_eq!(groups.len(), 2);
                // Sorted by first_seen_ms DESC: S-2 first, then S-1.
                assert_eq!(groups[0]["value"], "S-2");
                assert!(groups[0]["label"].is_null());
                assert_eq!(groups[1]["value"], "S-1");
                assert_eq!(groups[1]["label"], "refactor auth");
            }
            _ => panic!("expected group_traces_by success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_list_traces_exclude_attributes_hides_matching_roots() {
        reset_observability_test_state();

        let module = make_test_module(Arc::new(Engine::new()));
        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            make_span(
                "t-noisy",
                "n1",
                None,
                "call ui::subscribe",
                "engine",
                1_000_000_000,
                1_100_000_000,
                "OK",
                vec![("function_id", "ui::subscribe")],
            ),
            make_span(
                "t-kept",
                "k1",
                None,
                "call harness::turn",
                "engine",
                2_000_000_000,
                2_100_000_000,
                "OK",
                vec![("function_id", "harness::turn")],
            ),
            // The excluded function appearing INSIDE a trace must not hide it
            // (root-match only).
            make_span(
                "t-kept-2",
                "k2",
                None,
                "call other::fn",
                "engine",
                3_000_000_000,
                3_200_000_000,
                "OK",
                vec![("function_id", "other::fn")],
            ),
            make_span(
                "t-kept-2",
                "k2-child",
                Some("k2"),
                "call ui::subscribe",
                "engine",
                3_050_000_000,
                3_100_000_000,
                "OK",
                vec![("function_id", "ui::subscribe")],
            ),
        ]);

        let result = module
            .list_trace_spans(TracesListInput {
                trace_id: None,
                trace_ids: None,
                offset: None,
                limit: None,
                service_name: None,
                name: None,
                status: None,
                min_duration_ms: None,
                max_duration_ms: None,
                start_time: None,
                end_time: None,
                sort_by: None,
                sort_order: None,
                attributes: None,
                exclude_attributes: Some(vec![vec![
                    "function_id".to_string(),
                    "ui::subscribe".to_string(),
                ]]),
                include_internal: Some(true),
                search_all_spans: None,
                attribute_projection: None,
            })
            .await;

        match result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let spans = value["spans"].as_array().expect("spans array");
                let trace_ids: Vec<&str> = spans
                    .iter()
                    .map(|s| s["trace_id"].as_str().unwrap())
                    .collect();
                assert_eq!(
                    trace_ids,
                    vec!["t-kept", "t-kept-2"],
                    "root matching the excluded pair is hidden; a child match is not"
                );
            }
            _ => panic!("expected list_traces success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_list_traces_trace_ids_filters_to_requested_set() {
        reset_observability_test_state();

        let module = make_test_module(Arc::new(Engine::new()));
        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            make_span(
                "t-1",
                "s1",
                None,
                "op-1",
                "svc",
                1_000_000_000,
                1_100_000_000,
                "OK",
                vec![],
            ),
            make_span(
                "t-2",
                "s2",
                None,
                "op-2",
                "svc",
                2_000_000_000,
                2_100_000_000,
                "OK",
                vec![],
            ),
            make_span(
                "t-3",
                "s3",
                None,
                "op-3",
                "svc",
                3_000_000_000,
                3_100_000_000,
                "OK",
                vec![],
            ),
        ]);

        let result = module
            .list_trace_spans(TracesListInput {
                trace_id: None,
                trace_ids: Some(vec!["t-1".to_string(), "t-3".to_string()]),
                offset: None,
                limit: None,
                service_name: None,
                name: None,
                status: None,
                min_duration_ms: None,
                max_duration_ms: None,
                start_time: None,
                end_time: None,
                sort_by: None,
                sort_order: None,
                attributes: None,
                exclude_attributes: None,
                include_internal: Some(true),
                search_all_spans: None,
                attribute_projection: None,
            })
            .await;

        match result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let spans = value["spans"].as_array().expect("spans array");
                let trace_ids: Vec<&str> = spans
                    .iter()
                    .map(|s| s["trace_id"].as_str().unwrap())
                    .collect();
                assert_eq!(trace_ids, vec!["t-1", "t-3"]);
            }
            _ => panic!("expected list_traces success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_list_traces_pages_unfiltered_all_spans_before_archive_decode() {
        reset_observability_test_state();

        let module = make_test_module(Arc::new(Engine::new()));
        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        span_storage.add_spans(vec![
            make_span("t-1", "s-1", None, "one", "svc", 1_000, 1_100, "OK", vec![]),
            make_span("t-2", "s-2", None, "two", "svc", 2_000, 2_100, "OK", vec![]),
            make_span(
                "t-3",
                "s-3",
                None,
                "three",
                "svc",
                3_000,
                3_100,
                "OK",
                vec![],
            ),
        ]);

        let result = module
            .list_trace_spans(TracesListInput {
                offset: Some(1),
                limit: Some(1),
                sort_by: Some("start_time".to_string()),
                sort_order: Some("desc".to_string()),
                include_internal: Some(true),
                search_all_spans: Some(true),
                attribute_projection: None,
                ..Default::default()
            })
            .await;

        match result {
            FunctionResult::Success(value) => {
                assert_eq!(value.total, 3);
                assert_eq!(value.spans.len(), 1);
                assert_eq!(value.spans[0]["trace_id"], "t-2");
            }
            _ => panic!("expected list_traces success"),
        }
    }

    /// Detaches the test archive even on panic so `#[serial]` neighbors never
    /// inherit a stale trace store.
    struct ArchiveTestGuard;

    impl Drop for ArchiveTestGuard {
        fn drop(&mut self) {
            otel::configure_trace_storage(None);
        }
    }

    fn attach_test_archive(directory: &std::path::Path) -> ArchiveTestGuard {
        otel::configure_trace_storage(Some(config::TraceStorageConfig {
            directory: directory.to_string_lossy().into_owned(),
            ..config::TraceStorageConfig::default()
        }));
        let storage = otel::get_span_storage().expect("span storage should exist");
        otel::attach_trace_disk_storage(&storage);
        ArchiveTestGuard
    }

    /// A degraded durable tier must surface on the spans COMPONENT, not just
    /// the overall status — consumers reading `components.spans.status`
    /// alone must never see "healthy" while the archive is failing.
    #[tokio::test]
    #[serial]
    async fn test_health_check_spans_component_reflects_degraded_archive() {
        reset_observability_test_state();
        let directory = tempfile::tempdir().expect("temp trace directory");
        let _guard = attach_test_archive(directory.path());
        let module = make_test_module(Arc::new(Engine::new()));

        let healthy = match module.health_check(HealthCheckInput {}).await {
            FunctionResult::Success(value) => value,
            _ => panic!("expected health_check success"),
        };
        assert_eq!(healthy.status, "healthy");
        assert_eq!(healthy.components.spans["status"], "healthy");

        otel::get_trace_disk_storage()
            .expect("archive should be configured")
            .mark_degraded("test-induced failure");
        let degraded = match module.health_check(HealthCheckInput {}).await {
            FunctionResult::Success(value) => value,
            _ => panic!("expected health_check success"),
        };
        assert_eq!(degraded.status, "degraded");
        assert_eq!(degraded.components.spans["status"], "degraded");
        assert_eq!(
            degraded.components.spans["details"]["archive"]["last_error"],
            "test-induced failure"
        );
    }

    fn flush_test_archive() {
        otel::get_trace_disk_storage()
            .expect("archive should be configured")
            .flush()
            .expect("flush trace archive");
    }

    #[tokio::test]
    #[serial]
    async fn test_list_traces_default_pages_archived_roots_with_hot_overlay() {
        reset_observability_test_state();
        let directory = tempfile::tempdir().expect("temp trace directory");
        let _guard = attach_test_archive(directory.path());

        let module = make_test_module(Arc::new(Engine::new()));
        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();

        // Archived history: an external root with a child, an internal root,
        // and a root the hot cache will later shadow with a newer version.
        span_storage.add_spans(vec![
            make_span(
                "t-1",
                "root-1",
                None,
                "one",
                "svc",
                1_000,
                1_100,
                "OK",
                vec![],
            ),
            make_span(
                "t-1",
                "child-1",
                Some("root-1"),
                "one-child",
                "svc",
                1_050,
                1_090,
                "OK",
                vec![],
            ),
            make_span(
                "t-int",
                "root-int",
                None,
                "internal",
                "svc",
                1_500,
                1_600,
                "OK",
                vec![("function_id", "engine::traces::list")],
            ),
            make_span(
                "t-2",
                "root-2",
                None,
                "old",
                "svc",
                2_000,
                2_100,
                "OK",
                vec![],
            ),
        ]);
        flush_test_archive();
        // Evict everything from the hot cache; the archive keeps the rows.
        span_storage.clear();

        // Hot-only state: a shadowing final for root-2, a fresh root, and a
        // child whose parent exists only in the archive (must not list as a
        // root).
        span_storage.add_spans(vec![
            make_span(
                "t-2",
                "root-2",
                None,
                "new",
                "svc",
                2_000,
                2_200,
                "OK",
                vec![],
            ),
            make_span(
                "t-3",
                "root-3",
                None,
                "three",
                "svc",
                3_000,
                3_100,
                "OK",
                vec![],
            ),
            make_span(
                "t-1",
                "late-child",
                Some("root-1"),
                "late",
                "svc",
                3_500,
                3_600,
                "OK",
                vec![],
            ),
        ]);

        let result = module
            .list_trace_spans(TracesListInput {
                limit: Some(10),
                ..Default::default()
            })
            .await;
        match result {
            FunctionResult::Success(value) => {
                assert_eq!(value.total, 3, "archived + hot roots, internal excluded");
                let rows: Vec<(&str, &str)> = value
                    .spans
                    .iter()
                    .map(|span| {
                        (
                            span["span_id"].as_str().unwrap(),
                            span["name"].as_str().unwrap(),
                        )
                    })
                    .collect();
                assert_eq!(
                    rows,
                    vec![("root-1", "one"), ("root-2", "new"), ("root-3", "three")],
                    "hot version wins for root-2; archived child and late-child excluded"
                );
            }
            _ => panic!("expected list_traces success"),
        }

        // include_internal widens the same page to the archived internal root.
        let result = module
            .list_trace_spans(TracesListInput {
                limit: Some(10),
                include_internal: Some(true),
                ..Default::default()
            })
            .await;
        match result {
            FunctionResult::Success(value) => {
                assert_eq!(value.total, 4);
                assert!(value.spans.iter().any(|span| span["span_id"] == "root-int"));
            }
            _ => panic!("expected list_traces success"),
        }

        // Descending offset/limit slices the merged order.
        let result = module
            .list_trace_spans(TracesListInput {
                offset: Some(1),
                limit: Some(1),
                sort_order: Some("desc".to_string()),
                ..Default::default()
            })
            .await;
        match result {
            FunctionResult::Success(value) => {
                assert_eq!(value.total, 3);
                assert_eq!(value.spans.len(), 1);
                assert_eq!(value.spans[0]["span_id"], "root-2");
                assert_eq!(value.spans[0]["name"], "new");
            }
            _ => panic!("expected list_traces success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_list_traces_root_page_widens_past_demoted_archived_children() {
        reset_observability_test_state();
        let directory = tempfile::tempdir().expect("temp trace directory");
        let _guard = attach_test_archive(directory.path());

        let module = make_test_module(Arc::new(Engine::new()));
        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();

        // Archive: five children of a parent that only exists as a hot pending
        // span (SQL sees them as dangling roots), plus one genuinely older
        // root beyond the initial `offset + limit + hot_count` window.
        let mut archived = vec![make_span(
            "t-old",
            "root-old",
            None,
            "old-root",
            "svc",
            1_000,
            1_100,
            "OK",
            vec![],
        )];
        for index in 0..5 {
            archived.push(make_span(
                "t-p",
                &format!("child-{index}"),
                Some("pending-parent"),
                "child",
                "svc",
                2_000 + index,
                2_100 + index,
                "OK",
                vec![],
            ));
        }
        span_storage.add_spans(archived);
        flush_test_archive();
        span_storage.clear();

        // Hot: only the pending parent (start before every archived child so
        // the descending page must reach past the demoted prefix).
        let mut pending = make_span(
            "t-p",
            "pending-parent",
            None,
            "parent",
            "svc",
            500,
            0,
            "OK",
            vec![],
        );
        pending.pending = true;
        span_storage.add_pending_span(pending);

        // Descending, limit 1: the window prefix is entirely demoted children;
        // the widening loop must keep doubling until root-old surfaces.
        let result = module
            .list_trace_spans(TracesListInput {
                limit: Some(1),
                sort_order: Some("desc".to_string()),
                ..Default::default()
            })
            .await;
        match result {
            FunctionResult::Success(value) => {
                assert_eq!(value.spans.len(), 1);
                assert_eq!(
                    value.spans[0]["span_id"], "root-old",
                    "the widening loop must surface roots past the demoted prefix"
                );
                assert_eq!(value.total, 2, "root-old plus the hot pending parent");
            }
            _ => panic!("expected list_traces success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_list_traces_merges_trace_tags_from_child_spans() {
        reset_observability_test_state();

        let module = make_test_module(Arc::new(Engine::new()));
        let span_storage = otel::get_span_storage().expect("span storage should exist");
        span_storage.clear();
        // Engine-side root has no baggage-derived attributes (it starts before
        // the worker handler stamps baggage); children carry them.
        span_storage.add_spans(vec![
            make_span(
                "t-turn",
                "root",
                None,
                "call harness::turn",
                "engine",
                1_000_000_000,
                2_000_000_000,
                "OK",
                vec![("function_id", "harness::turn")],
            ),
            make_span(
                "t-turn",
                "child-1",
                Some("root"),
                "call session::messages",
                "session-manager",
                1_100_000_000,
                1_200_000_000,
                "OK",
                vec![
                    ("iii.session.id", "S-1"),
                    ("iii.session.name", "untitled"),
                    ("iii.tag.message", "fix the login bug"),
                ],
            ),
            // Newer child carries the renamed session: it wins the merge.
            make_span(
                "t-turn",
                "child-2",
                Some("root"),
                "call router::chat",
                "llm-router",
                1_300_000_000,
                1_900_000_000,
                "OK",
                vec![
                    ("iii.session.id", "S-1"),
                    ("iii.session.name", "refactor auth"),
                    ("iii.message.id", "turn-9"),
                ],
            ),
        ]);

        let result = module
            .list_trace_spans(TracesListInput {
                trace_id: None,
                trace_ids: None,
                offset: None,
                limit: None,
                service_name: None,
                name: None,
                status: None,
                min_duration_ms: None,
                max_duration_ms: None,
                start_time: None,
                end_time: None,
                sort_by: None,
                sort_order: None,
                attributes: None,
                exclude_attributes: None,
                include_internal: Some(true),
                search_all_spans: None,
                attribute_projection: None,
            })
            .await;

        match result {
            FunctionResult::Success(value) => {
                let value = serde_json::to_value(&value).unwrap();
                let spans = value["spans"].as_array().expect("spans array");
                assert_eq!(spans.len(), 1, "one root row expected");
                let tags = &spans[0]["trace_tags"];
                assert_eq!(tags["iii.tag.message"], "fix the login bug");
                assert_eq!(tags["iii.session.id"], "S-1");
                assert_eq!(
                    tags["iii.session.name"], "refactor auth",
                    "newest span's value wins on key conflict"
                );
                assert_eq!(tags["iii.message.id"], "turn-9");
                // Non-tag attributes must not leak into trace_tags.
                assert!(tags.get("function_id").is_none());
            }
            _ => panic!("expected list_traces success"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_otel_module_initialize_start_background_tasks_and_destroy() {
        reset_observability_test_state();

        let engine = Arc::new(Engine::new());
        let module = make_test_module(engine.clone());

        module.initialize().await.expect("initialize");
        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key(&crate::trigger::type_key(
                    crate::protocol::DEFAULT_NAMESPACE,
                    LOG_TRIGGER_TYPE
                ))
        );

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        module
            .start_background_tasks(shutdown_rx, shutdown_tx.clone())
            .await
            .expect("start_background_tasks");
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let _ = shutdown_tx.send(true);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        module.destroy().await.expect("destroy");
    }

    #[tokio::test]
    #[serial]
    async fn test_initialize_returns_ok_when_disabled() {
        reset_observability_test_state();
        let config = config::ObservabilityWorkerConfig {
            enabled: Some(false),
            ..config::ObservabilityWorkerConfig::default()
        };
        let _config_guard = OtelConfigTestGuard::install(config.clone());
        let engine = Arc::new(Engine::new());
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        let worker = ObservabilityWorker {
            _config: config,
            triggers: Arc::new(OtelLogTriggers::new()),
            trace_triggers: Arc::new(OtelTraceTriggers::new()),
            engine: engine.clone(),
            shutdown_tx: Arc::new(shutdown_tx),
            worker_shutdown_rx: Arc::new(std::sync::Mutex::new(None)),
            logs_retention_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_exporter_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_trigger_stop: Arc::new(std::sync::Mutex::new(None)),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
        };

        let result = worker.initialize().await;
        assert!(result.is_ok());

        // Verify the trigger types were NOT registered (early return skipped it)
        assert!(
            !engine
                .trigger_registry
                .trigger_types
                .contains_key(&crate::trigger::type_key(
                    crate::protocol::DEFAULT_NAMESPACE,
                    LOG_TRIGGER_TYPE
                ))
        );
        assert!(
            !engine
                .trigger_registry
                .trigger_types
                .contains_key(&crate::trigger::type_key(
                    crate::protocol::DEFAULT_NAMESPACE,
                    TRACE_TRIGGER_TYPE
                ))
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_initialize_defaults_to_enabled_when_none() {
        reset_observability_test_state();
        let config = config::ObservabilityWorkerConfig {
            enabled: None,
            ..config::ObservabilityWorkerConfig::default()
        };
        let _config_guard = OtelConfigTestGuard::install(config.clone());
        let engine = Arc::new(Engine::new());
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        let worker = ObservabilityWorker {
            _config: config,
            triggers: Arc::new(OtelLogTriggers::new()),
            trace_triggers: Arc::new(OtelTraceTriggers::new()),
            engine: engine.clone(),
            shutdown_tx: Arc::new(shutdown_tx),
            worker_shutdown_rx: Arc::new(std::sync::Mutex::new(None)),
            logs_retention_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_exporter_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_trigger_stop: Arc::new(std::sync::Mutex::new(None)),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
        };

        let result = worker.initialize().await;
        assert!(result.is_ok());

        // Verify the trigger types WERE registered (enabled: None defaults to true)
        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key(&crate::trigger::type_key(
                    crate::protocol::DEFAULT_NAMESPACE,
                    LOG_TRIGGER_TYPE
                ))
        );
        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key(&crate::trigger::type_key(
                    crate::protocol::DEFAULT_NAMESPACE,
                    TRACE_TRIGGER_TYPE
                ))
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_start_background_tasks_returns_ok_when_disabled() {
        reset_observability_test_state();
        let config = config::ObservabilityWorkerConfig {
            enabled: Some(false),
            ..config::ObservabilityWorkerConfig::default()
        };
        let _config_guard = OtelConfigTestGuard::install(config.clone());
        let engine = Arc::new(Engine::new());
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let worker = ObservabilityWorker {
            _config: config,
            triggers: Arc::new(OtelLogTriggers::new()),
            trace_triggers: Arc::new(OtelTraceTriggers::new()),
            engine: engine.clone(),
            shutdown_tx: Arc::new(shutdown_tx.clone()),
            worker_shutdown_rx: Arc::new(std::sync::Mutex::new(None)),
            logs_retention_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_exporter_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_trigger_stop: Arc::new(std::sync::Mutex::new(None)),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
        };

        let result = worker
            .start_background_tasks(shutdown_rx, shutdown_tx)
            .await;
        assert!(result.is_ok());
    }
}
