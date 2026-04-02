use super::connection::SharedEngineConnection;
use super::json_serializer::{attrs_to_json, resource_attrs_to_json, system_time_to_nanos_string};
use super::types::PREFIX_METRICS;
use opentelemetry::KeyValue;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::metrics::Temporality;
use opentelemetry_sdk::metrics::data::{
    AggregatedMetrics, Gauge, Histogram, Metric, MetricData, ResourceMetrics, ScopeMetrics, Sum,
};
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use serde_json::{Value as JsonValue, json};
use std::fmt;
use std::sync::Arc;
use std::time::SystemTime;

/// Custom metrics exporter that sends OTLP JSON over a shared WebSocket connection.
///
/// Uses a hand-built JSON serializer to match the III Engine's expected format.
pub struct EngineMetricsExporter {
    connection: Arc<SharedEngineConnection>,
}

impl EngineMetricsExporter {
    pub fn new(connection: Arc<SharedEngineConnection>) -> Self {
        Self { connection }
    }
}

impl fmt::Debug for EngineMetricsExporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineMetricsExporter").finish()
    }
}

fn attrs_vec(iter: impl Iterator<Item = impl std::borrow::Borrow<KeyValue>>) -> Vec<KeyValue> {
    iter.map(|kv| kv.borrow().clone()).collect()
}

fn time_nanos(t: SystemTime) -> String {
    system_time_to_nanos_string(t)
}

fn opt_time_nanos(t: Option<SystemTime>) -> String {
    t.map(system_time_to_nanos_string)
        .unwrap_or_else(|| "0".to_string())
}

fn serialize_sum_f64(metric: &Metric, sum: &Sum<f64>) -> JsonValue {
    let data_points: Vec<JsonValue> = sum
        .data_points()
        .map(|dp| {
            let a = attrs_vec(dp.attributes());
            let mut point = json!({
                "attributes": attrs_to_json(&a),
                "startTimeUnixNano": time_nanos(sum.start_time()),
                "timeUnixNano": time_nanos(sum.time()),
                "asDouble": dp.value(),
            });
            if dp.exemplars().next().is_some() {
                point.as_object_mut().unwrap().insert(
                    "exemplars".to_string(),
                    json!(dp.exemplars().map(|_| json!({})).collect::<Vec<_>>()),
                );
            }
            point
        })
        .collect();
    json!({
        "name": metric.name(),
        "description": metric.description(),
        "unit": metric.unit(),
        "sum": {
            "dataPoints": data_points,
            "aggregationTemporality": temporality_value(sum.temporality()),
            "isMonotonic": sum.is_monotonic(),
        }
    })
}

fn serialize_sum_i64(metric: &Metric, sum: &Sum<i64>) -> JsonValue {
    let data_points: Vec<JsonValue> = sum
        .data_points()
        .map(|dp| {
            let a = attrs_vec(dp.attributes());
            json!({
                "attributes": attrs_to_json(&a),
                "startTimeUnixNano": time_nanos(sum.start_time()),
                "timeUnixNano": time_nanos(sum.time()),
                "asInt": dp.value().to_string(),
            })
        })
        .collect();
    json!({
        "name": metric.name(),
        "description": metric.description(),
        "unit": metric.unit(),
        "sum": {
            "dataPoints": data_points,
            "aggregationTemporality": temporality_value(sum.temporality()),
            "isMonotonic": sum.is_monotonic(),
        }
    })
}

fn serialize_gauge_f64(metric: &Metric, gauge: &Gauge<f64>) -> JsonValue {
    let data_points: Vec<JsonValue> = gauge
        .data_points()
        .map(|dp| {
            let a = attrs_vec(dp.attributes());
            json!({
                "attributes": attrs_to_json(&a),
                "startTimeUnixNano": opt_time_nanos(gauge.start_time()),
                "timeUnixNano": time_nanos(gauge.time()),
                "asDouble": dp.value(),
            })
        })
        .collect();
    json!({
        "name": metric.name(),
        "description": metric.description(),
        "unit": metric.unit(),
        "gauge": { "dataPoints": data_points }
    })
}

fn serialize_gauge_i64(metric: &Metric, gauge: &Gauge<i64>) -> JsonValue {
    let data_points: Vec<JsonValue> = gauge
        .data_points()
        .map(|dp| {
            let a = attrs_vec(dp.attributes());
            json!({
                "attributes": attrs_to_json(&a),
                "startTimeUnixNano": opt_time_nanos(gauge.start_time()),
                "timeUnixNano": time_nanos(gauge.time()),
                "asInt": dp.value().to_string(),
            })
        })
        .collect();
    json!({
        "name": metric.name(),
        "description": metric.description(),
        "unit": metric.unit(),
        "gauge": { "dataPoints": data_points }
    })
}

fn serialize_histogram_f64(metric: &Metric, hist: &Histogram<f64>) -> JsonValue {
    let data_points: Vec<JsonValue> = hist
        .data_points()
        .map(|dp| {
            let a = attrs_vec(dp.attributes());
            json!({
                "attributes": attrs_to_json(&a),
                "startTimeUnixNano": time_nanos(hist.start_time()),
                "timeUnixNano": time_nanos(hist.time()),
                "count": dp.count().to_string(),
                "sum": dp.sum(),
                "bucketCounts": dp.bucket_counts().map(|c: u64| c.to_string()).collect::<Vec<_>>(),
                "explicitBounds": dp.bounds().collect::<Vec<f64>>(),
                "min": dp.min(),
                "max": dp.max(),
            })
        })
        .collect();
    json!({
        "name": metric.name(),
        "description": metric.description(),
        "unit": metric.unit(),
        "histogram": {
            "dataPoints": data_points,
            "aggregationTemporality": temporality_value(hist.temporality()),
        }
    })
}

fn serialize_metric(metric: &Metric) -> JsonValue {
    match metric.data() {
        AggregatedMetrics::F64(data) => match data {
            MetricData::Sum(sum) => serialize_sum_f64(metric, sum),
            MetricData::Gauge(gauge) => serialize_gauge_f64(metric, gauge),
            MetricData::Histogram(hist) => serialize_histogram_f64(metric, hist),
            _ => metric_fallback(metric),
        },
        AggregatedMetrics::I64(data) => match data {
            MetricData::Sum(sum) => serialize_sum_i64(metric, sum),
            MetricData::Gauge(gauge) => serialize_gauge_i64(metric, gauge),
            _ => metric_fallback(metric),
        },
        AggregatedMetrics::U64(data) => match data {
            MetricData::Sum(sum) => {
                // Treat u64 sums as i64 for JSON serialization
                let data_points: Vec<JsonValue> = sum
                    .data_points()
                    .map(|dp| {
                        let a = attrs_vec(dp.attributes());
                        json!({
                            "attributes": attrs_to_json(&a),
                            "startTimeUnixNano": time_nanos(sum.start_time()),
                            "timeUnixNano": time_nanos(sum.time()),
                            "asInt": dp.value().to_string(),
                        })
                    })
                    .collect();
                json!({
                    "name": metric.name(),
                    "description": metric.description(),
                    "unit": metric.unit(),
                    "sum": {
                        "dataPoints": data_points,
                        "aggregationTemporality": temporality_value(sum.temporality()),
                        "isMonotonic": sum.is_monotonic(),
                    }
                })
            }
            MetricData::Gauge(gauge) => {
                let data_points: Vec<JsonValue> = gauge
                    .data_points()
                    .map(|dp| {
                        let a = attrs_vec(dp.attributes());
                        json!({
                            "attributes": attrs_to_json(&a),
                            "startTimeUnixNano": opt_time_nanos(gauge.start_time()),
                            "timeUnixNano": time_nanos(gauge.time()),
                            "asInt": dp.value().to_string(),
                        })
                    })
                    .collect();
                json!({
                    "name": metric.name(),
                    "description": metric.description(),
                    "unit": metric.unit(),
                    "gauge": { "dataPoints": data_points }
                })
            }
            _ => metric_fallback(metric),
        },
    }
}

fn metric_fallback(metric: &Metric) -> JsonValue {
    json!({
        "name": metric.name(),
        "description": metric.description(),
        "unit": metric.unit(),
    })
}

fn temporality_value(temporality: Temporality) -> u32 {
    match temporality {
        Temporality::Delta => 1,
        Temporality::Cumulative => 2,
        _ => 1,
    }
}

fn serialize_scope_metrics(
    scope_metrics: impl Iterator<Item = impl std::borrow::Borrow<ScopeMetrics>>,
) -> Vec<JsonValue> {
    scope_metrics
        .map(|sm| {
            let sm = sm.borrow();
            let metrics: Vec<JsonValue> = sm.metrics().map(serialize_metric).collect();
            json!({
                "scope": {
                    "name": sm.scope().name().to_string(),
                    "version": sm.scope().version().map(|v: &str| v.to_string()).unwrap_or_default(),
                },
                "metrics": metrics,
            })
        })
        .collect()
}

impl PushMetricExporter for EngineMetricsExporter {
    fn export(
        &self,
        metrics: &ResourceMetrics,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        let resource_attrs = resource_attrs_to_json(metrics.resource().iter());
        let scope_metrics = serialize_scope_metrics(metrics.scope_metrics());

        let result = json!({
            "resourceMetrics": [{
                "resource": { "attributes": resource_attrs },
                "scopeMetrics": scope_metrics,
            }]
        });

        let connection = self.connection.clone();
        async move {
            let json = serde_json::to_vec(&result).map_err(|e| {
                opentelemetry_sdk::error::OTelSdkError::InternalFailure(e.to_string())
            })?;
            connection
                .send(PREFIX_METRICS, json)
                .map_err(opentelemetry_sdk::error::OTelSdkError::InternalFailure)
        }
    }

    /// No-op: the synchronous PushMetricExporter trait cannot perform async I/O.
    /// Use `flush_otel()` for a full async flush of the connection layer.
    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _timeout: std::time::Duration) -> OTelSdkResult {
        Ok(())
    }

    fn temporality(&self) -> Temporality {
        Temporality::Cumulative
    }
}
