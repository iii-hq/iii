use serde_json::Value;

#[cfg(feature = "otel")]
use opentelemetry::logs::{AnyValue, LogRecord as _, Logger as _, LoggerProvider as _, Severity};

/// Convert a `serde_json::Value` into an OpenTelemetry `AnyValue` so that
/// nested objects and arrays are preserved as structured OTLP attributes
/// (`kvlistValue` / `arrayValue`) instead of being stringified.
#[cfg(feature = "otel")]
fn json_value_to_anyvalue(v: &Value) -> AnyValue {
    match v {
        Value::String(s) => AnyValue::String(s.clone().into()),
        Value::Bool(b) => AnyValue::Boolean(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                AnyValue::Int(i)
            } else {
                AnyValue::Double(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::Array(arr) => {
            AnyValue::ListAny(Box::new(arr.iter().map(json_value_to_anyvalue).collect()))
        }
        Value::Object(map) => AnyValue::Map(Box::new(
            map.iter()
                .map(|(k, v)| (k.clone().into(), json_value_to_anyvalue(v)))
                .collect(),
        )),
        Value::Null => AnyValue::String("".into()),
    }
}

/// Structured logger that emits logs as OpenTelemetry LogRecords.
///
/// Every log call automatically captures the active trace and span context,
/// correlating your logs with distributed traces without any manual wiring.
/// When OTel is not initialized, Logger gracefully falls back to the `tracing`
/// crate.
///
/// Pass structured data as the second argument to any log method. Using a
/// `serde_json::Value` object of key-value pairs (instead of string
/// interpolation) lets you filter, aggregate, and build dashboards in your
/// observability backend.
///
/// # Examples
///
/// ```rust
/// use iii_sdk::Logger;
/// use serde_json::json;
///
/// let logger = Logger::new();
///
/// // Basic logging — trace context is injected automatically
/// logger.info("Worker connected", None);
///
/// // Structured context for dashboards and alerting
/// logger.info("Order processed", Some(json!({ "order_id": "ord_123", "amount": 49.99, "currency": "USD" })));
/// logger.warn("Retry attempt", Some(json!({ "attempt": 3, "max_retries": 5, "endpoint": "/api/charge" })));
/// logger.error("Payment failed", Some(json!({ "order_id": "ord_123", "gateway": "stripe", "error_code": "card_declined" })));
/// ```
#[derive(Clone, Default)]
pub struct Logger {}

impl Logger {
    /// Create a new Logger instance.
    pub fn new() -> Self {
        Self {}
    }

    /// Emit a LogRecord via the OTel LoggerProvider with trace context from the active span.
    /// Returns `true` if the log was emitted via OTel, `false` otherwise.
    #[cfg(feature = "otel")]
    fn emit_otel(&self, message: &str, severity: Severity, data: Option<&Value>) -> bool {
        let Some(provider) = crate::telemetry::get_logger_provider() else {
            return false;
        };

        let logger = provider.logger("iii-rust-sdk");
        let mut record = logger.create_log_record();
        let now = std::time::SystemTime::now();
        record.set_timestamp(now);
        record.set_observed_timestamp(now);
        record.set_severity_number(severity);
        record.set_body(message.to_string().into());

        if let Some(d) = data {
            record.add_attribute("log.data", json_value_to_anyvalue(d));
        }

        // Attach trace context from the active OTel span
        {
            use opentelemetry::trace::TraceContextExt;
            let cx = opentelemetry::Context::current();
            let span_ctx = cx.span().span_context().clone();
            if span_ctx.is_valid() {
                record.set_trace_context(
                    span_ctx.trace_id(),
                    span_ctx.span_id(),
                    Some(span_ctx.trace_flags()),
                );
            }
        }

        logger.emit(record);
        true
    }

    /// Log an info-level message.
    ///
    /// # Arguments
    ///
    /// * `message` - Human-readable log message.
    /// * `data` - Structured context attached as OTel log attributes.
    ///   Use `serde_json::json!` objects to enable filtering and aggregation
    ///   in your observability backend (e.g. Grafana, Datadog, New Relic).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use iii_sdk::Logger;
    /// # use serde_json::json;
    /// # let logger = Logger::new();
    /// logger.info("Order processed", Some(json!({ "order_id": "ord_123", "status": "completed" })));
    /// ```
    #[cfg_attr(not(feature = "otel"), allow(unused_variables))]
    pub fn info(&self, message: &str, data: Option<Value>) {
        #[cfg(feature = "otel")]
        if self.emit_otel(message, Severity::Info, data.as_ref()) {
            return;
        }
        tracing::info!(message = %message);
    }

    /// Log a warning-level message.
    ///
    /// # Arguments
    ///
    /// * `message` - Human-readable log message.
    /// * `data` - Structured context attached as OTel log attributes.
    ///   Use `serde_json::json!` objects to enable filtering and aggregation
    ///   in your observability backend (e.g. Grafana, Datadog, New Relic).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use iii_sdk::Logger;
    /// # use serde_json::json;
    /// # let logger = Logger::new();
    /// logger.warn("Retry attempt", Some(json!({ "attempt": 3, "max_retries": 5, "endpoint": "/api/charge" })));
    /// ```
    #[cfg_attr(not(feature = "otel"), allow(unused_variables))]
    pub fn warn(&self, message: &str, data: Option<Value>) {
        #[cfg(feature = "otel")]
        if self.emit_otel(message, Severity::Warn, data.as_ref()) {
            return;
        }
        tracing::warn!(message = %message);
    }

    /// Log an error-level message.
    ///
    /// # Arguments
    ///
    /// * `message` - Human-readable log message.
    /// * `data` - Structured context attached as OTel log attributes.
    ///   Use `serde_json::json!` objects to enable filtering and aggregation
    ///   in your observability backend (e.g. Grafana, Datadog, New Relic).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use iii_sdk::Logger;
    /// # use serde_json::json;
    /// # let logger = Logger::new();
    /// logger.error("Payment failed", Some(json!({ "order_id": "ord_123", "gateway": "stripe", "error_code": "card_declined" })));
    /// ```
    #[cfg_attr(not(feature = "otel"), allow(unused_variables))]
    pub fn error(&self, message: &str, data: Option<Value>) {
        #[cfg(feature = "otel")]
        if self.emit_otel(message, Severity::Error, data.as_ref()) {
            return;
        }
        tracing::error!(message = %message);
    }

    /// Log a debug-level message.
    ///
    /// # Arguments
    ///
    /// * `message` - Human-readable log message.
    /// * `data` - Structured context attached as OTel log attributes.
    ///   Use `serde_json::json!` objects to enable filtering and aggregation
    ///   in your observability backend (e.g. Grafana, Datadog, New Relic).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use iii_sdk::Logger;
    /// # use serde_json::json;
    /// # let logger = Logger::new();
    /// logger.debug("Cache lookup", Some(json!({ "key": "user:42", "hit": false })));
    /// ```
    #[cfg_attr(not(feature = "otel"), allow(unused_variables))]
    pub fn debug(&self, message: &str, data: Option<Value>) {
        #[cfg(feature = "otel")]
        if self.emit_otel(message, Severity::Debug, data.as_ref()) {
            return;
        }
        tracing::debug!(message = %message);
    }
}
