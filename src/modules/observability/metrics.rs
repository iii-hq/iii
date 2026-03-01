// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! OpenTelemetry metrics infrastructure for the III Engine.
//!
//! This module provides metrics collection via OpenTelemetry with support for
//! Memory (in-memory storage) and OTLP (push) exporters.

use super::config::MetricsExporterType;
use opentelemetry::{
    global,
    metrics::{Counter, Gauge, Histogram, Meter, MeterProvider as _},
};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider},
};
use serde::Serialize;
use std::env;
use std::sync::{Arc, OnceLock};

/// Global OTLP meter provider reference
static OTLP_METER_PROVIDER: OnceLock<SdkMeterProvider> = OnceLock::new();

/// Global meter instance for the engine
static GLOBAL_METER: OnceLock<Meter> = OnceLock::new();

/// Configuration for OpenTelemetry metrics export.
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    /// Whether OpenTelemetry metrics export is enabled.
    pub enabled: bool,
    /// The service name to report.
    pub service_name: String,
    /// Exporter type: Memory or Otlp
    pub exporter: MetricsExporterType,
    /// OTLP endpoint (e.g., "http://localhost:4317"). Only used for OTLP exporter.
    pub endpoint: String,
    /// Metrics retention period in seconds
    pub retention_seconds: u64,
    /// Maximum number of metrics to keep in memory
    pub max_count: usize,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        // Check global config from YAML first, then fall back to environment variables
        let global_cfg = super::otel::get_otel_config();

        let enabled = global_cfg
            .and_then(|c| c.metrics_enabled)
            .or_else(|| {
                env::var("OTEL_METRICS_ENABLED")
                    .ok()
                    .map(|v| v == "true" || v == "1")
            })
            .unwrap_or(false);

        let service_name = global_cfg
            .and_then(|c| c.service_name.clone())
            .or_else(|| env::var("OTEL_SERVICE_NAME").ok())
            .unwrap_or_else(|| "iii".to_string());

        let exporter = global_cfg
            .and_then(|c| c.metrics_exporter.clone())
            .or_else(|| {
                env::var("OTEL_METRICS_EXPORTER")
                    .ok()
                    .and_then(|v| match v.to_lowercase().as_str() {
                        "otlp" => Some(MetricsExporterType::Otlp),
                        "memory" => Some(MetricsExporterType::Memory),
                        other => {
                            tracing::error!(
                                "Invalid metrics exporter type '{}'. Valid values are: 'memory', 'otlp'. \
                                Falling back to 'memory'. Note: 'prometheus' and 'both' have been removed.",
                                other
                            );
                            None // Fall through to default
                        }
                    })
            })
            .unwrap_or(MetricsExporterType::Memory);

        let endpoint = global_cfg
            .and_then(|c| c.endpoint.clone())
            .or_else(|| env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok())
            .unwrap_or_else(|| "http://localhost:4317".to_string());

        let retention_seconds = global_cfg
            .and_then(|c| c.metrics_retention_seconds)
            .or_else(|| {
                env::var("OTEL_METRICS_RETENTION_SECONDS")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(3600); // 1 hour

        let max_count = global_cfg
            .and_then(|c| c.metrics_max_count)
            .or_else(|| {
                env::var("OTEL_METRICS_MAX_COUNT")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(10000);

        Self {
            enabled,
            service_name,
            exporter,
            endpoint,
            retention_seconds,
            max_count,
        }
    }
}

/// Ensure GLOBAL_METER is initialized with at least a noop meter.
/// This is safe to call multiple times — subsequent calls are no-ops.
/// If OtelModule later calls `init_metrics()`, it will find the meter already set
/// and log "Global meter already initialized".
pub fn ensure_default_meter() {
    if GLOBAL_METER.get().is_some() {
        return;
    }

    // Initialize metric storage with defaults (needed for SDK metrics ingestion)
    init_metric_storage(None, None);

    let provider = SdkMeterProvider::builder().build();
    let meter = provider.meter("iii");
    global::set_meter_provider(provider);
    if GLOBAL_METER.set(meter).is_err() {
        tracing::debug!("Global meter already initialized by another thread");
    }
}

/// Initialize OpenTelemetry metrics with the given configuration.
///
/// Returns true if metrics were successfully initialized, false otherwise.
pub fn init_metrics(config: &MetricsConfig) -> bool {
    // Always initialize metric storage for SDK metrics ingestion, even if OTEL metrics are disabled
    init_metric_storage(Some(config.max_count), Some(config.retention_seconds));

    if !config.enabled {
        println!("[Metrics] OpenTelemetry metrics are disabled");
        return false;
    }

    let resource = Resource::builder()
        .with_service_name(config.service_name.clone())
        .build();

    match config.exporter {
        MetricsExporterType::Otlp => {
            // Initialize OTLP exporter
            let exporter = match opentelemetry_otlp::MetricExporter::builder()
                .with_tonic()
                .with_endpoint(&config.endpoint)
                .build()
            {
                Ok(exporter) => exporter,
                Err(e) => {
                    tracing::error!(
                        error = ?e,
                        endpoint = %config.endpoint,
                        service_name = %config.service_name,
                        "Failed to create OTLP metrics exporter"
                    );
                    println!(
                        "[Metrics] Failed to initialize OTLP metrics exporter (endpoint={}): {}",
                        config.endpoint, e
                    );
                    return false;
                }
            };

            let reader = PeriodicReader::builder(exporter)
                .with_interval(std::time::Duration::from_secs(60))
                .build();

            let provider = SdkMeterProvider::builder()
                .with_reader(reader)
                .with_resource(resource)
                .build();

            let meter = provider.meter("iii");
            global::set_meter_provider(provider.clone());
            if OTLP_METER_PROVIDER.set(provider).is_err() {
                tracing::debug!("OTLP meter provider already initialized");
            }
            if GLOBAL_METER.set(meter).is_err() {
                tracing::debug!("Global meter already initialized");
            }

            println!(
                "[Metrics] OpenTelemetry metrics initialized: exporter=otlp (endpoint={}), service_name={}",
                config.endpoint, config.service_name
            );
            true
        }
        MetricsExporterType::Memory => {
            // Memory-only mode: no external exporters, just use in-memory storage
            // The metric storage is already initialized above (init_metric_storage)
            // SDK metrics from Node will be stored and queryable via metrics.list

            // Still need to initialize a meter for EngineMetrics to use
            let provider = SdkMeterProvider::builder().with_resource(resource).build();
            let meter = provider.meter("iii");
            global::set_meter_provider(provider);
            if GLOBAL_METER.set(meter).is_err() {
                tracing::debug!("Global meter already initialized");
            }

            println!(
                "[Metrics] OpenTelemetry metrics initialized: exporter=memory (in-memory only), service_name={}",
                config.service_name
            );
            true
        }
    }
}

/// Get the global meter instance for creating metrics.
pub fn get_meter() -> Option<&'static Meter> {
    GLOBAL_METER.get()
}

/// Shutdown OpenTelemetry metrics, flushing any pending data.
pub fn shutdown_metrics() {
    if let Some(provider) = OTLP_METER_PROVIDER.get()
        && let Err(e) = provider.shutdown()
    {
        tracing::warn!(error = ?e, "Error shutting down OpenTelemetry metrics");
    }
}

/// Helper struct for engine-wide metrics.
pub struct EngineMetrics {
    pub invocations_total: Counter<u64>,
    pub invocation_duration: Histogram<f64>,
    pub invocation_errors_total: Counter<u64>,
    pub workers_active: Gauge<i64>,
    pub workers_spawns_total: Counter<u64>,
    pub workers_deaths_total: Counter<u64>,
    pub workers_by_status: Gauge<i64>,
    // Worker resource metrics
    pub worker_memory_heap_bytes: Gauge<i64>,
    pub worker_memory_rss_bytes: Gauge<i64>,
    pub worker_cpu_percent: Gauge<f64>,
    pub worker_event_loop_lag_ms: Gauge<f64>,
    pub worker_uptime_seconds: Gauge<i64>,
}

impl EngineMetrics {
    /// Create a new EngineMetrics instance with all metrics initialized.
    ///
    /// # Panics
    ///
    /// Panics if `GLOBAL_METER` has not been initialized via `init_metrics()`.
    pub fn new() -> Self {
        let meter = get_meter().expect("GLOBAL_METER not initialized - call init_metrics() first");

        Self {
            invocations_total: meter
                .u64_counter("iii.invocations.total")
                .with_description("Total number of function invocations")
                .with_unit("invocations")
                .build(),
            invocation_duration: meter
                .f64_histogram("iii.invocation.duration")
                .with_description("Duration of function invocations")
                .with_unit("s")
                .build(),
            invocation_errors_total: meter
                .u64_counter("iii.invocation.errors.total")
                .with_description("Total number of invocation errors")
                .with_unit("errors")
                .build(),
            workers_active: meter
                .i64_gauge("iii.workers.active")
                .with_description("Current number of active workers")
                .with_unit("workers")
                .build(),
            workers_spawns_total: meter
                .u64_counter("iii.workers.spawns.total")
                .with_description("Total number of worker connections")
                .with_unit("workers")
                .build(),
            workers_deaths_total: meter
                .u64_counter("iii.workers.deaths.total")
                .with_description("Total number of worker disconnections")
                .with_unit("workers")
                .build(),
            workers_by_status: meter
                .i64_gauge("iii.workers.by_status")
                .with_description("Number of workers by status")
                .with_unit("workers")
                .build(),
            worker_memory_heap_bytes: meter
                .i64_gauge("iii.worker.memory.heap.bytes")
                .with_description("Worker heap memory usage in bytes")
                .with_unit("bytes")
                .build(),
            worker_memory_rss_bytes: meter
                .i64_gauge("iii.worker.memory.rss.bytes")
                .with_description("Worker RSS memory usage in bytes")
                .with_unit("bytes")
                .build(),
            worker_cpu_percent: meter
                .f64_gauge("iii.worker.cpu.percent")
                .with_description("Worker CPU usage percentage")
                .with_unit("%")
                .build(),
            worker_event_loop_lag_ms: meter
                .f64_gauge("iii.worker.event_loop.lag.ms")
                .with_description("Worker event loop lag in milliseconds")
                .with_unit("ms")
                .build(),
            worker_uptime_seconds: meter
                .i64_gauge("iii.worker.uptime.seconds")
                .with_description("Worker uptime in seconds")
                .with_unit("s")
                .build(),
        }
    }
}

impl Default for EngineMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Global engine metrics instance
static ENGINE_METRICS: OnceLock<EngineMetrics> = OnceLock::new();

/// Initialize and get the global engine metrics instance.
pub fn get_engine_metrics() -> &'static EngineMetrics {
    ENGINE_METRICS.get_or_init(EngineMetrics::default)
}

/// Metrics accumulator for real-time readable metrics.
/// This complements OTEL metrics which are write-only.
pub struct MetricsAccumulator {
    pub invocations_total: std::sync::atomic::AtomicU64,
    pub invocations_success: std::sync::atomic::AtomicU64,
    pub invocations_error: std::sync::atomic::AtomicU64,
    pub invocations_deferred: std::sync::atomic::AtomicU64,
    pub invocations_by_function: dashmap::DashMap<String, u64>,
    pub workers_spawns: std::sync::atomic::AtomicU64,
    pub workers_deaths: std::sync::atomic::AtomicU64,
}

impl Default for MetricsAccumulator {
    fn default() -> Self {
        Self {
            invocations_total: std::sync::atomic::AtomicU64::new(0),
            invocations_success: std::sync::atomic::AtomicU64::new(0),
            invocations_error: std::sync::atomic::AtomicU64::new(0),
            invocations_deferred: std::sync::atomic::AtomicU64::new(0),
            invocations_by_function: dashmap::DashMap::new(),
            workers_spawns: std::sync::atomic::AtomicU64::new(0),
            workers_deaths: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl MetricsAccumulator {
    /// Get invocations for a specific function.
    /// More efficient than get_by_function() when only one function's count is needed.
    pub fn get_function_count(&self, function_id: &str) -> Option<u64> {
        self.invocations_by_function.get(function_id).map(|v| *v)
    }

    /// Get invocations grouped by function as a HashMap.
    /// Note: This clones all entries. For single-function lookups, use get_function_count() instead.
    pub fn get_by_function(&self) -> std::collections::HashMap<String, u64> {
        self.invocations_by_function
            .iter()
            .map(|entry| (entry.key().clone(), *entry.value()))
            .collect()
    }

    /// Get an iterator over function invocation counts.
    /// More efficient than get_by_function() when you don't need to own the data.
    pub fn iter_function_counts(&self) -> impl Iterator<Item = (String, u64)> + '_ {
        self.invocations_by_function
            .iter()
            .map(|entry| (entry.key().clone(), *entry.value()))
    }

    /// Increment invocation count for a specific function
    pub fn increment_function(&self, function_id: &str) {
        self.invocations_by_function
            .entry(function_id.to_string())
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }
}

/// Global metrics accumulator instance
static METRICS_ACCUMULATOR: OnceLock<MetricsAccumulator> = OnceLock::new();

/// Get the global metrics accumulator instance.
pub fn get_metrics_accumulator() -> &'static MetricsAccumulator {
    METRICS_ACCUMULATOR.get_or_init(MetricsAccumulator::default)
}

// =============================================================================
// In-Memory Metric Storage for SDK Metrics
// =============================================================================

/// Type of stored metric
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StoredMetricType {
    Counter,
    Gauge,
    Histogram,
    UpDownCounter,
}

/// Number data point for counter and gauge metrics
#[derive(Debug, Clone, Serialize)]
pub struct StoredNumberDataPoint {
    pub value: f64,
    pub attributes: Vec<(String, String)>,
    pub timestamp_unix_nano: u64,
}

/// Histogram data point
#[derive(Debug, Clone, Serialize)]
pub struct StoredHistogramDataPoint {
    pub count: u64,
    pub sum: f64,
    pub bucket_counts: Vec<u64>,
    pub explicit_bounds: Vec<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub attributes: Vec<(String, String)>,
    pub timestamp_unix_nano: u64,
}

/// Data point that can be either number or histogram
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum StoredDataPoint {
    Number(StoredNumberDataPoint),
    Histogram(StoredHistogramDataPoint),
}

/// Stored metric from SDK ingestion
#[derive(Debug, Clone, Serialize)]
pub struct StoredMetric {
    pub name: String,
    pub description: String,
    pub unit: String,
    pub metric_type: StoredMetricType,
    pub data_points: Vec<StoredDataPoint>,
    pub service_name: String,
    pub timestamp_unix_nano: u64,
    /// Instrumentation scope name (e.g., "@opentelemetry/instrumentation-http")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrumentation_scope_name: Option<String>,
    /// Instrumentation scope version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrumentation_scope_version: Option<String>,
}

/// Aggregated metric for rollup queries
#[derive(Debug, Clone, Serialize)]
pub struct AggregatedMetric {
    pub name: String,
    pub bucket_start_ns: u64,
    pub bucket_end_ns: u64,
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    /// Percentile values (p50, p75, p90, p95, p99)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentiles: Option<PercentileValues>,
}

/// Percentile values for metrics
#[derive(Debug, Clone, Serialize)]
pub struct PercentileValues {
    pub p50: f64,
    pub p75: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
}

impl PercentileValues {
    /// Calculate percentiles from a sorted slice of values
    pub fn from_sorted_values(values: &[f64]) -> Self {
        let len = values.len();
        if len == 0 {
            return Self {
                p50: 0.0,
                p75: 0.0,
                p90: 0.0,
                p95: 0.0,
                p99: 0.0,
            };
        }

        let p50_idx = (len as f64 * 0.50) as usize;
        let p75_idx = (len as f64 * 0.75) as usize;
        let p90_idx = (len as f64 * 0.90) as usize;
        let p95_idx = (len as f64 * 0.95) as usize;
        let p99_idx = (len as f64 * 0.99) as usize;

        Self {
            p50: values[p50_idx.min(len - 1)],
            p75: values[p75_idx.min(len - 1)],
            p90: values[p90_idx.min(len - 1)],
            p95: values[p95_idx.min(len - 1)],
            p99: values[p99_idx.min(len - 1)],
        }
    }
}

/// Aggregated histogram for histogram metrics
#[derive(Debug, Clone, Serialize)]
pub struct AggregatedHistogram {
    pub name: String,
    pub bucket_start_ns: u64,
    pub bucket_end_ns: u64,
    pub total_count: u64,
    pub total_sum: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Merged bucket counts
    pub bucket_counts: Vec<u64>,
    /// Explicit bounds for buckets
    pub explicit_bounds: Vec<f64>,
}

/// Rollup level configuration
#[derive(Debug, Clone)]
pub struct RollupLevel {
    /// Interval in nanoseconds
    pub interval_ns: u64,
    /// Retention in nanoseconds
    pub retention_ns: u64,
}

/// Multi-level rollup storage
pub struct RollupStorage {
    /// Rollup levels configuration
    levels: Vec<RollupLevel>,
    /// Storage for each rollup level: level_index -> (name, bucket_start) -> AggregatedMetric
    rollups: std::sync::RwLock<Vec<std::collections::HashMap<(String, u64), AggregatedMetric>>>,
    /// Histogram rollups for each level
    histogram_rollups:
        std::sync::RwLock<Vec<std::collections::HashMap<(String, u64), AggregatedHistogram>>>,
}

impl std::fmt::Debug for RollupStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RollupStorage")
            .field("levels", &self.levels)
            .finish()
    }
}

impl RollupStorage {
    pub fn new(levels: Vec<RollupLevel>) -> Self {
        let rollups = vec![std::collections::HashMap::new(); levels.len()];
        let histogram_rollups = vec![std::collections::HashMap::new(); levels.len()];
        Self {
            levels,
            rollups: std::sync::RwLock::new(rollups),
            histogram_rollups: std::sync::RwLock::new(histogram_rollups),
        }
    }

    /// Process new metrics and update rollups
    pub fn process_metrics(&self, metrics: &[StoredMetric]) {
        let mut rollups = self.rollups.write().unwrap();
        let mut histogram_rollups = self.histogram_rollups.write().unwrap();

        for (level_idx, level) in self.levels.iter().enumerate() {
            for metric in metrics {
                let bucket_start =
                    (metric.timestamp_unix_nano / level.interval_ns) * level.interval_ns;
                let key = (metric.name.clone(), bucket_start);

                // Check if this is a histogram metric
                let is_histogram = matches!(metric.metric_type, StoredMetricType::Histogram);

                if is_histogram {
                    // Aggregate histogram data points
                    for dp in &metric.data_points {
                        if let StoredDataPoint::Histogram(hist_dp) = dp {
                            let entry = histogram_rollups[level_idx]
                                .entry(key.clone())
                                .or_insert_with(|| AggregatedHistogram {
                                    name: metric.name.clone(),
                                    bucket_start_ns: bucket_start,
                                    bucket_end_ns: bucket_start + level.interval_ns,
                                    total_count: 0,
                                    total_sum: 0.0,
                                    min: None,
                                    max: None,
                                    bucket_counts: vec![0; hist_dp.bucket_counts.len()],
                                    explicit_bounds: hist_dp.explicit_bounds.clone(),
                                });

                            entry.total_count += hist_dp.count;
                            entry.total_sum += hist_dp.sum;

                            if let Some(min) = hist_dp.min {
                                entry.min = Some(entry.min.map(|m| m.min(min)).unwrap_or(min));
                            }
                            if let Some(max) = hist_dp.max {
                                entry.max = Some(entry.max.map(|m| m.max(max)).unwrap_or(max));
                            }

                            // Merge bucket counts
                            if entry.bucket_counts.len() == hist_dp.bucket_counts.len() {
                                for (i, count) in hist_dp.bucket_counts.iter().enumerate() {
                                    entry.bucket_counts[i] += count;
                                }
                            }
                        }
                    }
                } else {
                    // Aggregate number data points
                    for dp in &metric.data_points {
                        if let StoredDataPoint::Number(num_dp) = dp {
                            let entry =
                                rollups[level_idx].entry(key.clone()).or_insert_with(|| {
                                    AggregatedMetric {
                                        name: metric.name.clone(),
                                        bucket_start_ns: bucket_start,
                                        bucket_end_ns: bucket_start + level.interval_ns,
                                        count: 0,
                                        sum: 0.0,
                                        min: f64::INFINITY,
                                        max: f64::NEG_INFINITY,
                                        avg: 0.0,
                                        percentiles: None,
                                    }
                                });

                            entry.count += 1;
                            entry.sum += num_dp.value;
                            entry.min = entry.min.min(num_dp.value);
                            entry.max = entry.max.max(num_dp.value);
                            entry.avg = entry.sum / entry.count as f64;
                        }
                    }
                }
            }
        }
    }

    /// Get rollups for a specific level and time range
    pub fn get_rollups(
        &self,
        level_idx: usize,
        start_ns: u64,
        end_ns: u64,
        metric_name: Option<&str>,
    ) -> Vec<AggregatedMetric> {
        let rollups = self.rollups.read().unwrap();

        if level_idx >= rollups.len() {
            return Vec::new();
        }

        rollups[level_idx]
            .values()
            .filter(|m| {
                m.bucket_start_ns >= start_ns
                    && m.bucket_end_ns <= end_ns
                    && metric_name.map(|n| m.name == n).unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    /// Get histogram rollups for a specific level and time range
    pub fn get_histogram_rollups(
        &self,
        level_idx: usize,
        start_ns: u64,
        end_ns: u64,
        metric_name: Option<&str>,
    ) -> Vec<AggregatedHistogram> {
        let rollups = self.histogram_rollups.read().unwrap();

        if level_idx >= rollups.len() {
            return Vec::new();
        }

        rollups[level_idx]
            .values()
            .filter(|m| {
                m.bucket_start_ns >= start_ns
                    && m.bucket_end_ns <= end_ns
                    && metric_name.map(|n| m.name == n).unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    /// Apply retention to rollup storage
    pub fn apply_retention(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let mut rollups = self.rollups.write().unwrap();
        let mut histogram_rollups = self.histogram_rollups.write().unwrap();

        for (level_idx, level) in self.levels.iter().enumerate() {
            let cutoff = now.saturating_sub(level.retention_ns);

            rollups[level_idx].retain(|(_, bucket_start), _| *bucket_start >= cutoff);
            histogram_rollups[level_idx].retain(|(_, bucket_start), _| *bucket_start >= cutoff);
        }
    }
}

/// Time-indexed metric storage with efficient range queries
pub struct TimeIndexedMetricStorage {
    /// Primary storage: timestamp -> metrics
    metrics_by_time: std::sync::RwLock<std::collections::BTreeMap<u64, Vec<StoredMetric>>>,
    /// Secondary index: name -> timestamps (for name+time queries)
    metrics_by_name:
        std::sync::RwLock<std::collections::HashMap<String, std::collections::BTreeSet<u64>>>,
    /// Configuration
    max_age_ns: u64,
    max_metrics: usize,
    /// Maximum unique metric names (cardinality limit)
    max_unique_names: usize,
    /// Metric counter for eviction
    total_metrics: std::sync::atomic::AtomicUsize,
    /// Flag to warn once about cardinality limit
    cardinality_warned: std::sync::atomic::AtomicBool,
}

impl std::fmt::Debug for TimeIndexedMetricStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimeIndexedMetricStorage")
            .field("metrics_by_time", &self.metrics_by_time)
            .field("metrics_by_name", &self.metrics_by_name)
            .field("max_age_ns", &self.max_age_ns)
            .field("max_metrics", &self.max_metrics)
            .field("max_unique_names", &self.max_unique_names)
            .finish()
    }
}

impl TimeIndexedMetricStorage {
    pub fn new(max_metrics: usize, max_age_ns: u64) -> Self {
        Self {
            metrics_by_time: std::sync::RwLock::new(std::collections::BTreeMap::new()),
            metrics_by_name: std::sync::RwLock::new(std::collections::HashMap::new()),
            max_age_ns,
            max_metrics,
            max_unique_names: 10000, // Default cardinality limit
            total_metrics: std::sync::atomic::AtomicUsize::new(0),
            cardinality_warned: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn add_metrics(&self, new_metrics: Vec<StoredMetric>) {
        let mut by_time = self.metrics_by_time.write().unwrap();
        let mut by_name = self.metrics_by_name.write().unwrap();

        for metric in new_metrics {
            let timestamp = metric.timestamp_unix_nano;

            // Check cardinality limit before adding new metric names
            if !by_name.contains_key(&metric.name) && by_name.len() >= self.max_unique_names {
                // Only warn once
                if !self
                    .cardinality_warned
                    .swap(true, std::sync::atomic::Ordering::Relaxed)
                {
                    tracing::warn!(
                        current_names = by_name.len(),
                        max_names = self.max_unique_names,
                        rejected_name = %metric.name,
                        "Metric cardinality limit reached, new metric names will be dropped. \
                        Consider reducing metric label cardinality or increasing max_unique_names."
                    );
                }
                continue; // Skip this metric
            }

            // Add to time index
            by_time.entry(timestamp).or_default().push(metric.clone());

            // Add to name index
            by_name
                .entry(metric.name.clone())
                .or_default()
                .insert(timestamp);

            self.total_metrics
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        // Evict old metrics if we exceed max_metrics
        self.evict_if_needed(&mut by_time, &mut by_name);
    }

    fn evict_if_needed(
        &self,
        by_time: &mut std::collections::BTreeMap<u64, Vec<StoredMetric>>,
        by_name: &mut std::collections::HashMap<String, std::collections::BTreeSet<u64>>,
    ) {
        while self
            .total_metrics
            .load(std::sync::atomic::Ordering::Relaxed)
            > self.max_metrics
        {
            // Remove oldest timestamp bucket
            if let Some((oldest_ts, metrics)) = by_time.iter().next() {
                let oldest_ts = *oldest_ts;
                let count = metrics.len();

                // Remove from name index
                for metric in metrics {
                    if let Some(timestamps) = by_name.get_mut(&metric.name) {
                        timestamps.remove(&oldest_ts);
                        if timestamps.is_empty() {
                            by_name.remove(&metric.name);
                        }
                    }
                }

                // Remove from time index
                by_time.remove(&oldest_ts);
                self.total_metrics
                    .fetch_sub(count, std::sync::atomic::Ordering::Relaxed);
            } else {
                break;
            }
        }
    }

    pub fn get_metrics(&self) -> Vec<StoredMetric> {
        let by_time = self.metrics_by_time.read().unwrap();
        by_time
            .values()
            .flat_map(|metrics| metrics.iter().cloned())
            .collect()
    }

    pub fn get_metrics_in_range(&self, start_ns: u64, end_ns: u64) -> Vec<StoredMetric> {
        let by_time = self.metrics_by_time.read().unwrap();
        by_time
            .range(start_ns..=end_ns)
            .flat_map(|(_, metrics)| metrics.iter().cloned())
            .collect()
    }

    pub fn get_metrics_by_name(&self, name: &str) -> Vec<StoredMetric> {
        let by_name = self.metrics_by_name.read().unwrap();
        let by_time = self.metrics_by_time.read().unwrap();

        if let Some(timestamps) = by_name.get(name) {
            timestamps
                .iter()
                .filter_map(|ts| by_time.get(ts))
                .flat_map(|metrics| metrics.iter().filter(|m| m.name == name).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_metrics_by_name_in_range(
        &self,
        name: &str,
        start_ns: u64,
        end_ns: u64,
    ) -> Vec<StoredMetric> {
        let by_name = self.metrics_by_name.read().unwrap();
        let by_time = self.metrics_by_time.read().unwrap();

        if let Some(timestamps) = by_name.get(name) {
            timestamps
                .range(start_ns..=end_ns)
                .filter_map(|ts| by_time.get(ts))
                .flat_map(|metrics| metrics.iter().filter(|m| m.name == name).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_aggregated_metrics(
        &self,
        start_ns: u64,
        end_ns: u64,
        interval_ns: u64,
    ) -> Vec<AggregatedMetric> {
        let metrics = self.get_metrics_in_range(start_ns, end_ns);
        let mut buckets: std::collections::HashMap<(String, u64), Vec<f64>> =
            std::collections::HashMap::new();

        // Group metrics by name and bucket
        for metric in metrics {
            let bucket_start = (metric.timestamp_unix_nano / interval_ns) * interval_ns;

            // Extract numeric values from data points
            for dp in &metric.data_points {
                if let StoredDataPoint::Number(num_dp) = dp {
                    buckets
                        .entry((metric.name.clone(), bucket_start))
                        .or_default()
                        .push(num_dp.value);
                }
            }
        }

        // Aggregate each bucket
        let mut aggregated = Vec::new();
        for ((name, bucket_start), mut values) in buckets {
            if values.is_empty() {
                continue;
            }

            let count = values.len() as u64;
            let sum: f64 = values.iter().sum();
            let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let avg = sum / count as f64;

            // Calculate percentiles
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let percentiles = if values.len() >= 5 {
                Some(PercentileValues::from_sorted_values(&values))
            } else {
                None
            };

            aggregated.push(AggregatedMetric {
                name,
                bucket_start_ns: bucket_start,
                bucket_end_ns: bucket_start + interval_ns,
                count,
                sum,
                min,
                max,
                avg,
                percentiles,
            });
        }

        aggregated.sort_by_key(|a| (a.name.clone(), a.bucket_start_ns));
        aggregated
    }

    /// Get aggregated histogram metrics for a time range
    pub fn get_aggregated_histograms(
        &self,
        start_ns: u64,
        end_ns: u64,
        interval_ns: u64,
    ) -> Vec<AggregatedHistogram> {
        let metrics = self.get_metrics_in_range(start_ns, end_ns);
        let mut buckets: std::collections::HashMap<(String, u64), AggregatedHistogram> =
            std::collections::HashMap::new();

        // Group histogram metrics by name and bucket
        for metric in metrics {
            if !matches!(metric.metric_type, StoredMetricType::Histogram) {
                continue;
            }

            let bucket_start = (metric.timestamp_unix_nano / interval_ns) * interval_ns;

            for dp in &metric.data_points {
                if let StoredDataPoint::Histogram(hist_dp) = dp {
                    let key = (metric.name.clone(), bucket_start);
                    let entry = buckets.entry(key).or_insert_with(|| AggregatedHistogram {
                        name: metric.name.clone(),
                        bucket_start_ns: bucket_start,
                        bucket_end_ns: bucket_start + interval_ns,
                        total_count: 0,
                        total_sum: 0.0,
                        min: None,
                        max: None,
                        bucket_counts: vec![0; hist_dp.bucket_counts.len()],
                        explicit_bounds: hist_dp.explicit_bounds.clone(),
                    });

                    entry.total_count += hist_dp.count;
                    entry.total_sum += hist_dp.sum;

                    if let Some(min) = hist_dp.min {
                        entry.min = Some(entry.min.map(|m| m.min(min)).unwrap_or(min));
                    }
                    if let Some(max) = hist_dp.max {
                        entry.max = Some(entry.max.map(|m| m.max(max)).unwrap_or(max));
                    }

                    // Merge bucket counts if they match
                    if entry.bucket_counts.len() == hist_dp.bucket_counts.len() {
                        for (i, count) in hist_dp.bucket_counts.iter().enumerate() {
                            entry.bucket_counts[i] += count;
                        }
                    }
                }
            }
        }

        let mut aggregated: Vec<_> = buckets.into_values().collect();
        aggregated.sort_by_key(|a| (a.name.clone(), a.bucket_start_ns));
        aggregated
    }

    pub fn apply_retention(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let cutoff = now.saturating_sub(self.max_age_ns);

        let mut by_time = self.metrics_by_time.write().unwrap();
        let mut by_name = self.metrics_by_name.write().unwrap();

        // Find all timestamps older than cutoff
        let old_timestamps: Vec<u64> = by_time.range(..cutoff).map(|(ts, _)| *ts).collect();

        // Remove old metrics
        for ts in old_timestamps {
            if let Some(metrics) = by_time.remove(&ts) {
                let count = metrics.len();

                // Remove from name index
                for metric in metrics {
                    if let Some(timestamps) = by_name.get_mut(&metric.name) {
                        timestamps.remove(&ts);
                        if timestamps.is_empty() {
                            by_name.remove(&metric.name);
                        }
                    }
                }

                self.total_metrics
                    .fetch_sub(count, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    pub fn clear(&self) {
        self.metrics_by_time.write().unwrap().clear();
        self.metrics_by_name.write().unwrap().clear();
        self.total_metrics
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn len(&self) -> usize {
        self.total_metrics
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.total_metrics
            .load(std::sync::atomic::Ordering::Relaxed)
            == 0
    }
}

/// Legacy alias for backward compatibility
pub type InMemoryMetricStorage = TimeIndexedMetricStorage;

/// Global in-memory metric storage
static IN_MEMORY_METRIC_STORAGE: OnceLock<Arc<TimeIndexedMetricStorage>> = OnceLock::new();

/// Default maximum number of metrics to keep in memory
const DEFAULT_MAX_METRICS: usize = 10000;

/// Default retention period (1 hour in nanoseconds)
const DEFAULT_RETENTION_NS: u64 = 3600 * 1_000_000_000;

/// Initialize metric storage with the given capacity and retention
pub fn init_metric_storage(max_metrics: Option<usize>, retention_seconds: Option<u64>) {
    let max_metrics = max_metrics.unwrap_or(DEFAULT_MAX_METRICS);
    let retention_ns = if let Some(seconds) = retention_seconds {
        match seconds.checked_mul(1_000_000_000) {
            Some(ns) => ns,
            None => {
                tracing::error!(
                    "retention_seconds overflow when converting to nanoseconds, using default retention"
                );
                DEFAULT_RETENTION_NS
            }
        }
    } else {
        DEFAULT_RETENTION_NS
    };

    let storage = Arc::new(TimeIndexedMetricStorage::new(max_metrics, retention_ns));
    if IN_MEMORY_METRIC_STORAGE.set(storage).is_err() {
        tracing::debug!("Metric storage already initialized");
    }
}

/// Get the global in-memory metric storage (if initialized)
pub fn get_metric_storage() -> Option<Arc<TimeIndexedMetricStorage>> {
    IN_MEMORY_METRIC_STORAGE.get().cloned()
}

// =============================================================================
// Worker Metrics Query
// =============================================================================

use crate::protocol::WorkerMetrics;

/// Worker metric names from the Node SDK (iii.worker.*)
const WORKER_METRIC_NAMES: &[&str] = &[
    "iii.worker.memory.heap_used",
    "iii.worker.memory.heap_total",
    "iii.worker.memory.rss",
    "iii.worker.memory.external",
    "iii.worker.cpu.percent",
    "iii.worker.cpu.user_micros",
    "iii.worker.cpu.system_micros",
    "iii.worker.event_loop.lag_ms",
    "iii.worker.uptime_seconds",
];

/// Get the latest metrics for a specific worker from OTEL storage
pub fn get_worker_metrics_from_storage(worker_id: &str) -> Option<WorkerMetrics> {
    let storage = get_metric_storage()?;
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos() as u64;
    // Look back 2 minutes for metrics
    let lookback_ns = now_ns.saturating_sub(120_000_000_000);

    let mut memory_heap_used: Option<u64> = None;
    let mut memory_heap_total: Option<u64> = None;
    let mut memory_rss: Option<u64> = None;
    let mut memory_external: Option<u64> = None;
    let mut cpu_percent: Option<f64> = None;
    let mut cpu_user_micros: Option<u64> = None;
    let mut cpu_system_micros: Option<u64> = None;
    let mut event_loop_lag_ms: Option<f64> = None;
    let mut uptime_seconds: Option<u64> = None;
    let mut latest_timestamp_ns: u64 = 0;
    let mut found_any = false;

    // Query each metric type
    for metric_name in WORKER_METRIC_NAMES {
        let metrics = storage.get_metrics_by_name_in_range(metric_name, lookback_ns, now_ns);

        // Find the latest data point with matching worker.id attribute
        for metric in metrics.iter().rev() {
            for dp in &metric.data_points {
                if let StoredDataPoint::Number(num_dp) = dp {
                    // Check if this data point belongs to our worker
                    let is_our_worker = num_dp
                        .attributes
                        .iter()
                        .any(|(k, v)| k == "worker.id" && v == worker_id);

                    if is_our_worker {
                        found_any = true;
                        let value = num_dp.value;
                        let ts = num_dp.timestamp_unix_nano;

                        // Update the latest timestamp
                        if ts > latest_timestamp_ns {
                            latest_timestamp_ns = ts;
                        }

                        // Map metric name to field
                        match *metric_name {
                            "iii.worker.memory.heap_used" => {
                                if memory_heap_used.is_none() {
                                    memory_heap_used = Some(value as u64);
                                }
                            }
                            "iii.worker.memory.heap_total" => {
                                if memory_heap_total.is_none() {
                                    memory_heap_total = Some(value as u64);
                                }
                            }
                            "iii.worker.memory.rss" => {
                                if memory_rss.is_none() {
                                    memory_rss = Some(value as u64);
                                }
                            }
                            "iii.worker.memory.external" => {
                                if memory_external.is_none() {
                                    memory_external = Some(value as u64);
                                }
                            }
                            "iii.worker.cpu.percent" => {
                                if cpu_percent.is_none() {
                                    cpu_percent = Some(value);
                                }
                            }
                            "iii.worker.cpu.user_micros" => {
                                if cpu_user_micros.is_none() {
                                    cpu_user_micros = Some(value as u64);
                                }
                            }
                            "iii.worker.cpu.system_micros" => {
                                if cpu_system_micros.is_none() {
                                    cpu_system_micros = Some(value as u64);
                                }
                            }
                            "iii.worker.event_loop.lag_ms" => {
                                if event_loop_lag_ms.is_none() {
                                    event_loop_lag_ms = Some(value);
                                }
                            }
                            "iii.worker.uptime_seconds" => {
                                if uptime_seconds.is_none() {
                                    uptime_seconds = Some(value as u64);
                                }
                            }
                            _ => {}
                        }
                        break; // Found latest for this metric, move to next
                    }
                }
            }
        }
    }

    if !found_any {
        return None;
    }

    Some(WorkerMetrics {
        memory_heap_used,
        memory_heap_total,
        memory_rss,
        memory_external,
        cpu_user_micros,
        cpu_system_micros,
        cpu_percent,
        event_loop_lag_ms,
        uptime_seconds,
        timestamp_ms: latest_timestamp_ns / 1_000_000, // Convert ns to ms
        runtime: "node".to_string(), // Default to node since these metrics come from Node SDK
    })
}

// =============================================================================
// Alerting System
// =============================================================================

use super::config::{AlertAction, AlertRule};
use crate::engine::EngineTrait;
use std::collections::HashMap;

/// State for a single alert
#[derive(Debug, Clone, Serialize)]
pub struct AlertState {
    /// Name of the alert
    pub name: String,
    /// Whether the alert is currently firing
    pub firing: bool,
    /// Last time the alert was evaluated
    pub last_evaluated: u64,
    /// Last time the alert was triggered
    pub last_triggered: Option<u64>,
    /// Current metric value
    pub current_value: Option<f64>,
    /// Threshold value
    pub threshold: f64,
    /// Operator
    pub operator: String,
}

/// Alert event for logging/webhooks
#[derive(Debug, Clone, Serialize)]
pub struct AlertEvent {
    pub name: String,
    pub metric: String,
    pub value: f64,
    pub threshold: f64,
    pub operator: String,
    pub firing: bool,
    pub timestamp: u64,
}

/// Alert manager for evaluating and triggering alerts
pub struct AlertManager {
    /// Configured alert rules
    rules: Vec<AlertRule>,
    /// Current state of each alert
    states: std::sync::RwLock<HashMap<String, AlertState>>,
    /// Engine reference for function invocation (optional for backward compatibility)
    engine: Option<Arc<crate::engine::Engine>>,
}

impl std::fmt::Debug for AlertManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlertManager")
            .field("rules_count", &self.rules.len())
            .finish()
    }
}

impl AlertManager {
    pub fn new(rules: Vec<AlertRule>) -> Self {
        let states = HashMap::new();
        Self {
            rules,
            states: std::sync::RwLock::new(states),
            engine: None,
        }
    }

    pub fn with_engine(rules: Vec<AlertRule>, engine: Arc<crate::engine::Engine>) -> Self {
        let states = HashMap::new();
        Self {
            rules,
            states: std::sync::RwLock::new(states),
            engine: Some(engine),
        }
    }

    /// Evaluate all alert rules against current metrics
    pub async fn evaluate(&self) -> Vec<AlertEvent> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let accumulator = get_metrics_accumulator();
        let mut events = Vec::new();

        for rule in &self.rules {
            if !rule.enabled {
                continue;
            }

            // Calculate time window for SDK metrics (nanoseconds for storage)
            let now_ns = match now.checked_mul(1_000_000) {
                Some(ns) => ns,
                None => {
                    tracing::warn!(
                        "Time overflow when converting to nanoseconds in alert evaluation"
                    );
                    continue; // Skip this rule evaluation
                }
            };
            let window_ns = if rule.window_seconds > 0 {
                match rule.window_seconds.checked_mul(1_000_000_000) {
                    Some(ns) => Some(ns),
                    None => {
                        tracing::warn!(
                            "window_seconds overflow for rule '{}' when converting to nanoseconds",
                            rule.name
                        );
                        continue; // Skip this rule evaluation
                    }
                }
            } else {
                None
            };

            // Get the metric value based on the metric name
            let value = self.get_metric_value(&rule.metric, accumulator, window_ns, now_ns);

            if let Some(value) = value {
                let is_firing = rule.operator.evaluate(value, rule.threshold);

                // Check cooldown
                let should_trigger = {
                    let states = self.states.read().unwrap();
                    if let Some(state) = states.get(&rule.name) {
                        // Check if we're past the cooldown period
                        if let Some(last_triggered) = state.last_triggered {
                            let cooldown_ms = rule.cooldown_seconds * 1000;
                            is_firing && (now - last_triggered) >= cooldown_ms
                        } else {
                            is_firing
                        }
                    } else {
                        is_firing
                    }
                };

                // Update state
                {
                    let mut states = self.states.write().unwrap();
                    let state = states
                        .entry(rule.name.clone())
                        .or_insert_with(|| AlertState {
                            name: rule.name.clone(),
                            firing: false,
                            last_evaluated: now,
                            last_triggered: None,
                            current_value: None,
                            threshold: rule.threshold,
                            operator: format!("{:?}", rule.operator),
                        });

                    state.last_evaluated = now;
                    state.current_value = Some(value);
                    state.firing = is_firing;

                    if should_trigger {
                        state.last_triggered = Some(now);
                    }
                }

                // Create alert event
                if should_trigger {
                    let event = AlertEvent {
                        name: rule.name.clone(),
                        metric: rule.metric.clone(),
                        value,
                        threshold: rule.threshold,
                        operator: format!("{:?}", rule.operator),
                        firing: true,
                        timestamp: now,
                    };

                    // Take action
                    self.take_action(rule, &event).await;

                    events.push(event);
                }
            }
        }

        events
    }

    /// Get the value of a metric by name
    ///
    /// For engine metrics (iii.*), returns point-in-time values as these are cumulative counters.
    /// For SDK metrics, applies time-windowed aggregation if `window_ns` is provided:
    /// - Counters/UpDownCounters: sum of values in window
    /// - Gauges: average of values in window (or last value if only one)
    fn get_metric_value(
        &self,
        metric_name: &str,
        accumulator: &MetricsAccumulator,
        window_ns: Option<u64>,
        now_ns: u64,
    ) -> Option<f64> {
        use std::sync::atomic::Ordering;

        // Check engine metrics first - these are cumulative counters, window doesn't apply
        match metric_name {
            "iii.invocations.total" => {
                Some(accumulator.invocations_total.load(Ordering::Relaxed) as f64)
            }
            "iii.invocations.success" => {
                Some(accumulator.invocations_success.load(Ordering::Relaxed) as f64)
            }
            "iii.invocations.error" => {
                Some(accumulator.invocations_error.load(Ordering::Relaxed) as f64)
            }
            "iii.invocations.deferred" => {
                Some(accumulator.invocations_deferred.load(Ordering::Relaxed) as f64)
            }
            "iii.workers.spawns" => Some(accumulator.workers_spawns.load(Ordering::Relaxed) as f64),
            "iii.workers.deaths" => Some(accumulator.workers_deaths.load(Ordering::Relaxed) as f64),
            "iii.workers.active" => {
                let spawns = accumulator.workers_spawns.load(Ordering::Relaxed);
                let deaths = accumulator.workers_deaths.load(Ordering::Relaxed);
                Some(spawns.saturating_sub(deaths) as f64)
            }
            _ => {
                // Try to get from SDK metrics storage
                if let Some(storage) = get_metric_storage() {
                    // If window is specified, get metrics in time range and aggregate
                    if let Some(window) = window_ns {
                        let start_ns = now_ns.saturating_sub(window);
                        let metrics =
                            storage.get_metrics_by_name_in_range(metric_name, start_ns, now_ns);

                        if metrics.is_empty() {
                            return None;
                        }

                        // Determine metric type from first metric
                        let metric_type = metrics.first().map(|m| m.metric_type.clone());

                        // Collect all numeric values from data points
                        let values: Vec<f64> = metrics
                            .iter()
                            .flat_map(|m| m.data_points.iter())
                            .filter_map(|dp| {
                                if let StoredDataPoint::Number(num_dp) = dp {
                                    Some(num_dp.value)
                                } else {
                                    None
                                }
                            })
                            .collect();

                        if values.is_empty() {
                            return None;
                        }

                        // Aggregate based on metric type
                        match metric_type {
                            Some(StoredMetricType::Counter)
                            | Some(StoredMetricType::UpDownCounter) => {
                                // For counters, sum the values in the window
                                Some(values.iter().sum())
                            }
                            Some(StoredMetricType::Gauge) => {
                                // For gauges, return the average value in the window
                                let sum: f64 = values.iter().sum();
                                Some(sum / values.len() as f64)
                            }
                            Some(StoredMetricType::Histogram) => {
                                // For histograms with number data points, return sum
                                Some(values.iter().sum())
                            }
                            None => {
                                // Default to last value if type unknown
                                values.last().copied()
                            }
                        }
                    } else {
                        // No window specified, return latest value (original behavior)
                        let metrics = storage.get_metrics_by_name(metric_name);
                        if let Some(metric) = metrics.last() {
                            for dp in &metric.data_points {
                                if let StoredDataPoint::Number(num_dp) = dp {
                                    return Some(num_dp.value);
                                }
                            }
                        }
                        None
                    }
                } else {
                    None
                }
            }
        }
    }

    /// Take action when an alert triggers
    async fn take_action(&self, rule: &AlertRule, event: &AlertEvent) {
        match &rule.action {
            AlertAction::Log => {
                tracing::warn!(
                    alert_name = %event.name,
                    metric = %event.metric,
                    value = %event.value,
                    threshold = %event.threshold,
                    operator = %event.operator,
                    "Alert triggered"
                );
            }
            AlertAction::Webhook { url } => {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new());
                let payload = serde_json::json!({
                    "alert": event.name,
                    "metric": event.metric,
                    "value": event.value,
                    "threshold": event.threshold,
                    "operator": event.operator,
                    "timestamp": event.timestamp,
                });

                let url = url.clone();
                let alert_name = event.name.clone();

                tokio::spawn(async move {
                    let delays = [1, 2, 4]; // Exponential backoff: 1s, 2s, 4s
                    let mut last_error = None;

                    for (attempt, delay_secs) in delays.iter().enumerate() {
                        match client.post(&url).json(&payload).send().await {
                            Ok(response) if response.status().is_success() => {
                                tracing::debug!(
                                    alert_name = %alert_name,
                                    attempt = attempt + 1,
                                    "Alert webhook sent successfully"
                                );
                                return;
                            }
                            Ok(response) => {
                                last_error = Some(format!(
                                    "HTTP {} - {}",
                                    response.status(),
                                    response.text().await.unwrap_or_default()
                                ));
                            }
                            Err(e) => {
                                last_error = Some(e.to_string());
                            }
                        }

                        tracing::warn!(
                            alert_name = %alert_name,
                            attempt = attempt + 1,
                            delay_secs = delay_secs,
                            error = ?last_error,
                            "Alert webhook failed, retrying..."
                        );

                        tokio::time::sleep(std::time::Duration::from_secs(*delay_secs)).await;
                    }

                    // All retries exhausted
                    tracing::error!(
                        alert_name = %alert_name,
                        webhook_url = %url,
                        error = ?last_error,
                        "Alert webhook failed after 3 attempts"
                    );
                });
            }
            AlertAction::Function { path } => {
                if let Some(engine) = &self.engine {
                    let engine = engine.clone();
                    let function_id = path.clone();
                    let payload = serde_json::json!({
                        "alert": event.name,
                        "metric": event.metric,
                        "value": event.value,
                        "threshold": event.threshold,
                        "operator": event.operator,
                        "timestamp": event.timestamp,
                        "firing": event.firing,
                    });

                    tokio::spawn(async move {
                        match engine.call(&function_id, payload).await {
                            Ok(_) => {
                                tracing::debug!(
                                    function_id = %function_id,
                                    "Alert function invoked successfully"
                                );
                            }
                            Err(e) => {
                                tracing::error!(
                                    function_id = %function_id,
                                    error = ?e,
                                    "Failed to invoke alert function"
                                );
                            }
                        }
                    });
                } else {
                    tracing::warn!(
                        alert_name = %event.name,
                        function_id = %path,
                        "Alert function action configured but no engine reference available"
                    );
                }
            }
        }
    }

    /// Get current state of all alerts
    pub fn get_states(&self) -> Vec<AlertState> {
        self.states.read().unwrap().values().cloned().collect()
    }

    /// Get alerts that are currently firing
    pub fn get_firing_alerts(&self) -> Vec<AlertState> {
        self.states
            .read()
            .unwrap()
            .values()
            .filter(|s| s.firing)
            .cloned()
            .collect()
    }
}

/// Global alert manager
static ALERT_MANAGER: OnceLock<Arc<AlertManager>> = OnceLock::new();

/// Initialize the alert manager with the given rules
pub fn init_alert_manager(rules: Vec<AlertRule>) {
    let manager = Arc::new(AlertManager::new(rules));
    if ALERT_MANAGER.set(manager).is_err() {
        tracing::debug!("Alert manager already initialized");
    }
}

/// Initialize the alert manager with engine reference for function invocation
pub fn init_alert_manager_with_engine(rules: Vec<AlertRule>, engine: Arc<crate::engine::Engine>) {
    let manager = Arc::new(AlertManager::with_engine(rules, engine));
    if ALERT_MANAGER.set(manager).is_err() {
        tracing::debug!("Alert manager already initialized");
    }
}

/// Get the global alert manager (if initialized)
pub fn get_alert_manager() -> Option<Arc<AlertManager>> {
    ALERT_MANAGER.get().cloned()
}

// =============================================================================
// Global Rollup Storage
// =============================================================================

/// Global rollup storage
static ROLLUP_STORAGE: OnceLock<Arc<RollupStorage>> = OnceLock::new();

/// Default rollup levels:
/// - Level 0: 1 minute intervals, 1 hour retention
/// - Level 1: 5 minute intervals, 6 hours retention
/// - Level 2: 1 hour intervals, 7 days retention
pub fn default_rollup_levels() -> Vec<RollupLevel> {
    vec![
        RollupLevel {
            interval_ns: 60 * 1_000_000_000,    // 1 minute
            retention_ns: 3600 * 1_000_000_000, // 1 hour
        },
        RollupLevel {
            interval_ns: 300 * 1_000_000_000,    // 5 minutes
            retention_ns: 21600 * 1_000_000_000, // 6 hours
        },
        RollupLevel {
            interval_ns: 3600 * 1_000_000_000,    // 1 hour
            retention_ns: 604800 * 1_000_000_000, // 7 days
        },
    ]
}

/// Initialize the rollup storage with default levels
pub fn init_rollup_storage() {
    let storage = Arc::new(RollupStorage::new(default_rollup_levels()));
    if ROLLUP_STORAGE.set(storage).is_err() {
        tracing::debug!("Rollup storage already initialized");
    }
}

/// Initialize the rollup storage with custom levels
pub fn init_rollup_storage_with_levels(levels: Vec<RollupLevel>) {
    let storage = Arc::new(RollupStorage::new(levels));
    if ROLLUP_STORAGE.set(storage).is_err() {
        tracing::debug!("Rollup storage already initialized");
    }
}

/// Get the global rollup storage (if initialized)
pub fn get_rollup_storage() -> Option<Arc<RollupStorage>> {
    ROLLUP_STORAGE.get().cloned()
}

/// Process new metrics through the rollup system
pub fn process_metrics_for_rollups(metrics: &[StoredMetric]) {
    if let Some(storage) = get_rollup_storage() {
        storage.process_metrics(metrics);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_default_meter_makes_get_meter_available() {
        // After calling ensure_default_meter, get_meter() should return Some
        ensure_default_meter();
        assert!(
            get_meter().is_some(),
            "get_meter() should return Some after ensure_default_meter()"
        );
    }

    #[test]
    fn test_get_engine_metrics_does_not_panic_after_ensure_default_meter() {
        ensure_default_meter();
        // This should NOT panic
        let metrics = get_engine_metrics();
        // Verify we can use the metrics (they should be functional noop counters)
        metrics.invocations_total.add(1, &[]);
    }

    // =========================================================================
    // Helper functions for tests
    // =========================================================================

    /// Create a StoredMetric with number data points for testing.
    fn make_number_metric(
        name: &str,
        service_name: &str,
        metric_type: StoredMetricType,
        values: &[(f64, u64)], // (value, timestamp_unix_nano)
        attributes: Vec<(String, String)>,
    ) -> StoredMetric {
        let data_points = values
            .iter()
            .map(|(value, ts)| {
                StoredDataPoint::Number(StoredNumberDataPoint {
                    value: *value,
                    attributes: attributes.clone(),
                    timestamp_unix_nano: *ts,
                })
            })
            .collect();
        StoredMetric {
            name: name.to_string(),
            description: format!("Test metric {}", name),
            unit: "1".to_string(),
            metric_type,
            data_points,
            service_name: service_name.to_string(),
            timestamp_unix_nano: values.first().map(|(_, ts)| *ts).unwrap_or(0),
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
        }
    }

    /// Create a StoredMetric with histogram data points for testing.
    fn make_histogram_metric(
        name: &str,
        service_name: &str,
        count: u64,
        sum: f64,
        bucket_counts: Vec<u64>,
        explicit_bounds: Vec<f64>,
        min: Option<f64>,
        max: Option<f64>,
        timestamp: u64,
    ) -> StoredMetric {
        let dp = StoredDataPoint::Histogram(StoredHistogramDataPoint {
            count,
            sum,
            bucket_counts,
            explicit_bounds,
            min,
            max,
            attributes: vec![],
            timestamp_unix_nano: timestamp,
        });
        StoredMetric {
            name: name.to_string(),
            description: format!("Test histogram {}", name),
            unit: "ms".to_string(),
            metric_type: StoredMetricType::Histogram,
            data_points: vec![dp],
            service_name: service_name.to_string(),
            timestamp_unix_nano: timestamp,
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
        }
    }

    // =========================================================================
    // 1. InMemoryMetricStorage (TimeIndexedMetricStorage) tests
    // =========================================================================

    #[test]
    fn test_metric_storage_store_and_get() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        let metric = make_number_metric(
            "cpu.usage",
            "my-service",
            StoredMetricType::Gauge,
            &[(75.5, 1_000_000_000)],
            vec![],
        );

        storage.add_metrics(vec![metric]);

        let all = storage.get_metrics();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "cpu.usage");
        assert_eq!(all[0].service_name, "my-service");

        if let StoredDataPoint::Number(dp) = &all[0].data_points[0] {
            assert_eq!(dp.value, 75.5);
        } else {
            panic!("Expected Number data point");
        }
    }

    #[test]
    fn test_metric_storage_get_by_name() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        let m1 = make_number_metric(
            "cpu.usage",
            "svc",
            StoredMetricType::Gauge,
            &[(50.0, 1_000_000_000)],
            vec![],
        );
        let m2 = make_number_metric(
            "memory.usage",
            "svc",
            StoredMetricType::Gauge,
            &[(1024.0, 2_000_000_000)],
            vec![],
        );
        let m3 = make_number_metric(
            "cpu.usage",
            "svc",
            StoredMetricType::Gauge,
            &[(60.0, 3_000_000_000)],
            vec![],
        );

        storage.add_metrics(vec![m1, m2, m3]);

        let cpu_metrics = storage.get_metrics_by_name("cpu.usage");
        assert_eq!(cpu_metrics.len(), 2);
        assert!(cpu_metrics.iter().all(|m| m.name == "cpu.usage"));

        let mem_metrics = storage.get_metrics_by_name("memory.usage");
        assert_eq!(mem_metrics.len(), 1);
        assert_eq!(mem_metrics[0].name, "memory.usage");

        let missing = storage.get_metrics_by_name("nonexistent.metric");
        assert!(missing.is_empty());
    }

    #[test]
    fn test_metric_storage_circular_buffer() {
        // Create storage with max capacity of 5
        let storage = InMemoryMetricStorage::new(5, DEFAULT_RETENTION_NS);

        // Add 8 metrics with distinct timestamps so each goes into its own bucket
        for i in 0..8u64 {
            let metric = make_number_metric(
                &format!("metric_{}", i),
                "svc",
                StoredMetricType::Gauge,
                &[(i as f64, (i + 1) * 1_000_000_000)],
                vec![],
            );
            storage.add_metrics(vec![metric]);
        }

        // Should have at most 5 metrics (oldest evicted)
        let all = storage.get_metrics();
        assert!(
            all.len() <= 5,
            "Expected at most 5 metrics, got {}",
            all.len()
        );

        // The oldest metrics (metric_0, metric_1, metric_2) should have been evicted
        let names: Vec<&str> = all.iter().map(|m| m.name.as_str()).collect();
        assert!(
            !names.contains(&"metric_0"),
            "metric_0 should have been evicted"
        );
        assert!(
            !names.contains(&"metric_1"),
            "metric_1 should have been evicted"
        );
        assert!(
            !names.contains(&"metric_2"),
            "metric_2 should have been evicted"
        );
    }

    #[test]
    fn test_metric_storage_clear() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        let m1 = make_number_metric(
            "a",
            "svc",
            StoredMetricType::Counter,
            &[(1.0, 1_000_000_000)],
            vec![],
        );
        let m2 = make_number_metric(
            "b",
            "svc",
            StoredMetricType::Counter,
            &[(2.0, 2_000_000_000)],
            vec![],
        );

        storage.add_metrics(vec![m1, m2]);
        assert_eq!(storage.len(), 2);
        assert!(!storage.is_empty());

        storage.clear();

        assert_eq!(storage.len(), 0);
        assert!(storage.is_empty());
        assert!(storage.get_metrics().is_empty());
        assert!(storage.get_metrics_by_name("a").is_empty());
    }

    #[test]
    fn test_metric_storage_get_filtered() {
        let storage = InMemoryMetricStorage::new(1000, DEFAULT_RETENTION_NS);

        // Add metrics with different names, services, and timestamps
        let m1 = make_number_metric(
            "http.requests",
            "web-server",
            StoredMetricType::Counter,
            &[(10.0, 100_000_000_000)],
            vec![("env".to_string(), "prod".to_string())],
        );
        let m2 = make_number_metric(
            "http.requests",
            "web-server",
            StoredMetricType::Counter,
            &[(20.0, 200_000_000_000)],
            vec![("env".to_string(), "prod".to_string())],
        );
        let m3 = make_number_metric(
            "http.requests",
            "api-server",
            StoredMetricType::Counter,
            &[(5.0, 300_000_000_000)],
            vec![("env".to_string(), "staging".to_string())],
        );
        let m4 = make_number_metric(
            "db.queries",
            "web-server",
            StoredMetricType::Counter,
            &[(100.0, 150_000_000_000)],
            vec![],
        );

        storage.add_metrics(vec![m1, m2, m3, m4]);

        // Filter by name
        let http_metrics = storage.get_metrics_by_name("http.requests");
        assert_eq!(http_metrics.len(), 3);

        // Filter by name and time range
        let range_metrics =
            storage.get_metrics_by_name_in_range("http.requests", 50_000_000_000, 250_000_000_000);
        assert_eq!(range_metrics.len(), 2);

        // Filter by time range only
        let all_in_range = storage.get_metrics_in_range(100_000_000_000, 200_000_000_000);
        assert_eq!(all_in_range.len(), 3); // m1, m4, m2

        // Filter by service name - iterate and check manually since there's no direct service filter
        let web_metrics: Vec<_> = storage
            .get_metrics()
            .into_iter()
            .filter(|m| m.service_name == "web-server")
            .collect();
        assert_eq!(web_metrics.len(), 3);
    }

    #[test]
    fn test_metric_storage_ttl_expiry() {
        // Create storage with a very short retention: 1 second (1_000_000_000 ns)
        let storage = InMemoryMetricStorage::new(1000, 1_000_000_000);

        // Add a metric with a timestamp far in the past (well beyond TTL)
        let old_metric = make_number_metric(
            "old.metric",
            "svc",
            StoredMetricType::Gauge,
            &[(1.0, 1_000_000)], // Extremely old timestamp (1 millisecond after epoch)
            vec![],
        );

        // Add a metric with a current timestamp
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let current_metric = make_number_metric(
            "current.metric",
            "svc",
            StoredMetricType::Gauge,
            &[(2.0, now_ns)],
            vec![],
        );

        storage.add_metrics(vec![old_metric, current_metric]);
        assert_eq!(storage.len(), 2);

        // Apply retention - should evict the old metric
        storage.apply_retention();

        let remaining = storage.get_metrics();
        assert_eq!(remaining.len(), 1, "Old metric should have been evicted");
        assert_eq!(remaining[0].name, "current.metric");
    }

    // =========================================================================
    // 2. MetricsAccumulator tests
    // =========================================================================

    #[test]
    fn test_accumulator_increment_function() {
        let acc = MetricsAccumulator::default();

        acc.increment_function("myFunc");

        assert_eq!(acc.get_function_count("myFunc"), Some(1));

        acc.increment_function("myFunc");
        acc.increment_function("myFunc");

        assert_eq!(acc.get_function_count("myFunc"), Some(3));
    }

    #[test]
    fn test_accumulator_get_function_count() {
        let acc = MetricsAccumulator::default();

        acc.increment_function("alpha");
        acc.increment_function("alpha");
        acc.increment_function("beta");

        assert_eq!(acc.get_function_count("alpha"), Some(2));
        assert_eq!(acc.get_function_count("beta"), Some(1));
    }

    #[test]
    fn test_accumulator_get_function_count_missing() {
        let acc = MetricsAccumulator::default();

        // Function that was never incremented should return None (not 0)
        assert_eq!(acc.get_function_count("nonexistent"), None);
    }

    #[test]
    fn test_accumulator_get_by_function() {
        let acc = MetricsAccumulator::default();

        acc.increment_function("fn_a");
        acc.increment_function("fn_a");
        acc.increment_function("fn_b");

        let by_func = acc.get_by_function();
        assert_eq!(by_func.len(), 2);
        assert_eq!(by_func.get("fn_a"), Some(&2));
        assert_eq!(by_func.get("fn_b"), Some(&1));
    }

    #[test]
    fn test_accumulator_iter_function_counts() {
        let acc = MetricsAccumulator::default();

        acc.increment_function("x");
        acc.increment_function("x");
        acc.increment_function("y");
        acc.increment_function("z");
        acc.increment_function("z");
        acc.increment_function("z");

        let mut counts: std::collections::HashMap<String, u64> =
            acc.iter_function_counts().collect();

        assert_eq!(counts.remove("x"), Some(2));
        assert_eq!(counts.remove("y"), Some(1));
        assert_eq!(counts.remove("z"), Some(3));
        assert!(counts.is_empty());
    }

    #[test]
    fn test_accumulator_multiple_functions() {
        let acc = MetricsAccumulator::default();

        let functions = ["login", "logout", "register", "profile", "settings"];
        for (i, func) in functions.iter().enumerate() {
            for _ in 0..=(i as u64) {
                acc.increment_function(func);
            }
        }

        assert_eq!(acc.get_function_count("login"), Some(1));
        assert_eq!(acc.get_function_count("logout"), Some(2));
        assert_eq!(acc.get_function_count("register"), Some(3));
        assert_eq!(acc.get_function_count("profile"), Some(4));
        assert_eq!(acc.get_function_count("settings"), Some(5));

        let by_func = acc.get_by_function();
        assert_eq!(by_func.len(), 5);
    }

    // =========================================================================
    // 3. StoredMetric / StoredDataPoint tests
    // =========================================================================

    #[test]
    fn test_stored_metric_gauge() {
        let dp1 = StoredDataPoint::Number(StoredNumberDataPoint {
            value: 42.0,
            attributes: vec![("host".to_string(), "server-1".to_string())],
            timestamp_unix_nano: 1_000_000_000,
        });
        let dp2 = StoredDataPoint::Number(StoredNumberDataPoint {
            value: 43.5,
            attributes: vec![("host".to_string(), "server-2".to_string())],
            timestamp_unix_nano: 2_000_000_000,
        });

        let metric = StoredMetric {
            name: "system.cpu.usage".to_string(),
            description: "CPU usage percentage".to_string(),
            unit: "%".to_string(),
            metric_type: StoredMetricType::Gauge,
            data_points: vec![dp1, dp2],
            service_name: "monitoring".to_string(),
            timestamp_unix_nano: 1_000_000_000,
            instrumentation_scope_name: Some("cpu-monitor".to_string()),
            instrumentation_scope_version: Some("1.0.0".to_string()),
        };

        assert_eq!(metric.name, "system.cpu.usage");
        assert!(matches!(metric.metric_type, StoredMetricType::Gauge));
        assert_eq!(metric.data_points.len(), 2);
        assert_eq!(metric.service_name, "monitoring");
        assert_eq!(
            metric.instrumentation_scope_name.as_deref(),
            Some("cpu-monitor")
        );
        assert_eq!(
            metric.instrumentation_scope_version.as_deref(),
            Some("1.0.0")
        );

        // Verify data point values
        if let StoredDataPoint::Number(dp) = &metric.data_points[0] {
            assert_eq!(dp.value, 42.0);
            assert_eq!(
                dp.attributes[0],
                ("host".to_string(), "server-1".to_string())
            );
        } else {
            panic!("Expected Number data point");
        }

        if let StoredDataPoint::Number(dp) = &metric.data_points[1] {
            assert_eq!(dp.value, 43.5);
        } else {
            panic!("Expected Number data point");
        }
    }

    #[test]
    fn test_stored_metric_counter() {
        let dp = StoredDataPoint::Number(StoredNumberDataPoint {
            value: 1234.0,
            attributes: vec![
                ("method".to_string(), "GET".to_string()),
                ("status".to_string(), "200".to_string()),
            ],
            timestamp_unix_nano: 5_000_000_000,
        });

        let metric = StoredMetric {
            name: "http.requests.total".to_string(),
            description: "Total HTTP requests".to_string(),
            unit: "1".to_string(),
            metric_type: StoredMetricType::Counter,
            data_points: vec![dp],
            service_name: "api-gateway".to_string(),
            timestamp_unix_nano: 5_000_000_000,
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
        };

        assert!(matches!(metric.metric_type, StoredMetricType::Counter));
        assert_eq!(metric.data_points.len(), 1);

        if let StoredDataPoint::Number(dp) = &metric.data_points[0] {
            assert_eq!(dp.value, 1234.0);
            assert_eq!(dp.attributes.len(), 2);
            assert_eq!(dp.attributes[0].0, "method");
            assert_eq!(dp.attributes[0].1, "GET");
            assert_eq!(dp.attributes[1].0, "status");
            assert_eq!(dp.attributes[1].1, "200");
        } else {
            panic!("Expected Number data point");
        }
    }

    #[test]
    fn test_stored_metric_histogram() {
        let metric = make_histogram_metric(
            "http.request.duration",
            "web-app",
            500,
            2345.67,
            vec![50, 150, 200, 80, 20],
            vec![10.0, 50.0, 100.0, 500.0],
            Some(0.5),
            Some(980.3),
            10_000_000_000,
        );

        assert!(matches!(metric.metric_type, StoredMetricType::Histogram));
        assert_eq!(metric.name, "http.request.duration");
        assert_eq!(metric.service_name, "web-app");
        assert_eq!(metric.unit, "ms");

        if let StoredDataPoint::Histogram(dp) = &metric.data_points[0] {
            assert_eq!(dp.count, 500);
            assert_eq!(dp.sum, 2345.67);
            assert_eq!(dp.bucket_counts.len(), 5);
            assert_eq!(dp.explicit_bounds.len(), 4);
            assert_eq!(dp.min, Some(0.5));
            assert_eq!(dp.max, Some(980.3));
            // Verify bucket counts sum up to the total count
            let total: u64 = dp.bucket_counts.iter().sum();
            assert_eq!(total, dp.count);
        } else {
            panic!("Expected Histogram data point");
        }

        // Also verify histogram storage works via TimeIndexedMetricStorage
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);
        storage.add_metrics(vec![metric]);
        let retrieved = storage.get_metrics_by_name("http.request.duration");
        assert_eq!(retrieved.len(), 1);
        assert!(matches!(
            retrieved[0].metric_type,
            StoredMetricType::Histogram
        ));
    }

    #[test]
    fn test_stored_metric_summary() {
        // The codebase uses UpDownCounter as the closest analogue to summary-style metrics.
        // Verify UpDownCounter construction and serialization.
        let dp = StoredDataPoint::Number(StoredNumberDataPoint {
            value: -5.0, // UpDownCounter can go negative
            attributes: vec![("queue".to_string(), "jobs".to_string())],
            timestamp_unix_nano: 7_000_000_000,
        });

        let metric = StoredMetric {
            name: "queue.depth".to_string(),
            description: "Current queue depth".to_string(),
            unit: "1".to_string(),
            metric_type: StoredMetricType::UpDownCounter,
            data_points: vec![dp],
            service_name: "worker-pool".to_string(),
            timestamp_unix_nano: 7_000_000_000,
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
        };

        assert!(matches!(
            metric.metric_type,
            StoredMetricType::UpDownCounter
        ));

        if let StoredDataPoint::Number(dp) = &metric.data_points[0] {
            assert_eq!(dp.value, -5.0);
        } else {
            panic!("Expected Number data point");
        }

        // Verify it serializes correctly
        let json = serde_json::to_string(&metric).unwrap();
        assert!(json.contains("\"updowncounter\""));
        assert!(json.contains("queue.depth"));
    }

    // =========================================================================
    // 4. OTLP metrics ingestion edge cases
    //    These tests use ingest_otlp_metrics from the otel module and require
    //    global metric storage, so they need #[serial].
    // =========================================================================

    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn test_ingest_metrics_multiple_resources() {
        init_metric_storage(Some(100), Some(3600));
        if let Some(storage) = get_metric_storage() {
            storage.clear();
        }

        let otlp_json = r#"{
            "resourceMetrics": [
                {
                    "resource": {
                        "attributes": [{
                            "key": "service.name",
                            "value": {"stringValue": "service-alpha"}
                        }]
                    },
                    "scopeMetrics": [{
                        "metrics": [{
                            "name": "alpha.requests",
                            "description": "Alpha requests",
                            "unit": "1",
                            "sum": {
                                "dataPoints": [{
                                    "timeUnixNano": "1704067200000000000",
                                    "asDouble": 100.0
                                }],
                                "isMonotonic": true
                            }
                        }]
                    }]
                },
                {
                    "resource": {
                        "attributes": [{
                            "key": "service.name",
                            "value": {"stringValue": "service-beta"}
                        }]
                    },
                    "scopeMetrics": [{
                        "metrics": [{
                            "name": "beta.requests",
                            "description": "Beta requests",
                            "unit": "1",
                            "sum": {
                                "dataPoints": [{
                                    "timeUnixNano": "1704067200000000000",
                                    "asDouble": 200.0
                                }],
                                "isMonotonic": true
                            }
                        }]
                    }]
                }
            ]
        }"#;

        let result = crate::modules::observability::otel::ingest_otlp_metrics(otlp_json).await;
        assert!(result.is_ok());

        let storage = get_metric_storage().unwrap();
        let all = storage.get_metrics();
        assert_eq!(all.len(), 2);

        let alpha: Vec<_> = all.iter().filter(|m| m.name == "alpha.requests").collect();
        let beta: Vec<_> = all.iter().filter(|m| m.name == "beta.requests").collect();

        assert_eq!(alpha.len(), 1);
        assert_eq!(alpha[0].service_name, "service-alpha");

        assert_eq!(beta.len(), 1);
        assert_eq!(beta[0].service_name, "service-beta");
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_metrics_multiple_scopes() {
        init_metric_storage(Some(100), Some(3600));
        if let Some(storage) = get_metric_storage() {
            storage.clear();
        }

        let otlp_json = r#"{
            "resourceMetrics": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "multi-scope-svc"}
                    }]
                },
                "scopeMetrics": [
                    {
                        "scope": {
                            "name": "@opentelemetry/instrumentation-http",
                            "version": "0.52.0"
                        },
                        "metrics": [{
                            "name": "http.client.duration",
                            "description": "HTTP client duration",
                            "unit": "ms",
                            "gauge": {
                                "dataPoints": [{
                                    "timeUnixNano": "1704067200000000000",
                                    "asDouble": 150.0
                                }]
                            }
                        }]
                    },
                    {
                        "scope": {
                            "name": "@opentelemetry/instrumentation-express",
                            "version": "0.40.0"
                        },
                        "metrics": [{
                            "name": "express.request.count",
                            "description": "Express request count",
                            "unit": "1",
                            "sum": {
                                "dataPoints": [{
                                    "timeUnixNano": "1704067200000000000",
                                    "asDouble": 42.0
                                }],
                                "isMonotonic": true
                            }
                        }]
                    }
                ]
            }]
        }"#;

        let result = crate::modules::observability::otel::ingest_otlp_metrics(otlp_json).await;
        assert!(result.is_ok());

        let storage = get_metric_storage().unwrap();
        let all = storage.get_metrics();
        assert_eq!(all.len(), 2);

        let http_metrics: Vec<_> = all
            .iter()
            .filter(|m| m.name == "http.client.duration")
            .collect();
        assert_eq!(http_metrics.len(), 1);
        assert_eq!(
            http_metrics[0].instrumentation_scope_name.as_deref(),
            Some("@opentelemetry/instrumentation-http")
        );
        assert_eq!(
            http_metrics[0].instrumentation_scope_version.as_deref(),
            Some("0.52.0")
        );

        let express_metrics: Vec<_> = all
            .iter()
            .filter(|m| m.name == "express.request.count")
            .collect();
        assert_eq!(express_metrics.len(), 1);
        assert_eq!(
            express_metrics[0].instrumentation_scope_name.as_deref(),
            Some("@opentelemetry/instrumentation-express")
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_metrics_missing_service_name() {
        init_metric_storage(Some(100), Some(3600));
        if let Some(storage) = get_metric_storage() {
            storage.clear();
        }

        // Resource with no service.name attribute at all
        let otlp_json = r#"{
            "resourceMetrics": [{
                "resource": {
                    "attributes": [{
                        "key": "host.name",
                        "value": {"stringValue": "my-host"}
                    }]
                },
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "no.service.metric",
                        "description": "Metric without service.name",
                        "unit": "1",
                        "gauge": {
                            "dataPoints": [{
                                "timeUnixNano": "1704067200000000000",
                                "asDouble": 99.0
                            }]
                        }
                    }]
                }]
            }]
        }"#;

        let result = crate::modules::observability::otel::ingest_otlp_metrics(otlp_json).await;
        assert!(result.is_ok());

        let storage = get_metric_storage().unwrap();
        let metrics = storage.get_metrics_by_name("no.service.metric");
        assert_eq!(metrics.len(), 1);
        // When service.name is missing, it should default to "unknown"
        assert_eq!(metrics[0].service_name, "unknown");
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_metrics_with_attributes() {
        init_metric_storage(Some(100), Some(3600));
        if let Some(storage) = get_metric_storage() {
            storage.clear();
        }

        let otlp_json = r#"{
            "resourceMetrics": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "attr-service"}
                    }]
                },
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "http.server.duration",
                        "description": "Duration with attributes",
                        "unit": "ms",
                        "gauge": {
                            "dataPoints": [{
                                "attributes": [
                                    {
                                        "key": "http.method",
                                        "value": {"stringValue": "POST"}
                                    },
                                    {
                                        "key": "http.status_code",
                                        "value": {"intValue": 201}
                                    },
                                    {
                                        "key": "http.response_time",
                                        "value": {"doubleValue": 0.456}
                                    }
                                ],
                                "timeUnixNano": "1704067200000000000",
                                "asDouble": 123.45
                            }]
                        }
                    }]
                }]
            }]
        }"#;

        let result = crate::modules::observability::otel::ingest_otlp_metrics(otlp_json).await;
        assert!(result.is_ok());

        let storage = get_metric_storage().unwrap();
        let metrics = storage.get_metrics_by_name("http.server.duration");
        assert_eq!(metrics.len(), 1);

        if let StoredDataPoint::Number(dp) = &metrics[0].data_points[0] {
            assert_eq!(dp.value, 123.45);
            assert_eq!(dp.attributes.len(), 3);

            // Verify all attributes are present
            let attrs: std::collections::HashMap<&str, &str> = dp
                .attributes
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            assert_eq!(attrs.get("http.method"), Some(&"POST"));
            assert_eq!(attrs.get("http.status_code"), Some(&"201"));
            assert_eq!(attrs.get("http.response_time"), Some(&"0.456"));
        } else {
            panic!("Expected Number data point");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_ingest_metrics_gauge_with_int() {
        init_metric_storage(Some(100), Some(3600));
        if let Some(storage) = get_metric_storage() {
            storage.clear();
        }

        // Use asInt instead of asDouble for the gauge data point
        let otlp_json = r#"{
            "resourceMetrics": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": "int-service"}
                    }]
                },
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "active.connections",
                        "description": "Active connections as integer",
                        "unit": "1",
                        "gauge": {
                            "dataPoints": [{
                                "timeUnixNano": "1704067200000000000",
                                "asInt": 42
                            }]
                        }
                    }]
                }]
            }]
        }"#;

        let result = crate::modules::observability::otel::ingest_otlp_metrics(otlp_json).await;
        assert!(result.is_ok());

        let storage = get_metric_storage().unwrap();
        let metrics = storage.get_metrics_by_name("active.connections");
        assert_eq!(metrics.len(), 1);
        assert!(matches!(metrics[0].metric_type, StoredMetricType::Gauge));

        if let StoredDataPoint::Number(dp) = &metrics[0].data_points[0] {
            // asInt: 42 should be converted to f64 value 42.0
            assert_eq!(dp.value, 42.0);
        } else {
            panic!("Expected Number data point");
        }
    }

    // =========================================================================
    // get_metrics_in_range tests
    // =========================================================================

    #[test]
    fn test_get_metrics_in_range_basic() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        storage.add_metrics(vec![
            make_number_metric(
                "cpu.usage",
                "svc",
                StoredMetricType::Gauge,
                &[(50.0, 1000)],
                vec![],
            ),
            make_number_metric(
                "cpu.usage",
                "svc",
                StoredMetricType::Gauge,
                &[(60.0, 2000)],
                vec![],
            ),
            make_number_metric(
                "cpu.usage",
                "svc",
                StoredMetricType::Gauge,
                &[(70.0, 3000)],
                vec![],
            ),
            make_number_metric(
                "cpu.usage",
                "svc",
                StoredMetricType::Gauge,
                &[(80.0, 4000)],
                vec![],
            ),
        ]);

        let range_metrics = storage.get_metrics_in_range(2000, 3000);
        assert_eq!(range_metrics.len(), 2);
        // Should include timestamps 2000 and 3000
        let timestamps: Vec<u64> = range_metrics
            .iter()
            .map(|m| m.timestamp_unix_nano)
            .collect();
        assert!(timestamps.contains(&2000));
        assert!(timestamps.contains(&3000));
    }

    #[test]
    fn test_get_metrics_in_range_empty_result() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        storage.add_metrics(vec![make_number_metric(
            "mem",
            "svc",
            StoredMetricType::Gauge,
            &[(100.0, 1000)],
            vec![],
        )]);

        let range_metrics = storage.get_metrics_in_range(5000, 9000);
        assert!(range_metrics.is_empty());
    }

    #[test]
    fn test_get_metrics_in_range_single_point() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        storage.add_metrics(vec![make_number_metric(
            "disk",
            "svc",
            StoredMetricType::Gauge,
            &[(42.0, 5000)],
            vec![],
        )]);

        // Exact match on boundaries
        let range_metrics = storage.get_metrics_in_range(5000, 5000);
        assert_eq!(range_metrics.len(), 1);
    }

    // =========================================================================
    // get_metrics_by_name_in_range tests
    // =========================================================================

    #[test]
    fn test_get_metrics_by_name_in_range_basic() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        storage.add_metrics(vec![
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(10.0, 1000)],
                vec![],
            ),
            make_number_metric(
                "mem",
                "svc",
                StoredMetricType::Gauge,
                &[(20.0, 2000)],
                vec![],
            ),
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(30.0, 3000)],
                vec![],
            ),
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(40.0, 4000)],
                vec![],
            ),
        ]);

        let result = storage.get_metrics_by_name_in_range("cpu", 1000, 3000);
        assert_eq!(result.len(), 2);
        for m in &result {
            assert_eq!(m.name, "cpu");
        }
    }

    #[test]
    fn test_get_metrics_by_name_in_range_nonexistent_name() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        storage.add_metrics(vec![make_number_metric(
            "cpu",
            "svc",
            StoredMetricType::Gauge,
            &[(10.0, 1000)],
            vec![],
        )]);

        let result = storage.get_metrics_by_name_in_range("nonexistent", 0, 9999);
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_metrics_by_name_in_range_out_of_range() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        storage.add_metrics(vec![make_number_metric(
            "cpu",
            "svc",
            StoredMetricType::Gauge,
            &[(10.0, 1000)],
            vec![],
        )]);

        let result = storage.get_metrics_by_name_in_range("cpu", 5000, 9000);
        assert!(result.is_empty());
    }

    // =========================================================================
    // apply_retention tests
    // =========================================================================

    #[test]
    fn test_apply_retention_removes_old_metrics() {
        // Use a very short retention (1 second in nanoseconds)
        let retention_ns = 1_000_000_000u64;
        let storage = InMemoryMetricStorage::new(100, retention_ns);

        // Add a metric with a very old timestamp (well before any retention cutoff)
        let old_ts = 1_000_000_000u64; // 1 second since epoch - ancient
        storage.add_metrics(vec![make_number_metric(
            "old.metric",
            "svc",
            StoredMetricType::Gauge,
            &[(42.0, old_ts)],
            vec![],
        )]);

        assert_eq!(storage.len(), 1);

        // Apply retention - this uses current time as reference
        // Since the metric timestamp is ~50 years old, it should be removed
        storage.apply_retention();

        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_apply_retention_keeps_recent_metrics() {
        // Use a large retention (1 hour)
        let retention_ns = 3600_000_000_000u64;
        let storage = InMemoryMetricStorage::new(100, retention_ns);

        // Use a current-ish timestamp
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        storage.add_metrics(vec![make_number_metric(
            "recent.metric",
            "svc",
            StoredMetricType::Gauge,
            &[(42.0, now_ns)],
            vec![],
        )]);

        assert_eq!(storage.len(), 1);

        storage.apply_retention();

        // Recent metric should still be present
        assert_eq!(storage.len(), 1);
    }

    // =========================================================================
    // get_aggregated_metrics tests
    // =========================================================================

    #[test]
    fn test_get_aggregated_metrics_single_bucket() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        // All metrics within the same bucket (interval_ns = 10000)
        storage.add_metrics(vec![
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(10.0, 1000)],
                vec![],
            ),
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(20.0, 2000)],
                vec![],
            ),
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(30.0, 3000)],
                vec![],
            ),
        ]);

        let aggregated = storage.get_aggregated_metrics(0, 10000, 10000);

        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].name, "cpu");
        assert_eq!(aggregated[0].count, 3);
        assert_eq!(aggregated[0].sum, 60.0);
        assert_eq!(aggregated[0].min, 10.0);
        assert_eq!(aggregated[0].max, 30.0);
        assert_eq!(aggregated[0].avg, 20.0);
    }

    #[test]
    fn test_get_aggregated_metrics_multiple_buckets() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        // Metrics across two buckets (interval_ns = 5)
        storage.add_metrics(vec![
            make_number_metric("cpu", "svc", StoredMetricType::Gauge, &[(10.0, 1)], vec![]),
            make_number_metric("cpu", "svc", StoredMetricType::Gauge, &[(20.0, 3)], vec![]),
            make_number_metric("cpu", "svc", StoredMetricType::Gauge, &[(30.0, 6)], vec![]),
            make_number_metric("cpu", "svc", StoredMetricType::Gauge, &[(40.0, 8)], vec![]),
        ]);

        let aggregated = storage.get_aggregated_metrics(0, 10, 5);

        assert_eq!(aggregated.len(), 2);
        // First bucket (0-4): values 10, 20
        // Second bucket (5-9): values 30, 40
        // Sorted by name then bucket_start
        let first = &aggregated[0];
        assert_eq!(first.count, 2);
        assert_eq!(first.sum, 30.0);

        let second = &aggregated[1];
        assert_eq!(second.count, 2);
        assert_eq!(second.sum, 70.0);
    }

    #[test]
    fn test_get_aggregated_metrics_with_percentiles() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        // Need at least 5 data points for percentiles
        for i in 0..10 {
            storage.add_metrics(vec![make_number_metric(
                "latency",
                "svc",
                StoredMetricType::Gauge,
                &[((i as f64 + 1.0) * 10.0, i)],
                vec![],
            )]);
        }

        let aggregated = storage.get_aggregated_metrics(0, 100, 100);

        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].count, 10);
        assert!(aggregated[0].percentiles.is_some());
        let p = aggregated[0].percentiles.as_ref().unwrap();
        // Values are 10, 20, 30, 40, 50, 60, 70, 80, 90, 100
        assert!(p.p50 > 0.0);
        assert!(p.p95 > p.p50);
        assert!(p.p99 >= p.p95);
    }

    #[test]
    fn test_get_aggregated_metrics_no_percentiles_for_small_sets() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        // Only 3 data points - not enough for percentiles
        storage.add_metrics(vec![
            make_number_metric("cpu", "svc", StoredMetricType::Gauge, &[(10.0, 1)], vec![]),
            make_number_metric("cpu", "svc", StoredMetricType::Gauge, &[(20.0, 2)], vec![]),
            make_number_metric("cpu", "svc", StoredMetricType::Gauge, &[(30.0, 3)], vec![]),
        ]);

        let aggregated = storage.get_aggregated_metrics(0, 100, 100);

        assert_eq!(aggregated.len(), 1);
        assert!(aggregated[0].percentiles.is_none());
    }

    // =========================================================================
    // get_aggregated_histograms tests
    // =========================================================================

    #[test]
    fn test_get_aggregated_histograms_basic() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        storage.add_metrics(vec![
            make_histogram_metric(
                "request.duration",
                "svc",
                50,
                250.0,
                vec![10, 20, 15, 5],
                vec![10.0, 50.0, 100.0],
                Some(1.0),
                Some(95.0),
                1000,
            ),
            make_histogram_metric(
                "request.duration",
                "svc",
                30,
                150.0,
                vec![5, 15, 8, 2],
                vec![10.0, 50.0, 100.0],
                Some(2.0),
                Some(88.0),
                2000,
            ),
        ]);

        let aggregated = storage.get_aggregated_histograms(0, 10000, 10000);

        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].name, "request.duration");
        assert_eq!(aggregated[0].total_count, 80); // 50 + 30
        assert_eq!(aggregated[0].total_sum, 400.0); // 250 + 150
        assert_eq!(aggregated[0].min, Some(1.0));
        assert_eq!(aggregated[0].max, Some(95.0));
        // Merged bucket counts: [10+5, 20+15, 15+8, 5+2]
        assert_eq!(aggregated[0].bucket_counts, vec![15, 35, 23, 7]);
    }

    #[test]
    fn test_get_aggregated_histograms_ignores_non_histogram_metrics() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);

        storage.add_metrics(vec![
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(42.0, 1000)],
                vec![],
            ),
            make_histogram_metric(
                "latency",
                "svc",
                10,
                50.0,
                vec![5, 5],
                vec![10.0],
                Some(1.0),
                Some(20.0),
                1000,
            ),
        ]);

        let aggregated = storage.get_aggregated_histograms(0, 10000, 10000);

        // Only the histogram metric should appear
        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].name, "latency");
    }

    // =========================================================================
    // PercentileValues tests
    // =========================================================================

    #[test]
    fn test_percentile_values_empty_input() {
        let p = PercentileValues::from_sorted_values(&[]);
        assert_eq!(p.p50, 0.0);
        assert_eq!(p.p75, 0.0);
        assert_eq!(p.p90, 0.0);
        assert_eq!(p.p95, 0.0);
        assert_eq!(p.p99, 0.0);
    }

    #[test]
    fn test_percentile_values_single_element() {
        let p = PercentileValues::from_sorted_values(&[42.0]);
        assert_eq!(p.p50, 42.0);
        assert_eq!(p.p75, 42.0);
        assert_eq!(p.p90, 42.0);
        assert_eq!(p.p95, 42.0);
        assert_eq!(p.p99, 42.0);
    }

    #[test]
    fn test_percentile_values_sorted_data() {
        let values: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let p = PercentileValues::from_sorted_values(&values);

        // With 100 elements: p50 = values[50], p75 = values[75], etc.
        assert!(p.p50 > 0.0);
        assert!(p.p75 > p.p50);
        assert!(p.p90 > p.p75);
        assert!(p.p95 > p.p90);
        assert!(p.p99 > p.p95);
    }

    // =========================================================================
    // RollupStorage tests
    // =========================================================================

    #[test]
    fn test_rollup_storage_process_number_metrics() {
        let levels = vec![RollupLevel {
            interval_ns: 60_000_000_000,    // 1 minute
            retention_ns: 3600_000_000_000, // 1 hour
        }];
        let rollup = RollupStorage::new(levels);

        let metrics = vec![
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(10.0, 1_000_000_000)],
                vec![],
            ),
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(20.0, 2_000_000_000)],
                vec![],
            ),
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(30.0, 3_000_000_000)],
                vec![],
            ),
        ];

        rollup.process_metrics(&metrics);

        // All three metrics should be in the same bucket (0-59s bucket = bucket 0)
        let results = rollup.get_rollups(0, 0, 60_000_000_000, Some("cpu"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].count, 3);
        assert_eq!(results[0].sum, 60.0);
        assert_eq!(results[0].min, 10.0);
        assert_eq!(results[0].max, 30.0);
        assert_eq!(results[0].avg, 20.0);
    }

    #[test]
    fn test_rollup_storage_process_histogram_metrics() {
        let levels = vec![RollupLevel {
            interval_ns: 60_000_000_000,
            retention_ns: 3600_000_000_000,
        }];
        let rollup = RollupStorage::new(levels);

        let metrics = vec![make_histogram_metric(
            "duration",
            "svc",
            100,
            500.0,
            vec![20, 40, 30, 10],
            vec![10.0, 50.0, 100.0],
            Some(1.0),
            Some(200.0),
            5_000_000_000,
        )];

        rollup.process_metrics(&metrics);

        let results = rollup.get_histogram_rollups(0, 0, 60_000_000_000, Some("duration"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].total_count, 100);
        assert_eq!(results[0].total_sum, 500.0);
        assert_eq!(results[0].min, Some(1.0));
        assert_eq!(results[0].max, Some(200.0));
    }

    #[test]
    fn test_rollup_storage_single_histogram_preserves_bucket_counts() {
        let levels = vec![RollupLevel {
            interval_ns: 60_000_000_000,
            retention_ns: 3600_000_000_000,
        }];
        let rollup = RollupStorage::new(levels);

        let metrics = vec![make_histogram_metric(
            "duration",
            "svc",
            3,
            60.0,
            vec![1, 1, 1],
            vec![10.0, 50.0],
            None,
            None,
            5_000_000_000,
        )];

        rollup.process_metrics(&metrics);

        let results = rollup.get_histogram_rollups(0, 0, 60_000_000_000, Some("duration"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].bucket_counts, vec![1, 1, 1]);
    }

    #[test]
    fn test_rollup_storage_get_rollups_out_of_range() {
        let levels = vec![RollupLevel {
            interval_ns: 60_000_000_000,
            retention_ns: 3600_000_000_000,
        }];
        let rollup = RollupStorage::new(levels);

        let metrics = vec![make_number_metric(
            "cpu",
            "svc",
            StoredMetricType::Gauge,
            &[(42.0, 1_000_000_000)],
            vec![],
        )];
        rollup.process_metrics(&metrics);

        // Query a time range that doesn't contain the bucket
        let results = rollup.get_rollups(0, 120_000_000_000, 180_000_000_000, None);
        assert!(results.is_empty());
    }

    #[test]
    fn test_rollup_storage_get_rollups_invalid_level() {
        let levels = vec![RollupLevel {
            interval_ns: 60_000_000_000,
            retention_ns: 3600_000_000_000,
        }];
        let rollup = RollupStorage::new(levels);

        // Level 5 doesn't exist (only level 0)
        let results = rollup.get_rollups(5, 0, u64::MAX, None);
        assert!(results.is_empty());

        let hist_results = rollup.get_histogram_rollups(5, 0, u64::MAX, None);
        assert!(hist_results.is_empty());
    }

    #[test]
    fn test_rollup_storage_get_rollups_filter_by_name() {
        let levels = vec![RollupLevel {
            interval_ns: 60_000_000_000,
            retention_ns: 3600_000_000_000,
        }];
        let rollup = RollupStorage::new(levels);

        let metrics = vec![
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(42.0, 1_000_000_000)],
                vec![],
            ),
            make_number_metric(
                "mem",
                "svc",
                StoredMetricType::Gauge,
                &[(1024.0, 1_000_000_000)],
                vec![],
            ),
        ];
        rollup.process_metrics(&metrics);

        let cpu_results = rollup.get_rollups(0, 0, 60_000_000_000, Some("cpu"));
        assert_eq!(cpu_results.len(), 1);
        assert_eq!(cpu_results[0].name, "cpu");

        let all_results = rollup.get_rollups(0, 0, 60_000_000_000, None);
        assert_eq!(all_results.len(), 2);
    }

    #[test]
    fn test_rollup_storage_apply_retention() {
        let levels = vec![RollupLevel {
            interval_ns: 60_000_000_000, // 1 minute
            retention_ns: 1_000_000_000, // 1 second retention (very short)
        }];
        let rollup = RollupStorage::new(levels);

        // Add a metric from a very old timestamp
        let metrics = vec![make_number_metric(
            "old.metric",
            "svc",
            StoredMetricType::Gauge,
            &[(42.0, 1_000_000_000)], // 1 second since epoch
            vec![],
        )];
        rollup.process_metrics(&metrics);

        // Before retention
        let all = rollup.get_rollups(0, 0, u64::MAX, None);
        assert!(!all.is_empty());

        // Apply retention - the metric's bucket_start is ancient, should be removed
        rollup.apply_retention();

        let remaining = rollup.get_rollups(0, 0, u64::MAX, None);
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_rollup_storage_multiple_levels() {
        let levels = vec![
            RollupLevel {
                interval_ns: 10_000_000_000,    // 10 seconds
                retention_ns: 3600_000_000_000, // 1 hour
            },
            RollupLevel {
                interval_ns: 60_000_000_000,     // 1 minute
                retention_ns: 86400_000_000_000, // 1 day
            },
        ];
        let rollup = RollupStorage::new(levels);

        let metrics = vec![
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(10.0, 5_000_000_000)],
                vec![],
            ),
            make_number_metric(
                "cpu",
                "svc",
                StoredMetricType::Gauge,
                &[(20.0, 15_000_000_000)],
                vec![],
            ),
        ];
        rollup.process_metrics(&metrics);

        // Level 0 (10s interval): both metrics in different buckets
        let level0 = rollup.get_rollups(0, 0, 60_000_000_000, None);
        assert_eq!(level0.len(), 2);

        // Level 1 (60s interval): both metrics in the same bucket
        let level1 = rollup.get_rollups(1, 0, 60_000_000_000, None);
        assert_eq!(level1.len(), 1);
        assert_eq!(level1[0].count, 2);
    }

    #[test]
    fn test_rollup_storage_debug() {
        let levels = vec![RollupLevel {
            interval_ns: 60_000_000_000,
            retention_ns: 3600_000_000_000,
        }];
        let rollup = RollupStorage::new(levels);
        let debug_str = format!("{:?}", rollup);
        assert!(debug_str.contains("RollupStorage"));
        assert!(debug_str.contains("levels"));
    }

    // =========================================================================
    // MetricsAccumulator tests
    // =========================================================================

    #[test]
    fn test_metrics_accumulator_get_function_count() {
        let acc = MetricsAccumulator::default();
        assert!(acc.get_function_count("unknown").is_none());

        acc.increment_function("func_a");
        acc.increment_function("func_a");
        acc.increment_function("func_b");

        assert_eq!(acc.get_function_count("func_a"), Some(2));
        assert_eq!(acc.get_function_count("func_b"), Some(1));
        assert!(acc.get_function_count("func_c").is_none());
    }

    #[test]
    fn test_metrics_accumulator_iter_function_counts() {
        let acc = MetricsAccumulator::default();

        acc.increment_function("func_x");
        acc.increment_function("func_x");
        acc.increment_function("func_y");

        let counts: std::collections::HashMap<String, u64> = acc.iter_function_counts().collect();

        assert_eq!(counts.get("func_x"), Some(&2));
        assert_eq!(counts.get("func_y"), Some(&1));
    }

    // =========================================================================
    // TimeIndexedMetricStorage len / is_empty
    // =========================================================================

    #[test]
    fn test_metric_storage_len_and_is_empty() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);

        storage.add_metrics(vec![make_number_metric(
            "cpu",
            "svc",
            StoredMetricType::Gauge,
            &[(10.0, 1000)],
            vec![],
        )]);
        assert!(!storage.is_empty());
        assert_eq!(storage.len(), 1);

        storage.clear();
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);
    }

    // =========================================================================
    // TimeIndexedMetricStorage Debug impl
    // =========================================================================

    #[test]
    fn test_metric_storage_debug() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);
        let debug_str = format!("{:?}", storage);
        assert!(debug_str.contains("TimeIndexedMetricStorage"));
        assert!(debug_str.contains("max_age_ns"));
        assert!(debug_str.contains("max_metrics"));
    }

    // =========================================================================
    // AggregatedMetric struct validation
    // =========================================================================

    #[test]
    fn test_aggregated_metric_serialization() {
        let am = AggregatedMetric {
            name: "test".to_string(),
            bucket_start_ns: 1000,
            bucket_end_ns: 2000,
            count: 5,
            sum: 100.0,
            min: 10.0,
            max: 30.0,
            avg: 20.0,
            percentiles: Some(PercentileValues {
                p50: 15.0,
                p75: 22.0,
                p90: 27.0,
                p95: 29.0,
                p99: 30.0,
            }),
        };

        let json = serde_json::to_string(&am).expect("serialize");
        assert!(json.contains("\"name\":\"test\""));
        assert!(json.contains("\"percentiles\""));
    }

    #[test]
    fn test_aggregated_metric_without_percentiles_serialization() {
        let am = AggregatedMetric {
            name: "test".to_string(),
            bucket_start_ns: 1000,
            bucket_end_ns: 2000,
            count: 2,
            sum: 30.0,
            min: 10.0,
            max: 20.0,
            avg: 15.0,
            percentiles: None,
        };

        let json = serde_json::to_string(&am).expect("serialize");
        assert!(json.contains("\"name\":\"test\""));
        // percentiles should be omitted when None
        assert!(!json.contains("percentiles"));
    }

    #[test]
    fn test_shutdown_metrics_is_safe_after_default_meter_init() {
        ensure_default_meter();
        shutdown_metrics();
        assert!(get_meter().is_some());
    }

    #[test]
    fn test_rollup_storage_get_rollups_empty_for_unknown_metric_name() {
        let storage = RollupStorage::new(default_rollup_levels());
        let metrics = vec![make_number_metric(
            "cpu",
            "svc",
            StoredMetricType::Gauge,
            &[(42.0, 1_000_000_000)],
            vec![],
        )];

        storage.process_metrics(&metrics);

        let results = storage.get_rollups(0, 0, u64::MAX, Some("missing"));
        assert!(results.is_empty());
    }

    #[test]
    fn test_get_worker_metrics_from_storage_returns_none_for_unknown_worker() {
        init_metric_storage(Some(100), Some(3600));
        let storage = get_metric_storage().expect("metric storage should be initialized");
        storage.clear();

        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        storage.add_metrics(vec![make_number_metric(
            "iii.worker.cpu.percent",
            "svc",
            StoredMetricType::Gauge,
            &[(7.5, now_ns)],
            vec![("worker.id".to_string(), "worker-a".to_string())],
        )]);

        assert!(get_worker_metrics_from_storage("worker-b").is_none());
    }

    #[tokio::test]
    async fn test_alert_manager_no_rules_emits_no_events() {
        let manager = AlertManager::new(Vec::new());
        let events = manager.evaluate().await;
        assert!(events.is_empty());
    }

    #[test]
    fn test_get_aggregated_metrics_ignores_histogram_datapoints() {
        let storage = InMemoryMetricStorage::new(100, DEFAULT_RETENTION_NS);
        storage.add_metrics(vec![make_histogram_metric(
            "request.duration",
            "svc",
            3,
            12.0,
            vec![1, 2],
            vec![5.0, 10.0],
            Some(1.0),
            Some(10.0),
            1_000_000_000,
        )]);

        let aggregated = storage.get_aggregated_metrics(0, u64::MAX, 60_000_000_000);
        assert!(aggregated.is_empty());
    }
}
