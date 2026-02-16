// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod logs_layer;
pub mod metrics;
pub mod otel;
mod sampler;

mod config;

use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    sync::Arc,
    time::SystemTime,
};

use async_trait::async_trait;
use colored::Colorize;
use function_macros::{function, service};
use futures::Future;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock as TokioRwLock;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::module::Module,
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};

#[derive(Serialize, Deserialize, Default)]
pub struct TracesListInput {
    /// Filter by specific trace ID
    trace_id: Option<String>,
    /// Pagination offset (default: 0)
    offset: Option<usize>,
    /// Pagination limit (default: 100)
    limit: Option<usize>,
    /// Filter by service name (case-insensitive substring match)
    service_name: Option<String>,
    /// Filter by span name (case-insensitive substring match)
    name: Option<String>,
    /// Filter by status (case-insensitive substring match)
    status: Option<String>,
    /// Minimum span duration in milliseconds (sub-ms precision)
    min_duration_ms: Option<f64>,
    /// Maximum span duration in milliseconds (sub-ms precision)
    max_duration_ms: Option<f64>,
    /// Start time in unix timestamp milliseconds (include spans overlapping after this)
    start_time: Option<u64>,
    /// End time in unix timestamp milliseconds (include spans overlapping before this)
    end_time: Option<u64>,
    /// Sort field: "duration" | "start_time" | "name" (default: "start_time")
    sort_by: Option<String>,
    /// Sort order: "asc" | "desc" (default: "asc")
    sort_order: Option<String>,
    /// Filter by span attributes (array of [key, value] pairs, AND logic, exact match)
    attributes: Option<Vec<Vec<String>>>,
    /// Include internal engine traces (engine.* functions). Defaults to false.
    #[serde(default)]
    include_internal: Option<bool>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct TracesClearInput {}

#[derive(Serialize, Deserialize)]
pub struct TracesTreeInput {
    /// Trace ID to build the tree for
    trace_id: String,
}

#[derive(Serialize)]
pub struct SpanTreeNode {
    #[serde(flatten)]
    pub span: otel::StoredSpan,
    pub children: Vec<SpanTreeNode>,
}

#[derive(Serialize, Deserialize, Default)]
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

#[derive(Serialize, Deserialize, Default)]
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

#[derive(Serialize, Deserialize, Default)]
pub struct LogsClearInput {}

#[derive(Serialize, Deserialize, Default)]
pub struct HealthCheckInput {}

#[derive(Serialize, Deserialize, Default)]
pub struct AlertsListInput {}

#[derive(Serialize, Deserialize, Default)]
pub struct AlertsEvaluateInput {}

#[derive(Serialize, Deserialize, Default)]
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

/// Input for OTEL log functions (log.info, log.warn, log.error)
#[derive(Serialize, Deserialize)]
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
#[derive(Serialize, Deserialize, Default)]
pub struct BaggageGetInput {
    /// The baggage key to retrieve
    pub key: String,
}

/// Input for baggage.set function
#[derive(Serialize, Deserialize, Default)]
pub struct BaggageSetInput {
    /// The baggage key to set
    pub key: String,
    /// The baggage value to set
    pub value: String,
}

/// Input for baggage.getAll function (empty)
#[derive(Serialize, Deserialize, Default)]
pub struct BaggageGetAllInput {}

/// OpenTelemetry configuration module.
/// This module provides OTEL-native logging, traces, metrics, and logs access.
/// It sets the global OTEL configuration from YAML before logging is initialized.
#[derive(Clone)]
pub struct OtelModule {
    _config: config::OtelModuleConfig,
    triggers: Arc<OtelLogTriggers>,
    engine: Arc<Engine>,
    /// Shutdown signal sender for background tasks
    shutdown_tx: Arc<tokio::sync::watch::Sender<bool>>,
}

fn build_span_tree(spans: Vec<otel::StoredSpan>) -> Vec<SpanTreeNode> {
    let mut children_map: HashMap<String, Vec<otel::StoredSpan>> = HashMap::new();
    let mut roots: Vec<otel::StoredSpan> = Vec::new();

    for span in spans {
        match &span.parent_span_id {
            Some(parent_id) => {
                children_map
                    .entry(parent_id.clone())
                    .or_default()
                    .push(span);
            }
            None => roots.push(span),
        }
    }

    fn build_node(
        span: otel::StoredSpan,
        children_map: &mut HashMap<String, Vec<otel::StoredSpan>>,
    ) -> SpanTreeNode {
        let children = children_map
            .remove(&span.span_id)
            .unwrap_or_default()
            .into_iter()
            .map(|child| build_node(child, children_map))
            .collect();

        SpanTreeNode { span, children }
    }

    roots
        .into_iter()
        .map(|root| build_node(root, &mut children_map))
        .collect()
}

#[service(name = "otel")]
impl OtelModule {
    // =========================================================================
    // OTEL-native Log Functions (recommended over legacy logger.*)
    // =========================================================================

    #[function(id = "engine::log::info", description = "Log an info message using OTEL")]
    pub async fn log_info(&self, input: OtelLogInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.store_and_emit_log(&input, "INFO", 9).await;
        FunctionResult::NoResult
    }

    #[function(
        id = "engine::log::warn",
        description = "Log a warning message using OTEL"
    )]
    pub async fn log_warn(&self, input: OtelLogInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.store_and_emit_log(&input, "WARN", 13).await;
        FunctionResult::NoResult
    }

    #[function(
        id = "engine::log::error",
        description = "Log an error message using OTEL"
    )]
    pub async fn log_error(&self, input: OtelLogInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.store_and_emit_log(&input, "ERROR", 17).await;
        FunctionResult::NoResult
    }

    #[function(
        id = "engine::log::debug",
        description = "Log a debug message using OTEL"
    )]
    pub async fn log_debug(&self, input: OtelLogInput) -> FunctionResult<Option<Value>, ErrorBody> {
        self.store_and_emit_log(&input, "DEBUG", 5).await;
        FunctionResult::NoResult
    }

    #[function(
        id = "engine::log::trace",
        description = "Log a trace-level message using OTEL"
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
        description = "Get a baggage item value from the current context"
    )]
    pub async fn baggage_get(
        &self,
        input: BaggageGetInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        use opentelemetry::baggage::BaggageExt;

        let cx = opentelemetry::Context::current();
        let baggage = cx.baggage();
        let value = baggage.get(&input.key).map(|v| v.to_string());
        FunctionResult::Success(Some(serde_json::json!({ "value": value })))
    }

    #[function(
        id = "engine::baggage::set",
        description = "Set a baggage item value (returns new context, does not modify global)"
    )]
    pub async fn baggage_set(
        &self,
        input: BaggageSetInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        use opentelemetry::KeyValue;
        use opentelemetry::baggage::BaggageExt;

        // Note: Baggage in OpenTelemetry is immutable - we create a new context
        // but since this is a function call, we can't actually propagate the new context
        // back to the caller. This function is mainly useful for verification/debugging.
        // Real baggage propagation should be done at the SDK/invocation level.
        let cx = opentelemetry::Context::current();
        let _new_cx = cx.with_baggage([KeyValue::new(input.key.clone(), input.value.clone())]);

        FunctionResult::Success(Some(serde_json::json!({
            "success": true,
            "note": "Baggage set in new context. For propagation, use SDK-level baggage headers."
        })))
    }

    #[function(
        id = "engine::baggage::get_all",
        description = "Get all baggage items from the current context"
    )]
    pub async fn baggage_get_all(
        &self,
        _input: BaggageGetAllInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        use opentelemetry::baggage::BaggageExt;

        let cx = opentelemetry::Context::current();
        let baggage = cx.baggage();
        let items: std::collections::HashMap<String, String> = baggage
            .iter()
            .map(|(k, (v, _))| (k.to_string(), v.to_string()))
            .collect();
        FunctionResult::Success(Some(serde_json::json!({ "baggage": items })))
    }

    /// Store a log in OTEL format and emit tracing event
    async fn store_and_emit_log(
        &self,
        input: &OtelLogInput,
        severity_text: &str,
        severity_number: i32,
    ) {
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

        // Initialize storage if not already done
        if otel::get_log_storage().is_none() {
            otel::init_log_storage(None);
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
                        Call otel::init_log_storage() or ensure OtelModule is initialized."
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

            if trigger_level == "all" || trigger_level == log_level {
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
                let function_id = trigger.function_id.clone();

                tokio::spawn(async move {
                    let _ = engine.call(&function_id, log_data).await;
                });
            }
        }
    }

    // =========================================================================
    // Traces Functions
    // =========================================================================

    #[function(
        id = "engine::traces::list",
        description = "List stored traces (only available when exporter is 'memory' or 'both')"
    )]
    pub async fn list_traces(
        &self,
        input: TracesListInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match otel::get_span_storage() {
            Some(storage) => {
                let all_spans = match input.trace_id {
                    Some(trace_id) => storage.get_spans_by_trace_id(&trace_id),
                    None => storage.get_spans(),
                };


                let include_internal = input.include_internal.unwrap_or(false);

                let mut filtered: Vec<_> = all_spans
                    .into_iter()
                    .filter(|s| s.parent_span_id.is_none())
                    .filter(|s| {
                        // Exclude internal engine traces unless explicitly requested
                        if !include_internal {
                            let is_internal = s.attributes.iter().any(|(k, v)| {
                                (k == "iii.function.kind" && v == "internal")
                                    || (k == "function_id" && v.starts_with("engine."))
                            });
                            if is_internal {
                                return false;
                            }
                        }
                        true
                    })
                    .filter(|s| {
                        if let Some(ref sn) = input.service_name
                            && !s.service_name.to_lowercase().contains(&sn.to_lowercase())
                        {
                            return false;
                        }
                        if let Some(ref n) = input.name
                            && !s.name.to_lowercase().contains(&n.to_lowercase())
                        {
                            return false;
                        }
                        if let Some(ref st) = input.status
                            && !s.status.to_lowercase().contains(&st.to_lowercase())
                        {
                            return false;
                        }
                        let duration_ns = s.end_time_unix_nano.saturating_sub(s.start_time_unix_nano);
                        let duration_ms: f64 = duration_ns as f64 / 1_000_000.0;
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
                            if s.end_time_unix_nano < start_ns {
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
                            for pair in attrs {
                                if pair.len() == 2 {
                                    let key = &pair[0];
                                    let value = &pair[1];
                                    if !s.attributes.iter().any(|(k, v)| k == key && v == value) {
                                        return false;
                                    }
                                }
                            }
                        }
                        true
                    })
                    .collect();

                let sort_order_asc = input
                    .sort_order
                    .as_deref()
                    .map(|o| o.eq_ignore_ascii_case("asc"))
                    .unwrap_or(true);

                filtered.sort_by(|a, b| {
                    let cmp = match input.sort_by.as_deref().unwrap_or("start_time") {
                        "duration" => {
                            let da = a.end_time_unix_nano.saturating_sub(a.start_time_unix_nano)
                                as f64;
                            let db = b.end_time_unix_nano.saturating_sub(b.start_time_unix_nano)
                                as f64;
                            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                        }
                        "name" => a.name.cmp(&b.name),
                        _ => a.start_time_unix_nano.cmp(&b.start_time_unix_nano),
                    };
                    if sort_order_asc {
                        cmp
                    } else {
                        cmp.reverse()
                    }
                });

                let total = filtered.len();
                let offset = input.offset.unwrap_or(0);
                let limit = input.limit.unwrap_or(100);

                let spans: Vec<_> = filtered.into_iter().skip(offset).take(limit).collect();

                let response = serde_json::json!({
                    "spans": spans,
                    "total": total,
                    "offset": offset,
                    "limit": limit,
                });
                FunctionResult::Success(Some(response))
            }
            None => {
                FunctionResult::Failure(ErrorBody {
                    code: "memory_exporter_not_enabled".to_string(),
                    message: "In-memory span storage is not available. Set exporter: memory or both in config.".to_string(),
                })
            }
        }
    }

    #[function(
        id = "engine::traces::tree",
        description = "Get trace tree with nested children (only available when exporter is 'memory' or 'both')"
    )]
    pub async fn get_trace_tree(
        &self,
        input: TracesTreeInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match otel::get_span_storage() {
            Some(storage) => {
                let all_spans = storage.get_spans_by_trace_id(&input.trace_id);

                if all_spans.is_empty() {
                    return FunctionResult::Success(Some(serde_json::json!({
                        "roots": [],
                    })));
                }

                let roots = build_span_tree(all_spans);

                let response = serde_json::json!({
                    "roots": roots,
                });
                FunctionResult::Success(Some(response))
            }
            None => {
                FunctionResult::Failure(ErrorBody {
                    code: "memory_exporter_not_enabled".to_string(),
                    message: "In-memory span storage is not available. Set exporter: memory or both in config.".to_string(),
                })
            }
        }
    }

    #[function(
        id = "engine::traces::clear",
        description = "Clear all stored traces (only available when exporter is 'memory' or 'both')"
    )]
    pub async fn clear_traces(
        &self,
        _input: TracesClearInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match otel::get_span_storage() {
            Some(storage) => {
                storage.clear();
                FunctionResult::Success(Some(serde_json::json!({ "success": true })))
            }
            None => {
                FunctionResult::Failure(ErrorBody {
                    code: "memory_exporter_not_enabled".to_string(),
                    message: "In-memory span storage is not available. Set exporter: memory or both in config.".to_string(),
                })
            }
        }
    }

    // =========================================================================
    // Metrics Functions
    // =========================================================================

    #[function(
        id = "engine::metrics::list",
        description = "List current metrics values"
    )]
    pub async fn list_metrics(
        &self,
        input: MetricsListInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
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
                        return FunctionResult::Success(Some(serde_json::json!({
                            "error": "start_time value too large",
                            "sdk_metrics": [],
                            "aggregated_metrics": [],
                        })));
                    }
                };
                let end_ns = match end.checked_mul(1_000_000) {
                    Some(ns) => ns,
                    None => {
                        tracing::warn!("end_time overflow when converting to nanoseconds");
                        return FunctionResult::Success(Some(serde_json::json!({
                            "error": "end_time value too large",
                            "sdk_metrics": [],
                            "aggregated_metrics": [],
                        })));
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
                            return FunctionResult::Success(Some(serde_json::json!({
                                "error": "start_time value too large",
                                "sdk_metrics": [],
                                "aggregated_metrics": [],
                            })));
                        }
                    };
                    let end_ns = match end.checked_mul(1_000_000) {
                        Some(ns) => ns,
                        None => {
                            tracing::warn!("end_time overflow in aggregated metrics");
                            return FunctionResult::Success(Some(serde_json::json!({
                                "error": "end_time value too large",
                                "sdk_metrics": [],
                                "aggregated_metrics": [],
                            })));
                        }
                    };
                    let interval_ns = match interval_secs.checked_mul(1_000_000_000) {
                        Some(ns) => ns,
                        None => {
                            tracing::warn!(
                                "aggregate_interval overflow when converting to nanoseconds"
                            );
                            return FunctionResult::Success(Some(serde_json::json!({
                                "error": "aggregate_interval value too large",
                                "sdk_metrics": [],
                                "aggregated_metrics": [],
                            })));
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

        let mut response = serde_json::json!({
            "engine_metrics": {
                "invocations": {
                    "total": invocations_total,
                    "success": invocations_success,
                    "error": invocations_error,
                    "deferred": invocations_deferred,
                    "by_function": accumulator.get_by_function(),
                },
                "workers": {
                    "spawns": workers_spawns,
                    "deaths": workers_deaths,
                    "active": workers_spawns.saturating_sub(workers_deaths),
                },
                "performance": {
                    "avg_duration_ms": avg_duration_ms,
                    "p50_duration_ms": p50_duration_ms,
                    "p95_duration_ms": p95_duration_ms,
                    "p99_duration_ms": p99_duration_ms,
                    "min_duration_ms": min_duration_ms,
                    "max_duration_ms": max_duration_ms,
                }
            },
            "sdk_metrics": sdk_metrics,
            "timestamp": chrono::Utc::now().timestamp_millis(),
        });

        // Add aggregated metrics if available
        if !aggregated_metrics.is_empty() {
            response["aggregated_metrics"] = serde_json::json!(aggregated_metrics);
        }

        // Add query parameters to response for reference
        if input.start_time.is_some()
            || input.end_time.is_some()
            || input.aggregate_interval.is_some()
        {
            response["query"] = serde_json::json!({
                "start_time": input.start_time,
                "end_time": input.end_time,
                "aggregate_interval": input.aggregate_interval,
                "metric_name": input.metric_name,
            });
        }

        FunctionResult::Success(Some(response))
    }

    // =========================================================================
    // Logs Functions
    // =========================================================================

    #[function(id = "engine::logs::list", description = "List stored OTEL logs")]
    pub async fn list_logs(
        &self,
        input: LogsListInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
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
                let response = serde_json::json!({
                    "logs": logs,
                    "total": total,
                    "query": {
                        "trace_id": input.trace_id,
                        "span_id": input.span_id,
                        "severity_min": input.severity_min,
                        "severity_text": input.severity_text,
                        "start_time": input.start_time,
                        "end_time": input.end_time,
                        "offset": input.offset,
                        "limit": input.limit,
                    },
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                });
                FunctionResult::Success(Some(response))
            }
            None => {
                // Initialize storage if not already done and return empty result
                otel::init_log_storage(None);
                let response = serde_json::json!({
                    "logs": [],
                    "total": 0,
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                });
                FunctionResult::Success(Some(response))
            }
        }
    }

    #[function(id = "engine::logs::clear", description = "Clear all stored OTEL logs")]
    pub async fn clear_logs(
        &self,
        _input: LogsClearInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match otel::get_log_storage() {
            Some(storage) => {
                storage.clear();
                FunctionResult::Success(Some(serde_json::json!({ "success": true })))
            }
            None => FunctionResult::Success(Some(
                serde_json::json!({ "success": true, "message": "No log storage initialized" }),
            )),
        }
    }

    // =========================================================================
    // Sampling Diagnostic Functions
    // =========================================================================

    #[function(
        id = "engine::sampling::rules",
        description = "Get active sampling rules configuration"
    )]
    pub async fn get_sampling_rules(
        &self,
        _input: LogsClearInput, // Reusing empty input type
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let config = otel::get_otel_config();

        let (default_ratio, rules, parent_based, logs_sampling_ratio) = match config {
            Some(cfg) => {
                let default_ratio = cfg
                    .sampling
                    .as_ref()
                    .and_then(|s| s.default)
                    .or(cfg.sampling_ratio)
                    .unwrap_or(1.0);

                let rules: Vec<Value> = cfg
                    .sampling
                    .as_ref()
                    .map(|s| {
                        s.rules
                            .iter()
                            .map(|r| {
                                serde_json::json!({
                                    "operation": r.operation,
                                    "service": r.service,
                                    "rate": r.rate,
                                })
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

        let response = serde_json::json!({
            "traces": {
                "default_ratio": default_ratio,
                "rules": rules,
                "parent_based": parent_based,
            },
            "logs": {
                "sampling_ratio": logs_sampling_ratio,
            },
            "timestamp": chrono::Utc::now().timestamp_millis(),
        });

        FunctionResult::Success(Some(response))
    }

    // =========================================================================
    // Health Check Functions
    // =========================================================================

    #[function(id = "engine::health::check", description = "Check system health status")]
    pub async fn health_check(
        &self,
        _input: HealthCheckInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        // Check OTEL configuration
        let otel_component = if let Some(config) = otel::get_otel_config() {
            let enabled = config.enabled.unwrap_or(false);
            if enabled {
                serde_json::json!({
                    "status": "healthy",
                    "details": {
                        "enabled": true,
                        "service_name": config.service_name,
                        "exporter": format!("{:?}", config.exporter),
                    }
                })
            } else {
                serde_json::json!({
                    "status": "disabled",
                    "details": null
                })
            }
        } else {
            serde_json::json!({
                "status": "disabled",
                "details": null
            })
        };

        // Check metrics storage
        let metrics_component = if let Some(storage) = metrics::get_metric_storage() {
            serde_json::json!({
                "status": "healthy",
                "details": {
                    "stored_metrics": storage.len(),
                }
            })
        } else {
            serde_json::json!({
                "status": "disabled",
                "details": null
            })
        };

        // Check logs storage
        let logs_component = if let Some(storage) = otel::get_log_storage() {
            serde_json::json!({
                "status": "healthy",
                "details": {
                    "stored_logs": storage.len(),
                }
            })
        } else {
            serde_json::json!({
                "status": "disabled",
                "details": null
            })
        };

        // Check span storage
        let spans_component = if let Some(storage) = otel::get_span_storage() {
            serde_json::json!({
                "status": "healthy",
                "details": {
                    "stored_spans": storage.len(),
                }
            })
        } else {
            serde_json::json!({
                "status": "disabled",
                "details": null
            })
        };

        let response = serde_json::json!({
            "status": "healthy",
            "components": {
                "otel": otel_component,
                "metrics": metrics_component,
                "logs": logs_component,
                "spans": spans_component,
            },
            "timestamp": chrono::Utc::now().timestamp_millis(),
            "version": env!("CARGO_PKG_VERSION"),
        });

        FunctionResult::Success(Some(response))
    }

    // =========================================================================
    // Alerts Functions
    // =========================================================================

    #[function(id = "engine::alerts::list", description = "List current alert states")]
    pub async fn list_alerts(
        &self,
        _input: AlertsListInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        if let Some(manager) = metrics::get_alert_manager() {
            let states = manager.get_states();
            let firing = manager.get_firing_alerts();

            let response = serde_json::json!({
                "alerts": states,
                "firing_count": firing.len(),
                "timestamp": chrono::Utc::now().timestamp_millis(),
            });

            FunctionResult::Success(Some(response))
        } else {
            let response = serde_json::json!({
                "alerts": [],
                "firing_count": 0,
                "message": "Alert manager not initialized",
                "timestamp": chrono::Utc::now().timestamp_millis(),
            });
            FunctionResult::Success(Some(response))
        }
    }

    #[function(
        id = "engine::alerts::evaluate",
        description = "Manually trigger alert evaluation"
    )]
    pub async fn evaluate_alerts(
        &self,
        _input: AlertsEvaluateInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        if let Some(manager) = metrics::get_alert_manager() {
            let events = manager.evaluate().await;

            let response = serde_json::json!({
                "evaluated": true,
                "triggered_alerts": events,
                "timestamp": chrono::Utc::now().timestamp_millis(),
            });

            FunctionResult::Success(Some(response))
        } else {
            let response = serde_json::json!({
                "evaluated": false,
                "message": "Alert manager not initialized",
                "timestamp": chrono::Utc::now().timestamp_millis(),
            });
            FunctionResult::Success(Some(response))
        }
    }

    // =========================================================================
    // Rollups Functions
    // =========================================================================

    #[function(
        id = "engine::rollups::list",
        description = "Get pre-aggregated metrics rollups"
    )]
    pub async fn list_rollups(
        &self,
        input: RollupsListInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
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
                    return FunctionResult::Success(Some(serde_json::json!({
                        "error": "end_time value too large",
                        "rollups": [],
                        "histogram_rollups": [],
                    })));
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
                    return FunctionResult::Success(Some(serde_json::json!({
                        "error": "start_time value too large",
                        "rollups": [],
                        "histogram_rollups": [],
                    })));
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

            let response = serde_json::json!({
                "rollups": rollups,
                "histogram_rollups": histograms,
                "level": level,
                "query": {
                    "start_time": input.start_time,
                    "end_time": input.end_time,
                    "metric_name": input.metric_name,
                },
                "timestamp": chrono::Utc::now().timestamp_millis(),
            });

            FunctionResult::Success(Some(response))
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

                let response = serde_json::json!({
                    "rollups": rollups,
                    "histogram_rollups": histograms,
                    "level": level,
                    "source": "on_the_fly",
                    "query": {
                        "start_time": input.start_time,
                        "end_time": input.end_time,
                        "metric_name": input.metric_name,
                    },
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                });

                FunctionResult::Success(Some(response))
            } else {
                let response = serde_json::json!({
                    "rollups": [],
                    "histogram_rollups": [],
                    "message": "Metric storage not initialized",
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                });
                FunctionResult::Success(Some(response))
            }
        }
    }
}

impl TriggerRegistrator for OtelModule {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
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
        let triggers = &self.triggers.triggers;

        Box::pin(async move {
            tracing::debug!(trigger_id = %trigger.id, "Unregistering log trigger");
            triggers.write().await.remove(&trigger);
            Ok(())
        })
    }
}

#[async_trait]
impl Module for OtelModule {
    fn name(&self) -> &'static str {
        "OtelModule"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        let otel_config: config::OtelModuleConfig = match config {
            Some(cfg) => serde_json::from_value(cfg)?,
            None => config::OtelModuleConfig::default(),
        };

        // Set the global OTEL config so logging can use it
        if !otel::set_otel_config(otel_config.clone()) {
            tracing::warn!(
                "OtelModule created but global config was already set - using existing config"
            );
        }

        let (shutdown_tx, _) = tokio::sync::watch::channel(false);

        Ok(Box::new(OtelModule {
            _config: otel_config,
            triggers: Arc::new(OtelLogTriggers::new()),
            engine,
            shutdown_tx: Arc::new(shutdown_tx),
        }))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        // Initialize metrics if enabled
        let metrics_config = metrics::MetricsConfig::default();
        if metrics_config.enabled && metrics::init_metrics(&metrics_config) {
            // Pre-initialize global engine metrics only if init succeeded
            let _ = metrics::get_engine_metrics();
        }

        // Initialize log storage
        otel::init_log_storage(None);

        // Initialize rollup storage for multi-level metric aggregation
        metrics::init_rollup_storage();
        tracing::info!(
            "{} Rollup storage initialized with 3 levels (1m, 5m, 1h)",
            "[ROLLUPS]".cyan()
        );

        // Initialize alert manager if alerts are configured
        if !self._config.alerts.is_empty() {
            tracing::info!(
                "{} {} alert rules configured",
                "[ALERTS]".cyan(),
                self._config.alerts.len()
            );
            metrics::init_alert_manager_with_engine(
                self._config.alerts.clone(),
                self.engine.clone(),
            );
        }

        // Register log trigger type
        let log_trigger_type = TriggerType {
            id: LOG_TRIGGER_TYPE.to_string(),
            _description: "Log event trigger".to_string(),
            registrator: Box::new(self.clone()),
            worker_id: None,
        };

        let _ = self.engine.register_trigger_type(log_trigger_type).await;

        tracing::info!(
            "{} OpenTelemetry module initialized (log, traces, metrics, logs, rollups functions available)",
            "[READY]".green()
        );
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        // Start log subscriber to invoke triggers for all logs
        {
            let triggers = self.triggers.clone();
            let engine = self.engine.clone();
            let mut shutdown_rx = shutdown.clone();

            tokio::spawn(async move {
                // Wait a bit for log storage to be initialized
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                if let Some(storage) = otel::get_log_storage() {
                    let mut rx = storage.subscribe();

                    tracing::debug!("[OtelModule] Log trigger subscriber started");

                    loop {
                        tokio::select! {
                            _ = shutdown_rx.changed() => {
                                if *shutdown_rx.borrow() {
                                    tracing::debug!("[OtelModule] Log trigger subscriber shutting down");
                                    break;
                                }
                            }
                            result = rx.recv() => {
                                match result {
                                    Ok(log) => {
                                        OtelModule::invoke_triggers_for_log(&triggers, &engine, &log).await;
                                    }
                                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                        tracing::warn!(skipped, "Log trigger subscriber lagged, some logs were skipped");
                                    }
                                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                        tracing::debug!("[OtelModule] Log broadcast channel closed");
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    tracing::debug!("[OtelModule] Log trigger subscriber stopped");
                } else {
                    tracing::warn!(
                        "[OtelModule] Log storage not available, log triggers will not work"
                    );
                }
            });
        }

        // Spawn background task for metrics retention cleanup and rollup processing
        if let Some(storage) = metrics::get_metric_storage() {
            let mut shutdown_rx = shutdown.clone();

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
                loop {
                    tokio::select! {
                        _ = shutdown_rx.changed() => {
                            if *shutdown_rx.borrow() {
                                tracing::debug!("[OtelModule] Metrics retention task shutting down");
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

        // Spawn background task for alert evaluation
        if !self._config.alerts.is_empty() {
            let mut shutdown_rx = shutdown.clone();

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
                loop {
                    tokio::select! {
                        _ = shutdown_rx.changed() => {
                            if *shutdown_rx.borrow() {
                                tracing::debug!("[OtelModule] Alert evaluation task shutting down");
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

        // Start OTLP logs exporter if configured
        let logs_exporter_type = otel::get_logs_exporter_type();
        if (logs_exporter_type == config::LogsExporterType::Otlp
            || logs_exporter_type == config::LogsExporterType::Both)
            && let Some(log_storage) = otel::get_log_storage()
        {
            let endpoint = self
                ._config
                .endpoint
                .clone()
                .unwrap_or_else(|| "http://localhost:4317".to_string());
            let service_name = self
                ._config
                .service_name
                .clone()
                .unwrap_or_else(|| "iii".to_string());
            let service_version = self
                ._config
                .service_version
                .clone()
                .unwrap_or_else(|| "unknown".to_string());

            let rx = log_storage.subscribe();
            let exporter =
                otel::OtlpLogsExporter::new(endpoint.clone(), service_name, service_version);
            exporter.start_with_shutdown(rx, shutdown.clone());

            tracing::info!(
                "{} OTLP logs exporter started (endpoint: {})",
                "[LOGS]".cyan(),
                endpoint
            );
        }

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Shutting down OtelModule...");

        // Signal all background tasks to stop
        let _ = self.shutdown_tx.send(true);

        // Give background tasks time to finish
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Shutdown OTEL components
        otel::shutdown_otel();
        metrics::shutdown_metrics();

        tracing::info!("OtelModule shutdown complete");
        Ok(())
    }
}

crate::register_module!(
    "modules::observability::OtelModule",
    OtelModule,
    enabled_by_default = false
);
