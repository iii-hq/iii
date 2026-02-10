// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::Deserialize;

/// Exporter type for OpenTelemetry traces (for YAML deserialization)
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OtelExporterType {
    /// Export traces via OTLP/gRPC to a collector
    #[default]
    Otlp,
    /// Store traces in memory (queryable via API)
    Memory,
    /// Export traces via OTLP and store in memory (enables triggers with OTLP export)
    Both,
}

/// Exporter type for OpenTelemetry metrics (for YAML deserialization)
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MetricsExporterType {
    /// Store metrics in memory only (queryable via metrics.list API)
    #[default]
    Memory,
    /// Export metrics via OTLP/gRPC to a collector
    Otlp,
}

/// Exporter type for OpenTelemetry logs (for YAML deserialization)
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LogsExporterType {
    /// Store logs in memory only (queryable via logs.list API)
    #[default]
    Memory,
    /// Export logs via OTLP/gRPC to a collector
    Otlp,
    /// Export logs via OTLP and store in memory
    Both,
}

/// Comparison operator for alert thresholds
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AlertOperator {
    #[default]
    #[serde(alias = ">")]
    GreaterThan,
    #[serde(alias = ">=")]
    GreaterThanOrEqual,
    #[serde(alias = "<")]
    LessThan,
    #[serde(alias = "<=")]
    LessThanOrEqual,
    #[serde(alias = "==")]
    Equal,
    #[serde(alias = "!=")]
    NotEqual,
}

impl AlertOperator {
    pub fn evaluate(&self, value: f64, threshold: f64) -> bool {
        match self {
            AlertOperator::GreaterThan => value > threshold,
            AlertOperator::GreaterThanOrEqual => value >= threshold,
            AlertOperator::LessThan => value < threshold,
            AlertOperator::LessThanOrEqual => value <= threshold,
            AlertOperator::Equal => (value - threshold).abs() < f64::EPSILON,
            AlertOperator::NotEqual => (value - threshold).abs() >= f64::EPSILON,
        }
    }
}

/// Alert action type
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AlertAction {
    /// Log the alert (default)
    #[default]
    Log,
    /// Send webhook notification to the specified URL
    Webhook {
        /// The webhook URL to send the alert to
        url: String,
    },
    /// Invoke a function at the specified path
    Function {
        /// The function path to invoke
        path: String,
    },
}

/// Single alert rule configuration
#[derive(Debug, Clone, Deserialize)]
pub struct AlertRule {
    /// Name of the alert (for identification)
    pub name: String,

    /// Metric name to monitor (e.g., "iii.invocations.error")
    pub metric: String,

    /// Threshold value for the alert
    pub threshold: f64,

    /// Comparison operator (>, >=, <, <=, ==, !=)
    #[serde(default)]
    pub operator: AlertOperator,

    /// Time window in seconds to evaluate the metric (default: 60)
    #[serde(default = "default_alert_window")]
    pub window_seconds: u64,

    /// Action to take when alert triggers (Log, Webhook { url }, Function { path })
    #[serde(default)]
    pub action: AlertAction,

    /// Whether the alert is enabled (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Minimum interval between alert triggers in seconds (default: 60)
    #[serde(default = "default_alert_cooldown")]
    pub cooldown_seconds: u64,
}

fn default_alert_window() -> u64 {
    60
}

fn default_alert_cooldown() -> u64 {
    60
}

fn default_true() -> bool {
    true
}

/// Sampling rule for per-operation or per-service sampling
#[derive(Debug, Clone, Deserialize)]
pub struct SamplingRule {
    /// Operation name pattern (supports wildcards like "api.*")
    #[serde(default)]
    pub operation: Option<String>,

    /// Service name pattern
    #[serde(default)]
    pub service: Option<String>,

    /// Sampling rate for this rule (0.0 to 1.0)
    pub rate: f64,
}

/// Advanced sampling configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SamplingConfig {
    /// Default sampling ratio for traces not matching any rule
    #[serde(default)]
    pub default: Option<f64>,

    /// List of sampling rules (evaluated in order)
    #[serde(default)]
    pub rules: Vec<SamplingRule>,

    /// Enable parent-based sampling (inherit sampling decision from parent)
    #[serde(default)]
    pub parent_based: Option<bool>,

    /// Rate limiting configuration
    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,
}

/// Rate limiting configuration for trace sampling
#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum traces per second
    pub max_traces_per_second: u32,
}

/// OpenTelemetry module configuration (for YAML deserialization)
#[derive(Debug, Clone, Deserialize, Default)]
pub struct OtelModuleConfig {
    /// Whether OpenTelemetry export is enabled
    #[serde(default)]
    pub enabled: Option<bool>,

    /// The service name to report
    #[serde(default)]
    pub service_name: Option<String>,

    /// The service version to report (OTEL semantic convention: service.version)
    #[serde(default)]
    pub service_version: Option<String>,

    /// The service namespace to report (OTEL semantic convention: service.namespace)
    #[serde(default)]
    pub service_namespace: Option<String>,

    /// Exporter type: "otlp", "memory", or "both"
    #[serde(default)]
    pub exporter: Option<OtelExporterType>,

    /// OTLP endpoint (used when exporter is "otlp" or "both")
    #[serde(default)]
    pub endpoint: Option<String>,

    /// Sampling ratio (0.0 to 1.0). 1.0 means sample everything
    #[serde(default)]
    pub sampling_ratio: Option<f64>,

    /// Advanced sampling configuration
    #[serde(default)]
    pub sampling: Option<SamplingConfig>,

    /// Maximum spans to keep in memory (used when exporter is "memory" or "both")
    #[serde(default)]
    pub memory_max_spans: Option<usize>,

    /// Whether OpenTelemetry metrics export is enabled
    #[serde(default)]
    pub metrics_enabled: Option<bool>,

    /// Metrics exporter type: "memory" or "otlp"
    #[serde(default)]
    pub metrics_exporter: Option<MetricsExporterType>,

    /// Metrics retention period in seconds (default: 3600 = 1 hour)
    #[serde(default)]
    pub metrics_retention_seconds: Option<u64>,

    /// Maximum number of metrics to keep in memory (default: 10000)
    #[serde(default)]
    pub metrics_max_count: Option<usize>,

    /// Whether OTEL logs storage is enabled (default: true)
    #[serde(default)]
    pub logs_enabled: Option<bool>,

    /// Logs exporter type: "memory", "otlp", or "both"
    #[serde(default)]
    pub logs_exporter: Option<LogsExporterType>,

    /// Maximum number of logs to keep in memory (default: 1000)
    #[serde(default)]
    pub logs_max_count: Option<usize>,

    /// Logs retention period in seconds (default: 3600 = 1 hour)
    #[serde(default)]
    pub logs_retention_seconds: Option<u64>,

    /// Sampling ratio for logs (0.0 to 1.0). 1.0 means keep all logs.
    #[serde(default = "default_logs_sampling_ratio")]
    pub logs_sampling_ratio: f64,

    /// Whether to output ingested OTEL logs to the console via tracing (default: true)
    #[serde(default = "default_logs_console_output")]
    pub logs_console_output: bool,

    /// Alert rules for metric thresholds
    #[serde(default)]
    pub alerts: Vec<AlertRule>,

    /// Log level for the engine (e.g., "info", "debug", "warn", "error", "trace")
    #[serde(default)]
    pub level: Option<String>,

    /// Log format: "default" for human-readable, "json" for structured JSON
    #[serde(default)]
    pub format: Option<String>,
}

fn default_logs_sampling_ratio() -> f64 {
    1.0 // Keep all logs by default
}

fn default_logs_console_output() -> bool {
    true
}
