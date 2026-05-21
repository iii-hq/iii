//! iii-observability: shared OTel + Logger primitives for the iii Rust SDK.
pub const VERSION: &str = "0.13.0-next.1";

pub mod telemetry;

pub use telemetry::context::{
    CapturedContext, capture_otel_context, current_span_id, current_trace_id, extract_baggage,
    extract_context, extract_traceparent, get_all_baggage, get_baggage_entry, inject_baggage,
    inject_traceparent, remove_baggage_entry, run_with_baggage, set_baggage_entry,
};
pub use telemetry::types::{OtelConfig, ReconnectionConfig};
