// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Exporter type for OpenTelemetry traces (for YAML deserialization)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default, PartialEq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MetricsExporterType {
    /// Store metrics in memory only (queryable via metrics.list API)
    #[default]
    Memory,
    /// Export metrics via OTLP/gRPC to a collector
    Otlp,
}

/// Exporter type for OpenTelemetry logs (for YAML deserialization)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default, PartialEq)]
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
/// NOTE: the symbol aliases (`>`, `>=`, ...) are serde-only conveniences for
/// config.yaml. The generated JSON Schema advertises the canonical lowercase
/// names, and `configuration::set` validates against the schema — so remote
/// edits must use the canonical names.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default, PartialEq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default, PartialEq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
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
    #[schemars(range(min = 1))]
    pub window_seconds: u64,

    /// Action to take when alert triggers (Log, Webhook { url }, Function { path })
    #[serde(default)]
    pub action: AlertAction,

    /// Whether the alert is enabled (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Minimum interval between alert triggers in seconds (default: 60)
    #[serde(default = "default_alert_cooldown")]
    #[schemars(range(min = 1))]
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SamplingRule {
    /// Operation name pattern (supports wildcards like "api.*")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,

    /// Service name pattern
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    /// Sampling rate for this rule (0.0 to 1.0)
    #[schemars(range(min = 0.0, max = 1.0))]
    pub rate: f64,
}

/// Rule for collapsing spans in the trace-tree view. Matching spans are
/// removed and their children reparented to the nearest surviving ancestor,
/// so the tree stays connected. Use to hide redundant pass-through wrapper
/// spans (e.g. an SDK handler wrapper that duplicates the engine's invocation
/// span) without changing worker code.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SpanCollapseRule {
    /// Span name pattern (supports wildcards like `trigger *`). Required.
    pub name: String,

    /// Optional `service.name` pattern. When set, only spans whose service
    /// matches are collapsed — disambiguates same-named spans across services
    /// (e.g. a worker `trigger *` vs the engine's own `trigger *`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

/// Advanced sampling configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SamplingConfig {
    /// Default sampling ratio for traces not matching any rule
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub default: Option<f64>,

    /// List of sampling rules (evaluated in order)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<SamplingRule>,

    /// Enable parent-based sampling (inherit sampling decision from parent)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_based: Option<bool>,

    /// Rate limiting configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfig>,
}

/// Rate limiting configuration for trace sampling
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RateLimitConfig {
    /// Maximum traces per second
    #[schemars(range(min = 1))]
    pub max_traces_per_second: u32,
}

/// OpenTelemetry module configuration (for YAML deserialization)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ObservabilityWorkerConfig {
    /// Whether OpenTelemetry export is enabled
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// The service name to report
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,

    /// The service version to report (OTEL semantic convention: service.version)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_version: Option<String>,

    /// The service namespace to report (OTEL semantic convention: service.namespace)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_namespace: Option<String>,

    /// Exporter type: "otlp", "memory", or "both"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exporter: Option<OtelExporterType>,

    /// OTLP endpoint (used when exporter is "otlp" or "both")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,

    /// Sampling ratio (0.0 to 1.0). 1.0 means sample everything
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub sampling_ratio: Option<f64>,

    /// Advanced sampling configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling: Option<SamplingConfig>,

    /// Maximum spans to keep in memory (used when exporter is "memory" or "both")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub memory_max_spans: Option<usize>,

    /// Mirror spans into the in-memory store at start as `pending` snapshots
    /// so live trace views show in-progress work. Defaults to on for the
    /// "memory" exporter (local dev) and off otherwise — set `true` to opt in
    /// with "both" in production. Ignored for "otlp"-only (no in-memory
    /// finals to replace pendings); never affects OTLP output either way.
    /// Restart-tier: the span processor set is fixed at provider build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_spans: Option<bool>,

    /// Whether OpenTelemetry metrics export is enabled
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_enabled: Option<bool>,

    /// Metrics exporter type: "memory" or "otlp"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_exporter: Option<MetricsExporterType>,

    /// Metrics retention period in seconds (default: 3600 = 1 hour)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub metrics_retention_seconds: Option<u64>,

    /// Maximum number of metrics to keep in memory (default: 10000)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub metrics_max_count: Option<usize>,

    /// Whether OTEL logs storage is enabled (default: true)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logs_enabled: Option<bool>,

    /// Logs exporter type: "memory", "otlp", or "both"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logs_exporter: Option<LogsExporterType>,

    /// Maximum number of logs to keep in memory (default: 1000)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub logs_max_count: Option<usize>,

    /// Logs retention period in seconds (default: 3600 = 1 hour)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub logs_retention_seconds: Option<u64>,

    /// Batch size for OTLP logs export (default: 100)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 10_000))]
    pub logs_batch_size: Option<usize>,

    /// Flush interval in milliseconds for OTLP logs export (default: 5000)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 100, max = 3_600_000))]
    pub logs_flush_interval_ms: Option<u64>,

    /// Sampling ratio for logs (0.0 to 1.0). 1.0 means keep all logs.
    #[serde(default = "default_logs_sampling_ratio")]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub logs_sampling_ratio: f64,

    /// Whether to output ingested OTEL logs to the console via tracing (default: true)
    #[serde(default = "default_logs_console_output")]
    pub logs_console_output: bool,

    /// Alert rules for metric thresholds
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<AlertRule>,

    /// Log level for the engine (e.g., "info", "debug", "warn", "error", "trace")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,

    /// Log format: "default" for human-readable, "json" for structured JSON
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,

    /// Trace-view span collapse rules. Matching spans are hidden from the
    /// trace tree and their children reparented to the nearest surviving
    /// ancestor. Hides redundant pass-through wrapper spans without touching
    /// worker code. Empty by default (no collapsing).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collapse_spans: Vec<SpanCollapseRule>,
}

impl Default for ObservabilityWorkerConfig {
    fn default() -> Self {
        Self {
            enabled: Some(true),
            service_name: Some("iii".to_string()),
            exporter: Some(OtelExporterType::Memory),
            metrics_enabled: Some(true),
            metrics_exporter: Some(MetricsExporterType::Memory),
            logs_enabled: Some(true),
            logs_exporter: Some(LogsExporterType::Memory),
            logs_console_output: true,
            logs_sampling_ratio: 1.0,
            // All other fields left as None/empty — resolved at runtime
            service_version: None,
            service_namespace: None,
            endpoint: None,
            sampling_ratio: None,
            sampling: None,
            memory_max_spans: None,
            live_spans: None,
            metrics_retention_seconds: None,
            metrics_max_count: None,
            logs_max_count: None,
            logs_retention_seconds: None,
            logs_batch_size: None,
            logs_flush_interval_ms: None,
            alerts: Vec::new(),
            level: None,
            format: None,
            collapse_spans: Vec::new(),
        }
    }
}

impl ObservabilityWorkerConfig {
    /// Clamp out-of-range top-level values into safe bounds.
    ///
    /// The JSON Schema rejects out-of-range values at `configuration::set`
    /// time, but stored entries can predate a schema tightening or be
    /// hand-edited on disk (`./config/iii-observability.yaml`),
    /// so the top-level ratio and count fields are re-normalized on every
    /// read: ratios are clamped into `0..=1` and zero counts fall back to the
    /// built-in defaults (`None`) rather than creating zero-capacity stores.
    /// Nested alert-rule and rate-limit bounds are not re-clamped here — they
    /// are enforced by the schema at set time and guarded at their use sites.
    pub fn normalized(mut self) -> Self {
        fn clamp_ratio(v: f64) -> f64 {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                1.0
            }
        }
        fn nonzero_usize(v: Option<usize>) -> Option<usize> {
            v.filter(|&n| n > 0)
        }
        fn nonzero_u64(v: Option<u64>) -> Option<u64> {
            v.filter(|&n| n > 0)
        }

        self.sampling_ratio = self.sampling_ratio.map(clamp_ratio);
        self.logs_sampling_ratio = clamp_ratio(self.logs_sampling_ratio);
        if let Some(sampling) = &mut self.sampling {
            sampling.default = sampling.default.map(clamp_ratio);
            for rule in &mut sampling.rules {
                rule.rate = clamp_ratio(rule.rate);
            }
        }
        self.memory_max_spans = nonzero_usize(self.memory_max_spans);
        self.metrics_max_count = nonzero_usize(self.metrics_max_count);
        self.metrics_retention_seconds = nonzero_u64(self.metrics_retention_seconds);
        self.logs_max_count = nonzero_usize(self.logs_max_count);
        self.logs_retention_seconds = nonzero_u64(self.logs_retention_seconds);
        self.logs_batch_size = nonzero_usize(self.logs_batch_size);
        self.logs_flush_interval_ms = nonzero_u64(self.logs_flush_interval_ms);
        self
    }
}

fn default_logs_sampling_ratio() -> f64 {
    1.0 // Keep all logs by default
}

fn default_logs_console_output() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_operator_evaluates_all_variants() {
        assert!(AlertOperator::GreaterThan.evaluate(2.0, 1.0));
        assert!(AlertOperator::GreaterThanOrEqual.evaluate(2.0, 2.0));
        assert!(AlertOperator::LessThan.evaluate(1.0, 2.0));
        assert!(AlertOperator::LessThanOrEqual.evaluate(2.0, 2.0));
        assert!(AlertOperator::Equal.evaluate(1.0 + f64::EPSILON / 2.0, 1.0));
        assert!(AlertOperator::NotEqual.evaluate(2.0, 1.0));
    }

    #[test]
    fn alert_rule_and_otel_config_apply_defaults() {
        let rule: AlertRule = serde_json::from_value(serde_json::json!({
            "name": "high-errors",
            "metric": "iii.invocations.error",
            "threshold": 5.0
        }))
        .expect("deserialize alert rule");
        assert_eq!(rule.operator, AlertOperator::GreaterThan);
        assert_eq!(rule.window_seconds, 60);
        assert!(matches!(rule.action, AlertAction::Log));
        assert!(rule.enabled);
        assert_eq!(rule.cooldown_seconds, 60);

        let config: ObservabilityWorkerConfig = serde_json::from_value(serde_json::json!({
            "exporter": "both",
            "metrics_exporter": "otlp",
            "logs_exporter": "both",
            "alerts": [
                {
                    "name": "alert",
                    "metric": "iii.latency",
                    "threshold": 10.0,
                    "action": { "type": "function", "path": "alerts.notify" }
                }
            ]
        }))
        .expect("deserialize otel config");

        assert_eq!(config.exporter, Some(OtelExporterType::Both));
        assert_eq!(config.metrics_exporter, Some(MetricsExporterType::Otlp));
        assert_eq!(config.logs_exporter, Some(LogsExporterType::Both));
        assert_eq!(config.logs_sampling_ratio, 1.0);
        assert!(config.logs_console_output);
        assert_eq!(config.alerts.len(), 1);
        assert!(matches!(
            config.alerts[0].action,
            AlertAction::Function { ref path } if path == "alerts.notify"
        ));
    }

    #[test]
    fn otel_config_deny_unknown_fields() {
        let json = r#"{"enabled": true, "fake_key": "value"}"#;
        let result: Result<ObservabilityWorkerConfig, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "should reject unknown fields in ObservabilityWorkerConfig"
        );
    }

    #[test]
    fn alert_rule_deny_unknown_fields() {
        let json = serde_json::json!({
            "name": "test",
            "metric": "m",
            "threshold": 1.0,
            "fake_key": true
        });
        let result: Result<AlertRule, _> = serde_json::from_value(json);
        assert!(result.is_err(), "should reject unknown fields in AlertRule");
    }

    #[test]
    fn sampling_config_deny_unknown_fields() {
        let json = r#"{"default": 0.5, "fake_key": true}"#;
        let result: Result<SamplingConfig, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "should reject unknown fields in SamplingConfig"
        );
    }

    #[test]
    fn rate_limit_config_deny_unknown_fields() {
        let json = r#"{"max_traces_per_second": 100, "fake_key": true}"#;
        let result: Result<RateLimitConfig, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "should reject unknown fields in RateLimitConfig"
        );
    }

    #[test]
    fn schema_contains_all_fields_and_rejects_unknown() {
        let schema = serde_json::to_value(schemars::schema_for!(ObservabilityWorkerConfig))
            .expect("schema serializes");
        let props = schema["properties"].as_object().expect("properties");
        for field in [
            "enabled",
            "service_name",
            "service_version",
            "service_namespace",
            "exporter",
            "endpoint",
            "sampling_ratio",
            "sampling",
            "memory_max_spans",
            "live_spans",
            "metrics_enabled",
            "metrics_exporter",
            "metrics_retention_seconds",
            "metrics_max_count",
            "logs_enabled",
            "logs_exporter",
            "logs_max_count",
            "logs_retention_seconds",
            "logs_batch_size",
            "logs_flush_interval_ms",
            "logs_sampling_ratio",
            "logs_console_output",
            "alerts",
            "level",
            "format",
            "collapse_spans",
        ] {
            assert!(props.contains_key(field), "schema missing `{field}`");
        }
        assert_eq!(
            schema["additionalProperties"],
            serde_json::Value::Bool(false),
            "deny_unknown_fields must surface as additionalProperties: false"
        );
    }

    #[test]
    fn schema_has_range_constraints() {
        let schema = serde_json::to_value(schemars::schema_for!(ObservabilityWorkerConfig))
            .expect("schema serializes");
        let props = &schema["properties"];
        assert_eq!(props["sampling_ratio"]["minimum"], 0.0);
        assert_eq!(props["sampling_ratio"]["maximum"], 1.0);
        assert_eq!(props["logs_sampling_ratio"]["minimum"], 0.0);
        assert_eq!(props["logs_sampling_ratio"]["maximum"], 1.0);
        assert_eq!(props["memory_max_spans"]["minimum"], 1.0);
        assert_eq!(props["logs_batch_size"]["minimum"], 1.0);
        assert_eq!(props["logs_batch_size"]["maximum"], 10000.0);
        assert_eq!(props["metrics_max_count"]["minimum"], 1.0);
        // Log flush/batch bounds must mirror the runtime's accepted range so
        // configuration::set rejects values the exporter would silently drop.
        assert_eq!(props["logs_flush_interval_ms"]["minimum"], 100.0);
        assert_eq!(props["logs_flush_interval_ms"]["maximum"], 3600000.0);
    }

    #[test]
    fn schema_alert_operator_lists_canonical_names_only() {
        // serde aliases (">", ">=", ...) are accepted from config.yaml but are
        // NOT advertised in the schema — configuration::set must use the
        // canonical lowercase names.
        let schema =
            serde_json::to_value(schemars::schema_for!(AlertOperator)).expect("schema serializes");
        let variants = schema["enum"].as_array().expect("enum variants");
        assert!(variants.contains(&serde_json::json!("greaterthan")));
        assert!(!variants.contains(&serde_json::json!(">")));
    }

    #[test]
    fn schema_alert_action_is_internally_tagged() {
        let schema =
            serde_json::to_value(schemars::schema_for!(AlertAction)).expect("schema serializes");
        let rendered = schema.to_string();
        assert!(
            rendered.contains("\"type\""),
            "AlertAction schema must carry the internal tag: {rendered}"
        );
    }

    #[test]
    fn seed_serialization_skips_unset_fields() {
        let value = serde_json::to_value(ObservabilityWorkerConfig::default()).unwrap();
        let obj = value.as_object().unwrap();
        assert!(
            !obj.contains_key("endpoint"),
            "None fields must not serialize"
        );
        assert!(!obj.contains_key("alerts"), "empty vecs must not serialize");
        assert!(obj.contains_key("enabled"));
        // Round-trips through the schema validator (all serialized fields valid).
        let parsed: ObservabilityWorkerConfig = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, ObservabilityWorkerConfig::default());
    }

    #[test]
    fn normalized_clamps_ratios_and_zero_limits() {
        let config = ObservabilityWorkerConfig {
            sampling_ratio: Some(7.5),
            logs_sampling_ratio: -1.0,
            memory_max_spans: Some(0),
            logs_batch_size: Some(0),
            metrics_max_count: Some(0),
            metrics_retention_seconds: Some(0),
            logs_max_count: Some(0),
            logs_retention_seconds: Some(0),
            logs_flush_interval_ms: Some(0),
            sampling: Some(SamplingConfig {
                default: Some(2.0),
                rules: vec![SamplingRule {
                    operation: Some("api.*".into()),
                    service: None,
                    rate: -3.0,
                }],
                parent_based: None,
                rate_limit: None,
            }),
            ..ObservabilityWorkerConfig::default()
        }
        .normalized();

        assert_eq!(config.sampling_ratio, Some(1.0));
        assert_eq!(config.logs_sampling_ratio, 0.0);
        assert_eq!(config.memory_max_spans, None);
        assert_eq!(config.logs_batch_size, None);
        assert_eq!(config.metrics_max_count, None);
        assert_eq!(config.metrics_retention_seconds, None);
        assert_eq!(config.logs_max_count, None);
        assert_eq!(config.logs_retention_seconds, None);
        assert_eq!(config.logs_flush_interval_ms, None);
        let sampling = config.sampling.unwrap();
        assert_eq!(sampling.default, Some(1.0));
        assert_eq!(sampling.rules[0].rate, 0.0);
    }
}
