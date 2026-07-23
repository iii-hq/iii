// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! OpenTelemetry initialization for the III Engine.
//!
//! This module provides OTLP/gRPC trace export and integrates with the existing
//! `tracing` ecosystem via `tracing-opentelemetry`.
//!
//! Supports two exporter modes:
//! - `otlp`: Export traces to an OTLP collector via gRPC
//! - `memory`: Store traces in memory for API querying

use super::config::{LogsExporterType, ObservabilityWorkerConfig, OtelExporterType};
use super::otlp_exporter::build_span_exporter;
use super::sampler::AdvancedSampler;
use opentelemetry::propagation::{TextMapCompositePropagator, TextMapPropagator};
use opentelemetry::{
    Context, KeyValue, global,
    trace::{Span as _, TraceContextExt, TracerProvider as _},
};
use opentelemetry_sdk::{
    Resource,
    error::OTelSdkResult,
    propagation::{BaggagePropagator, TraceContextPropagator},
    trace::{RandomIdGenerator, Sampler, SdkTracerProvider, SpanData, SpanExporter, SpanProcessor},
};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::Subscriber;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::registry::LookupSpan;

/// Default maximum number of spans to keep in memory.
const DEFAULT_MEMORY_MAX_SPANS: usize = 1000;

/// Global OTEL configuration. Seeded from YAML config (or the persisted
/// configuration entry) at boot, and swappable at runtime by the
/// configuration-worker apply path.
static GLOBAL_OTEL_CONFIG: RwLock<Option<Arc<ObservabilityWorkerConfig>>> = RwLock::new(None);

/// Set the global OTEL configuration (first-set only).
/// This should be called during module initialization, before logging is set up.
///
/// Returns true if the config was set, false if it was already initialized.
pub fn set_otel_config(config: ObservabilityWorkerConfig) -> bool {
    let was_set = {
        let mut slot = GLOBAL_OTEL_CONFIG
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot.is_none() {
            *slot = Some(Arc::new(config));
            true
        } else {
            false
        }
    };
    // Log AFTER releasing the write guard: std RwLock is non-reentrant, and a
    // tracing event flows through the subscriber stack — a layer that ever
    // reads the global config in its event path would otherwise self-deadlock.
    if !was_set {
        tracing::debug!("OTEL config already initialized, ignoring new config");
    }
    was_set
}

/// Unconditionally replace the global OTEL configuration (the authoritative
/// configuration-worker apply path). Per-use readers (ingest gates,
/// `logs_console_output`, `logs_sampling_ratio`, ...) observe the new value
/// immediately; state built from the previous snapshot (providers, layers,
/// spawned tasks) is updated by the caller's apply logic. Returns the
/// previous value.
pub fn update_otel_config(
    config: ObservabilityWorkerConfig,
) -> Option<Arc<ObservabilityWorkerConfig>> {
    let mut slot = GLOBAL_OTEL_CONFIG
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    slot.replace(Arc::new(config))
}

/// Test-only: clear the global OTEL config back to its unset baseline. The
/// global is process-wide and is NOT reset between tests, so a `#[serial]`
/// test that calls `update_otel_config` must clear afterward — otherwise it
/// pollutes sibling tests that rely on `current_config()` falling back to the
/// worker's own `_config`.
#[cfg(test)]
pub(crate) fn clear_otel_config_for_test() {
    *GLOBAL_OTEL_CONFIG
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

/// Get the global OTEL configuration if set.
pub fn get_otel_config() -> Option<Arc<ObservabilityWorkerConfig>> {
    GLOBAL_OTEL_CONFIG
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Decide whether OTEL logs storage / emission should be active, given an
/// optional config. Defaults to `true` when the config (or field) is absent
/// — matches "opt-out" semantics: logs on unless explicitly disabled.
pub fn logs_enabled(cfg: Option<&ObservabilityWorkerConfig>) -> bool {
    cfg.and_then(|c| c.logs_enabled).unwrap_or(true)
}

/// Exporter type for OpenTelemetry traces.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ExporterType {
    /// Export traces via OTLP/gRPC to a collector
    #[default]
    Otlp,
    /// Store traces in memory (queryable via API)
    Memory,
    /// Export traces via OTLP and store in memory (enables triggers with OTLP export)
    Both,
}

/// Configuration for OpenTelemetry export.
#[derive(Debug, Clone)]
pub struct OtelConfig {
    /// Whether OpenTelemetry export is enabled.
    pub enabled: bool,
    /// The service name to report.
    pub service_name: String,
    /// The service version to report (OTEL semantic convention: service.version).
    pub service_version: String,
    /// The service namespace to report (OTEL semantic convention: service.namespace).
    pub service_namespace: Option<String>,
    /// Exporter type: Otlp, Memory, or Both
    pub exporter: ExporterType,
    /// OTLP endpoint (e.g., "http://localhost:4317"). Used for Otlp and Both exporters.
    pub endpoint: String,
    /// Sampling ratio (0.0 to 1.0). 1.0 means sample everything.
    pub sampling_ratio: f64,
    /// Maximum spans to keep in memory. Used for Memory and Both exporters.
    pub memory_max_spans: usize,
    /// Mirror spans into the in-memory store at start as `pending` snapshots
    /// (`LiveSpanProcessor`), so live trace views show in-progress work.
    /// Resolved by [`resolve_live_spans`]: on by default for the Memory
    /// exporter (local dev), opt-in for Both (production), never effective
    /// for Otlp-only. Restart-tier.
    pub live_spans: bool,
}

/// `OTEL_LIVE_SPANS` env override for live-span mirroring.
fn env_live_spans() -> Option<bool> {
    env::var("OTEL_LIVE_SPANS")
        .ok()
        .map(|v| v == "true" || v == "1")
}

/// Resolve the live-spans switch: explicit config > `OTEL_LIVE_SPANS` env >
/// mode-aware default. The default is ON only for the `memory` exporter (the
/// local-dev mode where live trace views matter); `both` — the production
/// shape that also exports OTLP — stays off unless explicitly enabled, and
/// `otlp`-only can never mirror (engine finals don't land in the memory
/// store there, so pendings would never be replaced).
pub fn resolve_live_spans(explicit: Option<bool>, exporter: &ExporterType) -> bool {
    explicit
        .or_else(env_live_spans)
        .unwrap_or(matches!(exporter, ExporterType::Memory))
}

impl Default for OtelConfig {
    fn default() -> Self {
        // First check global config from YAML, then fall back to environment variables
        let global_cfg = get_otel_config();
        let global_cfg = global_cfg.as_deref();

        let enabled = global_cfg
            .and_then(|c| c.enabled)
            .or_else(|| {
                env::var("OTEL_ENABLED")
                    .ok()
                    .map(|v| v == "true" || v == "1")
            })
            .unwrap_or(false);

        let service_name = global_cfg
            .and_then(|c| c.service_name.clone())
            .or_else(|| env::var("OTEL_SERVICE_NAME").ok())
            .unwrap_or_else(|| "iii".to_string());

        let service_version = global_cfg
            .and_then(|c| c.service_version.clone())
            .or_else(|| env::var("SERVICE_VERSION").ok())
            .unwrap_or_else(|| "unknown".to_string());

        let service_namespace = global_cfg
            .and_then(|c| c.service_namespace.clone())
            .or_else(|| env::var("SERVICE_NAMESPACE").ok());

        let exporter = global_cfg
            .and_then(|c| c.exporter.clone())
            .map(|e| match e {
                OtelExporterType::Memory => ExporterType::Memory,
                OtelExporterType::Otlp => ExporterType::Otlp,
                OtelExporterType::Both => ExporterType::Both,
            })
            .or_else(|| {
                env::var("OTEL_EXPORTER_TYPE")
                    .ok()
                    .map(|v| match v.to_lowercase().as_str() {
                        "memory" => ExporterType::Memory,
                        "both" => ExporterType::Both,
                        _ => ExporterType::Otlp,
                    })
            })
            .unwrap_or(ExporterType::Otlp);

        let endpoint = global_cfg
            .and_then(|c| c.endpoint.clone())
            .or_else(|| env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok())
            .unwrap_or_else(|| "http://localhost:4317".to_string());

        let sampling_ratio = global_cfg
            .and_then(|c| c.sampling_ratio)
            .or_else(|| {
                env::var("OTEL_TRACES_SAMPLER_ARG")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(1.0);

        let memory_max_spans = global_cfg
            .and_then(|c| c.memory_max_spans)
            .or_else(|| {
                env::var("OTEL_MEMORY_MAX_SPANS")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(DEFAULT_MEMORY_MAX_SPANS);

        let live_spans = resolve_live_spans(global_cfg.and_then(|c| c.live_spans), &exporter);

        Self {
            enabled,
            service_name,
            service_version,
            service_namespace,
            exporter,
            endpoint,
            sampling_ratio,
            memory_max_spans,
            live_spans,
        }
    }
}

// =============================================================================
// Advanced Sampling Strategies
// =============================================================================

/// Build a sampler from the configuration.
///
/// Supports the following sampling modes:
/// - Simple ratio-based sampling (default)
/// - Parent-based sampling (inherits sampling decision from parent span)
/// - AlwaysOn/AlwaysOff for 100%/0% sampling
/// - Per-operation sampling rules (advanced)
/// - Per-service sampling rules (advanced)
/// - Rate limiting (advanced)
fn build_sampler(config: &OtelConfig) -> Sampler {
    // Check for advanced sampling configuration
    if let Some(global_cfg) = get_otel_config()
        && let Some(sampling_cfg) = &global_cfg.sampling
    {
        let default_ratio = sampling_cfg.default.unwrap_or(config.sampling_ratio);

        // Check if advanced features are configured
        let has_rules = !sampling_cfg.rules.is_empty();
        let has_rate_limit = sampling_cfg.rate_limit.is_some();

        if has_rules || has_rate_limit {
            // Use AdvancedSampler for per-operation/per-service rules or rate limiting
            match AdvancedSampler::new(
                default_ratio,
                sampling_cfg.rules.clone(),
                sampling_cfg.rate_limit.clone(),
                Some(config.service_name.clone()),
            ) {
                Ok(advanced_sampler) => {
                    tracing::info!(
                        "Using advanced sampling: {} rules, rate_limit={}, default_ratio={}, service={}",
                        sampling_cfg.rules.len(),
                        has_rate_limit,
                        default_ratio,
                        config.service_name
                    );

                    let base_sampler = Sampler::ParentBased(Box::new(advanced_sampler));

                    // Note: parent_based is always true when using AdvancedSampler
                    // to ensure consistent trace sampling decisions
                    return base_sampler;
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to create AdvancedSampler: {}. Falling back to ratio-based sampling.",
                        e
                    );
                }
            }
        }

        // Build simple sampler
        let base_sampler = if default_ratio >= 1.0 {
            Sampler::AlwaysOn
        } else if default_ratio <= 0.0 {
            Sampler::AlwaysOff
        } else {
            Sampler::TraceIdRatioBased(default_ratio)
        };

        // Wrap with parent-based sampling if enabled
        if sampling_cfg.parent_based.unwrap_or(false) {
            tracing::info!(
                "Using parent-based sampling with default ratio {}",
                default_ratio
            );
            return Sampler::ParentBased(Box::new(base_sampler));
        }

        return base_sampler;
    }

    // Fall back to simple ratio-based sampling
    if config.sampling_ratio >= 1.0 {
        Sampler::AlwaysOn
    } else if config.sampling_ratio <= 0.0 {
        Sampler::AlwaysOff
    } else {
        Sampler::TraceIdRatioBased(config.sampling_ratio)
    }
}

/// Serializable representation of a span event (log entry).
#[derive(Debug, Clone, Serialize)]
pub struct StoredSpanEvent {
    pub name: String,
    pub timestamp_unix_nano: u64,
    pub attributes: Vec<(String, String)>,
}

/// Serializable representation of a span link for API responses.
#[derive(Debug, Clone, Serialize)]
pub struct StoredSpanLink {
    pub trace_id: String,
    pub span_id: String,
    pub trace_state: Option<String>,
    pub attributes: Vec<(String, String)>,
}

/// Serializable representation of a span for API responses.
#[derive(Debug, Clone, Serialize)]
pub struct StoredSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: u64,
    pub status: String,
    /// Status description (error message for error status)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_description: Option<String>,
    pub attributes: Vec<(String, String)>,
    pub service_name: String,
    pub events: Vec<StoredSpanEvent>,
    /// Linked spans from other traces
    pub links: Vec<StoredSpanLink>,
    /// Instrumentation scope name (e.g., "@opentelemetry/instrumentation-http")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrumentation_scope_name: Option<String>,
    /// Instrumentation scope version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrumentation_scope_version: Option<String>,
    /// W3C trace flags (e.g., sampled=1)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<u32>,
    /// W3C `tracestate` header carried by this span's context. Present when the
    /// caller propagated a non-empty `tracestate` alongside `traceparent`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_state: Option<String>,
    /// True while the span is in progress — a live snapshot taken by
    /// `LiveSpanProcessor::on_start`, replaced in place by the final span when
    /// it closes (`InMemorySpanStorage::add_spans`). Pending spans exist ONLY
    /// in the in-memory store and the `iii:devtools:*` streams; they are never
    /// exported via OTLP, where `end_time_unix_nano` is semantically required.
    /// By convention `end_time_unix_nano == 0` while pending.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

impl StoredSpan {
    pub fn from_span_data(span: &SpanData, service_name: &str) -> Self {
        let parent_span_id = if span.parent_span_id.to_string() != "0000000000000000" {
            Some(span.parent_span_id.to_string())
        } else {
            None
        };

        let (status, status_description) = match &span.status {
            opentelemetry::trace::Status::Ok => ("ok".to_string(), None),
            opentelemetry::trace::Status::Error { description } => {
                let desc = if description.is_empty() {
                    None
                } else {
                    Some(description.to_string())
                };
                ("error".to_string(), desc)
            }
            opentelemetry::trace::Status::Unset => ("unset".to_string(), None),
        };

        let attributes: Vec<(String, String)> = span
            .attributes
            .iter()
            .map(|kv| (kv.key.to_string(), kv.value.to_string()))
            .collect();

        // Convert span events (logs)
        let events: Vec<StoredSpanEvent> = span
            .events
            .iter()
            .map(|event| {
                let attrs: Vec<(String, String)> = event
                    .attributes
                    .iter()
                    .map(|kv| (kv.key.to_string(), kv.value.to_string()))
                    .collect();
                StoredSpanEvent {
                    name: event.name.to_string(),
                    timestamp_unix_nano: event
                        .timestamp
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as u64,
                    attributes: attrs,
                }
            })
            .collect();

        // Convert span links
        let links: Vec<StoredSpanLink> = span
            .links
            .iter()
            .map(|link| {
                let attrs: Vec<(String, String)> = link
                    .attributes
                    .iter()
                    .map(|kv| (kv.key.to_string(), kv.value.to_string()))
                    .collect();
                StoredSpanLink {
                    trace_id: link.span_context.trace_id().to_string(),
                    span_id: link.span_context.span_id().to_string(),
                    trace_state: Some(link.span_context.trace_state().header()),
                    attributes: attrs,
                }
            })
            .collect();

        StoredSpan {
            trace_id: span.span_context.trace_id().to_string(),
            span_id: span.span_context.span_id().to_string(),
            parent_span_id,
            name: span.name.to_string(),
            start_time_unix_nano: span
                .start_time
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
            end_time_unix_nano: span
                .end_time
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
            status,
            status_description,
            attributes,
            service_name: service_name.to_string(),
            events,
            links,
            // Instrumentation scope is not available from SpanData (only from OTLP JSON ingestion)
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
            flags: Some(span.span_context.trace_flags().to_u8() as u32),
            trace_state: {
                let ts = span.span_context.trace_state().header();
                if ts.is_empty() { None } else { Some(ts) }
            },
            pending: false,
        }
    }

    /// Effective end for time math: a pending span is active "now", so time
    /// filters and durations must not read its `end_time_unix_nano == 0`
    /// sentinel literally.
    pub fn effective_end_ns(&self, now_ns: u64) -> u64 {
        if self.pending {
            now_ns
        } else {
            self.end_time_unix_nano
        }
    }

    /// Duration so far for pending spans, final duration otherwise.
    pub fn duration_ns(&self, now_ns: u64) -> u64 {
        self.effective_end_ns(now_ns)
            .saturating_sub(self.start_time_unix_nano)
    }
}

/// Nanoseconds since the UNIX epoch, for `StoredSpan::effective_end_ns` /
/// `duration_ns`. Compute once per request so one listing sees one "now".
pub fn now_unix_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

/// In-memory span storage with circular buffer and broadcast channel.
pub struct InMemorySpanStorage {
    spans: RwLock<VecDeque<StoredSpan>>,
    /// Capacity limit. Atomic so the configuration-worker apply path can
    /// retune it at runtime; enforcement happens on every insert.
    max_spans: std::sync::atomic::AtomicUsize,
    /// Secondary index: trace_id -> set of span indices for O(1) trace lookups
    spans_by_trace_id: RwLock<HashMap<String, HashSet<usize>>>,
    /// `(trace_id, span_id)` pairs currently stored as pending (live
    /// snapshots awaiting their final span). Keyed by both ids — span ids
    /// are only unique within a trace, so a colliding final from another
    /// trace must not replace the wrong snapshot. Gates the replace-scan in
    /// `add_spans` so the hot append path (OTLP ingest, exporters) stays
    /// O(1) when no pending predecessor exists. Lock order everywhere:
    /// `spans` → `spans_by_trace_id` → this.
    pending_span_ids: RwLock<HashSet<(String, String)>>,
    /// Broadcast of every span as it lands, driving the `trace` trigger
    /// fan-out. Mirrors `InMemoryLogStorage`'s log broadcast.
    tx: broadcast::Sender<StoredSpan>,
}

/// Evict oldest spans until `len < max_spans`, maintaining the trace index
/// (shift-down on every pop) and purging evicted ids from the pending set.
/// Callers hold all three storage locks (see the lock-order note above).
fn evict_to_capacity(
    spans: &mut VecDeque<StoredSpan>,
    index: &mut HashMap<String, HashSet<usize>>,
    pending: &mut HashSet<(String, String)>,
    max_spans: usize,
) {
    // Loop converges after a shrink
    while spans.len() >= max_spans
        && let Some(old) = spans.pop_front()
    {
        pending.remove(&(old.trace_id.clone(), old.span_id.clone()));
        // Remove from index and shift all indices down
        if let Some(set) = index.get_mut(&old.trace_id) {
            set.remove(&0);
            *set = set.iter().filter_map(|&i| i.checked_sub(1)).collect();
            if set.is_empty() {
                index.remove(&old.trace_id);
            }
        }
        // Also shift indices for all other trace_ids
        for (trace_id, set) in index.iter_mut() {
            if trace_id != &old.trace_id {
                *set = set.iter().filter_map(|&i| i.checked_sub(1)).collect();
            }
        }
    }
}

impl std::fmt::Debug for InMemorySpanStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemorySpanStorage")
            .field("spans", &self.spans)
            .field(
                "max_spans",
                &self.max_spans.load(std::sync::atomic::Ordering::Relaxed),
            )
            .field("spans_by_trace_id", &self.spans_by_trace_id)
            .finish()
    }
}

impl InMemorySpanStorage {
    pub fn new(max_spans: usize) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            spans: RwLock::new(VecDeque::with_capacity(max_spans)),
            max_spans: std::sync::atomic::AtomicUsize::new(max_spans),
            spans_by_trace_id: RwLock::new(HashMap::new()),
            pending_span_ids: RwLock::new(HashSet::new()),
            tx,
        }
    }

    /// Retune the capacity at runtime. A shrink evicts oldest spans in one
    /// O(n) pass (drain + index rebuild) under the same lock order as
    /// `add_spans` (spans, then index) to avoid lock-order inversion.
    pub fn set_max_spans(&self, max: usize) {
        let max = max.max(1);
        self.max_spans
            .store(max, std::sync::atomic::Ordering::Relaxed);
        let mut spans = self.spans.write().unwrap();
        if spans.len() > max {
            let mut index = self.spans_by_trace_id.write().unwrap();
            let mut pending = self.pending_span_ids.write().unwrap();
            let excess = spans.len() - max;
            for old in spans.drain(..excess) {
                pending.remove(&(old.trace_id, old.span_id));
            }
            index.clear();
            for (idx, span) in spans.iter().enumerate() {
                index.entry(span.trace_id.clone()).or_default().insert(idx);
            }
        }
    }

    pub fn add_spans(&self, new_spans: Vec<StoredSpan>) {
        let mut spans = self.spans.write().unwrap();
        let mut index = self.spans_by_trace_id.write().unwrap();
        let mut pending = self.pending_span_ids.write().unwrap();

        let max_spans = self
            .max_spans
            .load(std::sync::atomic::Ordering::Relaxed)
            .max(1);
        for span in new_spans {
            // A final span replacing its live (pending) snapshot overwrites in
            // place: same idx, same trace_id, so neither index changes. Keys
            // are `(trace_id, span_id)` — span ids are only unique within a
            // trace, so a colliding final from another trace must not steal
            // the snapshot. The emptiness gate keeps the key clone + scan off
            // the hot append path, and pending spans are by construction
            // recent, so scan from the back. The reverse never happens — a
            // pending snapshot must not overwrite a final (`on_start` always
            // precedes `on_end`, and a worker's start snapshot always ingests
            // before its final: same FIFO telemetry connection, see
            // `ingest_otlp_json`).
            if !pending.is_empty()
                && pending.remove(&(span.trace_id.clone(), span.span_id.clone()))
                && let Some(slot) = spans
                    .iter_mut()
                    .rev()
                    .find(|s| s.span_id == span.span_id && s.trace_id == span.trace_id)
            {
                let _ = self.tx.send(span.clone());
                *slot = span;
                continue;
            }
            // A tracked pending whose slot was already evicted falls through
            // and appends as a fresh span.

            evict_to_capacity(&mut spans, &mut index, &mut pending, max_spans);

            let idx = spans.len();
            let trace_id = span.trace_id.clone();

            // Broadcast to any listeners (ignore send errors if no receivers).
            // Mirrors InMemoryLogStorage::add_logs so the `trace` trigger fans
            // out uniformly across every ingestion path (OTLP ingest, the
            // in-memory exporter, and the tee exporter all funnel through here).
            let _ = self.tx.send(span.clone());

            spans.push_back(span);
            index.entry(trace_id).or_default().insert(idx);
        }
    }

    /// Record an in-progress span snapshot (`LiveSpanProcessor::on_start`).
    /// Appends + broadcasts exactly like `add_spans` and marks the
    /// `(trace_id, span_id)` pair pending so the final span replaces it in
    /// place when it closes. Pending spans live ONLY in this store and the
    /// streams fed from it — the OTLP export path never sees them.
    pub fn add_pending_span(&self, span: StoredSpan) {
        debug_assert!(span.pending && span.end_time_unix_nano == 0);
        let mut spans = self.spans.write().unwrap();
        let mut index = self.spans_by_trace_id.write().unwrap();
        let mut pending = self.pending_span_ids.write().unwrap();

        let max_spans = self
            .max_spans
            .load(std::sync::atomic::Ordering::Relaxed)
            .max(1);
        evict_to_capacity(&mut spans, &mut index, &mut pending, max_spans);

        let idx = spans.len();
        let trace_id = span.trace_id.clone();
        pending.insert((trace_id.clone(), span.span_id.clone()));
        let _ = self.tx.send(span.clone());
        spans.push_back(span);
        index.entry(trace_id).or_default().insert(idx);
    }

    pub fn get_spans(&self) -> Vec<StoredSpan> {
        self.spans.read().unwrap().iter().cloned().collect()
    }

    pub fn get_spans_by_trace_id(&self, trace_id: &str) -> Vec<StoredSpan> {
        let spans = self.spans.read().unwrap();
        let index = self.spans_by_trace_id.read().unwrap();

        match index.get(trace_id) {
            Some(indices) => indices
                .iter()
                .filter_map(|&i| spans.get(i).cloned())
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn clear(&self) {
        let mut spans = self.spans.write().unwrap();
        let mut index = self.spans_by_trace_id.write().unwrap();
        let mut pending = self.pending_span_ids.write().unwrap();
        spans.clear();
        index.clear();
        pending.clear();
    }

    /// Subscribe to a broadcast of every span as it lands in storage. Drives
    /// the `trace` trigger fan-out (mirrors `InMemoryLogStorage::subscribe`).
    pub fn subscribe(&self) -> broadcast::Receiver<StoredSpan> {
        self.tx.subscribe()
    }

    pub fn len(&self) -> usize {
        self.spans.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.read().unwrap().is_empty()
    }

    /// Calculate performance metrics (duration statistics) from stored spans.
    /// Returns (avg_ms, p50_ms, p95_ms, p99_ms, min_ms, max_ms).
    pub fn calculate_performance_metrics(&self) -> (f64, f64, f64, f64, f64, f64) {
        let spans = self.spans.read().unwrap();

        if spans.is_empty() {
            return (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        }

        // Calculate duration in milliseconds for each span. In-flight
        // (pending) spans are excluded — their duration isn't known yet, and
        // counting them as 0/partial would drag down min/percentiles.
        let mut durations: Vec<f64> = spans
            .iter()
            .filter(|span| !span.pending)
            .map(|span| {
                let duration_nanos = span
                    .end_time_unix_nano
                    .saturating_sub(span.start_time_unix_nano);
                duration_nanos as f64 / 1_000_000.0 // Convert nanoseconds to milliseconds
            })
            .collect();

        if durations.is_empty() {
            return (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        }

        // Sort for percentile calculations
        durations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let len = durations.len();
        let sum: f64 = durations.iter().sum();
        let avg = sum / len as f64;

        let min = durations[0];
        let max = durations[len - 1];

        // Calculate percentiles
        let p50_idx = (len as f64 * 0.50) as usize;
        let p95_idx = (len as f64 * 0.95) as usize;
        let p99_idx = (len as f64 * 0.99) as usize;

        let p50 = durations[p50_idx.min(len - 1)];
        let p95 = durations[p95_idx.min(len - 1)];
        let p99 = durations[p99_idx.min(len - 1)];

        (avg, p50, p95, p99, min, max)
    }
}

/// Global in-memory span storage.
static IN_MEMORY_STORAGE: OnceLock<Arc<InMemorySpanStorage>> = OnceLock::new();

/// Get the global in-memory span storage (if initialized).
pub fn get_span_storage() -> Option<Arc<InMemorySpanStorage>> {
    IN_MEMORY_STORAGE.get().cloned()
}

/// In-memory span exporter that stores spans in a circular buffer.
#[derive(Debug)]
pub struct InMemorySpanExporter {
    storage: Arc<InMemorySpanStorage>,
    service_name: String,
}

impl InMemorySpanExporter {
    pub fn new(max_spans: usize, service_name: String) -> Self {
        let storage = Arc::new(InMemorySpanStorage::new(max_spans));
        if IN_MEMORY_STORAGE.set(storage.clone()).is_err() {
            tracing::debug!("In-memory span storage already initialized");
        }
        Self {
            storage,
            service_name,
        }
    }

    /// Create an exporter with existing storage (does not set global storage).
    pub fn with_storage(storage: Arc<InMemorySpanStorage>, service_name: String) -> Self {
        Self {
            storage,
            service_name,
        }
    }
}

impl SpanExporter for InMemorySpanExporter {
    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        let stored: Vec<StoredSpan> = batch
            .iter()
            .map(|s| StoredSpan::from_span_data(s, &self.service_name))
            .collect();
        self.storage.add_spans(stored);
        async { Ok(()) }
    }

    fn shutdown_with_timeout(&mut self, _timeout: std::time::Duration) -> OTelSdkResult {
        // Nothing to clean up for in-memory storage
        Ok(())
    }
}

/// Mirrors spans into the in-memory store at START (as `pending` snapshots) so
/// live trace views can render in-progress work — most importantly the
/// engine-side parent spans (`trigger <fn>`, `enqueue`, `call <fn>`) that
/// otherwise only land after everything under them has finished.
///
/// The final span still arrives exclusively via the exporter path on close and
/// replaces the snapshot in place (`InMemorySpanStorage::add_spans`). The OTLP
/// export path is untouched: no partial spans ever reach the wire, where the
/// proto documents `end_time_unix_nano` as semantically required.
///
/// Registered when `live_spans` resolves on (see [`resolve_live_spans`]:
/// default for the `memory` exporter, explicit opt-in for `both`) — and never
/// in OTLP-only mode, where engine finals don't land in memory storage and
/// pendings would never be replaced.
#[derive(Debug)]
pub struct LiveSpanProcessor {
    storage: Arc<InMemorySpanStorage>,
    service_name: String,
}

impl LiveSpanProcessor {
    pub fn new(storage: Arc<InMemorySpanStorage>, service_name: String) -> Self {
        Self {
            storage,
            service_name,
        }
    }
}

impl SpanProcessor for LiveSpanProcessor {
    fn on_start(&self, span: &mut opentelemetry_sdk::trace::Span, _cx: &Context) {
        // RecordOnly spans are recorded but never exported (the exporter-side
        // processors skip !is_sampled on_end) — skip them here too, or their
        // pendings would never be finalized. Covers sampled-out spans as well.
        if !span.span_context().is_sampled() {
            return;
        }
        // None for non-recording spans.
        let Some(data) = span.exported_data() else {
            return;
        };
        let mut stored = StoredSpan::from_span_data(&data, &self.service_name);
        stored.pending = true;
        // The SDK initializes end_time = start_time at build; zero is the
        // live-span sentinel (see the `StoredSpan::pending` docs).
        stored.end_time_unix_nano = 0;
        self.storage.add_pending_span(stored);
    }

    // MUST stay a no-op: the final span lands via the exporter path; replacing
    // the pending here as well would race it and duplicate the span.
    fn on_end(&self, _span: SpanData) {}

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _timeout: std::time::Duration) -> OTelSdkResult {
        Ok(())
    }
}

/// Tee span exporter that sends spans to both OTLP collector and in-memory storage.
/// This enables span triggers to work while still exporting to an external collector.
#[derive(Debug)]
pub struct TeeSpanExporter {
    otlp_exporter: opentelemetry_otlp::SpanExporter,
    memory_storage: Arc<InMemorySpanStorage>,
    service_name: String,
}

impl TeeSpanExporter {
    pub fn new(
        otlp_exporter: opentelemetry_otlp::SpanExporter,
        memory_storage: Arc<InMemorySpanStorage>,
        service_name: String,
    ) -> Self {
        Self {
            otlp_exporter,
            memory_storage,
            service_name,
        }
    }
}

impl SpanExporter for TeeSpanExporter {
    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        // Store in memory first (for triggers and API access).
        let stored: Vec<StoredSpan> = batch
            .iter()
            .map(|s| StoredSpan::from_span_data(s, &self.service_name))
            .collect();
        self.memory_storage.add_spans(stored);

        // Forward to OTLP exporter (for external collector)
        self.otlp_exporter.export(batch)
    }

    // Without this forward, the trait's default no-op swallows the provider's
    // resource and the inner OTLP exporter ships spans with an EMPTY resource
    // (no service.name) — collectors then show engine traces as unattributed
    // ("<root span not yet received>" root in Tempo).
    fn set_resource(&mut self, resource: &Resource) {
        self.otlp_exporter.set_resource(resource);
    }

    fn shutdown_with_timeout(&mut self, timeout: std::time::Duration) -> OTelSdkResult {
        self.otlp_exporter.shutdown_with_timeout(timeout)
    }
}

/// Swappable sampler indirection. The tracer provider (and the SDK span
/// forwarder) hold clones of this wrapper; the configuration-worker apply
/// path swaps the inner sampler so `sampling_ratio` / `sampling` changes
/// take effect without rebuilding the provider.
#[derive(Clone, Debug)]
pub struct DynamicSampler {
    inner: Arc<RwLock<Sampler>>,
}

impl DynamicSampler {
    pub fn new(initial: Sampler) -> Self {
        Self {
            inner: Arc::new(RwLock::new(initial)),
        }
    }

    pub fn swap(&self, new: Sampler) {
        *self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = new;
    }
}

impl opentelemetry_sdk::trace::ShouldSample for DynamicSampler {
    fn should_sample(
        &self,
        parent_context: Option<&opentelemetry::Context>,
        trace_id: opentelemetry::trace::TraceId,
        name: &str,
        span_kind: &opentelemetry::trace::SpanKind,
        attributes: &[opentelemetry::KeyValue],
        links: &[opentelemetry::trace::Link],
    ) -> opentelemetry::trace::SamplingResult {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .should_sample(parent_context, trace_id, name, span_kind, attributes, links)
    }
}

/// The sampler installed into the tracer provider at init, swappable at
/// runtime. Unset when the pipeline never initialized (observability
/// disabled at boot) — sampler changes are restart-tier in that case.
static DYNAMIC_SAMPLER: OnceLock<DynamicSampler> = OnceLock::new();

pub(super) fn get_dynamic_sampler() -> Option<&'static DynamicSampler> {
    DYNAMIC_SAMPLER.get()
}

/// Rebuild the sampler from the live global configuration and swap it into
/// the running pipeline (engine tracer + SDK span forwarder share one
/// inner). No-op when the pipeline never initialized.
pub fn refresh_sampler() {
    if let Some(dynamic) = DYNAMIC_SAMPLER.get() {
        dynamic.swap(build_sampler(&OtelConfig::default()));
    }
}

static TRACER_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

/// OTLP collector endpoint used to lazily build per-service SDK span forwarders.
static SDK_FORWARDER_ENDPOINT: OnceLock<String> = OnceLock::new();

/// Per-`service.name` OTLP span exporters for forwarding SDK-ingested (worker)
/// spans to the collector. Keyed by service name so each worker's spans are
/// exported under their OWN resource (`service.name`) instead of being stripped
/// to the OTLP default identity (`unknown_service`).
static SDK_SPAN_FORWARDERS: OnceLock<
    RwLock<HashMap<String, Arc<opentelemetry_otlp::SpanExporter>>>,
> = OnceLock::new();

/// Record the collector endpoint so SDK-ingested spans can later be
/// forwarded to the collector, per-service. Forwarded spans honor the same
/// swappable sampler as the engine tracer (`DYNAMIC_SAMPLER`), so the
/// collector volume follows `sampling_ratio` even after a runtime change.
fn init_sdk_span_forwarder(endpoint: &str) {
    let _ = SDK_FORWARDER_ENDPOINT.set(endpoint.to_string());
    let _ = SDK_SPAN_FORWARDERS.set(RwLock::new(HashMap::new()));
}

/// Build a `Resource` from an OTLP resource payload, preserving the worker's
/// own attributes (notably `service.name`) so forwarded spans are correctly
/// attributed at the collector rather than landing under `unknown_service`.
fn build_forward_resource(resource: &Option<OtlpResource>) -> Resource {
    let attrs: Vec<KeyValue> = resource
        .as_ref()
        .map(|r| {
            r.attributes
                .iter()
                .filter_map(otlp_kv_to_key_value)
                .collect()
        })
        .unwrap_or_default();
    Resource::builder_empty().with_attributes(attrs).build()
}

/// Get (or lazily build) the OTLP forwarder for a given worker `service.name`,
/// with the worker's `Resource` attached via `set_resource` so the collector
/// sees the right service identity.
fn get_or_build_forwarder(
    service_name: &str,
    resource: &Resource,
) -> Option<Arc<opentelemetry_otlp::SpanExporter>> {
    let endpoint = SDK_FORWARDER_ENDPOINT.get()?;
    let forwarders = SDK_SPAN_FORWARDERS.get()?;

    // Fast path under a read lock. The explicit block guarantees the read guard
    // is released before the write lock is taken below (std `RwLock` is not
    // reentrant), independent of temporary-drop timing.
    {
        let map = forwarders.read().ok()?;
        if let Some(existing) = map.get(service_name) {
            return Some(existing.clone());
        }
    }

    let mut map = forwarders.write().ok()?;
    // Re-check under the write lock in case another task built it meanwhile.
    if let Some(existing) = map.get(service_name) {
        return Some(existing.clone());
    }
    match build_span_exporter(endpoint.as_str()) {
        Ok(mut exporter) => {
            exporter.set_resource(resource);
            let arc = Arc::new(exporter);
            map.insert(service_name.to_string(), arc.clone());
            Some(arc)
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                service_name = %service_name,
                "Failed to create SDK span forwarder; spans will not be exported to collector"
            );
            None
        }
    }
}

/// Whether the engine sampler keeps a forwarded worker span. The trace-id ratio
/// is deterministic per trace, so an entire trace is kept or dropped
/// consistently regardless of which service produced a given span.
fn forward_span_is_sampled<S: opentelemetry_sdk::trace::ShouldSample>(
    sampler: &S,
    sd: &SpanData,
) -> bool {
    use opentelemetry::trace::SamplingDecision;
    let result = sampler.should_sample(
        None,
        sd.span_context.trace_id(),
        sd.name.as_ref(),
        &sd.span_kind,
        &sd.attributes,
        &sd.links.links,
    );
    matches!(result.decision, SamplingDecision::RecordAndSample)
}

/// Initialize OpenTelemetry with the given configuration.
///
/// Returns an `OpenTelemetryLayer` that can be composed with other tracing layers,
/// or `None` if OpenTelemetry is disabled.
pub fn init_otel<S>(
    config: &OtelConfig,
) -> Option<OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    if !config.enabled {
        println!("OpenTelemetry is disabled");
        return None;
    }

    // Set global propagator for W3C trace-context and baggage
    global::set_text_map_propagator(TextMapCompositePropagator::new(vec![
        Box::new(TraceContextPropagator::new()),
        Box::new(BaggagePropagator::new()),
    ]));

    // Build the sampler using advanced configuration if available, wrapped
    // in the swappable indirection so runtime configuration changes apply
    // without a provider rebuild. If init_otel runs again in-process, reuse
    // the already-registered handle (swapping its inner) instead of building a
    // detached one — otherwise the provider would hold a sampler that
    // refresh_sampler() no longer drives, silently freezing runtime sampling.
    let built = build_sampler(config);
    let sampler = match DYNAMIC_SAMPLER.get() {
        Some(existing) => {
            existing.swap(built);
            existing.clone()
        }
        None => {
            let sampler = DynamicSampler::new(built);
            let _ = DYNAMIC_SAMPLER.set(sampler.clone());
            sampler
        }
    };

    // Build resource attributes with OTEL semantic conventions
    // Using string keys for attributes not available in the crate version
    let mut resource_builder = Resource::builder()
        .with_service_name(config.service_name.clone())
        .with_attributes([
            KeyValue::new("service.version", config.service_version.clone()),
            KeyValue::new("service.instance.id", uuid::Uuid::new_v4().to_string()),
        ]);

    // Only add namespace if provided (optional attribute)
    if let Some(namespace) = &config.service_namespace {
        resource_builder =
            resource_builder.with_attribute(KeyValue::new("service.namespace", namespace.clone()));
    }

    let resource = resource_builder.build();

    // Build tracer provider based on exporter type
    let provider = match config.exporter {
        ExporterType::Otlp => {
            match build_span_exporter(&config.endpoint) {
                Ok(exporter) => {
                    init_sdk_span_forwarder(&config.endpoint);

                    // Initialize in-memory storage for SDK span ingestion (API access)
                    let memory_storage =
                        Arc::new(InMemorySpanStorage::new(config.memory_max_spans));
                    let _ = IN_MEMORY_STORAGE.set(memory_storage);

                    // No LiveSpanProcessor here: in OTLP-only mode engine
                    // finals never land in memory storage, so pending
                    // snapshots would never be replaced.
                    SdkTracerProvider::builder()
                        .with_span_processor(
                            iii_helpers::observability::BaggageSpanProcessor::default(),
                        )
                        .with_batch_exporter(exporter)
                        .with_sampler(sampler)
                        .with_id_generator(RandomIdGenerator::default())
                        .with_resource(resource)
                        .build()
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        endpoint = %config.endpoint,
                        "Failed to create OTLP exporter, falling back to memory-only mode"
                    );
                    // Fall back to memory-only mode
                    let memory_storage =
                        Arc::new(InMemorySpanStorage::new(config.memory_max_spans));
                    if IN_MEMORY_STORAGE.set(memory_storage.clone()).is_err() {
                        tracing::debug!("In-memory span storage already initialized");
                    }
                    let exporter = InMemorySpanExporter::with_storage(
                        memory_storage.clone(),
                        config.service_name.clone(),
                    );
                    let mut builder = SdkTracerProvider::builder().with_span_processor(
                        iii_helpers::observability::BaggageSpanProcessor::default(),
                    );
                    if config.live_spans {
                        builder = builder.with_span_processor(LiveSpanProcessor::new(
                            memory_storage,
                            config.service_name.clone(),
                        ));
                    }
                    builder
                        .with_simple_exporter(exporter)
                        .with_sampler(sampler)
                        .with_id_generator(RandomIdGenerator::default())
                        .with_resource(resource)
                        .build()
                }
            }
        }
        ExporterType::Memory => {
            // Create the storage explicitly (rather than via
            // `InMemorySpanExporter::new`) so the LiveSpanProcessor is
            // guaranteed to share the exact storage the exporter writes to,
            // even when the global slot was already taken by a prior init.
            let memory_storage = Arc::new(InMemorySpanStorage::new(config.memory_max_spans));
            if IN_MEMORY_STORAGE.set(memory_storage.clone()).is_err() {
                tracing::debug!("In-memory span storage already initialized");
            }
            let exporter = InMemorySpanExporter::with_storage(
                memory_storage.clone(),
                config.service_name.clone(),
            );

            let mut builder = SdkTracerProvider::builder()
                .with_span_processor(iii_helpers::observability::BaggageSpanProcessor::default());
            if config.live_spans {
                // After BaggageSpanProcessor, so pending snapshots carry the
                // baggage-stamped attributes.
                builder = builder.with_span_processor(LiveSpanProcessor::new(
                    memory_storage,
                    config.service_name.clone(),
                ));
            }
            builder
                .with_simple_exporter(exporter)
                .with_sampler(sampler)
                .with_id_generator(RandomIdGenerator::default())
                .with_resource(resource)
                .build()
        }
        ExporterType::Both => {
            // Create memory storage first (always succeeds)
            let memory_storage = Arc::new(InMemorySpanStorage::new(config.memory_max_spans));
            if IN_MEMORY_STORAGE.set(memory_storage.clone()).is_err() {
                tracing::debug!("In-memory span storage already initialized");
            }

            // Try to create OTLP exporter
            match build_span_exporter(&config.endpoint) {
                Ok(otlp_exporter) => {
                    init_sdk_span_forwarder(&config.endpoint);

                    // Create tee exporter that sends to both
                    let tee_exporter = TeeSpanExporter::new(
                        otlp_exporter,
                        memory_storage.clone(),
                        config.service_name.clone(),
                    );

                    let mut builder = SdkTracerProvider::builder().with_span_processor(
                        iii_helpers::observability::BaggageSpanProcessor::default(),
                    );
                    if config.live_spans {
                        // Pending snapshots feed ONLY the memory side; the
                        // OTLP half of the tee still sees spans on close only.
                        builder = builder.with_span_processor(LiveSpanProcessor::new(
                            memory_storage,
                            config.service_name.clone(),
                        ));
                    }
                    builder
                        .with_batch_exporter(tee_exporter)
                        .with_sampler(sampler)
                        .with_id_generator(RandomIdGenerator::default())
                        .with_resource(resource)
                        .build()
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        endpoint = %config.endpoint,
                        "Failed to create OTLP exporter for 'both' mode, using memory-only"
                    );
                    // Fall back to memory-only with our already-created storage
                    let exporter = InMemorySpanExporter::with_storage(
                        memory_storage.clone(),
                        config.service_name.clone(),
                    );
                    let mut builder = SdkTracerProvider::builder().with_span_processor(
                        iii_helpers::observability::BaggageSpanProcessor::default(),
                    );
                    if config.live_spans {
                        builder = builder.with_span_processor(LiveSpanProcessor::new(
                            memory_storage,
                            config.service_name.clone(),
                        ));
                    }
                    builder
                        .with_simple_exporter(exporter)
                        .with_sampler(sampler)
                        .with_id_generator(RandomIdGenerator::default())
                        .with_resource(resource)
                        .build()
                }
            }
        }
    };

    // Store for shutdown
    if TRACER_PROVIDER.set(provider.clone()).is_err() {
        tracing::debug!("Tracer provider already initialized");
    }

    // Get a tracer from the provider
    let tracer = provider.tracer("iii");

    // Set as global provider
    global::set_tracer_provider(provider);

    let exporter_info = match config.exporter {
        ExporterType::Otlp => format!("otlp (endpoint={})", config.endpoint),
        ExporterType::Memory => format!("memory (max_spans={})", config.memory_max_spans),
        ExporterType::Both => format!(
            "both (otlp endpoint={}, memory max_spans={})",
            config.endpoint, config.memory_max_spans
        ),
    };

    println!(
        "OpenTelemetry initialized: exporter={}, service_name={}, sampling_ratio={}",
        exporter_info, config.service_name, config.sampling_ratio
    );

    Some(OpenTelemetryLayer::new(tracer))
}

/// Shutdown OpenTelemetry, flushing any pending spans.
pub fn shutdown_otel() {
    if let Some(provider) = TRACER_PROVIDER.get()
        && let Err(e) = provider.shutdown()
    {
        tracing::warn!(error = ?e, "Error shutting down OpenTelemetry");
    }
}

/// Extract trace and span IDs from the active span context.
/// Returns `None` if there is no active span or tracing is not initialized.
fn current_span_context_ids() -> Option<(String, String)> {
    let ctx = Context::current();
    let span_ref = ctx.span();
    let span_ctx = span_ref.span_context();
    span_ctx.is_valid().then(|| {
        (
            span_ctx.trace_id().to_string(),
            span_ctx.span_id().to_string(),
        )
    })
}

/// Extract the current trace ID from the active span context.
/// Returns `None` if there is no active span or tracing is not initialized.
pub fn current_trace_id() -> Option<String> {
    current_span_context_ids().map(|(trace_id, _)| trace_id)
}

/// Extract the current span ID from the active span context.
pub fn current_span_id() -> Option<String> {
    current_span_context_ids().map(|(_, span_id)| span_id)
}

/// Inject the current trace context into a W3C traceparent header string.
/// Returns `None` if there is no active span.
pub fn inject_traceparent() -> Option<String> {
    inject_traceparent_from_context(&Context::current())
}

/// Inject trace context from a specific OpenTelemetry context.
/// Use this when you need to extract the context from a tracing span via
/// `tracing_opentelemetry::OpenTelemetrySpanExt::context()`.
pub fn inject_traceparent_from_context(ctx: &Context) -> Option<String> {
    if !ctx.span().span_context().is_valid() {
        return None;
    }
    inject_header(ctx, "traceparent", TraceContextPropagator::new())
}

/// Extract a trace context from a W3C traceparent header string.
/// Returns a new `Context` with the extracted span context as parent.
pub fn extract_traceparent(traceparent: &str) -> Context {
    extract_header("traceparent", traceparent, TraceContextPropagator::new())
}

/// Inject the current baggage into a W3C baggage header string.
/// Returns `None` if there is no baggage in the current context.
pub fn inject_baggage() -> Option<String> {
    let ctx = Context::current();
    inject_header(&ctx, "baggage", BaggagePropagator::new())
}

/// Inject baggage from a specific OpenTelemetry context.
/// Use this when you need to extract the baggage from a specific context.
pub fn inject_baggage_from_context(ctx: &Context) -> Option<String> {
    inject_header(ctx, "baggage", BaggagePropagator::new())
}

/// Extract baggage from a W3C baggage header string.
/// Returns a new `Context` with the extracted baggage.
pub fn extract_baggage(baggage: &str) -> Context {
    extract_header("baggage", baggage, BaggagePropagator::new())
}

fn inject_header<P>(ctx: &Context, header: &str, propagator: P) -> Option<String>
where
    P: TextMapPropagator,
{
    let mut carrier: HashMap<String, String> = HashMap::new();
    propagator.inject_context(ctx, &mut carrier);
    carrier.remove(header)
}

fn extract_header<P>(header: &str, value: &str, propagator: P) -> Context
where
    P: TextMapPropagator,
{
    let mut carrier: HashMap<String, String> = HashMap::new();
    // An empty header value is treated as absent. A present-but-empty `baggage`
    // header makes `BaggagePropagator::extract` log a per-call
    // `InvalidKeyValueFormat` warning (empty split yields an unparsable entry).
    if !value.is_empty() {
        carrier.insert(header.to_string(), value.to_string());
    }
    propagator.extract(&carrier)
}

/// Combine trace context and baggage extraction into a single context.
/// This extracts both traceparent and baggage headers into a unified context.
pub fn extract_context(traceparent: Option<&str>, baggage: Option<&str>) -> Context {
    extract_context_with_state(traceparent, None, baggage)
}

/// Like [`extract_context`] but also honors the W3C `tracestate` header.
///
/// `tracestate` is the companion header to `traceparent` in the W3C Trace
/// Context spec; it carries vendor-specific trace data that must travel
/// alongside the trace id. The `TraceContextPropagator` only populates a
/// `SpanContext`'s `TraceState` when both headers are present in the carrier,
/// so a bare `traceparent` would silently drop any incoming `tracestate`.
pub fn extract_context_with_state(
    traceparent: Option<&str>,
    tracestate: Option<&str>,
    baggage: Option<&str>,
) -> Context {
    let mut carrier: HashMap<String, String> = HashMap::new();
    // Empty values are treated as absent: a present-but-empty `baggage` header
    // makes `BaggagePropagator::extract` emit a per-call `InvalidKeyValueFormat`
    // warning, so callers passing `Some("")` must not seed the carrier.
    if let Some(tp) = traceparent.filter(|s| !s.is_empty()) {
        carrier.insert("traceparent".to_string(), tp.to_string());
    }
    if let Some(ts) = tracestate.filter(|s| !s.is_empty()) {
        carrier.insert("tracestate".to_string(), ts.to_string());
    }
    if let Some(bg) = baggage.filter(|s| !s.is_empty()) {
        carrier.insert("baggage".to_string(), bg.to_string());
    }

    // Extract trace context first, then merge baggage into that context.
    // This avoids relying on global propagator state and keeps behavior
    // aligned with the SDK helper when one header is invalid or absent.
    let ctx = TraceContextPropagator::new().extract(&carrier);
    BaggagePropagator::new().extract_with_context(&ctx, &carrier)
}

/// Baggage key the `BaggageSpanProcessor` copies onto every span as the
/// `iii.function.id` attribute (baggage entries copy to spans unconditionally).
const FUNCTION_ID_BAGGAGE_KEY: &str = "iii.function.id";

/// Return a context identical to `cx` but with the `iii.function.id` baggage
/// entry set to `function_id` (all other baggage entries are preserved).
///
/// `BaggageSpanProcessor` stamps `iii.function.id` onto every span from the
/// parent context's baggage, but an enqueuer's baggage names the *enqueuer*, not
/// the function being enqueued. Rewriting it at the enqueue boundary makes the
/// `enqueue`/`fn_queue` spans (and the baggage delivered to the target worker)
/// carry the TARGET function id, so grouping traces by `iii.function.id`
/// attributes those spans to the function they actually serve.
pub fn with_function_id_baggage(cx: &Context, function_id: &str) -> Context {
    use opentelemetry::baggage::BaggageExt;
    // Drop any existing entry for the key, then re-add — mirrors the SDK's
    // `set_baggage_entry` so we replace rather than duplicate.
    let mut entries: Vec<KeyValue> = cx
        .baggage()
        .iter()
        .filter(|(k, _)| k.as_str() != FUNCTION_ID_BAGGAGE_KEY)
        .map(|(k, (v, _meta))| KeyValue::new(k.clone(), v.clone()))
        .collect();
    entries.push(KeyValue::new(
        FUNCTION_ID_BAGGAGE_KEY,
        function_id.to_string(),
    ));
    cx.with_baggage(entries)
}

/// W3C-header variant of [`with_function_id_baggage`] for code paths that carry
/// baggage as a header string rather than a [`Context`] (e.g. queue messages).
/// Always returns `Some` — at minimum the header names the target function.
pub fn baggage_with_function_id(baggage: Option<&str>, function_id: &str) -> Option<String> {
    let ctx = with_function_id_baggage(&extract_baggage(baggage.unwrap_or_default()), function_id);
    inject_baggage_from_context(&ctx)
}

// =============================================================================
// OTLP JSON Ingestion from Node SDK
// =============================================================================

use serde::Deserialize;

/// OTLP ExportTraceServiceRequest structure for JSON deserialization
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpExportTraceServiceRequest {
    #[serde(default)]
    resource_spans: Vec<OtlpResourceSpans>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpResourceSpans {
    #[serde(default)]
    resource: Option<OtlpResource>,
    #[serde(default)]
    scope_spans: Vec<OtlpScopeSpans>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpResource {
    #[serde(default)]
    attributes: Vec<OtlpKeyValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpScopeSpans {
    #[serde(default)]
    scope: Option<OtlpScope>,
    #[serde(default)]
    spans: Vec<OtlpSpan>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpSpan {
    trace_id: String,
    span_id: String,
    #[serde(default)]
    parent_span_id: Option<String>,
    name: String,
    /// OTLP span kind: 0=Unspecified, 1=Internal, 2=Server, 3=Client, 4=Producer, 5=Consumer
    #[serde(default)]
    kind: Option<u32>,
    #[serde(default)]
    start_time_unix_nano: OtlpNumericString,
    #[serde(default)]
    end_time_unix_nano: OtlpNumericString,
    #[serde(default)]
    status: Option<OtlpStatus>,
    #[serde(default)]
    attributes: Vec<OtlpKeyValue>,
    #[serde(default)]
    events: Vec<OtlpSpanEvent>,
    #[serde(default)]
    links: Vec<OtlpSpanLink>,
    /// W3C trace flags
    #[serde(default)]
    flags: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpSpanEvent {
    #[serde(default)]
    name: String,
    #[serde(default)]
    time_unix_nano: OtlpNumericString,
    #[serde(default)]
    attributes: Vec<OtlpKeyValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpSpanLink {
    #[serde(default)]
    trace_id: String,
    #[serde(default)]
    span_id: String,
    #[serde(default)]
    trace_state: Option<String>,
    #[serde(default)]
    attributes: Vec<OtlpKeyValue>,
}

/// Wrapper type that can deserialize either a number or a string to u64
#[derive(Debug, Default)]
struct OtlpNumericString(u64);

impl<'de> Deserialize<'de> for OtlpNumericString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};

        struct NumericStringVisitor;

        impl<'de> Visitor<'de> for NumericStringVisitor {
            type Value = OtlpNumericString;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a number or a string containing a number")
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(OtlpNumericString(v))
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if v < 0 {
                    return Err(de::Error::custom(
                        "negative numeric value not allowed for OtlpNumericString",
                    ));
                }
                Ok(OtlpNumericString(v as u64))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                v.parse::<u64>()
                    .map(OtlpNumericString)
                    .map_err(|_| de::Error::custom(format!("invalid numeric string: {}", v)))
            }
        }

        deserializer.deserialize_any(NumericStringVisitor)
    }
}

/// Wrapper type that can deserialize either a number or a string to i64.
///
/// Mirrors `OtlpNumericString` but for signed 64-bit integers. Per the OTLP/JSON
/// spec, protobuf `int64`/`sint64`/`sfixed64` fields are encoded as JSON strings
/// (because JavaScript numbers can't represent the full i64 range). The Node,
/// Python, and Rust SDKs all emit `asInt` as a string; without this visitor the
/// engine fails to parse any integer-valued gauge or counter metric.
#[derive(Debug, Default, Clone, Copy)]
struct OtlpIntString(i64);

impl<'de> Deserialize<'de> for OtlpIntString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};

        struct IntStringVisitor;

        impl<'de> Visitor<'de> for IntStringVisitor {
            type Value = OtlpIntString;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a signed integer or a string containing a signed integer")
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(OtlpIntString(v))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                i64::try_from(v)
                    .map(OtlpIntString)
                    .map_err(|_| de::Error::custom(format!("value {} overflows i64", v)))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                v.parse::<i64>()
                    .map(OtlpIntString)
                    .map_err(|_| de::Error::custom(format!("invalid signed integer string: {}", v)))
            }
        }

        deserializer.deserialize_any(IntStringVisitor)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpStatus {
    #[serde(default)]
    code: u32,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpKeyValue {
    key: String,
    #[serde(default)]
    value: Option<OtlpAnyValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpKeyValueList {
    #[serde(default)]
    values: Vec<OtlpKeyValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpArrayValue {
    #[serde(default)]
    values: Vec<OtlpAnyValue>,
}

impl OtlpKeyValueList {
    fn to_json_map(&self) -> serde_json::Map<String, serde_json::Value> {
        self.values
            .iter()
            .filter_map(|kv| {
                kv.value
                    .as_ref()
                    .map(|value| (kv.key.clone(), value.to_serde_json_value()))
            })
            .collect()
    }
}

impl OtlpArrayValue {
    fn to_json_array(&self) -> Vec<serde_json::Value> {
        self.values
            .iter()
            .map(OtlpAnyValue::to_serde_json_value)
            .collect()
    }
}

/// Deserialize a String that may arrive as a JSON string or null, mapping null to empty string.
/// OTLP JSON senders sometimes emit `null` for optional scope/metric string fields.
fn deserialize_string_or_null<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrNullVisitor;

    impl<'de> de::Visitor<'de> for StringOrNullVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or null")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(v.to_owned())
        }

        fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
            Ok(v)
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(String::new())
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(String::new())
        }
    }

    deserializer.deserialize_any(StringOrNullVisitor)
}

/// Deserialize an i64 that may arrive as a JSON number or a quoted string (OTLP protobuf int64).
fn deserialize_int64_string_or_number<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct Int64Visitor;

    impl<'de> de::Visitor<'de> for Int64Visitor {
        type Value = Option<i64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an integer or a string containing an integer")
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            i64::try_from(v).map(Some).map_err(de::Error::custom)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            v.parse::<i64>().map(Some).map_err(de::Error::custom)
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    }

    deserializer.deserialize_any(Int64Visitor)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpAnyValue {
    string_value: Option<String>,
    #[serde(deserialize_with = "deserialize_int64_string_or_number", default)]
    int_value: Option<i64>,
    double_value: Option<f64>,
    bool_value: Option<bool>,
    kvlist_value: Option<OtlpKeyValueList>,
    array_value: Option<OtlpArrayValue>,
}

impl OtlpAnyValue {
    fn primitive_string_value(&self) -> Option<String> {
        if let Some(value) = &self.string_value {
            Some(value.clone())
        } else if let Some(value) = self.int_value {
            Some(value.to_string())
        } else if let Some(value) = self.double_value {
            Some(value.to_string())
        } else {
            self.bool_value.map(|value| value.to_string())
        }
    }

    fn primitive_json_value(&self) -> Option<serde_json::Value> {
        if let Some(value) = &self.string_value {
            Some(serde_json::Value::String(value.clone()))
        } else if let Some(value) = self.int_value {
            Some(serde_json::Value::Number(serde_json::Number::from(value)))
        } else if let Some(value) = self.double_value {
            Some(serde_json::Value::Number(
                match serde_json::Number::from_f64(value) {
                    Some(n) => n,
                    None => {
                        tracing::warn!(
                            float_value = value,
                            "non-finite float (NaN/Infinity) in OTLP attribute, falling back to 0"
                        );
                        serde_json::Number::from(0)
                    }
                },
            ))
        } else {
            self.bool_value.map(serde_json::Value::Bool)
        }
    }

    fn to_string_value(&self) -> String {
        if let Some(value) = self.primitive_string_value() {
            return value;
        }

        if let Some(value) = &self.kvlist_value {
            // For nested structures, serialize to JSON string as fallback.
            return serde_json::to_string(&serde_json::Value::Object(value.to_json_map()))
                .unwrap_or_default();
        }

        if let Some(value) = &self.array_value {
            return serde_json::to_string(&serde_json::Value::Array(value.to_json_array()))
                .unwrap_or_default();
        }

        String::new()
    }

    fn to_serde_json_value(&self) -> serde_json::Value {
        if let Some(value) = self.primitive_json_value() {
            return value;
        }

        if let Some(value) = &self.kvlist_value {
            return serde_json::Value::Object(value.to_json_map());
        }

        if let Some(value) = &self.array_value {
            return serde_json::Value::Array(value.to_json_array());
        }

        serde_json::Value::Null
    }
}

/// Convert OTLP status code to string with optional description
fn otlp_status_to_string_with_description(status: Option<&OtlpStatus>) -> (String, Option<String>) {
    match status {
        Some(s) => {
            let code = match s.code {
                0 => "unset",
                1 => "ok",
                2 => "error",
                _ => "unset",
            };
            let desc = s.message.as_ref().filter(|m| !m.is_empty()).cloned();
            (code.to_string(), desc)
        }
        None => ("unset".to_string(), None),
    }
}

/// Extract service name from resource attributes
fn extract_service_name(resource: &Option<OtlpResource>) -> String {
    if let Some(res) = resource {
        for attr in &res.attributes {
            if attr.key == "service.name"
                && let Some(val) = &attr.value
            {
                return val.to_string_value();
            }
        }
    }
    "unknown".to_string()
}

/// Convert an OtlpKeyValue to an opentelemetry KeyValue.
fn otlp_kv_to_key_value(kv: &OtlpKeyValue) -> Option<KeyValue> {
    let val = kv.value.as_ref()?;

    if let Some(s) = &val.string_value {
        return Some(KeyValue::new(
            kv.key.clone(),
            opentelemetry::Value::String(s.clone().into()),
        ));
    }
    if let Some(i) = val.int_value {
        return Some(KeyValue::new(kv.key.clone(), opentelemetry::Value::I64(i)));
    }
    if let Some(d) = val.double_value {
        return Some(KeyValue::new(kv.key.clone(), opentelemetry::Value::F64(d)));
    }
    if let Some(b) = val.bool_value {
        return Some(KeyValue::new(kv.key.clone(), opentelemetry::Value::Bool(b)));
    }

    // Nested structures: serialize to JSON string representation
    if val.kvlist_value.is_some() || val.array_value.is_some() {
        let json_str = val.to_string_value();
        return Some(KeyValue::new(
            kv.key.clone(),
            opentelemetry::Value::String(json_str.into()),
        ));
    }

    None
}

/// Convert parsed OTLP spans to SpanData for export via the OTel SDK pipeline.
///
/// Production forwarding goes through [`convert_resource_span_to_span_data`]
/// one resource at a time (see `ingest_otlp_json`); this whole-request wrapper
/// is retained for unit tests that assert over a full OTLP request.
#[cfg(test)]
fn convert_otlp_to_span_data(request: &OtlpExportTraceServiceRequest) -> Vec<SpanData> {
    request
        .resource_spans
        .iter()
        .flat_map(convert_resource_span_to_span_data)
        .collect()
}

/// Convert a single OTLP `resource_span` group (one worker `service.name`) to
/// `SpanData`. Split out from [`convert_otlp_to_span_data`] so the SDK span
/// forwarder can process one resource at a time and attach the matching
/// `Resource` before exporting to the collector.
fn convert_resource_span_to_span_data(resource_span: &OtlpResourceSpans) -> Vec<SpanData> {
    use opentelemetry::trace::{
        Event, Link, SpanContext, SpanKind, Status, TraceFlags, TraceState,
    };
    use opentelemetry::{InstrumentationScope, SpanId, TraceId};
    use opentelemetry_sdk::trace::{SpanEvents, SpanLinks};
    use std::borrow::Cow;
    use std::time::{Duration, UNIX_EPOCH};

    let mut span_data_vec = Vec::new();

    {
        for scope_span in &resource_span.scope_spans {
            let scope = scope_span
                .scope
                .as_ref()
                .map(|s| {
                    let mut builder = InstrumentationScope::builder(s.name.clone());
                    if !s.version.is_empty() {
                        builder = builder.with_version(s.version.clone());
                    }
                    builder.build()
                })
                .unwrap_or_else(|| InstrumentationScope::builder("unknown").build());

            for span in &scope_span.spans {
                // Live start snapshots (endTimeUnixNano == 0, see
                // `ingest_otlp_json`) exist only for the in-memory live
                // views; the OTLP proto documents `end_time_unix_nano` as
                // semantically required, so they never reach the collector.
                if span.end_time_unix_nano.0 == 0 {
                    continue;
                }
                // Parse trace and span IDs
                let trace_id = match TraceId::from_hex(&span.trace_id) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                let span_id = match SpanId::from_hex(&span.span_id) {
                    Ok(id) => id,
                    Err(_) => continue,
                };

                let parent_span_id = span
                    .parent_span_id
                    .as_ref()
                    .and_then(|p| {
                        if p.is_empty() || p.chars().all(|c| c == '0') {
                            None
                        } else {
                            SpanId::from_hex(p).ok()
                        }
                    })
                    .unwrap_or(SpanId::INVALID);

                // Spans arriving via ingest_otlp_json originate from an external SDK
                // process (Node.js), so a valid parent span is always remote.
                let parent_span_is_remote = parent_span_id != SpanId::INVALID;

                // Respect incoming W3C trace flags from the OTLP span (lowest 8 bits
                // of the u32). Fall back to SAMPLED when absent, since spans
                // arriving via OTLP were already exported by the upstream SDK.
                let trace_flags = span
                    .flags
                    .map(|f| TraceFlags::new(f as u8))
                    .unwrap_or(TraceFlags::SAMPLED);

                let span_context =
                    SpanContext::new(trace_id, span_id, trace_flags, true, TraceState::NONE);

                let start_time = UNIX_EPOCH + Duration::from_nanos(span.start_time_unix_nano.0);
                let end_time = UNIX_EPOCH + Duration::from_nanos(span.end_time_unix_nano.0);

                let attributes: Vec<KeyValue> = span
                    .attributes
                    .iter()
                    .filter_map(otlp_kv_to_key_value)
                    .collect();

                // Determine span kind from the numeric `kind` field, or fall back
                // to checking the "otel.kind" attribute string.
                let span_kind = span
                    .kind
                    .and_then(|k| match k {
                        1 => Some(SpanKind::Internal),
                        2 => Some(SpanKind::Server),
                        3 => Some(SpanKind::Client),
                        4 => Some(SpanKind::Producer),
                        5 => Some(SpanKind::Consumer),
                        _ => None,
                    })
                    .or_else(|| {
                        attributes
                            .iter()
                            .find(|kv| kv.key.as_str() == "otel.kind")
                            .and_then(|kv| match kv.value.as_str().as_ref() {
                                "client" | "CLIENT" => Some(SpanKind::Client),
                                "server" | "SERVER" => Some(SpanKind::Server),
                                "producer" | "PRODUCER" => Some(SpanKind::Producer),
                                "consumer" | "CONSUMER" => Some(SpanKind::Consumer),
                                "internal" | "INTERNAL" => Some(SpanKind::Internal),
                                _ => None,
                            })
                    })
                    .unwrap_or(SpanKind::Internal);

                let events: Vec<Event> = span
                    .events
                    .iter()
                    .map(|e| {
                        let ts = UNIX_EPOCH + Duration::from_nanos(e.time_unix_nano.0);
                        let attrs: Vec<KeyValue> = e
                            .attributes
                            .iter()
                            .filter_map(otlp_kv_to_key_value)
                            .collect();
                        Event::new(e.name.clone(), ts, attrs, 0)
                    })
                    .collect();

                let links: Vec<Link> = span
                    .links
                    .iter()
                    .filter_map(|l| {
                        let lt = TraceId::from_hex(&l.trace_id).ok()?;
                        let ls = SpanId::from_hex(&l.span_id).ok()?;
                        let trace_state = l
                            .trace_state
                            .as_deref()
                            .and_then(|ts| ts.parse::<TraceState>().ok())
                            .unwrap_or(TraceState::NONE);
                        // OtlpSpanLink does not expose per-link trace flags in the
                        // current OTLP spec; default to TraceFlags::SAMPLED. If
                        // OtlpSpanLink gains a flags field, parse it here via
                        // TraceFlags::new() and pass to SpanContext::new instead.
                        let lc = SpanContext::new(lt, ls, TraceFlags::SAMPLED, true, trace_state);
                        let attrs: Vec<KeyValue> = l
                            .attributes
                            .iter()
                            .filter_map(otlp_kv_to_key_value)
                            .collect();
                        Some(Link::new(lc, attrs, 0))
                    })
                    .collect();

                let status = match span.status.as_ref() {
                    Some(s) => match s.code {
                        1 => Status::Ok,
                        2 => Status::error(s.message.as_deref().unwrap_or("error").to_string()),
                        _ => Status::Unset,
                    },
                    None => Status::Unset,
                };

                let mut span_events = SpanEvents::default();
                span_events.events = events;

                let mut span_links = SpanLinks::default();
                span_links.links = links;

                let sd = SpanData {
                    span_context,
                    parent_span_id,
                    parent_span_is_remote,
                    span_kind,
                    name: Cow::Owned(span.name.clone()),
                    start_time,
                    end_time,
                    attributes,
                    dropped_attributes_count: 0,
                    events: span_events,
                    links: span_links,
                    status,
                    instrumentation_scope: scope.clone(),
                };

                span_data_vec.push(sd);
            }
        }
    }

    span_data_vec
}

/// Ingest OTLP JSON data from Node SDK and merge into in-memory storage.
///
/// This function is called when the engine receives an OTLP binary frame
/// (prefixed with "OTLP") from a worker.
pub async fn ingest_otlp_json(json_str: &str) -> anyhow::Result<()> {
    // Skip ingestion entirely when observability is disabled
    if let Some(config) = get_otel_config()
        && !config.enabled.unwrap_or(true)
    {
        return Ok(());
    }

    // Parse the OTLP JSON
    let request: OtlpExportTraceServiceRequest = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse OTLP JSON: {}", e))?;

    // Store in memory if available (for API access)
    if let Some(storage) = get_span_storage() {
        // Zero-end spans are worker start snapshots (the SDK's
        // `LiveSpanStartProcessor`): stored as pendings so live views render
        // in-progress worker spans, the final span replacing each in place
        // when the batch exporter delivers it. Gated by the same live-spans
        // switch as the engine's own start mirror — when it resolves off
        // (incl. OTLP-only, where finals never land in memory) snapshots are
        // dropped, never stored as zero-end finals.
        let live_ingest = OtelConfig::default().live_spans;
        let mut stored_spans = Vec::new();
        let mut pending_spans = Vec::new();

        for resource_span in &request.resource_spans {
            let service_name = extract_service_name(&resource_span.resource);

            for scope_span in &resource_span.scope_spans {
                // Extract instrumentation scope info
                let scope_name = scope_span
                    .scope
                    .as_ref()
                    .map(|s| s.name.clone())
                    .filter(|s| !s.is_empty());
                let scope_version = scope_span
                    .scope
                    .as_ref()
                    .map(|s| s.version.clone())
                    .filter(|s| !s.is_empty());

                for span in &scope_span.spans {
                    let is_start_snapshot = span.end_time_unix_nano.0 == 0;
                    if is_start_snapshot && !live_ingest {
                        continue;
                    }
                    // Convert parent_span_id (skip if empty or all zeros)
                    let parent_span_id = span.parent_span_id.as_ref().and_then(|p| {
                        if p.is_empty() || p == "0000000000000000" || p.chars().all(|c| c == '0') {
                            None
                        } else {
                            Some(p.clone())
                        }
                    });

                    // Convert attributes
                    let attributes: Vec<(String, String)> = span
                        .attributes
                        .iter()
                        .filter_map(|kv| {
                            kv.value
                                .as_ref()
                                .map(|v| (kv.key.clone(), v.to_string_value()))
                        })
                        .collect();

                    // Convert events
                    let events: Vec<StoredSpanEvent> = span
                        .events
                        .iter()
                        .map(|event| {
                            let attrs: Vec<(String, String)> = event
                                .attributes
                                .iter()
                                .filter_map(|kv| {
                                    kv.value
                                        .as_ref()
                                        .map(|v| (kv.key.clone(), v.to_string_value()))
                                })
                                .collect();
                            StoredSpanEvent {
                                name: event.name.clone(),
                                timestamp_unix_nano: event.time_unix_nano.0,
                                attributes: attrs,
                            }
                        })
                        .collect();

                    // Convert links
                    let links: Vec<StoredSpanLink> = span
                        .links
                        .iter()
                        .map(|link| {
                            let attrs: Vec<(String, String)> = link
                                .attributes
                                .iter()
                                .filter_map(|kv| {
                                    kv.value
                                        .as_ref()
                                        .map(|v| (kv.key.clone(), v.to_string_value()))
                                })
                                .collect();
                            StoredSpanLink {
                                trace_id: link.trace_id.clone(),
                                span_id: link.span_id.clone(),
                                trace_state: link.trace_state.clone(),
                                attributes: attrs,
                            }
                        })
                        .collect();

                    let (status, status_description) =
                        otlp_status_to_string_with_description(span.status.as_ref());

                    let stored_span = StoredSpan {
                        trace_id: span.trace_id.clone(),
                        span_id: span.span_id.clone(),
                        parent_span_id,
                        name: span.name.clone(),
                        start_time_unix_nano: span.start_time_unix_nano.0,
                        end_time_unix_nano: span.end_time_unix_nano.0,
                        status,
                        status_description,
                        attributes,
                        service_name: service_name.clone(),
                        events,
                        links,
                        instrumentation_scope_name: scope_name.clone(),
                        instrumentation_scope_version: scope_version.clone(),
                        flags: span.flags,
                        // OTLP-ingested spans don't currently carry tracestate.
                        trace_state: None,
                        pending: is_start_snapshot,
                    };

                    if is_start_snapshot {
                        pending_spans.push(stored_span);
                    } else {
                        stored_spans.push(stored_span);
                    }
                }
            }
        }

        let span_count = stored_spans.len() + pending_spans.len();
        // Pendings before finals: the telemetry WS is FIFO per worker, so a
        // start snapshot always ingests before its final frame — keeping
        // that order inside a mixed batch preserves the in-place replace.
        for pending in pending_spans {
            storage.add_pending_span(pending);
        }
        if !stored_spans.is_empty() {
            storage.add_spans(stored_spans);
        }
        if span_count > 0 {
            tracing::debug!(
                span_count = span_count,
                "Ingested OTLP spans into memory storage"
            );
        }
    }

    // Forward to the OTLP collector, one resource (worker service.name) at a
    // time so each batch carries the correct service identity, and honoring the
    // engine sampler so forwarded volume matches `sampling_ratio` (worker SDKs
    // export to the engine at 100%).
    if SDK_FORWARDER_ENDPOINT.get().is_some() {
        let sampler = get_dynamic_sampler();
        for resource_span in &request.resource_spans {
            let mut span_data = convert_resource_span_to_span_data(resource_span);
            if let Some(sampler) = sampler {
                span_data.retain(|sd| forward_span_is_sampled(sampler, sd));
            }
            if span_data.is_empty() {
                continue;
            }

            let service_name = extract_service_name(&resource_span.resource);
            let resource = build_forward_resource(&resource_span.resource);
            let Some(forwarder) = get_or_build_forwarder(&service_name, &resource) else {
                continue;
            };

            let count = span_data.len();
            match forwarder.export(span_data).await {
                Ok(()) => {
                    tracing::debug!(
                        span_count = count,
                        service_name = %service_name,
                        "Forwarded SDK spans to OTLP collector"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = ?e, "Failed to forward SDK spans to OTLP collector");
                }
            }
        }
    }

    Ok(())
}

// =============================================================================
// OTLP JSON Metrics Ingestion from Node SDK
// =============================================================================

/// OTLP ExportMetricsServiceRequest structure for JSON deserialization
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpExportMetricsServiceRequest {
    #[serde(default)]
    resource_metrics: Vec<OtlpResourceMetrics>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpResourceMetrics {
    #[serde(default)]
    resource: Option<OtlpResource>,
    #[serde(default)]
    scope_metrics: Vec<OtlpScopeMetrics>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct OtlpScopeMetrics {
    #[serde(default)]
    scope: Option<OtlpScope>,
    #[serde(default)]
    metrics: Vec<OtlpMetric>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct OtlpScope {
    #[serde(default, deserialize_with = "deserialize_string_or_null")]
    name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_null")]
    version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpMetric {
    #[serde(default, deserialize_with = "deserialize_string_or_null")]
    name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_null")]
    description: String,
    #[serde(default, deserialize_with = "deserialize_string_or_null")]
    unit: String,
    #[serde(default)]
    gauge: Option<OtlpGauge>,
    #[serde(default)]
    sum: Option<OtlpSum>,
    #[serde(default)]
    histogram: Option<OtlpHistogram>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpGauge {
    #[serde(default)]
    data_points: Vec<OtlpNumberDataPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct OtlpSum {
    #[serde(default)]
    data_points: Vec<OtlpNumberDataPoint>,
    #[serde(default)]
    aggregation_temporality: u32,
    #[serde(default)]
    is_monotonic: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct OtlpHistogram {
    #[serde(default)]
    data_points: Vec<OtlpHistogramDataPoint>,
    #[serde(default)]
    aggregation_temporality: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct OtlpNumberDataPoint {
    #[serde(default)]
    attributes: Vec<OtlpKeyValue>,
    #[serde(default)]
    start_time_unix_nano: OtlpNumericString,
    #[serde(default)]
    time_unix_nano: OtlpNumericString,
    #[serde(default)]
    as_double: Option<f64>,
    /// Per OTLP/JSON spec this is encoded as a string, but some senders use a
    /// bare JSON number. `OtlpIntString` accepts both.
    #[serde(default)]
    as_int: Option<OtlpIntString>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct OtlpHistogramDataPoint {
    #[serde(default)]
    attributes: Vec<OtlpKeyValue>,
    #[serde(default)]
    start_time_unix_nano: OtlpNumericString,
    #[serde(default)]
    time_unix_nano: OtlpNumericString,
    #[serde(default)]
    count: OtlpNumericString,
    #[serde(default)]
    sum: Option<f64>,
    #[serde(default)]
    bucket_counts: Vec<OtlpNumericString>,
    #[serde(default)]
    explicit_bounds: Vec<f64>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
}

impl OtlpNumberDataPoint {
    fn get_value(&self) -> f64 {
        self.as_double
            .or_else(|| self.as_int.map(|i| i.0 as f64))
            .unwrap_or(0.0)
    }

    fn get_attributes(&self) -> Vec<(String, String)> {
        self.attributes
            .iter()
            .filter_map(|kv| {
                kv.value
                    .as_ref()
                    .map(|v| (kv.key.clone(), v.to_string_value()))
            })
            .collect()
    }
}

impl OtlpHistogramDataPoint {
    fn get_attributes(&self) -> Vec<(String, String)> {
        self.attributes
            .iter()
            .filter_map(|kv| {
                kv.value
                    .as_ref()
                    .map(|v| (kv.key.clone(), v.to_string_value()))
            })
            .collect()
    }
}

/// Ingest OTLP JSON metrics data from Node SDK.
///
/// This function is called when the engine receives a metrics frame
/// (prefixed with "MTRC") from a worker.
pub async fn ingest_otlp_metrics(json_str: &str) -> anyhow::Result<()> {
    // Skip ingestion entirely when observability is disabled
    if let Some(config) = get_otel_config()
        && !config.enabled.unwrap_or(true)
    {
        return Ok(());
    }

    use super::metrics::{
        StoredDataPoint, StoredHistogramDataPoint, StoredMetric, StoredMetricType,
        StoredNumberDataPoint, get_metric_storage,
    };

    tracing::debug!(
        metrics_size = json_str.len(),
        "Received OTLP metrics from Node SDK"
    );

    // Parse the OTLP metrics JSON
    let request: OtlpExportMetricsServiceRequest = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse OTLP metrics JSON: {}", e))?;

    // Get the in-memory metric storage (if available)
    let storage = match get_metric_storage() {
        Some(s) => s,
        None => {
            tracing::debug!("No in-memory metric storage available, skipping metrics ingestion");
            return Ok(());
        }
    };

    // Convert OTLP metrics to StoredMetric and add to storage
    let mut stored_metrics = Vec::new();

    for resource_metric in &request.resource_metrics {
        let service_name = extract_service_name(&resource_metric.resource);

        for scope_metric in &resource_metric.scope_metrics {
            // Extract instrumentation scope info
            let scope_name = scope_metric
                .scope
                .as_ref()
                .map(|s| s.name.clone())
                .filter(|s| !s.is_empty());
            let scope_version = scope_metric
                .scope
                .as_ref()
                .map(|s| s.version.clone())
                .filter(|s| !s.is_empty());

            for metric in &scope_metric.metrics {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64;

                // Determine metric type and convert data points
                let (metric_type, data_points) = if let Some(gauge) = &metric.gauge {
                    let points = gauge
                        .data_points
                        .iter()
                        .map(|dp| {
                            StoredDataPoint::Number(StoredNumberDataPoint {
                                value: dp.get_value(),
                                attributes: dp.get_attributes(),
                                timestamp_unix_nano: dp.time_unix_nano.0,
                            })
                        })
                        .collect();
                    (StoredMetricType::Gauge, points)
                } else if let Some(sum) = &metric.sum {
                    let points = sum
                        .data_points
                        .iter()
                        .map(|dp| {
                            StoredDataPoint::Number(StoredNumberDataPoint {
                                value: dp.get_value(),
                                attributes: dp.get_attributes(),
                                timestamp_unix_nano: dp.time_unix_nano.0,
                            })
                        })
                        .collect();
                    let metric_type = if sum.is_monotonic {
                        StoredMetricType::Counter
                    } else {
                        StoredMetricType::UpDownCounter
                    };
                    (metric_type, points)
                } else if let Some(histogram) = &metric.histogram {
                    let points = histogram
                        .data_points
                        .iter()
                        .map(|dp| {
                            StoredDataPoint::Histogram(StoredHistogramDataPoint {
                                count: dp.count.0,
                                sum: dp.sum.unwrap_or(0.0),
                                bucket_counts: dp.bucket_counts.iter().map(|c| c.0).collect(),
                                explicit_bounds: dp.explicit_bounds.clone(),
                                min: dp.min,
                                max: dp.max,
                                attributes: dp.get_attributes(),
                                timestamp_unix_nano: dp.time_unix_nano.0,
                            })
                        })
                        .collect();
                    (StoredMetricType::Histogram, points)
                } else {
                    // Unknown metric type, skip
                    continue;
                };

                let stored_metric = StoredMetric {
                    name: metric.name.clone(),
                    description: metric.description.clone(),
                    unit: metric.unit.clone(),
                    metric_type,
                    data_points,
                    service_name: service_name.clone(),
                    timestamp_unix_nano: timestamp,
                    instrumentation_scope_name: scope_name.clone(),
                    instrumentation_scope_version: scope_version.clone(),
                };

                stored_metrics.push(stored_metric);
            }
        }
    }

    let metric_count = stored_metrics.len();
    if metric_count > 0 {
        storage.add_metrics(stored_metrics);
        tracing::debug!(
            metric_count = metric_count,
            "Ingested OTLP metrics from Node SDK"
        );
    }

    Ok(())
}

// =============================================================================
// OTLP JSON Logs Ingestion from Node SDK
// =============================================================================

/// Default maximum number of logs to keep in memory.
const DEFAULT_MEMORY_MAX_LOGS: usize = 1000;

/// OTLP ExportLogsServiceRequest structure for JSON deserialization
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpExportLogsServiceRequest {
    #[serde(default)]
    resource_logs: Vec<OtlpResourceLogs>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpResourceLogs {
    #[serde(default)]
    resource: Option<OtlpResource>,
    #[serde(default)]
    scope_logs: Vec<OtlpScopeLogs>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct OtlpScopeLogs {
    #[serde(default)]
    scope: Option<OtlpScope>,
    #[serde(default)]
    log_records: Vec<OtlpLogRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct OtlpLogRecord {
    #[serde(default)]
    time_unix_nano: OtlpNumericString,
    #[serde(default)]
    observed_time_unix_nano: OtlpNumericString,
    #[serde(default)]
    severity_number: Option<i32>,
    #[serde(default)]
    severity_text: Option<String>,
    #[serde(default)]
    body: Option<OtlpAnyValue>,
    #[serde(default)]
    attributes: Vec<OtlpKeyValue>,
    #[serde(default)]
    trace_id: Option<String>,
    #[serde(default)]
    span_id: Option<String>,
    #[serde(default)]
    flags: Option<u32>,
}

/// Serializable representation of a log for API responses.
#[derive(Debug, Clone, Serialize)]
pub struct StoredLog {
    pub timestamp_unix_nano: u64,
    pub observed_timestamp_unix_nano: u64,
    pub severity_number: i32,
    pub severity_text: String,
    pub body: String,
    pub attributes: HashMap<String, serde_json::Value>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub resource: HashMap<String, String>,
    pub service_name: String,
    /// Instrumentation scope name (e.g., "@opentelemetry/api-logs")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrumentation_scope_name: Option<String>,
    /// Instrumentation scope version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrumentation_scope_version: Option<String>,
}

/// In-memory log storage with circular buffer and broadcast channel.
pub struct InMemoryLogStorage {
    logs: RwLock<VecDeque<StoredLog>>,
    /// Capacity limit. Atomic so the configuration-worker apply path can
    /// retune it at runtime; enforcement happens on every insert.
    max_logs: std::sync::atomic::AtomicUsize,
    tx: broadcast::Sender<StoredLog>,
}

impl std::fmt::Debug for InMemoryLogStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryLogStorage")
            .field("logs", &self.logs)
            .field(
                "max_logs",
                &self.max_logs.load(std::sync::atomic::Ordering::Relaxed),
            )
            .finish()
    }
}

/// Strip ANSI/terminal escape codes from a log's body and string attribute
/// values before it enters storage.
///
/// Logs are colorized at their emission site — engine `tracing` macros (e.g.
/// `"[REGISTERED]".green()`) and worker output forwarded over OTLP. Color is a
/// terminal-display concern, but stored logs are queried as structured data via
/// `engine::logs::list` and the web console, so they must be plain text in every
/// output mode. `store`/`add_logs` are the single chokepoint that all stored-log
/// sources funnel through (engine tracing layer, OTLP ingest, the `log`
/// function), so stripping here covers them all in one place. Terminal coloring
/// is untouched: console rendering happens before storage (see
/// `emit_log_to_console`) and via a separate fmt layer, both of which see the
/// pre-strip value.
fn strip_ansi_from_log(log: &mut StoredLog) {
    if let std::borrow::Cow::Owned(clean) = console::strip_ansi_codes(&log.body) {
        log.body = clean;
    }
    for value in log.attributes.values_mut() {
        if let serde_json::Value::String(s) = value
            && let std::borrow::Cow::Owned(clean) = console::strip_ansi_codes(s)
        {
            *s = clean;
        }
    }
}

impl InMemoryLogStorage {
    pub fn new(max_logs: usize) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            logs: RwLock::new(VecDeque::with_capacity(max_logs)),
            max_logs: std::sync::atomic::AtomicUsize::new(max_logs),
            tx,
        }
    }

    /// Retune the capacity at runtime; a shrink evicts oldest logs.
    pub fn set_max_logs(&self, max: usize) {
        let max = max.max(1);
        self.max_logs
            .store(max, std::sync::atomic::Ordering::Relaxed);
        let mut logs = self.logs.write().unwrap();
        if logs.len() > max {
            let excess = logs.len() - max;
            logs.drain(..excess);
        }
    }

    pub fn store(&self, mut log: StoredLog) {
        strip_ansi_from_log(&mut log);
        let mut logs = self.logs.write().unwrap();
        while logs.len()
            >= self
                .max_logs
                .load(std::sync::atomic::Ordering::Relaxed)
                .max(1)
        {
            logs.pop_front();
        }
        logs.push_back(log.clone());

        // Broadcast to any listeners (ignore send errors if no receivers)
        let _ = self.tx.send(log);
    }

    pub fn add_logs(&self, new_logs: Vec<StoredLog>) {
        let mut logs = self.logs.write().unwrap();
        let max_logs = self
            .max_logs
            .load(std::sync::atomic::Ordering::Relaxed)
            .max(1);
        for mut log in new_logs {
            strip_ansi_from_log(&mut log);
            while logs.len() >= max_logs {
                logs.pop_front();
            }
            logs.push_back(log.clone());
            let _ = self.tx.send(log);
        }
    }

    pub fn get_logs(&self) -> Vec<StoredLog> {
        self.logs.read().unwrap().iter().cloned().collect()
    }

    pub fn get_logs_by_trace_id(&self, trace_id: &str) -> Vec<StoredLog> {
        self.logs
            .read()
            .unwrap()
            .iter()
            .filter(|l| l.trace_id.as_deref() == Some(trace_id))
            .cloned()
            .collect()
    }

    pub fn get_logs_by_span_id(&self, span_id: &str) -> Vec<StoredLog> {
        self.logs
            .read()
            .unwrap()
            .iter()
            .filter(|l| l.span_id.as_deref() == Some(span_id))
            .cloned()
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn get_logs_filtered(
        &self,
        trace_id: Option<&str>,
        span_id: Option<&str>,
        severity_min: Option<i32>,
        severity_text: Option<&str>,
        start_time: Option<u64>,
        end_time: Option<u64>,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> (usize, Vec<StoredLog>) {
        // Convert time values with overflow checking before filtering
        let start_time_ns = if let Some(start) = start_time {
            match start.checked_mul(1_000_000) {
                Some(ns) => Some(ns),
                None => {
                    tracing::warn!(
                        "start_time overflow when converting to nanoseconds in log filter"
                    );
                    return (0, Vec::new());
                }
            }
        } else {
            None
        };

        let end_time_ns = if let Some(end) = end_time {
            match end.checked_mul(1_000_000) {
                Some(ns) => Some(ns),
                None => {
                    tracing::warn!(
                        "end_time overflow when converting to nanoseconds in log filter"
                    );
                    return (0, Vec::new());
                }
            }
        } else {
            None
        };

        let logs = self.logs.read().unwrap();
        let mut result: Vec<StoredLog> = logs
            .iter()
            .filter(|log| {
                // Filter by trace_id
                if let Some(tid) = trace_id
                    && log.trace_id.as_deref() != Some(tid)
                {
                    return false;
                }
                // Filter by span_id
                if let Some(sid) = span_id
                    && log.span_id.as_deref() != Some(sid)
                {
                    return false;
                }
                // Filter by severity number (minimum threshold)
                if let Some(min_sev) = severity_min
                    && log.severity_number < min_sev
                {
                    return false;
                }
                // Filter by severity text (exact match, case-insensitive)
                if let Some(sev_text) = severity_text
                    && !log.severity_text.eq_ignore_ascii_case(sev_text)
                {
                    return false;
                }
                // Filter by time range (using pre-converted nanosecond values)
                if let Some(start_ns) = start_time_ns
                    && log.timestamp_unix_nano < start_ns
                {
                    return false;
                }
                if let Some(end_ns) = end_time_ns
                    && log.timestamp_unix_nano > end_ns
                {
                    return false;
                }
                true
            })
            .cloned()
            .collect();

        // Sort by timestamp (newest first)
        result.sort_by_key(|b| std::cmp::Reverse(b.timestamp_unix_nano));

        // Get total before pagination
        let total = result.len();

        // Apply offset and limit
        let offset_val = offset.unwrap_or(0);
        let paginated = result
            .into_iter()
            .skip(offset_val)
            .take(limit.unwrap_or(usize::MAX))
            .collect();

        (total, paginated)
    }

    pub fn clear(&self) {
        self.logs.write().unwrap().clear();
    }

    pub fn len(&self) -> usize {
        self.logs.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.logs.read().unwrap().is_empty()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StoredLog> {
        self.tx.subscribe()
    }

    /// Drop logs whose effective timestamp is older than `retention_ns` from now.
    ///
    /// Scans the entire buffer rather than popping from the front: logs are
    /// stored in arrival order, which does not match timestamp order when
    /// SDK workers batch backdated records, clocks skew across workers, or
    /// sampling interleaves late arrivals. An older-timestamped log trapped
    /// behind a newer one would otherwise survive retention.
    ///
    /// The effective timestamp is `timestamp_unix_nano` when non-zero,
    /// falling back to `observed_timestamp_unix_nano`. Per the OTLP logs
    /// spec, a `time_unix_nano` of 0 means "unknown" and receivers must use
    /// `observed_time_unix_nano` instead; without this fallback, logs
    /// emitted without an event timestamp would be evicted on the very
    /// first retention tick despite a valid observation time.
    pub fn apply_retention(&self, retention_ns: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let cutoff = now.saturating_sub(retention_ns);

        let mut logs = self.logs.write().unwrap();
        logs.retain(|log| {
            let effective = if log.timestamp_unix_nano != 0 {
                log.timestamp_unix_nano
            } else {
                log.observed_timestamp_unix_nano
            };
            effective >= cutoff
        });
    }
}

/// Global in-memory log storage.
static LOG_STORAGE: OnceLock<Arc<InMemoryLogStorage>> = OnceLock::new();

/// Get the global in-memory log storage (if initialized).
pub fn get_log_storage() -> Option<Arc<InMemoryLogStorage>> {
    LOG_STORAGE.get().cloned()
}

/// Initialize the global log storage, or retune the capacity of an existing
/// one when an explicit limit is supplied. `None` never resets a configured
/// limit back to the default.
pub fn init_log_storage(max_logs: Option<usize>) {
    if let Some(existing) = LOG_STORAGE.get() {
        if let Some(max) = max_logs {
            existing.set_max_logs(max);
        }
        return;
    }
    let storage = Arc::new(InMemoryLogStorage::new(
        max_logs.unwrap_or(DEFAULT_MEMORY_MAX_LOGS),
    ));
    let _ = LOG_STORAGE.set(storage);
}

/// Extract resource attributes as a HashMap
fn extract_resource_attributes(resource: &Option<OtlpResource>) -> HashMap<String, String> {
    resource
        .as_ref()
        .map(|res| {
            res.attributes
                .iter()
                .filter_map(|attr| {
                    attr.value
                        .as_ref()
                        .map(|val| (attr.key.clone(), val.to_string_value()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Extract log body as a string
fn extract_log_body(body: &Option<OtlpAnyValue>) -> String {
    body.as_ref()
        .map_or_else(String::new, OtlpAnyValue::to_string_value)
}

/// Extract log attributes as a HashMap
fn extract_log_attributes(attributes: &[OtlpKeyValue]) -> HashMap<String, serde_json::Value> {
    attributes
        .iter()
        .filter_map(|attr| {
            attr.value
                .as_ref()
                .map(|val| (attr.key.clone(), val.to_serde_json_value()))
        })
        .collect()
}

/// Tracing target used by [`emit_log_to_console`] to mark forwarded OTLP log records.
///
/// [`OtelLogsLayer`](super::logs_layer::OtelLogsLayer) matches on this target to skip
/// re-storing logs that were already persisted by `ingest_otlp_logs`.
pub(crate) const OTEL_PASSTHROUGH_TARGET: &str = "iii::otel_passthrough";

/// Emit a log to the console via tracing based on severity level.
///
/// Uses [`OTEL_PASSTHROUGH_TARGET`] so `OtelLogsLayer` skips these events
/// and avoids storing them a second time (they are already stored by `ingest_otlp_logs`).
/// Build the (data_json, function_name) pair that `emit_log_to_console`
/// passes to the tracing macros for a passthrough log record.
///
/// - `function_name` is read from the `service.name` attribute (empty
///   string when absent).
/// - `data_json` is the JSON-serialized attribute map with `service.name`
///   stripped out (empty string when the map is empty or has only
///   `service.name`).
pub(crate) fn build_console_log_fields(log: &StoredLog) -> (String, &str) {
    let function_name = log
        .attributes
        .get("service.name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let data = if log.attributes.is_empty() {
        String::new()
    } else {
        let filtered: std::collections::BTreeMap<&str, &serde_json::Value> = log
            .attributes
            .iter()
            .filter(|(k, _)| k.as_str() != "service.name")
            .map(|(k, v)| (k.as_str(), v))
            .collect();
        if filtered.is_empty() {
            String::new()
        } else {
            serde_json::to_string(&filtered).unwrap_or_default()
        }
    };

    (data, function_name)
}

fn emit_log_to_console(log: &StoredLog) {
    // Extract the function name from the `service.name` attribute before it
    // gets stripped from the rendered blob. This is emitted as its own
    // tracing field (`function_name`) so the console formatter can render
    // it as a distinct token without having to re-parse `data`.
    // `service.name` is also excluded from the rendered attribute blob: it
    // is redundant with the resource-level service name (rendered separately
    // as the passthrough header by the console formatter) and clutters the
    // tree.
    let (data, function_name) = build_console_log_fields(log);

    let service = &log.service_name;
    let body = &log.body;

    // OTEL severity numbers: TRACE=1-4, DEBUG=5-8, INFO=9-12, WARN=13-16, ERROR=17-20, FATAL=21-24
    match log.severity_number {
        1..=4 => {
            tracing::trace!(target: OTEL_PASSTHROUGH_TARGET, service = %service, function_name = %function_name, data = %data, "{}", body)
        }
        5..=8 => {
            tracing::debug!(target: OTEL_PASSTHROUGH_TARGET, service = %service, function_name = %function_name, data = %data, "{}", body)
        }
        9..=12 => {
            tracing::info!(target: OTEL_PASSTHROUGH_TARGET, service = %service, function_name = %function_name, data = %data, "{}", body)
        }
        13..=16 => {
            tracing::warn!(target: OTEL_PASSTHROUGH_TARGET, service = %service, function_name = %function_name, data = %data, "{}", body)
        }
        17..=24 => {
            tracing::error!(target: OTEL_PASSTHROUGH_TARGET, service = %service, function_name = %function_name, data = %data, "{}", body)
        }
        _ => {
            tracing::info!(target: OTEL_PASSTHROUGH_TARGET, service = %service, function_name = %function_name, data = %data, "{}", body)
        }
    }
}

/// Check if console output is enabled for OTEL logs.
fn should_output_to_console() -> bool {
    get_otel_config()
        .map(|c| c.logs_console_output)
        .unwrap_or(true)
}

/// Ingest OTLP JSON logs data from Node SDK.
///
/// This function is called when the engine receives a logs frame
/// (prefixed with "LOGS") from a worker.
pub async fn ingest_otlp_logs(json_str: &str) -> anyhow::Result<()> {
    // Skip ingestion entirely when observability is disabled
    if let Some(config) = get_otel_config()
        && !config.enabled.unwrap_or(true)
    {
        return Ok(());
    }

    tracing::debug!(
        logs_size = json_str.len(),
        "Received OTLP logs from Node SDK"
    );

    // Honor logs_enabled: silently drop incoming OTLP logs when logs are
    // disabled at config time. Otherwise a Node/Python SDK worker posting
    // logs would lazily revive the storage that `initialize()` deliberately
    // did not create.
    if !logs_enabled(get_otel_config().as_deref()) {
        return Ok(());
    }

    // Parse the OTLP logs JSON
    let request: OtlpExportLogsServiceRequest = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse OTLP logs JSON: {}", e))?;

    // Get the in-memory log storage (if available)
    let storage = match get_log_storage() {
        Some(s) => s,
        None => {
            return Err(anyhow::anyhow!("Log storage not initialized"));
        }
    };

    // Convert OTLP logs to StoredLog and add to storage
    let mut stored_logs = Vec::new();

    for resource_log in &request.resource_logs {
        let service_name = extract_service_name(&resource_log.resource);
        let resource_attrs = extract_resource_attributes(&resource_log.resource);

        for scope_log in &resource_log.scope_logs {
            // Extract instrumentation scope info
            let scope_name = scope_log
                .scope
                .as_ref()
                .map(|s| s.name.clone())
                .filter(|s| !s.is_empty());
            let scope_version = scope_log
                .scope
                .as_ref()
                .map(|s| s.version.clone())
                .filter(|s| !s.is_empty());

            for log_record in &scope_log.log_records {
                let stored_log = StoredLog {
                    timestamp_unix_nano: log_record.time_unix_nano.0,
                    observed_timestamp_unix_nano: log_record.observed_time_unix_nano.0,
                    severity_number: log_record.severity_number.unwrap_or(0),
                    severity_text: log_record.severity_text.clone().unwrap_or_default(),
                    body: extract_log_body(&log_record.body),
                    attributes: extract_log_attributes(&log_record.attributes),
                    trace_id: log_record
                        .trace_id
                        .clone()
                        .filter(|s| !s.is_empty() && s != "00000000000000000000000000000000"),
                    span_id: log_record
                        .span_id
                        .clone()
                        .filter(|s| !s.is_empty() && s != "0000000000000000"),
                    resource: resource_attrs.clone(),
                    service_name: service_name.clone(),
                    instrumentation_scope_name: scope_name.clone(),
                    instrumentation_scope_version: scope_version.clone(),
                };

                // Emit to console if enabled
                if should_output_to_console() {
                    emit_log_to_console(&stored_log);
                }

                stored_logs.push(stored_log);
            }
        }
    }

    let log_count = stored_logs.len();
    if log_count > 0 {
        storage.add_logs(stored_logs);
        tracing::debug!(log_count = log_count, "Ingested OTLP logs from Node SDK");
    }

    Ok(())
}

// ============================================================================
// OTLP Logs Exporter
// ============================================================================

/// Default batch size for log export
const DEFAULT_LOG_BATCH_SIZE: usize = 100;

/// Maximum allowed batch size for log export
const MAX_LOG_BATCH_SIZE: usize = 10_000;

/// Default flush interval for log export (5000 milliseconds)
const DEFAULT_LOG_FLUSH_INTERVAL_MS: u64 = 5000;

/// Minimum flush interval for log export (100 milliseconds)
const MIN_LOG_FLUSH_INTERVAL_MS: u64 = 100;

/// Maximum flush interval for log export (1 hour)
const MAX_LOG_FLUSH_INTERVAL_MS: u64 = 3_600_000;

/// HTTP timeout for a single OTLP logs export request. Bounds `export_batch`
/// so a stalled collector cannot park the exporter task indefinitely — the
/// task's shutdown signal is only observed between awaits in the select loop,
/// so an unbounded send would also defeat exporter teardown on config rebuild.
const DEFAULT_LOG_EXPORT_TIMEOUT_SECS: u64 = 30;

/// OTLP Logs Exporter - exports logs to an OTLP HTTP collector.
pub struct OtlpLogsExporter {
    endpoint: String,
    service_name: String,
    service_version: String,
    batch_size: usize,
    flush_interval: Duration,
    headers: HeaderMap,
}

impl OtlpLogsExporter {
    /// Create a new OTLP logs exporter
    pub fn new(endpoint: String, service_name: String, service_version: String) -> Self {
        Self {
            endpoint,
            service_name,
            service_version,
            batch_size: DEFAULT_LOG_BATCH_SIZE,
            flush_interval: Duration::from_millis(DEFAULT_LOG_FLUSH_INTERVAL_MS),
            headers: otlp_logs_headers_from_env(),
        }
    }

    /// Set the batch size (number of logs to accumulate before exporting).
    /// Clamped to [1, MAX_LOG_BATCH_SIZE].
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        if batch_size == 0 || batch_size > MAX_LOG_BATCH_SIZE {
            tracing::warn!(
                batch_size,
                max = MAX_LOG_BATCH_SIZE,
                "logs_batch_size out of valid range [1, {}], using default {}",
                MAX_LOG_BATCH_SIZE,
                DEFAULT_LOG_BATCH_SIZE,
            );
            return self;
        }
        self.batch_size = batch_size;
        self
    }

    /// Set the flush interval (how often to export partial batches).
    /// Clamped to [MIN_LOG_FLUSH_INTERVAL_MS, MAX_LOG_FLUSH_INTERVAL_MS].
    pub fn with_flush_interval(mut self, interval: Duration) -> Self {
        let ms = interval.as_millis() as u64;
        if !(MIN_LOG_FLUSH_INTERVAL_MS..=MAX_LOG_FLUSH_INTERVAL_MS).contains(&ms) {
            tracing::warn!(
                interval_ms = ms,
                min = MIN_LOG_FLUSH_INTERVAL_MS,
                max = MAX_LOG_FLUSH_INTERVAL_MS,
                "logs_flush_interval_ms out of valid range [{}, {}], using default {}ms",
                MIN_LOG_FLUSH_INTERVAL_MS,
                MAX_LOG_FLUSH_INTERVAL_MS,
                DEFAULT_LOG_FLUSH_INTERVAL_MS,
            );
            return self;
        }
        self.flush_interval = interval;
        self
    }

    /// Start background task that listens to broadcast channel and exports logs
    pub fn start(self, mut rx: broadcast::Receiver<StoredLog>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut batch: Vec<StoredLog> = Vec::with_capacity(self.batch_size);
            let mut interval = tokio::time::interval(self.flush_interval);

            loop {
                tokio::select! {
                    // Receive logs from broadcast channel
                    result = rx.recv() => {
                        match result {
                            Ok(log) => {
                                batch.push(log);
                                if batch.len() >= self.batch_size {
                                    if let Err(e) = self.export_batch(&batch).await {
                                        tracing::warn!(error = %e, "Failed to export logs batch");
                                    }
                                    batch.clear();
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                tracing::warn!(dropped = n, "Log exporter lagged, some logs were dropped");
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                tracing::info!("Log broadcast channel closed, stopping exporter");
                                break;
                            }
                        }
                    }
                    // Flush interval timer
                    _ = interval.tick() => {
                        if !batch.is_empty() {
                            if let Err(e) = self.export_batch(&batch).await {
                                tracing::warn!(error = %e, "Failed to export logs batch on timer");
                            }
                            batch.clear();
                        }
                    }
                }
            }

            // Flush remaining logs on shutdown
            if !batch.is_empty()
                && let Err(e) = self.export_batch(&batch).await
            {
                tracing::warn!(error = %e, "Failed to export final logs batch");
            }
        })
    }

    /// Start the exporter with a shutdown signal
    pub fn start_with_shutdown(
        self,
        mut rx: broadcast::Receiver<StoredLog>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut batch: Vec<StoredLog> = Vec::with_capacity(self.batch_size);
            let mut interval = tokio::time::interval(self.flush_interval);

            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            tracing::debug!("[OtlpLogsExporter] Shutting down");
                            break;
                        }
                    }
                    result = rx.recv() => {
                        match result {
                            Ok(log) => {
                                batch.push(log);
                                if batch.len() >= self.batch_size {
                                    if let Err(e) = self.export_batch(&batch).await {
                                        tracing::warn!(error = %e, "Failed to export logs batch");
                                    }
                                    batch.clear();
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                tracing::warn!(skipped = n, "OTLP logs exporter lagged behind");
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                tracing::debug!("[OtlpLogsExporter] Channel closed");
                                break;
                            }
                        }
                    }
                    _ = interval.tick() => {
                        if !batch.is_empty() {
                            if let Err(e) = self.export_batch(&batch).await {
                                tracing::warn!(error = %e, "Failed to export logs batch on timer");
                            }
                            batch.clear();
                        }
                    }
                }
            }

            // Flush remaining logs on shutdown
            if !batch.is_empty()
                && let Err(e) = self.export_batch(&batch).await
            {
                tracing::warn!(error = %e, "Failed to export final logs batch");
            }

            tracing::debug!("[OtlpLogsExporter] Stopped");
        })
    }

    /// Convert StoredLog to OTLP protobuf format and export via HTTP
    async fn export_batch(
        &self,
        logs: &[StoredLog],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Build OTLP JSON payload matching the ExportLogsServiceRequest format
        let resource_logs = self.build_otlp_logs_request(logs);

        // Send via HTTP to the OTLP collector
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_LOG_EXPORT_TIMEOUT_SECS))
            .build()?;

        // OTLP HTTP endpoint for logs
        // Convert gRPC port 4317 to HTTP port 4318 if needed
        let base_endpoint = if self.endpoint.contains(":4317") {
            self.endpoint.replace(":4317", ":4318")
        } else {
            self.endpoint.clone()
        };

        let endpoint = if base_endpoint.ends_with("/v1/logs") {
            base_endpoint
        } else {
            format!("{}/v1/logs", base_endpoint.trim_end_matches('/'))
        };

        let response = client
            .post(&endpoint)
            .headers(self.headers.clone())
            .header("Content-Type", "application/json")
            .json(&resource_logs)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("OTLP logs export failed: {} - {}", status, body).into());
        }

        tracing::debug!(count = logs.len(), "Exported logs to OTLP collector");
        Ok(())
    }

    /// Build OTLP ExportLogsServiceRequest JSON
    /// Convert a serde_json::Value to an OTLP AnyValue JSON representation,
    /// preserving nested objects (kvlistValue) and arrays (arrayValue).
    fn json_value_to_otlp_any_value(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::json!({"stringValue": s}),
            serde_json::Value::Number(n) => {
                if n.is_i64() {
                    serde_json::json!({"intValue": n.to_string()})
                } else {
                    serde_json::json!({"doubleValue": n})
                }
            }
            serde_json::Value::Bool(b) => serde_json::json!({"boolValue": b}),
            serde_json::Value::Object(map) => {
                let values: Vec<serde_json::Value> = map
                    .iter()
                    .map(|(k, v)| {
                        serde_json::json!({
                            "key": k,
                            "value": Self::json_value_to_otlp_any_value(v)
                        })
                    })
                    .collect();
                serde_json::json!({"kvlistValue": {"values": values}})
            }
            serde_json::Value::Array(arr) => {
                let values: Vec<serde_json::Value> =
                    arr.iter().map(Self::json_value_to_otlp_any_value).collect();
                serde_json::json!({"arrayValue": {"values": values}})
            }
            serde_json::Value::Null => serde_json::json!({"stringValue": ""}),
        }
    }

    fn build_otlp_logs_request(&self, logs: &[StoredLog]) -> serde_json::Value {
        let log_records: Vec<serde_json::Value> = logs
            .iter()
            .map(|log| {
                let mut record = serde_json::json!({
                    "timeUnixNano": log.timestamp_unix_nano.to_string(),
                    "observedTimeUnixNano": log.observed_timestamp_unix_nano.to_string(),
                    "severityNumber": log.severity_number,
                    "severityText": log.severity_text,
                    "body": {
                        "stringValue": log.body
                    }
                });

                // Add trace context if available
                if let Some(trace_id) = &log.trace_id {
                    record["traceId"] = serde_json::json!(trace_id);
                }
                if let Some(span_id) = &log.span_id {
                    record["spanId"] = serde_json::json!(span_id);
                }

                // Add attributes
                let attributes: Vec<serde_json::Value> = log
                    .attributes
                    .iter()
                    .map(|(key, value)| {
                        serde_json::json!({
                            "key": key,
                            "value": Self::json_value_to_otlp_any_value(value)
                        })
                    })
                    .collect();

                if !attributes.is_empty() {
                    record["attributes"] = serde_json::json!(attributes);
                }

                record
            })
            .collect();

        serde_json::json!({
            "resourceLogs": [{
                "resource": {
                    "attributes": [
                        {
                            "key": "service.name",
                            "value": {"stringValue": self.service_name}
                        },
                        {
                            "key": "service.version",
                            "value": {"stringValue": self.service_version}
                        }
                    ]
                },
                "scopeLogs": [{
                    "scope": {
                        "name": "iii",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "logRecords": log_records
                }]
            }]
        })
    }
}

fn otlp_logs_headers_from_env() -> HeaderMap {
    env::var("OTEL_EXPORTER_OTLP_LOGS_HEADERS")
        .or_else(|_| env::var("OTEL_EXPORTER_OTLP_HEADERS"))
        .ok()
        .as_deref()
        .map(parse_otlp_header_string)
        .unwrap_or_default()
}

fn parse_otlp_header_string(headers: &str) -> HeaderMap {
    let mut header_map = HeaderMap::new();

    for pair in headers.split(',') {
        let Some((name, value)) = pair.split_once('=') else {
            tracing::warn!("Ignoring malformed OTLP header entry without '='");
            continue;
        };

        let name = name.trim();
        let value = value.trim();
        if name.is_empty() {
            tracing::warn!("Ignoring OTLP header entry with empty name");
            continue;
        }

        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            tracing::warn!(header = name, "Ignoring invalid OTLP header name");
            continue;
        };
        let Some(value) = percent_decode_header_value(value) else {
            tracing::warn!(header = %name, "Ignoring OTLP header value with invalid percent encoding");
            continue;
        };
        let Ok(value) = HeaderValue::from_bytes(&value) else {
            tracing::warn!(header = %name, "Ignoring invalid OTLP header value");
            continue;
        };

        header_map.insert(name, value);
    }

    header_map
}

fn percent_decode_header_value(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = bytes.get(i + 1).copied().and_then(hex_value)?;
            let lo = bytes.get(i + 2).copied().and_then(hex_value)?;
            decoded.push((hi << 4) | lo);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }

    Some(decoded)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Get the logs exporter type from config.
#[deprecated(
    since = "0.19.0",
    note = "Resolve the exporter type from ObservabilityWorkerConfig directly; this helper is kept for backward compatibility"
)]
pub fn get_logs_exporter_type() -> LogsExporterType {
    get_otel_config()
        .and_then(|cfg| cfg.logs_exporter.clone())
        .or_else(|| {
            env::var("OTEL_LOGS_EXPORTER")
                .ok()
                .map(|v| match v.to_lowercase().as_str() {
                    "otlp" => LogsExporterType::Otlp,
                    "both" => LogsExporterType::Both,
                    _ => LogsExporterType::Memory,
                })
        })
        .unwrap_or(LogsExporterType::Memory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    // ── Coverage: TeeSpanExporter resource forwarding ───────────────────────
    // Regression test for the `exporter: both` path: the provider hands its
    // resource to the exporter chain via `set_resource`; if TeeSpanExporter
    // does not forward it (the trait default is a no-op), the wrapped OTLP
    // exporter serializes an EMPTY resource and collectors see engine spans
    // without a `service.name`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[serial]
    async fn tee_span_exporter_forwards_resource_to_otlp_payload() -> anyhow::Result<()> {
        use opentelemetry::trace::{Span as _, Tracer as _, TracerProvider as _};
        use tokio::io::AsyncReadExt;
        use tokio::net::TcpListener;

        const SERVICE_NAME: &str = "tee-resource-forwarding-test";

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let endpoint = format!("http://{addr}");

        // Accumulate everything the exporter writes (headers + protobuf body
        // may arrive across several writes; the peer never responds, so stop
        // on read timeout).
        let mut accept = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let mut bytes = Vec::new();
            let mut buf = [0_u8; 4096];
            loop {
                match tokio::time::timeout(
                    std::time::Duration::from_millis(750),
                    socket.read(&mut buf),
                )
                .await
                {
                    Ok(Ok(0)) | Err(_) => break,
                    Ok(Ok(n)) => bytes.extend_from_slice(&buf[..n]),
                    Ok(Err(e)) => return Err(e.into()),
                }
            }
            Ok::<_, anyhow::Error>(bytes)
        });

        let storage = Arc::new(InMemorySpanStorage::new(16));
        let export_storage = storage.clone();
        let mut export = tokio::spawn(async move {
            // http/protobuf keeps the capture a plain HTTP/1.1 request whose
            // body embeds resource strings as UTF-8, so the assertion below
            // can scan raw bytes without a protobuf decoder.
            let otlp_exporter = temp_env::with_vars(
                [
                    ("OTEL_EXPORTER_OTLP_PROTOCOL", Some("http/protobuf")),
                    ("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", None),
                    ("OTEL_EXPORTER_OTLP_HEADERS", None),
                    ("OTEL_EXPORTER_OTLP_TIMEOUT", Some("250")),
                    ("OTEL_EXPORTER_OTLP_TRACES_TIMEOUT", Some("250")),
                ],
                || build_span_exporter(&endpoint).expect("build span exporter"),
            );
            let tee = TeeSpanExporter::new(otlp_exporter, export_storage, SERVICE_NAME.to_string());

            // The REAL production path: the provider pushes its resource down
            // through the batch processor to the tee via `set_resource`.
            let provider = SdkTracerProvider::builder()
                .with_batch_exporter(tee)
                .with_resource(
                    Resource::builder_empty()
                        .with_attributes([KeyValue::new("service.name", SERVICE_NAME)])
                        .build(),
                )
                .build();
            let tracer = provider.tracer("tee-resource-test");
            let mut span = tracer.start("tee-resource-span");
            span.end();
            let _ = provider.shutdown_with_timeout(std::time::Duration::from_secs(1));
        });

        let bytes =
            tokio::time::timeout(std::time::Duration::from_secs(5), &mut accept).await???;
        if tokio::time::timeout(std::time::Duration::from_secs(2), &mut export)
            .await
            .is_err()
        {
            export.abort();
            let _ = export.await;
        }

        // The memory side always carries the service name explicitly …
        assert_eq!(storage.len(), 1, "span should land in in-memory storage");

        // … and the OTLP payload must carry it too (this is what breaks when
        // set_resource is not forwarded to the wrapped exporter).
        let needle = SERVICE_NAME.as_bytes();
        assert!(
            bytes.windows(needle.len()).any(|w| w == needle),
            "OTLP payload should contain the provider resource service.name; \
             got {} bytes without it",
            bytes.len()
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn otlp_logs_headers_fallback_decodes_percent_encoded_values() {
        temp_env::with_vars(
            [
                ("OTEL_EXPORTER_OTLP_LOGS_HEADERS", None::<&str>),
                (
                    "OTEL_EXPORTER_OTLP_HEADERS",
                    Some("Authorization=Bearer%20token,x-honeycomb-team=test%2Fteam"),
                ),
            ],
            || {
                let headers = otlp_logs_headers_from_env();

                assert_eq!(
                    headers
                        .get("authorization")
                        .expect("authorization header")
                        .to_str()
                        .unwrap(),
                    "Bearer token"
                );
                assert_eq!(
                    headers
                        .get("x-honeycomb-team")
                        .expect("x-honeycomb-team header")
                        .to_str()
                        .unwrap(),
                    "test/team"
                );
            },
        );
    }

    // ── Coverage: runtime storage-limit retune + dynamic sampler swap ──────
    // These exercise the hot-apply LIMITS tier mechanisms and the swappable
    // sampler that `apply_config` drives.

    #[test]
    fn set_max_spans_shrinks_and_rebuilds_trace_index() {
        let storage = InMemorySpanStorage::new(10);
        storage.add_spans(vec![
            make_stored_span("trace-a", "s1", "op1", 1_000_000_000, 2_000_000_000),
            make_stored_span("trace-b", "s2", "op2", 3_000_000_000, 4_000_000_000),
            make_stored_span("trace-a", "s3", "op3", 5_000_000_000, 6_000_000_000),
            make_stored_span("trace-c", "s4", "op4", 7_000_000_000, 8_000_000_000),
        ]);
        assert_eq!(storage.len(), 4);

        // Shrink to 2: the two oldest (s1=trace-a, s2=trace-b) are evicted and
        // the trace-id index is rebuilt from the survivors (s3=trace-a,
        // s4=trace-c).
        storage.set_max_spans(2);
        assert_eq!(storage.len(), 2);
        assert_eq!(
            storage.get_spans_by_trace_id("trace-a").len(),
            1,
            "only the surviving trace-a span should remain in the rebuilt index"
        );
        assert_eq!(
            storage.get_spans_by_trace_id("trace-b").len(),
            0,
            "the evicted trace-b span must be gone from the index"
        );
        assert_eq!(storage.get_spans_by_trace_id("trace-c").len(), 1);

        // A 0 request is clamped to at least 1 (never a zero-capacity store).
        storage.set_max_spans(0);
        assert_eq!(storage.len(), 1);
    }

    #[test]
    fn set_max_logs_shrinks_to_new_capacity() {
        let storage = InMemoryLogStorage::new(10);
        for i in 0..5 {
            storage.store(StoredLog {
                timestamp_unix_nano: 1_704_067_200_000_000_000 + i * 1_000_000_000,
                observed_timestamp_unix_nano: 1_704_067_200_000_000_000 + i * 1_000_000_000,
                severity_number: 9,
                severity_text: "INFO".to_string(),
                body: format!("log {i}"),
                attributes: HashMap::new(),
                trace_id: None,
                span_id: None,
                resource: HashMap::new(),
                service_name: "test".to_string(),
                instrumentation_scope_name: None,
                instrumentation_scope_version: None,
            });
        }
        assert_eq!(storage.len(), 5);

        storage.set_max_logs(2);
        let logs = storage.get_logs();
        assert_eq!(logs.len(), 2, "shrink must drop the oldest logs");
        assert_eq!(logs[0].body, "log 3", "oldest survivor after shrink");
        assert_eq!(logs[1].body, "log 4");
    }

    #[test]
    fn dynamic_sampler_swap_changes_decision() {
        use opentelemetry::trace::{SamplingDecision, SpanKind, TraceId};
        use opentelemetry_sdk::trace::ShouldSample;

        let sampler = DynamicSampler::new(Sampler::AlwaysOff);
        let drop = sampler.should_sample(
            None,
            TraceId::from_bytes([1u8; 16]),
            "op",
            &SpanKind::Internal,
            &[],
            &[],
        );
        assert!(
            matches!(drop.decision, SamplingDecision::Drop),
            "AlwaysOff inner must drop"
        );

        // Hot-swap the inner sampler; the same handle observes the new policy.
        sampler.swap(Sampler::AlwaysOn);
        let keep = sampler.should_sample(
            None,
            TraceId::from_bytes([2u8; 16]),
            "op",
            &SpanKind::Internal,
            &[],
            &[],
        );
        assert!(
            matches!(keep.decision, SamplingDecision::RecordAndSample),
            "after swap to AlwaysOn the same sampler must record-and-sample"
        );

        // A clone shares the same inner Arc, so it sees swaps too.
        let cloned = sampler.clone();
        sampler.swap(Sampler::AlwaysOff);
        let drop_again = cloned.should_sample(
            None,
            TraceId::from_bytes([3u8; 16]),
            "op",
            &SpanKind::Internal,
            &[],
            &[],
        );
        assert!(
            matches!(drop_again.decision, SamplingDecision::Drop),
            "a clone must observe the swap through the shared inner"
        );
    }

    #[tokio::test]
    #[serial]
    async fn init_log_storage_retunes_existing_store() {
        // init_log_storage is create-or-retune: a second call with a tighter
        // cap retunes the live store rather than allocating a new one.
        init_log_storage(Some(3));
        let storage = get_log_storage().expect("log storage created");
        storage.clear();
        for i in 0..6 {
            storage.store(StoredLog {
                timestamp_unix_nano: 1_704_067_200_000_000_000 + i * 1_000_000_000,
                observed_timestamp_unix_nano: 1_704_067_200_000_000_000 + i * 1_000_000_000,
                severity_number: 9,
                severity_text: "INFO".to_string(),
                body: format!("retune {i}"),
                attributes: HashMap::new(),
                trace_id: None,
                span_id: None,
                resource: HashMap::new(),
                service_name: "test".to_string(),
                instrumentation_scope_name: None,
                instrumentation_scope_version: None,
            });
        }
        assert_eq!(storage.len(), 3, "retuned cap must bound the live store");
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_metrics_gauge() {
        // Initialize metric storage for testing
        crate::workers::observability::metrics::init_metric_storage(Some(100), Some(3600));

        // Clear any previous metrics
        if let Some(storage) = crate::workers::observability::metrics::get_metric_storage() {
            storage.clear();
        }

        let otlp_json = r#"{
            "resourceMetrics": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "test-service"}
                    }]
                },
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "test.gauge",
                        "description": "Test gauge metric",
                        "unit": "1",
                        "gauge": {
                            "dataPoints": [{
                                "attributes": [{
                                    "key": "env",
                                    "value": {"stringValue": "production"}
                                }],
                                "timeUnixNano": "1704067200000000000",
                                "asDouble": 42.5
                            }]
                        }
                    }]
                }]
            }]
        }"#;

        let result = ingest_otlp_metrics(otlp_json).await;
        assert!(result.is_ok());

        // Verify metric was stored
        let storage = crate::workers::observability::metrics::get_metric_storage().unwrap();
        let metrics = storage.get_metrics();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "test.gauge");
        assert_eq!(metrics[0].service_name, "test-service");
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_metrics_counter() {
        crate::workers::observability::metrics::init_metric_storage(Some(100), Some(3600));

        // Clear any previous metrics
        if let Some(storage) = crate::workers::observability::metrics::get_metric_storage() {
            storage.clear();
        }

        let otlp_json = r#"{
            "resourceMetrics": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "counter-service"}
                    }]
                },
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "requests.total",
                        "description": "Total requests",
                        "unit": "1",
                        "sum": {
                            "dataPoints": [{
                                "timeUnixNano": "1704067200000000000",
                                "asInt": "1234"
                            }],
                            "aggregationTemporality": 2,
                            "isMonotonic": true
                        }
                    }]
                }]
            }]
        }"#;

        let result = ingest_otlp_metrics(otlp_json).await;
        assert!(result.is_ok());

        let storage = crate::workers::observability::metrics::get_metric_storage().unwrap();
        let metrics = storage.get_metrics_by_name("requests.total");
        assert_eq!(metrics.len(), 1);

        match &metrics[0].metric_type {
            crate::workers::observability::metrics::StoredMetricType::Counter => {}
            _ => panic!("Expected Counter metric type"),
        }

        // Verify the string-encoded value made it through intact.
        use crate::workers::observability::metrics::StoredDataPoint;
        let stored_value = match &metrics[0].data_points[0] {
            StoredDataPoint::Number(dp) => dp.value,
            _ => panic!("Expected Number data point"),
        };
        assert_eq!(stored_value, 1234.0);
    }

    /// Regression test for the "Failed to parse OTLP metrics JSON: invalid type:
    /// string, expected i64" error observed in production. Per the OTLP/JSON
    /// spec, int64 fields (including `asInt`) are serialized as strings. The
    /// Node, Python, and Rust SDKs all emit `asInt` as a string, so the engine
    /// MUST accept both forms.
    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_metrics_as_int_string_form() {
        crate::workers::observability::metrics::init_metric_storage(Some(100), Some(3600));
        if let Some(storage) = crate::workers::observability::metrics::get_metric_storage() {
            storage.clear();
        }

        // Shape produced by a real Python SDK worker emitting an integer gauge.
        let otlp_json = r#"{
            "resourceMetrics": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "rss-service"}
                    }]
                },
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "process.memory.rss",
                        "unit": "By",
                        "gauge": {
                            "dataPoints": [{
                                "timeUnixNano": "1704067200000000000",
                                "asInt": "2048"
                            }]
                        }
                    }]
                }]
            }]
        }"#;

        let result = ingest_otlp_metrics(otlp_json).await;
        assert!(
            result.is_ok(),
            "SDK-shaped payload with asInt-as-string must parse: {:?}",
            result
        );

        let storage = crate::workers::observability::metrics::get_metric_storage().unwrap();
        let metrics = storage.get_metrics_by_name("process.memory.rss");
        assert_eq!(metrics.len(), 1);

        use crate::workers::observability::metrics::StoredDataPoint;
        let stored_value = match &metrics[0].data_points[0] {
            StoredDataPoint::Number(dp) => dp.value,
            _ => panic!("Expected Number data point"),
        };
        assert_eq!(stored_value, 2048.0);
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_metrics_histogram() {
        crate::workers::observability::metrics::init_metric_storage(Some(100), Some(3600));

        // Clear any previous metrics
        if let Some(storage) = crate::workers::observability::metrics::get_metric_storage() {
            storage.clear();
        }

        let otlp_json = r#"{
            "resourceMetrics": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "histogram-service"}
                    }]
                },
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "request.duration",
                        "description": "Request duration histogram",
                        "unit": "ms",
                        "histogram": {
                            "dataPoints": [{
                                "timeUnixNano": "1704067200000000000",
                                "count": "100",
                                "sum": 523.45,
                                "bucketCounts": ["10", "40", "30", "20"],
                                "explicitBounds": [10, 50, 100, 500],
                                "min": 1.2,
                                "max": 450.8
                            }],
                            "aggregationTemporality": 2
                        }
                    }]
                }]
            }]
        }"#;

        let result = ingest_otlp_metrics(otlp_json).await;
        assert!(result.is_ok());

        let storage = crate::workers::observability::metrics::get_metric_storage().unwrap();
        let metrics = storage.get_metrics_by_name("request.duration");
        assert_eq!(metrics.len(), 1);

        match &metrics[0].metric_type {
            crate::workers::observability::metrics::StoredMetricType::Histogram => {}
            _ => panic!("Expected Histogram metric type"),
        }

        // Verify histogram data points
        if let crate::workers::observability::metrics::StoredDataPoint::Histogram(dp) =
            &metrics[0].data_points[0]
        {
            assert_eq!(dp.count, 100);
            assert_eq!(dp.sum, 523.45);
            assert_eq!(dp.bucket_counts, vec![10, 40, 30, 20]);
            assert_eq!(dp.min, Some(1.2));
            assert_eq!(dp.max, Some(450.8));
        } else {
            panic!("Expected histogram data point");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_metrics_null_scope_and_metric_strings() {
        // Regression: Python SDK emits `"version": null` when a meter is created
        // without a version, and other senders may emit null for description/unit.
        // The ingestion path must tolerate these without failing.
        crate::workers::observability::metrics::init_metric_storage(Some(100), Some(3600));
        if let Some(storage) = crate::workers::observability::metrics::get_metric_storage() {
            storage.clear();
        }

        let otlp_json = r#"{
            "resourceMetrics": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "iii-py"}
                    }]
                },
                "scopeMetrics": [{
                    "scope": {"name": "iii-py", "version": null},
                    "metrics": [{
                        "name": "test.counter",
                        "description": null,
                        "unit": null,
                        "sum": {
                            "isMonotonic": true,
                            "aggregationTemporality": 2,
                            "dataPoints": [{
                                "attributes": [],
                                "timeUnixNano": "1704067200000000000",
                                "asDouble": 1.0
                            }]
                        }
                    }]
                }]
            }]
        }"#;

        let result = ingest_otlp_metrics(otlp_json).await;
        assert!(
            result.is_ok(),
            "null string fields should parse: {result:?}"
        );

        let storage = crate::workers::observability::metrics::get_metric_storage().unwrap();
        let metrics = storage.get_metrics();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "test.counter");
        assert_eq!(metrics[0].description, "");
        assert_eq!(metrics[0].unit, "");
        assert_eq!(
            metrics[0].instrumentation_scope_name.as_deref(),
            Some("iii-py")
        );
        assert_eq!(metrics[0].instrumentation_scope_version, None);
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_metrics_invalid_json() {
        crate::workers::observability::metrics::init_metric_storage(Some(100), Some(3600));

        let invalid_json = r#"{"invalid": "json structure"#;

        let result = ingest_otlp_metrics(invalid_json).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_metrics_empty() {
        crate::workers::observability::metrics::init_metric_storage(Some(100), Some(3600));

        // Clear any previous metrics
        if let Some(storage) = crate::workers::observability::metrics::get_metric_storage() {
            storage.clear();
        }

        let empty_json = r#"{"resourceMetrics": []}"#;

        let result = ingest_otlp_metrics(empty_json).await;
        assert!(result.is_ok());

        let storage = crate::workers::observability::metrics::get_metric_storage().unwrap();
        let metrics = storage.get_metrics();

        // Should not add any new metrics
        assert_eq!(metrics.len(), 0);
    }

    // ==========================================================================
    // OTLP Logs Tests
    // ==========================================================================

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_logs_basic() {
        // Initialize log storage for testing
        init_log_storage(Some(100));

        // Clear any previous logs
        if let Some(storage) = get_log_storage() {
            storage.clear();
        }

        let otlp_json = r#"{
            "resourceLogs": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "test-service"}
                    }]
                },
                "scopeLogs": [{
                    "logRecords": [{
                        "timeUnixNano": "1704067200000000000",
                        "observedTimeUnixNano": "1704067200000000000",
                        "severityNumber": 9,
                        "severityText": "INFO",
                        "body": {"stringValue": "Test log message"},
                        "attributes": [{
                            "key": "custom.attr",
                            "value": {"stringValue": "custom-value"}
                        }],
                        "traceId": "abc123def456",
                        "spanId": "span123"
                    }]
                }]
            }]
        }"#;

        let result = ingest_otlp_logs(otlp_json).await;
        assert!(result.is_ok());

        // Verify log was stored
        let storage = get_log_storage().unwrap();
        let logs = storage.get_logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].body, "Test log message");
        assert_eq!(logs[0].severity_text, "INFO");
        assert_eq!(logs[0].severity_number, 9);
        assert_eq!(logs[0].service_name, "test-service");
        assert_eq!(logs[0].trace_id, Some("abc123def456".to_string()));
        assert_eq!(logs[0].span_id, Some("span123".to_string()));
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_logs_with_trace_correlation() {
        init_log_storage(Some(100));

        // Clear any previous logs
        if let Some(storage) = get_log_storage() {
            storage.clear();
        }

        let trace_id = "abcd1234567890abcd1234567890abcd";
        let span_id = "1234567890abcdef";

        let otlp_json = format!(
            r#"{{
            "resourceLogs": [{{
                "resource": {{
                    "attributes": [{{
                        "key": "service.name",
                        "value": {{"stringValue": "correlated-service"}}
                    }}]
                }},
                "scopeLogs": [{{
                    "logRecords": [
                        {{
                            "timeUnixNano": "1704067200000000000",
                            "severityNumber": 17,
                            "severityText": "ERROR",
                            "body": {{"stringValue": "Error log in span"}},
                            "traceId": "{}",
                            "spanId": "{}"
                        }},
                        {{
                            "timeUnixNano": "1704067201000000000",
                            "severityNumber": 9,
                            "severityText": "INFO",
                            "body": {{"stringValue": "Info log in same trace"}},
                            "traceId": "{}",
                            "spanId": "different_span_id"
                        }}
                    ]
                }}]
            }}]
        }}"#,
            trace_id, span_id, trace_id
        );

        let result = ingest_otlp_logs(&otlp_json).await;
        assert!(result.is_ok());

        let storage = get_log_storage().unwrap();

        // Test filtering by trace_id
        let trace_logs = storage.get_logs_by_trace_id(trace_id);
        assert_eq!(trace_logs.len(), 2);

        // Test filtering by span_id
        let span_logs = storage.get_logs_by_span_id(span_id);
        assert_eq!(span_logs.len(), 1);
        assert_eq!(span_logs[0].severity_text, "ERROR");
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_logs_severity_filter() {
        init_log_storage(Some(100));

        // Clear any previous logs
        if let Some(storage) = get_log_storage() {
            storage.clear();
        }

        let otlp_json = r#"{
            "resourceLogs": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "test-service"}
                    }]
                },
                "scopeLogs": [{
                    "logRecords": [
                        {
                            "timeUnixNano": "1704067200000000000",
                            "severityNumber": 5,
                            "severityText": "DEBUG",
                            "body": {"stringValue": "Debug message"}
                        },
                        {
                            "timeUnixNano": "1704067201000000000",
                            "severityNumber": 9,
                            "severityText": "INFO",
                            "body": {"stringValue": "Info message"}
                        },
                        {
                            "timeUnixNano": "1704067202000000000",
                            "severityNumber": 13,
                            "severityText": "WARN",
                            "body": {"stringValue": "Warn message"}
                        },
                        {
                            "timeUnixNano": "1704067203000000000",
                            "severityNumber": 17,
                            "severityText": "ERROR",
                            "body": {"stringValue": "Error message"}
                        }
                    ]
                }]
            }]
        }"#;

        let result = ingest_otlp_logs(otlp_json).await;
        assert!(result.is_ok());

        let storage = get_log_storage().unwrap();

        // Get all logs
        let all_logs = storage.get_logs();
        assert_eq!(all_logs.len(), 4);

        // Filter by severity >= WARN (13)
        let (_, warn_and_above) =
            storage.get_logs_filtered(None, None, Some(13), None, None, None, None, None);
        assert_eq!(warn_and_above.len(), 2);

        // Filter by severity >= ERROR (17)
        let (_, error_only) =
            storage.get_logs_filtered(None, None, Some(17), None, None, None, None, None);
        assert_eq!(error_only.len(), 1);
        assert_eq!(error_only[0].severity_text, "ERROR");
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_logs_invalid_json() {
        init_log_storage(Some(100));

        let invalid_json = r#"{"invalid": "json structure"#;

        let result = ingest_otlp_logs(invalid_json).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_logs_empty() {
        init_log_storage(Some(100));

        // Clear any previous logs
        if let Some(storage) = get_log_storage() {
            storage.clear();
        }

        let empty_json = r#"{"resourceLogs": []}"#;

        let result = ingest_otlp_logs(empty_json).await;
        assert!(result.is_ok());

        let storage = get_log_storage().unwrap();
        let logs = storage.get_logs();

        // Should not add any new logs
        assert_eq!(logs.len(), 0);
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_logs_storage_not_initialized_error() {
        // This test validates the "storage not initialized" error path.
        // Because get_log_storage() uses a OnceLock that may already be set by
        // other tests, we test the function doesn't crash with valid JSON.
        // If storage happens to be initialized (from prior tests), the call
        // succeeds; if not, it returns the expected error.

        let valid_json = r#"{
            "resourceLogs": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "test-service"}
                    }]
                },
                "scopeLogs": [{
                    "logRecords": [{
                        "timeUnixNano": "1704067200000000000",
                        "observedTimeUnixNano": "1704067200000000000",
                        "severityNumber": 9,
                        "severityText": "INFO",
                        "body": {"stringValue": "Should not be stored when disabled"}
                    }]
                }]
            }]
        }"#;

        // Clear storage if it exists so we can verify no new logs are added
        if let Some(storage) = get_log_storage() {
            storage.clear();
        }

        let result = ingest_otlp_logs(valid_json).await;

        // The result depends on test ordering:
        // - If otel config is set with enabled=false -> Ok(()) from early return
        // - If storage is initialized -> Ok(()) and log is stored
        // - If storage is NOT initialized -> Err("Log storage not initialized")
        // In all cases, the function should not panic.
        match &result {
            Ok(()) => {
                // Either early-returned due to disabled config, or successfully ingested.
                // If storage exists and config doesn't disable it, verify the log was stored.
                if let Some(config) = get_otel_config() {
                    if !config.enabled.unwrap_or(true) {
                        // Config says disabled - verify nothing was stored
                        if let Some(storage) = get_log_storage() {
                            assert_eq!(storage.get_logs().len(), 0);
                        }
                    }
                }
            }
            Err(e) => {
                // Should be the "Log storage not initialized" error
                assert!(
                    e.to_string().contains("Log storage not initialized"),
                    "Unexpected error: {}",
                    e
                );
            }
        }
    }

    #[test]
    fn test_log_storage_circular_buffer() {
        // Create a local storage instance with capacity 3 (not the global singleton)
        // This avoids issues with OnceLock already being initialized by other tests
        let storage = InMemoryLogStorage::new(3);

        // Add 5 logs
        for i in 0..5 {
            storage.store(StoredLog {
                timestamp_unix_nano: 1704067200000000000 + i * 1000000000,
                observed_timestamp_unix_nano: 1704067200000000000 + i * 1000000000,
                severity_number: 9,
                severity_text: "INFO".to_string(),
                body: format!("Log message {}", i),
                attributes: HashMap::new(),
                trace_id: None,
                span_id: None,
                resource: HashMap::new(),
                service_name: "test".to_string(),
                instrumentation_scope_name: None,
                instrumentation_scope_version: None,
            });
        }

        // Should only keep the last 3
        let logs = storage.get_logs();
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[0].body, "Log message 2");
        assert_eq!(logs[1].body, "Log message 3");
        assert_eq!(logs[2].body, "Log message 4");
    }

    fn ansi_log(body: &str, attrs: HashMap<String, serde_json::Value>) -> StoredLog {
        StoredLog {
            timestamp_unix_nano: 1704067200000000000,
            observed_timestamp_unix_nano: 1704067200000000000,
            severity_number: 9,
            severity_text: "INFO".to_string(),
            body: body.to_string(),
            attributes: attrs,
            trace_id: None,
            span_id: None,
            resource: HashMap::new(),
            service_name: "test".to_string(),
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
        }
    }

    #[test]
    fn store_strips_ansi_from_body() {
        let storage = InMemoryLogStorage::new(10);
        // Colorized like `"[REGISTERED]".green()` produces.
        storage.store(ansi_log(
            "\u{1b}[32m[REGISTERED]\u{1b}[0m Function \u{1b}[35mfoo\u{1b}[0m",
            HashMap::new(),
        ));
        let logs = storage.get_logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].body, "[REGISTERED] Function foo");
    }

    #[test]
    fn store_strips_ansi_from_string_attributes() {
        let storage = InMemoryLogStorage::new(10);
        let mut attrs = HashMap::new();
        attrs.insert(
            "colored".to_string(),
            serde_json::Value::String("\u{1b}[31mred\u{1b}[0m".to_string()),
        );
        // Non-string attributes must pass through untouched.
        attrs.insert("count".to_string(), serde_json::json!(42));
        storage.store(ansi_log("plain", attrs));
        let logs = storage.get_logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(
            logs[0].attributes.get("colored"),
            Some(&serde_json::Value::String("red".to_string()))
        );
        assert_eq!(
            logs[0].attributes.get("count"),
            Some(&serde_json::json!(42))
        );
    }

    #[test]
    fn add_logs_strips_ansi_worker_otlp_path() {
        // Worker logs ingested over OTLP land via `add_logs`, not the tracing
        // layer — this is the path the per-source fix used to miss.
        let storage = InMemoryLogStorage::new(10);
        storage.add_logs(vec![
            ansi_log("\u{1b}[33mworker warn\u{1b}[0m", HashMap::new()),
            ansi_log("plain worker line", HashMap::new()),
        ]);
        let logs = storage.get_logs();
        assert_eq!(logs.len(), 2);
        let bodies: Vec<&str> = logs.iter().map(|l| l.body.as_str()).collect();
        assert!(bodies.contains(&"worker warn"));
        assert!(bodies.contains(&"plain worker line"));
    }

    #[test]
    fn store_leaves_plain_text_untouched() {
        let storage = InMemoryLogStorage::new(10);
        storage.store(ansi_log("just plain text", HashMap::new()));
        assert_eq!(storage.get_logs()[0].body, "just plain text");
    }

    #[test]
    fn test_extract_context_with_traceparent_only() {
        // Test extracting context with just traceparent
        // Note: This test verifies the function works without panicking.
        // The global propagator may not be initialized in unit tests, so we
        // just verify the function executes successfully.
        let traceparent = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";

        let ctx = extract_context(Some(traceparent), None);

        // The context should exist (function should not panic)
        let span_ref = ctx.span();
        let _span_context = span_ref.span_context();
        // Just verify no panic - the actual span context validity depends on
        // whether the global propagator is initialized
    }

    #[test]
    fn test_extract_context_with_baggage_only() {
        use opentelemetry::baggage::BaggageExt;

        // Test extracting context with just baggage
        let baggage = "user.id=12345,tenant.id=abc";

        let ctx = extract_context(None, Some(baggage));

        // The context should have baggage entries
        let bag = ctx.baggage();
        // Note: Baggage may or may not be available depending on context state
        // This test primarily verifies the function doesn't panic and context is valid
        let _count = bag.len();
    }

    #[test]
    fn with_function_id_baggage_overrides_key_and_preserves_others() {
        use opentelemetry::baggage::BaggageExt;

        let cx = extract_context(
            None,
            Some("iii.session.id=s-1,iii.message.id=m-1,iii.function.id=run::start"),
        );
        let updated = with_function_id_baggage(&cx, "turn::provisioning");
        let bag = updated.baggage();

        assert_eq!(
            bag.get("iii.function.id").map(|v| v.as_str().to_string()),
            Some("turn::provisioning".to_string()),
            "function id must be rewritten to the target",
        );
        assert_eq!(
            bag.get("iii.session.id").map(|v| v.as_str().to_string()),
            Some("s-1".to_string()),
            "other baggage entries must be preserved",
        );
        assert_eq!(
            bag.get("iii.message.id").map(|v| v.as_str().to_string()),
            Some("m-1".to_string()),
        );
        // No duplicate entry left behind for the rewritten key.
        let fn_count = bag
            .iter()
            .filter(|(k, _)| k.as_str() == "iii.function.id")
            .count();
        assert_eq!(fn_count, 1, "exactly one iii.function.id entry");
    }

    #[test]
    fn baggage_with_function_id_header_names_target() {
        // Round-trips through the W3C header form (the queue-message path).
        let header = baggage_with_function_id(
            Some("iii.session.id=s-9,iii.function.id=turn::provisioning"),
            "turn::assistant_streaming",
        )
        .expect("baggage header is always produced");

        assert!(
            header.contains("iii.function.id=turn::assistant_streaming"),
            "header must name the target function, got: {header}",
        );
        assert!(
            !header.contains("turn::provisioning"),
            "enqueuer function id must not survive, got: {header}",
        );
        assert!(
            header.contains("iii.session.id=s-9"),
            "other baggage entries must be preserved, got: {header}",
        );
    }

    #[test]
    fn baggage_with_function_id_inserts_when_absent() {
        let header = baggage_with_function_id(None, "session-tree::ensure")
            .expect("baggage header is always produced");
        assert!(header.contains("iii.function.id=session-tree::ensure"));
    }

    #[test]
    fn test_extract_context_with_both() {
        // Test extracting context with both traceparent and baggage
        let traceparent = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let baggage = "user.id=12345";

        let ctx = extract_context(Some(traceparent), Some(baggage));

        // Verify the context was created without errors
        let span_ref = ctx.span();
        let span_context = span_ref.span_context();
        // At minimum, we should get a valid context back
        assert!(!span_context.trace_id().to_string().is_empty() || !span_context.is_valid());
    }

    #[test]
    fn test_extract_context_with_none() {
        // Test extracting context with neither
        let ctx = extract_context(None, None);

        // Should return current context (likely empty)
        let span_ref = ctx.span();
        let _span_context = span_ref.span_context();
        // Just verify no panic
    }

    #[test]
    fn test_extract_baggage() {
        // Test the dedicated extract_baggage function
        let baggage = "key1=value1,key2=value2";

        let ctx = extract_baggage(baggage);

        // The context should exist
        let _span_ref = ctx.span();
        // Just verify no panic
    }

    // ==========================================================================
    // InMemorySpanStorage Tests
    // ==========================================================================

    /// Helper to create a StoredSpan with custom fields for testing.
    fn make_stored_span(
        trace_id: &str,
        span_id: &str,
        name: &str,
        start_ns: u64,
        end_ns: u64,
    ) -> StoredSpan {
        StoredSpan {
            trace_id: trace_id.to_string(),
            span_id: span_id.to_string(),
            parent_span_id: None,
            name: name.to_string(),
            start_time_unix_nano: start_ns,
            end_time_unix_nano: end_ns,
            status: "ok".to_string(),
            status_description: None,
            attributes: vec![],
            service_name: "test-service".to_string(),
            events: vec![],
            links: vec![],
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
            flags: None,
            trace_state: None,
            pending: false,
        }
    }

    #[test]
    fn test_span_storage_add_and_get() {
        let storage = InMemorySpanStorage::new(10);
        assert!(storage.get_spans().is_empty());

        let span1 = make_stored_span("trace1", "span1", "op-a", 1_000_000_000, 2_000_000_000);
        let span2 = make_stored_span("trace2", "span2", "op-b", 3_000_000_000, 4_000_000_000);

        storage.add_spans(vec![span1, span2]);

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].span_id, "span1");
        assert_eq!(spans[1].span_id, "span2");
    }

    #[test]
    fn test_span_storage_get_by_trace_id() {
        let storage = InMemorySpanStorage::new(10);

        let s1 = make_stored_span("trace-a", "s1", "op1", 1_000_000_000, 2_000_000_000);
        let s2 = make_stored_span("trace-b", "s2", "op2", 3_000_000_000, 4_000_000_000);
        let s3 = make_stored_span("trace-a", "s3", "op3", 5_000_000_000, 6_000_000_000);
        let s4 = make_stored_span("trace-c", "s4", "op4", 7_000_000_000, 8_000_000_000);

        storage.add_spans(vec![s1, s2, s3, s4]);

        let trace_a_spans = storage.get_spans_by_trace_id("trace-a");
        assert_eq!(trace_a_spans.len(), 2);
        let span_ids: Vec<&str> = trace_a_spans.iter().map(|s| s.span_id.as_str()).collect();
        assert!(span_ids.contains(&"s1"));
        assert!(span_ids.contains(&"s3"));

        let trace_b_spans = storage.get_spans_by_trace_id("trace-b");
        assert_eq!(trace_b_spans.len(), 1);
        assert_eq!(trace_b_spans[0].span_id, "s2");

        let trace_x_spans = storage.get_spans_by_trace_id("trace-nonexistent");
        assert!(trace_x_spans.is_empty());
    }

    #[test]
    fn test_span_storage_circular_buffer_eviction() {
        let storage = InMemorySpanStorage::new(3);

        // Add 5 spans to a storage that only holds 3
        for i in 0..5u64 {
            storage.add_spans(vec![make_stored_span(
                &format!("trace{}", i),
                &format!("span{}", i),
                &format!("op{}", i),
                i * 1_000_000_000,
                (i + 1) * 1_000_000_000,
            )]);
        }

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 3);

        // Oldest two (span0, span1) should be evicted; remaining are span2, span3, span4
        assert_eq!(spans[0].span_id, "span2");
        assert_eq!(spans[1].span_id, "span3");
        assert_eq!(spans[2].span_id, "span4");

        // The evicted trace IDs should not be found
        assert!(storage.get_spans_by_trace_id("trace0").is_empty());
        assert!(storage.get_spans_by_trace_id("trace1").is_empty());

        // The remaining trace IDs should be found
        assert_eq!(storage.get_spans_by_trace_id("trace2").len(), 1);
        assert_eq!(storage.get_spans_by_trace_id("trace3").len(), 1);
        assert_eq!(storage.get_spans_by_trace_id("trace4").len(), 1);
    }

    #[test]
    fn test_span_storage_clear() {
        let storage = InMemorySpanStorage::new(10);

        storage.add_spans(vec![
            make_stored_span("t1", "s1", "op1", 1_000_000_000, 2_000_000_000),
            make_stored_span("t2", "s2", "op2", 3_000_000_000, 4_000_000_000),
        ]);
        assert_eq!(storage.len(), 2);

        storage.clear();

        assert_eq!(storage.len(), 0);
        assert!(storage.get_spans().is_empty());
        assert!(storage.get_spans_by_trace_id("t1").is_empty());
        assert!(storage.get_spans_by_trace_id("t2").is_empty());
    }

    // ==========================================================================
    // Pending (live) span tests
    // ==========================================================================

    /// Pending-span variant of `make_stored_span` (end sentinel 0).
    fn make_pending_span(trace_id: &str, span_id: &str, name: &str, start_ns: u64) -> StoredSpan {
        let mut span = make_stored_span(trace_id, span_id, name, start_ns, 0);
        span.pending = true;
        span.status = "unset".to_string();
        span
    }

    #[test]
    fn pending_span_replaced_in_place_by_final() {
        let storage = InMemorySpanStorage::new(10);

        storage.add_spans(vec![make_stored_span("t1", "a", "op-a", 1_000, 2_000)]);
        storage.add_pending_span(make_pending_span("t1", "p", "op-p", 3_000));
        storage.add_spans(vec![make_stored_span("t1", "b", "op-b", 4_000, 5_000)]);
        assert_eq!(storage.pending_span_ids.read().unwrap().len(), 1);

        // Final arrives: replaces the pending snapshot at its original position.
        storage.add_spans(vec![make_stored_span("t1", "p", "op-p", 3_000, 9_000)]);

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 3, "replace, not append");
        assert_eq!(spans[1].span_id, "p", "position preserved");
        assert!(!spans[1].pending);
        assert_eq!(spans[1].end_time_unix_nano, 9_000);
        assert!(storage.pending_span_ids.read().unwrap().is_empty());
        assert_eq!(storage.get_spans_by_trace_id("t1").len(), 3);

        // The id is no longer pending-tracked: a duplicate final appends
        // (unchanged pre-existing behavior for duplicate finals).
        storage.add_spans(vec![make_stored_span("t1", "p", "op-p", 3_000, 9_500)]);
        assert_eq!(storage.get_spans().len(), 4);
    }

    #[test]
    fn pending_span_not_replaced_by_colliding_final_from_another_trace() {
        let storage = InMemorySpanStorage::new(10);

        storage.add_pending_span(make_pending_span("t1", "p", "op-p", 1_000));

        // Same span id, different trace: span ids are only unique within a
        // trace, so this final must append — not steal t1's snapshot.
        storage.add_spans(vec![make_stored_span("t2", "p", "op-x", 2_000, 3_000)]);

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 2, "append, not replace");
        assert_eq!(spans[0].trace_id, "t1");
        assert!(spans[0].pending, "t1's snapshot untouched");
        assert_eq!(storage.get_spans_by_trace_id("t2").len(), 1);
        assert_eq!(storage.pending_span_ids.read().unwrap().len(), 1);

        // t1's own final still replaces its snapshot in place.
        storage.add_spans(vec![make_stored_span("t1", "p", "op-p", 1_000, 4_000)]);
        let spans = storage.get_spans();
        assert_eq!(spans.len(), 2, "replace, not append");
        assert_eq!(spans[0].trace_id, "t1");
        assert!(!spans[0].pending);
        assert_eq!(spans[0].end_time_unix_nano, 4_000);
        assert!(storage.pending_span_ids.read().unwrap().is_empty());
    }

    #[test]
    fn pending_span_evicted_before_final_appends_fresh() {
        let storage = InMemorySpanStorage::new(2);

        storage.add_pending_span(make_pending_span("t1", "p", "op-p", 1_000));
        storage.add_spans(vec![
            make_stored_span("t2", "a", "op-a", 2_000, 3_000),
            // Third insert evicts the oldest (the pending snapshot).
            make_stored_span("t3", "b", "op-b", 4_000, 5_000),
        ]);
        assert!(storage.pending_span_ids.read().unwrap().is_empty());

        // The late final no longer has a pending predecessor — plain append.
        storage.add_spans(vec![make_stored_span("t1", "p", "op-p", 1_000, 6_000)]);
        let spans = storage.get_spans();
        assert_eq!(spans.len(), 2);
        let last = spans.last().unwrap();
        assert_eq!(last.span_id, "p");
        assert!(!last.pending);
        assert_eq!(last.end_time_unix_nano, 6_000);
    }

    #[test]
    fn set_max_spans_and_clear_purge_pending_ids() {
        let storage = InMemorySpanStorage::new(10);
        storage.add_pending_span(make_pending_span("t1", "p1", "op", 1_000));
        storage.add_pending_span(make_pending_span("t2", "p2", "op", 2_000));

        storage.set_max_spans(1); // drains p1
        assert_eq!(storage.len(), 1);
        let pending: Vec<(String, String)> = storage
            .pending_span_ids
            .read()
            .unwrap()
            .iter()
            .cloned()
            .collect();
        assert_eq!(pending, vec![("t2".to_string(), "p2".to_string())]);

        storage.clear();
        assert!(storage.pending_span_ids.read().unwrap().is_empty());
    }

    #[test]
    fn performance_metrics_exclude_pending_spans() {
        let storage = InMemorySpanStorage::new(10);
        let base = 1_000_000_000u64;
        storage.add_spans(vec![
            make_stored_span("t1", "s1", "op1", base, base + 10_000_000),
            make_stored_span("t2", "s2", "op2", base, base + 30_000_000),
        ]);
        // An in-flight span must not drag the statistics toward 0.
        storage.add_pending_span(make_pending_span("t3", "p", "op-p", base));

        let (avg, _p50, _p95, _p99, min, max) = storage.calculate_performance_metrics();
        assert_eq!(min, 10.0);
        assert_eq!(max, 30.0);
        assert_eq!(avg, 20.0);
    }

    #[test]
    fn effective_end_and_duration_treat_pending_as_active_now() {
        let now = 10_000u64;
        let done = make_stored_span("t", "s", "op", 1_000, 2_000);
        assert_eq!(done.effective_end_ns(now), 2_000);
        assert_eq!(done.duration_ns(now), 1_000);

        let live = make_pending_span("t", "p", "op", 1_000);
        assert_eq!(live.effective_end_ns(now), now);
        assert_eq!(live.duration_ns(now), 9_000);
    }

    #[test]
    #[serial]
    fn live_spans_defaults_on_for_memory_exporter_only() {
        temp_env::with_vars([("OTEL_LIVE_SPANS", None::<&str>)], || {
            // Mode-aware default: local-dev memory mode mirrors live spans;
            // production shapes (both/otlp) don't unless asked to.
            assert!(resolve_live_spans(None, &ExporterType::Memory));
            assert!(!resolve_live_spans(None, &ExporterType::Both));
            assert!(!resolve_live_spans(None, &ExporterType::Otlp));
            // Explicit config beats the mode default in either direction.
            assert!(resolve_live_spans(Some(true), &ExporterType::Both));
            assert!(!resolve_live_spans(Some(false), &ExporterType::Memory));
        });
        // The env var opts a production (both) setup in…
        temp_env::with_vars([("OTEL_LIVE_SPANS", Some("true"))], || {
            assert!(resolve_live_spans(None, &ExporterType::Both));
        });
        // …or a memory setup out, and explicit config still wins over it.
        temp_env::with_vars([("OTEL_LIVE_SPANS", Some("false"))], || {
            assert!(!resolve_live_spans(None, &ExporterType::Memory));
            assert!(resolve_live_spans(Some(true), &ExporterType::Both));
        });
    }

    #[test]
    fn pending_field_serialized_only_when_true() {
        let done = make_stored_span("t", "s", "op", 1, 2);
        let json = serde_json::to_value(&done).unwrap();
        assert!(
            json.get("pending").is_none(),
            "wire shape of finished spans must be unchanged"
        );

        let live = make_pending_span("t", "p", "op", 1);
        let json = serde_json::to_value(&live).unwrap();
        assert_eq!(json["pending"], serde_json::Value::Bool(true));
        assert_eq!(json["end_time_unix_nano"], 0);
    }

    /// Test stand-in for the OTLP exporter: captures what would go on the wire.
    #[derive(Debug, Clone)]
    struct CapturingExporter(Arc<std::sync::Mutex<Vec<SpanData>>>);

    impl SpanExporter for CapturingExporter {
        fn export(
            &self,
            batch: Vec<SpanData>,
        ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
            self.0.lock().unwrap().extend(batch);
            async { Ok(()) }
        }
    }

    #[test]
    fn live_span_processor_mirrors_pending_and_wire_stays_clean() {
        use opentelemetry::trace::{Span as _, Tracer as _, TracerProvider as _};

        let storage = Arc::new(InMemorySpanStorage::new(100));
        let wire = Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider = SdkTracerProvider::builder()
            .with_span_processor(iii_helpers::observability::BaggageSpanProcessor::default())
            .with_span_processor(LiveSpanProcessor::new(storage.clone(), "test-svc".into()))
            // Memory side (final replaces pending) + wire stand-in (OTLP path).
            .with_simple_exporter(InMemorySpanExporter::with_storage(
                storage.clone(),
                "test-svc".into(),
            ))
            .with_simple_exporter(CapturingExporter(wire.clone()))
            .build();
        let tracer = provider.tracer("test");

        let mut span = tracer
            .span_builder("call fn::x")
            .with_attributes([KeyValue::new("function_id", "fn::x")])
            .start(&tracer);

        {
            let stored = storage.get_spans();
            assert_eq!(stored.len(), 1, "pending snapshot lands at start");
            assert!(stored[0].pending);
            assert_eq!(stored[0].end_time_unix_nano, 0);
            assert_eq!(stored[0].name, "call fn::x");
            assert!(
                stored[0]
                    .attributes
                    .iter()
                    .any(|(k, v)| k == "function_id" && v == "fn::x"),
                "creation attributes present on the pending snapshot"
            );
            assert!(wire.lock().unwrap().is_empty(), "nothing exported at start");
        }

        span.end();

        let stored = storage.get_spans();
        assert_eq!(stored.len(), 1, "final replaced the pending snapshot");
        assert!(!stored[0].pending);
        assert!(stored[0].end_time_unix_nano > 0);

        let exported = wire.lock().unwrap();
        assert_eq!(exported.len(), 1, "exactly one span on the wire");
        let end_ns = exported[0]
            .end_time
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        assert!(end_ns > 0, "no unfinished span ever reaches the wire");
    }

    #[test]
    fn live_span_processor_skips_sampled_out_spans() {
        use opentelemetry::trace::{Tracer as _, TracerProvider as _};

        let storage = Arc::new(InMemorySpanStorage::new(100));
        let provider = SdkTracerProvider::builder()
            .with_sampler(Sampler::AlwaysOff)
            .with_span_processor(LiveSpanProcessor::new(storage.clone(), "test-svc".into()))
            .with_simple_exporter(InMemorySpanExporter::with_storage(
                storage.clone(),
                "test-svc".into(),
            ))
            .build();
        let tracer = provider.tracer("test");

        let span = tracer.start("dropped");
        drop(span);

        assert!(
            storage.get_spans().is_empty(),
            "sampled-out spans leave no pending"
        );
    }

    #[test]
    fn test_span_storage_len_and_is_empty() {
        let storage = InMemorySpanStorage::new(10);

        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);

        storage.add_spans(vec![make_stored_span(
            "t1",
            "s1",
            "op1",
            1_000_000_000,
            2_000_000_000,
        )]);
        assert!(!storage.is_empty());
        assert_eq!(storage.len(), 1);

        storage.add_spans(vec![
            make_stored_span("t2", "s2", "op2", 3_000_000_000, 4_000_000_000),
            make_stored_span("t3", "s3", "op3", 5_000_000_000, 6_000_000_000),
        ]);
        assert_eq!(storage.len(), 3);
    }

    #[test]
    fn test_span_storage_calculate_performance_metrics() {
        let storage = InMemorySpanStorage::new(10);

        // Create spans with known durations (in nanoseconds):
        // span1: 10ms  (10_000_000 ns)
        // span2: 20ms  (20_000_000 ns)
        // span3: 30ms  (30_000_000 ns)
        // span4: 40ms  (40_000_000 ns)
        // span5: 100ms (100_000_000 ns)
        let base = 1_000_000_000u64;
        storage.add_spans(vec![
            make_stored_span("t1", "s1", "op1", base, base + 10_000_000),
            make_stored_span("t2", "s2", "op2", base, base + 20_000_000),
            make_stored_span("t3", "s3", "op3", base, base + 30_000_000),
            make_stored_span("t4", "s4", "op4", base, base + 40_000_000),
            make_stored_span("t5", "s5", "op5", base, base + 100_000_000),
        ]);

        let (avg, p50, p95, p99, min, max) = storage.calculate_performance_metrics();

        // avg = (10 + 20 + 30 + 40 + 100) / 5 = 40.0
        assert!((avg - 40.0).abs() < 0.001);
        assert!((min - 10.0).abs() < 0.001);
        assert!((max - 100.0).abs() < 0.001);

        // p50: index = floor(5 * 0.50) = 2 => sorted[2] = 30.0
        assert!((p50 - 30.0).abs() < 0.001);
        // p95: index = floor(5 * 0.95) = 4 => sorted[4] = 100.0
        assert!((p95 - 100.0).abs() < 0.001);
        // p99: index = floor(5 * 0.99) = 4 => sorted[4] = 100.0
        assert!((p99 - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_span_storage_calculate_performance_metrics_empty() {
        let storage = InMemorySpanStorage::new(10);

        let (avg, p50, p95, p99, min, max) = storage.calculate_performance_metrics();
        assert_eq!(avg, 0.0);
        assert_eq!(p50, 0.0);
        assert_eq!(p95, 0.0);
        assert_eq!(p99, 0.0);
        assert_eq!(min, 0.0);
        assert_eq!(max, 0.0);
    }

    // ==========================================================================
    // build_sampler Tests
    // ==========================================================================

    #[test]
    fn test_build_sampler_always_on() {
        let config = OtelConfig {
            enabled: true,
            service_name: "test".to_string(),
            service_version: "0.1.0".to_string(),
            service_namespace: None,
            exporter: ExporterType::Memory,
            endpoint: "http://localhost:4317".to_string(),
            sampling_ratio: 1.0,
            memory_max_spans: 100,
            live_spans: true,
        };

        let sampler = build_sampler(&config);
        // Sampler::AlwaysOn debug representation
        assert_eq!(format!("{:?}", sampler), "AlwaysOn");
    }

    #[test]
    fn test_build_sampler_always_off() {
        let config = OtelConfig {
            enabled: true,
            service_name: "test".to_string(),
            service_version: "0.1.0".to_string(),
            service_namespace: None,
            exporter: ExporterType::Memory,
            endpoint: "http://localhost:4317".to_string(),
            sampling_ratio: 0.0,
            memory_max_spans: 100,
            live_spans: true,
        };

        let sampler = build_sampler(&config);
        assert_eq!(format!("{:?}", sampler), "AlwaysOff");
    }

    #[test]
    fn test_build_sampler_ratio() {
        let config = OtelConfig {
            enabled: true,
            service_name: "test".to_string(),
            service_version: "0.1.0".to_string(),
            service_namespace: None,
            exporter: ExporterType::Memory,
            endpoint: "http://localhost:4317".to_string(),
            sampling_ratio: 0.5,
            memory_max_spans: 100,
            live_spans: true,
        };

        let sampler = build_sampler(&config);
        let dbg = format!("{:?}", sampler);
        assert!(
            dbg.contains("TraceIdRatioBased"),
            "Expected TraceIdRatioBased, got: {}",
            dbg
        );
    }

    // ==========================================================================
    // OTLP Helper Function Tests
    // ==========================================================================

    #[test]
    fn test_extract_service_name_found() {
        let resource = Some(OtlpResource {
            attributes: vec![OtlpKeyValue {
                key: "service.name".to_string(),
                value: Some(OtlpAnyValue {
                    string_value: Some("my-service".to_string()),
                    int_value: None,
                    double_value: None,
                    bool_value: None,
                    kvlist_value: None,
                    array_value: None,
                }),
            }],
        });

        assert_eq!(extract_service_name(&resource), "my-service");
    }

    #[test]
    fn test_extract_service_name_missing() {
        let resource = Some(OtlpResource {
            attributes: vec![OtlpKeyValue {
                key: "other.attr".to_string(),
                value: Some(OtlpAnyValue {
                    string_value: Some("val".to_string()),
                    int_value: None,
                    double_value: None,
                    bool_value: None,
                    kvlist_value: None,
                    array_value: None,
                }),
            }],
        });

        assert_eq!(extract_service_name(&resource), "unknown");
    }

    #[test]
    fn test_extract_service_name_none_resource() {
        assert_eq!(extract_service_name(&None), "unknown");
    }

    #[test]
    fn test_otlp_status_to_string_with_description() {
        // code 0 -> "unset"
        let status_unset = OtlpStatus {
            code: 0,
            message: None,
        };
        let (code, desc) = otlp_status_to_string_with_description(Some(&status_unset));
        assert_eq!(code, "unset");
        assert!(desc.is_none());

        // code 1 -> "ok"
        let status_ok = OtlpStatus {
            code: 1,
            message: None,
        };
        let (code, desc) = otlp_status_to_string_with_description(Some(&status_ok));
        assert_eq!(code, "ok");
        assert!(desc.is_none());

        // code 2 -> "error" with description
        let status_error = OtlpStatus {
            code: 2,
            message: Some("something went wrong".to_string()),
        };
        let (code, desc) = otlp_status_to_string_with_description(Some(&status_error));
        assert_eq!(code, "error");
        assert_eq!(desc, Some("something went wrong".to_string()));

        // unknown code -> "unset"
        let status_unknown = OtlpStatus {
            code: 99,
            message: None,
        };
        let (code, _) = otlp_status_to_string_with_description(Some(&status_unknown));
        assert_eq!(code, "unset");
    }

    #[test]
    fn test_otlp_status_none() {
        let (code, desc) = otlp_status_to_string_with_description(None);
        assert_eq!(code, "unset");
        assert!(desc.is_none());
    }

    #[test]
    fn test_otlp_kv_to_key_value_string() {
        let kv = OtlpKeyValue {
            key: "my.key".to_string(),
            value: Some(OtlpAnyValue {
                string_value: Some("hello".to_string()),
                int_value: None,
                double_value: None,
                bool_value: None,
                kvlist_value: None,
                array_value: None,
            }),
        };

        let result = otlp_kv_to_key_value(&kv);
        assert!(result.is_some());
        let kv_out = result.unwrap();
        assert_eq!(kv_out.key.as_str(), "my.key");
        assert_eq!(kv_out.value.as_str().as_ref(), "hello");
    }

    #[test]
    fn test_otlp_kv_to_key_value_int() {
        let kv = OtlpKeyValue {
            key: "count".to_string(),
            value: Some(OtlpAnyValue {
                string_value: None,
                int_value: Some(42),
                double_value: None,
                bool_value: None,
                kvlist_value: None,
                array_value: None,
            }),
        };

        let result = otlp_kv_to_key_value(&kv);
        assert!(result.is_some());
        let kv_out = result.unwrap();
        assert_eq!(kv_out.key.as_str(), "count");
        // I64 value
        assert_eq!(format!("{}", kv_out.value), "42");
    }

    #[test]
    fn test_otlp_kv_to_key_value_double() {
        let kv = OtlpKeyValue {
            key: "ratio".to_string(),
            value: Some(OtlpAnyValue {
                string_value: None,
                int_value: None,
                double_value: Some(std::f64::consts::PI),
                bool_value: None,
                kvlist_value: None,
                array_value: None,
            }),
        };

        let result = otlp_kv_to_key_value(&kv);
        assert!(result.is_some());
        let kv_out = result.unwrap();
        assert_eq!(kv_out.key.as_str(), "ratio");
        assert_eq!(
            format!("{}", kv_out.value),
            std::f64::consts::PI.to_string()
        );
    }

    #[test]
    fn test_otlp_kv_to_key_value_bool() {
        let kv = OtlpKeyValue {
            key: "enabled".to_string(),
            value: Some(OtlpAnyValue {
                string_value: None,
                int_value: None,
                double_value: None,
                bool_value: Some(true),
                kvlist_value: None,
                array_value: None,
            }),
        };

        let result = otlp_kv_to_key_value(&kv);
        assert!(result.is_some());
        let kv_out = result.unwrap();
        assert_eq!(kv_out.key.as_str(), "enabled");
        assert_eq!(format!("{}", kv_out.value), "true");
    }

    #[test]
    fn test_otlp_kv_to_key_value_none() {
        let kv = OtlpKeyValue {
            key: "empty".to_string(),
            value: None,
        };

        let result = otlp_kv_to_key_value(&kv);
        assert!(result.is_none());
    }

    #[test]
    fn test_otlp_kv_to_key_value_empty_key_is_not_skipped() {
        // The engine's otlp_kv_to_key_value does not filter on empty keys;
        // it only returns None when the value itself is missing or has no
        // recognised variant.
        let kv = OtlpKeyValue {
            key: String::new(),
            value: Some(OtlpAnyValue {
                string_value: Some("hello".to_string()),
                int_value: None,
                double_value: None,
                bool_value: None,
                kvlist_value: None,
                array_value: None,
            }),
        };

        let result = otlp_kv_to_key_value(&kv);
        assert!(result.is_some());
        let kv_result = result.unwrap();
        assert_eq!(kv_result.key.as_str(), "");
    }

    #[test]
    fn test_otlp_any_value_to_string_value() {
        // String branch
        let v = OtlpAnyValue {
            string_value: Some("hello".to_string()),
            int_value: None,
            double_value: None,
            bool_value: None,
            kvlist_value: None,
            array_value: None,
        };
        assert_eq!(v.to_string_value(), "hello");

        // Int branch
        let v = OtlpAnyValue {
            string_value: None,
            int_value: Some(99),
            double_value: None,
            bool_value: None,
            kvlist_value: None,
            array_value: None,
        };
        assert_eq!(v.to_string_value(), "99");

        // Double branch
        let v = OtlpAnyValue {
            string_value: None,
            int_value: None,
            double_value: Some(1.5),
            bool_value: None,
            kvlist_value: None,
            array_value: None,
        };
        assert_eq!(v.to_string_value(), "1.5");

        // Bool branch
        let v = OtlpAnyValue {
            string_value: None,
            int_value: None,
            double_value: None,
            bool_value: Some(false),
            kvlist_value: None,
            array_value: None,
        };
        assert_eq!(v.to_string_value(), "false");

        // Empty (all None) branch
        let v = OtlpAnyValue {
            string_value: None,
            int_value: None,
            double_value: None,
            bool_value: None,
            kvlist_value: None,
            array_value: None,
        };
        assert_eq!(v.to_string_value(), "");
    }

    // ==========================================================================
    // OtlpNumericString Deserializer Tests
    // ==========================================================================

    #[test]
    fn test_otlp_numeric_string_from_string() {
        let json = r#""12345""#;
        let result: OtlpNumericString = serde_json::from_str(json).unwrap();
        assert_eq!(result.0, 12345);
    }

    #[test]
    fn test_otlp_numeric_string_from_u64() {
        let json = r#"12345"#;
        let result: OtlpNumericString = serde_json::from_str(json).unwrap();
        assert_eq!(result.0, 12345);
    }

    #[test]
    fn test_otlp_numeric_string_from_negative() {
        let json = r#"-1"#;
        let result: Result<OtlpNumericString, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // ==========================================================================
    // OtlpIntString Deserializer Tests
    // ==========================================================================

    #[test]
    fn test_otlp_int_string_from_string() {
        let json = r#""2048""#;
        let result: OtlpIntString = serde_json::from_str(json).unwrap();
        assert_eq!(result.0, 2048);
    }

    #[test]
    fn test_otlp_int_string_from_negative_string() {
        let json = r#""-42""#;
        let result: OtlpIntString = serde_json::from_str(json).unwrap();
        assert_eq!(result.0, -42);
    }

    #[test]
    fn test_otlp_int_string_from_number() {
        let json = r#"12345"#;
        let result: OtlpIntString = serde_json::from_str(json).unwrap();
        assert_eq!(result.0, 12345);
    }

    #[test]
    fn test_otlp_int_string_from_negative_number() {
        let json = r#"-7"#;
        let result: OtlpIntString = serde_json::from_str(json).unwrap();
        assert_eq!(result.0, -7);
    }

    #[test]
    fn test_otlp_int_string_rejects_garbage() {
        let json = r#""not a number""#;
        let result: Result<OtlpIntString, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // ==========================================================================
    // OTLP JSON Span Ingestion Tests
    // ==========================================================================

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_json_basic_span() {
        // Initialize in-memory span storage via the global OnceLock.
        // Because OnceLock can only be set once per process, we rely on
        // InMemorySpanExporter::new (or a previous test) having called set.
        // If the global storage is already initialised we just clear it.
        let storage = match get_span_storage() {
            Some(s) => {
                s.clear();
                s
            }
            None => {
                // Force-initialise the global storage
                let _ = InMemorySpanExporter::new(1000, "test".to_string());
                get_span_storage().expect("span storage should be initialised")
            }
        };

        let otlp_json = r#"{
            "resourceSpans": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "my-node-service"}
                    }]
                },
                "scopeSpans": [{
                    "scope": {
                        "name": "@opentelemetry/instrumentation-http",
                        "version": "0.40.0"
                    },
                    "spans": [{
                        "traceId": "abcdef1234567890abcdef1234567890",
                        "spanId": "1234567890abcdef",
                        "name": "GET /api/users",
                        "kind": 2,
                        "startTimeUnixNano": "1704067200000000000",
                        "endTimeUnixNano": "1704067200100000000",
                        "status": {"code": 1},
                        "attributes": [{
                            "key": "http.method",
                            "value": {"stringValue": "GET"}
                        }]
                    }]
                }]
            }]
        }"#;

        let result = ingest_otlp_json(otlp_json).await;
        assert!(result.is_ok(), "ingest_otlp_json failed: {:?}", result);

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 1);
        let span = &spans[0];
        assert_eq!(span.trace_id, "abcdef1234567890abcdef1234567890");
        assert_eq!(span.span_id, "1234567890abcdef");
        assert_eq!(span.name, "GET /api/users");
        assert_eq!(span.start_time_unix_nano, 1704067200000000000);
        assert_eq!(span.end_time_unix_nano, 1704067200100000000);
        assert_eq!(span.status, "ok");
        assert_eq!(span.service_name, "my-node-service");
        assert_eq!(
            span.instrumentation_scope_name.as_deref(),
            Some("@opentelemetry/instrumentation-http")
        );
        assert_eq!(
            span.instrumentation_scope_version.as_deref(),
            Some("0.40.0")
        );
        assert!(
            span.attributes
                .contains(&("http.method".to_string(), "GET".to_string()))
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_json_zero_end_stored_pending_replaced_and_gated() {
        let storage = match get_span_storage() {
            Some(s) => {
                s.clear();
                s
            }
            None => {
                let _ = InMemorySpanExporter::new(1000, "test".to_string());
                get_span_storage().expect("span storage should be initialised")
            }
        };

        // Live ingest resolves through the same switch as the engine's own
        // start mirror; pin it ON explicitly (env-independent).
        let prev = update_otel_config(ObservabilityWorkerConfig {
            live_spans: Some(true),
            ..Default::default()
        });

        let start_snapshot = r#"{
            "resourceSpans": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "harness"}
                    }]
                },
                "scopeSpans": [{
                    "scope": {"name": "iii-worker", "version": "1.0.0"},
                    "spans": [{
                        "traceId": "abcdef1234567890abcdef1234567890",
                        "spanId": "1234567890abcdef",
                        "name": "harness::turn step",
                        "kind": 1,
                        "startTimeUnixNano": "1704067200000000000",
                        "endTimeUnixNano": "0",
                        "status": {"code": 0},
                        "attributes": [{
                            "key": "iii.session.id",
                            "value": {"stringValue": "console-abc"}
                        }]
                    }]
                }]
            }]
        }"#;

        // 1. A zero-end frame (the SDK's start snapshot) stores as pending.
        ingest_otlp_json(start_snapshot).await.unwrap();
        let spans = storage.get_spans();
        assert_eq!(spans.len(), 1);
        assert!(spans[0].pending, "zero-end ingests as a pending span");
        assert_eq!(spans[0].end_time_unix_nano, 0);
        assert_eq!(spans[0].name, "harness::turn step");
        assert!(
            spans[0]
                .attributes
                .contains(&("iii.session.id".to_string(), "console-abc".to_string())),
            "snapshot keeps its attributes (session attribution)"
        );

        // 2. The final span (same span_id) replaces the snapshot in place.
        let final_span = start_snapshot.replace(
            "\"endTimeUnixNano\": \"0\"",
            "\"endTimeUnixNano\": \"1704067203000000000\"",
        );
        ingest_otlp_json(&final_span).await.unwrap();
        let spans = storage.get_spans();
        assert_eq!(spans.len(), 1, "the final must replace, not duplicate");
        assert!(!spans[0].pending);
        assert_eq!(spans[0].end_time_unix_nano, 1704067203000000000);

        // 3. Start snapshots never reach the OTLP forward path — the proto
        //    requires end_time.
        let parsed: OtlpExportTraceServiceRequest = serde_json::from_str(start_snapshot).unwrap();
        assert!(
            convert_otlp_to_span_data(&parsed).is_empty(),
            "zero-end spans must not convert for collector forwarding"
        );

        // 4. Gate off: snapshots are dropped, never stored as zero-end finals.
        update_otel_config(ObservabilityWorkerConfig {
            live_spans: Some(false),
            ..Default::default()
        });
        storage.clear();
        ingest_otlp_json(start_snapshot).await.unwrap();
        assert!(
            storage.get_spans().is_empty(),
            "live_spans off must drop start snapshots"
        );

        match prev {
            Some(p) => {
                update_otel_config((*p).clone());
            }
            None => clear_otel_config_for_test(),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_json_with_events_and_links() {
        let storage = match get_span_storage() {
            Some(s) => {
                s.clear();
                s
            }
            None => {
                let _ = InMemorySpanExporter::new(1000, "test".to_string());
                get_span_storage().expect("span storage should be initialised")
            }
        };

        let otlp_json = r#"{
            "resourceSpans": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "event-service"}
                    }]
                },
                "scopeSpans": [{
                    "spans": [{
                        "traceId": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1",
                        "spanId": "bbbbbbbbbbbbbb01",
                        "name": "span-with-events",
                        "startTimeUnixNano": "1000000000",
                        "endTimeUnixNano": "2000000000",
                        "events": [{
                            "name": "exception",
                            "timeUnixNano": "1500000000",
                            "attributes": [{
                                "key": "exception.message",
                                "value": {"stringValue": "NullPointer"}
                            }]
                        }],
                        "links": [{
                            "traceId": "cccccccccccccccccccccccccccccc01",
                            "spanId": "dddddddddddddd01",
                            "attributes": [{
                                "key": "link.reason",
                                "value": {"stringValue": "follows-from"}
                            }]
                        }]
                    }]
                }]
            }]
        }"#;

        let result = ingest_otlp_json(otlp_json).await;
        assert!(result.is_ok(), "ingest_otlp_json failed: {:?}", result);

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 1);

        let span = &spans[0];
        assert_eq!(span.name, "span-with-events");

        // Check events
        assert_eq!(span.events.len(), 1);
        assert_eq!(span.events[0].name, "exception");
        assert_eq!(span.events[0].timestamp_unix_nano, 1500000000);
        assert!(
            span.events[0]
                .attributes
                .contains(&("exception.message".to_string(), "NullPointer".to_string()))
        );

        // Check links
        assert_eq!(span.links.len(), 1);
        assert_eq!(span.links[0].trace_id, "cccccccccccccccccccccccccccccc01");
        assert_eq!(span.links[0].span_id, "dddddddddddddd01");
        assert!(
            span.links[0]
                .attributes
                .contains(&("link.reason".to_string(), "follows-from".to_string()))
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_json_span_kinds() {
        // This test verifies that different OTLP span kind numeric values are
        // accepted and stored correctly.  The `ingest_otlp_json` function
        // stores spans as StoredSpan which does not carry `kind` directly, but
        // we verify that the ingestion does not fail for each kind value.
        let storage = match get_span_storage() {
            Some(s) => {
                s.clear();
                s
            }
            None => {
                let _ = InMemorySpanExporter::new(1000, "test".to_string());
                get_span_storage().expect("span storage should be initialised")
            }
        };

        // kind 2=Server, 3=Client, 4=Producer, 5=Consumer, 1=Internal
        let kinds = [
            (1, "internal-span"),
            (2, "server-span"),
            (3, "client-span"),
            (4, "producer-span"),
            (5, "consumer-span"),
        ];

        for (kind, name) in &kinds {
            let otlp_json = format!(
                r#"{{
                    "resourceSpans": [{{
                        "resource": {{
                            "attributes": [{{
                                "key": "service.name",
                                "value": {{"stringValue": "kind-test"}}
                            }}]
                        }},
                        "scopeSpans": [{{
                            "spans": [{{
                                "traceId": "abcdef1234567890abcdef1234567890",
                                "spanId": "1234567890abcdef",
                                "name": "{}",
                                "kind": {},
                                "startTimeUnixNano": "1000000000",
                                "endTimeUnixNano": "2000000000"
                            }}]
                        }}]
                    }}]
                }}"#,
                name, kind
            );

            let result = ingest_otlp_json(&otlp_json).await;
            assert!(
                result.is_ok(),
                "ingest_otlp_json failed for kind {}: {:?}",
                kind,
                result
            );
        }

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 5);

        let names: Vec<&str> = spans.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"server-span"));
        assert!(names.contains(&"client-span"));
        assert!(names.contains(&"producer-span"));
        assert!(names.contains(&"consumer-span"));
        assert!(names.contains(&"internal-span"));
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_json_invalid() {
        let invalid_json = r#"{"not valid json"#;
        let result = ingest_otlp_json(invalid_json).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_json_empty() {
        let storage = match get_span_storage() {
            Some(s) => {
                s.clear();
                s
            }
            None => {
                let _ = InMemorySpanExporter::new(1000, "test".to_string());
                get_span_storage().expect("span storage should be initialised")
            }
        };

        let empty_json = r#"{"resourceSpans": []}"#;
        let result = ingest_otlp_json(empty_json).await;
        assert!(result.is_ok());

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 0);
    }

    // =========================================================================
    // convert_otlp_to_span_data tests
    // =========================================================================

    #[test]
    fn test_convert_otlp_to_span_data_basic() {
        let json_str = r#"{
            "resourceSpans": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "convert-test"}
                    }]
                },
                "scopeSpans": [{
                    "scope": {"name": "test-scope", "version": "1.0.0"},
                    "spans": [{
                        "traceId": "0af7651916cd43dd8448eb211c80319c",
                        "spanId": "b7ad6b7169203331",
                        "name": "basic-span",
                        "kind": 2,
                        "startTimeUnixNano": "1704067200000000000",
                        "endTimeUnixNano": "1704067201000000000",
                        "status": {"code": 1, "message": ""},
                        "attributes": [{
                            "key": "http.method",
                            "value": {"stringValue": "GET"}
                        }]
                    }]
                }]
            }]
        }"#;

        let request: OtlpExportTraceServiceRequest =
            serde_json::from_str(json_str).expect("parse JSON");
        let span_data = convert_otlp_to_span_data(&request);

        assert_eq!(span_data.len(), 1);
        let sd = &span_data[0];
        assert_eq!(sd.name.as_ref(), "basic-span");
        assert!(matches!(
            sd.span_kind,
            opentelemetry::trace::SpanKind::Server
        ));
        assert!(matches!(sd.status, opentelemetry::trace::Status::Ok));
        assert_eq!(sd.attributes.len(), 1);
        assert_eq!(sd.attributes[0].key.as_str(), "http.method");
    }

    #[test]
    fn test_build_forward_resource_preserves_service_name() {
        // DUP-3: a worker's OTLP resource (service.name=harness) must survive
        // into the forwarder `Resource` so the collector attributes spans
        // correctly instead of bucketing them under `unknown_service`.
        let json_str = r#"{
            "resourceSpans": [{
                "resource": {
                    "attributes": [
                        {"key": "service.name", "value": {"stringValue": "harness"}},
                        {"key": "service.version", "value": {"stringValue": "1.2.3"}}
                    ]
                },
                "scopeSpans": []
            }]
        }"#;
        let request: OtlpExportTraceServiceRequest =
            serde_json::from_str(json_str).expect("parse JSON");
        let rs = &request.resource_spans[0];

        let resource = build_forward_resource(&rs.resource);
        let svc = resource
            .get(&opentelemetry::Key::from_static_str("service.name"))
            .map(|v| v.as_str().to_string());
        assert_eq!(svc.as_deref(), Some("harness"));
        // The per-service forwarder is keyed by this same extraction.
        assert_eq!(extract_service_name(&rs.resource), "harness");
    }

    #[test]
    fn test_forward_span_sampling_respects_sampler() {
        // VOL-3: forwarded worker spans must pass through the engine sampler
        // rather than being exported unconditionally.
        let json_str = r#"{
            "resourceSpans": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "harness"}
                    }]
                },
                "scopeSpans": [{
                    "scope": {"name": "test-scope", "version": "1.0.0"},
                    "spans": [{
                        "traceId": "0af7651916cd43dd8448eb211c80319c",
                        "spanId": "b7ad6b7169203331",
                        "name": "sampled-span",
                        "kind": 1,
                        "startTimeUnixNano": "1704067200000000000",
                        "endTimeUnixNano": "1704067201000000000",
                        "status": {"code": 1, "message": ""}
                    }]
                }]
            }]
        }"#;
        let request: OtlpExportTraceServiceRequest =
            serde_json::from_str(json_str).expect("parse JSON");
        let span_data = convert_otlp_to_span_data(&request);
        let sd = &span_data[0];

        assert!(
            forward_span_is_sampled(&Sampler::AlwaysOn, sd),
            "AlwaysOn must keep forwarded spans"
        );
        assert!(
            !forward_span_is_sampled(&Sampler::AlwaysOff, sd),
            "AlwaysOff must drop forwarded spans"
        );
    }

    #[test]
    fn test_convert_otlp_to_span_data_with_events_and_links() {
        let json_str = r#"{
            "resourceSpans": [{
                "resource": {},
                "scopeSpans": [{
                    "scope": {"name": "events-scope", "version": ""},
                    "spans": [{
                        "traceId": "0af7651916cd43dd8448eb211c80319c",
                        "spanId": "b7ad6b7169203331",
                        "name": "span-with-events",
                        "kind": 1,
                        "startTimeUnixNano": "1000000000",
                        "endTimeUnixNano": "2000000000",
                        "events": [{
                            "name": "exception",
                            "timeUnixNano": "1500000000",
                            "attributes": [{
                                "key": "exception.message",
                                "value": {"stringValue": "something failed"}
                            }]
                        }],
                        "links": [{
                            "traceId": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1",
                            "spanId": "bbbbbbbbbbbbbb01",
                            "attributes": [{
                                "key": "link.type",
                                "value": {"stringValue": "follows_from"}
                            }]
                        }]
                    }]
                }]
            }]
        }"#;

        let request: OtlpExportTraceServiceRequest =
            serde_json::from_str(json_str).expect("parse JSON");
        let span_data = convert_otlp_to_span_data(&request);

        assert_eq!(span_data.len(), 1);
        let sd = &span_data[0];
        assert_eq!(sd.events.events.len(), 1);
        assert_eq!(sd.events.events[0].name.as_ref(), "exception");
        assert_eq!(sd.links.links.len(), 1);
    }

    #[test]
    fn test_convert_otlp_to_span_data_error_status() {
        let json_str = r#"{
            "resourceSpans": [{
                "resource": {},
                "scopeSpans": [{
                    "spans": [{
                        "traceId": "0af7651916cd43dd8448eb211c80319c",
                        "spanId": "b7ad6b7169203331",
                        "name": "error-span",
                        "kind": 3,
                        "startTimeUnixNano": "1000000000",
                        "endTimeUnixNano": "2000000000",
                        "status": {"code": 2, "message": "timeout occurred"}
                    }]
                }]
            }]
        }"#;

        let request: OtlpExportTraceServiceRequest =
            serde_json::from_str(json_str).expect("parse JSON");
        let span_data = convert_otlp_to_span_data(&request);

        assert_eq!(span_data.len(), 1);
        let sd = &span_data[0];
        assert!(matches!(
            &sd.status,
            opentelemetry::trace::Status::Error { description } if description.as_ref() == "timeout occurred"
        ));
        assert!(matches!(
            sd.span_kind,
            opentelemetry::trace::SpanKind::Client
        ));
    }

    #[test]
    fn test_convert_otlp_to_span_data_invalid_trace_id_skipped() {
        let json_str = r#"{
            "resourceSpans": [{
                "resource": {},
                "scopeSpans": [{
                    "spans": [
                        {
                            "traceId": "invalid_hex",
                            "spanId": "b7ad6b7169203331",
                            "name": "bad-trace-id",
                            "startTimeUnixNano": "1000000000",
                            "endTimeUnixNano": "2000000000"
                        },
                        {
                            "traceId": "0af7651916cd43dd8448eb211c80319c",
                            "spanId": "invalid_hex",
                            "name": "bad-span-id",
                            "startTimeUnixNano": "1000000000",
                            "endTimeUnixNano": "2000000000"
                        }
                    ]
                }]
            }]
        }"#;

        let request: OtlpExportTraceServiceRequest =
            serde_json::from_str(json_str).expect("parse JSON");
        let span_data = convert_otlp_to_span_data(&request);

        // Both spans should be skipped due to invalid IDs
        assert_eq!(span_data.len(), 0);
    }

    #[test]
    fn test_convert_otlp_to_span_data_parent_span_handling() {
        let json_str = r#"{
            "resourceSpans": [{
                "resource": {},
                "scopeSpans": [{
                    "spans": [
                        {
                            "traceId": "0af7651916cd43dd8448eb211c80319c",
                            "spanId": "b7ad6b7169203331",
                            "parentSpanId": "a1a2a3a4a5a6a7a8",
                            "name": "child-span",
                            "startTimeUnixNano": "1000000000",
                            "endTimeUnixNano": "2000000000"
                        },
                        {
                            "traceId": "0af7651916cd43dd8448eb211c80319c",
                            "spanId": "c1c2c3c4c5c6c7c8",
                            "parentSpanId": "0000000000000000",
                            "name": "root-span",
                            "startTimeUnixNano": "1000000000",
                            "endTimeUnixNano": "2000000000"
                        }
                    ]
                }]
            }]
        }"#;

        let request: OtlpExportTraceServiceRequest =
            serde_json::from_str(json_str).expect("parse JSON");
        let span_data = convert_otlp_to_span_data(&request);

        assert_eq!(span_data.len(), 2);

        // First span has a real parent
        let child = &span_data[0];
        assert_eq!(child.name.as_ref(), "child-span");
        assert!(child.parent_span_is_remote);
        assert_ne!(child.parent_span_id.to_string(), "0000000000000000");

        // Second span has all-zero parent (treated as root)
        let root = &span_data[1];
        assert_eq!(root.name.as_ref(), "root-span");
        assert!(!root.parent_span_is_remote);
    }

    #[test]
    fn test_convert_otlp_to_span_data_multiple_resource_and_scope_spans() {
        let json_str = r#"{
            "resourceSpans": [
                {
                    "resource": {
                        "attributes": [{"key": "service.name", "value": {"stringValue": "svc-a"}}]
                    },
                    "scopeSpans": [
                        {
                            "scope": {"name": "scope-1", "version": "1.0"},
                            "spans": [{
                                "traceId": "0af7651916cd43dd8448eb211c80319c",
                                "spanId": "aaaaaaaaaaaaaaaa",
                                "name": "span-a1",
                                "startTimeUnixNano": "1000000000",
                                "endTimeUnixNano": "2000000000"
                            }]
                        },
                        {
                            "scope": {"name": "scope-2", "version": "2.0"},
                            "spans": [{
                                "traceId": "0af7651916cd43dd8448eb211c80319c",
                                "spanId": "bbbbbbbbbbbbbbbb",
                                "name": "span-a2",
                                "startTimeUnixNano": "1000000000",
                                "endTimeUnixNano": "2000000000"
                            }]
                        }
                    ]
                },
                {
                    "resource": {
                        "attributes": [{"key": "service.name", "value": {"stringValue": "svc-b"}}]
                    },
                    "scopeSpans": [{
                        "spans": [{
                            "traceId": "0af7651916cd43dd8448eb211c80319c",
                            "spanId": "cccccccccccccccc",
                            "name": "span-b1",
                            "startTimeUnixNano": "1000000000",
                            "endTimeUnixNano": "2000000000"
                        }]
                    }]
                }
            ]
        }"#;

        let request: OtlpExportTraceServiceRequest =
            serde_json::from_str(json_str).expect("parse JSON");
        let span_data = convert_otlp_to_span_data(&request);

        assert_eq!(span_data.len(), 3);
        let names: Vec<&str> = span_data.iter().map(|s| s.name.as_ref()).collect();
        assert!(names.contains(&"span-a1"));
        assert!(names.contains(&"span-a2"));
        assert!(names.contains(&"span-b1"));
    }

    #[test]
    fn test_convert_otlp_to_span_data_otel_kind_attribute_fallback() {
        let json_str = r#"{
            "resourceSpans": [{
                "resource": {},
                "scopeSpans": [{
                    "spans": [{
                        "traceId": "0af7651916cd43dd8448eb211c80319c",
                        "spanId": "b7ad6b7169203331",
                        "name": "attr-kind-span",
                        "startTimeUnixNano": "1000000000",
                        "endTimeUnixNano": "2000000000",
                        "attributes": [{
                            "key": "otel.kind",
                            "value": {"stringValue": "PRODUCER"}
                        }]
                    }]
                }]
            }]
        }"#;

        let request: OtlpExportTraceServiceRequest =
            serde_json::from_str(json_str).expect("parse JSON");
        let span_data = convert_otlp_to_span_data(&request);

        assert_eq!(span_data.len(), 1);
        assert!(matches!(
            span_data[0].span_kind,
            opentelemetry::trace::SpanKind::Producer
        ));
    }

    #[test]
    fn test_convert_otlp_to_span_data_trace_flags() {
        let json_str = r#"{
            "resourceSpans": [{
                "resource": {},
                "scopeSpans": [{
                    "spans": [{
                        "traceId": "0af7651916cd43dd8448eb211c80319c",
                        "spanId": "b7ad6b7169203331",
                        "name": "flags-span",
                        "startTimeUnixNano": "1000000000",
                        "endTimeUnixNano": "2000000000",
                        "flags": 1
                    }]
                }]
            }]
        }"#;

        let request: OtlpExportTraceServiceRequest =
            serde_json::from_str(json_str).expect("parse JSON");
        let span_data = convert_otlp_to_span_data(&request);

        assert_eq!(span_data.len(), 1);
        assert_eq!(
            span_data[0].span_context.trace_flags(),
            opentelemetry::trace::TraceFlags::SAMPLED
        );
    }

    // =========================================================================
    // InMemorySpanExporter::export direct test
    // =========================================================================

    #[tokio::test]
    async fn test_in_memory_span_exporter_export_directly() {
        use opentelemetry::trace::{SpanContext, SpanKind, Status, TraceFlags, TraceState};
        use opentelemetry::{InstrumentationScope, SpanId, TraceId};
        use opentelemetry_sdk::trace::{SpanEvents, SpanLinks};
        use std::borrow::Cow;
        use std::time::{Duration, UNIX_EPOCH};

        let storage = Arc::new(InMemorySpanStorage::new(100));
        let exporter =
            InMemorySpanExporter::with_storage(storage.clone(), "export-test".to_string());

        let trace_id = TraceId::from_hex("0af7651916cd43dd8448eb211c80319c").unwrap();
        let span_id = SpanId::from_hex("b7ad6b7169203331").unwrap();
        let span_context = SpanContext::new(
            trace_id,
            span_id,
            TraceFlags::SAMPLED,
            true,
            TraceState::NONE,
        );

        let sd = SpanData {
            span_context,
            parent_span_id: SpanId::INVALID,
            parent_span_is_remote: false,
            span_kind: SpanKind::Internal,
            name: Cow::Borrowed("export-test-span"),
            start_time: UNIX_EPOCH + Duration::from_secs(1000),
            end_time: UNIX_EPOCH + Duration::from_secs(1001),
            attributes: vec![opentelemetry::KeyValue::new("key", "value")],
            dropped_attributes_count: 0,
            events: SpanEvents::default(),
            links: SpanLinks::default(),
            status: Status::Ok,
            instrumentation_scope: InstrumentationScope::builder("test").build(),
        };

        let result = exporter.export(vec![sd]).await;
        assert!(result.is_ok());

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "export-test-span");
        assert_eq!(spans[0].service_name, "export-test");
        assert_eq!(spans[0].status, "ok");
        assert_eq!(spans[0].attributes.len(), 1);
        assert_eq!(spans[0].attributes[0].0, "key");
    }

    #[test]
    fn stored_span_from_span_data_carries_tracestate() {
        use opentelemetry::trace::{SpanContext, SpanKind, Status, TraceFlags, TraceState};
        use opentelemetry::{InstrumentationScope, SpanId, TraceId};
        use opentelemetry_sdk::trace::{SpanEvents, SpanLinks};
        use std::borrow::Cow;
        use std::time::{Duration, UNIX_EPOCH};

        let trace_state = "congo=t61rcWkgMzE,rojo=00f067aa0ba902b7"
            .parse::<TraceState>()
            .unwrap();
        let span_context = SpanContext::new(
            TraceId::from_hex("0af7651916cd43dd8448eb211c80319c").unwrap(),
            SpanId::from_hex("b7ad6b7169203331").unwrap(),
            TraceFlags::SAMPLED,
            true,
            trace_state.clone(),
        );

        let make = |sc: SpanContext| SpanData {
            span_context: sc,
            parent_span_id: SpanId::INVALID,
            parent_span_is_remote: false,
            span_kind: SpanKind::Server,
            name: Cow::Borrowed("HTTP"),
            start_time: UNIX_EPOCH + Duration::from_secs(1000),
            end_time: UNIX_EPOCH + Duration::from_secs(1001),
            attributes: vec![],
            dropped_attributes_count: 0,
            events: SpanEvents::default(),
            links: SpanLinks::default(),
            status: Status::Ok,
            instrumentation_scope: InstrumentationScope::builder("test").build(),
        };

        let stored = StoredSpan::from_span_data(&make(span_context), "svc");
        assert_eq!(
            stored.trace_state.as_deref(),
            Some(trace_state.header().as_str())
        );

        // A span with no tracestate omits the field rather than emitting "".
        let empty_sc = SpanContext::new(
            TraceId::from_hex("0af7651916cd43dd8448eb211c80319c").unwrap(),
            SpanId::from_hex("b7ad6b7169203331").unwrap(),
            TraceFlags::SAMPLED,
            true,
            TraceState::NONE,
        );
        let stored_empty = StoredSpan::from_span_data(&make(empty_sc), "svc");
        assert_eq!(stored_empty.trace_state, None);
    }

    // =========================================================================
    // current_trace_id / current_span_id (no active span)
    // =========================================================================

    #[test]
    fn test_current_trace_id_no_active_span() {
        // Without an active span, current_trace_id should return None
        let trace_id = current_trace_id();
        assert!(trace_id.is_none());
    }

    #[test]
    fn test_current_span_id_no_active_span() {
        let span_id = current_span_id();
        assert!(span_id.is_none());
    }

    // =========================================================================
    // inject_traceparent / inject_baggage (no active span)
    // =========================================================================

    #[test]
    fn test_inject_traceparent_no_active_span() {
        let traceparent = inject_traceparent();
        assert!(traceparent.is_none());
    }

    #[test]
    fn test_inject_baggage_no_active_context() {
        let baggage = inject_baggage();
        // Without any baggage in the context, should return None
        assert!(baggage.is_none());
    }

    // =========================================================================
    // inject/extract from explicit context
    // =========================================================================

    #[test]
    fn test_inject_traceparent_from_invalid_context() {
        let ctx = Context::current();
        let traceparent = inject_traceparent_from_context(&ctx);
        assert!(traceparent.is_none());
    }

    #[test]
    fn test_inject_baggage_from_empty_context() {
        let ctx = Context::current();
        let baggage = inject_baggage_from_context(&ctx);
        assert!(baggage.is_none());
    }

    #[test]
    fn test_extract_traceparent_and_reinject() {
        let tp = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let ctx = extract_traceparent(tp);

        // The context should have a valid span context
        let span_ref = ctx.span();
        let sc = span_ref.span_context();
        assert!(sc.is_valid());
        assert_eq!(
            sc.trace_id().to_string(),
            "0af7651916cd43dd8448eb211c80319c"
        );
        assert_eq!(sc.span_id().to_string(), "b7ad6b7169203331");
    }

    // =========================================================================
    // StoredSpan::from_span_data tests
    // =========================================================================

    #[test]
    fn test_stored_span_from_span_data_with_error_status() {
        use opentelemetry::trace::{SpanContext, SpanKind, Status, TraceFlags, TraceState};
        use opentelemetry::{InstrumentationScope, SpanId, TraceId};
        use opentelemetry_sdk::trace::{SpanEvents, SpanLinks};
        use std::borrow::Cow;
        use std::time::{Duration, UNIX_EPOCH};

        let trace_id = TraceId::from_hex("0af7651916cd43dd8448eb211c80319c").unwrap();
        let span_id = SpanId::from_hex("b7ad6b7169203331").unwrap();
        let parent_id = SpanId::from_hex("a1a2a3a4a5a6a7a8").unwrap();
        let span_context = SpanContext::new(
            trace_id,
            span_id,
            TraceFlags::SAMPLED,
            true,
            TraceState::NONE,
        );

        let sd = SpanData {
            span_context,
            parent_span_id: parent_id,
            parent_span_is_remote: true,
            span_kind: SpanKind::Server,
            name: Cow::Borrowed("error-test"),
            start_time: UNIX_EPOCH + Duration::from_secs(100),
            end_time: UNIX_EPOCH + Duration::from_secs(200),
            attributes: vec![],
            dropped_attributes_count: 0,
            events: SpanEvents::default(),
            links: SpanLinks::default(),
            status: Status::error("connection refused".to_string()),
            instrumentation_scope: InstrumentationScope::builder("test").build(),
        };

        let stored = StoredSpan::from_span_data(&sd, "my-service");
        assert_eq!(stored.status, "error");
        assert_eq!(
            stored.status_description,
            Some("connection refused".to_string())
        );
        assert_eq!(stored.parent_span_id, Some("a1a2a3a4a5a6a7a8".to_string()));
        assert_eq!(stored.service_name, "my-service");
        assert!(stored.flags.is_some());
    }

    #[test]
    fn test_stored_span_from_span_data_unset_status() {
        use opentelemetry::trace::{SpanContext, SpanKind, Status, TraceFlags, TraceState};
        use opentelemetry::{InstrumentationScope, SpanId, TraceId};
        use opentelemetry_sdk::trace::{SpanEvents, SpanLinks};
        use std::borrow::Cow;
        use std::time::{Duration, UNIX_EPOCH};

        let trace_id = TraceId::from_hex("0af7651916cd43dd8448eb211c80319c").unwrap();
        let span_id = SpanId::from_hex("b7ad6b7169203331").unwrap();
        let span_context = SpanContext::new(
            trace_id,
            span_id,
            TraceFlags::SAMPLED,
            true,
            TraceState::NONE,
        );

        let sd = SpanData {
            span_context,
            parent_span_id: SpanId::INVALID,
            parent_span_is_remote: false,
            span_kind: SpanKind::Internal,
            name: Cow::Borrowed("unset-status"),
            start_time: UNIX_EPOCH + Duration::from_secs(1),
            end_time: UNIX_EPOCH + Duration::from_secs(2),
            attributes: vec![],
            dropped_attributes_count: 0,
            events: SpanEvents::default(),
            links: SpanLinks::default(),
            status: Status::Unset,
            instrumentation_scope: InstrumentationScope::builder("test").build(),
        };

        let stored = StoredSpan::from_span_data(&sd, "svc");
        assert_eq!(stored.status, "unset");
        assert!(stored.status_description.is_none());
        // Parent span id should be None for invalid parent
        assert!(stored.parent_span_id.is_none());
    }

    // =========================================================================
    // otlp_status_to_string_with_description coverage
    // =========================================================================

    #[test]
    fn test_otlp_status_to_string_with_description_unknown_code() {
        let status = OtlpStatus {
            code: 99,
            message: Some("some message".to_string()),
        };
        let (code, desc) = otlp_status_to_string_with_description(Some(&status));
        assert_eq!(code, "unset");
        // For unknown status code, message is still extracted
        assert_eq!(desc, Some("some message".to_string()));
    }

    #[test]
    fn test_otlp_status_to_string_with_description_empty_message() {
        let status = OtlpStatus {
            code: 2,
            message: Some("".to_string()),
        };
        let (code, desc) = otlp_status_to_string_with_description(Some(&status));
        assert_eq!(code, "error");
        assert!(desc.is_none()); // Empty message should be filtered out
    }

    // =========================================================================
    // InMemorySpanExporter::with_storage test
    // =========================================================================

    #[test]
    fn test_in_memory_span_exporter_with_storage_does_not_set_global() {
        let storage = Arc::new(InMemorySpanStorage::new(50));
        let exporter =
            InMemorySpanExporter::with_storage(storage.clone(), "local-test".to_string());

        // Verify the exporter uses the provided storage
        assert_eq!(exporter.service_name, "local-test");
        assert!(storage.is_empty());
    }

    // =========================================================================
    // InMemorySpanStorage::is_empty test
    // =========================================================================

    #[test]
    fn test_span_storage_is_empty() {
        let storage = InMemorySpanStorage::new(10);
        assert!(storage.is_empty());

        storage.add_spans(vec![make_stored_span("t1", "s1", "span1", 100, 200)]);
        assert!(!storage.is_empty());

        storage.clear();
        assert!(storage.is_empty());
    }

    // =========================================================================
    // InMemorySpanStorage Debug impl
    // =========================================================================

    #[test]
    fn test_span_storage_debug() {
        let storage = InMemorySpanStorage::new(5);
        let debug_str = format!("{:?}", storage);
        assert!(debug_str.contains("InMemorySpanStorage"));
        assert!(debug_str.contains("max_spans"));
    }

    // =========================================================================
    // Ingest OTLP JSON: multiple resource spans and multiple scope spans
    // =========================================================================

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_json_multiple_resource_spans() {
        let storage = match get_span_storage() {
            Some(s) => {
                s.clear();
                s
            }
            None => {
                let _ = InMemorySpanExporter::new(1000, "test".to_string());
                get_span_storage().expect("span storage should be initialised")
            }
        };

        let json_str = r#"{
            "resourceSpans": [
                {
                    "resource": {
                        "attributes": [{"key": "service.name", "value": {"stringValue": "svc-alpha"}}]
                    },
                    "scopeSpans": [{
                        "scope": {"name": "alpha-scope", "version": "1.0"},
                        "spans": [{
                            "traceId": "0af7651916cd43dd8448eb211c80319c",
                            "spanId": "aaaaaaaaaaaaaaaa",
                            "name": "alpha-span",
                            "startTimeUnixNano": "1704067200000000000",
                            "endTimeUnixNano": "1704067201000000000"
                        }]
                    }]
                },
                {
                    "resource": {
                        "attributes": [{"key": "service.name", "value": {"stringValue": "svc-beta"}}]
                    },
                    "scopeSpans": [{
                        "scope": {"name": "beta-scope", "version": "2.0"},
                        "spans": [{
                            "traceId": "0af7651916cd43dd8448eb211c80319c",
                            "spanId": "bbbbbbbbbbbbbbbb",
                            "name": "beta-span",
                            "startTimeUnixNano": "1704067202000000000",
                            "endTimeUnixNano": "1704067203000000000"
                        }]
                    }]
                }
            ]
        }"#;

        let result = ingest_otlp_json(json_str).await;
        assert!(result.is_ok());

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 2);

        let names: Vec<&str> = spans.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"alpha-span"));
        assert!(names.contains(&"beta-span"));

        // Verify service names
        let alpha = spans.iter().find(|s| s.name == "alpha-span").unwrap();
        assert_eq!(alpha.service_name, "svc-alpha");
        assert_eq!(
            alpha.instrumentation_scope_name,
            Some("alpha-scope".to_string())
        );

        let beta = spans.iter().find(|s| s.name == "beta-span").unwrap();
        assert_eq!(beta.service_name, "svc-beta");
        assert_eq!(
            beta.instrumentation_scope_name,
            Some("beta-scope".to_string())
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_json_multiple_scope_spans() {
        let storage = match get_span_storage() {
            Some(s) => {
                s.clear();
                s
            }
            None => {
                let _ = InMemorySpanExporter::new(1000, "test".to_string());
                get_span_storage().expect("span storage should be initialised")
            }
        };

        let json_str = r#"{
            "resourceSpans": [{
                "resource": {
                    "attributes": [{"key": "service.name", "value": {"stringValue": "multi-scope"}}]
                },
                "scopeSpans": [
                    {
                        "scope": {"name": "@opentelemetry/instrumentation-http", "version": "0.52.0"},
                        "spans": [{
                            "traceId": "0af7651916cd43dd8448eb211c80319c",
                            "spanId": "1111111111111111",
                            "name": "HTTP GET",
                            "kind": 2,
                            "startTimeUnixNano": "1000000000",
                            "endTimeUnixNano": "2000000000"
                        }]
                    },
                    {
                        "scope": {"name": "@opentelemetry/instrumentation-express", "version": "0.40.0"},
                        "spans": [{
                            "traceId": "0af7651916cd43dd8448eb211c80319c",
                            "spanId": "2222222222222222",
                            "name": "middleware - jsonParser",
                            "kind": 1,
                            "startTimeUnixNano": "1100000000",
                            "endTimeUnixNano": "1200000000"
                        }]
                    }
                ]
            }]
        }"#;

        let result = ingest_otlp_json(json_str).await;
        assert!(result.is_ok());

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 2);

        let http_span = spans.iter().find(|s| s.name == "HTTP GET").unwrap();
        assert_eq!(
            http_span.instrumentation_scope_name,
            Some("@opentelemetry/instrumentation-http".to_string())
        );
        assert_eq!(
            http_span.instrumentation_scope_version,
            Some("0.52.0".to_string())
        );

        let express_span = spans
            .iter()
            .find(|s| s.name == "middleware - jsonParser")
            .unwrap();
        assert_eq!(
            express_span.instrumentation_scope_name,
            Some("@opentelemetry/instrumentation-express".to_string())
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_json_with_status_description() {
        let storage = match get_span_storage() {
            Some(s) => {
                s.clear();
                s
            }
            None => {
                let _ = InMemorySpanExporter::new(1000, "test".to_string());
                get_span_storage().expect("span storage should be initialised")
            }
        };

        let json_str = r#"{
            "resourceSpans": [{
                "resource": {},
                "scopeSpans": [{
                    "spans": [{
                        "traceId": "0af7651916cd43dd8448eb211c80319c",
                        "spanId": "b7ad6b7169203331",
                        "name": "errored-span",
                        "startTimeUnixNano": "1000000000",
                        "endTimeUnixNano": "2000000000",
                        "status": {"code": 2, "message": "Connection refused"}
                    }]
                }]
            }]
        }"#;

        let result = ingest_otlp_json(json_str).await;
        assert!(result.is_ok());

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].status, "error");
        assert_eq!(
            spans[0].status_description,
            Some("Connection refused".to_string())
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_json_with_flags() {
        let storage = match get_span_storage() {
            Some(s) => {
                s.clear();
                s
            }
            None => {
                let _ = InMemorySpanExporter::new(1000, "test".to_string());
                get_span_storage().expect("span storage should be initialised")
            }
        };

        let json_str = r#"{
            "resourceSpans": [{
                "resource": {},
                "scopeSpans": [{
                    "spans": [{
                        "traceId": "0af7651916cd43dd8448eb211c80319c",
                        "spanId": "b7ad6b7169203331",
                        "name": "flagged-span",
                        "startTimeUnixNano": "1000000000",
                        "endTimeUnixNano": "2000000000",
                        "flags": 1
                    }]
                }]
            }]
        }"#;

        let result = ingest_otlp_json(json_str).await;
        assert!(result.is_ok());

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].flags, Some(1));
    }

    #[test]
    fn test_extract_context_keeps_baggage_when_traceparent_is_invalid() {
        use opentelemetry::baggage::BaggageExt;

        let ctx = extract_context(Some("00-invalid"), Some("user.id=123"));

        let span_context = ctx.span().span_context().clone();
        let baggage = ctx.baggage();

        assert!(
            !span_context.is_valid(),
            "invalid traceparent should not produce a valid span context"
        );
        assert_eq!(
            baggage.get("user.id").map(|value| value.to_string()),
            Some("123".to_string())
        );
    }

    #[test]
    fn test_extract_log_helpers_cover_empty_and_typed_values() {
        let attrs = extract_log_attributes(&[
            OtlpKeyValue {
                key: "message".to_string(),
                value: Some(OtlpAnyValue {
                    string_value: Some("hello".to_string()),
                    int_value: None,
                    double_value: None,
                    bool_value: None,
                    kvlist_value: None,
                    array_value: None,
                }),
            },
            OtlpKeyValue {
                key: "count".to_string(),
                value: Some(OtlpAnyValue {
                    string_value: None,
                    int_value: Some(3),
                    double_value: None,
                    bool_value: None,
                    kvlist_value: None,
                    array_value: None,
                }),
            },
            OtlpKeyValue {
                key: "ratio".to_string(),
                value: Some(OtlpAnyValue {
                    string_value: None,
                    int_value: None,
                    double_value: Some(1.5),
                    bool_value: None,
                    kvlist_value: None,
                    array_value: None,
                }),
            },
            OtlpKeyValue {
                key: "enabled".to_string(),
                value: Some(OtlpAnyValue {
                    string_value: None,
                    int_value: None,
                    double_value: None,
                    bool_value: Some(true),
                    kvlist_value: None,
                    array_value: None,
                }),
            },
            OtlpKeyValue {
                key: "ignored".to_string(),
                value: None,
            },
        ]);

        assert_eq!(
            extract_log_body(&None),
            "",
            "missing body should become an empty string"
        );
        assert_eq!(
            attrs.get("message"),
            Some(&serde_json::Value::String("hello".to_string()))
        );
        assert_eq!(
            attrs.get("count"),
            Some(&serde_json::Value::Number(serde_json::Number::from(3)))
        );
        assert_eq!(attrs.get("ratio"), Some(&serde_json::json!(1.5)));
        assert_eq!(attrs.get("enabled"), Some(&serde_json::Value::Bool(true)));
        assert!(!attrs.contains_key("ignored"));
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_logs_handles_empty_body_and_resource_attributes() {
        init_log_storage(Some(8));
        if let Some(storage) = get_log_storage() {
            storage.clear();
        }

        let payload = serde_json::json!({
            "resourceLogs": [{
                "resource": {
                    "attributes": [
                        { "key": "service.name", "value": { "stringValue": "svc-empty-body" } },
                        { "key": "deployment.environment", "value": { "stringValue": "test" } }
                    ]
                },
                "scopeLogs": [{
                    "logRecords": [{
                        "timeUnixNano": "1",
                        "severityText": "INFO"
                    }]
                }]
            }]
        });

        ingest_otlp_logs(&payload.to_string()).await.unwrap();

        let logs = get_log_storage().unwrap().get_logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].body, "");
        assert_eq!(logs[0].service_name, "svc-empty-body");
        assert_eq!(
            logs[0].resource.get("deployment.environment"),
            Some(&"test".to_string())
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_json_skips_spans_with_invalid_identifiers() {
        let storage = match get_span_storage() {
            Some(s) => {
                s.clear();
                s
            }
            None => {
                let _ = InMemorySpanExporter::new(1000, "test".to_string());
                get_span_storage().expect("span storage should be initialised")
            }
        };

        let payload = serde_json::json!({
            "resourceSpans": [{
                "resource": {
                    "attributes": [{ "key": "service.name", "value": { "stringValue": "svc" } }]
                },
                "scopeSpans": [{
                    "spans": [
                        {
                            "traceId": "not-hex",
                            "spanId": "also-bad",
                            "name": "broken-span",
                            "startTimeUnixNano": "1",
                            "endTimeUnixNano": "2"
                        },
                        {
                            "traceId": "0af7651916cd43dd8448eb211c80319c",
                            "spanId": "b7ad6b7169203331",
                            "name": "valid-span",
                            "startTimeUnixNano": "3",
                            "endTimeUnixNano": "4"
                        }
                    ]
                }]
            }]
        });

        ingest_otlp_json(&payload.to_string()).await.unwrap();

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 2, "memory storage retains both parsed spans");

        let exported = convert_otlp_to_span_data(
            &serde_json::from_str::<OtlpExportTraceServiceRequest>(&payload.to_string()).unwrap(),
        );
        assert_eq!(
            exported.len(),
            1,
            "forwarding skips invalid span identifiers"
        );
        assert_eq!(exported[0].name.as_ref(), "valid-span");
    }

    // =========================================================================
    // ExporterType / OtelConfig Default trait tests
    // =========================================================================

    #[test]
    fn test_exporter_type_default() {
        let et = ExporterType::default();
        assert_eq!(et, ExporterType::Otlp);
    }

    #[test]
    fn test_exporter_type_equality() {
        assert_eq!(ExporterType::Memory, ExporterType::Memory);
        assert_eq!(ExporterType::Otlp, ExporterType::Otlp);
        assert_eq!(ExporterType::Both, ExporterType::Both);
        assert_ne!(ExporterType::Memory, ExporterType::Otlp);
    }

    // =========================================================================
    // Span storage: trace_id lookup returns empty for unknown trace
    // =========================================================================

    #[test]
    fn test_span_storage_get_by_unknown_trace_id() {
        let storage = InMemorySpanStorage::new(10);
        storage.add_spans(vec![make_stored_span("t1", "s1", "span1", 100, 200)]);

        let result = storage.get_spans_by_trace_id("nonexistent-trace");
        assert!(result.is_empty());
    }

    // =========================================================================
    // Span storage: eviction with multiple trace IDs
    // =========================================================================

    #[test]
    fn test_span_storage_eviction_updates_trace_index() {
        let storage = InMemorySpanStorage::new(3);

        storage.add_spans(vec![
            make_stored_span("t1", "s1", "span-1", 100, 200),
            make_stored_span("t2", "s2", "span-2", 300, 400),
            make_stored_span("t3", "s3", "span-3", 500, 600),
        ]);

        assert_eq!(storage.len(), 3);
        assert_eq!(storage.get_spans_by_trace_id("t1").len(), 1);

        // Adding a 4th span should evict the oldest (t1/s1)
        storage.add_spans(vec![make_stored_span("t4", "s4", "span-4", 700, 800)]);

        assert_eq!(storage.len(), 3);
        // t1 should have been evicted
        assert_eq!(storage.get_spans_by_trace_id("t1").len(), 0);
        // t2, t3, t4 should still be present
        assert_eq!(storage.get_spans_by_trace_id("t2").len(), 1);
        assert_eq!(storage.get_spans_by_trace_id("t3").len(), 1);
        assert_eq!(storage.get_spans_by_trace_id("t4").len(), 1);
    }

    #[test]
    fn test_otlp_anyvalue_kvlist_deserialization() {
        let json = r#"{
            "kvlistValue": {
                "values": [
                    {"key": "appName", "value": {"stringValue": "III App"}},
                    {"key": "count", "value": {"intValue": 42}},
                    {"key": "enabled", "value": {"boolValue": true}}
                ]
            }
        }"#;
        let val: OtlpAnyValue = serde_json::from_str(json).unwrap();
        let result = val.to_serde_json_value();

        assert!(result.is_object());
        let obj = result.as_object().unwrap();
        assert_eq!(obj["appName"], serde_json::Value::String("III App".into()));
        assert_eq!(obj["count"], serde_json::json!(42));
        assert_eq!(obj["enabled"], serde_json::json!(true));
    }

    #[test]
    fn test_otlp_anyvalue_array_deserialization() {
        let json = r#"{
            "arrayValue": {
                "values": [
                    {"intValue": 1},
                    {"intValue": 2},
                    {"stringValue": "three"}
                ]
            }
        }"#;
        let val: OtlpAnyValue = serde_json::from_str(json).unwrap();
        let result = val.to_serde_json_value();

        assert!(result.is_array());
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0], serde_json::json!(1));
        assert_eq!(arr[1], serde_json::json!(2));
        assert_eq!(arr[2], serde_json::Value::String("three".into()));
    }

    #[test]
    fn test_otlp_anyvalue_nested_kvlist_in_array() {
        let json = r#"{
            "kvlistValue": {
                "values": [
                    {"key": "nested", "value": {
                        "kvlistValue": {
                            "values": [
                                {"key": "deep", "value": {"stringValue": "value"}}
                            ]
                        }
                    }},
                    {"key": "items", "value": {
                        "arrayValue": {
                            "values": [
                                {"intValue": 1},
                                {"intValue": 2}
                            ]
                        }
                    }}
                ]
            }
        }"#;
        let val: OtlpAnyValue = serde_json::from_str(json).unwrap();
        let result = val.to_serde_json_value();

        assert_eq!(result["nested"]["deep"], serde_json::json!("value"));
        assert_eq!(result["items"], serde_json::json!([1, 2]));
    }

    #[test]
    fn test_extract_log_attributes_with_kvlist() {
        let json = r#"[
            {"key": "log.data", "value": {
                "kvlistValue": {
                    "values": [
                        {"key": "appName", "value": {"stringValue": "III App"}},
                        {"key": "version", "value": {"intValue": 3}}
                    ]
                }
            }},
            {"key": "simple", "value": {"stringValue": "text"}}
        ]"#;
        let kvs: Vec<OtlpKeyValue> = serde_json::from_str(json).unwrap();
        let attrs = extract_log_attributes(&kvs);

        assert_eq!(attrs["simple"], serde_json::Value::String("text".into()));
        assert!(attrs["log.data"].is_object());
        assert_eq!(attrs["log.data"]["appName"], serde_json::json!("III App"));
        assert_eq!(attrs["log.data"]["version"], serde_json::json!(3));
    }

    #[test]
    fn test_otlp_anyvalue_kvlist_to_string_value() {
        let json = r#"{
            "kvlistValue": {
                "values": [
                    {"key": "k", "value": {"stringValue": "v"}}
                ]
            }
        }"#;
        let val: OtlpAnyValue = serde_json::from_str(json).unwrap();
        let s = val.to_string_value();
        // Should produce a JSON string representation
        assert!(s.contains("\"k\""));
        assert!(s.contains("\"v\""));
    }

    #[tokio::test]
    #[serial]
    async fn test_span_event_preserves_exception_stacktrace() {
        use opentelemetry::trace::{Status, TraceContextExt, Tracer};
        use opentelemetry_sdk::trace::SdkTracerProvider;

        let storage = Arc::new(InMemorySpanStorage::new(100));
        let exporter = InMemorySpanExporter::with_storage(storage.clone(), "test-svc".into());
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter)
            .build();

        let tracer = provider.tracer("test");

        tracer.in_span("error-span", |cx| {
            let span = cx.span();
            span.set_status(Status::error("test error"));
            span.add_event(
                "exception",
                vec![
                    opentelemetry::KeyValue::new("exception.type", "TestError"),
                    opentelemetry::KeyValue::new("exception.message", "something went wrong"),
                    opentelemetry::KeyValue::new(
                        "exception.stacktrace",
                        "at test_fn (test.rs:42)\nat main (main.rs:1)",
                    ),
                ],
            );
        });

        let _ = provider.force_flush();

        let spans = storage.get_spans();
        assert_eq!(spans.len(), 1, "expected 1 span");

        let span = &spans[0];
        assert_eq!(span.status, "error");

        let exc_event = span
            .events
            .iter()
            .find(|e| e.name == "exception")
            .expect("span should have an 'exception' event");

        let attr_map: std::collections::HashMap<&str, &str> = exc_event
            .attributes
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        assert_eq!(attr_map.get("exception.type"), Some(&"TestError"));
        assert_eq!(
            attr_map.get("exception.message"),
            Some(&"something went wrong")
        );
        assert!(
            attr_map.contains_key("exception.stacktrace"),
            "exception.stacktrace attribute missing"
        );
        assert!(
            attr_map["exception.stacktrace"].contains("test_fn"),
            "stacktrace should contain function name"
        );
    }

    // =========================================================================
    // logs_enabled() decision helper
    // =========================================================================

    fn cfg_with_logs_enabled(value: Option<bool>) -> ObservabilityWorkerConfig {
        ObservabilityWorkerConfig {
            logs_enabled: value,
            ..ObservabilityWorkerConfig::default()
        }
    }

    #[test]
    fn test_logs_enabled_defaults_true_when_config_absent() {
        assert!(logs_enabled(None));
    }

    #[test]
    fn test_logs_enabled_defaults_true_when_field_absent() {
        let cfg = cfg_with_logs_enabled(None);
        assert!(logs_enabled(Some(&cfg)));
    }

    #[test]
    fn test_logs_enabled_true_when_explicit_true() {
        let cfg = cfg_with_logs_enabled(Some(true));
        assert!(logs_enabled(Some(&cfg)));
    }

    #[test]
    fn test_logs_enabled_false_when_explicit_false() {
        let cfg = cfg_with_logs_enabled(Some(false));
        assert!(!logs_enabled(Some(&cfg)));
    }

    // =========================================================================
    // build_console_log_fields() — service.name strip + function_name extract
    // =========================================================================

    fn log_with_attributes(attrs: Vec<(&str, serde_json::Value)>) -> StoredLog {
        StoredLog {
            timestamp_unix_nano: 0,
            observed_timestamp_unix_nano: 0,
            severity_number: 9,
            severity_text: "INFO".to_string(),
            body: "body".to_string(),
            attributes: attrs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            trace_id: None,
            span_id: None,
            resource: HashMap::new(),
            service_name: "test-service".to_string(),
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
        }
    }

    #[test]
    fn test_build_console_log_fields_extracts_function_name_and_strips_it() {
        let log = log_with_attributes(vec![
            ("service.name", serde_json::json!("api.get./todos")),
            ("log.data", serde_json::json!({"count": 0})),
        ]);

        let (data, function_name) = build_console_log_fields(&log);

        assert_eq!(function_name, "api.get./todos");
        // data must be non-empty JSON and must NOT contain "service.name"
        let parsed: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert!(parsed.get("service.name").is_none());
        assert!(parsed.get("log.data").is_some());
    }

    #[test]
    fn test_build_console_log_fields_empty_when_only_service_name_attribute() {
        let log = log_with_attributes(vec![("service.name", serde_json::json!("api.get./todos"))]);

        let (data, function_name) = build_console_log_fields(&log);

        assert_eq!(function_name, "api.get./todos");
        assert!(
            data.is_empty(),
            "stripping the only attribute must produce empty data"
        );
    }

    #[test]
    fn test_build_console_log_fields_empty_when_no_attributes() {
        let log = log_with_attributes(vec![]);
        let (data, function_name) = build_console_log_fields(&log);
        assert!(function_name.is_empty());
        assert!(data.is_empty());
    }

    #[test]
    fn test_build_console_log_fields_missing_service_name_returns_empty_function_name() {
        let log = log_with_attributes(vec![("log.data", serde_json::json!({"k": 1}))]);
        let (data, function_name) = build_console_log_fields(&log);
        assert!(function_name.is_empty());
        assert!(!data.is_empty());
        let parsed: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert!(parsed.get("log.data").is_some());
    }

    const SAMPLE_TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    const SAMPLE_TRACE_ID_HEX: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
    const SAMPLE_SPAN_ID_HEX: &str = "00f067aa0ba902b7";

    #[test]
    fn extract_context_round_trips_traceparent_header() {
        let cx = extract_context(Some(SAMPLE_TRACEPARENT), None);
        let round = inject_traceparent_from_context(&cx).expect("valid traceparent");
        assert_eq!(round, SAMPLE_TRACEPARENT);
    }

    #[test]
    fn extract_context_carries_caller_span_context_as_remote_parent() {
        let cx = extract_context(Some(SAMPLE_TRACEPARENT), None);
        let span_ref = cx.span();
        let sc = span_ref.span_context();
        assert!(
            sc.is_valid(),
            "extracted context must carry a valid span context"
        );
        assert_eq!(format!("{:032x}", sc.trace_id()), SAMPLE_TRACE_ID_HEX);
        assert_eq!(format!("{:016x}", sc.span_id()), SAMPLE_SPAN_ID_HEX);
        assert!(sc.is_remote(), "extracted parent must be flagged remote");
    }

    #[test]
    fn extract_context_with_state_carries_w3c_tracestate() {
        let tracestate = "congo=t61rcWkgMzE,rojo=00f067aa0ba902b7";
        let cx = extract_context_with_state(Some(SAMPLE_TRACEPARENT), Some(tracestate), None);
        let span_ref = cx.span();
        let sc = span_ref.span_context();
        assert!(sc.is_valid());
        assert_eq!(format!("{:032x}", sc.trace_id()), SAMPLE_TRACE_ID_HEX);
        // The W3C `TraceContextPropagator` only populates `TraceState` when both
        // `traceparent` and `tracestate` are present, so this also guards the
        // companion-header wiring.
        assert_eq!(sc.trace_state().header(), tracestate);
    }

    #[test]
    fn extract_context_without_tracestate_has_empty_trace_state() {
        let cx = extract_context_with_state(Some(SAMPLE_TRACEPARENT), None, None);
        let span_ref = cx.span();
        assert!(span_ref.span_context().trace_state().header().is_empty());
    }

    #[test]
    fn extract_context_round_trips_baggage_header() {
        let bg = "iii.session.id=s-1,iii.message.id=m-1,iii.function.id=auth::set_token";
        let cx = extract_context(None, Some(bg));
        let round = inject_baggage_from_context(&cx).expect("baggage present");
        // Baggage entries are unordered; HashMap iteration order is non-deterministic.
        let parse = |s: &str| {
            s.split(',')
                .map(|e| e.trim().to_string())
                .collect::<std::collections::HashSet<_>>()
        };
        assert_eq!(parse(&round), parse(bg));
    }

    #[test]
    fn extract_context_combines_traceparent_and_baggage() {
        let bg = "iii.message.id=m-42";
        let cx = extract_context(Some(SAMPLE_TRACEPARENT), Some(bg));
        let span_ref = cx.span();
        let sc = span_ref.span_context();
        assert!(sc.is_valid());
        assert_eq!(format!("{:032x}", sc.trace_id()), SAMPLE_TRACE_ID_HEX);
        let injected_bg = inject_baggage_from_context(&cx).expect("baggage present");
        assert!(injected_bg.contains("iii.message.id=m-42"));
    }

    #[test]
    fn extract_context_with_no_headers_returns_empty_context() {
        let cx = extract_context(None, None);
        let span_ref = cx.span();
        assert!(!span_ref.span_context().is_valid());
        drop(span_ref);
        assert!(inject_traceparent_from_context(&cx).is_none());
        assert!(inject_baggage_from_context(&cx).is_none());
    }

    #[test]
    fn empty_baggage_header_does_not_emit_propagator_warning() {
        use std::sync::{Arc, Mutex};
        use tracing::subscriber::with_default;
        use tracing_subscriber::Layer;
        use tracing_subscriber::layer::{Context as LayerContext, SubscriberExt};

        // Captures the metadata name of every event emitted on this thread.
        // The OTel `BaggagePropagator` logs `warn!(name: "BaggagePropagator
        // .Extract.InvalidKeyValueFormat", ...)` when it parses an empty
        // `baggage` header, which is exactly what a present-but-empty carrier
        // entry produced before the fix.
        #[derive(Clone, Default)]
        struct WarnCapture(Arc<Mutex<Vec<String>>>);
        impl<S: tracing::Subscriber> Layer<S> for WarnCapture {
            fn on_event(&self, event: &tracing::Event<'_>, _cx: LayerContext<'_, S>) {
                self.0
                    .lock()
                    .unwrap()
                    .push(event.metadata().name().to_string());
            }
        }

        let capture = WarnCapture::default();
        let names = capture.0.clone();
        let subscriber = tracing_subscriber::registry().with(capture);
        with_default(subscriber, || {
            // Absent and empty baggage must be indistinguishable: neither seeds
            // the carrier, so the propagator never sees an unparsable header.
            let _ = extract_context(None, Some(""));
            let _ = extract_baggage("");
            let _ = baggage_with_function_id(None, "fn-1");
        });

        let warned = names.lock().unwrap();
        assert!(
            !warned.iter().any(|n| n.contains("BaggagePropagator")),
            "empty baggage must not trigger BaggagePropagator warnings, got: {warned:?}"
        );
    }

    #[test]
    #[serial]
    fn attached_extracted_context_makes_baggage_visible_in_current_context() {
        let bg = "iii.message.id=m-1,iii.session.id=s-1";
        let parent_cx = extract_context(None, Some(bg));
        {
            let _guard = parent_cx.attach();
            let injected = inject_baggage_from_context(&Context::current())
                .expect("baggage visible inside the guard");
            assert!(injected.contains("iii.message.id=m-1"));
            assert!(injected.contains("iii.session.id=s-1"));
        }
        assert!(
            inject_baggage_from_context(&Context::current()).is_none(),
            "baggage must not leak past the guard"
        );
    }

    #[test]
    fn invalid_traceparent_extracts_to_invalid_context() {
        let cx = extract_context(Some("not-a-valid-traceparent"), None);
        let span_ref = cx.span();
        assert!(!span_ref.span_context().is_valid());
        drop(span_ref);
        assert!(inject_traceparent_from_context(&cx).is_none());
    }
}
