//! Observability module: OTel + Logger primitives for the iii Rust SDK.
pub mod logger;
pub mod telemetry;

/// Re-export the raw `opentelemetry` crate so dependents can use OTel API
/// types (traits, `KeyValue`, `global`, etc.) without a direct dep.
pub use opentelemetry;

pub use self::logger::Logger;
pub use self::telemetry::baggage_span_processor::{BaggageSpanProcessor, DEFAULT_ALLOWLIST};
pub use self::telemetry::context::{
    CapturedContext, capture_otel_context, current_span_id, current_trace_id, extract_baggage,
    extract_context, extract_traceparent, get_all_baggage, get_baggage_entry, inject_baggage,
    inject_traceparent, remove_baggage_entry, run_with_baggage, set_baggage_entry,
};
pub use self::telemetry::http_instrumentation::execute_traced_request;
pub use self::telemetry::payload::{
    REDACTED_PLACEHOLDER, redact, redact_and_truncate, resolve_max_bytes_from_env,
};
pub use self::telemetry::span_ops::{
    current_span_is_recording, record_span_event, set_current_span_attribute,
    set_current_span_error,
};
pub use self::telemetry::types::{OtelConfig, ReconnectionConfig};
pub use self::telemetry::{flush_otel, init_otel, run_in_span, shutdown_otel, with_span};
