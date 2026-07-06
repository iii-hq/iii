// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! OpenTelemetry initialization for the III Engine.
//!
//! This module re-exports from `workers::observability::otel` for backward compatibility.
//! The canonical implementation is in `crate::workers::observability::otel`.

pub use crate::workers::observability::otel::*;

use opentelemetry::KeyValue;
use opentelemetry::trace::TraceContextExt;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Extension trait for `tracing::Span` to simplify setting parent context from HTTP headers.
///
/// This trait provides a fluent API for setting the parent context of a span using
/// W3C Trace Context (`traceparent` + `tracestate`) and Baggage headers.
///
/// # Example
/// ```ignore
/// use crate::telemetry::SpanExt;
///
/// let span = tracing::info_span!("my_operation")
///     .with_parent_headers(traceparent.as_deref(), tracestate.as_deref(), baggage.as_deref());
/// ```
pub trait SpanExt {
    /// Sets the parent context of this span from optional W3C trace-context and baggage headers.
    ///
    /// If any of `traceparent`, `tracestate`, or `baggage` is provided, the span's parent
    /// context is set using the extracted context. `tracestate` is only meaningful alongside
    /// a valid `traceparent` (it is ignored without one, per the W3C spec). If all are `None`,
    /// the span is returned unchanged.
    fn with_parent_headers(
        self,
        traceparent: Option<&str>,
        tracestate: Option<&str>,
        baggage: Option<&str>,
    ) -> Self;
}

impl SpanExt for Span {
    fn with_parent_headers(
        self,
        traceparent: Option<&str>,
        tracestate: Option<&str>,
        baggage: Option<&str>,
    ) -> Self {
        if traceparent.is_some() || tracestate.is_some() || baggage.is_some() {
            let parent_context = extract_context_with_state(traceparent, tracestate, baggage);
            let tp_owned = traceparent.map(|s| s.to_string());
            let parent_trace_id = parent_context.span().span_context().trace_id();
            let parent_valid = parent_context.span().span_context().is_valid();
            match self.set_parent(parent_context) {
                Ok(()) => {
                    tracing::debug!(
                        traceparent = tp_owned.as_deref().unwrap_or("(none)"),
                        parent_trace_id = %parent_trace_id,
                        parent_context_valid = parent_valid,
                        "with_parent_headers: successfully set parent context"
                    );
                    // Record an event on this span so it shows up in OTel export
                    self.add_event(
                        "traceparent.propagated",
                        vec![
                            KeyValue::new("parent.trace_id", format!("{parent_trace_id}")),
                            KeyValue::new(
                                "traceparent",
                                tp_owned.as_deref().unwrap_or("(none)").to_string(),
                            ),
                        ],
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        traceparent = tp_owned.as_deref().unwrap_or("(none)"),
                        parent_trace_id = %parent_trace_id,
                        error = %err,
                        "with_parent_headers: failed to set parent context"
                    );
                    self.add_event(
                        "traceparent.set_parent_failed",
                        vec![
                            KeyValue::new("error", err.to_string()),
                            KeyValue::new(
                                "traceparent",
                                tp_owned.as_deref().unwrap_or("(none)").to_string(),
                            ),
                        ],
                    );
                }
            }
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
            None,
            Some("user_id=123"),
        );
    }

    #[test]
    fn span_ext_accepts_w3c_tracestate_header() {
        let span = tracing::info_span!("telemetry-test");
        let _span = span.with_parent_headers(
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
            Some("congo=t61rcWkgMzE,rojo=00f067aa0ba902b7"),
            None,
        );
    }
}
