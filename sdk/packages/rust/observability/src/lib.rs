//! iii-observability: shared OTel + Logger primitives for the iii Rust SDK.
pub const VERSION: &str = "0.13.0-next.1";

pub mod telemetry;

pub use telemetry::context::{
    CapturedContext, capture_otel_context, current_span_id, current_trace_id, extract_baggage,
    extract_context, extract_traceparent, get_all_baggage, get_baggage_entry, inject_baggage,
    inject_traceparent, remove_baggage_entry, run_with_baggage, set_baggage_entry,
};
pub use telemetry::types::{OtelConfig, ReconnectionConfig};
pub use telemetry::baggage_span_processor::{BaggageSpanProcessor, DEFAULT_ALLOWLIST};
pub use telemetry::payload::{REDACTED_PLACEHOLDER, redact, redact_and_truncate, resolve_max_bytes_from_env};
pub use telemetry::span_ops::{
    current_span_is_recording, record_span_event, set_current_span_attribute, set_current_span_error,
};
pub use telemetry::http_instrumentation::execute_traced_request;
pub use telemetry::{flush_otel, init_otel, run_in_span, shutdown_otel, with_span};

pub mod logger;
pub use logger::Logger;
