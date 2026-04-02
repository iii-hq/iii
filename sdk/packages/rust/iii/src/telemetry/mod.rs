pub mod connection;
pub mod context;
pub mod http_instrumentation;
pub mod json_serializer;
pub mod log_exporter;
pub mod metrics_exporter;
pub mod otel_worker_gauges;
pub mod span_exporter;
pub mod types;
pub mod worker_metrics;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use opentelemetry::propagation::TextMapCompositePropagator;
use opentelemetry::trace::{SpanKind, Status, TraceContextExt, Tracer};
use opentelemetry::{Context as OtelContext, KeyValue};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::{BatchConfigBuilder, BatchLogProcessor, SdkLoggerProvider};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::propagation::{BaggagePropagator, TraceContextPropagator};
use opentelemetry_sdk::trace::SdkTracerProvider;
use tokio::sync::Mutex;

use self::connection::SharedEngineConnection;
use self::log_exporter::EngineLogExporter;
use self::metrics_exporter::EngineMetricsExporter;
use self::span_exporter::EngineSpanExporter;
use self::types::OtelConfig;

/// Global OTEL state, initialized once via `init_otel`
struct OtelState {
    tracer_provider: SdkTracerProvider,
    meter_provider: Option<SdkMeterProvider>,
    logger_provider: Option<SdkLoggerProvider>,
    connection: Arc<SharedEngineConnection>,
    shutdown_timeout: std::time::Duration,
}

static OTEL_STATE: OnceLock<Mutex<Option<OtelState>>> = OnceLock::new();
static OTEL_INITIALIZED: AtomicBool = AtomicBool::new(false);

fn get_otel_lock() -> &'static Mutex<Option<OtelState>> {
    OTEL_STATE.get_or_init(|| Mutex::new(None))
}

/// Initialize OpenTelemetry with the given configuration.
///
/// Sets up distributed tracing, optional metrics, and optional log export
/// over a shared WebSocket connection to the III Engine.
///
/// This should be called once at startup. Subsequent calls are no-ops.
pub async fn init_otel(config: OtelConfig) {
    let lock = get_otel_lock();
    let mut state = lock.lock().await;

    if state.is_some() {
        tracing::warn!("OpenTelemetry already initialized, skipping");
        return;
    }

    let enabled = config.enabled.unwrap_or_else(|| {
        std::env::var("OTEL_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(true)
    });

    if !enabled {
        tracing::debug!("OpenTelemetry disabled, skipping initialization");
        return;
    }

    let service_name = config
        .service_name
        .or_else(|| std::env::var("OTEL_SERVICE_NAME").ok())
        .unwrap_or_else(|| "iii-rust-sdk".to_string());

    let service_version = config
        .service_version
        .or_else(|| std::env::var("SERVICE_VERSION").ok())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    let service_instance_id = config
        .service_instance_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let ws_url = config
        .engine_ws_url
        .or_else(|| std::env::var("III_URL").ok())
        .unwrap_or_else(|| "ws://localhost:49134".to_string());

    let reconnection_config = config.reconnection_config.unwrap_or_default();

    // Build resource
    let mut resource_attrs = vec![
        KeyValue::new("service.name", service_name.clone()),
        KeyValue::new("service.version", service_version),
        KeyValue::new("service.instance.id", service_instance_id),
        KeyValue::new("telemetry.sdk.name", "iii-rust-sdk"),
        KeyValue::new("telemetry.sdk.language", "rust"),
        KeyValue::new("telemetry.sdk.version", env!("CARGO_PKG_VERSION")),
    ];

    if let Some(ns) = config.service_namespace {
        resource_attrs.push(KeyValue::new("service.namespace", ns));
    }

    let resource = Resource::builder().with_attributes(resource_attrs).build();

    // Create shared WebSocket connection
    let channel_capacity = config.channel_capacity.unwrap_or(10_000);
    let connection = Arc::new(SharedEngineConnection::with_channel_capacity(
        ws_url,
        reconnection_config,
        channel_capacity,
    ));

    // Set up global propagator (W3C Trace Context + Baggage)
    let propagator = TextMapCompositePropagator::new(vec![
        Box::new(TraceContextPropagator::new()),
        Box::new(BaggagePropagator::new()),
    ]);
    opentelemetry::global::set_text_map_propagator(propagator);

    // Set up tracer provider with span exporter
    let span_exporter = EngineSpanExporter::new(connection.clone());
    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
        .build();

    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    // Set up metrics if enabled
    let meter_provider = if config.metrics_enabled.unwrap_or(true) {
        let metrics_exporter = EngineMetricsExporter::new(connection.clone());
        let interval_ms = config.metrics_export_interval_ms.unwrap_or(60_000);

        let reader = PeriodicReader::builder(metrics_exporter)
            .with_interval(std::time::Duration::from_millis(interval_ms))
            .build();

        let provider = SdkMeterProvider::builder()
            .with_resource(resource.clone())
            .with_reader(reader)
            .build();

        opentelemetry::global::set_meter_provider(provider.clone());
        Some(provider)
    } else {
        None
    };

    // Set up logger provider if enabled
    let (logger_provider, resolved_flush_ms, resolved_batch_size) =
        if config.logs_enabled.unwrap_or(true) {
            let log_exporter = EngineLogExporter::new(connection.clone());

            // Resolve: config > custom env var > default (100ms flush, batch size 1).
            // Deliberately overrides standard OTEL_BLRP_* env vars for cross-SDK consistency.
            let flush_ms = config
                .logs_flush_interval_ms
                .or_else(|| {
                    std::env::var("OTEL_LOGS_FLUSH_INTERVAL_MS")
                        .ok()
                        .and_then(|v| v.parse::<u64>().ok())
                })
                .unwrap_or(100);

            let batch_size = config
                .logs_batch_size
                .or_else(|| {
                    std::env::var("OTEL_LOGS_BATCH_SIZE")
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                        .filter(|&v| v >= 1)
                })
                .unwrap_or(1);

            let batch_config = BatchConfigBuilder::default()
                .with_scheduled_delay(std::time::Duration::from_millis(flush_ms))
                .with_max_export_batch_size(batch_size)
                .build();

            let log_processor = BatchLogProcessor::builder(log_exporter)
                .with_batch_config(batch_config)
                .build();

            tracing::debug!(
                flush_interval_ms = flush_ms,
                batch_size = batch_size,
                "Log provider configured"
            );

            let provider = SdkLoggerProvider::builder()
                .with_resource(resource)
                .with_log_processor(log_processor)
                .build();

            (Some(provider), flush_ms, batch_size)
        } else {
            (None, 0, 0)
        };

    let shutdown_timeout =
        std::time::Duration::from_millis(config.shutdown_timeout_ms.unwrap_or(10_000));

    let otel_state = OtelState {
        tracer_provider,
        meter_provider,
        logger_provider,
        connection,
        shutdown_timeout,
    };

    *state = Some(otel_state);
    OTEL_INITIALIZED.store(true, Ordering::Release);

    tracing::info!(
        service = %service_name,
        logs_flush_ms = resolved_flush_ms,
        logs_batch = resolved_batch_size,
        "OpenTelemetry initialized"
    );
}

/// Shutdown OpenTelemetry gracefully, flushing all pending data.
///
/// The shutdown sequence is bounded by the configured `shutdown_timeout_ms`
/// (default 10 seconds). If the timeout is exceeded, a warning is logged and
/// the function returns without waiting further.
pub async fn shutdown_otel() {
    let lock = get_otel_lock();
    let mut state = lock.lock().await;

    if let Some(otel) = state.take() {
        OTEL_INITIALIZED.store(false, Ordering::Release);

        let timeout_duration = otel.shutdown_timeout;

        match tokio::time::timeout(timeout_duration, async {
            // Shutdown tracer provider (flushes pending spans)
            if let Err(e) = otel.tracer_provider.shutdown() {
                tracing::warn!(error = %e, "Error shutting down tracer provider");
            }

            // Shutdown meter provider
            if let Some(meter) = otel.meter_provider {
                if let Err(e) = meter.shutdown() {
                    tracing::warn!(error = %e, "Error shutting down meter provider");
                }
            }

            // Shutdown logger provider
            if let Some(logger) = otel.logger_provider {
                if let Err(e) = logger.shutdown() {
                    tracing::warn!(error = %e, "Error shutting down logger provider");
                }
            }

            // Shutdown shared connection
            otel.connection.shutdown().await;
        })
        .await
        {
            Ok(()) => {
                tracing::info!("OpenTelemetry shut down");
            }
            Err(_) => {
                tracing::warn!(
                    timeout_ms = timeout_duration.as_millis() as u64,
                    "OpenTelemetry shutdown timed out"
                );
            }
        }
    }
}

/// Flush all pending telemetry data (spans, metrics, logs) through the connection.
///
/// This forces the OTEL providers to export any buffered data and then drains
/// the WebSocket connection's message queue. This is a no-op if OpenTelemetry
/// has not been initialized.
pub async fn flush_otel() {
    let lock = get_otel_lock();
    let state = lock.lock().await;

    if let Some(otel) = state.as_ref() {
        if let Err(e) = otel.tracer_provider.force_flush() {
            tracing::warn!(error = %e, "Error flushing tracer provider");
        }

        if let Some(meter) = &otel.meter_provider {
            if let Err(e) = meter.force_flush() {
                tracing::warn!(error = %e, "Error flushing meter provider");
            }
        }

        // Drain buffered messages to WebSocket
        otel.connection.flush().await;
    }
}

/// Execute a function within a new span, automatically handling errors and status.
///
/// If OpenTelemetry is not initialized, the function is executed without tracing.
///
/// # Arguments
/// * `name` - The span name
/// * `traceparent` - Optional W3C traceparent header to set parent context
/// * `kind` - Optional span kind (defaults to Internal)
/// * `f` - The async function to execute within the span
pub async fn with_span<F, Fut, T>(
    name: &str,
    traceparent: Option<&str>,
    kind: Option<SpanKind>,
    f: F,
) -> Result<T, Box<dyn std::error::Error + Send + Sync>>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
{
    use opentelemetry::trace::FutureExt as OtelFutureExt;

    let tracer = opentelemetry::global::tracer("iii-rust-sdk");

    let parent_cx = match traceparent {
        Some(tp) => context::extract_traceparent(tp),
        None => OtelContext::current(),
    };

    let span_kind = kind.unwrap_or(SpanKind::Internal);

    let span = tracer
        .span_builder(name.to_string())
        .with_kind(span_kind)
        .start_with_context(&tracer, &parent_cx);

    let cx = parent_cx.with_span(span);

    match f().with_context(cx.clone()).await {
        Ok(result) => {
            cx.span().set_status(Status::Ok);
            Ok(result)
        }
        Err(err) => {
            let span = cx.span();
            span.set_status(Status::error(err.to_string()));

            let backtrace = std::backtrace::Backtrace::force_capture();
            span.add_event(
                "exception",
                vec![
                    KeyValue::new("exception.type", "Error"),
                    KeyValue::new("exception.message", err.to_string()),
                    KeyValue::new("exception.stacktrace", backtrace.to_string()),
                ],
            );

            Err(err)
        }
    }
}

/// Get a tracer instance for creating spans manually.
pub fn get_tracer() -> opentelemetry::global::BoxedTracer {
    opentelemetry::global::tracer("iii-rust-sdk")
}

/// Get a meter instance for creating metrics.
pub fn get_meter() -> opentelemetry::metrics::Meter {
    opentelemetry::global::meter("iii-rust-sdk")
}

/// Check whether OpenTelemetry has been initialized.
pub fn is_initialized() -> bool {
    OTEL_INITIALIZED.load(Ordering::Acquire)
}

/// Get a clone of the logger provider, if OTel has been initialized and logs are enabled.
///
/// Returns `None` if OTel is not initialized or logs are disabled.
/// This is used by the SDK logger to emit OTel LogRecords alongside the
/// engine invoker calls.
pub fn get_logger_provider() -> Option<SdkLoggerProvider> {
    if !is_initialized() {
        return None;
    }
    // Try to acquire the lock without blocking. If contended, skip emission.
    let lock = get_otel_lock();
    let state = lock.try_lock().ok()?;
    state.as_ref()?.logger_provider.clone()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_logs_env_var_parsing_valid() {
        // Verify the parsing logic: valid u64 string parses correctly
        let val: Option<u64> = "500".parse::<u64>().ok();
        assert_eq!(val, Some(500));
    }

    #[test]
    fn test_logs_env_var_parsing_invalid() {
        // Verify the parsing logic: non-numeric string returns None
        let val: Option<u64> = "not-a-number".parse::<u64>().ok();
        assert!(val.is_none());
    }

    #[test]
    fn test_logs_batch_size_minimum_filter() {
        // Verify filter logic: batch size 0 is rejected
        let val: Option<usize> = "0".parse::<usize>().ok().filter(|&v| v >= 1);
        assert!(val.is_none());

        // Verify filter logic: batch size 1 is accepted
        let val: Option<usize> = "1".parse::<usize>().ok().filter(|&v| v >= 1);
        assert_eq!(val, Some(1));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_with_span_error_records_exception_event() {
        use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};

        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();

        opentelemetry::global::set_tracer_provider(provider.clone());

        let result = super::with_span("test-error-span", None, None, || async {
            Err::<(), _>("test error".into())
        })
        .await;
        assert!(result.is_err());

        let _ = provider.force_flush();

        let spans = exporter.get_finished_spans().unwrap();
        assert!(!spans.is_empty(), "expected at least 1 span");

        let span = spans
            .iter()
            .find(|s| s.name == "test-error-span")
            .expect("should find test-error-span");
        let exc_event = span
            .events
            .iter()
            .find(|e| e.name == "exception")
            .expect("span should have an 'exception' event");

        let has_type = exc_event
            .attributes
            .iter()
            .any(|kv| kv.key.as_str() == "exception.type");
        let has_message = exc_event
            .attributes
            .iter()
            .any(|kv| kv.key.as_str() == "exception.message");
        let has_stacktrace = exc_event
            .attributes
            .iter()
            .any(|kv| kv.key.as_str() == "exception.stacktrace");

        assert!(has_type, "exception event should have exception.type");
        assert!(has_message, "exception event should have exception.message");
        assert!(
            has_stacktrace,
            "exception event should have exception.stacktrace"
        );

        let message_val = exc_event
            .attributes
            .iter()
            .find(|kv| kv.key.as_str() == "exception.message")
            .map(|kv| kv.value.to_string())
            .unwrap();
        assert!(
            message_val.contains("test error"),
            "exception.message should contain error text"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_with_span_success_no_exception_event() {
        use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};

        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();

        opentelemetry::global::set_tracer_provider(provider.clone());

        let result = super::with_span("test-ok-span", None, None, || async {
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(42)
        })
        .await;
        assert!(result.is_ok());

        let _ = provider.force_flush();

        let spans = exporter.get_finished_spans().unwrap();
        assert!(!spans.is_empty(), "expected at least 1 span");

        let span = spans
            .iter()
            .find(|s| s.name == "test-ok-span")
            .expect("should find test-ok-span");
        let exc_event = span.events.iter().find(|e| e.name == "exception");
        assert!(
            exc_event.is_none(),
            "successful span should not have an exception event"
        );
    }
}
