//! OpenTelemetry initialization for the III Engine.
//!
//! This module re-exports from `modules::observability::otel` for backward compatibility.
//! The canonical implementation is in `crate::modules::observability::otel`.

pub use crate::modules::observability::otel::*;

use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Extension trait for `tracing::Span` to simplify setting parent context from HTTP headers.
///
/// This trait provides a fluent API for setting the parent context of a span using
/// W3C Trace Context (`traceparent`) and Baggage headers.
///
/// # Example
/// ```ignore
/// use crate::telemetry::SpanExt;
///
/// let span = tracing::info_span!("my_operation")
///     .with_parent_headers(traceparent.as_deref(), baggage.as_deref());
/// ```
pub trait SpanExt {
    /// Sets the parent context of this span from optional traceparent and baggage headers.
    ///
    /// If either `traceparent` or `baggage` is provided, the span's parent context will be
    /// set using the extracted context. If both are `None`, the span is returned unchanged.
    fn with_parent_headers(self, traceparent: Option<&str>, baggage: Option<&str>) -> Self;
}

impl SpanExt for Span {
    fn with_parent_headers(self, traceparent: Option<&str>, baggage: Option<&str>) -> Self {
        if traceparent.is_some() || baggage.is_some() {
            let parent_context = extract_context(traceparent, baggage);
            let _ = self.set_parent(parent_context);
        }
        self
    }
}
