// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_ext_accepts_traceparent_and_baggage_headers() {
        let span = tracing::info_span!("telemetry-test");
        let _span = span.with_parent_headers(
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
            Some("user_id=123"),
        );
    }
}
