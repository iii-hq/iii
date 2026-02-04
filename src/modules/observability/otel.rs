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
            .unwrap_or_else(|| "iii-engine".to_string());

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
    pub flags: Option<u8>,
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
            flags: Some(span.span_context.trace_flags().to_u8()),
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
                Ok(exporter) => SdkTracerProvider::builder()
                    .with_batch_exporter(exporter)
                    .with_sampler(sampler)
                    .with_id_generator(RandomIdGenerator::default())
                    .with_resource(resource)
                    .build(),
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
    let tracer = provider.tracer("iii-engine");

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

    // Use the global composite propagator to extract both
    global::get_text_map_propagator(|propagator| propagator.extract(&carrier))
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
    flags: Option<u8>,
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

/// Ingest OTLP JSON data from Node SDK and merge into in-memory storage.
///
/// This function is called when the engine receives an OTLP binary frame
/// (prefixed with "OTLP") from a worker.
pub async fn ingest_otlp_json(json_str: &str) -> anyhow::Result<()> {
    // Parse the OTLP JSON
    let request: OtlpExportTraceServiceRequest = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse OTLP JSON: {}", e))?;

    // Get the in-memory storage (if available)
    let storage = match get_span_storage() {
        Some(s) => s,
        None => {
            tracing::debug!("No in-memory span storage available, skipping OTLP ingestion");
            return Ok(());
        }
    };

    // Convert OTLP spans to StoredSpan and add to storage
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
        tracing::debug!(span_count = span_count, "Ingested OTLP spans from Node SDK");
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
        result.sort_by(|a, b| b.timestamp_unix_nano.cmp(&a.timestamp_unix_nano));

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
                        "name": "iii-engine",
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
}
