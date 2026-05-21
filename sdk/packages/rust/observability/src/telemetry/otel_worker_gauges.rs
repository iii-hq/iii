use opentelemetry::KeyValue;
use opentelemetry::metrics::Meter;
use std::sync::Arc;

use super::worker_metrics::WorkerMetricsCollector;

/// Options for registering worker gauges
pub struct WorkerGaugesOptions {
    pub worker_id: String,
    pub worker_name: Option<String>,
}

/// Register observable gauges for worker metrics with the given meter.
///
/// Returns a handle that keeps the gauges alive. Drop it to stop reporting.
pub fn register_worker_gauges(
    meter: &Meter,
    collector: Arc<WorkerMetricsCollector>,
    options: WorkerGaugesOptions,
) -> WorkerGaugesHandle {
    let mut attrs = vec![KeyValue::new("worker.id", options.worker_id)];
    if let Some(name) = options.worker_name {
        attrs.push(KeyValue::new("worker.name", name));
    }
    let attrs = Arc::new(attrs);

    let c = collector.clone();
    let a = attrs.clone();
    let memory_rss = meter
        .u64_observable_gauge("iii.worker.memory.rss")
        .with_description("Resident set size in bytes")
        .with_unit("By")
        .with_callback(move |observer| {
            let metrics = c.collect_cached();
            observer.observe(metrics.memory_rss, &a);
        })
        .build();

    let c = collector.clone();
    let a = attrs.clone();
    let memory_virtual = meter
        .u64_observable_gauge("iii.worker.memory.virtual")
        .with_description("Virtual memory in bytes")
        .with_unit("By")
        .with_callback(move |observer| {
            let metrics = c.collect_cached();
            observer.observe(metrics.memory_virtual, &a);
        })
        .build();

    let c = collector.clone();
    let a = attrs.clone();
    let cpu_percent = meter
        .f64_observable_gauge("iii.worker.cpu.percent")
        .with_description("CPU usage percentage")
        .with_unit("%")
        .with_callback(move |observer| {
            let metrics = c.collect_cached();
            observer.observe(metrics.cpu_percent as f64, &a);
        })
        .build();

    let c = collector.clone();
    let a = attrs.clone();
    let uptime = meter
        .f64_observable_gauge("iii.worker.uptime_seconds")
        .with_description("Worker uptime in seconds")
        .with_unit("s")
        .with_callback(move |observer| {
            let metrics = c.collect_cached();
            observer.observe(metrics.uptime_seconds, &a);
        })
        .build();

    WorkerGaugesHandle {
        _memory_rss: memory_rss,
        _memory_virtual: memory_virtual,
        _cpu_percent: cpu_percent,
        _uptime: uptime,
    }
}

/// Handle that keeps the OTEL gauges alive. Drop to stop reporting.
pub struct WorkerGaugesHandle {
    _memory_rss: opentelemetry::metrics::ObservableGauge<u64>,
    _memory_virtual: opentelemetry::metrics::ObservableGauge<u64>,
    _cpu_percent: opentelemetry::metrics::ObservableGauge<f64>,
    _uptime: opentelemetry::metrics::ObservableGauge<f64>,
}
