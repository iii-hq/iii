//! High-level span operations for consumers who don't want to depend on
//! `opentelemetry` / `opentelemetry_sdk` crates directly.
//!
//! Internally each helper reaches into `opentelemetry::Context::current()`
//! and operates on its active span, but consumers see only plain `&str` /
//! `impl Into<String>` arguments. This closes the "iii-sdk public API has
//! holes" gap that previously forced consumers (e.g. the harness wrapper)
//! to import `opentelemetry::{Context, KeyValue, trace::TraceContextExt}`.

use opentelemetry::trace::{Status, TraceContextExt};
use opentelemetry::{Context, KeyValue};

/// True when the current span is being recorded (its `SpanContext` is valid).
///
/// Returns `false` when there is no active span, when the sampler dropped
/// the span, or when no global tracer provider is installed. Consumers
/// should gate expensive attribute-construction code on this check to avoid
/// the per-call allocation cost of `set_current_span_attribute`.
#[must_use]
pub fn current_span_is_recording() -> bool {
    Context::current().span().span_context().is_valid()
}

/// Set an attribute on the current span. No-op when the current span is
/// not recording.
///
/// `value` accepts any `Into<String>`: `&'static str`, owned `String`, or
/// `String` references via `.clone()`. Matches the harness's existing call
/// sites where the value is usually an owned ID string.
pub fn set_current_span_attribute(key: &'static str, value: impl Into<String>) {
    let cx = Context::current();
    let span = cx.span();
    if span.span_context().is_valid() {
        span.set_attribute(KeyValue::new(key, value.into()));
    }
}

/// Mark the current span as `Status::error(message)`.
///
/// Used by harness wrappers and other middleware that need to surface
/// handler errors on the span without re-importing OTel types.
pub fn set_current_span_error(message: impl Into<String>) {
    let cx = Context::current();
    cx.span().set_status(Status::error(message.into()));
}

/// Record an event on the current span. No-op when the current span is
/// not recording.
///
/// `attrs` is a slice of `(key, value)` pairs; both are owned strings to
/// keep the surface free of OTel `KeyValue` imports for consumers. The
/// helper allocates internally so callers can build attribute lists from
/// any source (`Vec<(String, String)>`, fixed arrays, iterator collects).
///
/// Used by the iii-sdk dispatcher to record `iii.invocation.input` and
/// `iii.invocation.output` events when `III_TRACE_PAYLOADS=1` is set;
/// also exposed publicly so worker code can record domain events such as
/// `llm.request` or `tool.invoked` without depending on `opentelemetry`
/// directly.
pub fn record_span_event(name: impl Into<String>, attrs: &[(String, String)]) {
    let cx = Context::current();
    let span = cx.span();
    if !span.span_context().is_valid() {
        return;
    }
    let kvs: Vec<KeyValue> = attrs
        .iter()
        .map(|(k, v)| KeyValue::new(k.clone(), v.clone()))
        .collect();
    span.add_event(name.into(), kvs);
}

#[cfg(test)]
mod tests {
    //! These unit tests exercise the no-tracer / no-span paths only.
    //! End-to-end tests against `InMemorySpanExporter` live in
    //! `tests/span_ops_api.rs` so they can build their own
    //! `SdkTracerProvider` without racing the global one.

    use super::*;

    #[test]
    fn current_span_is_recording_returns_false_without_tracer() {
        // No global tracer installed in this test process: the active
        // span on `Context::current()` is the default no-op span.
        assert!(!current_span_is_recording());
    }

    #[test]
    fn set_current_span_attribute_is_safe_without_tracer() {
        // Must not panic when no tracer is installed — the gate inside
        // the helper prevents the inner set_attribute call.
        set_current_span_attribute("test.key", "value");
    }

    #[test]
    fn set_current_span_error_is_safe_without_tracer() {
        // Setting status on a no-op span is harmless.
        set_current_span_error("test error");
    }

    #[test]
    fn record_span_event_is_safe_without_tracer() {
        // No tracer installed → recording check short-circuits, no panic.
        record_span_event(
            "test.event",
            &[
                ("k1".to_string(), "v1".to_string()),
                ("k2".to_string(), "v2".to_string()),
            ],
        );
    }
}
