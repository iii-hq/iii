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
    let mut map = HashMap::new();

    if let Some(cfg) = otel::get_otel_config() {
        if let Some(name) = &cfg.service_name {
            map.insert("service.name".to_string(), name.clone());
        }
        if let Some(ns) = &cfg.service_namespace {
            map.insert("service.namespace".to_string(), ns.clone());
        }
        if let Some(ver) = &cfg.service_version {
            map.insert("service.version".to_string(), ver.clone());
        }
    }

    if let Ok(env) = std::env::var("DEPLOYMENT_ENVIRONMENT") {
        map.insert("deployment.environment".to_string(), env);
    }

    map
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
    let mut children_map: HashMap<(String, String), Vec<otel::StoredSpan>> = HashMap::new();
    let mut roots: Vec<otel::StoredSpan> = Vec::new();

    for span in spans {
        match &span.parent_span_id {
            Some(parent_id) => {
                children_map
                    .entry((span.trace_id.clone(), parent_id.clone()))
                    .or_default()
                    .push(span);
            }
            None => roots.push(span),
        }
    }

    fn build_node(
        span: otel::StoredSpan,
        children_map: &mut HashMap<(String, String), Vec<otel::StoredSpan>>,
    ) -> SpanTreeNode {
        let children = children_map
            .remove(&(span.trace_id.clone(), span.span_id.clone()))
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

    #[function(
        id = "engine::log::info",
        description = "Log an info message using OTEL"
    )]
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
                                    || (k == "function_id" && v.starts_with("engine::"))
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

    #[function(
        id = "engine::health::check",
        description = "Check system health status"
    )]
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
                            result = shutdown_rx.changed() => {
                                if result.is_err() {
                                    tracing::debug!("[OtelModule] Shutdown channel closed");
                                    break;
                                }
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
                        result = shutdown_rx.changed() => {
                            if result.is_err() {
                                tracing::debug!("[OtelModule] Shutdown channel closed");
                                break;
                            }
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
                        result = shutdown_rx.changed() => {
                            if result.is_err() {
                                tracing::debug!("[OtelModule] Shutdown channel closed");
                                break;
                            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::collections::HashMap;

    fn with_env_var<T>(key: &str, value: Option<&str>, f: impl FnOnce() -> T) -> T {
        let previous = std::env::var(key).ok();
        match value {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }

        let result = f();

        match previous {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }

        result
    }

    // =========================================================================
    // Helper: create a StoredSpan with configurable fields
    // =========================================================================
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

    fn make_test_module(engine: Arc<Engine>) -> OtelModule {
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        OtelModule {
            _config: config::OtelModuleConfig::default(),
            triggers: Arc::new(OtelLogTriggers::new()),
            engine,
            shutdown_tx: Arc::new(shutdown_tx),
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

    #[test]
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
            "include_internal": true
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
        assert_eq!(input.include_internal.unwrap(), true);
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

    #[test]
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
    fn test_build_span_tree_orphan_child() {
        // A child whose parent is not in the span list becomes an orphan
        // The current implementation only puts spans with None parent in roots.
        // Orphan children (with parent_span_id set but parent not in list) are
        // not treated as roots; they go into children_map but never get collected.
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

        // Since the span has a parent_span_id, it won't appear as a root
        assert!(tree.is_empty());
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

    #[test]
    fn test_get_resource_attributes_includes_deployment_environment_without_otel_config() {
        let attrs = with_env_var("DEPLOYMENT_ENVIRONMENT", Some("staging"), || {
            get_resource_attributes()
        });

        assert_eq!(
            attrs.get("deployment.environment"),
            Some(&"staging".to_string())
        );
    }

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
    fn test_build_span_tree_skips_parent_from_other_trace() {
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
        assert!(tree[0].children.is_empty());
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
            FunctionResult::Success(Some(value)) => assert!(value["value"].is_null()),
            _ => panic!("expected baggage_get to succeed"),
        }

        let set_result = module
            .baggage_set(BaggageSetInput {
                key: "user.id".to_string(),
                value: "123".to_string(),
            })
            .await;
        match set_result {
            FunctionResult::Success(Some(value)) => assert_eq!(value["success"], true),
            _ => panic!("expected baggage_set to succeed"),
        }

        let get_all_result = module.baggage_get_all(BaggageGetAllInput {}).await;
        match get_all_result {
            FunctionResult::Success(Some(value)) => {
                assert!(value["baggage"].is_object());
            }
            _ => panic!("expected baggage_get_all to succeed"),
        }
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
            .list_traces(TracesListInput {
                trace_id: None,
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
                include_internal: Some(false),
            })
            .await;

        match traces_result {
            FunctionResult::Success(Some(value)) => {
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
            FunctionResult::Success(Some(value)) => {
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
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value["error"], "start_time value too large");
            }
            _ => panic!("expected list_metrics overflow response"),
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
            FunctionResult::Success(Some(value)) => {
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
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value["total"], 1);
                assert_eq!(value["logs"].as_array().unwrap().len(), 1);
            }
            _ => panic!("expected list_logs success"),
        }

        let clear_logs_result = module.clear_logs(LogsClearInput {}).await;
        assert!(matches!(
            clear_logs_result,
            FunctionResult::Success(Some(_))
        ));
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
            FunctionResult::Success(Some(value)) => {
                assert!(value["rollups"].is_array());
            }
            _ => panic!("expected list_rollups success"),
        }

        let health_result = module.health_check(HealthCheckInput {}).await;
        match health_result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value["status"], "healthy");
                assert!(value["components"].is_object());
            }
            _ => panic!("expected health_check success"),
        }

        let alerts_result = module.list_alerts(AlertsListInput {}).await;
        match alerts_result {
            FunctionResult::Success(Some(value)) => {
                assert!(value["alerts"].is_array());
            }
            _ => panic!("expected list_alerts success"),
        }

        let evaluate_result = module.evaluate_alerts(AlertsEvaluateInput {}).await;
        match evaluate_result {
            FunctionResult::Success(Some(value)) => {
                assert!(value.get("evaluated").is_some());
            }
            _ => panic!("expected evaluate_alerts success"),
        }

        assert!(matches!(
            module.clear_traces(TracesClearInput {}).await,
            FunctionResult::Success(Some(_))
        ));
        assert_eq!(span_storage.len(), 0);
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
                .contains_key(LOG_TRIGGER_TYPE)
        );

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        module
            .start_background_tasks(shutdown_rx)
            .await
            .expect("start_background_tasks");
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let _ = shutdown_tx.send(true);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        module.destroy().await.expect("destroy");
    }
}
