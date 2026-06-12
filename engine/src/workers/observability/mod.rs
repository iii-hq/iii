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

pub(crate) mod config;

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

#[derive(Serialize, Deserialize, Default, JsonSchema)]
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
    /// Sort field: "start_time" | "duration" (alias "duration_ms") |
    /// "service_name" | "name" (default: "start_time"). Unknown values fall
    /// back to "start_time".
    sort_by: Option<String>,
    /// Sort order: "asc" | "desc" (default: "asc")
    sort_order: Option<String>,
    /// Filter by span attributes (array of [key, value] pairs, AND logic, exact match)
    attributes: Option<Vec<Vec<String>>>,
    /// Include internal engine traces (engine.* functions). Defaults to false.
    #[serde(default)]
    include_internal: Option<bool>,
    /// Search across all spans in each trace, not just root spans.
    /// When true and a `name` filter is set, traces are matched if ANY span
    /// in the trace matches the name filter. Defaults to false.
    #[serde(default)]
    search_all_spans: Option<bool>,
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
}

#[derive(Serialize)]
pub struct TraceGroup {
    pub value: String,
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

fn memory_exporter_not_enabled_error() -> FunctionResult<Option<Value>, ErrorBody> {
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

/// Live span streams the observability worker pushes onto each coalesce tick,
/// consumed by the console Traces view (and any iii client) as a real-time
/// append feed instead of refetching `engine::traces::*`. Ephemeral
/// `stream::send`: the in-memory store stays the source of truth and is
/// re-read once on (re)connect, so a dropped frame self-heals.
const TRACE_ROWS_STREAM: &str = "iii:devtools:trace-rows";
const TRACE_SPANS_STREAM: &str = "iii:devtools:trace-spans";
/// The single group every list subscriber joins — the list is a global
/// firehose, not per-trace. Detail subscribers join the `trace_id` group.
const TRACE_ROWS_GROUP: &str = "all";

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
    /// The config.yaml block passed to `create()` (or built-in defaults).
    /// Used as the seed for first-time `configuration::register` and as the
    /// fetch fallback; the configuration worker entry is the runtime source
    /// of truth afterwards.
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
        self.name.is_match(name) && self.service.as_ref().map_or(true, |s| s.is_match(service))
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
/// handler that produced nothing traceable (e.g. the suppressed devtools stream
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

/// From a trace's corrected (post prune+collapse) spans and the ids that just
/// arrived in this coalesce window, select the spans to push on the detail
/// stream: each arrived survivor plus its corrected ancestor chain, walked to
/// the root.
///
/// Including the chain is what re-attaches a span whose nearest real parent is a
/// KEPT internal wrapper (e.g. `stream_triggers`) that the raw window omitted —
/// without it the consumer treats the span's absent parent as a new depth-0 root
/// (the console/web "phantom root" bug). Walking to the root keeps every frame
/// self-contained, so it survives a dropped earlier frame the same way the feed
/// already self-heals on reconnect; the cost is re-emitting ancestors, which is
/// safe because the detail feed is upsert-by-`span_id`. Returned spans carry
/// their CORRECTED `parent_span_id`, so the consumer's `buildSpanTree` nests
/// them identically to `traces::tree` instead of re-rooting.
fn detail_stream_spans(
    corrected: &[otel::StoredSpan],
    arrived_ids: &HashSet<String>,
) -> Vec<otel::StoredSpan> {
    let parent_of: HashMap<&str, Option<&str>> = corrected
        .iter()
        .map(|s| (s.span_id.as_str(), s.parent_span_id.as_deref()))
        .collect();

    let mut emit_ids: HashSet<String> = HashSet::new();
    for span in corrected {
        // Start only from spans that actually arrived this window AND survived
        // the pipeline; a collapsed/pruned arrival simply contributes nothing.
        if !arrived_ids.contains(&span.span_id) {
            continue;
        }
        let mut cursor: Option<&str> = Some(span.span_id.as_str());
        while let Some(id) = cursor {
            // Already walked this id (and therefore its ancestors) — stop.
            // This also terminates parent cycles: a cycle must revisit an id.
            if !emit_ids.insert(id.to_string()) {
                break;
            }
            cursor = parent_of.get(id).copied().flatten();
        }
    }

    corrected
        .iter()
        .filter(|s| emit_ids.contains(&s.span_id))
        .cloned()
        .collect()
}

/// Shared trace-correction pipeline: drop no-op trigger fan-out wrappers
/// (childless state_triggers/stream_triggers — wrappers that actually invoked
/// a handler are kept so trigger→handler causality stays visible), then
/// collapse user-configured pass-through spans, reparenting children to the
/// nearest survivor. Both `traces::tree` and the live detail stream go through
/// this one function so the live feed can never disagree with the REST tree.
fn correct_trace_spans(
    spans: Vec<otel::StoredSpan>,
    rules: &[CompiledCollapseRule],
) -> Vec<otel::StoredSpan> {
    collapse_spans(prune_empty_trigger_spans(spans), rules)
}

/// Build the detail-stream payload for one trace: run the same
/// [`correct_trace_spans`] pipeline `traces::tree` uses over the FULL raw
/// trace, then keep each arrived span and its corrected ancestor chain (see
/// [`detail_stream_spans`]).
///
/// The full trace — not just the window — is required because the surviving
/// ancestor a span must reparent to may have arrived in an earlier window.
/// Pure over its inputs so the correction is unit-testable without span storage.
fn corrected_detail_spans(
    full_trace: Vec<otel::StoredSpan>,
    arrived_ids: &HashSet<String>,
    rules: &[CompiledCollapseRule],
) -> Vec<otel::StoredSpan> {
    let corrected = correct_trace_spans(full_trace, rules);
    detail_stream_spans(&corrected, arrived_ids)
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

        // Seed the global OTEL config so logging can use it. On the serve
        // path the global is already populated during logging init (from the
        // persisted configuration entry or this same yaml block); first-set
        // semantics keep that value authoritative.
        if !otel::set_otel_config(otel_config.clone()) {
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
                metadata: None,
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
    /// - SWAP: sampler, alert rules, collapse-rule cache, and the engine log
    ///   level are rebuilt only when their fields changed.
    /// - TASK-REBUILD: the log-retention and OTLP-logs-exporter tasks are
    ///   respawned when their captured settings changed; refused (warn) when
    ///   the worker has not started or was destroyed.
    /// - RESTART-ONLY: trace exporter wiring, resource identity on traces,
    ///   pipeline enablement, metrics exporter, and the log format are baked
    ///   in at process start; changes are reported with a warning and take
    ///   effect at the next engine start (the persisted entry is read at
    ///   boot).
    pub(crate) async fn apply_config(&self) -> anyhow::Result<()> {
        let _guard = self.apply_lock.lock().await;

        let old = self.current_config();
        let new = tokio::time::timeout(
            configuration::CONFIG_BUS_TIMEOUT,
            configuration::fetch_config(self.engine.as_ref(), &old),
        )
        .await
        .map_err(|elapsed| anyhow::Error::new(elapsed).context("configuration::get timed out"))??;

        // LIVE tier: swap the global snapshot.
        otel::update_otel_config(new.clone());

        // LIMITS tier: idempotent, applied unconditionally.
        if let Some(storage) = otel::get_span_storage()
            && let Some(max) = new.memory_max_spans
        {
            storage.set_max_spans(max);
        }
        otel::init_log_storage(new.logs_max_count);
        metrics::init_metric_storage(new.metrics_max_count, new.metrics_retention_seconds);

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
        if old.level != new.level {
            match &new.level {
                Some(level) => {
                    if let Err(err) = crate::logging::reload_log_level(level) {
                        tracing::warn!(
                            error = %err,
                            "iii-observability: log level not applied; keeping current level"
                        );
                    }
                }
                // Removing the key does not revert to a default: the boot
                // level may have come from env/CLI, not this entry.
                None => tracing::debug!(
                    "iii-observability: level removed from configuration; keeping current level"
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
        let respawn_retention = old.logs_retention_seconds != new.logs_retention_seconds;
        let respawn_exporter = old.logs_exporter != new.logs_exporter
            || old.logs_batch_size != new.logs_batch_size
            || old.logs_flush_interval_ms != new.logs_flush_interval_ms
            || old.endpoint != new.endpoint
            || old.service_name != new.service_name
            || old.service_version != new.service_version;
        if respawn_retention || respawn_exporter {
            let started = self
                .worker_shutdown_rx
                .lock()
                .expect("worker_shutdown_rx mutex poisoned")
                .is_some();
            if started {
                if respawn_retention {
                    self.spawn_logs_retention(&new);
                }
                if respawn_exporter {
                    self.spawn_logs_exporter(&new);
                }
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
            restart_fields.push("endpoint (trace exporter/forwarder; logs exporter rebuilt live)");
        }
        if old.service_name != new.service_name
            || old.service_version != new.service_version
            || old.service_namespace != new.service_namespace
        {
            restart_fields.push("service identity on the trace resource");
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
        let previous = self
            .logs_exporter_stop
            .lock()
            .expect("logs_exporter_stop mutex poisoned")
            .replace(stop_tx);
        if let Some(previous) = previous {
            let _ = previous.send(true);
        }

        let exporter_type = cfg
            .logs_exporter
            .clone()
            .unwrap_or(config::LogsExporterType::Memory);
        if exporter_type == config::LogsExporterType::Memory {
            return; // OTLP export disabled: previous task stopped
        }
        let Some(log_storage) = otel::get_log_storage() else {
            return;
        };
        let Some(worker_shutdown_rx) = self
            .worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned")
            .clone()
        else {
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

        let rx = log_storage.subscribe();
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
                let function_id = trigger.function_id.clone();

                tokio::spawn(async move {
                    let _ = engine.call(&function_id, log_data).await;
                });
            }
        }
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
    /// The handler is invoked fire-and-forget via `engine.call`; results are
    /// ignored. NOTE: that `engine.call` is itself instrumented as a span, so
    /// the subscriber MUST exclude trigger-delivery spans before they reach
    /// here (see `start_background_tasks`) or the trigger would re-fire on its
    /// own delivery — an unbounded feedback loop.
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
            let function_id = trigger.function_id.clone();

            tokio::spawn(async move {
                let _ = engine.call(&function_id, payload).await;
            });
        }
    }

    /// Push the coalesce window's spans onto the live trace streams via
    /// ephemeral `stream::send`: root rows (internal/plumbing excluded) to the
    /// global list stream, and — per trace touched this window — tree-corrected
    /// spans to that trace's detail stream. The detail payload is rebuilt from
    /// the FULL trace in storage with the same prune+collapse pipeline as
    /// `traces::tree` (see `corrected_detail_spans`), so the live feed is
    /// directly appendable and never re-orphans a span whose real parent was an
    /// internal wrapper the window filtered out. Fire-and-forget — the in-memory
    /// store stays authoritative (re-read once on reconnect), so a dropped frame
    /// self-heals rather than corrupting client state.
    ///
    /// Loop-safe by construction: spans only ENTER the feed through the
    /// subscriber's post-filter window, which already excludes engine-internal
    /// spans (`is_internal_span`) and the trigger's own delivery spans. The
    /// detail payload may re-read KEPT internal wrappers from storage to repair
    /// causality, but those are existing stored spans, not new ones — and
    /// `stream::send` plus the `iii::console::*` consumer handlers are all
    /// builtins (`iii.function.kind=internal`), so neither delivery re-enters
    /// the feed (see the subscriber loop's loop-break + consumer contract).
    async fn push_trace_streams(engine: &Arc<Engine>, batch: &[otel::StoredSpan]) {
        fn send(engine: &Arc<Engine>, stream_name: &str, group_id: String, spans: Value) {
            let payload = serde_json::json!({
                "stream_name": stream_name,
                "group_id": group_id,
                "type": "spans",
                "data": { "spans": spans },
            });
            let engine = engine.clone();
            tokio::spawn(async move {
                let _ = engine.call("stream::send", payload).await;
            });
        }

        // List: one row per root span, internal/plumbing excluded (the same
        // view `traces::list` shows), to the single group every list view joins.
        let rows: Vec<&otel::StoredSpan> = batch
            .iter()
            .filter(|s| s.parent_span_id.is_none() && !is_internal_span(s))
            .collect();
        if !rows.is_empty()
            && let Ok(spans) = serde_json::to_value(&rows)
        {
            send(
                engine,
                TRACE_ROWS_STREAM,
                TRACE_ROWS_GROUP.to_string(),
                spans,
            );
        }

        // Detail: tree-corrected spans grouped by trace_id, to that trace's
        // group. The raw window is structurally lossy — its spans' parents point
        // at engine-internal wrappers (`stream_triggers` etc.) the subscriber
        // already filtered out, so a naive push re-orphans every span whose real
        // parent was such a wrapper. Instead, per touched trace, recompute
        // structure from the FULL trace in storage with the SAME pipeline as
        // `traces::tree`, then push each arrived span with its corrected ancestor
        // chain. Only a subscriber that joined this `trace_id` receives it
        // (engine-side fan-out filter), so a trace nobody is viewing costs just
        // the no-match early-out in `stream::invoke_triggers`.
        //
        // Cost: one in-memory `get_spans_by_trace_id` + prune/collapse per
        // distinct trace per coalesce tick (bounded by the 300ms window and the
        // set of traces actively streaming). Acceptable; a future optimization
        // could memoize the corrected parent map per trace.
        let Some(storage) = otel::get_span_storage() else {
            return; // No in-memory store → nothing to correct against.
        };
        let collapse_rules = cached_collapse_rules();
        let collapse_rules = collapse_rules.as_slice();

        let mut arrived_by_trace: HashMap<&str, HashSet<String>> = HashMap::new();
        for span in batch {
            arrived_by_trace
                .entry(&span.trace_id)
                .or_default()
                .insert(span.span_id.clone());
        }

        for (trace_id, arrived_ids) in arrived_by_trace {
            let full_trace = storage.get_spans_by_trace_id(trace_id);
            let to_send = corrected_detail_spans(full_trace, &arrived_ids, collapse_rules);
            if !to_send.is_empty()
                && let Ok(spans) = serde_json::to_value(&to_send)
            {
                send(engine, TRACE_SPANS_STREAM, trace_id.to_string(), spans);
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
                let search_all = input.search_all_spans.unwrap_or(false);

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

                let mut filtered: Vec<_> = all_spans
                    .into_iter()
                    // Root-only by default; `search_all_spans` widens to children too.
                    .filter(|s| search_all || s.parent_span_id.is_none())
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
                        let duration_ns =
                            s.end_time_unix_nano.saturating_sub(s.start_time_unix_nano);
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

                let sort_order_asc = input
                    .sort_order
                    .as_deref()
                    .map(|o| o.eq_ignore_ascii_case("asc"))
                    .unwrap_or(true);

                filtered.sort_by(|a, b| {
                    let cmp = match input.sort_by.as_deref().unwrap_or("start_time") {
                        // Accept both "duration" and "duration_ms"; the rest of
                        // the API (min_duration_ms/max_duration_ms, the output
                        // span `duration_ms`) uses the `_ms` suffix, so callers
                        // reasonably pass either spelling.
                        "duration" | "duration_ms" => {
                            let da =
                                a.end_time_unix_nano.saturating_sub(a.start_time_unix_nano) as f64;
                            let db =
                                b.end_time_unix_nano.saturating_sub(b.start_time_unix_nano) as f64;
                            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                        }
                        "service_name" => a.service_name.cmp(&b.service_name),
                        "name" => a.name.cmp(&b.name),
                        _ => a.start_time_unix_nano.cmp(&b.start_time_unix_nano),
                    };
                    if sort_order_asc { cmp } else { cmp.reverse() }
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
            None => memory_exporter_not_enabled_error(),
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

                let all_spans = correct_trace_spans(all_spans, &cached_collapse_rules());

                let roots = build_span_tree(all_spans);

                let response = serde_json::json!({
                    "roots": roots,
                });
                FunctionResult::Success(Some(response))
            }
            None => memory_exporter_not_enabled_error(),
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
            None => memory_exporter_not_enabled_error(),
        }
    }

    /// Aggregate stored spans by one attribute value. Returns up to
    /// `limit` groups (default 100), each with trace_ids, span_count,
    /// duration, and error_count.
    #[function(
        id = "engine::traces::group_by",
        description = "Group stored spans by an attribute value (only available when exporter is 'memory' or 'both')"
    )]
    pub async fn group_traces_by(
        &self,
        input: TracesGroupByInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match otel::get_span_storage() {
            Some(storage) => {
                let all_spans = storage.get_spans();

                let include_internal = input.include_internal.unwrap_or(false);
                let since_ns = input.since_ms.map(|ms| ms.saturating_mul(1_000_000));
                let limit = input.limit.unwrap_or(100) as usize;

                struct GroupBuilder {
                    trace_ids: std::collections::HashSet<String>,
                    span_count: u32,
                    first_seen_ns: u64,
                    last_seen_ns: u64,
                    error_count: u32,
                }
                let mut groups: std::collections::HashMap<String, GroupBuilder> =
                    std::collections::HashMap::new();

                for span in &all_spans {
                    if !include_internal {
                        let is_internal = span.attributes.iter().any(|(k, v)| {
                            (k == "iii.function.kind" && v == "internal")
                                || (k == "function_id" && v.starts_with("engine::"))
                        });
                        if is_internal {
                            continue;
                        }
                    }
                    if let Some(min_ns) = since_ns
                        && span.end_time_unix_nano < min_ns
                    {
                        continue;
                    }

                    let value = match span.attributes.iter().find(|(k, _)| k == &input.attribute) {
                        Some((_, v)) => v.clone(),
                        None => continue,
                    };

                    let is_error = span.status.eq_ignore_ascii_case("error");
                    let entry = groups.entry(value).or_insert_with(|| GroupBuilder {
                        trace_ids: std::collections::HashSet::new(),
                        span_count: 0,
                        first_seen_ns: span.start_time_unix_nano,
                        last_seen_ns: span.end_time_unix_nano,
                        error_count: 0,
                    });
                    entry.trace_ids.insert(span.trace_id.clone());
                    entry.span_count += 1;
                    entry.first_seen_ns = entry.first_seen_ns.min(span.start_time_unix_nano);
                    entry.last_seen_ns = entry.last_seen_ns.max(span.end_time_unix_nano);
                    if is_error {
                        entry.error_count += 1;
                    }
                }

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

                FunctionResult::Success(Some(serde_json::json!({ "groups": result })))
            }
            None => memory_exporter_not_enabled_error(),
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
                // Honor logs_enabled: do NOT lazily revive storage when logs
                // are disabled at config time. Return an empty result so API
                // consumers get a consistent shape.
                if otel::logs_enabled(Some(&self._config)) {
                    otel::init_log_storage(self.effective_logs_max_count());
                }
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

        // Check span storage
        let spans_component = if let Some(storage) = otel::get_span_storage() {
            healthy_component(serde_json::json!({
                "stored_spans": storage.len(),
            }))
        } else {
            disabled_component()
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
                "rules": manager.get_rules(),
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
        let enabled = self._config.enabled.unwrap_or(true);
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
        if otel::logs_enabled(Some(&self._config)) {
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
        if !self._config.alerts.is_empty() {
            tracing::info!(
                "{} {} alert rules configured",
                "[ALERTS]".cyan(),
                self._config.alerts.len()
            );
        }
        metrics::init_alert_manager_with_engine(self._config.alerts.clone(), self.engine.clone());

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

        // Skip all background tasks when observability is disabled
        if !self.current_config().enabled.unwrap_or(true) {
            tracing::debug!("[ObservabilityWorker] Skipping background tasks (disabled)");
            return Ok(());
        }

        // Start log subscriber to invoke triggers for all logs
        {
            let triggers = self.triggers.clone();
            let engine = self.engine.clone();
            let mut shutdown_rx = shutdown_rx.clone();

            tokio::spawn(async move {
                // Wait a bit for log storage to be initialized
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                if let Some(storage) = otel::get_log_storage() {
                    let mut rx = storage.subscribe();

                    tracing::debug!("[ObservabilityWorker] Log trigger subscriber started");

                    loop {
                        tokio::select! {
                            result = shutdown_rx.changed() => {
                                if result.is_err() {
                                    tracing::debug!("[ObservabilityWorker] Shutdown channel closed");
                                    break;
                                }
                                if *shutdown_rx.borrow() {
                                    tracing::debug!("[ObservabilityWorker] Log trigger subscriber shutting down");
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
                } else {
                    tracing::warn!(
                        "[ObservabilityWorker] Log storage not available, log triggers will not work"
                    );
                }
            });
        }

        // Start span subscriber: coalesce span activity into periodic `trace`
        // trigger fan-outs, excluding engine-internal spans and the trigger's
        // OWN delivery spans. `engine.call` to a consumer fn is itself
        // instrumented as a span, so without this exclusion the trigger would
        // re-fire on its own output — an unbounded feedback loop that floods
        // the consumer and fills the trace store with delivery spans.
        {
            let triggers = self.trace_triggers.clone();
            let engine = self.engine.clone();
            let mut shutdown_rx = shutdown_rx.clone();

            tokio::spawn(async move {
                // Wait a bit for span storage to be initialized by exporter setup.
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                let Some(storage) = otel::get_span_storage() else {
                    // Span storage is only present when the memory/both exporter
                    // is configured; with the OTLP-only exporter there is no
                    // in-memory store to watch, so trace triggers stay dormant.
                    tracing::debug!(
                        "[ObservabilityWorker] Span storage not available (memory exporter off), trace triggers will not fire"
                    );
                    return;
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
                                    // Loop-break: drop engine-internal spans and
                                    // the trigger's own delivery spans.
                                    if is_internal_span(&span) {
                                        continue;
                                    }
                                    if span_function_id(&span).is_some_and(|f| trigger_fns.contains(f)) {
                                        continue;
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
                            ObservabilityWorker::push_trace_streams(&engine, &batch).await;
                        }
                    }
                }

                tracing::debug!("[ObservabilityWorker] Trace trigger subscriber stopped");
            });
        }

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

        // Adopt the configuration worker as the runtime source of truth.
        // `configuration::*` is callable here on both pipelines (initial boot
        // registers all worker functions before serving; reload starts the
        // mandatory configuration worker independently). Failures degrade to
        // the static config.yaml block so observability stays up. Every bus
        // call is time-bounded: worker startup is awaited serially by the
        // boot and reload pipelines.
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

        // Spawn the respawnable tasks from the post-sync effective
        // configuration. The helpers stop any instance an apply above may
        // already have spawned, so this cannot double-spawn.
        let effective = self.current_config();
        self.spawn_logs_retention(&effective);
        self.spawn_logs_exporter(&effective);

        // Register the handler before the trigger so a configuration event
        // can never fan out to a missing function. On reload,
        // `register_functions` runs after this hook and re-registers the
        // handler inside the worker scope; the `get` check keeps the
        // initial-boot path (where it already ran) from logging a spurious
        // "already registered" overwrite.
        if self
            .engine
            .functions
            .get(configuration::CONFIG_FN_ID)
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
        for stop in [&self.logs_retention_stop, &self.logs_exporter_stop] {
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

    // =========================================================================
    // detail stream correction tests (corrected_detail_spans)
    // =========================================================================

    #[test]
    fn test_corrected_detail_spans_reattaches_under_kept_wrapper() {
        // turn -> steering_check -> stream_triggers (internal wrapper) -> leaf.
        // Only `leaf` arrives this window — the wrapper is internal, so the span
        // subscriber filtered it out of the batch. The OLD feed pushed just
        // `leaf` pointing at the absent wrapper → phantom depth-0 root. The
        // corrected payload must KEEP the wrapper (it has a child) and emit the
        // whole chain so `leaf` nests under it, matching `traces::tree`.
        let turn = make_span("t", "turn", None, "turn", "harness", 100, 900, "ok", vec![]);
        let steering = make_span(
            "t",
            "steer",
            Some("turn"),
            "steering_check",
            "harness",
            110,
            880,
            "ok",
            vec![],
        );
        let wrapper = make_span(
            "t",
            "wrap",
            Some("steer"),
            "stream_triggers",
            "harness",
            120,
            870,
            "ok",
            vec![("iii.function.kind", "internal")],
        );
        let leaf = make_span(
            "t",
            "leaf",
            Some("wrap"),
            "context-compaction::on_agent_event",
            "ctx",
            130,
            860,
            "ok",
            vec![],
        );

        let full = vec![turn, steering, wrapper, leaf];
        let arrived: HashSet<String> = ["leaf".to_string()].into_iter().collect();

        let sent = corrected_detail_spans(full, &arrived, &[]);
        let ids: HashSet<&str> = sent.iter().map(|s| s.span_id.as_str()).collect();

        // Kept internal wrapper + the spine are emitted so nothing re-roots.
        assert!(
            ids.contains("wrap"),
            "kept internal wrapper must be streamed so the leaf has a present parent"
        );
        assert!(ids.contains("leaf"));
        assert!(ids.contains("steer"));
        assert!(ids.contains("turn"));

        // The leaf still parents to the wrapper, not to a phantom root.
        let leaf = sent.iter().find(|s| s.span_id == "leaf").unwrap();
        assert_eq!(leaf.parent_span_id.as_deref(), Some("wrap"));

        // The chain is intact: building the tree yields a single root.
        let tree = build_span_tree(sent);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].span.span_id, "turn");
    }

    #[test]
    fn test_corrected_detail_spans_drops_childless_wrapper() {
        // steering_check has a childless `stream_triggers` (no-op fan-out) plus a
        // real `leaf`. The childless wrapper is pruned and never streamed; the
        // leaf streams under the surviving `steering_check`.
        let steering = make_span(
            "t",
            "steer",
            None,
            "steering_check",
            "harness",
            100,
            500,
            "ok",
            vec![],
        );
        let empty_wrapper = make_span(
            "t",
            "wrap",
            Some("steer"),
            "stream_triggers",
            "harness",
            110,
            200,
            "ok",
            vec![("iii.function.kind", "internal")],
        );
        let leaf = make_span(
            "t",
            "leaf",
            Some("steer"),
            "models::get",
            "models",
            120,
            480,
            "ok",
            vec![],
        );

        let full = vec![steering, empty_wrapper, leaf];
        let arrived: HashSet<String> = ["leaf".to_string()].into_iter().collect();

        let sent = corrected_detail_spans(full, &arrived, &[]);
        let ids: HashSet<&str> = sent.iter().map(|s| s.span_id.as_str()).collect();

        assert!(
            !ids.contains("wrap"),
            "childless trigger wrapper must be pruned, not streamed"
        );
        assert!(ids.contains("leaf"));
        assert!(ids.contains("steer"));
        let leaf = sent.iter().find(|s| s.span_id == "leaf").unwrap();
        assert_eq!(leaf.parent_span_id.as_deref(), Some("steer"));
    }

    #[test]
    fn test_corrected_detail_spans_applies_collapse_reparent() {
        // call -> trigger (collapse rule) -> leaf. The collapsed wrapper is
        // removed and the leaf reparents to `call` on the stream, just as in
        // `traces::tree`.
        let call = make_span(
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
        let full = vec![call, wrapper, leaf];
        let arrived: HashSet<String> = ["leaf".to_string()].into_iter().collect();

        let sent = corrected_detail_spans(full, &arrived, &rules);
        let ids: HashSet<&str> = sent.iter().map(|s| s.span_id.as_str()).collect();

        assert!(
            !ids.contains("trig"),
            "collapsed wrapper must not be streamed"
        );
        assert!(ids.contains("call"));
        let leaf = sent.iter().find(|s| s.span_id == "leaf").unwrap();
        assert_eq!(leaf.parent_span_id.as_deref(), Some("call"));
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
        assert!(input.include_internal.unwrap());
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
            include_internal: Some(false),
            search_all_spans: None,
        };

        let order = |result: FunctionResult<Option<Value>, ErrorBody>| -> Vec<String> {
            match result {
                FunctionResult::Success(Some(value)) => value["spans"]
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
                .list_traces(TracesListInput {
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
                .list_traces(TracesListInput {
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
                .list_traces(TracesListInput {
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
                search_all_spans: None,
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
            .list_traces(TracesListInput {
                trace_id: None,
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
                include_internal: Some(true),
                search_all_spans: Some(true),
            })
            .await;

        match result {
            FunctionResult::Success(Some(value)) => {
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
            .list_traces(TracesListInput {
                trace_id: None,
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
                include_internal: Some(true),
                search_all_spans: Some(false),
            })
            .await;
        match result_root_only {
            FunctionResult::Success(Some(value)) => {
                let spans = value["spans"].as_array().expect("spans array");
                assert_eq!(spans.len(), 1, "default mode = root only");
                assert_eq!(spans[0]["span_id"], "root-1");
            }
            _ => panic!("expected success"),
        }

        let result_all = module
            .list_traces(TracesListInput {
                trace_id: None,
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
                include_internal: Some(true),
                search_all_spans: Some(true),
            })
            .await;
        match result_all {
            FunctionResult::Success(Some(value)) => {
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
            .list_traces(TracesListInput {
                trace_id: None,
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
                include_internal: Some(true),
                search_all_spans: Some(false),
            })
            .await;

        match result {
            FunctionResult::Success(Some(value)) => {
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
            })
            .await;

        match result {
            FunctionResult::Success(Some(value)) => {
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
            })
            .await;

        match result {
            FunctionResult::Success(Some(value)) => {
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
            })
            .await;

        match result {
            FunctionResult::Success(Some(value)) => {
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
            })
            .await;

        match result {
            FunctionResult::Success(Some(value)) => {
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
        let engine = Arc::new(Engine::new());
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        let worker = ObservabilityWorker {
            _config: config::ObservabilityWorkerConfig {
                enabled: Some(false),
                ..config::ObservabilityWorkerConfig::default()
            },
            triggers: Arc::new(OtelLogTriggers::new()),
            trace_triggers: Arc::new(OtelTraceTriggers::new()),
            engine: engine.clone(),
            shutdown_tx: Arc::new(shutdown_tx),
            worker_shutdown_rx: Arc::new(std::sync::Mutex::new(None)),
            logs_retention_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_exporter_stop: Arc::new(std::sync::Mutex::new(None)),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
        };

        let result = worker.initialize().await;
        assert!(result.is_ok());

        // Verify the trigger types were NOT registered (early return skipped it)
        assert!(
            !engine
                .trigger_registry
                .trigger_types
                .contains_key(LOG_TRIGGER_TYPE)
        );
        assert!(
            !engine
                .trigger_registry
                .trigger_types
                .contains_key(TRACE_TRIGGER_TYPE)
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_initialize_defaults_to_enabled_when_none() {
        reset_observability_test_state();
        let engine = Arc::new(Engine::new());
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        let worker = ObservabilityWorker {
            _config: config::ObservabilityWorkerConfig {
                enabled: None,
                ..config::ObservabilityWorkerConfig::default()
            },
            triggers: Arc::new(OtelLogTriggers::new()),
            trace_triggers: Arc::new(OtelTraceTriggers::new()),
            engine: engine.clone(),
            shutdown_tx: Arc::new(shutdown_tx),
            worker_shutdown_rx: Arc::new(std::sync::Mutex::new(None)),
            logs_retention_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_exporter_stop: Arc::new(std::sync::Mutex::new(None)),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
        };

        let result = worker.initialize().await;
        assert!(result.is_ok());

        // Verify the trigger types WERE registered (enabled: None defaults to true)
        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key(LOG_TRIGGER_TYPE)
        );
        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key(TRACE_TRIGGER_TYPE)
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_start_background_tasks_returns_ok_when_disabled() {
        reset_observability_test_state();
        let engine = Arc::new(Engine::new());
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let worker = ObservabilityWorker {
            _config: config::ObservabilityWorkerConfig {
                enabled: Some(false),
                ..config::ObservabilityWorkerConfig::default()
            },
            triggers: Arc::new(OtelLogTriggers::new()),
            trace_triggers: Arc::new(OtelTraceTriggers::new()),
            engine: engine.clone(),
            shutdown_tx: Arc::new(shutdown_tx.clone()),
            worker_shutdown_rx: Arc::new(std::sync::Mutex::new(None)),
            logs_retention_stop: Arc::new(std::sync::Mutex::new(None)),
            logs_exporter_stop: Arc::new(std::sync::Mutex::new(None)),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
        };

        let result = worker
            .start_background_tasks(shutdown_rx, shutdown_tx)
            .await;
        assert!(result.is_ok());
    }
}
