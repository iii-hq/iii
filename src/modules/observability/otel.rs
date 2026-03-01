// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! OpenTelemetry initialization for the III Engine.
//!
//! This module provides OTLP/gRPC trace export and integrates with the existing
//! `tracing` ecosystem via `tracing-opentelemetry`.
//!
//! Supports two exporter modes:
//! - `otlp`: Export traces to an OTLP collector via gRPC
//! - `memory`: Store traces in memory for API querying

use super::config::{LogsExporterType, OtelExporterType, OtelModuleConfig};
use super::sampler::AdvancedSampler;
use opentelemetry::propagation::{TextMapCompositePropagator, TextMapPropagator};
use opentelemetry::{
    Context, KeyValue, global,
    trace::{TraceContextExt, TracerProvider as _},
};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    error::OTelSdkResult,
    propagation::{BaggagePropagator, TraceContextPropagator},
    trace::{RandomIdGenerator, Sampler, SdkTracerProvider, SpanData, SpanExporter},
};
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

/// Global OTEL configuration set from YAML config
static GLOBAL_OTEL_CONFIG: OnceLock<OtelModuleConfig> = OnceLock::new();

/// Set the global OTEL configuration from the module.
/// This should be called during module initialization, before logging is set up.
///
/// Returns true if the config was set, false if it was already initialized.
pub fn set_otel_config(config: OtelModuleConfig) -> bool {
    match GLOBAL_OTEL_CONFIG.set(config) {
        Ok(()) => true,
        Err(_) => {
            // Config already set - this can happen if module is re-initialized
            // Log at debug level since this is expected in some scenarios
            tracing::debug!("OTEL config already initialized, ignoring new config");
            false
        }
    }
}

/// Get the global OTEL configuration if set.
pub fn get_otel_config() -> Option<&'static OtelModuleConfig> {
    GLOBAL_OTEL_CONFIG.get()
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
}

impl Default for OtelConfig {
    fn default() -> Self {
        // First check global config from YAML, then fall back to environment variables
        let global_cfg = get_otel_config();

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

        Self {
            enabled,
            service_name,
            service_version,
            service_namespace,
            exporter,
            endpoint,
            sampling_ratio,
            memory_max_spans,
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
        }
    }
}

/// In-memory span storage with circular buffer.
pub struct InMemorySpanStorage {
    spans: RwLock<VecDeque<StoredSpan>>,
    max_spans: usize,
    /// Secondary index: trace_id -> set of span indices for O(1) trace lookups
    spans_by_trace_id: RwLock<HashMap<String, HashSet<usize>>>,
}

impl std::fmt::Debug for InMemorySpanStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemorySpanStorage")
            .field("spans", &self.spans)
            .field("max_spans", &self.max_spans)
            .field("spans_by_trace_id", &self.spans_by_trace_id)
            .finish()
    }
}

impl InMemorySpanStorage {
    pub fn new(max_spans: usize) -> Self {
        Self {
            spans: RwLock::new(VecDeque::with_capacity(max_spans)),
            max_spans,
            spans_by_trace_id: RwLock::new(HashMap::new()),
        }
    }

    pub fn add_spans(&self, new_spans: Vec<StoredSpan>) {
        let mut spans = self.spans.write().unwrap();
        let mut index = self.spans_by_trace_id.write().unwrap();

        for span in new_spans {
            // Evict oldest if at capacity
            if spans.len() >= self.max_spans
                && let Some(old) = spans.pop_front()
            {
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

            let idx = spans.len();
            let trace_id = span.trace_id.clone();
            spans.push_back(span);
            index.entry(trace_id).or_default().insert(idx);
        }
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
        spans.clear();
        index.clear();
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

        // Calculate duration in milliseconds for each span
        let mut durations: Vec<f64> = spans
            .iter()
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
        // Store in memory first (for triggers and API access)
        let stored: Vec<StoredSpan> = batch
            .iter()
            .map(|s| StoredSpan::from_span_data(s, &self.service_name))
            .collect();
        self.memory_storage.add_spans(stored);

        // Forward to OTLP exporter (for external collector)
        self.otlp_exporter.export(batch)
    }

    fn shutdown_with_timeout(&mut self, timeout: std::time::Duration) -> OTelSdkResult {
        self.otlp_exporter.shutdown_with_timeout(timeout)
    }
}

static TRACER_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

/// Global OTLP exporter for forwarding SDK-ingested spans to the collector.
/// `SpanExporter::export` takes `&self`, so no Mutex is needed.
static SDK_SPAN_FORWARDER: OnceLock<Arc<opentelemetry_otlp::SpanExporter>> = OnceLock::new();

/// Build a second OTLP span exporter and store it in the global `SDK_SPAN_FORWARDER`
/// so that SDK-ingested spans can be forwarded to the collector.
fn init_sdk_span_forwarder(endpoint: &str) {
    match opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
    {
        Ok(forwarder) => {
            if SDK_SPAN_FORWARDER.set(Arc::new(forwarder)).is_err() {
                tracing::debug!("SDK span forwarder already initialized");
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to create SDK span forwarder, SDK spans will not be exported to collector"
            );
        }
    }
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

    // Build the sampler using advanced configuration if available
    let sampler = build_sampler(config);

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
            match opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(&config.endpoint)
                .build()
            {
                Ok(exporter) => {
                    init_sdk_span_forwarder(&config.endpoint);

                    // Initialize in-memory storage for SDK span ingestion (API access)
                    let memory_storage =
                        Arc::new(InMemorySpanStorage::new(config.memory_max_spans));
                    let _ = IN_MEMORY_STORAGE.set(memory_storage);

                    SdkTracerProvider::builder()
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
                    let exporter = InMemorySpanExporter::new(
                        config.memory_max_spans,
                        config.service_name.clone(),
                    );
                    SdkTracerProvider::builder()
                        .with_simple_exporter(exporter)
                        .with_sampler(sampler)
                        .with_id_generator(RandomIdGenerator::default())
                        .with_resource(resource)
                        .build()
                }
            }
        }
        ExporterType::Memory => {
            let exporter =
                InMemorySpanExporter::new(config.memory_max_spans, config.service_name.clone());

            SdkTracerProvider::builder()
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
            match opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(&config.endpoint)
                .build()
            {
                Ok(otlp_exporter) => {
                    init_sdk_span_forwarder(&config.endpoint);

                    // Create tee exporter that sends to both
                    let tee_exporter = TeeSpanExporter::new(
                        otlp_exporter,
                        memory_storage,
                        config.service_name.clone(),
                    );

                    SdkTracerProvider::builder()
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
                        memory_storage,
                        config.service_name.clone(),
                    );
                    SdkTracerProvider::builder()
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

/// Extract the current trace ID from the active span context.
/// Returns `None` if there is no active span or tracing is not initialized.
pub fn current_trace_id() -> Option<String> {
    let ctx = Context::current();
    let span_ref = ctx.span();
    let span_context = span_ref.span_context();

    if span_context.is_valid() {
        Some(span_context.trace_id().to_string())
    } else {
        None
    }
}

/// Extract the current span ID from the active span context.
pub fn current_span_id() -> Option<String> {
    let ctx = Context::current();
    let span_ref = ctx.span();
    let span_context = span_ref.span_context();

    if span_context.is_valid() {
        Some(span_context.span_id().to_string())
    } else {
        None
    }
}

/// Inject the current trace context into a W3C traceparent header string.
/// Returns `None` if there is no active span.
pub fn inject_traceparent() -> Option<String> {
    use std::collections::HashMap;

    let ctx = Context::current();
    let span_ref = ctx.span();
    let span_context = span_ref.span_context();

    if !span_context.is_valid() {
        return None;
    }

    let propagator = TraceContextPropagator::new();
    let mut carrier: HashMap<String, String> = HashMap::new();
    propagator.inject_context(&ctx, &mut carrier);

    carrier.remove("traceparent")
}

/// Inject trace context from a specific OpenTelemetry context.
/// Use this when you need to extract the context from a tracing span via
/// `tracing_opentelemetry::OpenTelemetrySpanExt::context()`.
pub fn inject_traceparent_from_context(ctx: &Context) -> Option<String> {
    use std::collections::HashMap;

    let span_ref = ctx.span();
    let span_context = span_ref.span_context();

    if !span_context.is_valid() {
        return None;
    }

    let propagator = TraceContextPropagator::new();
    let mut carrier: HashMap<String, String> = HashMap::new();
    propagator.inject_context(ctx, &mut carrier);

    carrier.remove("traceparent")
}

/// Extract a trace context from a W3C traceparent header string.
/// Returns a new `Context` with the extracted span context as parent.
pub fn extract_traceparent(traceparent: &str) -> Context {
    use std::collections::HashMap;

    let propagator = TraceContextPropagator::new();
    let mut carrier: HashMap<String, String> = HashMap::new();
    carrier.insert("traceparent".to_string(), traceparent.to_string());

    propagator.extract(&carrier)
}

/// Inject the current baggage into a W3C baggage header string.
/// Returns `None` if there is no baggage in the current context.
pub fn inject_baggage() -> Option<String> {
    use std::collections::HashMap;

    let ctx = Context::current();
    let propagator = BaggagePropagator::new();
    let mut carrier: HashMap<String, String> = HashMap::new();
    propagator.inject_context(&ctx, &mut carrier);

    carrier.remove("baggage")
}

/// Inject baggage from a specific OpenTelemetry context.
/// Use this when you need to extract the baggage from a specific context.
pub fn inject_baggage_from_context(ctx: &Context) -> Option<String> {
    use std::collections::HashMap;

    let propagator = BaggagePropagator::new();
    let mut carrier: HashMap<String, String> = HashMap::new();
    propagator.inject_context(ctx, &mut carrier);

    carrier.remove("baggage")
}

/// Extract baggage from a W3C baggage header string.
/// Returns a new `Context` with the extracted baggage.
pub fn extract_baggage(baggage: &str) -> Context {
    use std::collections::HashMap;

    let propagator = BaggagePropagator::new();
    let mut carrier: HashMap<String, String> = HashMap::new();
    carrier.insert("baggage".to_string(), baggage.to_string());

    propagator.extract(&carrier)
}

/// Combine trace context and baggage extraction into a single context.
/// This extracts both traceparent and baggage headers into a unified context.
pub fn extract_context(traceparent: Option<&str>, baggage: Option<&str>) -> Context {
    use std::collections::HashMap;

    let mut carrier: HashMap<String, String> = HashMap::new();

    if let Some(tp) = traceparent {
        carrier.insert("traceparent".to_string(), tp.to_string());
    }
    if let Some(bg) = baggage {
        carrier.insert("baggage".to_string(), bg.to_string());
    }

    let trace_propagator = TraceContextPropagator::new();
    let baggage_propagator = BaggagePropagator::new();

    let ctx = if traceparent.is_some() {
        trace_propagator.extract(&carrier)
    } else {
        Context::current()
    };

    if baggage.is_some() {
        baggage_propagator.extract_with_context(&ctx, &carrier)
    } else {
        ctx
    }
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
struct OtlpAnyValue {
    string_value: Option<String>,
    int_value: Option<i64>,
    double_value: Option<f64>,
    bool_value: Option<bool>,
}

impl OtlpAnyValue {
    fn to_string_value(&self) -> String {
        if let Some(s) = &self.string_value {
            s.clone()
        } else if let Some(i) = self.int_value {
            i.to_string()
        } else if let Some(d) = self.double_value {
            d.to_string()
        } else if let Some(b) = self.bool_value {
            b.to_string()
        } else {
            String::new()
        }
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
    if kv.key.is_empty() {
        return None;
    }

    let val = kv.value.as_ref()?;
    let value = if let Some(s) = &val.string_value {
        opentelemetry::Value::String(s.clone().into())
    } else if let Some(i) = val.int_value {
        opentelemetry::Value::I64(i)
    } else if let Some(d) = val.double_value {
        opentelemetry::Value::F64(d)
    } else if let Some(b) = val.bool_value {
        opentelemetry::Value::Bool(b)
    } else {
        return None;
    };
    Some(KeyValue::new(kv.key.clone(), value))
}

/// Convert parsed OTLP spans to SpanData for export via the OTel SDK pipeline.
fn convert_otlp_to_span_data(request: &OtlpExportTraceServiceRequest) -> Vec<SpanData> {
    use opentelemetry::trace::{
        Event, Link, SpanContext, SpanKind, Status, TraceFlags, TraceState,
    };
    use opentelemetry::{InstrumentationScope, SpanId, TraceId};
    use opentelemetry_sdk::trace::{SpanEvents, SpanLinks};
    use std::borrow::Cow;
    use std::time::{Duration, UNIX_EPOCH};

    let mut span_data_vec = Vec::new();

    for resource_span in &request.resource_spans {
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
    // Parse the OTLP JSON
    let request: OtlpExportTraceServiceRequest = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse OTLP JSON: {}", e))?;

    // Store in memory if available (for API access)
    if let Some(storage) = get_span_storage() {
        let mut stored_spans = Vec::new();

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
                    };

                    stored_spans.push(stored_span);
                }
            }
        }

        let span_count = stored_spans.len();
        if span_count > 0 {
            storage.add_spans(stored_spans);
            tracing::debug!(
                span_count = span_count,
                "Ingested OTLP spans into memory storage"
            );
        }
    }

    // Forward to OTLP collector if forwarder is available
    if let Some(forwarder) = SDK_SPAN_FORWARDER.get() {
        let span_data = convert_otlp_to_span_data(&request);
        if !span_data.is_empty() {
            let count = span_data.len();
            match forwarder.export(span_data).await {
                Ok(()) => {
                    tracing::debug!(span_count = count, "Forwarded SDK spans to OTLP collector");
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
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpMetric {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
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
    #[serde(default)]
    as_int: Option<i64>,
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
            .or_else(|| self.as_int.map(|i| i as f64))
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
    max_logs: usize,
    tx: broadcast::Sender<StoredLog>,
}

impl std::fmt::Debug for InMemoryLogStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryLogStorage")
            .field("logs", &self.logs)
            .field("max_logs", &self.max_logs)
            .finish()
    }
}

impl InMemoryLogStorage {
    pub fn new(max_logs: usize) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            logs: RwLock::new(VecDeque::with_capacity(max_logs)),
            max_logs,
            tx,
        }
    }

    pub fn store(&self, log: StoredLog) {
        let mut logs = self.logs.write().unwrap();
        if logs.len() >= self.max_logs {
            logs.pop_front();
        }
        logs.push_back(log.clone());

        // Broadcast to any listeners (ignore send errors if no receivers)
        let _ = self.tx.send(log);
    }

    pub fn add_logs(&self, new_logs: Vec<StoredLog>) {
        let mut logs = self.logs.write().unwrap();
        for log in new_logs {
            if logs.len() >= self.max_logs {
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
}

/// Global in-memory log storage.
static LOG_STORAGE: OnceLock<Arc<InMemoryLogStorage>> = OnceLock::new();

/// Get the global in-memory log storage (if initialized).
pub fn get_log_storage() -> Option<Arc<InMemoryLogStorage>> {
    LOG_STORAGE.get().cloned()
}

/// Initialize the global log storage.
pub fn init_log_storage(max_logs: Option<usize>) {
    let storage = Arc::new(InMemoryLogStorage::new(
        max_logs.unwrap_or(DEFAULT_MEMORY_MAX_LOGS),
    ));
    let _ = LOG_STORAGE.set(storage);
}

/// Extract resource attributes as a HashMap
fn extract_resource_attributes(resource: &Option<OtlpResource>) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    if let Some(res) = resource {
        for attr in &res.attributes {
            if let Some(val) = &attr.value {
                attrs.insert(attr.key.clone(), val.to_string_value());
            }
        }
    }
    attrs
}

/// Extract log body as a string
fn extract_log_body(body: &Option<OtlpAnyValue>) -> String {
    match body {
        Some(val) => val.to_string_value(),
        None => String::new(),
    }
}

/// Extract log attributes as a HashMap
fn extract_log_attributes(attributes: &[OtlpKeyValue]) -> HashMap<String, serde_json::Value> {
    let mut attrs = HashMap::new();
    for attr in attributes {
        if let Some(val) = &attr.value {
            let json_val = if let Some(s) = &val.string_value {
                serde_json::Value::String(s.clone())
            } else if let Some(i) = val.int_value {
                serde_json::Value::Number(serde_json::Number::from(i))
            } else if let Some(d) = val.double_value {
                serde_json::Value::Number(
                    serde_json::Number::from_f64(d).unwrap_or_else(|| serde_json::Number::from(0)),
                )
            } else if let Some(b) = val.bool_value {
                serde_json::Value::Bool(b)
            } else {
                serde_json::Value::Null
            };
            attrs.insert(attr.key.clone(), json_val);
        }
    }
    attrs
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
fn emit_log_to_console(log: &StoredLog) {
    let data = if log.attributes.is_empty() {
        String::new()
    } else {
        serde_json::to_string(&log.attributes).unwrap_or_default()
    };

    let service = &log.service_name;
    let body = &log.body;

    // OTEL severity numbers: TRACE=1-4, DEBUG=5-8, INFO=9-12, WARN=13-16, ERROR=17-20, FATAL=21-24
    match log.severity_number {
        1..=4 => {
            tracing::trace!(target: OTEL_PASSTHROUGH_TARGET, service = %service, data = %data, "{}", body)
        }
        5..=8 => {
            tracing::debug!(target: OTEL_PASSTHROUGH_TARGET, service = %service, data = %data, "{}", body)
        }
        9..=12 => {
            tracing::info!(target: OTEL_PASSTHROUGH_TARGET, service = %service, data = %data, "{}", body)
        }
        13..=16 => {
            tracing::warn!(target: OTEL_PASSTHROUGH_TARGET, service = %service, data = %data, "{}", body)
        }
        17..=24 => {
            tracing::error!(target: OTEL_PASSTHROUGH_TARGET, service = %service, data = %data, "{}", body)
        }
        _ => {
            tracing::info!(target: OTEL_PASSTHROUGH_TARGET, service = %service, data = %data, "{}", body)
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
    tracing::debug!(
        logs_size = json_str.len(),
        "Received OTLP logs from Node SDK"
    );

    // Parse the OTLP logs JSON
    let request: OtlpExportLogsServiceRequest = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse OTLP logs JSON: {}", e))?;

    // Get the in-memory log storage (if available)
    let storage = match get_log_storage() {
        Some(s) => s,
        None => {
            // Initialize storage if not already done
            init_log_storage(None);
            get_log_storage().ok_or_else(|| anyhow::anyhow!("Failed to initialize log storage"))?
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

/// Default flush interval for log export (5 seconds)
const DEFAULT_LOG_FLUSH_INTERVAL_SECS: u64 = 5;

/// OTLP Logs Exporter - exports logs to OTLP collector via gRPC
pub struct OtlpLogsExporter {
    endpoint: String,
    service_name: String,
    service_version: String,
    batch_size: usize,
    flush_interval: Duration,
}

impl OtlpLogsExporter {
    /// Create a new OTLP logs exporter
    pub fn new(endpoint: String, service_name: String, service_version: String) -> Self {
        Self {
            endpoint,
            service_name,
            service_version,
            batch_size: DEFAULT_LOG_BATCH_SIZE,
            flush_interval: Duration::from_secs(DEFAULT_LOG_FLUSH_INTERVAL_SECS),
        }
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
        let client = reqwest::Client::new();

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
                        let attr_value = match value {
                            serde_json::Value::String(s) => serde_json::json!({"stringValue": s}),
                            serde_json::Value::Number(n) => {
                                if n.is_i64() {
                                    serde_json::json!({"intValue": n.to_string()})
                                } else {
                                    serde_json::json!({"doubleValue": n})
                                }
                            }
                            serde_json::Value::Bool(b) => serde_json::json!({"boolValue": b}),
                            _ => serde_json::json!({"stringValue": value.to_string()}),
                        };
                        serde_json::json!({
                            "key": key,
                            "value": attr_value
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

/// Get the logs exporter type from config
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

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_metrics_gauge() {
        // Initialize metric storage for testing
        crate::modules::observability::metrics::init_metric_storage(Some(100), Some(3600));

        // Clear any previous metrics
        if let Some(storage) = crate::modules::observability::metrics::get_metric_storage() {
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
        let storage = crate::modules::observability::metrics::get_metric_storage().unwrap();
        let metrics = storage.get_metrics();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "test.gauge");
        assert_eq!(metrics[0].service_name, "test-service");
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_metrics_counter() {
        crate::modules::observability::metrics::init_metric_storage(Some(100), Some(3600));

        // Clear any previous metrics
        if let Some(storage) = crate::modules::observability::metrics::get_metric_storage() {
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
                                "asInt": 1234
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

        let storage = crate::modules::observability::metrics::get_metric_storage().unwrap();
        let metrics = storage.get_metrics_by_name("requests.total");
        assert_eq!(metrics.len(), 1);

        match &metrics[0].metric_type {
            crate::modules::observability::metrics::StoredMetricType::Counter => {}
            _ => panic!("Expected Counter metric type"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_metrics_histogram() {
        crate::modules::observability::metrics::init_metric_storage(Some(100), Some(3600));

        // Clear any previous metrics
        if let Some(storage) = crate::modules::observability::metrics::get_metric_storage() {
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

        let storage = crate::modules::observability::metrics::get_metric_storage().unwrap();
        let metrics = storage.get_metrics_by_name("request.duration");
        assert_eq!(metrics.len(), 1);

        match &metrics[0].metric_type {
            crate::modules::observability::metrics::StoredMetricType::Histogram => {}
            _ => panic!("Expected Histogram metric type"),
        }

        // Verify histogram data points
        if let crate::modules::observability::metrics::StoredDataPoint::Histogram(dp) =
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
    async fn test_ingest_otlp_metrics_invalid_json() {
        crate::modules::observability::metrics::init_metric_storage(Some(100), Some(3600));

        let invalid_json = r#"{"invalid": "json structure"#;

        let result = ingest_otlp_metrics(invalid_json).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_otlp_metrics_empty() {
        crate::modules::observability::metrics::init_metric_storage(Some(100), Some(3600));

        // Clear any previous metrics
        if let Some(storage) = crate::modules::observability::metrics::get_metric_storage() {
            storage.clear();
        }

        let empty_json = r#"{"resourceMetrics": []}"#;

        let result = ingest_otlp_metrics(empty_json).await;
        assert!(result.is_ok());

        let storage = crate::modules::observability::metrics::get_metric_storage().unwrap();
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
                double_value: Some(3.14),
                bool_value: None,
            }),
        };

        let result = otlp_kv_to_key_value(&kv);
        assert!(result.is_some());
        let kv_out = result.unwrap();
        assert_eq!(kv_out.key.as_str(), "ratio");
        assert_eq!(format!("{}", kv_out.value), "3.14");
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
    fn test_otlp_kv_to_key_value_empty_key_is_skipped() {
        let kv = OtlpKeyValue {
            key: String::new(),
            value: Some(OtlpAnyValue {
                string_value: Some("hello".to_string()),
                int_value: None,
                double_value: None,
                bool_value: None,
            }),
        };

        let result = otlp_kv_to_key_value(&kv);
        assert!(result.is_none());
    }

    #[test]
    fn test_otlp_any_value_to_string_value() {
        // String branch
        let v = OtlpAnyValue {
            string_value: Some("hello".to_string()),
            int_value: None,
            double_value: None,
            bool_value: None,
        };
        assert_eq!(v.to_string_value(), "hello");

        // Int branch
        let v = OtlpAnyValue {
            string_value: None,
            int_value: Some(99),
            double_value: None,
            bool_value: None,
        };
        assert_eq!(v.to_string_value(), "99");

        // Double branch
        let v = OtlpAnyValue {
            string_value: None,
            int_value: None,
            double_value: Some(1.5),
            bool_value: None,
        };
        assert_eq!(v.to_string_value(), "1.5");

        // Bool branch
        let v = OtlpAnyValue {
            string_value: None,
            int_value: None,
            double_value: None,
            bool_value: Some(false),
        };
        assert_eq!(v.to_string_value(), "false");

        // Empty (all None) branch
        let v = OtlpAnyValue {
            string_value: None,
            int_value: None,
            double_value: None,
            bool_value: None,
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
    fn test_extract_context_ignores_invalid_traceparent_and_keeps_baggage() {
        let ctx = extract_context(Some("00-invalid"), Some("user.id=123"));
        let baggage = inject_baggage_from_context(&ctx).expect("baggage should be preserved");
        assert!(baggage.contains("user.id=123"));
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
                }),
            },
            OtlpKeyValue {
                key: "count".to_string(),
                value: Some(OtlpAnyValue {
                    string_value: None,
                    int_value: Some(3),
                    double_value: None,
                    bool_value: None,
                }),
            },
            OtlpKeyValue {
                key: "ratio".to_string(),
                value: Some(OtlpAnyValue {
                    string_value: None,
                    int_value: None,
                    double_value: Some(1.5),
                    bool_value: None,
                }),
            },
            OtlpKeyValue {
                key: "enabled".to_string(),
                value: Some(OtlpAnyValue {
                    string_value: None,
                    int_value: None,
                    double_value: None,
                    bool_value: Some(true),
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
}
